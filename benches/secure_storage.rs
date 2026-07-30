use std::{collections::BTreeMap, fs, hint::black_box, path::PathBuf};
#[path = "support/secure_storage.rs"]
mod support;
use support::{
    BytesCodec, Samples, elapsed, env_usize, median, record, report, report_codecs, sample_codec,
};
use weavatrix_memory::{
    AgentId, Codec, CompactSnapshotCodec, Durability, EntityId, Evidence, FactId,
    FileSnapshotStore, Lz4Codec, MemoryFact, MemoryNode, MemoryProjection, ProjectionSnapshot,
    ReplayCursor, SessionId, SnapshotOptions, SnapshotStore, StaticKey, StreamId, Timestamp,
    XChaCha20Codec,
};

const MAX_BYTES: usize = 512 * 1024 * 1024;

struct BenchmarkCodecs {
    lz4: Lz4Codec<BytesCodec>,
    encrypted: XChaCha20Codec<BytesCodec, StaticKey>,
    secure: XChaCha20Codec<Lz4Codec<BytesCodec>, StaticKey>,
}

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
    let codecs = codecs();
    let lz4_bytes = codecs.lz4.encode(&compact_bytes).unwrap();
    let encrypted_bytes = codecs.encrypted.encode(&compact_bytes).unwrap();
    let secure_bytes = codecs.secure.encode(&compact_bytes).unwrap();

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
                &codecs.lz4,
                &compact_bytes,
                &lz4_bytes,
                &mut lz4_samples,
            );
            sample_codec(
                iteration,
                &codecs.encrypted,
                &compact_bytes,
                &encrypted_bytes,
                &mut encrypted_samples,
            );
            sample_codec(
                iteration,
                &codecs.secure,
                &compact_bytes,
                &secure_bytes,
                &mut secure_samples,
            );
        } else {
            sample_codec(
                iteration,
                &codecs.secure,
                &compact_bytes,
                &secure_bytes,
                &mut secure_samples,
            );
            sample_codec(
                iteration,
                &codecs.encrypted,
                &compact_bytes,
                &encrypted_bytes,
                &mut encrypted_samples,
            );
            sample_codec(
                iteration,
                &codecs.lz4,
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
    report_codecs(
        node_count,
        edge_count,
        [&compact_bytes, &lz4_bytes, &encrypted_bytes, &secure_bytes],
        [
            (&mut copy_samples, "copy"),
            (&mut lz4_samples, "lz4"),
            (&mut encrypted_samples, "xchacha20"),
            (&mut secure_samples, "lz4_xchacha20"),
        ],
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

fn codecs() -> BenchmarkCodecs {
    BenchmarkCodecs {
        lz4: Lz4Codec::new(BytesCodec, MAX_BYTES).unwrap(),
        encrypted: encryption_codec(BytesCodec),
        secure: encryption_codec(Lz4Codec::new(BytesCodec, MAX_BYTES).unwrap()),
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
