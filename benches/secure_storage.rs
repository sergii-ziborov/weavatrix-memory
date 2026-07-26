use std::{
    collections::BTreeMap,
    fs,
    hint::black_box,
    path::PathBuf,
    time::{Duration, Instant},
};
use weavatrix_memory::{
    AgentId, Codec, CompactSnapshotCodec, Durability, EntityId, Evidence, FactId,
    FileSnapshotStore, Lz4Codec, MemoryFact, MemoryNode, MemoryProjection, ProjectionSnapshot,
    ReplayCursor, SessionId, SnapshotOptions, SnapshotStore, StaticKey, StreamId, Timestamp,
    XChaCha20Codec,
};

const MAX_BYTES: usize = 512 * 1024 * 1024;

fn main() {
    let node_count = env_usize("WEAVATRIX_BENCH_NODES", 10_000);
    let edges_per_node = env_usize("WEAVATRIX_BENCH_EDGES_PER_NODE", 3);
    let edge_count = node_count * edges_per_node;
    let position = u64::try_from(node_count + edge_count - 1).unwrap();
    let snapshot = ProjectionSnapshot {
        cursor: ReplayCursor {
            global_position: Some(position),
            stream_versions: BTreeMap::from([(
                StreamId::new("secure-storage-benchmark").unwrap(),
                position,
            )]),
        },
        projection: fixture(node_count, edges_per_node, position),
    };
    let compact_bytes = CompactSnapshotCodec.encode(&snapshot).unwrap();
    let lz4 = Lz4Codec::new(BytesCodec, MAX_BYTES).unwrap();
    let encrypted = encryption_codec(BytesCodec);
    let secure = encryption_codec(Lz4Codec::new(BytesCodec, MAX_BYTES).unwrap());
    let lz4_bytes = lz4.encode(&compact_bytes).unwrap();
    let encrypted_bytes = encrypted.encode(&compact_bytes).unwrap();
    let secure_bytes = secure.encode(&compact_bytes).unwrap();

    let mut copy_samples = Samples::default();
    let mut lz4_samples = Samples::default();
    let mut encrypted_samples = Samples::default();
    let mut secure_samples = Samples::default();
    for iteration in 0..11 {
        if iteration % 2 == 0 {
            sample_codec(
                iteration,
                &BytesCodec,
                &compact_bytes,
                &compact_bytes,
                &mut copy_samples,
            );
            sample_codec(
                iteration,
                &lz4,
                &compact_bytes,
                &lz4_bytes,
                &mut lz4_samples,
            );
            sample_codec(
                iteration,
                &encrypted,
                &compact_bytes,
                &encrypted_bytes,
                &mut encrypted_samples,
            );
            sample_codec(
                iteration,
                &secure,
                &compact_bytes,
                &secure_bytes,
                &mut secure_samples,
            );
        } else {
            sample_codec(
                iteration,
                &secure,
                &compact_bytes,
                &secure_bytes,
                &mut secure_samples,
            );
            sample_codec(
                iteration,
                &encrypted,
                &compact_bytes,
                &encrypted_bytes,
                &mut encrypted_samples,
            );
            sample_codec(
                iteration,
                &lz4,
                &compact_bytes,
                &lz4_bytes,
                &mut lz4_samples,
            );
            sample_codec(
                iteration,
                &BytesCodec,
                &compact_bytes,
                &compact_bytes,
                &mut copy_samples,
            );
        }
    }
    copy_samples.report("copy");
    lz4_samples.report("lz4");
    encrypted_samples.report("xchacha20");
    secure_samples.report("lz4_xchacha20");
    println!(
        "secure_storage_sizes nodes={node_count} edges={edge_count} compact_bytes={} lz4_bytes={} encrypted_bytes={} secure_bytes={}",
        compact_bytes.len(),
        lz4_bytes.len(),
        encrypted_bytes.len(),
        secure_bytes.len()
    );
    benchmark_snapshot_reads(&snapshot);
}

fn encryption_codec<C>(inner: C) -> XChaCha20Codec<C, StaticKey> {
    XChaCha20Codec::new(
        inner,
        StaticKey::new("benchmark", [7; 32]).unwrap(),
        b"projection-snapshot",
        MAX_BYTES,
    )
    .unwrap()
}

#[derive(Default)]
struct Samples {
    encode: Vec<Duration>,
    decode: Vec<Duration>,
}

impl Samples {
    fn report(&mut self, name: &str) {
        report(&format!("{name}_encode"), median(&mut self.encode));
        report(&format!("{name}_decode"), median(&mut self.decode));
    }
}

fn sample_codec<C>(
    iteration: usize,
    codec: &C,
    value: &Vec<u8>,
    bytes: &[u8],
    samples: &mut Samples,
) where
    C: Codec<Vec<u8>>,
{
    record(
        iteration,
        &mut samples.encode,
        elapsed(|| black_box(codec.encode(value).unwrap())),
    );
    record(
        iteration,
        &mut samples.decode,
        elapsed(|| black_box(codec.decode(bytes).unwrap())),
    );
}

#[derive(Clone, Copy)]
struct BytesCodec;

impl Codec<Vec<u8>> for BytesCodec {
    fn encode(&self, value: &Vec<u8>) -> weavatrix_memory::Result<Vec<u8>> {
        Ok(value.clone())
    }

    fn decode(&self, bytes: &[u8]) -> weavatrix_memory::Result<Vec<u8>> {
        Ok(bytes.to_vec())
    }
}

fn benchmark_snapshot_reads(snapshot: &ProjectionSnapshot<MemoryProjection>) {
    let directory = temporary_directory();
    let options = SnapshotOptions {
        durability: Durability::Flush,
        ..SnapshotOptions::default()
    };
    let mut buffered =
        FileSnapshotStore::open(&directory, "context", CompactSnapshotCodec, options).unwrap();
    buffered.save(snapshot).unwrap();
    let mapped = FileSnapshotStore::open(&directory, "context", CompactSnapshotCodec, options)
        .unwrap()
        .with_memory_mapped_reads();
    let mut buffered_samples = Vec::new();
    let mut mapped_samples = Vec::new();
    for iteration in 0..11 {
        let (buffered_time, mapped_time) = if iteration % 2 == 0 {
            (
                elapsed(|| black_box(buffered.load_latest().unwrap())),
                elapsed(|| black_box(mapped.load_latest().unwrap())),
            )
        } else {
            let mapped_time = elapsed(|| black_box(mapped.load_latest().unwrap()));
            let buffered_time = elapsed(|| black_box(buffered.load_latest().unwrap()));
            (buffered_time, mapped_time)
        };
        record(iteration, &mut buffered_samples, buffered_time);
        record(iteration, &mut mapped_samples, mapped_time);
    }
    report("snapshot_buffered_load", median(&mut buffered_samples));
    report("snapshot_mmap_load", median(&mut mapped_samples));
    fs::remove_dir_all(directory).unwrap();
}

fn fixture(nodes: usize, edges_per_node: usize, position: u64) -> MemoryProjection {
    let agent = AgentId::new("storage-agent").unwrap();
    let session = SessionId::new("storage-session").unwrap();
    let evidence = Evidence::new("benchmark", "secure-storage").unwrap();
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

fn temporary_directory() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "weavatrix-memory-storage-bench-{}",
        std::process::id()
    ));
    if path.exists() {
        fs::remove_dir_all(&path).unwrap();
    }
    fs::create_dir(&path).unwrap();
    path
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
