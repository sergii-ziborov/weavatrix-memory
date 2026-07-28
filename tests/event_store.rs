mod common;

use common::{event, node};
use weavatrix_memory::{
    AppendReceipt, EventId, EventStore, ExpectedVersion, InMemoryStore, MemoryError, MemoryEvent,
    StreamId,
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
    let result = blazingly_json::from_str::<EventId>("\" invalid \"");
    assert!(result.is_err());
}

#[test]
fn owned_append_preserves_the_event_contract() {
    let stream = StreamId::new("task:owned").unwrap();
    let pending = event(
        "event-owned",
        10,
        MemoryEvent::NodeUpserted {
            node: node("node:owned", "task", "Owned append"),
        },
    );
    let mut store = InMemoryStore::default();

    let committed = store
        .append_owned(&stream, ExpectedVersion::NoStream, vec![pending])
        .unwrap();

    assert_eq!(committed, store.load_stream(&stream, None));
}

#[test]
fn receipt_append_moves_payloads_and_reports_exact_positions() {
    let stream = StreamId::new("task:receipt").unwrap();
    let pending = (0..3)
        .map(|index| {
            event(
                &format!("event-receipt-{index}"),
                index,
                MemoryEvent::NodeUpserted {
                    node: node(&format!("node:receipt:{index}"), "task", "Receipt append"),
                },
            )
        })
        .collect();
    let mut store = InMemoryStore::default();

    let receipt = store
        .append_owned_receipt(&stream, ExpectedVersion::NoStream, pending)
        .unwrap();

    assert_eq!(receipt.event_count, 3);
    assert_eq!(receipt.first_stream_version, Some(0));
    assert_eq!(receipt.last_stream_version, Some(2));
    assert_eq!(receipt.first_global_position, Some(0));
    assert_eq!(receipt.last_global_position, Some(2));
    assert_eq!(store.len(), 3);
}

#[test]
fn empty_receipt_append_is_observable_without_creating_a_stream() {
    let stream = StreamId::new("task:empty-receipt").unwrap();
    let mut store = InMemoryStore::<MemoryEvent>::default();

    let receipt = store
        .append_owned_receipt(&stream, ExpectedVersion::NoStream, Vec::new())
        .unwrap();

    assert_eq!(receipt, AppendReceipt::default());
    assert_eq!(store.stream_version(&stream), None);
}
