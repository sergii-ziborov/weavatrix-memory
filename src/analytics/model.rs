use crate::{
    domain::MemoryFact,
    id::{EntityId, FactId},
    projection::ProjectionClock,
    time::Timestamp,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BeliefRevisionRequest {
    pub hypothesis: MemoryFact,
    pub clock: ProjectionClock,
    pub max_depth: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contradiction {
    pub fact: FactId,
    pub strength_bps: u16,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CascadeEffect {
    pub entity: EntityId,
    pub depth: usize,
    pub revised_confidence_bps: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BeliefRevisionReport {
    pub contradictions: Vec<Contradiction>,
    pub cascade: Vec<CascadeEffect>,
    pub invalidated_decisions: Vec<EntityId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReasoningGapRequest {
    pub clock: ProjectionClock,
    pub minimum_supports: usize,
    pub low_confidence_bps: u16,
    pub unstable_revision_count: usize,
    pub stale_after_micros: i64,
    pub max_results: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapKind {
    UnjustifiedDecision,
    SingleSourceInference,
    LowConfidenceFoundation,
    UnstableKnowledge,
    StaleEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningGap {
    pub entity: EntityId,
    pub fact: Option<FactId>,
    pub kind: GapKind,
    pub severity_bps: u16,
    pub downstream_entities: usize,
    pub explanation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GapReport {
    pub gaps: Vec<ReasoningGap>,
    pub health_bps: u16,
    pub analyzed_entities: usize,
    pub analyzed_facts: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Initial,
    Refined,
    Corrected,
    Reinforced,
    Weakened,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriftSnapshot {
    pub fact: FactId,
    pub target: EntityId,
    pub recorded_at: Timestamp,
    pub confidence_bps: u16,
    pub change: ChangeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriftReport {
    pub source: EntityId,
    pub relation: String,
    pub snapshots: Vec<DriftSnapshot>,
    pub correction_count: usize,
    pub stability_bps: u16,
    pub likely_to_change: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsolidationKind {
    SupersedeDuplicate,
    ReviewOrphan,
    CompactRevisionChain,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsolidationAction {
    pub kind: ConsolidationKind,
    pub keep: Option<FactId>,
    pub affected_facts: Vec<FactId>,
    pub affected_entities: Vec<EntityId>,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsolidationPlan {
    pub actions: Vec<ConsolidationAction>,
    pub projected_savings: usize,
    pub source_position: Option<u64>,
}
