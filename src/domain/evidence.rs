use crate::error::{MemoryError, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Confidence(u16);

impl Confidence {
    pub const CERTAIN: Self = Self(10_000);

    /// Creates confidence measured in basis points from 0 through 10,000.
    ///
    /// # Errors
    ///
    /// Rejects values above 10,000.
    pub fn from_basis_points(value: u16) -> Result<Self> {
        if value > 10_000 {
            return Err(MemoryError::InvalidValue {
                field: "confidence",
                reason: "must be between 0 and 10,000 basis points",
            });
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn basis_points(self) -> u16 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub kind: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locator: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

impl Evidence {
    /// Creates a provenance record.
    ///
    /// # Errors
    ///
    /// Rejects empty or whitespace-padded kind and source values.
    pub fn new(kind: impl Into<String>, source: impl Into<String>) -> Result<Self> {
        let evidence = Self {
            kind: kind.into(),
            source: source.into(),
            locator: None,
            digest: None,
        };
        evidence.validate()?;
        Ok(evidence)
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
        super::validate_text("evidence.kind", &self.kind)?;
        super::validate_text("evidence.source", &self.source)?;
        super::validate_optional_text("evidence.locator", self.locator.as_deref())?;
        super::validate_optional_text("evidence.digest", self.digest.as_deref())
    }
}
