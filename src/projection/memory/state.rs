use super::index::IdIndex;
use crate::{
    domain::{MemoryFact, MemoryNode},
    error::{MemoryError, Result},
    id::{EntityId, FactId},
    time::Timestamp,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct NodeRevision {
    pub(crate) node: MemoryNode,
    pub(crate) recorded_at: Timestamp,
    pub(crate) position: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct NodeHistory {
    pub(crate) first: NodeRevision,
    pub(crate) later: Vec<NodeRevision>,
}

impl NodeHistory {
    pub(super) const fn new(first: NodeRevision) -> Self {
        Self {
            first,
            later: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Supersession {
    pub(crate) replacement: FactId,
    pub(crate) valid_from: Timestamp,
    pub(crate) recorded_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Retraction {
    pub(crate) valid_until: Timestamp,
    pub(crate) recorded_at: Timestamp,
}

#[derive(Debug, Clone)]
pub struct MemoryProjection {
    pub(crate) nodes: Vec<NodeHistory>,
    pub(crate) facts: Vec<MemoryFact>,
    pub(crate) node_lookup: IdIndex<EntityId>,
    pub(crate) fact_lookup: IdIndex<FactId>,
    pub(crate) incident_offsets: Vec<usize>,
    pub(crate) incident_facts: Vec<usize>,
    pub(crate) incident_delta: HashMap<usize, Vec<usize>>,
    pub(crate) supersessions: BTreeMap<FactId, Supersession>,
    pub(crate) retractions: BTreeMap<FactId, Retraction>,
    pub(crate) last_global_position: Option<u64>,
}

impl MemoryProjection {
    pub(super) fn with_capacity(nodes: usize, facts: usize) -> Self {
        Self {
            nodes: Vec::with_capacity(nodes),
            facts: Vec::with_capacity(facts),
            node_lookup: IdIndex::with_capacity(nodes),
            fact_lookup: IdIndex::with_capacity(facts),
            incident_offsets: vec![0],
            incident_facts: Vec::with_capacity(facts.saturating_mul(2)),
            incident_delta: HashMap::new(),
            supersessions: BTreeMap::new(),
            retractions: BTreeMap::new(),
            last_global_position: None,
        }
    }

    pub(crate) fn rebuild_indexes(&mut self) -> Result<()> {
        self.node_lookup.reserve(self.nodes.len());
        for (index, revisions) in self.nodes.iter().enumerate() {
            for revision in core::iter::once(&revisions.first).chain(&revisions.later) {
                revision.node.validate()?;
                if revision.node.id != revisions.first.node.id {
                    return Err(MemoryError::InvalidValue {
                        field: "projection.nodes",
                        reason: "revision identifiers must match",
                    });
                }
            }
            if self
                .node_lookup
                .insert(revisions.first.node.id.clone(), index)
                .is_some()
            {
                return Err(MemoryError::InvalidValue {
                    field: "projection.nodes",
                    reason: "node identifiers must be unique",
                });
            }
        }
        self.fact_lookup.reserve(self.facts.len());
        let mut endpoints = Vec::with_capacity(self.facts.len());
        for (index, fact) in self.facts.iter().enumerate() {
            fact.validate()?;
            let source = self.entity_index(&fact.source)?;
            let target = self.entity_index(&fact.target)?;
            if self.fact_lookup.insert(fact.id.clone(), index).is_some() {
                return Err(MemoryError::ConflictingFact {
                    id: fact.id.to_string(),
                });
            }
            endpoints.push((source, target));
        }
        self.set_incidents(&endpoints)?;
        self.validate_changes()
    }

    pub(super) fn set_incidents(&mut self, endpoints: &[(usize, usize)]) -> Result<()> {
        let mut offsets = vec![0_usize; self.nodes.len() + 1];
        for &(source, target) in endpoints {
            offsets[source + 1] = offsets[source + 1]
                .checked_add(1)
                .ok_or(MemoryError::CapacityOverflow)?;
            if target != source {
                offsets[target + 1] = offsets[target + 1]
                    .checked_add(1)
                    .ok_or(MemoryError::CapacityOverflow)?;
            }
        }
        for index in 1..offsets.len() {
            offsets[index] = offsets[index]
                .checked_add(offsets[index - 1])
                .ok_or(MemoryError::CapacityOverflow)?;
        }
        let mut cursor = offsets[..self.nodes.len()].to_vec();
        let mut incidents = vec![0_usize; *offsets.last().unwrap_or(&0)];
        for (fact, &(source, target)) in endpoints.iter().enumerate() {
            incidents[cursor[source]] = fact;
            cursor[source] += 1;
            if target != source {
                incidents[cursor[target]] = fact;
                cursor[target] += 1;
            }
        }
        self.incident_offsets = offsets;
        self.incident_facts = incidents;
        self.incident_delta.clear();
        Ok(())
    }

    fn entity_index(&self, id: &EntityId) -> Result<usize> {
        self.node_lookup
            .get(id)
            .copied()
            .ok_or_else(|| MemoryError::MissingEntity { id: id.to_string() })
    }

    fn validate_changes(&self) -> Result<()> {
        for (prior, change) in &self.supersessions {
            let prior_fact = self
                .fact_lookup
                .get(prior)
                .map(|index| &self.facts[*index])
                .ok_or_else(|| MemoryError::MissingFact {
                    id: prior.to_string(),
                })?;
            let replacement = self
                .fact_lookup
                .get(&change.replacement)
                .map(|index| &self.facts[*index])
                .ok_or_else(|| MemoryError::MissingFact {
                    id: change.replacement.to_string(),
                })?;
            if replacement.supersedes.as_ref() != Some(&prior_fact.id)
                || replacement.valid_from != change.valid_from
                || replacement.recorded_at != change.recorded_at
            {
                return Err(MemoryError::InvalidValue {
                    field: "projection.supersessions",
                    reason: "supersession index disagrees with replacement fact",
                });
            }
        }
        for (fact_id, change) in &self.retractions {
            let fact = self
                .fact_lookup
                .get(fact_id)
                .map(|index| &self.facts[*index])
                .ok_or_else(|| MemoryError::MissingFact {
                    id: fact_id.to_string(),
                })?;
            if change.valid_until <= fact.valid_from {
                return Err(MemoryError::InvalidValue {
                    field: "projection.retractions",
                    reason: "retraction must follow fact validity",
                });
            }
        }
        Ok(())
    }
}

impl Default for MemoryProjection {
    fn default() -> Self {
        Self::with_capacity(0, 0)
    }
}

impl PartialEq for MemoryProjection {
    fn eq(&self, other: &Self) -> bool {
        self.nodes == other.nodes
            && self.facts == other.facts
            && self.supersessions == other.supersessions
            && self.retractions == other.retractions
            && self.last_global_position == other.last_global_position
    }
}

impl Eq for MemoryProjection {}

#[derive(Serialize)]
struct ProjectionRef<'a> {
    nodes: &'a [NodeHistory],
    facts: &'a [MemoryFact],
    supersessions: &'a BTreeMap<FactId, Supersession>,
    retractions: &'a BTreeMap<FactId, Retraction>,
    last_global_position: Option<u64>,
}

#[derive(Deserialize)]
struct ProjectionData {
    nodes: Vec<NodeHistory>,
    facts: Vec<MemoryFact>,
    supersessions: BTreeMap<FactId, Supersession>,
    retractions: BTreeMap<FactId, Retraction>,
    last_global_position: Option<u64>,
}

impl Serialize for MemoryProjection {
    fn serialize<S>(&self, serializer: S) -> core::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        ProjectionRef {
            nodes: &self.nodes,
            facts: &self.facts,
            supersessions: &self.supersessions,
            retractions: &self.retractions,
            last_global_position: self.last_global_position,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for MemoryProjection {
    fn deserialize<D>(deserializer: D) -> core::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data = ProjectionData::deserialize(deserializer)?;
        let node_count = data.nodes.len();
        let fact_count = data.facts.len();
        let mut projection = Self {
            nodes: data.nodes,
            facts: data.facts,
            node_lookup: IdIndex::with_capacity(node_count),
            fact_lookup: IdIndex::with_capacity(fact_count),
            incident_offsets: Vec::new(),
            incident_facts: Vec::new(),
            incident_delta: HashMap::new(),
            supersessions: data.supersessions,
            retractions: data.retractions,
            last_global_position: data.last_global_position,
        };
        projection.rebuild_indexes().map_err(D::Error::custom)?;
        Ok(projection)
    }
}
