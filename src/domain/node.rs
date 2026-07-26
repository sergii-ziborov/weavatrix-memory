use crate::{EntityId, MemoryError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryNode {
    pub id: EntityId,
    pub kind: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, String>,
}

impl MemoryNode {
    /// Creates a memory entity.
    ///
    /// # Errors
    ///
    /// Rejects empty or whitespace-padded kind and an empty label.
    pub fn new(id: EntityId, kind: impl Into<String>, label: impl Into<String>) -> Result<Self> {
        let node = Self {
            id,
            kind: kind.into(),
            label: label.into(),
            repository: None,
            branch: None,
            attributes: BTreeMap::new(),
        };
        node.validate()?;
        Ok(node)
    }

    #[must_use]
    pub fn in_repository(mut self, repository: impl Into<String>) -> Self {
        self.repository = Some(repository.into());
        self
    }

    #[must_use]
    pub fn on_branch(mut self, branch: impl Into<String>) -> Self {
        self.branch = Some(branch.into());
        self
    }

    #[must_use]
    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }

    pub(crate) fn validate(&self) -> Result<()> {
        super::validate_text("node.kind", &self.kind)?;
        if self.label.is_empty() {
            return Err(MemoryError::InvalidValue {
                field: "node.label",
                reason: "must be non-empty",
            });
        }
        super::validate_optional_text("node.repository", self.repository.as_deref())?;
        super::validate_optional_text("node.branch", self.branch.as_deref())?;
        for key in self.attributes.keys() {
            super::validate_text("node.attribute.key", key)?;
        }
        Ok(())
    }
}
