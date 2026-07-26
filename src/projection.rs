use crate::{
    EntityId, EventId, FactId, MemoryError, MemoryEvent, MemoryFact, MemoryNode, MemoryView,
    Result, StoredEvent, StreamId, Timestamp,
};
use std::collections::{BTreeMap, BTreeSet};

pub trait Projection<E>: Default {
    /// Applies one validated stored event.
    ///
    /// # Errors
    ///
    /// Returns domain-specific invariant violations.
    fn apply(&mut self, event: &StoredEvent<E>) -> Result<()>;
}

/// Replays a complete, zero-based event sequence after validating its cursors.
///
/// # Errors
///
/// Rejects global-position or per-stream-version gaps and projection errors.
pub fn replay<E, P>(events: &[StoredEvent<E>]) -> Result<P>
where
    P: Projection<E>,
{
    let mut projection = P::default();
    let mut stream_versions = BTreeMap::<StreamId, u64>::new();
    let mut event_ids = BTreeSet::<EventId>::new();
    for (index, event) in events.iter().enumerate() {
        if !event_ids.insert(event.metadata.id.clone()) {
            return Err(MemoryError::DuplicateEvent {
                id: event.metadata.id.to_string(),
            });
        }
        let expected_position = u64::try_from(index).map_err(|_| MemoryError::CapacityOverflow)?;
        if event.metadata.global_position != expected_position {
            return Err(MemoryError::InvalidReplay {
                reason: format!(
                    "global position {}, expected {expected_position}",
                    event.metadata.global_position
                ),
            });
        }
        let expected_version = stream_versions
            .get(&event.metadata.stream_id)
            .map_or(Ok(0), |version| {
                version.checked_add(1).ok_or(MemoryError::CapacityOverflow)
            })?;
        if event.metadata.stream_version != expected_version {
            return Err(MemoryError::InvalidReplay {
                reason: format!(
                    "stream {} version {}, expected {expected_version}",
                    event.metadata.stream_id, event.metadata.stream_version
                ),
            });
        }
        projection.apply(event)?;
        stream_versions.insert(event.metadata.stream_id.clone(), expected_version);
    }
    Ok(projection)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone)]
struct NodeRevision {
    node: MemoryNode,
    recorded_at: Timestamp,
    position: u64,
}

#[derive(Debug, Clone)]
struct Supersession {
    replacement: FactId,
    valid_from: Timestamp,
    recorded_at: Timestamp,
}

#[derive(Debug, Clone)]
struct Retraction {
    valid_until: Timestamp,
    recorded_at: Timestamp,
}

#[derive(Debug, Clone, Default)]
pub struct MemoryProjection {
    nodes: BTreeMap<EntityId, Vec<NodeRevision>>,
    facts: BTreeMap<FactId, MemoryFact>,
    supersessions: BTreeMap<FactId, Supersession>,
    retractions: BTreeMap<FactId, Retraction>,
    last_global_position: Option<u64>,
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

    #[must_use]
    pub fn view(&self, clock: ProjectionClock) -> MemoryView {
        let nodes = self
            .nodes
            .values()
            .filter_map(|revisions| visible_revision(revisions, clock.known_at))
            .cloned()
            .collect::<Vec<_>>();
        let visible_ids = nodes
            .iter()
            .map(|node| node.id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let facts = self
            .facts
            .values()
            .filter(|fact| {
                visible_ids.contains(&fact.source)
                    && visible_ids.contains(&fact.target)
                    && self.fact_is_active(fact, clock)
            })
            .cloned()
            .collect();
        MemoryView { nodes, facts }
    }

    fn fact_is_active(&self, fact: &MemoryFact, clock: ProjectionClock) -> bool {
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

    fn apply_node(&mut self, event: &StoredEvent<MemoryEvent>, node: &MemoryNode) -> Result<()> {
        node.validate()?;
        self.nodes
            .entry(node.id.clone())
            .or_default()
            .push(NodeRevision {
                node: node.clone(),
                recorded_at: event.metadata.recorded_at,
                position: event.metadata.global_position,
            });
        Ok(())
    }

    fn apply_fact(&mut self, event: &StoredEvent<MemoryEvent>, fact: &MemoryFact) -> Result<()> {
        fact.validate()?;
        if fact.recorded_at != event.metadata.recorded_at
            || fact.agent_id != event.metadata.agent_id
            || fact.session_id != event.metadata.session_id
        {
            return Err(MemoryError::InvalidValue {
                field: "fact.envelope",
                reason: "fact provenance must match its event envelope",
            });
        }
        self.require_entity(&fact.source)?;
        self.require_entity(&fact.target)?;
        if self.facts.contains_key(&fact.id) {
            return Err(MemoryError::ConflictingFact {
                id: fact.id.to_string(),
            });
        }
        if let Some(prior) = &fact.supersedes {
            if !self.facts.contains_key(prior) {
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
        }
        self.facts.insert(fact.id.clone(), fact.clone());
        Ok(())
    }

    fn require_entity(&self, id: &EntityId) -> Result<()> {
        if self.nodes.contains_key(id) {
            Ok(())
        } else {
            Err(MemoryError::MissingEntity { id: id.to_string() })
        }
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
            MemoryEvent::NodeUpserted { node } => self.apply_node(event, node)?,
            MemoryEvent::FactRecorded { fact } => self.apply_fact(event, fact)?,
            MemoryEvent::FactRetracted {
                fact_id,
                valid_until,
                evidence,
            } => {
                if !self.facts.contains_key(fact_id) {
                    return Err(MemoryError::MissingFact {
                        id: fact_id.to_string(),
                    });
                }
                if *valid_until <= self.facts[fact_id].valid_from {
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
                        valid_until: *valid_until,
                        recorded_at: event.metadata.recorded_at,
                    },
                );
            }
        }
        self.last_global_position = Some(event.metadata.global_position);
        Ok(())
    }
}

fn visible_revision(revisions: &[NodeRevision], known_at: Timestamp) -> Option<&MemoryNode> {
    revisions
        .iter()
        .filter(|revision| revision.recorded_at <= known_at)
        .max_by_key(|revision| (revision.recorded_at, revision.position))
        .map(|revision| &revision.node)
}
