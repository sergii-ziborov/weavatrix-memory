use super::{
    BytesTokenEstimator, ContextBundle, ContextReceipt, ContextRequest, TokenEstimator,
    scope::{graph_distances, relation_allowed, scope_view},
    token::{fact_tokens, node_tokens},
};
use crate::{MemoryError, MemoryProjection, MemoryView, ProjectionClock, Result, project_graph};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet},
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
        let active = projection.view(ProjectionClock::new(request.valid_at, request.known_at));
        let scoped = scope_view(&active, request);
        let excluded_by_scope = active.facts.len().saturating_sub(scoped.facts.len());
        let graph = project_graph(&scoped)?;
        let distances = graph_distances(&graph, request)?;
        let nodes_by_id = scoped
            .nodes
            .iter()
            .map(|node| (node.id.clone(), node))
            .collect::<BTreeMap<_, _>>();
        let (mut selected_nodes, mut used) = self.select_seeds(request, &nodes_by_id)?;

        let mut candidates = scoped
            .facts
            .iter()
            .filter(|fact| {
                distances.contains_key(&fact.source)
                    && distances.contains_key(&fact.target)
                    && relation_allowed(request, &fact.relation)
            })
            .collect::<Vec<_>>();
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
                .map(|id| node_tokens(&self.estimator, nodes_by_id[id]))
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
            .map(|id| (*nodes_by_id[id]).clone())
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
        nodes: &BTreeMap<crate::EntityId, &crate::MemoryNode>,
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

impl Default for ContextCompiler<BytesTokenEstimator> {
    fn default() -> Self {
        Self::new(BytesTokenEstimator::default())
    }
}
