mod binary;
mod index;
mod parts;
mod state;

use super::Projection;
use crate::{
    EntityId, FactId, MemoryError, MemoryEvent, MemoryFact, MemoryNode, MemoryView, Result,
    StoredEvent, Timestamp,
};
use serde::{Deserialize, Serialize};
use state::{NodeHistory, NodeRevision, Retraction, Supersession};
use std::collections::BTreeSet;

pub use binary::CompactSnapshotCodec;
pub use state::MemoryProjection;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionClock {
    pub valid_at: Timestamp,
    pub known_at: Timestamp,
}

impl ProjectionClock {
    #[must_use]
    pub const fn new(valid_at: Timestamp, known_at: Timestamp) -> Self {
        Self { valid_at, known_at }
    }
}

impl MemoryProjection {
    #[must_use]
    pub const fn last_global_position(&self) -> Option<u64> {
        self.last_global_position
    }

    #[must_use]
    pub fn superseded_by(&self, fact: &FactId) -> Option<&FactId> {
        self.supersessions
            .get(fact)
            .map(|change| &change.replacement)
    }

    pub(crate) fn visible_node(&self, id: &EntityId, known_at: Timestamp) -> Option<&MemoryNode> {
        self.node_lookup
            .get(id)
            .and_then(|index| visible_revision(&self.nodes[*index], known_at))
    }

    pub(crate) fn fact(&self, id: &FactId) -> Option<&MemoryFact> {
        self.fact_lookup.get(id).map(|index| &self.facts[*index])
    }

    pub(crate) fn all_facts(&self) -> &[MemoryFact] {
        &self.facts
    }

    pub(crate) fn incident_fact_ids(&self, id: &EntityId) -> impl Iterator<Item = &FactId> + '_ {
        self.node_lookup
            .get(id)
            .into_iter()
            .flat_map(|index| {
                let stable = &self.incident_facts
                    [self.incident_offsets[*index]..self.incident_offsets[*index + 1]];
                stable
                    .iter()
                    .chain(self.incident_delta.get(index).into_iter().flatten())
            })
            .map(|index| &self.facts[*index].id)
    }

    #[must_use]
    pub fn view(&self, clock: ProjectionClock) -> MemoryView {
        let nodes = self
            .nodes
            .iter()
            .filter_map(|revisions| visible_revision(revisions, clock.known_at))
            .cloned()
            .collect::<Vec<_>>();
        let visible_ids = nodes
            .iter()
            .map(|node| node.id.clone())
            .collect::<BTreeSet<_>>();
        let facts = self
            .facts
            .iter()
            .filter(|fact| {
                visible_ids.contains(&fact.source)
                    && visible_ids.contains(&fact.target)
                    && self.fact_is_active(fact, clock)
            })
            .cloned()
            .collect();
        MemoryView { nodes, facts }
    }

    pub(crate) fn fact_is_active(&self, fact: &MemoryFact, clock: ProjectionClock) -> bool {
        if fact.recorded_at > clock.known_at || fact.valid_from > clock.valid_at {
            return false;
        }
        if fact
            .valid_until
            .is_some_and(|until| clock.valid_at >= until)
        {
            return false;
        }
        if self.supersessions.get(&fact.id).is_some_and(|change| {
            change.recorded_at <= clock.known_at && change.valid_from <= clock.valid_at
        }) {
            return false;
        }
        !self.retractions.get(&fact.id).is_some_and(|change| {
            change.recorded_at <= clock.known_at && change.valid_until <= clock.valid_at
        })
    }

    fn insert_node(&mut self, revision: NodeRevision) -> Result<()> {
        revision.node.validate()?;
        if let Some(index) = self.node_lookup.get(&revision.node.id).copied() {
            self.nodes[index].later.push(revision);
        } else {
            let index = self.nodes.len();
            self.node_lookup.insert(revision.node.id.clone(), index);
            self.nodes.push(NodeHistory::new(revision));
            let offset = *self.incident_offsets.last().unwrap_or(&0);
            self.incident_offsets.push(offset);
        }
        Ok(())
    }

    fn insert_fact(&mut self, fact: MemoryFact) -> Result<()> {
        fact.validate()?;
        let source = self.require_entity(&fact.source)?;
        let target = self.require_entity(&fact.target)?;
        if self.fact_lookup.contains_key(&fact.id) {
            return Err(MemoryError::ConflictingFact {
                id: fact.id.to_string(),
            });
        }
        if let Some(prior) = &fact.supersedes {
            self.apply_supersession(prior, &fact)?;
        }
        let index = self.facts.len();
        self.fact_lookup.insert(fact.id.clone(), index);
        self.incident_delta.entry(source).or_default().push(index);
        if target != source {
            self.incident_delta.entry(target).or_default().push(index);
        }
        self.facts.push(fact);
        Ok(())
    }

    fn apply_supersession(&mut self, prior: &FactId, fact: &MemoryFact) -> Result<()> {
        if !self.fact_lookup.contains_key(prior) {
            return Err(MemoryError::MissingFact {
                id: prior.to_string(),
            });
        }
        if self.supersessions.contains_key(prior) {
            return Err(MemoryError::ConflictingFact {
                id: prior.to_string(),
            });
        }
        self.supersessions.insert(
            prior.clone(),
            Supersession {
                replacement: fact.id.clone(),
                valid_from: fact.valid_from,
                recorded_at: fact.recorded_at,
            },
        );
        Ok(())
    }

    fn require_entity(&self, id: &EntityId) -> Result<usize> {
        self.node_lookup
            .get(id)
            .copied()
            .ok_or_else(|| MemoryError::MissingEntity { id: id.to_string() })
    }

    fn apply_retraction(
        &mut self,
        event: &StoredEvent<MemoryEvent>,
        fact_id: &FactId,
        valid_until: Timestamp,
        evidence: &[crate::Evidence],
    ) -> Result<()> {
        let fact = self.fact(fact_id).ok_or_else(|| MemoryError::MissingFact {
            id: fact_id.to_string(),
        })?;
        if valid_until <= fact.valid_from {
            return Err(MemoryError::InvalidValue {
                field: "retraction.valid_until",
                reason: "must be later than the fact valid_from",
            });
        }
        if evidence.is_empty() {
            return Err(MemoryError::InvalidValue {
                field: "retraction.evidence",
                reason: "at least one evidence item is required",
            });
        }
        evidence.iter().try_for_each(crate::Evidence::validate)?;
        self.retractions.insert(
            fact_id.clone(),
            Retraction {
                valid_until,
                recorded_at: event.metadata.recorded_at,
            },
        );
        Ok(())
    }
}

impl Projection<MemoryEvent> for MemoryProjection {
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
            } => self.apply_retraction(event, fact_id, *valid_until, evidence)?,
        }
        self.last_global_position = Some(event.metadata.global_position);
        Ok(())
    }
}

fn visible_revision(history: &NodeHistory, known_at: Timestamp) -> Option<&MemoryNode> {
    core::iter::once(&history.first)
        .chain(&history.later)
        .filter(|revision| revision.recorded_at <= known_at)
        .max_by_key(|revision| (revision.recorded_at, revision.position))
        .map(|revision| &revision.node)
}
