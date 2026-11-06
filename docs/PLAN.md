# Shipfile: project plan

> **One file in, a whole service out.**
> Shipfile is a single-binary Rust CLI. It reads one `shipfile.yaml`, runs your stack on your machine with Docker, then deploys the same stack to AWS with a plan you review first. Along the way it generates the Dockerfiles, the CI pipeline and typed client bindings.

Status: planning · Started: November 2026

---

## 1. Decisions so far

| Topic | Decision | Why | Record |
|---|---|---|---|
| Name | **Shipfile**: crate `shipfile`, command `shipfile`, spec file `shipfile.yaml` | Free on crates.io, Homebrew and GitHub. The name explains itself, like `Dockerfile` or `Procfile`. It avoids the clash "Atlas" had with Ariga Atlas and the MongoDB Atlas CLI. | n/a |
| Language | Rust (edition 2024), split into a Cargo workspace | One static binary per OS, strong types, and it's on your skills list | [ADR-0001](adr/0001-rust-workspace.md) |
| Order of work | **Local first, then AWS** | You can demo it early without spending money on AWS, and both modes share one spec | [ADR-0003](adr/0003-local-compose-emulators.md) |
| Local runtime | A generated `compose.yaml` running light emulators: Postgres, ElasticMQ for SQS, DynamoDB Local | Free and fast, and you can read the file it writes | [ADR-0003](adr/0003-local-compose-emulators.md) |
| AWS deploy | A CloudFormation template built from typed Rust structs, deployed through **change sets** with the AWS SDK | "Plan before apply" comes built in, AWS tracks the state, and nothing else (Node, Terraform) needs installing | [ADR-0002](adr/0002-cloudformation-change-sets.md) |
| Plugins | External programs named `shipfile-gen-<name>` that receive JSON on stdin and answer with JSON on stdout | Plugins can be written in any language and work on Windows | [ADR-0004](adr/0004-plugin-protocol.md) |
| Spec schema | Generated from the Rust types with `schemars`, never written by hand | The types are the only source of truth, and editors get autocomplete for free | [ADR-0005](adr/0005-schema-from-types.md) |
| Local and AWS parity | One environment-variable contract, computed in one place and used by Compose, CloudFormation and the bindings | Code that works locally works on AWS without changes | [ADR-0006](adr/0006-env-contract.md) |

---

## 2. Positioning: "why not just use X?"

**One-line answer:** *The same file runs your stack locally and on AWS. You review a plan before anything changes, you install nothing besides Docker, and teams add their own conventions as plugins.*

| Tool | What it does well | Where Shipfile differs |
|---|---|---|
| AWS Copilot CLI | Deploying ECS services on AWS | Copilot uses several manifests and doesn't run your stack locally. Shipfile uses one file and runs the same stack locally. |
| SST | Full-stack serverless apps in TypeScript | SST needs Node and a TS config. Shipfile is a single binary with a YAML spec, built around containers. |
| Terraform / Pulumi / CDK | General-purpose infrastructure as code | With those you write the infrastructure yourself. Shipfile **generates** it from a short spec, and you can read the generated template. |
| Encore | Infers infrastructure from your application code | Encore requires its framework. Shipfile works with any app that has a Dockerfile. |
| Docker Compose | Running stacks locally | Compose stops at your machine. Shipfile **generates** the compose file and also deploys the stack. |

---

## 3. Scope

### Version 1 includes
- Services on ECS Fargate (public behind a load balancer, or private workers)
- Queues on SQS, each with a dead-letter queue
- Databases on RDS Postgres or DynamoDB
- Secrets: an `.env.local` file locally, SSM Parameter Store on AWS
- Environments (`dev`, `prod`, and so on) declared as overrides in the same file
- Dockerfiles generated for **Node, Python, Go and Rust**
- Typed **TypeScript** bindings (Python bindings ship as the example plugin)
- A GitHub Actions workflow with the plan posted as a PR comment and OIDC sign-in
- A plugin system with two example plugins

