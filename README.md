# Shipfile

> **One file in, a whole service out.**

Shipfile is a single-binary Rust CLI. It reads one `shipfile.yaml`, runs your stack on your machine with Docker, then deploys the same stack to AWS with a plan you review first.

**Status:** early development. Phases 0 (foundations) and 1 (spec & model: `init`, `check`, `graph`, `schema`, `explain`) are done; phase 2 (file generators) is next. See [docs/PLAN.md](docs/PLAN.md) for the roadmap and [docs/adr/](docs/adr/) for design decisions.

## Try it

```sh
cargo run -- init --yes                # finds service folders, writes shipfile.yaml
cargo run -- check                     # schema + semantic checks; errors point at the line
cargo run -- check -e prod --json      # with an environment overlay, machine-readable
cargo run -- graph                     # the resource graph (also --format dot|json)
cargo run -- explain E012              # long explanation of an error code
cargo run -- schema > shipfile.v1.json # the spec's JSON Schema, offline
cargo run -- doctor                    # checks Docker, Compose and AWS setup
```

Try it on the example: `cargo run -- -f examples/orders/shipfile.yaml graph`.

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

## Install

From a release (no Rust needed): use the shell or PowerShell installer from the [releases page](https://github.com/jainprateek0603/shipfile/releases), or `cargo binstall shipfile`.

Shell completions: `shipfile completions bash|zsh|fish|powershell|elvish`.

## Privacy and security

- **No telemetry.** Shipfile never phones home.
- **Credentials are never stored.** AWS credentials are read through the standard AWS chain (env, profiles, SSO) and never written anywhere by Shipfile.
- **Secrets are never printed.** Secret values aren't shown in output, logs, plans or crash reports.

Found a vulnerability? See [SECURITY.md](SECURITY.md).

## Develop

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask schema            # after changing the spec types; CI fails on a stale schema
```

CI runs the same on Linux, macOS and Windows. See [CONTRIBUTING.md](CONTRIBUTING.md), [docs/exit-codes.md](docs/exit-codes.md) and [docs/json-output.md](docs/json-output.md).

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.
