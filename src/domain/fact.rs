use super::{Confidence, Evidence};
use crate::{
    error::{MemoryError, Result},
    id::{AgentId, EntityId, FactId, SessionId},
    time::Timestamp,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryFact {
    pub id: FactId,
    pub source: EntityId,
    pub relation: String,
    pub target: EntityId,
    pub valid_from: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<Timestamp>,
    pub observed_at: Timestamp,
    pub recorded_at: Timestamp,
    pub agent_id: AgentId,
    pub session_id: SessionId,
    pub confidence: Confidence,
    pub evidence: Vec<Evidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<FactId>,
}

impl MemoryFact {
    /// Creates an evidence-carrying temporal relation.
    ///
    /// # Errors
    ///
    /// Rejects invalid relation text or missing evidence.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: FactId,
        source: EntityId,
        relation: impl Into<String>,
        target: EntityId,
        valid_from: Timestamp,
        recorded_at: Timestamp,
        agent_id: AgentId,
        session_id: SessionId,
        evidence: Evidence,
    ) -> Result<Self> {
        let fact = Self {
            id,
            source,
            relation: relation.into(),
            target,
            valid_from,
            valid_until: None,
            observed_at: recorded_at,
            recorded_at,
            agent_id,
            session_id,
            confidence: Confidence::CERTAIN,
            evidence: vec![evidence],
            supersedes: None,
        };
        fact.validate()?;
        Ok(fact)
    }

    /// Sets the exclusive end of the fact's valid interval.
    ///
    /// # Errors
    ///
    /// Rejects an end that is not later than `valid_from`.
    pub fn valid_until(mut self, value: Timestamp) -> Result<Self> {
        self.valid_until = Some(value);
        self.validate()?;
        Ok(self)
    }

    #[must_use]
    pub const fn observed_at(mut self, value: Timestamp) -> Self {
        self.observed_at = value;
        self
    }

    #[must_use]
    pub const fn with_confidence(mut self, value: Confidence) -> Self {
        self.confidence = value;
        self
    }

    /// Links this fact to the historical fact it replaces.
    ///
    /// # Errors
    ///
    /// Rejects self-supersession.
    pub fn supersedes(mut self, fact: FactId) -> Result<Self> {
        if fact == self.id {
            return Err(MemoryError::InvalidValue {
                field: "fact.supersedes",
                reason: "a fact cannot supersede itself",
            });
        }
        self.supersedes = Some(fact);
        Ok(self)
    }

    #[must_use]
    pub fn with_evidence(mut self, evidence: Evidence) -> Self {
        self.evidence.push(evidence);
        self
    }

    pub(crate) fn validate(&self) -> Result<()> {
        super::validate_text("fact.relation", &self.relation)?;
        if self.observed_at > self.recorded_at {
            return Err(MemoryError::InvalidValue {
                field: "fact.observed_at",
                reason: "must not be later than recorded_at",
            });
        }
        if self
            .valid_until
            .is_some_and(|until| until <= self.valid_from)
        {
            return Err(MemoryError::InvalidValue {
                field: "fact.valid_until",
                reason: "must be later than valid_from",
            });
        }
        if self.evidence.is_empty() {
            return Err(MemoryError::InvalidValue {
                field: "fact.evidence",
                reason: "at least one evidence item is required",
            });
        }
        self.evidence.iter().try_for_each(Evidence::validate)
    }
}
