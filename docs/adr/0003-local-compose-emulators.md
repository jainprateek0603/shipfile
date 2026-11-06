# ADR-0003: Run locally with generated Docker Compose and light emulators

**Status:** accepted · November 2026

## Context
Local-first development needs the same services, queues and databases on a laptop, free, and fast to start. The options were LocalStack, a generated Compose file, or driving the Docker API directly.

## Decision
Generate `.shipfile/local/compose.yaml` and run it with `docker compose`:
- Postgres → the official `postgres` image
- SQS → ElasticMQ, with queues and dead-letter queues pre-created from a generated config
- DynamoDB → `amazon/dynamodb-local`, with tables created through the SDK once it's healthy

Services reach the emulators through the standard `AWS_ENDPOINT_URL_SQS` / `AWS_ENDPOINT_URL_DYNAMODB` variables.

## Consequences
- Free, no account needed, and users can open the generated file to see what's running.
- It doesn't run the CloudFormation template locally, so template bugs only show up at `plan`. Golden tests plus `cfn-lint` cover that gap.
- Needs Docker Compose v2 or later, which `shipfile doctor` checks.