### Not in version 1 (listed so the scope stays fixed)
Other clouds · Kubernetes · Lambda/serverless functions · custom domains and HTTPS certificates · multi-region · drift detection · importing existing resources · a cost estimator · a GUI. Ideas go into a `post-1.0` GitHub label instead.

---

## 4. Using it

### 4.1 The everyday flow

```text
shipfile init          # detects your services and writes shipfile.yaml
shipfile up            # runs the whole stack locally (Docker)
shipfile plan -e prod  # shows exactly what will change on AWS
shipfile apply -e prod # does that, and nothing else
```

### 4.2 Commands

| Command | What it does | Phase |
|---|---|---|
| `shipfile init [--yes]` | Finds service folders (`package.json`, `pyproject.toml`, `go.mod`, `Cargo.toml`), asks a few questions and writes a starter `shipfile.yaml` | 1 |
| `shipfile check` | Validates the spec against the schema, then runs semantic checks. Errors point to the exact line. | 1 |
| `shipfile graph [--format tree\|dot\|json]` | Prints the resource graph | 1 |
| `shipfile gen [--check]` | Writes the Dockerfiles (only where missing), CI workflow, bindings, compose file and template. `--check` fails if any generated file is out of date, which is useful in CI. | 2–4 |
| `shipfile up [--detach]` / `down` / `logs [svc]` / `ps` | Runs the local stack | 3 |
| `shipfile doctor` | Checks Docker, Compose, AWS credentials and region, and explains how to fix anything missing | 0 → grows |
| `shipfile bootstrap [--github owner/repo]` | Run once per AWS account and region: creates the ECR repo and template bucket, plus the GitHub OIDC role if asked | 5 |
| `shipfile plan -e <env> [--out plan.json] [--detailed-exitcode]` | Creates a change set and prints a readable diff grouped by spec resource | 5 |
| `shipfile apply -e <env> [plan.json] [--yes] [--allow-data-loss]` | Builds and pushes images, runs the planned change set and streams events live | 6 |
| `shipfile status -e <env>` | Shows stack state and outputs, such as the public URL | 6 |
| `shipfile destroy -e <env>` | Tears the environment down after you type the stack name to confirm | 6 |
| `shipfile secret set <KEY> -e <env>` | Stores a secret in SSM. The value is read from a hidden prompt or stdin, never from a command-line argument. | 6 |
| `shipfile plugins` | Lists the plugins found and the protocol version each speaks | 7 |

**Global flags:** `-f/--file`, `-e/--env`, `--json` (machine-readable output on every reporting command), `--no-color` (also respects `NO_COLOR`), `-v/-q`.

**Exit codes:** `0` OK · `1` error · `2` the plan has changes (only with `--detailed-exitcode`) · `3` blocked by a safety guard.

### 4.3 Output rules
- Progress and spinners go to **stderr** and results go to **stdout**, so piping `--json` always works.
- Colour and spinners are off when the output isn't a terminal.
- Secrets are never printed, including in `--json` output, debug logs and plan output.
- Every error says **what** went wrong, **where** (file, line, column) and **what to do next**.

```text
error[E012]: unknown resource `orders-qeue` in `uses`
  ┌─ shipfile.yaml:14:18
  │
14│     uses: [orders-qeue, main-db]
  │            ^^^^^^^^^^^ not defined under `queues` or `databases`
  │
  = help: did you mean `orders-queue`?
```

### 4.4 What a plan looks like

```text
$ shipfile plan -e prod

  Stack  orders-prod   ap-south-1 · account ••••6789

  services.api
    ~ TaskDefinition   image  …:a1b2c3 → …:d4e5f6          new revision
    ~ Service          replicas  1 → 2
  queues.orders-queue
    + Queue            orders-queue
    + Queue            orders-queue-dlq
  databases.main-db
    ! DBInstance       db name  orders → orders_v2          REPLACE (data loss)

  Plan: 2 to add · 2 to change · 0 to remove · 1 to replace (1 holds data)
  ✗ apply is blocked: databases.main-db would be replaced. Re-run with --allow-data-loss if that is intended.
```

