mod common;

use common::{agent, entity, node, session, ts};
use weavatrix_memory::{
    AutoExtractionEngine, Confidence, EntityHint, EntityId, ExtractedEntity, ExtractedRelation,
    ExtractionError, ExtractionInput, ExtractionOutput, ExtractionProvider, LinkMethod, LinkPolicy,
    MemoryEvent, MemoryNode, MemoryView, TextSpan,
};

#[derive(Clone)]
struct StaticProvider {
    output: ExtractionOutput,
}

impl ExtractionProvider for StaticProvider {
    fn name(&self) -> &'static str {
        "static-test"
    }

    fn extract(&self, _input: &ExtractionInput) -> Result<ExtractionOutput, ExtractionError> {
        Ok(self.output.clone())
    }
}

struct WrongIdentity;

impl ExtractionProvider for WrongIdentity {
    fn name(&self) -> &'static str {
        "expected"
    }

    fn extract(&self, _input: &ExtractionInput) -> Result<ExtractionOutput, ExtractionError> {
        Err(ExtractionError::new("different", "failed"))
    }
}

fn confidence(value: u16) -> Confidence {
    Confidence::from_basis_points(value).unwrap()
}

fn input() -> ExtractionInput {
    ExtractionInput::new(
        "issue:GPRO-1",
        "build result reads public metrics",
        ts(10),
        ts(12),
        agent(),
        session(),
    )
    .unwrap()
    .in_repository("analytics")
    .on_branch("main")
    .with_locator("issue/GPRO-1")
    .with_digest("sha256:example")
}

fn relation(source: &str, target: &str) -> ExtractedRelation {
    ExtractedRelation::new("relation:reads", source, "reads", target, confidence(9_500))
        .unwrap()
        .with_span(TextSpan::new(13, 18).unwrap())
}

#[test]
fn scoped_and_external_links_create_evidence_fact() {
    let nodes = vec![
        node("fn:analytics", "function", "Build-Result")
            .in_repository("analytics")
            .on_branch("main"),
        node("fn:other", "function", "Build Result")
            .in_repository("other")
            .on_branch("main"),
        node("table:metrics", "table", "Metrics")
            .in_repository("analytics")
            .with_attribute("external_id.sql", "public.metrics"),
    ];
    let source =
        ExtractedEntity::new("source", "function", "build result", Confidence::CERTAIN).unwrap();
    let target = ExtractedEntity::new("target", "table", "unknown", Confidence::CERTAIN)
        .unwrap()
        .with_attribute("external_id.sql", "PUBLIC.METRICS");
    let provider = StaticProvider {
        output: ExtractionOutput {
            entities: vec![source, target],
            relations: vec![relation("source", "target")],
        },
    };

    let plan = AutoExtractionEngine::default()
        .plan(
            &provider,
            &input(),
            &MemoryView {
                nodes,
                facts: Vec::new(),
            },
        )
        .unwrap();

    assert_eq!(plan.node_event_count(), 0);
    assert_eq!(plan.fact_event_count(), 1);
    assert_eq!(plan.links[0].method, LinkMethod::ScopedLabel);
    assert_eq!(
        plan.links[0].entity_id.as_ref().unwrap().as_str(),
        "fn:analytics"
    );
    assert_eq!(plan.links[1].method, LinkMethod::ExternalId);
    let MemoryEvent::FactRecorded { fact } = &plan.events[0].payload else {
        panic!("expected fact event");
    };
    assert_eq!(fact.source.as_str(), "fn:analytics");
    assert_eq!(fact.target.as_str(), "table:metrics");
    assert_eq!(fact.confidence.basis_points(), 9_500);
    assert_eq!(fact.evidence.len(), 2);
    assert_eq!(fact.evidence[0].digest.as_deref(), Some("sha256:example"));
    assert_eq!(fact.evidence[1].locator.as_deref(), Some("bytes:13-18"));
}

