use std::{
    hint::black_box,
    time::{Duration, Instant},
};
use weavatrix_memory::{
    AgentId, EntityId, EventId, EventStore, ExpectedVersion, InMemoryStore, MemoryEvent,
    MemoryNode, MemoryProjection, NewEvent, SessionId, StreamId, Timestamp, replay,
};

fn main() {
    let count = std::env::var("WEAVATRIX_BENCH_EVENTS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(100_000);
    let events = fixture(count);
    for _ in 0..2 {
        black_box(replay::<_, MemoryProjection>(&events).unwrap());
    }
    let mut samples = (0..9)
        .map(|_| {
            let started = Instant::now();
            black_box(replay::<_, MemoryProjection>(&events).unwrap());
            started.elapsed()
        })
        .collect::<Vec<_>>();
    samples.sort_unstable();
    report(count, samples[samples.len() / 2]);
}

fn fixture(count: usize) -> Vec<weavatrix_memory::StoredEvent<MemoryEvent>> {
    let agent = AgentId::new("bench-agent").unwrap();
    let session = SessionId::new("bench-session").unwrap();
    let mut pending = Vec::with_capacity(count);
    for index in 0..count {
        let timestamp = i64::try_from(index).expect("benchmark event count fits i64");
        let node = MemoryNode::new(
            EntityId::new(format!("observation:{index}")).unwrap(),
            "observation",
            "benchmark observation",
        )
        .unwrap();
        pending.push(
            NewEvent::new(
                EventId::new(format!("event:{index}")).unwrap(),
                "node_upserted",
                Timestamp::from_unix_micros(timestamp),
                Timestamp::from_unix_micros(timestamp),
                agent.clone(),
                session.clone(),
                MemoryEvent::NodeUpserted { node },
            )
            .unwrap(),
        );
    }
    let stream = StreamId::new("benchmark").unwrap();
    let mut store = InMemoryStore::default();
    store
        .append(&stream, ExpectedVersion::NoStream, &pending)
        .unwrap()
}

fn report(count: usize, median: Duration) {
    let rate =
        u128::try_from(count).expect("usize fits u128") * 1_000_000_000 / median.as_nanos().max(1);
    println!(
        "replay events={count} median_ms={:.3} events_per_second={rate:.0}",
        median.as_secs_f64() * 1_000.0
    );
}
