# Shipfile

> **One file in, a whole service out.**

Shipfile is a single-binary Rust CLI. It reads one `shipfile.yaml`, runs your stack on your machine with Docker, then deploys the same stack to AWS with a plan you review first.

**Status:** early development (phase 0, foundations). See [docs/PLAN.md](docs/PLAN.md) for the roadmap and [docs/adr/](docs/adr/) for design decisions.

## Try it

```sh
cargo run -- doctor          # checks Docker, Compose and AWS setup
cargo run -- doctor --json   # the same, machine-readable
cargo run -- --version
```

## Develop

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI runs the same on Linux, macOS and Windows.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.
