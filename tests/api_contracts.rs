mod common;

use common::{agent, entity, event, fact, node, session, simple_projection, ts};
use std::str::FromStr;
use weavatrix_memory::{
    AgentId, BytesTokenEstimator, Confidence, ContextCompiler, ContextRequest, EntityId, EventId,
    EventStore, Evidence, ExpectedVersion, FactId, InMemoryStore, MemoryError, MemoryEvent,
    MemoryFact, MemoryNode, MemoryProjection, MemoryView, NewEvent, ProjectionClock, SessionId,
    StreamId, TokenEstimator, project_graph, replay,
};

#[test]
fn public_builders_validate_and_preserve_metadata() {
    assert!(Confidence::from_basis_points(10_001).is_err());
    let confidence = Confidence::from_basis_points(8_500).unwrap();
    assert_eq!(confidence.basis_points(), 8_500);

    let evidence = Evidence::new("test", "suite")
        .unwrap()
        .with_locator("tests/query.rs:10")
        .with_digest("sha256:abc");
    let node = MemoryNode::new(entity("file:1"), "file", "query.rs")
        .unwrap()
        .in_repository("example")
        .on_branch("main")
        .with_attribute("language", "rust");
    assert_eq!(node.repository.as_deref(), Some("example"));

    let fact = MemoryFact::new(
        FactId::new("fact:1").unwrap(),
        entity("task:1"),
        "affects",
        entity("file:1"),
        ts(10),
        ts(20),
        agent(),
        session(),
        evidence.clone(),
    )
    .unwrap()
    .valid_until(ts(30))
    .unwrap()
    .observed_at(ts(15))
    .with_confidence(confidence)
    .with_evidence(Evidence::new("user", "review").unwrap());
    assert_eq!(fact.evidence.len(), 2);
    assert_eq!(fact.confidence, confidence);
    assert!(fact.clone().valid_until(ts(10)).is_err());
    assert!(fact.supersedes(FactId::new("fact:1").unwrap()).is_err());

    assert!(Evidence::new("", "source").is_err());
    assert!(MemoryNode::new(entity("bad:1"), " bad", "Bad").is_err());
}

#[test]
fn event_and_identifier_contracts_are_checked_at_boundaries() {
    let payload = MemoryEvent::NodeUpserted {
        node: node("node:1", "task", "Task"),
    };
    assert!(
        NewEvent::new(
            EventId::new("event:bad-type").unwrap(),
            " bad ",
            ts(1),
            ts(1),
            agent(),
            session(),
            payload.clone(),
        )
        .is_err()
    );
    assert!(
        NewEvent::new(
            EventId::new("event:future").unwrap(),
            "node_upserted",
            ts(2),
            ts(1),
            agent(),
            session(),
            payload.clone(),
        )
        .is_err()
    );
    let event = NewEvent::new(
        EventId::new("event:ok").unwrap(),
        payload.event_type(),
        ts(1),
        ts(1),
        agent(),
        session(),
        payload,
    )
    .unwrap()
    .correlated_with(EventId::new("correlation:1").unwrap())
    .caused_by(EventId::new("cause:1").unwrap());
    assert!(event.correlation_id.is_some());
    assert!(event.causation_id.is_some());

    let id = EntityId::from_str("entity:1").unwrap();
    assert_eq!(id.to_string(), "entity:1");
    assert_eq!(id.into_inner(), "entity:1");
    assert!(AgentId::new("").is_err());
    assert!(SessionId::new(" padded ").is_err());
}

#[test]
fn graph_projection_keeps_temporal_and_provenance_attributes() {
    let nodes = vec![
        node("task:1", "task", "Task"),
        node("file:1", "file", "File"),
    ];
    let confidence = [10_000, 9_000, 6_000, 1_000];
    let facts = confidence
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            fact(
                &format!("fact:{index}"),
                "task:1",
                &format!("relation_{index}"),
                "file:1",
                10,
                20,
            )
            .with_confidence(Confidence::from_basis_points(value).unwrap())
            .with_evidence(
                Evidence::new("benchmark", "suite")
                    .unwrap()
                    .with_locator("result.json")
                    .with_digest("sha256:def"),
            )
        })
        .collect::<Vec<_>>();
    let graph = project_graph(&MemoryView { nodes, facts }).unwrap();

    assert_eq!(graph.node_count(), 2);
    assert_eq!(graph.edge_count(), 4);
    assert!(graph.edges()[0].attributes.contains_key("memory.fact_id"));

    let invalid = MemoryView {
        nodes: vec![node("task:1", "task", "Task")],
        facts: vec![fact(
            "fact:missing",
            "task:1",
            "affects",
            "file:missing",
            1,
            2,
        )],
    };
    assert!(matches!(
        project_graph(&invalid),
        Err(MemoryError::MissingEntity { .. })
    ));
}

