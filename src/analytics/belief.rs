use super::{
    BeliefRevisionReport, BeliefRevisionRequest, CascadeEffect, Contradiction, MemoryAnalytics,
};
use crate::{EntityId, MemoryProjection, Result, project_graph};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

impl MemoryAnalytics {
    /// Evaluates a structured hypothesis without mutating recorded memory.
    ///
    /// Exact competing targets and explicit `contradicts` facts start a
    /// confidence cascade over the canonical Weavatrix graph.
    ///
    /// # Errors
    ///
    /// Rejects an invalid hypothesis or graph projection.
    pub fn belief_revision(
        projection: &MemoryProjection,
        request: &BeliefRevisionRequest,
    ) -> Result<BeliefRevisionReport> {
        request.hypothesis.validate()?;
        let view = projection.view(request.clock);
        let graph = project_graph(&view)?;
        let mut contradictions = view
            .facts
            .iter()
            .filter_map(|fact| contradiction(fact, &request.hypothesis))
            .collect::<Vec<_>>();
        contradictions.sort_by(|left, right| left.fact.cmp(&right.fact));
        let roots = contradictions
            .iter()
            .filter_map(|item| {
                view.facts
                    .iter()
                    .find(|fact| fact.id == item.fact)
                    .map(|fact| fact.target.clone())
            })
            .collect::<BTreeSet<_>>();
        let cascade = cascade(
            &graph,
            roots,
            request.max_depth,
            request.hypothesis.confidence.basis_points(),
        );
        let kinds = view
            .nodes
            .iter()
            .map(|node| (node.id.clone(), node.kind.as_str()))
            .collect::<BTreeMap<_, _>>();
        let invalidated_decisions = cascade
            .iter()
            .filter(|effect| {
                kinds
                    .get(&effect.entity)
                    .is_some_and(|kind| kind.eq_ignore_ascii_case("decision"))
            })
            .map(|effect| effect.entity.clone())
            .collect();
        Ok(BeliefRevisionReport {
            contradictions,
            cascade,
            invalidated_decisions,
        })
    }
}

fn contradiction(
    fact: &crate::MemoryFact,
    hypothesis: &crate::MemoryFact,
) -> Option<Contradiction> {
    let competing = fact.source == hypothesis.source
        && fact.relation == hypothesis.relation
        && fact.target != hypothesis.target;
    let explicit = fact.relation.eq_ignore_ascii_case("contradicts")
        && ((fact.source == hypothesis.source && fact.target == hypothesis.target)
            || (fact.source == hypothesis.target && fact.target == hypothesis.source));
    let corrected = hypothesis.supersedes.as_ref() == Some(&fact.id);
    if !(competing || explicit || corrected) {
        return None;
    }
    let reason = if explicit {
        "explicit contradicts relation"
    } else if corrected {
        "hypothesis explicitly supersedes this fact"
    } else {
        "same source and relation assert a different target"
    };
    Some(Contradiction {
        fact: fact.id.clone(),
        strength_bps: fact
            .confidence
            .basis_points()
            .min(hypothesis.confidence.basis_points()),
        reason: reason.to_owned(),
    })
}

fn cascade(
    graph: &weavatrix_graph::Graph,
    roots: BTreeSet<EntityId>,
    max_depth: usize,
    hypothesis_confidence: u16,
) -> Vec<CascadeEffect> {
    let mut distances = BTreeMap::<EntityId, usize>::new();
    let mut queue = VecDeque::new();
    for root in roots {
        distances.insert(root.clone(), 0);
        queue.push_back(root);
    }
    while let Some(entity) = queue.pop_front() {
        let depth = distances[&entity];
        if depth >= max_depth {
            continue;
        }
        let Some(index) = graph.node_index(entity.as_str()) else {
            continue;
        };
        for neighbor in graph.outgoing_neighbors_at(index) {
            let Some(node) = graph.node_at(neighbor) else {
                continue;
            };
            let Ok(next) = EntityId::new(node.id.as_str()) else {
                continue;
            };
            if !distances.contains_key(&next) {
                distances.insert(next.clone(), depth + 1);
                queue.push_back(next);
            }
        }
    }
    distances
        .into_iter()
        .map(|(entity, depth)| CascadeEffect {
            entity,
            depth,
            revised_confidence_bps: revised_confidence(hypothesis_confidence, depth),
        })
        .collect()
}

fn revised_confidence(confidence: u16, depth: usize) -> u16 {
    let divisor = u32::try_from(depth + 2).unwrap_or(u32::MAX);
    let weakening = u32::from(confidence) / divisor;
    u16::try_from(10_000_u32.saturating_sub(weakening)).unwrap_or(0)
}
