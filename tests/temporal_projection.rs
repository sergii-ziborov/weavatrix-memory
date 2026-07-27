mod common;

use common::{event, fact, node, ts};
use weavatrix_memory::{
    EventStore, Evidence, ExpectedVersion, FactId, InMemoryStore, MemoryEvent, MemoryProjection,
    ProjectionClock, StreamId, replay,
};

#[test]
fn late_supersession_changes_known_future_not_known_past() {
    let stream = StreamId::new("task:temporal").unwrap();
    let old = fact("fact:old", "decision:1", "affects", "file:1", 10, 20);
    let replacement = fact("fact:new", "decision:1", "affects", "file:1", 30, 50)
        .supersedes(old.id.clone())
        .unwrap();
    let events = vec![
        event(
            "event:node:decision",
            5,
            MemoryEvent::NodeUpserted {
                node: node("decision:1", "decision", "Use DateHour"),
            },
        ),
        event(
            "event:node:file",
            5,
            MemoryEvent::NodeUpserted {
                node: node("file:1", "file", "query-builder.rs"),
            },
        ),
        event(
            "event:fact:old",
            20,
            MemoryEvent::FactRecorded { fact: old },
        ),
        event(
            "event:fact:new",
            50,
            MemoryEvent::FactRecorded { fact: replacement },
        ),
    ];
    let mut store = InMemoryStore::default();
    store
        .append(&stream, ExpectedVersion::NoStream, &events)
        .unwrap();
    let projection = replay::<_, MemoryProjection>(&store.load_all(None, usize::MAX)).unwrap();

    let before_learning = projection.view(ProjectionClock::new(ts(40), ts(40)));
    assert_eq!(
        before_learning,
        projection
            .view_ref(ProjectionClock::new(ts(40), ts(40)))
            .into_owned()
    );
    assert_eq!(before_learning.facts[0].id.as_str(), "fact:old");

    let after_learning = projection.view(ProjectionClock::new(ts(40), ts(60)));
    assert_eq!(
        after_learning,
        projection
            .view_ref(ProjectionClock::new(ts(40), ts(60)))
            .into_owned()
    );
    assert_eq!(after_learning.facts[0].id.as_str(), "fact:new");

    let before_nodes = projection.view_ref(ProjectionClock::new(ts(1), ts(1)));
    assert!(before_nodes.nodes.is_empty());
    assert!(before_nodes.facts.is_empty());
}

#[test]
fn replay_rejects_tampered_global_position() {
    let stream = StreamId::new("task:tamper").unwrap();
    let mut store = InMemoryStore::default();
    store
        .append(
            &stream,
            ExpectedVersion::NoStream,
            &[event(
                "event:1",
                1,
                MemoryEvent::NodeUpserted {
                    node: node("task:1", "task", "Task"),
                },
            )],
        )
        .unwrap();
    let mut events = store.load_all(None, usize::MAX);
    events[0].metadata.global_position = 9;

    assert!(replay::<_, MemoryProjection>(&events).is_err());
}

#[test]
fn retraction_is_visible_only_after_it_is_recorded() {
    let original = fact("fact:old", "task:1", "affects", "file:1", 10, 20);
    let events = vec![
        event(
            "event:task",
            1,
            MemoryEvent::NodeUpserted {
                node: node("task:1", "task", "Task"),
            },
        ),
        event(
            "event:file",
            1,
            MemoryEvent::NodeUpserted {
                node: node("file:1", "file", "File"),
            },
        ),
        event(
            "event:fact",
            20,
            MemoryEvent::FactRecorded { fact: original },
        ),
        event(
            "event:retract",
            50,
            MemoryEvent::FactRetracted {
                fact_id: FactId::new("fact:old").unwrap(),
                valid_until: ts(30),
                evidence: vec![Evidence::new("test", "regression-suite").unwrap()],
            },
        ),
    ];
    let stream = StreamId::new("task:retract").unwrap();
    let mut store = InMemoryStore::default();
    store
        .append(&stream, ExpectedVersion::NoStream, &events)
        .unwrap();
    let projection: MemoryProjection = replay(&store.load_all(None, usize::MAX)).unwrap();

    assert_eq!(
        projection
            .view(ProjectionClock::new(ts(40), ts(40)))
            .facts
            .len(),
        1
    );
    assert!(
        projection
            .view(ProjectionClock::new(ts(40), ts(60)))
            .facts
            .is_empty()
    );
}