#[test]
fn compiler_builders_and_budget_omissions_are_observable() {
    let projection = simple_projection();
    let estimator = BytesTokenEstimator::new(1).unwrap();
    assert_eq!(estimator.name(), "utf8_bytes");
    assert_eq!(estimator.estimate("abcd"), 4);
    assert!(BytesTokenEstimator::new(0).is_err());

    let seed = entity("task:1");
    let request = ContextRequest::new(vec![seed.clone()], ts(10), ts(10), 40)
        .unwrap()
        .with_max_depth(1)
        .include_relation("affects")
        .in_repository("example")
        .on_branch("main");
    let bundle = ContextCompiler::new(estimator)
        .compile(&projection, &request)
        .unwrap();
    assert_eq!(bundle.view.nodes.len(), 1);
    assert_eq!(bundle.receipt.omitted_by_budget, 1);

    let missing = ContextRequest::new(vec![entity("missing:1")], ts(10), ts(10), 100).unwrap();
    assert!(matches!(
        ContextCompiler::default().compile(&projection, &missing),
        Err(MemoryError::MissingEntity { .. })
    ));
    assert!(ContextRequest::new(Vec::new(), ts(1), ts(1), 10).is_err());
    assert!(ContextRequest::new(vec![seed], ts(1), ts(1), 0).is_err());
}

#[test]
fn projection_rejects_invalid_domain_events() {
    let stream = StreamId::new("stream:invalid-domain").unwrap();
    let missing_fact = fact("fact:missing", "task:1", "affects", "file:1", 1, 2);
    let mut store = InMemoryStore::default();
    store
        .append(
            &stream,
            ExpectedVersion::NoStream,
            &[event(
                "event:missing",
                2,
                MemoryEvent::FactRecorded { fact: missing_fact },
            )],
        )
        .unwrap();
    assert!(matches!(
        replay::<_, MemoryProjection>(&store.load_all(None, usize::MAX)),
        Err(MemoryError::MissingEntity { .. })
    ));

    let projection = simple_projection();
    assert_eq!(projection.last_global_position(), Some(2));
    assert!(
        projection
            .superseded_by(&FactId::new("fact:1").unwrap())
            .is_none()
    );
    assert_eq!(
        projection
            .view(ProjectionClock::new(ts(10), ts(10)))
            .facts
            .len(),
        1
    );
}

#[test]
fn error_messages_cover_every_public_failure_family() {
    let errors = vec![
        MemoryError::InvalidId {
            kind: "event",
            value: String::new(),
        },
        MemoryError::InvalidValue {
            field: "field",
            reason: "reason",
        },
        MemoryError::VersionConflict {
            stream: "stream".to_owned(),
            expected: Some(1),
            actual: Some(2),
        },
        MemoryError::DuplicateEvent {
            id: "event".to_owned(),
        },
        MemoryError::InvalidReplay {
            reason: "gap".to_owned(),
        },
        MemoryError::MissingEntity {
            id: "entity".to_owned(),
        },
        MemoryError::MissingFact {
            id: "fact".to_owned(),
        },
        MemoryError::ConflictingFact {
            id: "fact".to_owned(),
        },
        MemoryError::BudgetTooSmall {
            required: 2,
            available: 1,
        },
        MemoryError::CapacityOverflow,
        MemoryError::Io {
            operation: "read",
            message: "failed".to_owned(),
        },
        MemoryError::Codec {
            message: "invalid".to_owned(),
        },
        MemoryError::CorruptLog {
            offset: 4,
            reason: "checksum".to_owned(),
        },
        MemoryError::ExternalModification,
        MemoryError::Retrieval {
            provider: "search".to_owned(),
            message: "offline".to_owned(),
        },
        MemoryError::Extraction {
            provider: "extractor".to_owned(),
            message: "invalid output".to_owned(),
        },
        MemoryError::Graph("invalid".to_owned()),
    ];
    for error in errors {
        assert!(!error.to_string().is_empty());
        let source: &dyn std::error::Error = &error;
        assert!(source.source().is_none());
    }
}
