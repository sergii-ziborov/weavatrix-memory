use crate::{Confidence, EntityId, MemoryEvent, NewEvent, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkPolicy {
    pub minimum_score: u16,
    pub minimum_margin: u16,
    pub create_unmatched: bool,
}

impl Default for LinkPolicy {
    fn default() -> Self {
        Self {
            minimum_score: 8_000,
            minimum_margin: 500,
            create_unmatched: true,
        }
    }
}

impl LinkPolicy {
    /// Creates a strict entity-linking policy in basis points.
    ///
    /// # Errors
    ///
    /// Rejects scores or margins above 10,000.
    pub fn new(minimum_score: u16, minimum_margin: u16, create_unmatched: bool) -> Result<Self> {
        if minimum_score > 10_000 || minimum_margin > 10_000 {
            return Err(crate::MemoryError::InvalidValue {
                field: "link_policy",
                reason: "scores must be between 0 and 10,000 basis points",
            });
        }
        Ok(Self {
            minimum_score,
            minimum_margin,
            create_unmatched,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkMethod {
    StableId,
    ExternalId,
    ProviderHint,
    ScopedLabel,
    Label,
    Alias,
    Created,
    Ambiguous,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkCandidate {
    pub entity_id: EntityId,
    pub score: Confidence,
    pub method: LinkMethod,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkDecision {
    pub mention_id: String,
    pub entity_id: Option<EntityId>,
    pub score: Confidence,
    pub method: LinkMethod,
    pub candidates: Vec<LinkCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectedRelation {
    pub relation_id: String,
    pub source_mention: String,
    pub target_mention: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractionPlan {
    pub provider: String,
    pub source: String,
    pub events: Vec<NewEvent<MemoryEvent>>,
    pub links: Vec<LinkDecision>,
    pub rejected_relations: Vec<RejectedRelation>,
}

impl ExtractionPlan {
    #[must_use]
    pub fn node_event_count(&self) -> usize {
        self.events
            .iter()
            .filter(|event| matches!(event.payload, MemoryEvent::NodeUpserted { .. }))
            .count()
    }

    #[must_use]
    pub fn fact_event_count(&self) -> usize {
        self.events
            .iter()
            .filter(|event| matches!(event.payload, MemoryEvent::FactRecorded { .. }))
            .count()
    }
}
