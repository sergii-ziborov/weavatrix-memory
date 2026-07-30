use super::{AppendReceipt, EventStore, ExpectedVersion};
use crate::{
    error::{MemoryError, Result},
    event::{EventMetadata, NewEvent, StoredEvent},
    id::{EventId, StreamId},
};
use std::collections::{HashMap, HashSet};

mod validation;

#[derive(Debug, Clone)]
pub struct InMemoryStore<E> {
    events: Vec<StoredEvent<E>>,
    streams: HashMap<StreamId, Vec<usize>>,
    event_ids: HashSet<EventId>,
}

impl<E> Default for InMemoryStore<E> {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            streams: HashMap::new(),
            event_ids: HashSet::new(),
        }
    }
}

impl<E: Clone> InMemoryStore<E> {
    pub(crate) fn prepare_append(
        &self,
        stream: &StreamId,
        expected: ExpectedVersion,
        events: &[NewEvent<E>],
    ) -> Result<Vec<StoredEvent<E>>> {
        let actual = self.stream_version(stream);
        validation::expected(stream, expected, actual)?;
        validation::unique_ids(&self.event_ids, events)?;

        let start_version = actual.map_or(Ok(0), |version| {
            version.checked_add(1).ok_or(MemoryError::CapacityOverflow)
        })?;
        let start_position =
            u64::try_from(self.events.len()).map_err(|_| MemoryError::CapacityOverflow)?;
        let mut committed = Vec::with_capacity(events.len());

        for (offset, event) in events.iter().enumerate() {
            let offset = u64::try_from(offset).map_err(|_| MemoryError::CapacityOverflow)?;
            let stream_version = start_version
                .checked_add(offset)
                .ok_or(MemoryError::CapacityOverflow)?;
            let global_position = start_position
                .checked_add(offset)
                .ok_or(MemoryError::CapacityOverflow)?;
            committed.push(StoredEvent {
                metadata: EventMetadata {
                    id: event.id.clone(),
                    stream_id: stream.clone(),
                    stream_version,
                    global_position,
                    event_type: event.event_type.clone(),
                    occurred_at: event.occurred_at,
                    recorded_at: event.recorded_at,
                    agent_id: event.agent_id.clone(),
                    session_id: event.session_id.clone(),
                    correlation_id: event.correlation_id.clone(),
                    causation_id: event.causation_id.clone(),
                },
                payload: event.payload.clone(),
            });
        }

        Ok(committed)
    }

    pub(crate) fn prepare_append_owned(
        &self,
        stream: &StreamId,
        expected: ExpectedVersion,
        events: Vec<NewEvent<E>>,
    ) -> Result<Vec<StoredEvent<E>>> {
        let actual = self.stream_version(stream);
        validation::expected(stream, expected, actual)?;
        validation::unique_ids(&self.event_ids, &events)?;

        let start_version = actual.map_or(Ok(0), |version| {
            version.checked_add(1).ok_or(MemoryError::CapacityOverflow)
        })?;
        let start_position =
            u64::try_from(self.events.len()).map_err(|_| MemoryError::CapacityOverflow)?;
        let mut committed = Vec::with_capacity(events.len());

        for (offset, event) in events.into_iter().enumerate() {
            let offset = u64::try_from(offset).map_err(|_| MemoryError::CapacityOverflow)?;
            committed.push(StoredEvent {
                metadata: EventMetadata {
                    id: event.id,
                    stream_id: stream.clone(),
                    stream_version: start_version
                        .checked_add(offset)
                        .ok_or(MemoryError::CapacityOverflow)?,
                    global_position: start_position
                        .checked_add(offset)
                        .ok_or(MemoryError::CapacityOverflow)?,
                    event_type: event.event_type,
                    occurred_at: event.occurred_at,
                    recorded_at: event.recorded_at,
                    agent_id: event.agent_id,
                    session_id: event.session_id,
                    correlation_id: event.correlation_id,
                    causation_id: event.causation_id,
                },
                payload: event.payload,
            });
        }

        Ok(committed)
    }

    pub(crate) fn commit_prepared(&mut self, committed: &[StoredEvent<E>]) {
        let Some(first) = committed.first() else {
            return;
        };
        debug_assert!(
            committed
                .iter()
                .all(|event| event.metadata.stream_id == first.metadata.stream_id)
        );
        let Self {
            events,
            streams,
            event_ids,
        } = self;
        events.reserve(committed.len());
        event_ids.reserve(committed.len());
        let positions = streams.entry(first.metadata.stream_id.clone()).or_default();
        positions.reserve(committed.len());
        for event in committed {
            event_ids.insert(event.metadata.id.clone());
            positions.push(events.len());
            events.push(event.clone());
        }
    }

