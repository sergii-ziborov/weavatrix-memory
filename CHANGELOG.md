# Changelog

All notable changes to this project are documented in this file.

## [Unreleased]

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