Changes are grouped by **spec resource**, not by raw CloudFormation logical ID. Every template resource carries `Metadata.shipfile.source = "databases.main-db"`, which is how the plan maps each change back to the spec.

---

## 5. The spec (`shipfile.yaml`)

```yaml
# yaml-language-server: $schema=https://shipfile.dev/schema/v1.json
version: 1
name: orders
aws:
  region: ap-south-1

services:
  api:
    build: ./api              # Dockerfile is generated here if missing
    runtime: node22           # optional; detected from package.json
    port: 3000
    public: true              # load balancer in front
    health: /healthz
    scale: { cpu: 256, memory: 512, replicas: 1 }
    env:
      LOG_LEVEL: info
    secrets: [STRIPE_KEY]
    uses: [orders-queue, main-db]

  worker:
    build: ./worker
    runtime: python3.13
    command: ["python", "-m", "worker"]
    uses: [orders-queue]

queues:
  orders-queue:
    dlq: { max_receives: 5 }

databases:
  main-db:
    engine: postgres
    size: small               # small | medium; maps to instance classes
  sessions:
    engine: dynamodb
    key: { partition: id }    # sort key optional

environments:
  prod:
    services:
      api: { scale: { replicas: 2 } }
    databases:
      main-db: { size: medium }

plugins:
  - name: cost-tags
    options: { cost_centre: "CC-104" }
```

### 5.1 `uses` is the core idea
For every edge from a service to a resource, **one** computed binding drives four outputs:
1. **IAM:** the task role may touch only the resources the service lists, with only the actions it needs.
2. **Network:** the database security group accepts traffic only from the services that use it.
3. **Environment variables:** the same names locally and on AWS (§5.2).
4. **Typed bindings:** only the resources a service uses appear in its generated code.

### 5.2 The environment-variable contract

| Resource | Variables | Local value | AWS value |
|---|---|---|---|
| queue `orders-queue` | `ORDERS_QUEUE_URL` | ElasticMQ queue URL | SQS queue URL |
| postgres `main-db` | `MAIN_DB_HOST` `_PORT` `_NAME` `_USER` `_PASSWORD` | the `postgres` container | RDS endpoint; the password comes from Secrets Manager through ECS `secrets` |
| dynamodb `sessions` | `SESSIONS_TABLE` | `sessions` | the real table name |
| secret `STRIPE_KEY` | `STRIPE_KEY` | `.env.local` (git-ignored) | an SSM SecureString through ECS `secrets` |
| every service | `AWS_REGION` | plus `AWS_ENDPOINT_URL_SQS` / `AWS_ENDPOINT_URL_DYNAMODB` and dummy credentials | the real region, with credentials from the task role |

Locally, the standard `AWS_ENDPOINT_URL_<SERVICE>` variables send every AWS SDK to the emulators, **so application code needs no "if local" branches.**

### 5.3 Validation, in order
1. **Parse:** YAML syntax, keeping the position of every value.
2. **Schema:** types, required fields and allowed values, using a JSON Schema generated from the Rust types.
3. **Semantics:** `uses` points to resources that exist; names are DNS-safe and unique across kinds; there are no port clashes locally; `public` services have a `port` and a `health` path; DynamoDB tables have a key; an `environments` override may only touch fields that exist.
4. **Suggestions:** "did you mean …?" based on edit distance.

---

## 6. Architecture

### 6.1 Pipeline

