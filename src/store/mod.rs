mod file;
pub(crate) mod frame;
mod in_memory;
mod subscription;

pub use file::{Durability, FileEventStore, FileStoreOptions, RecoveryPolicy};
pub use in_memory::InMemoryStore;
pub use subscription::{CatchUpSubscription, SubscriptionCheckpoint};

use crate::{
    error::Result,
    event::{NewEvent, StoredEvent},
    id::StreamId,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AppendReceipt {
    pub event_count: usize,
    pub first_stream_version: Option<u64>,
    pub last_stream_version: Option<u64>,
    pub first_global_position: Option<u64>,
    pub last_global_position: Option<u64>,
}

impl AppendReceipt {
    fn from_events<E>(events: &[StoredEvent<E>]) -> Self {
        Self {
            event_count: events.len(),
            first_stream_version: events.first().map(|event| event.metadata.stream_version),
            last_stream_version: events.last().map(|event| event.metadata.stream_version),
            first_global_position: events.first().map(|event| event.metadata.global_position),
            last_global_position: events.last().map(|event| event.metadata.global_position),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedVersion {
    Any,
    NoStream,
    Exact(u64),
}

pub trait EventStore<E: Clone> {
    /// Atomically appends a batch to one stream.
    ///
    /// # Errors
    ///
    /// Returns a version conflict, duplicate event, or capacity error without
    /// committing a partial batch.
    fn append(
        &mut self,
        stream: &StreamId,
        expected: ExpectedVersion,
        events: &[NewEvent<E>],
    ) -> Result<Vec<StoredEvent<E>>>;

    /// Appends an owned batch without requiring callers to retain the inputs.
    ///
    /// Stores may override this to move payloads into committed envelopes and
    /// avoid the input clone required by [`Self::append`].
    ///
    /// # Errors
    ///
    /// Returns the same version, duplicate-event, capacity, or persistence
    /// errors as [`Self::append`].
    fn append_owned(
        &mut self,
        stream: &StreamId,
        expected: ExpectedVersion,
        events: Vec<NewEvent<E>>,
    ) -> Result<Vec<StoredEvent<E>>> {
        self.append(stream, expected, &events)
    }

    /// Appends an owned batch and returns positions without cloning committed
    /// payloads back to the caller.
    ///
    /// Stores may override this high-throughput path to move the committed
    /// envelopes directly into storage.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::append_owned`].
    fn append_owned_receipt(
        &mut self,
        stream: &StreamId,
        expected: ExpectedVersion,
        events: Vec<NewEvent<E>>,
    ) -> Result<AppendReceipt> {
        self.append_owned(stream, expected, events)
            .map(|events| AppendReceipt::from_events(&events))
    }

    fn load_stream(&self, stream: &StreamId, after: Option<u64>) -> Vec<StoredEvent<E>>;

    fn load_all(&self, after: Option<u64>, limit: usize) -> Vec<StoredEvent<E>>;

    fn stream_version(&self, stream: &StreamId) -> Option<u64>;

    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
