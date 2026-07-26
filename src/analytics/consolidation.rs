use super::{ConsolidationAction, ConsolidationKind, ConsolidationPlan, MemoryAnalytics};
use crate::{EntityId, MemoryError, MemoryProjection, ProjectionClock, Result, project_graph};
use std::collections::BTreeMap;

impl MemoryAnalytics {
    /// Produces a deterministic, non-mutating maintenance plan.
    ///
    /// Event history is never deleted. The caller may translate proposed
    /// duplicate actions into explicit supersession events.
    ///
    /// # Errors
    ///
    /// Rejects a zero action limit or graph projection failures.
    pub fn consolidation_plan(
        projection: &MemoryProjection,
        clock: ProjectionClock,
        max_actions: usize,
    ) -> Result<ConsolidationPlan> {
        if max_actions == 0 {
            return Err(MemoryError::InvalidValue {
                field: "consolidation.max_actions",
                reason: "must be greater than zero",
            });
        }
        let view = projection.view(clock);
        let graph = project_graph(&view)?;
        let mut actions = duplicate_actions(&view.facts);
        actions.extend(orphan_actions(&graph));
        actions.extend(revision_actions(projection, clock));
        actions.sort_by(|left, right| {
            left.kind
                .cmp(&right.kind)
                .then_with(|| left.affected_entities.cmp(&right.affected_entities))
                .then_with(|| left.affected_facts.cmp(&right.affected_facts))
        });
        actions.truncate(max_actions);
        let projected_savings = actions
            .iter()
            .map(|action| match action.kind {
                ConsolidationKind::ReviewOrphan => 0,
                _ => action.affected_facts.len().saturating_sub(1),
            })
            .sum();
        Ok(ConsolidationPlan {
            actions,
            projected_savings,
            source_position: projection.last_global_position(),
        })
    }
}

fn duplicate_actions(facts: &[crate::MemoryFact]) -> Vec<ConsolidationAction> {
    let mut groups = BTreeMap::<(EntityId, String, EntityId), Vec<&crate::MemoryFact>>::new();
    for fact in facts {
        groups
            .entry((
                fact.source.clone(),
                fact.relation.clone(),
                fact.target.clone(),
            ))
            .or_default()
            .push(fact);
    }
    let mut actions = Vec::new();
    for ((source, _, target), mut duplicates) in groups {
        if duplicates.len() < 2 {
            continue;
        }
        duplicates.sort_by(|left, right| {
            right
                .confidence
                .cmp(&left.confidence)
                .then_with(|| right.recorded_at.cmp(&left.recorded_at))
                .then_with(|| left.id.cmp(&right.id))
        });
        actions.push(ConsolidationAction {
            kind: ConsolidationKind::SupersedeDuplicate,
            keep: Some(duplicates[0].id.clone()),
            affected_facts: duplicates.iter().map(|fact| fact.id.clone()).collect(),
            affected_entities: vec![source, target],
            rationale: "same active source, relation, and target; keep strongest evidence"
                .to_owned(),
        });
    }
    actions
}

fn orphan_actions(graph: &weavatrix_graph::Graph) -> Vec<ConsolidationAction> {
    graph
        .nodes()
        .iter()
        .filter_map(|node| {
            let index = graph.node_index(node.id.as_str())?;
            let isolated = graph.in_degree(index) == Some(0) && graph.out_degree(index) == Some(0);
            isolated.then(|| ConsolidationAction {
                kind: ConsolidationKind::ReviewOrphan,
                keep: None,
                affected_facts: Vec::new(),
                affected_entities: EntityId::new(node.id.as_str()).into_iter().collect(),
                rationale: "entity has no active incoming or outgoing evidence".to_owned(),
            })
        })
        .collect()
}

fn revision_actions(
    projection: &MemoryProjection,
    clock: ProjectionClock,
) -> Vec<ConsolidationAction> {
    let mut groups = BTreeMap::<(EntityId, String), Vec<&crate::MemoryFact>>::new();
    for fact in projection
        .all_facts()
        .iter()
        .filter(|fact| fact.recorded_at <= clock.known_at)
    {
        groups
            .entry((fact.source.clone(), fact.relation.clone()))
            .or_default()
            .push(fact);
    }
    groups
        .into_iter()
        .filter_map(|((entity, _), mut facts)| {
            facts.sort_by_key(|fact| (fact.recorded_at, fact.id.clone()));
            let chain = facts
                .iter()
                .filter(|fact| fact.supersedes.is_some())
                .count();
            (chain >= 3).then(|| ConsolidationAction {
                kind: ConsolidationKind::CompactRevisionChain,
                keep: facts.last().map(|fact| fact.id.clone()),
                affected_facts: facts.iter().map(|fact| fact.id.clone()).collect(),
                affected_entities: vec![entity],
                rationale: "retain event history but checkpoint a long revision chain".to_owned(),
            })
        })
        .collect()
}
