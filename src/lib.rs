#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod analytics;
mod codec;
mod context;
mod domain;
mod error;
mod evaluation;
mod event;
mod extraction;
mod graph_projection;
mod id;
mod projection;
mod snapshot;
mod store;
mod time;

pub use analytics::{
    BeliefRevisionReport, BeliefRevisionRequest, CascadeEffect, ChangeKind, ConsolidationAction,
    ConsolidationKind, ConsolidationPlan, Contradiction, DriftReport, DriftSnapshot, GapKind,
    GapReport, MemoryAnalytics, ReasoningGap, ReasoningGapRequest,
};
pub use codec::Codec;
#[cfg(feature = "json")]
pub use codec::JsonCodec;
pub use context::{
    BytesTokenEstimator, ContextBundle, ContextCompiler, ContextReceipt, ContextRequest,
    FusedRetrievalHit, RetrievalChannel, RetrievalError, RetrievalHit, RetrievalProvider,
    RetrievalQuery, RetrievalResult, RetrievalSource, RetrievedContextBundle, TokenEstimator,
    fuse_retrieval,
};
pub use domain::{Confidence, Evidence, MemoryEvent, MemoryFact, MemoryNode, MemoryView};
pub use error::{MemoryError, Result};
pub use evaluation::{
    EvaluationCase, EvaluationReport, RankedPrediction, RetrievalMetrics, evaluate_retrieval,
};
pub use event::{EventMetadata, NewEvent, StoredEvent};
pub use extraction::{
    AutoExtractionEngine, EntityHint, EntityLinker, ExtractedEntity, ExtractedRelation,
    ExtractionError, ExtractionInput, ExtractionOutput, ExtractionPlan, ExtractionProvider,
    LinkCandidate, LinkDecision, LinkMethod, LinkPolicy, RejectedRelation, TextSpan,
};
pub use graph_projection::project_graph;
pub use id::{AgentId, EntityId, EventId, FactId, SessionId, StreamId};
pub use projection::{
    CompactSnapshotCodec, MemoryProjection, Projection, ProjectionClock, ProjectionSnapshot,
    ReplayCursor, replay, replay_tracked, resume,
};
pub use snapshot::{FileSnapshotStore, InMemorySnapshotStore, SnapshotOptions, SnapshotStore};
pub use store::{
    CatchUpSubscription, Durability, EventStore, ExpectedVersion, FileEventStore, FileStoreOptions,
    InMemoryStore, RecoveryPolicy, SubscriptionCheckpoint,
};
pub use time::Timestamp;
