use super::{
    BytesTokenEstimator, ContextBundle, ContextReceipt, ContextRequest, TokenEstimator,
    scope::{node_allowed, relation_allowed},
    token::{fact_tokens, node_tokens},
};
use crate::{
    EntityId, MemoryError, MemoryFact, MemoryNode, MemoryProjection, MemoryView, ProjectionClock,
    Result, project_graph,
};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, VecDeque},
};

#[derive(Debug, Clone)]
pub struct ContextCompiler<T = BytesTokenEstimator> {
    estimator: T,
}

impl<T> ContextCompiler<T>
where
    T: TokenEstimator,
{
    #[must_use]
    pub const fn new(estimator: T) -> Self {
        Self { estimator }
    }

    /// Compiles a deterministic, evidence-carrying graph within a hard budget.
    ///
    /// # Errors
    ///
    /// Returns an error for missing seeds, graph failures, or an undersized
    /// budget that cannot hold the requested seed nodes.
    pub fn compile(
        &self,
        projection: &MemoryProjection,
        request: &ContextRequest,
    ) -> Result<ContextBundle> {
        if request.seeds.is_empty() {
            return Err(MemoryError::InvalidValue {
                field: "context.seeds",
                reason: "at least one seed is required",
            });
        }
        let (nodes_by_id, candidate_facts, distances, excluded_by_scope) =
            neighborhood(projection, request)?;
        let (mut selected_nodes, mut used) = self.select_seeds(request, &nodes_by_id)?;

        let mut candidates = candidate_facts.values().copied().collect::<Vec<_>>();
        candidates.sort_by_key(|fact| {
            (
                distances[&fact.source].min(distances[&fact.target]),
                Reverse(fact.confidence),
                Reverse(fact.recorded_at),
                fact.id.clone(),
            )
        });

        let mut facts = Vec::new();
        let mut omitted = 0;
        for fact in &candidates {
            let missing_nodes = [&fact.source, &fact.target]
                .into_iter()
                .filter(|id| !selected_nodes.contains(*id))
                .collect::<Vec<_>>();
            let node_cost = missing_nodes
                .iter()
                .map(|id| node_tokens(&self.estimator, &nodes_by_id[id]))
                .sum::<usize>();
            let cost = fact_tokens(&self.estimator, fact) + node_cost;
            if used.saturating_add(cost) > request.token_budget {
                omitted += 1;
                continue;
            }
            used += cost;
            for id in missing_nodes {
                selected_nodes.insert(id.clone());
            }
            facts.push((*fact).clone());
        }
        let nodes = selected_nodes
            .iter()
            .map(|id| nodes_by_id[id].clone())
            .collect::<Vec<_>>();
        facts.sort_by(|left, right| left.id.cmp(&right.id));
        let view = MemoryView { nodes, facts };
        let graph = project_graph(&view)?;
        Ok(ContextBundle {
            receipt: ContextReceipt {
                valid_at: request.valid_at,
                known_at: request.known_at,
                source_position: projection.last_global_position(),
                estimator: self.estimator.name().to_owned(),
                token_budget: request.token_budget,
                estimated_tokens: used,
                examined_facts: candidates.len(),
                selected_facts: view.facts.len(),
                omitted_by_budget: omitted,
                excluded_by_scope,
            },
            view,
            graph,
        })
    }

    fn select_seeds(
        &self,
        request: &ContextRequest,
        nodes: &BTreeMap<EntityId, MemoryNode>,
    ) -> Result<(BTreeSet<crate::EntityId>, usize)> {
        let mut selected = BTreeSet::new();
        let mut used = 0;
        let mut seeds = request.seeds.clone();
        seeds.sort();
        seeds.dedup();
        for seed in &seeds {
            let node = nodes.get(seed).ok_or_else(|| MemoryError::MissingEntity {
                id: seed.to_string(),
            })?;
            used += node_tokens(&self.estimator, node);
            selected.insert(seed.clone());
        }
        if used > request.token_budget {
            return Err(MemoryError::BudgetTooSmall {
                required: used,
                available: request.token_budget,
            });
        }
        Ok((selected, used))
    }
}

type Neighborhood<'a> = (
    BTreeMap<EntityId, MemoryNode>,
    BTreeMap<crate::FactId, &'a MemoryFact>,
    BTreeMap<EntityId, usize>,
    usize,
);

fn neighborhood<'a>(
    projection: &'a MemoryProjection,
    request: &ContextRequest,
) -> Result<Neighborhood<'a>> {
    let clock = ProjectionClock::new(request.valid_at, request.known_at);
    let mut nodes = BTreeMap::new();
    let mut facts = BTreeMap::new();
    let mut distances = BTreeMap::new();
    let mut queue = VecDeque::new();
    let mut excluded = BTreeSet::new();
    let mut seeds = request.seeds.clone();
    seeds.sort();
    seeds.dedup();
    for seed in seeds {
        let node = scoped_node(projection, request, &seed)?;
        nodes.insert(seed.clone(), node.clone());
        distances.insert(seed.clone(), 0);
        queue.push_back(seed);
    }
    while let Some(current) = queue.pop_front() {
        let depth = distances[&current];
        for fact_id in projection.incident_fact_ids(&current) {
            let fact = projection.fact(fact_id).expect("projection index is valid");
            if !projection.fact_is_active(fact, clock) || !relation_allowed(request, &fact.relation)
            {
                continue;
            }
            let other = if fact.source == current {
                &fact.target
            } else {
                &fact.source
            };
            let Some(other_node) = projection.visible_node(other, request.known_at) else {
                continue;
            };
            if !node_allowed(request, other_node) {
                excluded.insert(fact.id.clone());
                continue;
            }
            if !distances.contains_key(other) {
                if depth >= request.max_depth {
                    continue;
                }
                distances.insert(other.clone(), depth + 1);
                nodes.insert(other.clone(), other_node.clone());
                queue.push_back(other.clone());
            }
            facts.insert(fact.id.clone(), fact);
        }
    }
    facts.retain(|_, fact| {
        distances.contains_key(&fact.source) && distances.contains_key(&fact.target)
    });
    Ok((nodes, facts, distances, excluded.len()))
}

fn scoped_node<'a>(
    projection: &'a MemoryProjection,
    request: &ContextRequest,
    id: &EntityId,
) -> Result<&'a MemoryNode> {
    projection
        .visible_node(id, request.known_at)
        .filter(|node| node_allowed(request, node))
        .ok_or_else(|| MemoryError::MissingEntity { id: id.to_string() })
}

impl Default for ContextCompiler<BytesTokenEstimator> {
    fn default() -> Self {
        Self::new(BytesTokenEstimator::default())
    }
}
