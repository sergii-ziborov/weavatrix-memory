use super::super::ExpectedVersion;
use crate::{
    error::{MemoryError, Result},
    event::NewEvent,
    id::{EventId, StreamId},
};
use std::collections::HashSet;

pub(super) fn expected(
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

pub(super) fn unique_ids<E>(known: &HashSet<EventId>, events: &[NewEvent<E>]) -> Result<()> {
    let mut batch = HashSet::<&EventId>::with_capacity(events.len());
    for event in events {
        if known.contains(&event.id) || !batch.insert(&event.id) {
            return Err(MemoryError::DuplicateEvent {
                id: event.id.to_string(),
            });
        }
    }
    Ok(())
}
