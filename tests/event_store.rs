mod common;

use common::{event, node};
use weavatrix_memory::{
    EventId, EventStore, ExpectedVersion, InMemoryStore, MemoryError, MemoryEvent, StreamId,
};

#[test]
fn append_is_atomic_and_optimistically_concurrent() {
    let stream = StreamId::new("task:GPRO-7301").unwrap();
    let first = event(
        "event-1",
        10,
        MemoryEvent::NodeUpserted {
            node: node("task:1", "task", "Fix shaping"),
        },
    );
    let second = event(
        "event-2",
        11,
        MemoryEvent::NodeUpserted {
            node: node("file:1", "file", "query-builder.rs"),
        },
    );
    let mut store = InMemoryStore::default();
    let committed = store
        .append(&stream, ExpectedVersion::NoStream, &[first.clone(), second])
        .unwrap();

    assert_eq!(committed[0].metadata.stream_version, 0);
    assert_eq!(committed[1].metadata.stream_version, 1);
    assert_eq!(store.stream_version(&stream), Some(1));

    let error = store
        .append(&stream, ExpectedVersion::Exact(0), &[first])
        .unwrap_err();
    assert!(matches!(error, MemoryError::VersionConflict { .. }));
    assert_eq!(store.len(), 2);
}

#[test]
fn duplicate_in_batch_does_not_partially_append() {
    let stream = StreamId::new("task:1").unwrap();
    let duplicate = event(
        "same-id",
        10,
        MemoryEvent::NodeUpserted {
            node: node("node:1", "task", "One"),
        },
    );
    let mut store = InMemoryStore::default();
    let error = store
        .append(
            &stream,
            ExpectedVersion::NoStream,
            &[duplicate.clone(), duplicate],
        )
        .unwrap_err();

    assert!(matches!(error, MemoryError::DuplicateEvent { .. }));
    assert!(store.is_empty());
}

#[test]
fn cursors_are_exclusive_and_deterministic() {
    let stream = StreamId::new("task:1").unwrap();
    let events = (0..4)
        .map(|index| {
            event(
                &format!("event-{index}"),
                index,
                MemoryEvent::NodeUpserted {
                    node: node(&format!("node:{index}"), "observation", "Observed"),
                },
            )
        })
        .collect::<Vec<_>>();
    let mut store = InMemoryStore::default();
    store
        .append(&stream, ExpectedVersion::NoStream, &events)
        .unwrap();

    assert_eq!(store.load_stream(&stream, Some(1)).len(), 2);
    assert_eq!(store.load_all(Some(1), 1)[0].metadata.global_position, 2);
}

#[test]
fn identifier_deserialization_preserves_validation() {
    let result = serde_json::from_str::<EventId>("\" invalid \"");
    assert!(result.is_err());
}
