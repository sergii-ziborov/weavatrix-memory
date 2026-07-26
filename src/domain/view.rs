use super::{MemoryFact, MemoryNode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryView {
    pub nodes: Vec<MemoryNode>,
    pub facts: Vec<MemoryFact>,
}
