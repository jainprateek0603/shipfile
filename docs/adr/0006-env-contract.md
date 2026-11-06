# ADR-0006: One environment-variable contract for local and AWS

**Status:** accepted · November 2026

## Context
The main promise is that code which works on `shipfile up` also works after `shipfile apply`. That breaks if the Compose generator, the CloudFormation generator and the bindings each decide variable names on their own.

## Decision
`Model::contract(service)` in `shipfile-core` is the only place that turns `uses` edges into an `EnvContract`: variable names, their local values, their AWS values (`Ref`, `GetAtt` or a `secrets` entry) and the IAM actions they need. Every generator reads from it.

## Consequences
- Local and AWS can't drift apart, and adding a resource kind means extending one function.
- RDS passwords come from Secrets Manager through ECS `secrets`, so no ready-made connection string can exist in AWS. The bindings build one from the separate parts.
