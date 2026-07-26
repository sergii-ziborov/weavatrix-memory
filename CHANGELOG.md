# Changelog

All notable changes to this project are documented in this file.

## [Unreleased]

## [0.2.0] - 2026-07-26

### Added

- Optional versioned LZ4 codec wrapper with incompressible-data fallback and a
  decoded-size allocation limit.
- Optional XChaCha20-Poly1305 codec wrapper with OS-generated nonces,
  authenticated context, envelope key identifiers, zeroizing static keys, and
  a provider contract for key rotation.
- Optional guarded read-only memory mapping for immutable snapshot generations.
- Output-checked compression, authentication, tamper, allocation-limit, and
  mmap snapshot tests.
- Interleaved, median-based secure-storage and buffered-versus-mmap benchmark.

### Changed

- Snapshot framing is isolated from generation management so buffered and
  memory-mapped reads share identical header, length, CRC32C, and cursor
  validation.
- Codec errors are storage-neutral, and encryption zeroizes temporary
  plaintext buffers in addition to owned static keys.

## [0.1.0] - 2026-07-26

### Added

- Atomic append-only event store contract with optimistic concurrency.
- Strict global and per-stream replay validation.
- Bitemporal node and fact projection.
- Evidence, confidence, supersession, and retraction models.
- Canonical projection into `weavatrix-graph`.
- Repository-, branch-, relation-, depth-, and budget-aware context compiler.
- Deterministic replay benchmark and cross-platform CI.
- Dependency-free framed filesystem event journal with CRC32C validation.
- Strict and incomplete-tail recovery modes.
- Replay cursors, immutable projection snapshots, and validated resume.
- Explicit-ack catch-up subscriptions.
- Optional serde JSON codec behind the `json` feature.
- Durable append, reopen/index, and projection replay benchmark.
- Owned-batch append path for evidence envelopes without an input payload clone.
- Evidence-equivalent in-memory comparison benchmark against `cqrs-es`.
- Output-checked context and projection benchmark against `agentic-memory`.
- Validated `MemoryProjection::try_from_parts` with parallel fact preparation,
  keyed identifier indexes, and compact dual-CSR adjacency.
- Provider-neutral retrieval interface for literal, lexical, semantic, and
  hybrid search with deterministic reciprocal-rank fusion.
- Versioned dependency-free compact binary projection snapshot codec.
- Belief revision, reasoning-gap, drift, and non-mutating consolidation
  analytics over canonical `weavatrix-graph` snapshots.
- Exact-evidence Hit@K, Recall@K, nDCG@K, and MRR evaluation.
- Full-format LoCoMo and LongMemEval adapters plus a coding-agent benchmark.
- Provider-neutral auto-extraction, reusable indexed entity linking, explicit
  ambiguity handling, provenance-carrying event plans, and a 100,000-entity
  benchmark.

### Changed

- Context compilation now traverses a deterministic incident-fact index from
  the requested seeds instead of materializing and canonicalizing the complete
  active graph before every query.
- Projection primary storage now preserves append order in compact vectors and
  rebuilds derived lookup/CSR indexes during deserialization.
