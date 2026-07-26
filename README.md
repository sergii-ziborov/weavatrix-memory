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
- Deterministic projection into canonical `weavatrix-graph` snapshots.
- Hard context budget with a replaceable token estimator and compilation
  receipt.
- Repository and branch-scoped projections.

## Architecture

```text
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
        v
immutable weavatrix-graph snapshot
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
default codec dependency. Enabling the optional `json` feature exposes
`JsonCodec`:

```toml
[dependencies]
weavatrix-memory = { version = "0.1", features = ["json"] }
```

`FileSnapshotStore` writes immutable, position-named snapshots through a
temporary file and atomic rename. `replay_tracked` produces the exact cursor;
`resume` rejects any gap between that cursor and the supplied event tail.

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

## Context compilation

`ContextCompiler` starts from exact entity identifiers, traverses the selected
relations in both directions, ranks nearby evidence deterministically, and
stops before exceeding the configured budget. The receipt records:

- projection time and source event position;
- estimator identity and estimated usage;
- examined and selected fact counts;
- omissions caused by budget;
- facts excluded by repository or branch scope.

The built-in byte estimator is deterministic and dependency-free. Applications
that need model-exact counts implement the small `TokenEstimator` trait.

## Benchmarks

The repository contains executable, median-based benchmarks rather than copied
one-off timings. On an Intel Core Ultra 7 255U, Windows 11, Rust 1.97.1,
`--release`, a 100,000-event run produced:

| Contract | Median | Throughput |
| --- | ---: | ---: |
| In-memory evidence append + load | 44.915 ms | 2,226,427 events/s |
| `cqrs-es` 0.5.0 evidence append + load | 63.572 ms | 1,573,029 events/s |
| CRC32C JSON append + `sync_data` | 438.244 ms | 228,183 events/s |
| Durable reopen + index validation | 383.401 ms | 260,823 events/s |
| Bitemporal projection replay | 129.592 ms | 771,654 events/s |

The competitor workload carries the same event identifier, event type,
occurred/recorded timestamps, and agent/session identities. Weavatrix
additionally checks identifier uniqueness and optimistic concurrency and
assigns a global cursor. Each side performs append followed by a cloned stream
load. Fixtures are created outside the timed region; nine samples are measured
after two warmups and the median is reported. Under this evidence-equivalent
contract, Weavatrix used 29.3% less time than `cqrs-es` in this run. These are
local measurements, not universal hardware claims.

## Development

```console
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features --lib --tests
cargo bench --bench replay
cargo bench --features json --bench durable
cargo bench --bench event_store_competitors
```

Set `WEAVATRIX_BENCH_EVENTS` to change any workload. The in-memory replay
benchmark runs two warmups and reports nine measured iterations. The durable
benchmark reports five isolated append, reopen/index, and projection samples.
The competitor benchmark reports nine isolated samples.

## Status

The public API is experimental before `1.0`. The filesystem journal and
snapshots are local embedded stores, not a distributed database. Cross-process
writer coordination, ACL policies, Git history, MCP tools, compaction, and
database adapters remain separate layers.

## License

MIT
