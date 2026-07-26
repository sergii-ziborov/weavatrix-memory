use super::ContextRequest;
use crate::{EntityId, MemoryError, MemoryView, Result};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use weavatrix_graph::Graph;

pub(super) fn scope_view(view: &MemoryView, request: &ContextRequest) -> MemoryView {
    let nodes = view
        .nodes
        .iter()
        .filter(|node| {
            (request.repositories.is_empty()
                || node
                    .repository
                    .as_ref()
                    .is_some_and(|value| request.repositories.contains(value)))
                && (request.branches.is_empty()
                    || node
                        .branch
                        .as_ref()
                        .is_some_and(|value| request.branches.contains(value)))
        })
        .cloned()
        .collect::<Vec<_>>();
    let ids = nodes
        .iter()
        .map(|node| node.id.clone())
        .collect::<BTreeSet<_>>();
    let facts = view
        .facts
        .iter()
        .filter(|fact| ids.contains(&fact.source) && ids.contains(&fact.target))
        .cloned()
        .collect();
    MemoryView { nodes, facts }
}

pub(super) fn graph_distances(
    graph: &Graph,
    request: &ContextRequest,
) -> Result<BTreeMap<EntityId, usize>> {
    let mut distances = BTreeMap::new();
    let mut queue = VecDeque::new();
    for seed in &request.seeds {
        if graph.node(seed.as_str()).is_none() {
            return Err(MemoryError::MissingEntity {
                id: seed.to_string(),
            });
        }
        distances.entry(seed.clone()).or_insert(0);
        queue.push_back(seed.clone());
    }
    while let Some(current) = queue.pop_front() {
        let depth = distances[&current];
        if depth >= request.max_depth {
            continue;
        }
        let node = graph
            .node(current.as_str())
            .ok_or_else(|| MemoryError::MissingEntity {
                id: current.to_string(),
            })?;
        let neighbors = graph
            .outgoing(&node.id)
            .filter(|edge| relation_allowed(request, edge.kind.as_str()))
            .map(|edge| edge.target.as_str())
            .chain(
                graph
                    .incoming(&node.id)
                    .filter(|edge| relation_allowed(request, edge.kind.as_str()))
                    .map(|edge| edge.source.as_str()),
            )
            .map(EntityId::new)
            .collect::<Result<Vec<_>>>()?;
        for neighbor in neighbors {
            if let std::collections::btree_map::Entry::Vacant(entry) =
                distances.entry(neighbor.clone())
            {
                entry.insert(depth + 1);
                queue.push_back(neighbor);
            }
        }
    }
    Ok(distances)
}

pub(super) fn relation_allowed(request: &ContextRequest, relation: &str) -> bool {
    request.relations.is_empty() || request.relations.contains(relation)
}
