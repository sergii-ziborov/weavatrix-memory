mod common;

use common::{event, fact, node, ts};
use weavatrix_memory::{
    ContextCompiler, ContextRequest, EntityId, EventStore, ExpectedVersion, InMemoryStore,
    MemoryError, MemoryEvent, MemoryProjection, StreamId, replay,
};

fn projection() -> MemoryProjection {
    let main_task = node("task:1", "task", "Fix shaping")
        .in_repository("analytics")
        .on_branch("main");
    let main_file = node("file:main", "file", "query-builder.rs")
        .in_repository("analytics")
        .on_branch("main");
    let other_file = node("file:other", "file", "legacy.rs")
        .in_repository("analytics")
        .on_branch("release");
    let events = vec![
        event(
            "node:task",
            1,
            MemoryEvent::NodeUpserted { node: main_task },
        ),
        event(
            "node:main",
            1,
            MemoryEvent::NodeUpserted { node: main_file },
        ),
        event(
            "node:other",
            1,
            MemoryEvent::NodeUpserted { node: other_file },
        ),
        event(
            "fact:main:event",
            2,
            MemoryEvent::FactRecorded {
                fact: fact("fact:main", "task:1", "affects", "file:main", 2, 2),
            },
        ),
        event(
            "fact:other:event",
            3,
            MemoryEvent::FactRecorded {
                fact: fact("fact:other", "task:1", "affects", "file:other", 3, 3),
            },
        ),
    ];
    let stream = StreamId::new("task:1").unwrap();
    let mut store = InMemoryStore::default();
    store
        .append(&stream, ExpectedVersion::NoStream, &events)
        .unwrap();
    replay(&store.load_all(None, usize::MAX)).unwrap()
}

#[test]
fn compiler_honors_scope_budget_and_produces_graph() {
    let projection = projection();
    let request = ContextRequest::new(vec![EntityId::new("task:1").unwrap()], ts(10), ts(10), 200)
        .unwrap()
        .in_repository("analytics")
        .on_branch("main")
        .include_relation("affects");
    let compiler = ContextCompiler::default();

    let first = compiler.compile(&projection, &request).unwrap();
    let second = compiler.compile(&projection, &request).unwrap();

    assert_eq!(first.view.nodes.len(), 2);
    assert_eq!(first.view.facts.len(), 1);
    assert_eq!(first.graph.edge_count(), 1);
    assert_eq!(first.receipt.excluded_by_scope, 1);
    assert_eq!(first.receipt, second.receipt);
    assert!(first.receipt.estimated_tokens <= request.token_budget);
}

#[test]
fn compiler_never_silently_exceeds_seed_budget() {
    let projection = projection();
    let request =
        ContextRequest::new(vec![EntityId::new("task:1").unwrap()], ts(10), ts(10), 1).unwrap();

    let error = ContextCompiler::default()
        .compile(&projection, &request)
        .unwrap_err();
    assert!(matches!(error, MemoryError::BudgetTooSmall { .. }));
}

#[test]
fn indexed_compiler_keeps_edges_between_depth_boundary_nodes() {
    let nodes = ["node:a", "node:b", "node:c"].map(|id| node(id, "observation", id));
    let events = vec![
        event(
            "event:a",
            1,
            MemoryEvent::NodeUpserted {
                node: nodes[0].clone(),
            },
        ),
        event(
            "event:b",
            1,
            MemoryEvent::NodeUpserted {
                node: nodes[1].clone(),
            },
        ),
        event(
            "event:c",
            1,
            MemoryEvent::NodeUpserted {
                node: nodes[2].clone(),
            },
        ),
        event(
            "event:ab",
            2,
            MemoryEvent::FactRecorded {
                fact: fact("fact:ab", "node:a", "supports", "node:b", 2, 2),
            },
        ),
        event(
            "event:ac",
            2,
            MemoryEvent::FactRecorded {
                fact: fact("fact:ac", "node:a", "supports", "node:c", 2, 2),
            },
        ),
        event(
            "event:bc",
            2,
            MemoryEvent::FactRecorded {
                fact: fact("fact:bc", "node:b", "supports", "node:c", 2, 2),
            },
        ),
    ];
    let mut store = InMemoryStore::default();
    store
        .append(
            &StreamId::new("boundary").unwrap(),
            ExpectedVersion::NoStream,
            &events,
        )
        .unwrap();
    let projection = replay(&store.load_all(None, usize::MAX)).unwrap();
    let request = ContextRequest::new(
        vec![EntityId::new("node:a").unwrap()],
        ts(10),
        ts(10),
        10_000,
    )
    .unwrap()
    .with_max_depth(1);

    let bundle = ContextCompiler::default()
        .compile(&projection, &request)
        .unwrap();

    assert_eq!(bundle.graph.node_count(), 3);
    assert_eq!(bundle.graph.edge_count(), 3);
}
