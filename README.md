# Weavatrix Memory

`weavatrix-memory` is an event-sourced, bitemporal context compiler for coding
agents. It turns immutable code, task, decision, test, failure, and handoff
events into small evidence-carrying graphs for a specific task and token budget.

The crate is a standalone MIT-licensed module of Weavatrix Rust Core. Its
deterministic core does not require an LLM, vector database, external graph
database, async runtime, system clock, or Git executable. Durable storage is
available without requiring a database.

## Why it exists

Generic agent memory usually answers “what looks similar?” Engineering work
also needs exact answers to:

- What was true on this branch at that time?
- What did the agent know when it made the decision?
- Was this approach already tried and why did it fail?
- Which commit, symbol, test, benchmark, or user correction supports the fact?
- What is the smallest reproducible evidence graph that fits this context
  window?

Weavatrix Memory keeps the immutable history and derives a fresh graph for each
query instead of mutating one opaque “current truth”.

## Current guarantees

- Atomic per-stream append.
- Owned-batch append path that avoids cloning caller-owned payloads.
- Optimistic concurrency with `NoStream`, `Exact`, and `Any` expectations.
- Globally ordered cursors and independently versioned streams.
- Globally unique event identifiers.
- Strict replay validation for cursor gaps and stream-version gaps.
- Framed filesystem journal with CRC32C corruption detection.
- Strict open by default and explicit recovery for incomplete trailing batches.
- Immutable generation-named projection snapshots and validated resume.
- Explicit-ack catch-up subscriptions with redelivery before acknowledgement.
- Caller-provided timestamps and identifiers for reproducible tests.
- Separate valid-time and known-time queries.
- Evidence required for every memory relation and retraction.
- Explicit supersession without deleting historical facts.
- Validated bulk projection with compact dual-CSR incident indexes.
- Deterministic projection into canonical `weavatrix-graph` snapshots.
- Provider-neutral literal, lexical, semantic, and hybrid retrieval with
  deterministic reciprocal-rank fusion.
- Provider-neutral auto-extraction with strict, scope-aware entity linking and
  idempotent event plans.
- Hard context budget with a replaceable token estimator and compilation
  receipt.
- Repository and branch-scoped projections.
- Dependency-free, versioned compact binary projection snapshots.
- Optional size-bounded LZ4 envelopes that keep incompressible input raw.
- Optional XChaCha20-Poly1305 authenticated encryption with key identifiers,
  purpose-bound AAD, and OS-generated nonces.
- Optional guarded mmap reads for immutable snapshot generations.
- Belief revision, reasoning-gap, drift, and consolidation analysis over the
  canonical graph.
- Exact-evidence retrieval metrics and adapters for public memory benchmarks.

## Architecture

```text
source / AST / issue / agent observation
        |
        v
ExtractionProvider -> strict EntityLinker -> reviewed event plan
        |
        v
append-only events
        |
        v
in-memory or framed file event store
        |
        v
strict replay validator + optional snapshot resume
        |
        v
bitemporal memory projection
        |
        +<-- lexical / semantic RetrievalProvider
        |
        v
immutable weavatrix-graph snapshot
        |
        +--> belief / gap / drift / consolidation reports
        |
        v
budgeted context compiler + receipt
```

The in-memory and filesystem stores implement the same `EventStore` contract.
Database, Git, lexical, semantic, and MCP adapters belong behind separate
interfaces or higher-level Weavatrix Rust Core modules.

Use `append` when the pending events must remain available to the caller. Use
`append_owned` to transfer a batch into the store without cloning its input
payloads.

## Durable storage

`FileEventStore` writes each append batch as one checksummed frame. Memory state
is updated only after the complete frame has been written and flushed or synced.
On reopen, the journal rebuilds and validates global positions, stream versions,
and event identifiers.

