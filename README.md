# Weavatrix Memory

`weavatrix-memory` is an event-sourced, bitemporal context compiler for coding
agents. It turns immutable code, task, decision, test, failure, and handoff
events into small evidence-carrying graphs for a specific task and token budget.

The crate is a standalone MIT-licensed module of Weavatrix Rust Core. Its
deterministic core does not require an LLM, vector database, external graph
database, async runtime, system clock, or Git executable.

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
- Optimistic concurrency with `NoStream`, `Exact`, and `Any` expectations.
- Globally ordered cursors and independently versioned streams.
- Globally unique event identifiers.
- Strict replay validation for cursor gaps and stream-version gaps.
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
strict replay validator
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

The in-memory store is the reference implementation. Durable filesystem,
database, Git, lexical, semantic, and MCP adapters belong behind separate
interfaces or higher-level Weavatrix Rust Core modules.

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

## Development

```console
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
cargo bench --bench replay
```

Set `WEAVATRIX_BENCH_EVENTS` to change the replay workload. The benchmark runs
two warmups and reports the median of nine measured iterations. Competitive
numbers are published only when workloads, outputs, hardware, build flags, and
methodology are equivalent.

## Status

The public API is experimental before `1.0`. The event log is append-only, but
the reference in-memory store is not durable. Filesystem persistence,
snapshots, subscriptions, ACL policies, Git history, MCP tools, and cross-store
conformance tests are planned as separate layers.

## License

MIT
