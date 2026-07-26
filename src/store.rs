mod in_memory;

pub use in_memory::InMemoryStore;

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

    fn load_stream(&self, stream: &StreamId, after: Option<u64>) -> Vec<StoredEvent<E>>;

    fn load_all(&self, after: Option<u64>, limit: usize) -> Vec<StoredEvent<E>>;

    fn stream_version(&self, stream: &StreamId) -> Option<u64>;

    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
