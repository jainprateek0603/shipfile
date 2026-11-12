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

Commands added later (`check`, `graph`, `plan`, `status`, `plugins`) document their shape here when they land.
