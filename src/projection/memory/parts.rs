use super::{
    MemoryProjection,
    state::{NodeHistory, NodeRevision},
};
use crate::{
    domain::{MemoryFact, MemoryNode},
    error::{MemoryError, Result},
    time::Timestamp,
};
use std::thread;

#[derive(Clone, Copy)]
struct PreparedFact {
    source: usize,
    target: usize,
    id_hash: u64,
}

impl MemoryProjection {
    /// Builds a current projection from already extracted nodes and facts.
    ///
    /// This bypasses event-envelope replay but retains domain, endpoint,
    /// uniqueness, evidence, and supersession validation. All nodes are treated
    /// as revisions recorded at `known_at`. Large fact sets are prepared in
    /// parallel using scoped standard-library threads.
    ///
    /// # Errors
    ///
    /// Rejects duplicate identifiers, invalid nodes or facts, missing
    /// endpoints, facts recorded after `known_at`, and invalid supersession
    /// chains.
    pub fn try_from_parts(
        nodes: Vec<MemoryNode>,
        facts: Vec<MemoryFact>,
        known_at: Timestamp,
        source_position: Option<u64>,
    ) -> Result<Self> {
        let mut projection = Self::with_capacity(nodes.len(), facts.len());
        for (position, node) in nodes.into_iter().enumerate() {
            node.validate()?;
            let index = projection.nodes.len();
            if projection
                .node_lookup
                .insert(node.id.clone(), index)
                .is_some()
            {
                return Err(MemoryError::InvalidValue {
                    field: "nodes",
                    reason: "node identifiers must be unique",
                });
            }
            projection.nodes.push(NodeHistory::new(NodeRevision {
                node,
                recorded_at: known_at,
                position: u64::try_from(position).map_err(|_| MemoryError::CapacityOverflow)?,
            }));
        }
        let prepared = prepare_facts(&projection, &facts, known_at)?;
        for (index, (fact, prepared)) in facts.iter().zip(&prepared).enumerate() {
            if projection
                .fact_lookup
                .insert_hashed(fact.id.clone(), index, prepared.id_hash)
                .is_some()
            {
                return Err(MemoryError::ConflictingFact {
                    id: fact.id.to_string(),
                });
            }
        }
        for fact in &facts {
            if let Some(prior) = &fact.supersedes {
                projection.apply_supersession(prior, fact)?;
            }
        }
        let endpoints = prepared
            .into_iter()
            .map(|fact| (fact.source, fact.target))
            .collect::<Vec<_>>();
        projection.facts = facts;
        projection.set_incidents(&endpoints)?;
        projection.last_global_position = source_position;
        Ok(projection)
    }
}

fn prepare_facts(
    projection: &MemoryProjection,
    facts: &[MemoryFact],
    known_at: Timestamp,
) -> Result<Vec<PreparedFact>> {
    let parallelism = thread::available_parallelism().map_or(1, usize::from);
    let workers = parallelism.min(facts.len().div_ceil(16_384)).max(1);
    if workers == 1 {
        return facts
            .iter()
            .map(|fact| prepare_fact(projection, fact, known_at))
            .collect();
    }
    let chunk_size = facts.len().div_ceil(workers);
    thread::scope(|scope| {
        let handles = facts
            .chunks(chunk_size)
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|fact| prepare_fact(projection, fact, known_at))
                        .collect::<Result<Vec<_>>>()
                })
            })
            .collect::<Vec<_>>();
        let mut prepared = Vec::with_capacity(facts.len());
        for handle in handles {
            let mut chunk = handle.join().map_err(|_| MemoryError::InvalidValue {
                field: "facts",
                reason: "parallel fact preparation panicked",
            })??;
            prepared.append(&mut chunk);
        }
        Ok(prepared)
    })
}

fn prepare_fact(
    projection: &MemoryProjection,
    fact: &MemoryFact,
    known_at: Timestamp,
) -> Result<PreparedFact> {
    if fact.recorded_at > known_at {
        return Err(MemoryError::InvalidValue {
            field: "facts",
            reason: "fact recorded_at must not exceed known_at",
        });
    }
    fact.validate()?;
    Ok(PreparedFact {
        source: projection.require_entity(&fact.source)?,
        target: projection.require_entity(&fact.target)?,
        id_hash: projection.fact_lookup.hash(&fact.id),
    })
}
