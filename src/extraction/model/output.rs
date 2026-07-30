use super::TextSpan;
use crate::{
    domain::{Confidence, validate_text},
    error::Result,
    time::Timestamp,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractedRelation {
    pub local_id: String,
    pub source: String,
    pub relation: String,
    pub target: String,
    pub confidence: Confidence,
    pub valid_from: Option<Timestamp>,
    pub valid_until: Option<Timestamp>,
    pub span: Option<TextSpan>,
}

impl ExtractedRelation {
    /// Creates a provider-local relation between two mentions.
    ///
    /// # Errors
    ///
    /// Rejects invalid identifiers or relation text.
    pub fn new(
        local_id: impl Into<String>,
        source: impl Into<String>,
        relation: impl Into<String>,
        target: impl Into<String>,
        confidence: Confidence,
    ) -> Result<Self> {
        let relation = Self {
            local_id: local_id.into(),
            source: source.into(),
            relation: relation.into(),
            target: target.into(),
            confidence,
            valid_from: None,
            valid_until: None,
            span: None,
        };
        relation.validate()?;
        Ok(relation)
    }

    #[must_use]
    pub const fn valid_from(mut self, value: Timestamp) -> Self {
        self.valid_from = Some(value);
        self
    }

    #[must_use]
    pub const fn valid_until(mut self, value: Timestamp) -> Self {
        self.valid_until = Some(value);
        self
    }

    #[must_use]
    pub const fn with_span(mut self, span: TextSpan) -> Self {
        self.span = Some(span);
        self
    }

    pub(crate) fn validate(&self) -> Result<()> {
        validate_text("extracted_relation.local_id", &self.local_id)?;
        validate_text("extracted_relation.source", &self.source)?;
        validate_text("extracted_relation.relation", &self.relation)?;
        validate_text("extracted_relation.target", &self.target)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractionOutput {
    pub entities: Vec<super::ExtractedEntity>,
    pub relations: Vec<ExtractedRelation>,
}
