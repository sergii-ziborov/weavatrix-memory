mod build;
mod validation;

use super::{EntityLinker, ExtractionInput, ExtractionPlan, ExtractionProvider, LinkPolicy};
use crate::{
    domain::MemoryView,
    error::{MemoryError, Result},
};

#[derive(Debug, Clone, Copy, Default)]
pub struct AutoExtractionEngine {
    policy: LinkPolicy,
}

impl AutoExtractionEngine {
    #[must_use]
    pub const fn new(policy: LinkPolicy) -> Self {
        Self { policy }
    }

    /// Extracts, links, and converts one source into an idempotent event plan.
    ///
    /// # Errors
    ///
    /// Rejects invalid provider output, unsafe ambiguity, provenance mismatch,
    /// identifier collisions, and invalid temporal facts.
    pub fn plan<P: ExtractionProvider>(
        &self,
        provider: &P,
        input: &ExtractionInput,
        view: &MemoryView,
    ) -> Result<ExtractionPlan> {
        let linker = EntityLinker::from_view(view)?;
        self.plan_with_linker(provider, input, &linker)
    }

    /// Uses a reusable linker for batch extraction against one projection.
    ///
    /// # Errors
    ///
    /// Has the same validation contract as [`Self::plan`].
    pub fn plan_with_linker<P: ExtractionProvider>(
        &self,
        provider: &P,
        input: &ExtractionInput,
        linker: &EntityLinker,
    ) -> Result<ExtractionPlan> {
        input.validate()?;
        crate::domain::validate_text("extraction.provider", provider.name())?;
        let output = provider.extract(input).map_err(|error| {
            if error.provider == provider.name() {
                MemoryError::from(error)
            } else {
                MemoryError::Extraction {
                    provider: provider.name().to_owned(),
                    message: "provider error identity does not match provider name".to_owned(),
                }
            }
        })?;
        validation::validate_output(&output, input)?;
        build::build_plan(self.policy, provider.name(), input, linker, &output)
    }
}
