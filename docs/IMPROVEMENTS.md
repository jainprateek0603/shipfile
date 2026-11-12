# Shipfile: improvements backlog

Gaps found in a review on 12 Nov 2026 (after Phase 0) that are **not** already covered by [PLAN.md](PLAN.md).
Each item has an ID, a priority, the problem and a hint on how to fix it. Tick the box when done and link the PR.

**Priority:** `P0` fix now (a bug, or it blocks a later phase) · `P1` before `v0.1.0`/`v0.2.0` · `P2` before `v1.0.0` · `P3` nice to have / post-1.0.

## Status (12 Nov 2026)

`[x]` = done. Items needing commands that don't exist yet are **specified in [PLAN.md](PLAN.md)** (commands table §4.2, spec §5, AWS runtime §6.5, plugins §6.6, testing §7, timeline §8) and are built in the phase shown.

| Item | State | Where / when |
|---|---|---|
| B1–B7 | done | `main.rs`, `doctor.rs`, `panic.rs`; `check`/`graph`/`explain` JSON carry `schema_version`, `check` honours `--ascii` |
| C1–C8 | done | `Cargo.toml`s, `ci.yml`, `deny.toml`, `dependabot.yml`, `release-plz.toml`, `dist-workspace.toml` |
| C9 | partly | `cargo binstall` + `~/.local/bin` done; Homebrew, Scoop/winget, `msi` in Phase 8 |
| C10 | partly | doc, coverage, nextest, machete in CI; `cargo-semver-checks` in Phase 7 (needs a published baseline) |
| C11 | done | `fuzz/` (`parse`, `load`), weekly `fuzz.yml`; a stable proptest twin (`no_panic.rs`) runs in every CI |
| C12 | specced | Phase 2, with `gen` |
| P1 | partly | ADR-0007; spec field `aws.arch` (default `arm64`) in the model; `doctor` checks buildx; `--platform` builds in Phases 3–6 |
| P2, P3 | partly | spec field `migrate` in the model; `run`, `logs -e`, `shell`: Phases 3 and 6 |
| P4, P5 | partly | spec field `health_cmd` in the model; mapped to `HealthCheck` in Phases 3–4; `rollback` Phase 6 |
| P6 | specced | private isolated subnets: Phase 4 |
| P7 | done | PLAN.md §9; shown in `plan` with the cost estimate |
| P8, P9 | partly | `environments.<env>.protect` (default `true` for `prod`) in the model; guard Phase 6; security-changes section Phase 5 |
| P10 | specced | PLAN §6.6 + ADR-0004; Phase 7 |
| P11 | specced | `migrate-spec` when `version: 2` exists |
| P12 | partly | completions, `schema`, `explain` (texts in `docs/errors/`) done; `.env.example` Phase 2; man pages Phase 8 |
| P13 | done | `doctor` |
| P14 | done | PLAN.md §12 (create the `post-1.0` label on GitHub) |
| D1–D4 | done | `SECURITY.md`, `docs/json-output.md`, `docs/exit-codes.md`, README, `CONTRIBUTING.md` |

**Needs you (GitHub settings):** turn on private vulnerability reporting; add a `RELEASE_PLZ_TOKEN` secret (fine-grained PAT, contents + pull-requests write); protect `main` so the CI checks are required; link the repo on Codecov.

---

## 1. Bugs in existing code

- [x] **B1 · P0 · Usage errors exit with `2`, the same as "plan has changes"**
  clap exits with `2` on bad arguments. A CI script using `plan --detailed-exitcode` would read a typo as "there are changes".
  *Fix:* in `main.rs`, use `Cli::try_parse()`. On `Err(e)`: print with `e.print()`. Return `0` for `DisplayHelp`/`DisplayVersion` and `1` (or `64`, `EX_USAGE`) for anything else. Add a CLI test: `shipfile --bogus` must not exit with `2`.

- [x] **B2 · P0 · `doctor` can hang forever**
  `probe()` calls `Command::output()` with no timeout, and `docker version` stalls while Docker Desktop is starting.
  *Fix:* start the probe with `spawn()` and poll `try_wait()` until a deadline (about 5 s), then `kill()`. Or use the `wait-timeout` crate. Add `Probe::TimedOut` with the hint "Docker is starting or stuck; retry in a moment".

