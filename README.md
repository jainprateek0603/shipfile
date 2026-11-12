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
```

CI runs the same on Linux, macOS and Windows. See [CONTRIBUTING.md](CONTRIBUTING.md), [docs/exit-codes.md](docs/exit-codes.md) and [docs/json-output.md](docs/json-output.md).

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.
