use std::{
    collections::BTreeMap,
    hint::black_box,
    time::{Duration, Instant},
};
use weavatrix_memory::{
    AgentId, Codec, CompactSnapshotCodec, EntityId, Evidence, FactId, JsonCodec, MemoryFact,
    MemoryNode, MemoryProjection, ProjectionSnapshot, ReplayCursor, SessionId, StreamId, Timestamp,
};

fn main() {
    let node_count = env_usize("WEAVATRIX_BENCH_NODES", 10_000);
    let edges_per_node = env_usize("WEAVATRIX_BENCH_EDGES_PER_NODE", 3);
    let edge_count = node_count * edges_per_node;
    let position = u64::try_from(node_count + edge_count - 1).unwrap();
    let snapshot = ProjectionSnapshot {
        cursor: ReplayCursor {
            global_position: Some(position),
            stream_versions: BTreeMap::from([(
                StreamId::new("snapshot-benchmark").unwrap(),
                position,
            )]),
        },
        projection: fixture(node_count, edges_per_node, position),
    };
    let compact = CompactSnapshotCodec.encode(&snapshot).unwrap();
    let json = JsonCodec.encode(&snapshot).unwrap();
    let mut compact_encode = Vec::new();
    let mut compact_decode = Vec::new();
    let mut json_encode = Vec::new();
    let mut json_decode = Vec::new();
    for iteration in 0..11 {
        record(
            iteration,
            &mut compact_encode,
            elapsed(|| black_box(CompactSnapshotCodec.encode(&snapshot).unwrap())),
        );
        record(
            iteration,
            &mut compact_decode,
            elapsed(|| black_box(CompactSnapshotCodec.decode(&compact).unwrap())),
        );
        record(
            iteration,
            &mut json_encode,
            elapsed(|| black_box(JsonCodec.encode(&snapshot).unwrap())),
        );
        record(
            iteration,
            &mut json_decode,
            elapsed(|| {
                let decoded: ProjectionSnapshot<MemoryProjection> =
                    JsonCodec.decode(&json).unwrap();
                black_box(decoded)
            }),
        );
    }
    report("compact_encode", median(&mut compact_encode));
    report("compact_decode", median(&mut compact_decode));
    report("json_encode", median(&mut json_encode));
    report("json_decode", median(&mut json_decode));
    println!(
        "snapshot_sizes nodes={node_count} edges={edge_count} compact_bytes={} json_bytes={} ratio={:.3}",
        compact.len(),
        json.len(),
        f64::from(u32::try_from(compact.len()).unwrap())
            / f64::from(u32::try_from(json.len()).unwrap())
    );
}

fn fixture(nodes: usize, edges_per_node: usize, position: u64) -> MemoryProjection {
    let agent = AgentId::new("snapshot-agent").unwrap();
    let session = SessionId::new("snapshot-session").unwrap();
    let evidence = Evidence::new("benchmark", "snapshot-codecs").unwrap();
    let nodes = (0..nodes)
        .map(|index| {
            MemoryNode::new(
                EntityId::new(format!("node:{index}")).unwrap(),
                "observation",
                format!("snapshot node {index}"),
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let mut facts = Vec::with_capacity(nodes.len() * edges_per_node);
    for source in 0..nodes.len() {
        for offset in 1..=edges_per_node {
            let target = (source + offset) % nodes.len();
            facts.push(
                MemoryFact::new(
                    FactId::new(format!("fact:{source}:{target}")).unwrap(),
                    nodes[source].id.clone(),
                    "supports",
                    nodes[target].id.clone(),
                    Timestamp::from_unix_micros(1),
                    Timestamp::from_unix_micros(1),
                    agent.clone(),
                    session.clone(),
                    evidence.clone(),
                )
                .unwrap(),
            );
        }
    }
    MemoryProjection::try_from_parts(nodes, facts, Timestamp::from_unix_micros(1), Some(position))
        .unwrap()
}

fn elapsed<T>(operation: impl FnOnce() -> T) -> Duration {
    let started = Instant::now();
    black_box(operation());
    started.elapsed()
}

fn record(iteration: usize, samples: &mut Vec<Duration>, value: Duration) {
    if iteration >= 2 {
        samples.push(value);
    }
}

fn median(samples: &mut [Duration]) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn report(name: &str, median: Duration) {
    println!("{name} median_ms={:.3}", median.as_secs_f64() * 1_000.0);
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}