- [x] **B3 · P1 · AWS credential detection gives wrong answers**
  - `AWS_PROFILE=x` is reported OK even if profile `x` doesn't exist.
  - `credential_process`, container credentials (`AWS_CONTAINER_CREDENTIALS_FULL_URI` / `_RELATIVE_URI`) and EC2 instance credentials aren't detected.
  - The `sso_` check matches any profile, not the active one.

  *Fix:* reuse the section parser from `region_from_config` as a small `profile_section(config, profile) -> Option<&str>`. Check that the profile exists in the config or credentials file, and look for `sso_`, `credential_process`, `role_arn` or `aws_access_key_id` only inside that section. Add the container env vars as a source. Keep "not yet verified"; STS verifies in Phase 5.

- [x] **B4 · P1 · `✓ ✗ !` turn into garbage on legacy Windows consoles**
  *Fix:* use ASCII (`[ok] [x] [!]`) when the console is not UTF-8 capable, or when `TERM=dumb`, or on a `--ascii` flag. The `supports-unicode` crate can detect it. Add a test for the ASCII render.

- [x] **B5 · P1 · `--json` has no output-format version**
  `doctor --json` has `version` = the CLI version, which says nothing about the output shape.
  *Fix:* add `"schema_version": 1` to every JSON output. Raise it only on breaking shape changes, and document it (see D2).

- [x] **B6 · P2 · A crash prints a raw Rust backtrace**
  *Fix:* `human-panic`, or a `std::panic::set_hook` that prints "this is a bug in shipfile, please report it at <issues URL>" plus the version and OS, and writes the backtrace to a temp file.

- [x] **B7 · P2 · `-v` and `-q` are parsed but do nothing**
  *Fix:* when Phase 1 adds logging, set up `tracing` + `tracing-subscriber` writing to stderr. `-v` = debug, `-vv` = trace, `-q` = errors only. Add a `SHIPFILE_LOG` env filter. Never log secret values (redact them in a `Debug` impl).

---

## 2. CI, release and supply chain

- [x] **C1 · P0 · Internal crates could be published by accident**
  *Fix:* add `publish = false` to `shipfile-core`, `-cfn`, `-gen`, `-local`, `-aws`. Only `shipfile` and `shipfile-plugin` get published. Consider reserving the names on crates.io early.

- [x] **C2 · P1 · No dependency audit or licence checks**
  *Fix:* add `deny.toml` (`cargo deny init`) covering advisories, licences (allow MIT/Apache-2.0/BSD/ISC/Unicode), bans for duplicates, and sources. Add a CI job using `EmbarkStudios/cargo-deny-action`.

- [x] **C3 · P1 · No automatic dependency updates**
  *Fix:* add `.github/dependabot.yml` for `cargo` and `github-actions`, weekly, grouped minor/patch updates. Renovate also works.

- [x] **C4 · P1 · Actions and toolchain aren't pinned**
  `ci.yml` uses `checkout@v4` while `release.yml` uses `@v6`, and the toolchain is `stable`, which moves.
  *Fix:* pin third-party actions to commit SHAs (Dependabot keeps them current). Set `channel = "1.xx.y"` in `rust-toolchain.toml` and bump it on purpose.

- [x] **C5 · P1 · Workflows aren't linted**
  *Fix:* a CI job with `zizmor` (Actions security) and `actionlint`. Add `crate-ci/typos` for spelling.

- [x] **C6 · P1 · Releases can ship without passing CI**
  *Fix:* protect `v*` tags with a repository ruleset, or have the release workflow check the commit's CI status. The simplest option is to release only from `main` once CI is green (release-plz does this, see C8).

- [x] **C7 · P1 · Missing release targets and provenance**
  *Fix:* in `dist-workspace.toml`:
  - add `x86_64-unknown-linux-musl` (Alpine, older glibc) and `aarch64-pc-windows-msvc`
  - set `github-attestations = true` for signed build provenance

  Then run `dist init` to regenerate `release.yml`.

