# Shipfile

> **One file in, a whole service out.**

Shipfile is a single-binary Rust CLI. It reads one `shipfile.yaml`, runs your stack on your machine with Docker, then deploys the same stack to AWS with a plan you review first.

**Status:** early development (phase 1, spec & model: `init`, `check`, `graph`). See [docs/PLAN.md](docs/PLAN.md) for the roadmap and [docs/adr/](docs/adr/) for design decisions.

## Try it

```sh
cargo run -- init --yes                # finds service folders, writes shipfile.yaml
cargo run -- check                     # schema + semantic checks; errors point at the line
cargo run -- check -e prod --json      # with an environment overlay, machine-readable
cargo run -- graph                     # the resource graph (also --format dot|json)
cargo run -- doctor                    # checks Docker, Compose and AWS setup
```

A broken spec gets a diagnostic like this:

```text
error[E012]: unknown resource `orders-qeue` in `uses`
  ┌─ shipfile.yaml:6:12
  │
6 │     uses: [orders-qeue, main-db]
  │            ^^^^^^^^^^^ not defined under `queues` or `databases`
  │
  = help: did you mean `orders-queue`?
```

Editors get autocomplete from [`schema/shipfile.v1.json`](schema/shipfile.v1.json), generated from the Rust types.

## Develop

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask schema            # regenerate schema/shipfile.v1.json after changing spec types
```

CI runs the same on Linux, macOS and Windows.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.
