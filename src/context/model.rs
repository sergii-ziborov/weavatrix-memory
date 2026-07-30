use crate::{
    domain::MemoryView,
    error::{MemoryError, Result},
    id::EntityId,
    time::Timestamp,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use weavatrix_graph::Graph;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextRequest {
    pub seeds: Vec<EntityId>,
    pub valid_at: Timestamp,
    pub known_at: Timestamp,
    pub token_budget: usize,
    pub max_depth: usize,
    #[serde(default)]
    pub relations: BTreeSet<String>,
    #[serde(default)]
    pub repositories: BTreeSet<String>,
    #[serde(default)]
    pub branches: BTreeSet<String>,
}

impl ContextRequest {
    /// Creates a bounded context request.
    ///
    /// # Errors
    ///
    /// Requires at least one seed and a non-zero token budget.
    pub fn new(
        seeds: Vec<EntityId>,
        valid_at: Timestamp,
        known_at: Timestamp,
        token_budget: usize,
    ) -> Result<Self> {
        if seeds.is_empty() {
            return Err(MemoryError::InvalidValue {
                field: "context.seeds",
                reason: "at least one seed is required",
            });
        }
        if token_budget == 0 {
            return Err(MemoryError::InvalidValue {
                field: "context.token_budget",
                reason: "must be greater than zero",
            });
        }
        Ok(Self {
            seeds,
            valid_at,
            known_at,
            token_budget,
            max_depth: 2,
            relations: BTreeSet::new(),
            repositories: BTreeSet::new(),
            branches: BTreeSet::new(),
        })
    }

    /// Creates a seedless template for `ContextCompiler::compile_with_retrieval`.
    ///
    /// # Errors
    ///
    /// Rejects a zero token budget.
    pub fn for_retrieval(
        valid_at: Timestamp,
        known_at: Timestamp,
        token_budget: usize,
    ) -> Result<Self> {
        if token_budget == 0 {
            return Err(MemoryError::InvalidValue {
                field: "context.token_budget",
                reason: "must be greater than zero",
            });
        }
        Ok(Self {
            seeds: Vec::new(),
            valid_at,
            known_at,
            token_budget,
            max_depth: 2,
            relations: BTreeSet::new(),
            repositories: BTreeSet::new(),
            branches: BTreeSet::new(),
        })
    }

    #[must_use]
    pub const fn with_max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = max_depth;
        self
    }

    #[must_use]
    pub fn include_relation(mut self, relation: impl Into<String>) -> Self {
        self.relations.insert(relation.into());
        self
    }

    #[must_use]
    pub fn in_repository(mut self, repository: impl Into<String>) -> Self {
        self.repositories.insert(repository.into());
        self
    }

    #[must_use]
    pub fn on_branch(mut self, branch: impl Into<String>) -> Self {
        self.branches.insert(branch.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextReceipt {
    pub valid_at: Timestamp,
    pub known_at: Timestamp,
    pub source_position: Option<u64>,
    pub estimator: String,
    pub token_budget: usize,
    pub estimated_tokens: usize,
    pub examined_facts: usize,
    pub selected_facts: usize,
    pub omitted_by_budget: usize,
    pub excluded_by_scope: usize,
}

#[derive(Debug, Clone)]
pub struct ContextBundle {
    pub view: MemoryView,
    pub graph: Graph,
    pub receipt: ContextReceipt,
}