- [x] **C8 · P1 · No CHANGELOG**
  `dist` uses one for release notes.
  *Fix:* use Conventional Commits, and adopt `release-plz` (PR with version bump + changelog) or `git-cliff`.

- [ ] **C9 · P2 · Few install channels**
  *Fix:* Homebrew is already planned. Also:
  - add Scoop/winget (`dist` supports `msi`; winget needs a manifest PR)
  - add `[package.metadata.binstall]` for `cargo binstall shipfile`
  - reconsider `install-path = "CARGO_HOME"`: users without Rust don't have it on their PATH, so `~/.local/bin` or `dist`'s default may suit them better

- [ ] **C10 · P2 · More quality gates**
  - `cargo doc --workspace --no-deps` with `RUSTDOCFLAGS=-D warnings`
  - `cargo-semver-checks` on `shipfile-plugin` (third parties depend on it)
  - coverage with `cargo-llvm-cov`, uploaded to Codecov, shown as a README badge
  - `cargo-nextest` for faster, clearer test runs
  - `cargo-machete` for unused dependencies

- [x] **C11 · P2 · The YAML parser isn't fuzzed**
  *Fix:* add `fuzz/` with `cargo-fuzz`, with targets `parse` and `parse+validate`. It must never panic, only return diagnostics. Run it for a few minutes in a weekly scheduled job.

- [ ] **C12 · P2 · Determinism isn't tested across operating systems**
  PLAN.md promises byte-identical output.
  *Fix:* in CI, run `gen` on each OS, upload the output, and have one job diff all three. Also run `gen` twice and compare.

---

## 3. Product gaps

- [x] **P1 · P0 (decide before Phase 6) · CPU architecture mismatch**
  On an Apple Silicon Mac, Docker builds `arm64` images, but Fargate runs `X86_64` by default, so the task fails with "exec format error".
  *Fix:* choose one:
  - (a) always `docker build --platform linux/amd64`, which is slow on ARM because of emulation
  - (b) set the TaskDefinition to `RuntimePlatform.CpuArchitecture: ARM64` (Graviton, about 20% cheaper) and build `linux/arm64`. Better.

  Add a spec field `arch: arm64 | x86_64`, with a sensible default, and a `doctor` check that `docker buildx` is present. Record the choice as ADR-0007.

- [ ] **P2 · P1 · No database migration story**
  *Fix:*
  - add `shipfile run <svc> -e <env> -- <cmd>`, which runs ECS `RunTask` with the service's task definition and command override, waits, streams logs and returns the exit code
  - add an optional per-service `migrate: ["npm","run","migrate"]` that `apply` runs after the new task definition is created and before the service updates
  - locally, the same `run` uses `docker compose run --rm`

- [ ] **P3 · P1 · No way to debug production**
  *Fix:*
  - `shipfile logs <svc> -e prod [--follow] [--since 10m]` using CloudWatch Logs `FilterLogEvents` / `StartLiveTail`
  - `shipfile shell <svc> -e prod` using ECS Exec (`enableExecuteCommand: true`, task role SSM messages permissions; requires the Session Manager plugin, which `doctor` should check)

- [ ] **P4 · P2 · No rollback**
  *Fix:* `shipfile rollback -e <env>` re-applies the previous image tags. Store the last N deployed tags in stack outputs or SSM. Change sets show it like any plan.

- [ ] **P5 · P2 · Workers have no health check**
  *Fix:* an optional `health_cmd: ["python","-m","worker.health"]` mapped to the container `HealthCheck` in both Compose and the TaskDefinition.

- [ ] **P6 · P1 · RDS sits in public subnets**
  *Fix:* add two private, isolated subnets (no route to the internet; free) and use them for the RDS subnet group. Services stay public; the security group rule from `uses` still applies.

- [x] **P7 · P1 · Cost notes miss public IPv4 charges**
  Every Fargate task in a public subnet gets a public IPv4 address, and AWS charges about $3.60 a month for each.
  *Fix:* add this to PLAN.md §9 and show it in `plan` when the cost estimator lands. Option: ARM64 + fewer replicas in dev.

