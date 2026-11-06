# ADR-0005: Generate the spec's JSON Schema from the Rust types

**Status:** accepted · November 2026

## Context
The spec needs schema validation inside the CLI and autocomplete in editors through yaml-language-server. If the schema were written by hand, it would drift away from the Rust types.

## Decision
Derive `schemars::JsonSchema` on the spec types. `cargo xtask schema` writes `schema/shipfile.v1.json`, which is committed. CI fails if the committed file differs from a fresh build. The CLI validates against the embedded schema with `jsonschema` before running semantic checks.

## Consequences
- There's one source of truth, and doc comments on the fields become descriptions in the editor.
- Schema error positions need mapping back to YAML positions through the JSON pointer. That mapping lives in `shipfile-core::diagnostics`.
