use crate::{
    error::{MemoryError, Result},
    event::StoredEvent,
    id::{EventId, StreamId},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

pub trait Projection<E>: Default {
    /// Reserves projection-specific capacity before a known replay batch.
    ///
    /// The default is a no-op. Implementations must not mutate observable
    /// projection state or weaken validation.
    fn prepare_replay(&mut self, _events: &[StoredEvent<E>]) {}

    /// Applies one validated stored event.
    ///
    /// # Errors
    ///
    /// Returns domain-specific invariant violations.
    fn apply(&mut self, event: &StoredEvent<E>) -> Result<()>;

    /// Applies one owned event. The default preserves the borrowed contract;
    /// projections may override it to move large payloads without cloning.
    ///
    /// # Errors
    ///
    /// Returns the same invariant violations as [`Self::apply`].
    fn apply_owned(&mut self, event: StoredEvent<E>) -> Result<()> {
        self.apply(&event)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayCursor {
    pub global_position: Option<u64>,
    pub stream_versions: BTreeMap<StreamId, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionSnapshot<P> {
    pub cursor: ReplayCursor,
    pub projection: P,
}

/// Replays a complete, zero-based event sequence after validating its cursors.
///
/// # Errors
///
/// Rejects global-position or per-stream-version gaps and projection errors.
pub fn replay<E, P>(events: &[StoredEvent<E>]) -> Result<P>
where
    P: Projection<E>,
{
    replay_tracked(events).map(|(projection, _)| projection)
}

/// Replays and consumes a complete sequence, allowing projections to move
/// payloads instead of cloning them.
///
/// # Errors
///
/// Rejects the same cursor, duplicate-ID, and projection failures as
/// [`replay`].
pub fn replay_owned<E, P>(events: Vec<StoredEvent<E>>) -> Result<P>
where
    P: Projection<E>,
{
    apply_owned_sequence(P::default(), ReplayCursor::default(), events, true)
        .map(|(projection, _)| projection)
}

/// Replays a complete sequence and returns the cursor needed for a snapshot.
///
/// # Errors
///
/// Rejects invalid event ordering, duplicate IDs, and projection errors.
pub fn replay_tracked<E, P>(events: &[StoredEvent<E>]) -> Result<(P, ReplayCursor)>
where
    P: Projection<E>,
{
    apply_sequence(P::default(), ReplayCursor::default(), events, true)
}

/// Resumes a materialized projection from its exact event cursor.
///
/// # Errors
///
/// Rejects gaps between the snapshot cursor and the supplied tail, invalid
/// stream versions, duplicate IDs within the tail, and projection errors.
pub fn resume<E, P>(
    snapshot: ProjectionSnapshot<P>,
    tail: &[StoredEvent<E>],
) -> Result<(P, ReplayCursor)>
where
    P: Projection<E>,
{
    apply_sequence(snapshot.projection, snapshot.cursor, tail, false)
}

fn apply_sequence<E, P>(
    mut projection: P,
    mut cursor: ReplayCursor,
    events: &[StoredEvent<E>],
    require_zero_start: bool,
) -> Result<(P, ReplayCursor)>
where
    P: Projection<E>,
{
    projection.prepare_replay(events);
    let mut event_ids = HashSet::<EventId>::with_capacity(events.len());
    for event in events {
        let (expected_position, expected_version) =
            validate_next(&cursor, &mut event_ids, event, require_zero_start)?;
        projection.apply(event)?;
        cursor
            .stream_versions
            .insert(event.metadata.stream_id.clone(), expected_version);
        cursor.global_position = Some(expected_position);
    }
    Ok((projection, cursor))
}

fn apply_owned_sequence<E, P>(
    mut projection: P,
    mut cursor: ReplayCursor,
    events: Vec<StoredEvent<E>>,
    require_zero_start: bool,
) -> Result<(P, ReplayCursor)>
where
    P: Projection<E>,
{
    projection.prepare_replay(&events);
    let mut event_ids = HashSet::<EventId>::with_capacity(events.len());
    for event in events {
        let (expected_position, expected_version) =
            validate_next(&cursor, &mut event_ids, &event, require_zero_start)?;
        let stream = event.metadata.stream_id.clone();
        projection.apply_owned(event)?;
        cursor.stream_versions.insert(stream, expected_version);
        cursor.global_position = Some(expected_position);
    }
    Ok((projection, cursor))
}

fn validate_next<E>(
    cursor: &ReplayCursor,
    event_ids: &mut HashSet<EventId>,
    event: &StoredEvent<E>,
    require_zero_start: bool,
) -> Result<(u64, u64)> {
    if !event_ids.insert(event.metadata.id.clone()) {
        return Err(MemoryError::DuplicateEvent {
            id: event.metadata.id.to_string(),
        });
    }
    let expected_position = cursor.global_position.map_or(Ok(0), |position| {
        position.checked_add(1).ok_or(MemoryError::CapacityOverflow)
    })?;
    if require_zero_start && cursor.global_position.is_none() && expected_position != 0 {
        return Err(MemoryError::InvalidReplay {
            reason: "complete replay must start at zero".to_owned(),
        });
    }
    if event.metadata.global_position != expected_position {
        return Err(MemoryError::InvalidReplay {
            reason: format!(
                "global position {}, expected {expected_position}",
                event.metadata.global_position
            ),
        });
    }
    let expected_version = cursor
        .stream_versions
        .get(&event.metadata.stream_id)
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
    Ok((expected_position, expected_version))
}
