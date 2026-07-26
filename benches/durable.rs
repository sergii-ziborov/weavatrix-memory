use std::{
    fs,
    hint::black_box,
    path::PathBuf,
    time::{Duration, Instant},
};
use weavatrix_memory::{
    AgentId, Durability, EntityId, EventId, EventStore, ExpectedVersion, FileEventStore,
    FileStoreOptions, JsonCodec, MemoryEvent, MemoryNode, MemoryProjection, NewEvent, SessionId,
    StreamId, Timestamp, replay,
};

fn main() {
    let count = std::env::var("WEAVATRIX_BENCH_EVENTS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(10_000);
    let pending = fixture(count);
    let mut append_samples = Vec::new();
    let mut reopen_samples = Vec::new();
    let mut replay_samples = Vec::new();
    for sample in 0..5 {
        let path = benchmark_path(sample);
        let _ = fs::remove_file(&path);
        let mut store = FileEventStore::open(&path, JsonCodec, durable_options()).unwrap();
        let stream = StreamId::new("benchmark").unwrap();
        let events = pending.clone();
        let started = Instant::now();
        store
            .append_owned(&stream, ExpectedVersion::NoStream, events)
            .unwrap();
        append_samples.push(started.elapsed());
        drop(store);

        let started = Instant::now();
        let reopened =
            FileEventStore::<MemoryEvent, _>::open(&path, JsonCodec, durable_options()).unwrap();
        reopen_samples.push(started.elapsed());
        let events = reopened.load_all(None, usize::MAX);

        let started = Instant::now();
        black_box(replay::<_, MemoryProjection>(&events).unwrap());
        replay_samples.push(started.elapsed());
        drop(reopened);
        fs::remove_file(path).unwrap();
    }
    report("durable_append", count, median(&mut append_samples));
    report("reopen_index", count, median(&mut reopen_samples));
    report("projection_replay", count, median(&mut replay_samples));
}

fn fixture(count: usize) -> Vec<NewEvent<MemoryEvent>> {
    let agent = AgentId::new("bench-agent").unwrap();
    let session = SessionId::new("bench-session").unwrap();
    (0..count)
        .map(|index| {
            let timestamp = i64::try_from(index).expect("benchmark count fits i64");
            let node = MemoryNode::new(
                EntityId::new(format!("observation:{index}")).unwrap(),
                "observation",
                "benchmark observation",
            )
            .unwrap();
            NewEvent::new(
                EventId::new(format!("event:{index}")).unwrap(),
                "node_upserted",
                Timestamp::from_unix_micros(timestamp),
                Timestamp::from_unix_micros(timestamp),
                agent.clone(),
                session.clone(),
                MemoryEvent::NodeUpserted { node },
            )
            .unwrap()
        })
        .collect()
}

fn durable_options() -> FileStoreOptions {
    FileStoreOptions {
        durability: Durability::SyncData,
        ..FileStoreOptions::default()
    }
}

fn benchmark_path(sample: usize) -> PathBuf {
    std::env::temp_dir().join(format!(
        "weavatrix-memory-durable-bench-{}-{sample}.wmem",
        std::process::id()
    ))
}

fn median(samples: &mut [Duration]) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn report(name: &str, count: usize, median: Duration) {
    let rate =
        u128::try_from(count).expect("usize fits u128") * 1_000_000_000 / median.as_nanos().max(1);
    println!(
        "{name} events={count} median_ms={:.3} events_per_second={rate}",
        median.as_secs_f64() * 1_000.0
    );
}
