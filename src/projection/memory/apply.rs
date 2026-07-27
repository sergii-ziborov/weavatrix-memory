use super::{MemoryProjection, state::NodeRevision};
use crate::{MemoryError, MemoryEvent, Projection, Result, StoredEvent};

impl Projection<MemoryEvent> for MemoryProjection {
    fn prepare_replay(&mut self, events: &[StoredEvent<MemoryEvent>]) {
        let mut nodes = 0;
        let mut facts = 0;
        for event in events {
            match event.payload {
                MemoryEvent::NodeUpserted { .. } => nodes += 1,
                MemoryEvent::FactRecorded { .. } => facts += 1,
                MemoryEvent::FactRetracted { .. } => {}
            }
        }
        self.nodes.reserve(nodes);
        self.facts.reserve(facts);
        self.node_lookup.reserve(nodes);
        self.fact_lookup.reserve(facts);
        self.incident_offsets.reserve(nodes);
    }

    fn apply(&mut self, event: &StoredEvent<MemoryEvent>) -> Result<()> {
        if event.metadata.event_type != event.payload.event_type() {
            return Err(MemoryError::InvalidValue {
                field: "event_type",
                reason: "must match the memory event payload",
            });
        }
        match &event.payload {
            MemoryEvent::NodeUpserted { node } => self.insert_node(NodeRevision {
                node: node.clone(),
                recorded_at: event.metadata.recorded_at,
                position: event.metadata.global_position,
            })?,
            MemoryEvent::FactRecorded { fact } => {
                if fact.recorded_at != event.metadata.recorded_at
                    || fact.agent_id != event.metadata.agent_id
                    || fact.session_id != event.metadata.session_id
                {
                    return Err(MemoryError::InvalidValue {
                        field: "fact.envelope",
                        reason: "fact provenance must match its event envelope",
                    });
                }
                self.insert_fact(fact.clone())?;
            }
            MemoryEvent::FactRetracted {
                fact_id,
                valid_until,
                evidence,
            } => {
                self.apply_retraction(event.metadata.recorded_at, fact_id, *valid_until, evidence)?;
            }
        }
        self.last_global_position = Some(event.metadata.global_position);
        Ok(())
    }

    fn apply_owned(&mut self, event: StoredEvent<MemoryEvent>) -> Result<()> {
        let StoredEvent { metadata, payload } = event;
        if metadata.event_type != payload.event_type() {
            return Err(MemoryError::InvalidValue {
                field: "event_type",
                reason: "must match the memory event payload",
            });
        }
        match payload {
            MemoryEvent::NodeUpserted { node } => self.insert_node(NodeRevision {
                node,
                recorded_at: metadata.recorded_at,
                position: metadata.global_position,
            })?,
            MemoryEvent::FactRecorded { fact } => {
                if fact.recorded_at != metadata.recorded_at
                    || fact.agent_id != metadata.agent_id
                    || fact.session_id != metadata.session_id
                {
                    return Err(MemoryError::InvalidValue {
                        field: "fact.envelope",
                        reason: "fact provenance must match its event envelope",
                    });
                }
                self.insert_fact(fact)?;
            }
            MemoryEvent::FactRetracted {
                fact_id,
                valid_until,
                evidence,
            } => {
                self.apply_retraction(metadata.recorded_at, &fact_id, valid_until, &evidence)?;
            }
        }
        self.last_global_position = Some(metadata.global_position);
        Ok(())
    }
}
