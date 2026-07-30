use super::EventStore;
use crate::{
    error::{MemoryError, Result},
    event::StoredEvent,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionCheckpoint {
    pub global_position: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatchUpSubscription {
    checkpoint: SubscriptionCheckpoint,
    delivered_through: Option<u64>,
    batch_size: usize,
}

impl CatchUpSubscription {
    /// Creates an explicitly acknowledged catch-up subscription.
    ///
    /// # Errors
    ///
    /// Rejects a zero batch size.
    pub fn new(checkpoint: SubscriptionCheckpoint, batch_size: usize) -> Result<Self> {
        if batch_size == 0 {
            return Err(MemoryError::InvalidValue {
                field: "subscription.batch_size",
                reason: "must be greater than zero",
            });
        }
        Ok(Self {
            checkpoint,
            delivered_through: None,
            batch_size,
        })
    }

    #[must_use]
    pub const fn checkpoint(&self) -> SubscriptionCheckpoint {
        self.checkpoint
    }

    /// Returns the next batch without advancing the durable checkpoint.
    pub fn poll<E, S>(&mut self, store: &S) -> Vec<StoredEvent<E>>
    where
        E: Clone,
        S: EventStore<E>,
    {
        let events = store.load_all(self.checkpoint.global_position, self.batch_size);
        self.delivered_through = events.last().map(|event| event.metadata.global_position);
        events
    }

    /// Acknowledges all delivered events through an inclusive position.
    ///
    /// # Errors
    ///
    /// Rejects acknowledgements beyond the last delivered event or behind the
    /// current checkpoint.
    pub fn acknowledge(&mut self, global_position: u64) -> Result<()> {
        let delivered = self.delivered_through.ok_or(MemoryError::InvalidValue {
            field: "subscription.acknowledge",
            reason: "no events have been delivered",
        })?;
        if global_position > delivered
            || self
                .checkpoint
                .global_position
                .is_some_and(|current| global_position < current)
        {
            return Err(MemoryError::InvalidValue {
                field: "subscription.acknowledge",
                reason: "position is outside the delivered range",
            });
        }
        self.checkpoint.global_position = Some(global_position);
        self.delivered_through = None;
        Ok(())
    }
}
