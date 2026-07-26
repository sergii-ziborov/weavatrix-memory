mod file;
pub(crate) mod frame;
mod in_memory;
mod subscription;

pub use file::{Durability, FileEventStore, FileStoreOptions, RecoveryPolicy};
pub use in_memory::InMemoryStore;
pub use subscription::{CatchUpSubscription, SubscriptionCheckpoint};

use crate::{NewEvent, Result, StoredEvent, StreamId};

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

    fn load_stream(&self, stream: &StreamId, after: Option<u64>) -> Vec<StoredEvent<E>>;

    fn load_all(&self, after: Option<u64>, limit: usize) -> Vec<StoredEvent<E>>;

    fn stream_version(&self, stream: &StreamId) -> Option<u64>;

    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
