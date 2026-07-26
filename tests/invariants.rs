mod common;

use common::{event, fact, node, ts};
use weavatrix_memory::{
    EventStore, ExpectedVersion, FactId, InMemoryStore, MemoryError, MemoryEvent, MemoryProjection,
    ProjectionClock, StoredEvent, StreamId, replay,
};

#[test]
fn generated_multi_stream_history_round_trips() {
    for count in 1_usize..64 {
        let mut store = InMemoryStore::default();
        for index in 0..count {
            let stream = StreamId::new(format!("stream:{}", index % 3)).unwrap();
            store
                .append(
                    &stream,
                    ExpectedVersion::Any,
                    &[event(
                        &format!("event:{count}:{index}"),
                        i64::try_from(index).unwrap(),
                        MemoryEvent::NodeUpserted {
                            node: node(
                                &format!("observation:{count}:{index}"),
                                "observation",
                                "Generated",
                            ),
                        },
                    )],
                )
                .unwrap();
        }
        let events = store.load_all(None, usize::MAX);
        let json = serde_json::to_vec(&events).unwrap();
        let decoded: Vec<StoredEvent<MemoryEvent>> = serde_json::from_slice(&json).unwrap();
        let original: MemoryProjection = replay(&events).unwrap();
        let round_trip: MemoryProjection = replay(&decoded).unwrap();
        let clock = ProjectionClock::new(ts(100), ts(100));
        assert_eq!(original.view(clock), round_trip.view(clock));
        assert_eq!(original.view(clock).nodes.len(), count);
    }
}

#[test]
fn replay_rejects_duplicate_event_id_and_wrong_event_type() {
    let stream = StreamId::new("stream:invalid").unwrap();
    let mut store = InMemoryStore::default();
    store
        .append(
            &stream,
            ExpectedVersion::NoStream,
            &[
                event(
                    "event:1",
                    1,
                    MemoryEvent::NodeUpserted {
                        node: node("node:1", "task", "One"),
                    },
                ),
                event(
                    "event:2",
                    2,
                    MemoryEvent::NodeUpserted {
                        node: node("node:2", "file", "Two"),
                    },
                ),
            ],
        )
        .unwrap();
    let mut events = store.load_all(None, usize::MAX);
    events[1].metadata.id = events[0].metadata.id.clone();
    assert!(matches!(
        replay::<_, MemoryProjection>(&events),
        Err(MemoryError::DuplicateEvent { .. })
    ));

    let mut events = store.load_all(None, usize::MAX);
    events[0].metadata.event_type = "fact_recorded".to_owned();
    assert!(replay::<_, MemoryProjection>(&events).is_err());
}

#[test]
fn one_fact_cannot_have_two_canonical_replacements() {
    let original = fact("fact:old", "decision:1", "affects", "file:1", 1, 2);
    let first = fact("fact:new:1", "decision:1", "affects", "file:1", 3, 3)
        .supersedes(original.id.clone())
        .unwrap();
    let second = fact("fact:new:2", "decision:1", "affects", "file:1", 4, 4)
        .supersedes(FactId::new("fact:old").unwrap())
        .unwrap();
    let payloads = vec![
        (
            1,
            MemoryEvent::NodeUpserted {
                node: node("decision:1", "decision", "Decision"),
            },
        ),
        (
            1,
            MemoryEvent::NodeUpserted {
                node: node("file:1", "file", "File"),
            },
        ),
        (2, MemoryEvent::FactRecorded { fact: original }),
        (3, MemoryEvent::FactRecorded { fact: first }),
        (4, MemoryEvent::FactRecorded { fact: second }),
    ];
    let events = payloads
        .into_iter()
        .enumerate()
        .map(|(index, (recorded_at, payload))| {
            event(&format!("event:{index}"), recorded_at, payload)
        })
        .collect::<Vec<_>>();
    let stream = StreamId::new("stream:supersession").unwrap();
    let mut store = InMemoryStore::default();
    store
        .append(&stream, ExpectedVersion::NoStream, &events)
        .unwrap();

    assert!(matches!(
        replay::<_, MemoryProjection>(&store.load_all(None, usize::MAX)),
        Err(MemoryError::ConflictingFact { .. })
    ));
}
