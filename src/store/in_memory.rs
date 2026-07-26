use crate::{
    EventId, EventMetadata, EventStore, ExpectedVersion, MemoryError, NewEvent, Result,
    StoredEvent, StreamId,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct InMemoryStore<E> {
    events: Vec<StoredEvent<E>>,
    streams: BTreeMap<StreamId, Vec<usize>>,
    event_ids: BTreeSet<EventId>,
}

impl<E> Default for InMemoryStore<E> {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            streams: BTreeMap::new(),
            event_ids: BTreeSet::new(),
        }
    }
}

impl<E: Clone> EventStore<E> for InMemoryStore<E> {
    fn append(
        &mut self,
        stream: &StreamId,
        expected: ExpectedVersion,
        events: &[NewEvent<E>],
    ) -> Result<Vec<StoredEvent<E>>> {
        let actual = self.stream_version(stream);
        validate_expected(stream, expected, actual)?;
        validate_unique_ids(&self.event_ids, events)?;

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

        let positions = self.streams.entry(stream.clone()).or_default();
        for event in &committed {
            self.event_ids.insert(event.metadata.id.clone());
            positions.push(self.events.len());
            self.events.push(event.clone());
        }
        Ok(committed)
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

fn validate_expected(
    stream: &StreamId,
    expected: ExpectedVersion,
    actual: Option<u64>,
) -> Result<()> {
    let matches = match expected {
        ExpectedVersion::Any => true,
        ExpectedVersion::NoStream => actual.is_none(),
        ExpectedVersion::Exact(version) => actual == Some(version),
    };
    if matches {
        return Ok(());
    }
    let expected = match expected {
        ExpectedVersion::Any | ExpectedVersion::NoStream => None,
        ExpectedVersion::Exact(version) => Some(version),
    };
    Err(MemoryError::VersionConflict {
        stream: stream.to_string(),
        expected,
        actual,
    })
}

fn validate_unique_ids<E>(known: &BTreeSet<EventId>, events: &[NewEvent<E>]) -> Result<()> {
    let mut batch = BTreeSet::new();
    for event in events {
        if known.contains(&event.id) || !batch.insert(event.id.clone()) {
            return Err(MemoryError::DuplicateEvent {
                id: event.id.to_string(),
            });
        }
    }
    Ok(())
}