```text
                       shipfile.yaml
                            │ parse (serde-saphyr, keeps positions)
                            ▼
┌───────────────────── shipfile-core ─────────────────────┐
│ RawSpec ─validate─▶ resolve (defaults, env overlay,     │
│                     runtime detection) ─▶ Model         │
│ Model = resources + `uses` edges + EnvContract          │
│ diagnostics → miette                                    │
└───────────────────────────┬─────────────────────────────┘
                            │ &Model (pure, deterministic)
                            ▼
┌───────────────────── shipfile-gen ──────────────────────┐
│ trait Generator → GenOutput { files, template_patch }   │
│ built-ins: dockerfile · ci · bindings-ts · compose · cfn│
│ plugin runner (external programs, same trait)           │
└───────────┬─────────────────────────────┬───────────────┘
            │ files + compose.yaml        │ template.json
            ▼                             ▼
     shipfile-local                 shipfile-aws
     docker compose up/down         bootstrap · ECR push
     emulator bootstrap             change set → plan diff → safety guard
     health wait                    execute · event stream · outputs
            ▲                             ▲
            └──────── shipfile (CLI) ─────┘
              clap commands, output, exit codes
```

**Main rule: a pure core inside an I/O shell.** Everything from parsing through generation is a pure function of `(spec, env, project files)` and always gives byte-identical output (maps are `BTreeMap`, lists sorted, no timestamps). That makes most of the code testable with golden files and no Docker or AWS. Only `shipfile-local` and `shipfile-aws` do I/O.

### 6.2 Workspace layout

```text
shipfile/
├─ Cargo.toml                 # workspace: shared dependencies, lints, profile.release (lto, strip)
├─ crates/
│  ├─ shipfile/               # the binary: clap commands, output, exit codes
│  ├─ shipfile-core/          # spec types, parsing, validation, env overlay, Model, EnvContract
│  ├─ shipfile-cfn/           # typed CloudFormation types (serde) + intrinsics (Ref, GetAtt, Sub)
│  ├─ shipfile-gen/           # Generator trait, built-in generators, minijinja templates
│  ├─ shipfile-local/         # compose runner, ElasticMQ config, DynamoDB Local table setup
│  ├─ shipfile-aws/           # sts, ecr, s3, cloudformation, ssm; plan diff + safety guard
│  └─ shipfile-plugin/        # protocol types; published so Rust plugins can depend on it
├─ xtask/                     # cargo xtask schema | golden | e2e
├─ schema/shipfile.v1.json    # generated, committed, published for editors
├─ examples/orders/           # api (Node) + worker (Python) + queue + Postgres + DynamoDB
├─ plugins/
│  ├─ cost-tags/              # Rust: tags every resource with a cost centre
│  └─ bindings-python/        # Python: generates typed Python bindings (proves cross-language plugins)
├─ tests/fixtures/            # specs → golden outputs; recorded DescribeChangeSet responses
└─ docs/                      # this plan, ADRs, mdBook user guide
```

### 6.3 Core types (sketch)

```rust
// shipfile-core
pub struct Model {
    pub app: App,                                  // name, env, region
    pub services: BTreeMap<Name, Service>,
    pub queues: BTreeMap<Name, Queue>,
    pub databases: BTreeMap<Name, Database>,
    pub uses: Vec<Use>,                            // service → resource edges
}

impl Model {
    /// The single source of truth for env vars, IAM actions and bindings.
    pub fn contract(&self, service: &Name) -> EnvContract { … }
}

// shipfile-gen
pub trait Generator {
    fn id(&self) -> &'static str;
    fn generate(&self, model: &Model, cx: &GenContext) -> Result<GenOutput, GenError>;
}

pub struct GenOutput {
    pub files: Vec<GeneratedFile>,                  // relative path, contents, ownership
    pub template_patch: Vec<json_patch::PatchOperation>,
}
```

**Who owns which generated file:**
- **Generated once, then yours:** Dockerfiles. They're written only if missing.
- **Fully managed:** `.github/workflows/shipfile.yml` and bindings. They're committed, carry a `# generated by shipfile, do not edit` header, and are checked by `gen --check`.
- **Build output:** everything in `.shipfile/` (compose file, template, plan files). It's git-ignored.