    pub(crate) fn commit_prepared_owned(&mut self, committed: Vec<StoredEvent<E>>) {
        let Some(first) = committed.first() else {
            return;
        };
        debug_assert!(
            committed
                .iter()
                .all(|event| event.metadata.stream_id == first.metadata.stream_id)
        );
        let Self {
            events,
            streams,
            event_ids,
        } = self;
        events.reserve(committed.len());
        event_ids.reserve(committed.len());
        let positions = streams.entry(first.metadata.stream_id.clone()).or_default();
        positions.reserve(committed.len());
        for event in committed {
            event_ids.insert(event.metadata.id.clone());
            positions.push(events.len());
            events.push(event);
        }
    }

    pub(crate) fn restore(events: Vec<StoredEvent<E>>) -> Result<Self> {
        let mut store = Self::default();
        store.events.reserve(events.len());
        store.event_ids.reserve(events.len());
        for event in events {
            let expected_position =
                u64::try_from(store.events.len()).map_err(|_| MemoryError::CapacityOverflow)?;
            if event.metadata.global_position != expected_position {
                return Err(MemoryError::InvalidReplay {
                    reason: format!(
                        "global position {}, expected {expected_position}",
                        event.metadata.global_position
                    ),
                });
            }
            let expected_version = store
                .stream_version(&event.metadata.stream_id)
                .map_or(Ok(0), |version| {
                    version.checked_add(1).ok_or(MemoryError::CapacityOverflow)
                })?;
            if event.metadata.stream_version != expected_version {
                return Err(MemoryError::InvalidReplay {
                    reason: format!(
                        "stream {} version {}, expected {expected_version}",
                        event.metadata.stream_id, event.metadata.stream_version
                    ),
                });
            }
            if !store.event_ids.insert(event.metadata.id.clone()) {
                return Err(MemoryError::DuplicateEvent {
                    id: event.metadata.id.to_string(),
                });
            }
            let positions = store
                .streams
                .entry(event.metadata.stream_id.clone())
                .or_default();
            positions.push(store.events.len());
            store.events.push(event);
        }
        Ok(store)
    }
}

impl<E: Clone> EventStore<E> for InMemoryStore<E> {
    fn append(
        &mut self,
        stream: &StreamId,
        expected: ExpectedVersion,
        events: &[NewEvent<E>],
    ) -> Result<Vec<StoredEvent<E>>> {
        let committed = self.prepare_append(stream, expected, events)?;
        self.commit_prepared(&committed);
        Ok(committed)
    }

    fn append_owned(
        &mut self,
        stream: &StreamId,
        expected: ExpectedVersion,
        events: Vec<NewEvent<E>>,
    ) -> Result<Vec<StoredEvent<E>>> {
        let committed = self.prepare_append_owned(stream, expected, events)?;
        self.commit_prepared(&committed);
        Ok(committed)
    }

    fn append_owned_receipt(
        &mut self,
        stream: &StreamId,
        expected: ExpectedVersion,
        events: Vec<NewEvent<E>>,
    ) -> Result<AppendReceipt> {
        let committed = self.prepare_append_owned(stream, expected, events)?;
        let receipt = AppendReceipt::from_events(&committed);
        self.commit_prepared_owned(committed);
        Ok(receipt)
    }

    fn load_stream(&self, stream: &StreamId, after: Option<u64>) -> Vec<StoredEvent<E>> {
        self.streams
            .get(stream)
            .into_iter()
            .flatten()
            .map(|index| &self.events[*index])
            .filter(|event| after.is_none_or(|cursor| event.metadata.stream_version > cursor))
            .cloned()
            .collect()
    }

    fn load_all(&self, after: Option<u64>, limit: usize) -> Vec<StoredEvent<E>> {
        self.events
            .iter()
            .filter(|event| after.is_none_or(|cursor| event.metadata.global_position > cursor))
            .take(limit)
            .cloned()
            .collect()
    }

    fn stream_version(&self, stream: &StreamId) -> Option<u64> {
        self.streams
            .get(stream)
            .and_then(|positions| positions.last())
            .map(|index| self.events[*index].metadata.stream_version)
    }

    fn len(&self) -> usize {
        self.events.len()
    }
}
