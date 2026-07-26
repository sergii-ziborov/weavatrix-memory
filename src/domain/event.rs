use super::{Evidence, MemoryFact, MemoryNode};
use crate::{FactId, Timestamp};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MemoryEvent {
    NodeUpserted {
        node: MemoryNode,
    },
    FactRecorded {
        fact: MemoryFact,
    },
    FactRetracted {
        fact_id: FactId,
        valid_until: Timestamp,
        evidence: Vec<Evidence>,
    },
}

impl MemoryEvent {
    #[must_use]
    pub const fn event_type(&self) -> &'static str {
        match self {
            Self::NodeUpserted { .. } => "node_upserted",
            Self::FactRecorded { .. } => "fact_recorded",
            Self::FactRetracted { .. } => "fact_retracted",
        }
    }
}
