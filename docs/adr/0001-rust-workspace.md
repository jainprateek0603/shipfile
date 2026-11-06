# ADR-0001: Rust in a Cargo workspace

**Status:** accepted · November 2026

## Context
The tool has to ship as a single binary with no runtime dependencies on Windows, macOS and Linux. The portfolio entry was written for Go, but Rust is on the author's skills list and Go isn't.

## Decision
Build it in Rust (edition 2024) as a Cargo workspace with these crates: `shipfile` (bin), `shipfile-core`, `shipfile-cfn`, `shipfile-gen`, `shipfile-local`, `shipfile-aws` and `shipfile-plugin`. Shared dependencies and lints live in the root `Cargo.toml`. Releases are built with `cargo-dist`.

## Consequences
- The pure crates (`core`, `cfn`, `gen`) never depend on `tokio` or the AWS SDK, so they build and test fast and are easy to reason about.
- `shipfile-plugin` can be published on its own, so Rust plugin authors get typed protocol structs.
- Seven crates add a little overhead. If any crate stays under ~300 lines by Phase 4, merge it.
