use crate::{AgentId, EventId, MemoryError, Result, SessionId, StreamId, Timestamp};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewEvent<E> {
    pub id: EventId,
    pub event_type: String,
    pub occurred_at: Timestamp,
    pub recorded_at: Timestamp,
    pub agent_id: AgentId,
    pub session_id: SessionId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<EventId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<EventId>,
    pub payload: E,
}

impl<E> NewEvent<E> {
    /// Creates an uncommitted event with caller-controlled time and identity.
    ///
    /// # Errors
    ///
    /// Rejects an empty event type or surrounding whitespace.
    pub fn new(
        id: EventId,
        event_type: impl Into<String>,
        occurred_at: Timestamp,
        recorded_at: Timestamp,
        agent_id: AgentId,
        session_id: SessionId,
        payload: E,
    ) -> Result<Self> {
        let event_type = event_type.into();
        if event_type.is_empty() || event_type.trim() != event_type {
            return Err(MemoryError::InvalidValue {
                field: "event_type",
                reason: "must be non-empty without surrounding whitespace",
            });
        }
        if occurred_at > recorded_at {
            return Err(MemoryError::InvalidValue {
                field: "occurred_at",
                reason: "must not be later than recorded_at",
            });
        }
        Ok(Self {
            id,
            event_type,
            occurred_at,
            recorded_at,
            agent_id,
            session_id,
            correlation_id: None,
            causation_id: None,
            payload,
        })
    }

    #[must_use]
    pub fn correlated_with(mut self, id: EventId) -> Self {
        self.correlation_id = Some(id);
        self
    }

    #[must_use]
    pub fn caused_by(mut self, id: EventId) -> Self {
        self.causation_id = Some(id);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventMetadata {
    pub id: EventId,
    pub stream_id: StreamId,
    pub stream_version: u64,
    pub global_position: u64,
    pub event_type: String,
    pub occurred_at: Timestamp,
    pub recorded_at: Timestamp,
    pub agent_id: AgentId,
    pub session_id: SessionId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<EventId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<EventId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredEvent<E> {
    pub metadata: EventMetadata,
    pub payload: E,
}
