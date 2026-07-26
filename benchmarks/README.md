# Accuracy benchmarks

The accuracy harness separates retrieval from answer generation. It reports
deterministic Hit@K, Recall@K, nDCG@K, and MRR over exact evidence identifiers;
it does not use an LLM judge or silently treat answer correctness as retrieval
correctness.

Supported inputs:

- [LoCoMo](https://github.com/snap-research/locomo): dialog-level evidence.
- [LongMemEval](https://github.com/xiaowu0162/LongMemEval): session-level
  evidence. The adapter intentionally skips abstention cases without evidence,
  matching the official retrieval protocol.
- `coding_agent_v1.json`: a small MIT-licensed regression suite for failed
  attempts, corrections, branch-aware compatibility, provenance, handoffs, and
  cross-component blast radius.

The public datasets are not copied into this repository. Download them from
their official sources and follow their licenses and citation requirements.

## Prepare

```console
cargo run --release --all-features --bin weavatrix-memory-eval -- \
  prepare-locomo path/to/locomo10.json target/locomo-prepared.json

cargo run --release --all-features --bin weavatrix-memory-eval -- \
  prepare-longmemeval path/to/longmemeval_s_cleaned.json \
  target/longmemeval-prepared.json

cargo run --release --all-features --bin weavatrix-memory-eval -- \
  validate-coding benchmarks/data/coding_agent_v1.json \
  target/coding-agent-prepared.json
```

Prepared files share one provider-neutral contract: groups of documents, query
cases, and exact relevant document IDs. A `weavatrix-search` or semantic/vector
adapter can emit either a JSON array or JSONL records shaped as:

```json
{"case_id":"coding-001","ranked_ids":["failure:one-day-accuracy","decision:projection-date-field"]}
```

## Smoke baseline and score

The literal baseline is deliberately simple; it validates adapters and metrics,
not product quality.

```console
cargo run --release --all-features --bin weavatrix-memory-eval -- \
  literal target/coding-agent-prepared.json target/coding-predictions.json 10

cargo run --release --all-features --bin weavatrix-memory-eval -- \
  score target/coding-agent-prepared.json target/coding-predictions.json \
  target/coding-report.json
```

Comparable reports must use the same dataset revision, evidence granularity,
cutoffs, filtering rules, and provider output. Latency and memory measurements
belong in a separate table from accuracy.
