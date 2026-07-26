use super::{ExtractionInput, ExtractionOutput};
use std::{error::Error, fmt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractionError {
    pub provider: String,
    pub message: String,
}

impl ExtractionError {
    #[must_use]
    pub fn new(provider: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ExtractionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "extraction provider {} failed: {}",
            self.provider, self.message
        )
    }
}

impl Error for ExtractionError {}

/// Converts source material into typed mentions and relations.
///
/// Providers may wrap AST parsers, issue trackers, language models, or future
/// Weavatrix search packages. The memory core owns validation, entity linking,
/// provenance, and event creation.
pub trait ExtractionProvider: Sync {
    fn name(&self) -> &str;

    /// Extracts candidates without mutating memory.
    ///
    /// # Errors
    ///
    /// Returns a provider-labelled failure. The engine rejects failures whose
    /// provider identity does not match [`Self::name`].
    fn extract(&self, input: &ExtractionInput) -> Result<ExtractionOutput, ExtractionError>;
}
