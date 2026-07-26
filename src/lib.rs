#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod context;
mod domain;
mod error;
mod event;
mod graph_projection;
mod id;
mod projection;
mod store;
mod time;

pub use context::{
    BytesTokenEstimator, ContextBundle, ContextCompiler, ContextReceipt, ContextRequest,
    TokenEstimator,
};
pub use domain::{Confidence, Evidence, MemoryEvent, MemoryFact, MemoryNode, MemoryView};
pub use error::{MemoryError, Result};
pub use event::{EventMetadata, NewEvent, StoredEvent};
pub use graph_projection::project_graph;
pub use id::{AgentId, EntityId, EventId, FactId, SessionId, StreamId};
pub use projection::{MemoryProjection, Projection, ProjectionClock, replay};
pub use store::{EventStore, ExpectedVersion, InMemoryStore};
pub use time::Timestamp;
