use super::{MemoryFact, MemoryNode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryView {
    pub nodes: Vec<MemoryNode>,
    pub facts: Vec<MemoryFact>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryViewRef<'a> {
    pub nodes: Vec<&'a MemoryNode>,
    pub facts: Vec<&'a MemoryFact>,
}

impl MemoryViewRef<'_> {
    #[must_use]
    pub fn into_owned(self) -> MemoryView {
        MemoryView {
            nodes: self.nodes.into_iter().cloned().collect(),
            facts: self.facts.into_iter().cloned().collect(),
        }
    }
}
