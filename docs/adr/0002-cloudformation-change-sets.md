# ADR-0002: Deploy with CloudFormation change sets built from Rust types

**Status:** accepted · November 2026

## Context
The project promises a reviewable plan before every apply, no extra runtime to install, and typed infrastructure. Generating CDK or Terraform code would make users install Node or Terraform, and the plan would become a wrapper around another tool's diff.

## Decision
Build the CloudFormation template from typed Rust structs (`shipfile-cfn`) and deploy it with the AWS SDK:
`CreateChangeSet` (template uploaded to S3) → `DescribeChangeSet` with property values → render → `ExecuteChangeSet`.
AWS keeps the state (the stack). Shipfile has no state file.

## Consequences
- The plan is accurate by construction: it's exactly what CloudFormation will do, including replacements.
- Failed deploys roll back automatically.
- Locked to AWS, which is fine because other clouds are a non-goal.
- CloudFormation can be slow (RDS takes 10–15 minutes) and can hang on unhealthy ECS services, so the ECS deployment circuit breaker is always on.
- Logical IDs must stay stable forever, which is enforced by golden tests.