The default `RecoveryPolicy::Strict` rejects any truncated tail. Explicit
`TruncatePartialTail` recovery removes only an incomplete final batch. Invalid
headers, impossible sizes, malformed payloads, and checksum failures are never
silently repaired.

The file store is intentionally single-writer. It detects file-length changes
made by another active handle, but it does not claim cross-process locking.
Applications needing multiple writers should provide a database-backed
`EventStore`.

Serialization is injected through the `Codec<T>` trait. The crate has no
default codec, compression, encryption, or mmap dependency. Storage features
are opt-in:

| Feature | Adds |
| --- | --- |
| `json` | `JsonCodec` |
| `compression` | Size-bounded `Lz4Codec<C>` |
| `encryption` | `XChaCha20Codec<C, K>` and key-provider contracts |
| `mmap` | Guarded read-only snapshot mapping |
| `secure-storage` | `compression`, `encryption`, and `mmap` |

```toml
[dependencies]
weavatrix-memory = { version = "0.2", features = ["secure-storage"] }
```

`FileSnapshotStore` writes immutable, position-named snapshots through a
temporary file and atomic rename. `replay_tracked` produces the exact cursor;
`resume` rejects any gap between that cursor and the supplied event tail.
`CompactSnapshotCodec` stores the complete replay cursor, temporal revisions,
facts, supersessions, and retractions in a bounds-checked versioned binary
format. Lookup and CSR indexes are derived and validated during decode rather
than serialized redundantly.

Codec wrappers compose in encode order. Compress before encrypting:

```rust,ignore
let codec = XChaCha20Codec::new(
    Lz4Codec::new(CompactSnapshotCodec, 512 * 1024 * 1024)?,
    StaticKey::new("2026-q3", key_from_secret_manager)?,
    b"projection-snapshot",
    512 * 1024 * 1024,
)?;
let snapshots = FileSnapshotStore::open(
    directory,
    "context",
    codec,
    SnapshotOptions::default(),
)?
.with_memory_mapped_reads();
```

`Lz4Codec` records the original length and rejects it before allocation when it
exceeds the configured limit. It stores raw bytes when compression would grow
the payload. `XChaCha20Codec` authenticates the envelope header and caller
context as AAD; the key identifier remains visible so an `EncryptionKeys`
provider can retain old decryption keys during rotation. It is a raw-key API,
not a password KDF.

`StaticKey` zeroizes its owned 256-bit key on drop, and the encryption wrapper
zeroizes temporary plaintext buffers after encode and decode. Applications must
still source keys from a secret manager or KMS, protect any copies made before
construction, and never use a deterministic `NonceSource` outside tests.
Authentication failures, wrong contexts, unavailable keys, malformed envelopes,
and oversized plaintexts fail closed.

Memory mapping is opt-in because it is a workload tradeoff, not a universal
speedup. Snapshot generations created by this store are never overwritten.
The mmap adapter holds a shared advisory lock, but a non-cooperating external
process can still truncate a mapped file; all writers must honor the lock and
immutable-generation contract.

`CatchUpSubscription` does not advance its checkpoint during `poll`. Consumers
must explicitly acknowledge a delivered position, so a failed handler receives
the same events again.

## Example

