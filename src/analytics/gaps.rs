use super::{GapKind, GapReport, MemoryAnalytics, ReasoningGap, ReasoningGapRequest};
use crate::{EntityId, FactId, MemoryError, MemoryProjection, Result, project_graph};
use std::collections::BTreeMap;

impl MemoryAnalytics {
    /// Finds evidence, support, stability, and freshness gaps.
    ///
    /// Downstream impact is computed on the canonical directed graph and all
    /// scores use deterministic basis points.
    ///
    /// # Errors
    ///
    /// Rejects invalid thresholds or graph projection failures.
    pub fn reasoning_gaps(
        projection: &MemoryProjection,
        request: ReasoningGapRequest,
    ) -> Result<GapReport> {
        validate_request(request)?;
        let view = projection.view(request.clock);
        let graph = project_graph(&view)?;
        let mut gaps = Vec::new();
        let support_counts = support_counts(&view.facts);
        for node in &view.nodes {
            let supports = support_counts.get(&node.id).copied().unwrap_or(0);
            if node.kind.eq_ignore_ascii_case("decision") && supports == 0 {
                gaps.push(gap(
                    &graph,
                    &node.id,
                    None,
                    GapKind::UnjustifiedDecision,
                    10_000,
                    "decision has no incoming support evidence",
                ));
            } else if node.kind.eq_ignore_ascii_case("inference")
                && supports < request.minimum_supports
            {
                let severity = support_severity(supports, request.minimum_supports);
                gaps.push(gap(
                    &graph,
                    &node.id,
                    None,
                    GapKind::SingleSourceInference,
                    severity,
                    "inference has fewer independent supports than required",
                ));
            }
        }
        add_fact_gaps(&mut gaps, &graph, &view.facts, request);
        add_unstable_gaps(&mut gaps, &graph, projection, request);
        gaps.sort_by(|left, right| {
            right
                .severity_bps
                .cmp(&left.severity_bps)
                .then_with(|| right.downstream_entities.cmp(&left.downstream_entities))
                .then_with(|| left.entity.cmp(&right.entity))
                .then_with(|| left.kind.cmp(&right.kind))
                .then_with(|| left.fact.cmp(&right.fact))
        });
        let total = gaps.len();
        gaps.truncate(request.max_results);
        let analyzed_entities = view.nodes.len();
        let health_bps = health(total, analyzed_entities);
        Ok(GapReport {
            gaps,
            health_bps,
            analyzed_entities,
            analyzed_facts: view.facts.len(),
        })
    }
}

fn validate_request(request: ReasoningGapRequest) -> Result<()> {
    if request.minimum_supports == 0
        || request.low_confidence_bps > 10_000
        || request.unstable_revision_count < 2
        || request.stale_after_micros < 0
        || request.max_results == 0
    {
        return Err(MemoryError::InvalidValue {
            field: "reasoning_gap",
            reason: "thresholds and limits must be in their documented ranges",
        });
    }
    Ok(())
}

fn support_counts(facts: &[crate::MemoryFact]) -> BTreeMap<EntityId, usize> {
    let mut counts = BTreeMap::new();
    for fact in facts {
        if matches!(
            fact.relation.to_ascii_lowercase().as_str(),
            "supports" | "supported_by" | "caused_by"
        ) {
            *counts.entry(fact.target.clone()).or_insert(0) += 1;
        }
    }
    counts
}

fn add_fact_gaps(
    gaps: &mut Vec<ReasoningGap>,
    graph: &weavatrix_graph::Graph,
    facts: &[crate::MemoryFact],
    request: ReasoningGapRequest,
) {
    for fact in facts {
        let confidence = fact.confidence.basis_points();
        if confidence < request.low_confidence_bps {
            gaps.push(gap(
                graph,
                &fact.target,
                Some(fact.id.clone()),
                GapKind::LowConfidenceFoundation,
                10_000 - confidence,
                "active fact confidence is below the requested floor",
            ));
        }
        let age = request
            .clock
            .known_at
            .as_unix_micros()
            .saturating_sub(fact.recorded_at.as_unix_micros());
        if age >= request.stale_after_micros {
            let severity = stale_severity(age, request.stale_after_micros);
            gaps.push(gap(
                graph,
                &fact.target,
                Some(fact.id.clone()),
                GapKind::StaleEvidence,
                severity,
                "active fact has not been refreshed within the requested interval",
            ));
        }
    }
}

fn add_unstable_gaps(
    gaps: &mut Vec<ReasoningGap>,
    graph: &weavatrix_graph::Graph,
    projection: &MemoryProjection,
    request: ReasoningGapRequest,
) {
    let mut revisions = BTreeMap::<(EntityId, String), Vec<&crate::MemoryFact>>::new();
    for fact in projection
        .all_facts()
        .iter()
        .filter(|fact| fact.recorded_at <= request.clock.known_at)
    {
        revisions
            .entry((fact.source.clone(), fact.relation.clone()))
            .or_default()
            .push(fact);
    }
    for ((entity, _), mut facts) in revisions {
        if facts.len() < request.unstable_revision_count {
            continue;
        }
        facts.sort_by_key(|fact| (fact.recorded_at, fact.id.clone()));
        let superseding = facts
            .iter()
            .filter(|fact| fact.supersedes.is_some())
            .count();
        if superseding + 1 < request.unstable_revision_count {
            continue;
        }
        gaps.push(gap(
            graph,
            &entity,
            facts.last().map(|fact| fact.id.clone()),
            GapKind::UnstableKnowledge,
            revision_severity(facts.len()),
            "belief has a long explicit supersession history",
        ));
    }
}

fn gap(
    graph: &weavatrix_graph::Graph,
    entity: &EntityId,
    fact: Option<FactId>,
    kind: GapKind,
    severity_bps: u16,
    explanation: &str,
) -> ReasoningGap {
    ReasoningGap {
        entity: entity.clone(),
        fact,
        kind,
        severity_bps,
        downstream_entities: downstream(graph, entity),
        explanation: explanation.to_owned(),
    }
}

fn downstream(graph: &weavatrix_graph::Graph, entity: &EntityId) -> usize {
    graph.node_index(entity.as_str()).map_or(0, |index| {
        weavatrix_graph::bfs(graph, index).len().saturating_sub(1)
    })
}

fn support_severity(actual: usize, required: usize) -> u16 {
    let missing = required.saturating_sub(actual);
    u16::try_from((missing.saturating_mul(10_000) / required).min(10_000)).unwrap_or(10_000)
}

fn stale_severity(age: i64, threshold: i64) -> u16 {
    if threshold == 0 {
        return 10_000;
    }
    let ratio = age.saturating_mul(5_000).saturating_div(threshold);
    u16::try_from(ratio.clamp(1_000, 10_000)).unwrap_or(10_000)
}

fn revision_severity(count: usize) -> u16 {
    u16::try_from(count.saturating_mul(2_000).min(10_000)).unwrap_or(10_000)
}

fn health(gaps: usize, entities: usize) -> u16 {
    if entities == 0 {
        return 10_000;
    }
    let penalty = gaps.saturating_mul(10_000).saturating_div(entities);
    u16::try_from(10_000_usize.saturating_sub(penalty.min(10_000))).unwrap_or(0)
}