### 6.4 Local runtime (`shipfile up`)
1. `gen` writes `.shipfile/local/compose.yaml` and `elasticmq.conf`, which pre-creates the queues and dead-letter queues.
2. It runs `docker compose -p <app> -f … up --build` (shelling out; `doctor` checks Compose v2 or later).
3. It waits for health checks, then creates the DynamoDB Local tables through the AWS SDK pointed at `localhost:8000`.
4. It prints a summary: service URLs, the database connection string and how to send a test message.

### 6.5 AWS runtime

**Bootstrap stack** (`shipfile-bootstrap`, once per account and region): an ECR repository, an S3 bucket for templates with a 30-day lifecycle rule, and, optionally, a GitHub OIDC provider plus a deploy role limited to one repository.

**App stack** (`<app>-<env>`):

| Spec | CloudFormation resources |
|---|---|
| (always) | VPC, 2 public subnets in 2 availability zones, internet gateway, routes. **No NAT gateway.** An ECS cluster. |
| service | Fargate TaskDefinition, Service with the **deployment circuit breaker and rollback**, LogGroup (7-day retention), task role (least privilege from `uses`), execution role (ECR, logs, its own secrets only), security group |
| `public: true` | ALB, HTTP listener, target group using the `health` path, ALB security group |
| queue | SQS Queue + DLQ with a redrive policy |
| postgres | RDS `db.t4g.micro` / `db.t4g.small`, `ManageMasterUserPassword`, subnet group, security group open only to the services that use it, `DeletionPolicy: Snapshot` |
| dynamodb | Table in `PAY_PER_REQUEST` mode, `DeletionPolicy: Retain` in `prod` |

**Rules that keep plans safe:**
- **Stable logical IDs.** They come only from spec names (`ServiceApiTaskDef`), never from ordering. Golden tests catch any change.
- **Always upload the template to S3** and use `TemplateURL`, so the 51,200-byte inline limit never matters.
- **Plan file** (`plan.json`): the change set ARN, stack, a hash of the template and a hash of the spec. `apply` refuses a plan that's stale (the spec changed) or no longer `AVAILABLE`.
- **Safety guard:** `Remove`, or a `Replacement` of `True` or `Conditional`, on RDS, DynamoDB or SQS needs `--allow-data-loss`. The exit code is `3`.
- **Readable failures:** when a deploy fails, Shipfile shows the **first** failed event (the root cause), not the cascade of rollback errors that follows it.

**Images:** `docker build`, an ECR login through `GetAuthorizationToken` piped to `docker login --password-stdin`, then a push tagged with `<app>-<service>-<content hash>`. Unchanged code means an unchanged tag, which means no new task revision.

### 6.6 Plugin protocol (v1)

```jsonc
// stdin → plugin
{ "protocol": 1, "env": "prod", "model": { … }, "options": { "cost_centre": "CC-104" } }
// plugin → stdout
{ "files": [{ "path": "src/shipfile_gen.py", "contents": "…" }],
  "template_patch": [{ "op": "add", "path": "/Resources/…/Properties/Tags/-", "value": { … } }],
  "diagnostics": [{ "level": "warning", "message": "…" }] }
```

- Plugins are found as `shipfile-gen-<name>` on `PATH` or through a `path:` in the spec. They run **only if listed in the spec**.
- **Sandboxing:** a 30-second timeout. Paths must be relative and stay inside the project (no `..`, no absolute paths, no symlinks pointing out). stderr is streamed as plugin logs.
- The plan lists every plugin that patched the template, so nothing changes without showing up there.

### 6.7 Main crates