```rust
use weavatrix_memory::{
    AgentId, ContextCompiler, ContextRequest, EntityId, EventId, EventStore,
    Evidence, ExpectedVersion, FactId, InMemoryStore, MemoryEvent, MemoryFact,
    MemoryNode, MemoryProjection, NewEvent, SessionId, StreamId, Timestamp,
    replay,
};

fn at(value: i64) -> Timestamp {
    Timestamp::from_unix_micros(value)
}

let agent = AgentId::new("agent:codex")?;
let session = SessionId::new("session:714")?;
let task = EntityId::new("task:714")?;
let file = EntityId::new("file:query-builder")?;
let recorded_at = at(20);

let nodes = [
    MemoryNode::new(task.clone(), "task", "Fix one-day query accuracy")?,
    MemoryNode::new(file.clone(), "file", "query-builder.rs")?,
];
let fact = MemoryFact::new(
    FactId::new("fact:714:affected")?,
    task.clone(),
    "affects",
    file,
    at(10),
    recorded_at,
    agent.clone(),
    session.clone(),
    Evidence::new("test", "query-accuracy-suite")?,
)?;

let payloads = vec![
    MemoryEvent::NodeUpserted { node: nodes[0].clone() },
    MemoryEvent::NodeUpserted { node: nodes[1].clone() },
    MemoryEvent::FactRecorded { fact },
];
let pending = payloads
    .into_iter()
    .enumerate()
    .map(|(index, payload)| {
        let event_type = payload.event_type();
        NewEvent::new(
            EventId::new(format!("event:{index}"))?,
            event_type,
            recorded_at,
            recorded_at,
            agent.clone(),
            session.clone(),
            payload,
        )
    })
    .collect::<Result<Vec<_>, weavatrix_memory::MemoryError>>()?;

let stream = StreamId::new("task:714")?;
let mut store = InMemoryStore::default();
store.append(&stream, ExpectedVersion::NoStream, &pending)?;
let projection: MemoryProjection = replay(&store.load_all(None, usize::MAX))?;

let request = ContextRequest::new(vec![task], at(30), at(30), 2_000)?;
let bundle = ContextCompiler::default().compile(&projection, &request)?;
assert_eq!(bundle.graph.edge_count(), 1);
assert!(bundle.receipt.estimated_tokens <= 2_000);

# Ok::<(), weavatrix_memory::MemoryError>(())
```

## Bitemporal semantics

`valid_at` asks when a fact was true in the modeled world. `known_at` asks what
the system had recorded by a given moment. A correction recorded today can
replace a fact from last month without rewriting what an agent knew yesterday.

Facts retain:

- valid interval;
- observation and recording times;
- agent and session identities;
- confidence in basis points;
- one or more evidence records;
- the fact they supersede, when applicable.

## Auto-extraction and entity linking

`ExtractionProvider` isolates parsing or model inference from the deterministic
memory core. Providers return typed local mentions, relation candidates,
confidence, optional byte spans, stable IDs, external IDs, and candidate hints.
`AutoExtractionEngine` then:

- validates UTF-8 spans, temporal intervals, identifiers, and provider identity;
- links by stable ID, external ID, scoped normalized label, alias, or provider
  hint;
- applies an explicit minimum score and winner margin;
- reports ambiguous and unresolved endpoints instead of guessing;
- creates deterministic node and fact IDs for unmatched entities;
- preserves source, locator, digest, provider, span, agent, session, and
  confidence provenance;
- returns a non-mutating event plan suitable for review and atomic append.

`EntityLinker` can be built once from a temporal `MemoryView` and reused for a
batch. This keeps AST, issue-tracker, model, and future semantic adapters outside
the core while giving all of them the same validation and linking contract.

## Context compilation

`ContextCompiler` can start from exact entity identifiers or from one or more
`RetrievalProvider` implementations. A provider returns exact entity IDs from
literal, lexical, semantic, or hybrid search. Integer reciprocal-rank fusion
combines their ranks without pretending BM25 and vector scores share a scale;
the result retains provider, channel, rank, and raw-score provenance.

After seed resolution, the compiler traverses selected relations in both
directions, ranks nearby evidence deterministically, and stops before exceeding
the configured budget. The receipt records:

- projection time and source event position;
- estimator identity and estimated usage;
- examined and selected fact counts;
- omissions caused by budget;
- facts excluded by repository or branch scope.

The built-in byte estimator is deterministic and dependency-free. Applications
that need model-exact counts implement the small `TokenEstimator` trait.

## Memory analytics

`MemoryAnalytics` operates on bitemporal projections and canonical
`weavatrix-graph` topology:

