//! The `shipfile.yaml` v1 types, exactly as written by the user (before defaults and overlays).
//!
//! These types are the single source of truth for the JSON Schema (`schema/shipfile.v1.json`,
//! see ADR-0005). Doc comments become descriptions in editors.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A Shipfile spec: one file describing services, queues and databases, run locally with
/// Docker and deployed to AWS.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "shipfile.yaml (v1)")]
pub struct Spec {
    /// Spec format version. Always `1`.
    #[schemars(extend("const" = 1))]
    pub version: u32,

    /// Application name. Lowercase letters, digits and hyphens; used to name stacks and resources.
    pub name: String,

    /// AWS settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aws: Option<Aws>,

    /// Containers that run your code, keyed by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub services: BTreeMap<String, Service>,

    /// SQS queues (ElasticMQ locally), keyed by name. Each gets a dead-letter queue.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub queues: BTreeMap<String, Queue>,

    /// Databases (RDS Postgres or DynamoDB), keyed by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub databases: BTreeMap<String, Database>,

    /// Per-environment overrides, keyed by environment name (for example `dev` or `prod`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub environments: BTreeMap<String, Environment>,

    /// Generator plugins to run, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plugins: Vec<Plugin>,
}

/// AWS settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Aws {
    /// AWS region to deploy to, for example `ap-south-1`. Falls back to `AWS_REGION`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
}

/// A container that runs your code.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Service {
    /// Build context, relative to the spec file. A Dockerfile is generated here if missing.
    pub build: String,

    /// Language runtime. Optional: detected from `package.json`, `pyproject.toml`, `go.mod`
    /// or `Cargo.toml` in the build folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<Runtime>,

    /// Command to run instead of the image default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,

    /// Port the container listens on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 65535))]
    pub port: Option<u16>,

    /// Put a load balancer in front of this service. Needs `port` and `health`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public: Option<bool>,

    /// HTTP path the load balancer and local health checks call, for example `/healthz`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<String>,

    /// CPU, memory and replica count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Scale>,

    /// Plain environment variables.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,

    /// Secret names. Read from `.env.local` locally and from SSM Parameter Store on AWS.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secrets: Vec<String>,

    /// Queues and databases this service may use. Each one becomes environment variables,
    /// IAM permissions and network access.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uses: Vec<String>,
}

/// Language runtime of a service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
pub enum Runtime {
    #[serde(rename = "node20")]
    Node20,
    #[serde(rename = "node22")]
    Node22,
    #[serde(rename = "node24")]
    Node24,
    #[serde(rename = "python3.12")]
    Python312,
    #[serde(rename = "python3.13")]
    Python313,
    #[serde(rename = "go1.24")]
    Go124,
    #[serde(rename = "go1.25")]
    Go125,
    #[serde(rename = "rust")]
    Rust,
}

impl Runtime {
    /// The spelling used in the spec.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Node20 => "node20",
            Self::Node22 => "node22",
            Self::Node24 => "node24",
            Self::Python312 => "python3.12",
            Self::Python313 => "python3.13",
            Self::Go124 => "go1.24",
            Self::Go125 => "go1.25",
            Self::Rust => "rust",
        }
    }
}

/// CPU, memory and replica count. Values follow Fargate's allowed sizes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Scale {
    /// CPU units (1024 = one vCPU). Default 256.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("enum" = [256, 512, 1024, 2048, 4096]))]
    pub cpu: Option<u32>,

    /// Memory in MiB. Default 512.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 512, max = 30720))]
    pub memory: Option<u32>,

    /// Number of running copies. Default 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 0, max = 100))]
    pub replicas: Option<u32>,
}

/// An SQS queue.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Queue {
    /// Dead-letter queue settings. Every queue has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dlq: Option<Dlq>,
}

/// Dead-letter queue settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Dlq {
    /// How many times a message is received before it moves to the dead-letter queue. Default 5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 1000))]
    pub max_receives: Option<u32>,
}

/// A database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Database {
    /// `postgres` (RDS) or `dynamodb`.
    pub engine: Engine,

    /// Postgres only: `small` or `medium`; maps to an instance class. Default `small`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<Size>,

    /// DynamoDB only: the table key. Required for DynamoDB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<TableKey>,
}

/// Database engine: `postgres` (RDS; a `postgres` container locally) or `dynamodb`
/// (DynamoDB Local locally).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Postgres,
    Dynamodb,
}

impl Engine {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Postgres => "postgres",
            Self::Dynamodb => "dynamodb",
        }
    }
}

/// Postgres instance size: `small` (`db.t4g.micro`) or `medium` (`db.t4g.small`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Size {
    Small,
    Medium,
}

impl Size {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
        }
    }
}

/// DynamoDB table key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TableKey {
    /// Partition (hash) key attribute name.
    pub partition: String,
    /// Optional sort (range) key attribute name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<String>,
}

/// Overrides for one environment. Only fields that exist in the base spec can be changed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    /// AWS settings for this environment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aws: Option<Aws>,

    /// Service overrides, keyed by an existing service name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub services: BTreeMap<String, ServicePatch>,

    /// Queue overrides, keyed by an existing queue name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub queues: BTreeMap<String, Queue>,

    /// Database overrides, keyed by an existing database name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub databases: BTreeMap<String, DatabasePatch>,
}

/// Service fields an environment may override. Maps are merged by key; lists are replaced.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ServicePatch {
    /// Command to run instead of the image default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
    /// Port the container listens on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 65535))]
    pub port: Option<u16>,
    /// Put a load balancer in front of this service.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public: Option<bool>,
    /// Health-check path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<String>,
    /// CPU, memory and replica count; merged field by field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Scale>,
    /// Extra or changed environment variables; merged by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Replaces the secret list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secrets: Option<Vec<String>>,
}

/// Database fields an environment may override.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DatabasePatch {
    /// Postgres only: instance size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<Size>,
}

/// A generator plugin (an external program named `shipfile-gen-<name>`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Plugin {
    /// Plugin name; the program `shipfile-gen-<name>` is looked up on `PATH`.
    pub name: String,
    /// Explicit path to the plugin program, relative to the spec file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Options passed to the plugin as-is.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, serde_json::Value>,
}