| Need | Crate |
|---|---|
| CLI | `clap` 4 (derive) + `clap_complete` |
| YAML with positions | `serde-saphyr` (wrapped behind a single module so it can be swapped) |
| Diagnostics | `miette` (fancy) + `strsim` (suggestions) |
| Schema | `schemars` 1 (generate) + `jsonschema` (validate) |
| AWS | `aws-config`, `aws-sdk-{cloudformation,ecr,s3,ssm,sts,dynamodb}`, `tokio` |
| Templates | `minijinja` (Dockerfiles, workflow, compose) |
| Patches | `json-patch` |
| Terminal | `owo-colors`, `indicatif`, `inquire` (prompts) |
| Errors | `thiserror` (libraries), `miette` at the edge (binary) |
| Testing | `insta` (golden), `assert_cmd` + `assert_fs` (CLI), `proptest` (env overlay merge) |
| Release | `cargo-dist`: GitHub Releases, shell/PowerShell installers, Homebrew tap |

---

## 7. Testing strategy

| Layer | What | Tooling | Needs |
|---|---|---|---|
| Unit | Overlay merging, the env contract, name rules, logical IDs, the safety guard | `cargo test`, `proptest` | nothing |
| Golden | Each fixture spec → diagnostics, Dockerfile, workflow, bindings, compose, template | `insta` snapshots | nothing |
| CLI | Commands, flags, exit codes, `--json` shapes, `gen --check` | `assert_cmd`, `assert_fs` | nothing |
| Plan renderer | Recorded `DescribeChangeSet` JSON → rendered diff and guard result | `insta` | nothing |
| Template lint | Every golden template passes `cfn-lint` | CI step | Python in CI |
| Local end-to-end | `up` → `curl /healthz` → send a message → worker consumes it → `down` | `cargo xtask e2e local` | Docker (Linux CI runner) |
| AWS end-to-end | `bootstrap` → `apply` → `curl` the URL → `destroy` | `cargo xtask e2e aws` | sandbox account; run manually or weekly |

The CI matrix runs on **Linux, macOS and Windows** (`fmt`, `clippy -D warnings`, tests). Generated files always use `/` paths and LF line endings, whatever the OS.

---

## 8. Timeline

About **10 hours a week**, starting **Monday 9 November 2026**. Each phase ends in something you can show, and the AWS work only starts after a free, demo-ready local release.

| # | Phase | Dates | Work | Done when |
|---|---|---|---|---|
| 0 | Foundations | 9 – 15 Nov | Repo, workspace, lints, 3-OS CI, `cargo-dist`, `--version`, `doctor` skeleton, ADRs | `v0.0.1` installs from a GitHub release on Windows |
| 1 | Spec & model | 16 – 29 Nov | Spec types, positioned parsing, schema generation, defaults, env overlay, semantic checks with suggestions, `Model` + `EnvContract`; `init`, `check`, `graph` | 15+ broken fixture specs give the expected diagnostics; schema committed |
| 2 | File generators | 30 Nov – 13 Dec | Dockerfile templates (Node/Python/Go/Rust; multi-stage, non-root), TS bindings, CI workflow (check + gen only), `gen`, `gen --check` | `examples/orders` gets every file; golden tests green |
| 3 | Local runtime | 14 – 27 Dec | Compose generator, ElasticMQ config, DynamoDB Local setup, `up/down/logs/ps`, health wait | Example stack runs locally: API answers and the worker consumes a queued message. **Release `v0.1.0`** + VHS demo GIF |
| (buffer) | Catch-up | 28 Dec – 3 Jan | Slack for exams, holidays or overruns | n/a |
| 4 | CloudFormation | 4 – 17 Jan | `shipfile-cfn` types, builders for network/ECS/ALB/SQS/RDS/DynamoDB/IAM, stable IDs, source metadata, `cfn-lint` in CI | Golden templates pass `cfn-lint` |
| 5 | Plan | 18 – 31 Jan | `bootstrap`, STS identity, change sets (with property values), grouped diff renderer, safety guard, plan file, exit codes, `--json` | A sandbox account shows a correct add/modify/replace diff; guard blocks a database replace |
| 6 | Apply | 1 – 14 Feb | Image build + ECR push, execute change set, live events, root-cause failures, `status`, `destroy`, `secret set` | Empty account → live URL with `bootstrap` + `apply`; `destroy` leaves nothing behind. **Release `v0.2.0`** |
| 7 | CI & plugins | 15 – 28 Feb | Full workflow (PR plan comment, OIDC through `bootstrap --github`, apply on merge), plugin crate + runner + sandboxing, `cost-tags` (Rust) and `bindings-python` (Python) | A PR in the example repo gets a plan comment; the Python plugin works end to end |
| 8 | Polish & launch | 1 – 14 Mar | mdBook docs on GitHub Pages, README with GIF, CONTRIBUTING, issue templates, "good first issue" labels, Homebrew tap, schema hosting, portfolio update | Someone new deploys the example in under 10 minutes. **Release `v1.0.0`** |

