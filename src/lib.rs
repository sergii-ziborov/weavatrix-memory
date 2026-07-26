#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod codec;
mod context;
mod domain;
mod error;
mod event;
mod graph_projection;
mod id;
mod projection;
mod snapshot;
mod store;
mod time;

pub use codec::Codec;
#[cfg(feature = "json")]
pub use codec::JsonCodec;
pub use context::{
    BytesTokenEstimator, ContextBundle, ContextCompiler, ContextReceipt, ContextRequest,
    TokenEstimator,
};
pub use domain::{Confidence, Evidence, MemoryEvent, MemoryFact, MemoryNode, MemoryView};
pub use error::{MemoryError, Result};
pub use event::{EventMetadata, NewEvent, StoredEvent};
pub use graph_projection::project_graph;
pub use id::{AgentId, EntityId, EventId, FactId, SessionId, StreamId};
pub use projection::{
    MemoryProjection, Projection, ProjectionClock, ProjectionSnapshot, ReplayCursor, replay,
    replay_tracked, resume,
};
pub use snapshot::{FileSnapshotStore, InMemorySnapshotStore, SnapshotOptions, SnapshotStore};
pub use store::{
    CatchUpSubscription, Durability, EventStore, ExpectedVersion, FileEventStore, FileStoreOptions,
    InMemoryStore, RecoveryPolicy, SubscriptionCheckpoint,
};
pub use time::Timestamp;
