mod memory;
mod replay;

pub use memory::{CompactSnapshotCodec, MemoryProjection, ProjectionClock};
pub use replay::{
    Projection, ProjectionSnapshot, ReplayCursor, replay, replay_owned, replay_tracked, resume,
};
