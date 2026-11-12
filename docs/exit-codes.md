# Exit codes

Scripts and CI can rely on these. They follow [PLAN.md §4.2](PLAN.md#42-commands).

| Code | Meaning |
|---|---|
| `0` | Success. Also for `--help` and `--version`. |
| `1` | Error: a failed command, a failed `doctor` check, or a usage error such as an unknown flag. |
| `2` | The plan has changes. Only returned by `plan --detailed-exitcode`; never by a usage error. |
| `3` | Blocked by a safety guard (for example data loss without `--allow-data-loss`). |

A crash (a bug in Shipfile) exits with `101`, the Rust default, after printing a crash report.
