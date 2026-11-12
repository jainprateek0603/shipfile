# JSON output

Every reporting command accepts `--json` and then prints one JSON document on stdout. Logs and errors go to stderr, so stdout stays parseable.

## Versioning

Every document has a top-level `schema_version` (an integer).

- It is raised only on **breaking** changes: a field removed or renamed, or a type or meaning changed.
- Adding a field or a new enum value is not breaking. Consumers should ignore unknown fields.
- `version` is the CLI version and says nothing about the shape.

## `shipfile doctor --json`

`schema_version: 1`

```json
{
  "schema_version": 1,
  "version": "0.0.1",
  "ok": false,
  "checks": [
    { "name": "docker", "status": "fail", "detail": "not installed", "fix": "install Docker Desktop ..." },
    { "name": "aws region", "status": "ok", "detail": "eu-west-1 (AWS_REGION)" }
  ]
}
```

| Field | Type | Notes |
|---|---|---|
| `ok` | bool | `false` if any check has status `fail`; matches exit code `1`. |
| `checks[].name` | string | Stable identifier, for example `docker`, `aws credentials`. |
| `checks[].status` | `"ok"` \| `"warn"` \| `"fail"` | `warn` means only later commands need it. |
| `checks[].detail` | string | Human text; don't parse it. |
| `checks[].fix` | string, optional | Present when status isn't `ok`. |

The shape is pinned by a snapshot test (`crates/shipfile/src/snapshots/`).

## `shipfile check --json`

`schema_version: 1`. Exit code `0` when `ok`, `1` otherwise. With `--json`, nothing is printed on stderr for spec errors.

```json
{
  "schema_version": 1,
  "ok": false,
  "file": "shipfile.yaml",
  "env": "prod",
  "diagnostics": [
    { "code": "E003", "severity": "error", "message": "unknown field `prot` in `services.api`",
      "file": "shipfile.yaml", "line": 6, "column": 5, "label": "not a known field",
      "help": "did you mean `port`?" }
  ]
}
```

| Field | Type | Notes |
|---|---|---|
| `file` | string | The spec path, always with `/`. |
| `env` | string or null | The `-e` environment. |
| `diagnostics[].code` | string | Stable code; `shipfile explain <code>` describes it. |
| `diagnostics[].line`, `.column` | number | 1-based; columns count characters. |
| `resources` | object, only when `ok` | Counts: `services`, `queues`, `databases`. |

The exact diagnostic fields are pinned by `crates/shipfile/tests/cli.rs`.

## `shipfile graph --json`

`schema_version: 1`, plus the resolved model: `app` (`name`, `env`, `region`, `arch`, `protect`), `services`, `queues`, `databases`, `uses` (edges `{service, resource, kind}`) and `plugins`. Defaults are filled in and the environment is applied. Pinned by `crates/shipfile/tests/snapshots/cli__spec__graph-json.snap`.

## `shipfile explain <code> --json`

`schema_version: 1`: `{ "schema_version": 1, "code": "E012", "text": "# E012: ..." }`. `text` is Markdown.

Commands added later (`plan`, `status`, `plugins`) document their shape here when they land.