**Milestones:** `v0.1.0` local (27 Dec) → `v0.2.0` AWS (14 Feb) → `v1.0.0` launch (14 Mar 2027).

If time gets tight, cut in this order: `bindings-python` plugin → `graph --format dot` → `secret set` (document `aws ssm put-parameter` instead) → the Rust Dockerfile template. **Never cut** the plan, the safety guard or `destroy`.

---

## 9. Costs and AWS safety

- Set an **AWS Budget alert at $5–10** before your first `apply`.
- No NAT gateway (about $32 a month each). Tasks run in public subnets with locked-down security groups.
- The **ALB costs about $16+ a month** even when idle, and RDS bills by the hour. `destroy` demo stacks right after recording. Use DynamoDB for quick demos, because RDS takes 10–15 minutes to create.
- Shipfile never stores AWS keys. It uses the standard credential chain, and CI signs in through GitHub OIDC only.

---

## 10. Risks

| Risk | Mitigation |
|---|---|
| An ECS service never stabilises and CloudFormation hangs for hours | Turn on the deployment circuit breaker with rollback, plus a health-check grace period |
| Renaming something in the spec replaces a resource that holds data | Logical IDs come from names, and the guard blocks data-holding replacements. A future `rename` is out of scope. |
| `serde-saphyr` is a young crate | Keep it behind one `parse` module, with golden tests on positions |
| Windows paths and line endings | Windows in the CI matrix; normalised output |
| Running out of AWS credit | Budget alert, `destroy` habit, most tests offline |
| Scope creep | The not-in-v1 list in §3; new ideas go to the `post-1.0` label |
| College workload | A buffer week; every phase ends in something you can ship |

---

## 11. Updating the portfolio (after `v1.0.0`)

In the project entry in `src/content/site.ts`:
- `sample: false`. Remove every placeholder in square brackets.
- Title and slug **Shipfile** / `shipfile`, year `2026`, timeline `2026 — now`, role "Author · maintainer", team "Solo".
- Tags: Rust, AWS, CloudFormation, Docker, GitHub Actions.
- Architecture boxes: `shipfile.yaml → Core (Rust) → Generators → Local (Compose) | AWS (change sets)`.
- Links: the real repo, the docs site, and the VHS GIF in place of the iframe demo.
- Gallery: `shipfile plan` with a blocked replacement, `shipfile up`, and a PR plan comment.
- Outcomes: **only measured numbers.** For example, the time from `init` to a live URL, the number of AWS resources produced by an N-line spec, the test count, and the platforms supported. Add stars and contributors only once they exist.

---

## 12. Decided later (not blocking)

- HTTPS and custom domains (ACM + Route 53): the first post-1.0 feature
- A rough monthly cost estimate in `plan`
- `shipfile eject --cdk`, which writes equivalent CDK TypeScript
- A trademark and domain check for "Shipfile" before launch (`shipfile.dev` is used above as a placeholder)
