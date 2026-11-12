# Contributing

Thanks for helping. Larger changes: open an issue first so we can agree on the approach.

## Build and test

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The toolchain is pinned in `rust-toolchain.toml`; rustup installs it on first build. The minimum supported Rust version is in `Cargo.toml` (`rust-version`).

After changing the spec types in `crates/shipfile-core/src/spec.rs`, run `cargo xtask schema` and commit `schema/shipfile.v1.json`; CI fails when it is stale. A new error code needs a `docs/errors/<code>.md` (a test checks this) and a broken fixture in `tests/fixtures/` whose first line is `# <code>: ...`.

Fuzzing (nightly): `cargo +nightly fuzz run load fuzz/corpus/load tests/fixtures examples/orders`. A weekly workflow runs it too.

CI also runs `cargo deny check`, `typos`, `cargo machete`, `cargo doc` and workflow linters; see `.github/workflows/ci.yml`.

## Commits

Use [Conventional Commits](https://www.conventionalcommits.org/): `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `ci:`, `chore:`, with an optional scope, for example `fix(doctor): ...`. Mark breaking changes with `!` (`feat!: ...`).
The CHANGELOG and version bumps are generated from these by release-plz.

## Design decisions

Bigger decisions are recorded as ADRs in [docs/adr/](docs/adr/). Add one when you change something they cover.

## Security

Report vulnerabilities privately; see [SECURITY.md](SECURITY.md).
