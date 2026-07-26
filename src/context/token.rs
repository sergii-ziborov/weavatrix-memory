use crate::{MemoryError, MemoryFact, MemoryNode, Result};

pub trait TokenEstimator {
    fn estimate(&self, value: &str) -> usize;
    fn name(&self) -> &'static str;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BytesTokenEstimator {
    bytes_per_token: usize,
}

impl BytesTokenEstimator {
    /// Creates a deterministic byte-based fallback estimator.
    ///
    /// # Errors
    ///
    /// Rejects zero bytes per token.
    pub fn new(bytes_per_token: usize) -> Result<Self> {
        if bytes_per_token == 0 {
            return Err(MemoryError::InvalidValue {
                field: "bytes_per_token",
                reason: "must be greater than zero",
            });
        }
        Ok(Self { bytes_per_token })
    }
}

impl Default for BytesTokenEstimator {
    fn default() -> Self {
        Self { bytes_per_token: 4 }
    }
}

impl TokenEstimator for BytesTokenEstimator {
    fn estimate(&self, value: &str) -> usize {
        value.len().div_ceil(self.bytes_per_token).max(1)
    }

    fn name(&self) -> &'static str {
        "utf8_bytes"
    }
}

pub(super) fn node_tokens(estimator: &impl TokenEstimator, node: &MemoryNode) -> usize {
    estimator.estimate(node.id.as_str())
        + estimator.estimate(&node.kind)
        + estimator.estimate(&node.label)
        + node
            .repository
            .iter()
            .chain(node.branch.iter())
            .map(|value| estimator.estimate(value))
            .sum::<usize>()
        + node
            .attributes
            .iter()
            .map(|(key, value)| estimator.estimate(key) + estimator.estimate(value))
            .sum::<usize>()
        + 6
}

pub(super) fn fact_tokens(estimator: &impl TokenEstimator, fact: &MemoryFact) -> usize {
    estimator.estimate(fact.id.as_str())
        + estimator.estimate(fact.source.as_str())
        + estimator.estimate(&fact.relation)
        + estimator.estimate(fact.target.as_str())
        + fact
            .evidence
            .iter()
            .map(|item| {
                estimator.estimate(&item.kind)
                    + estimator.estimate(&item.source)
                    + item
                        .locator
                        .iter()
                        .chain(item.digest.iter())
                        .map(|value| estimator.estimate(value))
                        .sum::<usize>()
            })
            .sum::<usize>()
        + 16
}