#[test]
fn unmatched_entities_produce_deterministic_idempotent_events() {
    let source = ExtractedEntity::new("source", "task", "Fix query", confidence(9_000))
        .unwrap()
        .with_alias("GPRO-1");
    let target = ExtractedEntity::new("target", "file", "query.rs", confidence(8_700)).unwrap();
    let provider = StaticProvider {
        output: ExtractionOutput {
            entities: vec![source, target],
            relations: vec![relation("source", "target")],
        },
    };
    let engine = AutoExtractionEngine::default();

    let first = engine
        .plan(&provider, &input(), &MemoryView::default())
        .unwrap();
    let second = engine
        .plan(&provider, &input(), &MemoryView::default())
        .unwrap();

    assert_eq!(first, second);
    assert_eq!(first.node_event_count(), 2);
    assert_eq!(first.fact_event_count(), 1);
    assert!(first.rejected_relations.is_empty());
    assert!(
        first
            .links
            .iter()
            .all(|link| link.method == LinkMethod::Created)
    );
    let node = first
        .events
        .iter()
        .find_map(|event| match &event.payload {
            MemoryEvent::NodeUpserted { node } if node.kind == "task" => Some(node),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        node.attributes.get("alias.0").map(String::as_str),
        Some("GPRO-1")
    );
}

#[test]
fn ambiguity_is_reported_instead_of_silently_merged() {
    let nodes = vec![
        node("fn:one", "function", "render"),
        node("fn:two", "function", "render"),
        node("file:one", "file", "view.rs"),
    ];
    let source = ExtractedEntity::new("source", "function", "render", Confidence::CERTAIN).unwrap();
    let target = ExtractedEntity::new("target", "file", "view.rs", Confidence::CERTAIN)
        .unwrap()
        .with_stable_id(entity("file:one"));
    let provider = StaticProvider {
        output: ExtractionOutput {
            entities: vec![source, target],
            relations: vec![relation("source", "target")],
        },
    };
    let no_scope = ExtractionInput::new(
        "message:1",
        "render affects view",
        ts(1),
        ts(1),
        agent(),
        session(),
    )
    .unwrap();

    let plan = AutoExtractionEngine::default()
        .plan(
            &provider,
            &no_scope,
            &MemoryView {
                nodes,
                facts: Vec::new(),
            },
        )
        .unwrap();

    assert_eq!(plan.links[0].method, LinkMethod::Ambiguous);
    assert_eq!(plan.links[0].candidates.len(), 2);
    assert_eq!(plan.fact_event_count(), 0);
    assert_eq!(plan.rejected_relations.len(), 1);
}

#[test]
fn provider_hint_and_strict_no_create_policy_are_supported() {
    let hinted = node("symbol:known", "function", "canonical");
    let hint = EntityHint::new(entity("symbol:known"), confidence(9_200), "semantic").unwrap();
    let known = ExtractedEntity::new("known", "function", "different", Confidence::CERTAIN)
        .unwrap()
        .with_hint(hint);
    let unknown =
        ExtractedEntity::new("unknown", "file", "missing.rs", Confidence::CERTAIN).unwrap();
    let linker = weavatrix_memory::EntityLinker::from_view(&MemoryView {
        nodes: vec![hinted],
        facts: Vec::new(),
    })
    .unwrap();
    let strict = LinkPolicy::new(8_000, 500, false).unwrap();

    let known = linker.link(&known, &input(), strict).unwrap();
    let unknown = linker.link(&unknown, &input(), strict).unwrap();

    assert_eq!(known.method, LinkMethod::ProviderHint);
    assert_eq!(known.entity_id.unwrap().as_str(), "symbol:known");
    assert_eq!(unknown.method, LinkMethod::Unresolved);
    assert!(unknown.entity_id.is_none());
}

#[test]
fn invalid_provider_output_and_identity_are_rejected() {
    let bad_span = ExtractedEntity::new("entity", "file", "файл", Confidence::CERTAIN)
        .unwrap()
        .with_span(TextSpan::new(100, 101).unwrap());
    let provider = StaticProvider {
        output: ExtractionOutput {
            entities: vec![bad_span],
            relations: Vec::new(),
        },
    };
    assert!(
        AutoExtractionEngine::default()
            .plan(&provider, &input(), &MemoryView::default())
            .is_err()
    );

    let error = AutoExtractionEngine::default()
        .plan(&WrongIdentity, &input(), &MemoryView::default())
        .unwrap_err();
    assert!(error.to_string().contains("identity"));
}

#[test]
fn stable_identifier_kind_conflicts_are_rejected() {
    let mention = ExtractedEntity::new("entity", "file", "same", Confidence::CERTAIN)
        .unwrap()
        .with_stable_id(EntityId::new("entity:same").unwrap());
    let linker = weavatrix_memory::EntityLinker::from_view(&MemoryView {
        nodes: vec![MemoryNode::new(entity("entity:same"), "task", "same").unwrap()],
        facts: Vec::new(),
    })
    .unwrap();

    assert!(
        linker
            .link(&mention, &input(), LinkPolicy::default())
            .is_err()
    );
}
