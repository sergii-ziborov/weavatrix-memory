mod belief;
mod consolidation;
mod drift;
mod gaps;
mod model;

pub use model::{
    BeliefRevisionReport, BeliefRevisionRequest, CascadeEffect, ChangeKind, ConsolidationAction,
    ConsolidationKind, ConsolidationPlan, Contradiction, DriftReport, DriftSnapshot, GapKind,
    GapReport, ReasoningGap, ReasoningGapRequest,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct MemoryAnalytics;