- [ ] **P8 · P1 · Apply allows a dirty git tree**
  *Fix:* `apply -e prod` warns (or refuses, depending on config) when `git status --porcelain` isn't empty. Add a `--allow-dirty` override. Tag images with the git SHA as well as the content hash, and record the SHA in stack tags.

- [ ] **P9 · P1 · The plan doesn't flag security changes**
  *Fix:* in the plan renderer, add a "Security changes" section listing IAM statement changes and security group ingress changes, like `cdk diff` does. Flag wildcards (`*` actions or resources, `0.0.0.0/0` on anything but the ALB) as warnings. This matters because plugins can patch the template.

- [ ] **P10 · P1 · Plugins inherit AWS credentials**
  *Fix:* when spawning a plugin, call `env_clear()` and add back only `PATH`, `HOME`/`USERPROFILE`, `SYSTEMROOT`, `TEMP`/`TMP`, `LANG` and `SHIPFILE_*`. Plugins get everything they need on stdin. Note this in ADR-0004.

- [ ] **P11 · P2 · No upgrade path for the spec version**
  *Fix:* keep `version:` required. Add `shipfile migrate-spec` that rewrites `v1 → v2` while keeping comments (edit with a comment-preserving YAML editor, or do targeted text edits). Old versions get a clear error naming the command.

- [ ] **P12 · P2 · Small developer conveniences**
  - `shipfile schema`: prints the JSON Schema offline (`schemars` is already planned)
  - `shipfile explain E012`: long description of an error code, like `rustc --explain`; keep the texts in `docs/errors/E012.md` and embed them with `include_str!`
  - `gen` writes `.env.example` listing every secret key from the spec, with no values
  - man pages via `clap_mangen` in an `xtask`, shipped in release archives
  - shell completions already planned with `clap_complete`

- [x] **P13 · P2 · More `doctor` checks**
  - local ports free: 5432, 8000, 9324 (try binding `127.0.0.1:<port>`)
  - free disk space (warn below about 5 GB)
  - `git` installed
  - `docker buildx` (see P1)
  - Session Manager plugin (see P3; warning only)

- [x] **P14 · P3 · Post-1.0 candidates (add to the `post-1.0` label)**
  - S3 buckets (with `AWS_ENDPOINT_URL_S3` + MinIO locally)
  - scheduled jobs (EventBridge Scheduler → RunTask; locally a cron sidecar)
  - autoscaling on CPU or queue depth
  - HTTPS + custom domains (already noted in PLAN.md §12)

---

## 4. Repo and docs

- [x] **D1 · P1 · No `SECURITY.md`**
  The tool handles AWS credentials, so a security contact belongs in the repo from the start. Don't wait for Phase 8.
  *Fix:* add `SECURITY.md` and turn on GitHub private vulnerability reporting. State which versions are supported.

- [x] **D2 · P2 · The machine-readable contract isn't documented**
  *Fix:* `docs/json-output.md` listing every `--json` shape with `schema_version` (B5). `docs/exit-codes.md` with the table from PLAN.md §4.2, plus the usage-error code from B1. Golden-test the JSON shapes with `insta`.

- [x] **D3 · P2 · The privacy stance isn't stated**
  *Fix:* a short README section: no telemetry; credentials are never stored, only read through the standard AWS chain; secrets are never printed.

- [x] **D4 · P3 · `CONTRIBUTING.md` and issue templates come earlier**
  Already in Phase 8. Optionally add a minimal `CONTRIBUTING.md` (build, test, commit style) as soon as C8 picks a commit convention.

---

## Suggested order

1. **Right away (one small PR):** B1, B2, C1, C4
2. **During Phase 1:** B3, B4, B5, B7, C2, C3, C5, D1
3. **Before `v0.1.0`:** C6, C7, C8, C12, P12, P13
4. **Before AWS work (Phase 4–6):** P1 (ADR-0007), P6, P7, P9, P10, then P2, P3, P8
5. **Before `v1.0.0`:** B6, C9, C10, C11, P4, P5, P11, D2, D3
