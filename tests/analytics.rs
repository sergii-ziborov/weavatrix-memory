mod common;

use common::{entity, event, fact, node, ts};
use weavatrix_memory::{
    BeliefRevisionRequest, Confidence, ConsolidationKind, EventStore, ExpectedVersion, GapKind,
    InMemoryStore, MemoryAnalytics, MemoryEvent, MemoryProjection, ProjectionClock,
    ReasoningGapRequest, StreamId, replay,
};

fn projection() -> MemoryProjection {
    let nodes = [
        node("belief:subject", "observation", "DateHour policy"),
        node("value:old", "observation", "Timestamp only"),
        node("value:new", "observation", "Projection plus DateHour"),
        node("decision:1", "decision", "Keep DateHour"),
        node("inference:1", "inference", "One-day precision"),
        node("orphan:1", "observation", "Unlinked note"),
    ];
    let mut low = fact("fact:low", "inference:1", "depends_on", "value:old", 1, 1);
    low.confidence = Confidence::from_basis_points(1_000).unwrap();
    let r1 = fact("fact:r1", "belief:subject", "policy", "value:old", 1, 1);
    let r2 = fact("fact:r2", "belief:subject", "policy", "value:new", 2, 2)
        .supersedes(r1.id.clone())
        .unwrap();
    let r3 = fact("fact:r3", "belief:subject", "policy", "value:old", 3, 3)
        .supersedes(r2.id.clone())
        .unwrap();
    let r4 = fact("fact:r4", "belief:subject", "policy", "value:new", 4, 4)
        .supersedes(r3.id.clone())
        .unwrap();
    let facts = vec![
        fact("fact:belief", "belief:subject", "state", "value:old", 1, 1),
        fact(
            "fact:support-decision",
            "value:old",
            "supports",
            "decision:1",
            1,
            1,
        ),
        fact(
            "fact:support-inference",
            "value:old",
            "supports",
            "inference:1",
            1,
            1,
        ),
        low,
        fact(
            "fact:duplicate-1",
            "belief:subject",
            "duplicates",
            "value:old",
            1,
            1,
        ),
        fact(
            "fact:duplicate-2",
            "belief:subject",
            "duplicates",
            "value:old",
            1,
            2,
        ),
        r1,
        r2,
        r3,
        r4,
    ];
    let mut pending = nodes
        .into_iter()
        .enumerate()
        .map(|(index, node)| {
            event(
                &format!("event:node:{index}"),
                0,
                MemoryEvent::NodeUpserted { node },
            )
        })
        .collect::<Vec<_>>();
    pending.extend(facts.into_iter().enumerate().map(|(index, fact)| {
        event(
            &format!("event:fact:{index}"),
            fact.recorded_at.as_unix_micros(),
            MemoryEvent::FactRecorded { fact },
        )
    }));
    let mut store = InMemoryStore::default();
    store
        .append(
            &StreamId::new("analytics").unwrap(),
            ExpectedVersion::NoStream,
            &pending,
        )
        .unwrap();
    replay(&store.load_all(None, usize::MAX)).unwrap()
}

#[test]
fn belief_revision_cascades_into_decisions_without_mutation() {
    let projection = projection();
    let hypothesis = fact(
        "fact:hypothesis",
        "belief:subject",
        "state",
        "value:new",
        10,
        10,
    );
    let report = MemoryAnalytics::belief_revision(
        &projection,
        &BeliefRevisionRequest {
            hypothesis,
            clock: ProjectionClock::new(ts(10), ts(10)),
            max_depth: 3,
        },
    )
    .unwrap();

    assert_eq!(report.contradictions.len(), 1);
    assert_eq!(report.contradictions[0].fact.as_str(), "fact:belief");
    assert!(
        report
            .cascade
            .iter()
            .any(|item| item.entity == entity("decision:1"))
    );
    assert_eq!(report.invalidated_decisions, vec![entity("decision:1")]);
}

#[test]
fn gaps_drift_and_consolidation_are_evidence_aware() {
    let projection = projection();
    let clock = ProjectionClock::new(ts(10), ts(10));
    let gaps = MemoryAnalytics::reasoning_gaps(
        &projection,
        ReasoningGapRequest {
            clock,
            minimum_supports: 2,
            low_confidence_bps: 5_000,
            unstable_revision_count: 4,
            stale_after_micros: 5,
            max_results: 100,
        },
    )
    .unwrap();

    assert!(
        gaps.gaps
            .iter()
            .any(|gap| gap.kind == GapKind::SingleSourceInference)
    );
    assert!(
        gaps.gaps
            .iter()
            .any(|gap| gap.kind == GapKind::LowConfidenceFoundation)
    );
    assert!(
        gaps.gaps
            .iter()
            .any(|gap| gap.kind == GapKind::UnstableKnowledge)
    );

    let drift =
        MemoryAnalytics::drift(&projection, &entity("belief:subject"), "policy", ts(10)).unwrap();
    assert_eq!(drift.snapshots.len(), 4);
    assert_eq!(drift.correction_count, 3);
    assert!(drift.likely_to_change);

    let plan = MemoryAnalytics::consolidation_plan(&projection, clock, 100).unwrap();
    assert!(plan.actions.iter().any(|action| {
        action.kind == ConsolidationKind::SupersedeDuplicate && action.affected_facts.len() == 2
    }));
    assert!(
        plan.actions
            .iter()
            .any(|action| action.kind == ConsolidationKind::ReviewOrphan)
    );
    assert!(
        plan.actions
            .iter()
            .any(|action| action.kind == ConsolidationKind::CompactRevisionChain)
    );
}

#[test]
fn analytics_reject_invalid_requests() {
    let projection = projection();
    let clock = ProjectionClock::new(ts(10), ts(10));
    assert!(
        MemoryAnalytics::reasoning_gaps(
            &projection,
            ReasoningGapRequest {
                clock,
                minimum_supports: 0,
                low_confidence_bps: 0,
                unstable_revision_count: 1,
                stale_after_micros: -1,
                max_results: 0,
            },
        )
        .is_err()
    );
    assert!(MemoryAnalytics::consolidation_plan(&projection, clock, 0).is_err());
    assert!(
        MemoryAnalytics::drift(&projection, &entity("belief:subject"), " policy", ts(10)).is_err()
    );
}
