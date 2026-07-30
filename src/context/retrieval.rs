use crate::error::MemoryError;
use crate::id::EntityId;
use core::fmt;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const RRF_K: u64 = 60;
const RRF_SCALE: u64 = 1_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalChannel {
    Literal,
    Lexical,
    Semantic,
    Hybrid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalQuery {
    pub text: String,
    pub limit: usize,
    pub channels: BTreeSet<RetrievalChannel>,
}

impl RetrievalQuery {
    /// Creates a provider-neutral retrieval request.
    ///
    /// # Errors
    ///
    /// Rejects empty text and a zero result limit.
    pub fn new(text: impl Into<String>, limit: usize) -> RetrievalResult<Self> {
        let text = text.into();
        if text.is_empty() || text.trim() != text {
            return Err(RetrievalError::new(
                "query",
                "text must be non-empty and trimmed",
            ));
        }
        if limit == 0 {
            return Err(RetrievalError::new(
                "query",
                "limit must be greater than zero",
            ));
        }
        Ok(Self {
            text,
            limit,
            channels: BTreeSet::new(),
        })
    }

    #[must_use]
    pub fn include(mut self, channel: RetrievalChannel) -> Self {
        self.channels.insert(channel);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalHit {
    pub entity: EntityId,
    pub channel: RetrievalChannel,
    pub score: u32,
}

impl RetrievalHit {
    #[must_use]
    pub const fn new(entity: EntityId, channel: RetrievalChannel, score: u32) -> Self {
        Self {
            entity,
            channel,
            score,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalSource {
    pub provider: String,
    pub channel: RetrievalChannel,
    pub rank: usize,
    pub raw_score: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FusedRetrievalHit {
    pub entity: EntityId,
    pub fused_score: u64,
    pub sources: Vec<RetrievalSource>,
}

pub trait RetrievalProvider: Sync {
    fn name(&self) -> &str;

    /// Finds exact memory entity identifiers for a free-text query.
    ///
    /// # Errors
    ///
    /// Returns a provider-specific retrieval failure.
    fn retrieve(&self, query: &RetrievalQuery) -> RetrievalResult<Vec<RetrievalHit>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievalError {
    pub provider: String,
    pub message: String,
}

impl RetrievalError {
    #[must_use]
    pub fn new(provider: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for RetrievalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "retrieval provider {} failed: {}",
            self.provider, self.message
        )
    }
}

impl std::error::Error for RetrievalError {}

impl From<RetrievalError> for MemoryError {
    fn from(value: RetrievalError) -> Self {
        Self::Retrieval {
            provider: value.provider,
            message: value.message,
        }
    }
}

pub type RetrievalResult<T> = core::result::Result<T, RetrievalError>;

/// Fuses provider ranks with deterministic integer reciprocal-rank fusion.
///
/// Scores from BM25 and vector stores are deliberately not compared directly.
/// Providers, ties, duplicates, and final output all have stable ordering.
///
/// # Errors
///
/// Propagates provider failures and rejects duplicate or empty provider names.
pub fn fuse_retrieval(
    providers: &[&dyn RetrievalProvider],
    query: &RetrievalQuery,
) -> RetrievalResult<Vec<FusedRetrievalHit>> {
    let mut providers = providers.to_vec();
    providers.sort_by(|left, right| left.name().cmp(right.name()));
    validate_providers(&providers)?;
    let mut fused = BTreeMap::<EntityId, FusedRetrievalHit>::new();
    for provider in providers {
        let mut hits = provider.retrieve(query)?;
        hits.retain(|hit| query.channels.is_empty() || query.channels.contains(&hit.channel));
        hits.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| left.entity.cmp(&right.entity))
                .then_with(|| left.channel.cmp(&right.channel))
        });
        let mut seen = BTreeSet::new();
        hits.retain(|hit| seen.insert(hit.entity.clone()));
        for (offset, hit) in hits.into_iter().take(query.limit).enumerate() {
            let rank = offset + 1;
            let contribution = RRF_SCALE / (RRF_K + rank as u64);
            let entry = fused
                .entry(hit.entity.clone())
                .or_insert_with(|| FusedRetrievalHit {
                    entity: hit.entity,
                    fused_score: 0,
                    sources: Vec::new(),
                });
            entry.fused_score = entry.fused_score.saturating_add(contribution);
            entry.sources.push(RetrievalSource {
                provider: provider.name().to_owned(),
                channel: hit.channel,
                rank,
                raw_score: hit.score,
            });
        }
    }
    let mut output = fused.into_values().collect::<Vec<_>>();
    output.sort_by(|left, right| {
        right
            .fused_score
            .cmp(&left.fused_score)
            .then_with(|| left.entity.cmp(&right.entity))
    });
    output.truncate(query.limit);
    Ok(output)
}

fn validate_providers(providers: &[&dyn RetrievalProvider]) -> RetrievalResult<()> {
    let mut names = BTreeSet::new();
    for provider in providers {
        let name = provider.name();
        if name.is_empty() || name.trim() != name {
            return Err(RetrievalError::new(
                "provider",
                "provider names must be non-empty and trimmed",
            ));
        }
        if !names.insert(name) {
            return Err(RetrievalError::new(name, "provider names must be unique"));
        }
    }
    Ok(())
}