- belief revision finds explicit corrections and competing targets, then
  traces their downstream confidence cascade;
- reasoning-gap analysis reports unsupported decisions, single-source
  inferences, weak foundations, stale evidence, and unstable revision chains;
- drift reconstructs immutable belief timelines and correction rates;
- consolidation returns a deterministic plan for duplicate supersession,
  orphan review, and revision checkpoints without deleting history.

## Accuracy evaluation

The provider-neutral evaluator reports Hit@K, Recall@K, nDCG@K, and MRR from
exact evidence identifiers. The repository includes adapters for the public
[LoCoMo](https://github.com/snap-research/locomo) and
[LongMemEval](https://github.com/xiaowu0162/LongMemEval) formats plus a
coding-agent regression suite. See [benchmark instructions](benchmarks/README.md).

The full official files were parsed successfully in the local verification
run: 1,978 evidence-bearing `LoCoMo` questions and 470 non-abstention
LongMemEval-S questions. The dependency-free literal smoke baseline produced:

| Dataset | Hit@1 | Hit@5 | Recall@5 | MRR |
| --- | ---: | ---: | ---: | ---: |
| `LoCoMo` | 0.2230 | 0.3918 | 0.3559 | 0.2980 |
| LongMemEval-S cleaned | 0.6787 | 0.8894 | 0.7883 | 0.7679 |
| Coding-agent v1 (7 cases) | 0.8571 | 1.0000 | 1.0000 | 0.9286 |

These are adapter smoke results, not claims about the future
`weavatrix-search` or semantic/vector provider.

## Benchmarks

The repository contains executable, median-based benchmarks rather than copied
one-off timings. On an Intel Core Ultra 7 255U, Windows 11, Rust 1.97.1,
`--release`, a 100,000-event run produced:

| Contract | Median | Throughput |
| --- | ---: | ---: |
| In-memory evidence append + load | 112.936 ms | 885,459 events/s |
| `cqrs-es` 0.5.0 evidence append + load | 186.668 ms | 535,709 events/s |
| CRC32C JSON append + `sync_data` | 438.244 ms | 228,183 events/s |
| Durable reopen + index validation | 383.401 ms | 260,823 events/s |
| Bitemporal projection replay | 129.592 ms | 771,654 events/s |

The competitor workload carries the same event identifier, event type,
occurred/recorded timestamps, and agent/session identities. Weavatrix
additionally checks identifier uniqueness and optimistic concurrency and
assigns a global cursor. Each side performs append followed by a cloned stream
load. Fixtures are created outside the timed region; nine samples are measured
after two warmups and the median is reported. Under this evidence-equivalent
contract, Weavatrix used 39.5% less time than `cqrs-es` in this run. These are
local measurements, not universal hardware claims.

The graph-memory harness also compares against `agentic-memory` 0.4.2. At
100,000 nodes and 300,000 edges:

| Contract | Weavatrix | `agentic-memory` | Result |
| --- | ---: | ---: | --- |
| Depth-2 context, identical 13-node/33-edge output | 0.293 ms | 6.750 ms | Weavatrix 23.0x faster |
| Validated `try_from_parts` + dual CSR | 133.915 ms | 93.103 ms | `agentic-memory` 1.44x faster |
| Strict replay of 400,000 envelopes | 1,248.583 ms | n/a | Different contract |

The context row is output-equivalent. The bulk-construction row compares each
crate's parts constructor, but the contracts are still not identical:
Weavatrix validates node and fact domains, evidence, uniqueness, endpoints, and
supersession before building both CSR directions. The harness records that
`agentic-memory::MemoryGraph::from_parts` accepts a dangling edge. The new bulk
path reduced the measured construction gap from 18.2x to 1.44x without dropping
those checks; strict event replay remains a separately reported operation.

At 10,000 nodes and 30,000 facts, the versioned compact snapshot codec measured:

| Contract | Compact binary | JSON | Result |
| --- | ---: | ---: | --- |
| Encode | 11.647 ms | 36.878 ms | Compact 3.17x faster |
| Decode and validate indexes | 121.508 ms | 186.702 ms | Compact 1.54x faster |
| Snapshot size | 3,824,378 bytes | 9,543,533 bytes | Compact 59.9% smaller |

Both decoders restore the same projection and rebuild validated lookup and CSR
indexes. The benchmark reports nine samples after two warmups.

The secure-storage harness first serializes a 100,000-node, 300,000-fact
projection into 38,828,001 compact bytes, then measures only the byte transform.
The copy row is the `Vec` allocation/copy baseline:

| Transform | Encode | Decode | Stored bytes |
| --- | ---: | ---: | ---: |
| Copy only | 9.649 ms | 9.594 ms | 38,828,001 |
| LZ4 | 32.216 ms | 27.322 ms | 5,249,623 |
| XChaCha20-Poly1305 | 70.923 ms | 71.745 ms | 38,828,059 |
| LZ4 then XChaCha20-Poly1305 | 51.154 ms | 37.384 ms | 5,249,681 |

For this repetitive evidence fixture, LZ4 reduced the snapshot by 86.5%.
Authenticated encryption added 58 bytes. The combined path encrypts only the
compressed payload; it remained 86.5% smaller than compact binary alone.

Snapshot loads include directory selection, frame bounds and CRC32C checks,
complete projection decode, and index validation. Buffered and mmap order is
alternated on every sample:

| Projection | Snapshot bytes | Buffered load | mmap load | Result |
| --- | ---: | ---: | ---: | --- |
| 10,000 nodes / 30,000 facts | 3,734,384 | 67.870 ms | 65.313 ms | mmap 3.8% faster |
| 100,000 nodes / 300,000 facts | 38,828,001 | 709.873 ms | 730.248 ms | mmap 2.9% slower |

The mmap path removes the encoded-payload heap allocation and copy, but its
mapping, locking, and page-fault overhead kept both cached local loads in the
same performance range and changed which path won. Buffered reads therefore
remain the default; mmap is for reducing peak heap and enabling large
immutable-file access, not a claimed speedup. Both tables report nine samples
after two warmups, with transform and read order alternated between samples.

The extraction harness indexes a 100,000-entity catalog containing label, alias,
and external-ID keys, then resolves 10,000 mentions:

| Contract | Median | Throughput |
| --- | ---: | ---: |
| Catalog build, 100,000 entities | 449.895 ms | 222,274 entities/s |
| Reused indexed linker, 10,000 mentions | 36.683 ms | 272,604 links/s |
| Validated extraction event plan, 10,000 mentions | 69.444 ms | 144,001 mentions/s |

Provider output and fixtures are created outside the indexed-link timing. Each
row reports the median of nine samples after two warmups.

## Development

```console
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features --lib --tests
cargo audit
cargo bench --bench replay
cargo bench --features json --bench durable
cargo bench --bench event_store_competitors
cargo bench --bench memory_competitors
cargo bench --features json --bench snapshot_codecs
cargo bench --features secure-storage --bench secure_storage
cargo bench --bench extraction
cargo run --release --all-features --bin weavatrix-memory-eval
```

Set `WEAVATRIX_BENCH_EVENTS` to change any workload. The in-memory replay
benchmark runs two warmups and reports nine measured iterations. The durable
benchmark reports five isolated append, reopen/index, and projection samples.
The competitor benchmarks report nine samples after two warmups. Set
`WEAVATRIX_BENCH_NODES` and `WEAVATRIX_BENCH_EDGES_PER_NODE` for the graph
workload.

## Status

The public API is experimental before `1.0`. The filesystem journal and
snapshots are local embedded stores, not a distributed database. Cross-process
writer coordination, ACL policies, Git history, MCP tools, compaction, and
database adapters remain separate layers.

## License

MIT
