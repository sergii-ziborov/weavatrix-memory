use crate::{
    AgentId, Confidence, EntityId, MemoryError, Result, SessionId, Timestamp,
    domain::{validate_optional_text, validate_text},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextSpan {
    pub start: usize,
    pub end: usize,
}

impl TextSpan {
    /// Creates a half-open UTF-8 byte span.
    ///
    /// # Errors
    ///
    /// Rejects empty or reversed spans.
    pub fn new(start: usize, end: usize) -> Result<Self> {
        if start >= end {
            return Err(MemoryError::InvalidValue {
                field: "text_span",
                reason: "start must be before end",
            });
        }
        Ok(Self { start, end })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractionInput {
    pub source: String,
    pub content: String,
    pub occurred_at: Timestamp,
    pub recorded_at: Timestamp,
    pub agent_id: AgentId,
    pub session_id: SessionId,
    pub repository: Option<String>,
    pub branch: Option<String>,
    pub locator: Option<String>,
    pub digest: Option<String>,
}

impl ExtractionInput {
    /// Creates source material with caller-controlled temporal provenance.
    ///
    /// # Errors
    ///
    /// Rejects empty content, invalid source text, or future occurrence time.
    pub fn new(
        source: impl Into<String>,
        content: impl Into<String>,
        occurred_at: Timestamp,
        recorded_at: Timestamp,
        agent_id: AgentId,
        session_id: SessionId,
    ) -> Result<Self> {
        let input = Self {
            source: source.into(),
            content: content.into(),
            occurred_at,
            recorded_at,
            agent_id,
            session_id,
            repository: None,
            branch: None,
            locator: None,
            digest: None,
        };
        input.validate()?;
        Ok(input)
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
    pub fn with_locator(mut self, locator: impl Into<String>) -> Self {
        self.locator = Some(locator.into());
        self
    }

    #[must_use]
    pub fn with_digest(mut self, digest: impl Into<String>) -> Self {
        self.digest = Some(digest.into());
        self
    }

    pub(crate) fn validate(&self) -> Result<()> {
        validate_text("extraction.source", &self.source)?;
        if self.content.trim().is_empty() {
            return Err(MemoryError::InvalidValue {
                field: "extraction.content",
                reason: "must contain non-whitespace text",
            });
        }
        if self.occurred_at > self.recorded_at {
            return Err(MemoryError::InvalidValue {
                field: "extraction.occurred_at",
                reason: "must not be later than recorded_at",
            });
        }
        validate_optional_text("extraction.repository", self.repository.as_deref())?;
        validate_optional_text("extraction.branch", self.branch.as_deref())?;
        validate_optional_text("extraction.locator", self.locator.as_deref())?;
        validate_optional_text("extraction.digest", self.digest.as_deref())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityHint {
    pub entity_id: EntityId,
    pub confidence: Confidence,
    pub source: String,
}

impl EntityHint {
    /// Creates a provider-supplied link candidate.
    ///
    /// # Errors
    ///
    /// Rejects an empty or whitespace-padded hint source.
    pub fn new(
        entity_id: EntityId,
        confidence: Confidence,
        source: impl Into<String>,
    ) -> Result<Self> {
        let hint = Self {
            entity_id,
            confidence,
            source: source.into(),
        };
        validate_text("entity_hint.source", &hint.source)?;
        Ok(hint)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractedEntity {
    pub local_id: String,
    pub kind: String,
    pub label: String,
    pub confidence: Confidence,
    pub stable_id: Option<EntityId>,
    pub aliases: Vec<String>,
    pub attributes: BTreeMap<String, String>,
    pub hints: Vec<EntityHint>,
    pub span: Option<TextSpan>,
}

impl ExtractedEntity {
    /// Creates a provider-local entity mention.
    ///
    /// # Errors
    ///
    /// Rejects invalid local identifier, kind, or label text.
    pub fn new(
        local_id: impl Into<String>,
        kind: impl Into<String>,
        label: impl Into<String>,
        confidence: Confidence,
    ) -> Result<Self> {
        let entity = Self {
            local_id: local_id.into(),
            kind: kind.into(),
            label: label.into(),
            confidence,
            stable_id: None,
            aliases: Vec::new(),
            attributes: BTreeMap::new(),
            hints: Vec::new(),
            span: None,
        };
        entity.validate()?;
        Ok(entity)
    }

    #[must_use]
    pub fn with_stable_id(mut self, id: EntityId) -> Self {
        self.stable_id = Some(id);
        self
    }

    #[must_use]
    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    #[must_use]
    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }

    #[must_use]
    pub fn with_hint(mut self, hint: EntityHint) -> Self {
        self.hints.push(hint);
        self
    }

    #[must_use]
    pub const fn with_span(mut self, span: TextSpan) -> Self {
        self.span = Some(span);
        self
    }

    pub(crate) fn validate(&self) -> Result<()> {
        validate_text("extracted_entity.local_id", &self.local_id)?;
        validate_text("extracted_entity.kind", &self.kind)?;
        validate_text("extracted_entity.label", &self.label)?;
        for alias in &self.aliases {
            validate_text("extracted_entity.alias", alias)?;
        }
        for key in self.attributes.keys() {
            validate_text("extracted_entity.attribute.key", key)?;
        }
        for (key, value) in &self.attributes {
            if key.starts_with("external_id.") {
                validate_text("extracted_entity.external_id", value)?;
            }
        }
        for hint in &self.hints {
            validate_text("entity_hint.source", &hint.source)?;
        }
        Ok(())
    }
}
