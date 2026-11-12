# ADR-0008: Positioned parsing through a JSON view, and diagnostics in one style

**Status:** accepted · November 2026

## Context
Every error must say where it is (§4.3), and the order of checks is fixed (§5.3): YAML, then the JSON Schema, then semantic checks. `serde-saphyr` can attach positions to values through `Spanned<T>`, but putting `Spanned` into the spec types would leak into the generated schema and the `Model`, and it loses positions inside untagged enums and `#[serde(flatten)]`. The schema validator (`jsonschema`) works on JSON and reports problems as JSON pointers. `serde-saphyr` 1.2+ needs Rust 1.89, one release above the old MSRV. miette's built-in handlers don't draw the codespan-like layout in §4.3.

## Decision
- `shipfile-core::parse` is the only module that uses `serde-saphyr`. It deserialises the file into a small `Node` tree (`Spanned` on every key and value, through a custom visitor), with duplicate keys passed through and reported by us, so both positions are known.
- The tree is converted to `serde_json::Value`. The schema validates that, and the typed `RawSpec` is deserialised from it, so the spec types carry no position data. Diagnostics find their span by looking a path up in the tree (`Document::span`, `Document::key_span`), which is also how schema errors map their instance path back to YAML.
- Input is normalised before parsing (CRLF → LF, no BOM). Lines and columns are therefore the same whatever line endings the file was saved with; a CRLF fixture guards this.
- The core produces plain `Diagnostic` data with stable codes (`E001`…). The binary wraps it as a `miette::Diagnostic` and draws it with its own `miette::ReportHandler` in the §4.3 layout. That handler is also the global miette hook, so every error the CLI prints looks the same.
- MSRV moves to Rust 1.89.

## Consequences
- Swapping the YAML library means rewriting one visitor; the rest only sees `Node` and JSON.
- YAML values reach the schema as JSON types: a YAML timestamp or a non-string key is seen as a string, which is what the spec wants anyway.
- Error codes are a public contract. New codes are appended; a code is never renumbered or reused.
