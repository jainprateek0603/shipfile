# Phase 1 — Spec & model

You are implementing **Phase 1** of Shipfile, a single-binary Rust CLI (Cargo workspace, edition 2024). Phase 0 (workspace, `doctor`, 3-OS CI, cargo-dist) is done on `main`.

## Read first
- `docs/PLAN.md` — especially §4.2 (commands), §4.3 (output rules + error format), §5 (spec, `uses`, env contract, validation order), §6.1–6.3 (pipeline, layout, core types), §7 (testing), §8 row "1".
- `docs/adr/0005-schema-from-types.md` and `docs/adr/0006-env-contract.md`.
- Existing code: `crates/shipfile/src/{main,cli,doctor}.rs`, `crates/shipfile/tests/cli.rs`, `crates/shipfile-core/`.

The plan is the source of truth. If something is ambiguous, pick the simplest option that fits it and note the choice in your final report.

## Scope (all in `shipfile-core` unless noted)
1. **Spec types** for `shipfile.yaml` v1 exactly as in PLAN §5, deriving `serde` + `schemars::JsonSchema`, with doc comments. `deny_unknown_fields`.
2. **Positioned parsing** behind one `parse` module (serde-saphyr per the plan; keep it swappable). Every node maps back to file/line/column.
3. **Schema generation**: `cargo xtask schema` writes `schema/shipfile.v1.json` (commit it). Add a test or CI step that fails when the committed schema is stale. Validate specs against the embedded schema with `jsonschema` before semantic checks; map JSON pointers back to YAML positions in `shipfile-core::diagnostics`.
4. **Defaults** and **env overlay** (`-e/--env`) per PLAN §5.
5. **Semantic checks** in the order of PLAN §5.3, with stable error codes (`E0xx`), "did you mean" suggestions (edit distance), diagnostics rendered in the PLAN §4.3 style (codespan-reporting or ariadne), plus a `--json` diagnostic form.
6. **`Model`** (resolved resource graph) and **`Model::contract(service) -> EnvContract`** per ADR-0006 — the only place `uses` edges become env vars (names, local values, AWS value kind, IAM actions).
7. **CLI** (`crates/shipfile`): `init [--yes]`, `check`, `graph [--format tree|dot|json]`, plus global flags `-f/--file`, `-e/--env`, `--json`, `--no-color`/`NO_COLOR`, `-v/-q`. Exit codes per §4.2. Progress → stderr, results → stdout. `init` detects service folders (`package.json`, `pyproject.toml`, `go.mod`, `Cargo.toml`), asks a few questions (skipped with `--yes`), and writes a starter spec that passes `check`.

## Done when
- **15+ broken fixture specs** under `crates/shipfile-core/tests/fixtures/` each produce the expected diagnostic (`insta` snapshot tests), plus valid fixtures that pass.
- `schema/shipfile.v1.json` committed; staleness check wired into CI.
- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` green locally. Generated output uses `/` paths and LF.
- README "Status"/"Try it" updated for `init`, `check`, `graph`; PLAN status line updated.
- Also add this prompt to the repo as `docs/prompts/phase-1-spec-and-model.md`.

## Workflow
- Work on your own branch, small logical commits. **No** `Co-Authored-By` or "Generated with Claude" lines in commits or PRs.
- When done, push the branch and open a PR to `main` titled `Phase 1: spec, model, init/check/graph`, with a short summary and anything deferred.
- Keep replies terse.
