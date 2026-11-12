# Contributing

Thanks for helping. Larger changes: open an issue first so we can agree on the approach.

## Build and test

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The toolchain is pinned in `rust-toolchain.toml`; rustup installs it on first build. The minimum supported Rust version is in `Cargo.toml` (`rust-version`).

CI also runs `cargo deny check`, `typos`, `cargo machete`, `cargo doc` and workflow linters; see `.github/workflows/ci.yml`.

## Commits

Use [Conventional Commits](https://www.conventionalcommits.org/): `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `ci:`, `chore:`, with an optional scope, for example `fix(doctor): ...`. Mark breaking changes with `!` (`feat!: ...`).
The CHANGELOG and version bumps are generated from these by release-plz.

## Design decisions

Bigger decisions are recorded as ADRs in [docs/adr/](docs/adr/). Add one when you change something they cover.

## Security

Report vulnerabilities privately; see [SECURITY.md](SECURITY.md).
