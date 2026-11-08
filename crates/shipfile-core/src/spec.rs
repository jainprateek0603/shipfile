//! The `shipfile.yaml` types, exactly as written by users.
//!
//! These types are the single source of truth for the JSON Schema (ADR-0005): doc comments
//! become descriptions in editors. Names are plain strings here; their rules are checked in
//! [`crate::validate`] so errors can point at the exact key.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A Shipfile spec: one file that describes a whole service, locally and on AWS.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Shipfile spec, version 1", extend("$id" = "https://shipfile.dev/schema/v1.json"))]
pub struct RawSpec {
    /// Spec format version. Must be `1`.
    pub version: u32,
    /// Application name. Used in stack and resource names, so it must be DNS-safe.
    pub name: String,
    /// AWS settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aws: Option<RawAws>,
    /// Containers to run: public services behind a load balancer, or private workers.
    #[serde(default)]
    pub services: BTreeMap<String, RawService>,
    /// SQS queues. Each one gets a dead-letter queue.
    #[serde(default)]
    pub queues: BTreeMap<String, Option<RawQueue>>,
    /// Databases: RDS Postgres or DynamoDB tables.
    #[serde(default)]
    pub databases: BTreeMap<String, RawDatabase>,
    /// Per-environment overrides (for example `dev` and `prod`), applied with `-e <env>`.
    #[serde(default)]
    pub environments: BTreeMap<String, Option<RawEnvironment>>,
    /// Generator plugins to run. Only plugins listed here ever run.
    #[serde(default)]
    pub plugins: Vec<RawPlugin>,
}

/// AWS settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RawAws {
    /// AWS region to deploy to, for example `ap-south-1`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(regex(pattern = r"^[a-z]{2}(-[a-z]+)+-[0-9]$"))]
    pub region: Option<String>,
}

/// A container service.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RawService {
    /// Folder with the service's code, relative to `shipfile.yaml`. A Dockerfile is generated
    /// there if it has none.
    pub build: String,
    /// Language runtime, for example `node22`, `python3.13`, `go1.23` or `rust`. Detected from
    /// `package.json`, `pyproject.toml`, `go.mod` or `Cargo.toml` when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(regex(pattern = r"^(node[0-9]+|python3\.[0-9]+|go1\.[0-9]+|rust(1\.[0-9]+)?)$"))]
    pub runtime: Option<String>,
    /// Port the container listens on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 65535))]
    pub port: Option<u32>,
    /// Put a load balancer in front of the service. Needs `port` and `health`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public: Option<bool>,
    /// HTTP path the load balancer and local runner use to check health, for example `/healthz`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(regex(pattern = r"^/"))]
    pub health: Option<String>,
    /// CPU, memory and replica count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<RawScale>,
    /// Plain environment variables. Values are passed as strings.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, EnvValue>,
    /// Names of secrets to pass as environment variables. Values live in `.env.local` locally
    /// and in SSM Parameter Store on AWS, never in this file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secrets: Vec<String>,
    /// Queues and databases this service may use. Drives IAM, networking, env vars and bindings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uses: Vec<String>,
    /// Command to run instead of the image's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
}

/// CPU, memory and replica count. Any field left out takes its default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RawScale {
    /// vCPU units: 256 is a quarter vCPU. Default 256.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("enum" = [256, 512, 1024, 2048, 4096]))]
    pub cpu: Option<u32>,
    /// Memory in MiB. Must suit `cpu` (Fargate sizes). Default 512.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 512, max = 30720))]
    pub memory: Option<u32>,
    /// Number of running copies. Default 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 0, max = 100))]
    pub replicas: Option<u32>,
}

/// An environment variable value. Numbers and booleans are turned into strings.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum EnvValue {
    String(String),
    Integer(i64),
    Float(f64),
    Bool(bool),
}

impl std::fmt::Display for EnvValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::String(s) => f.write_str(s),
            Self::Integer(i) => write!(f, "{i}"),
            Self::Float(x) => write!(f, "{x}"),
            Self::Bool(b) => write!(f, "{b}"),
        }
    }
}

/// An SQS queue.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RawQueue {
    /// Dead-letter queue settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dlq: Option<RawDlq>,
}

/// Dead-letter queue settings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RawDlq {
    /// Receives before a message moves to the dead-letter queue. Default 5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 1000))]
    pub max_receives: Option<u32>,
}

/// A database.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RawDatabase {
    /// `postgres` (RDS) or `dynamodb`.
    pub engine: Engine,
    /// Postgres only: `small` or `medium`. Default `small`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<Size>,
    /// DynamoDB only (required there): the table's key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<RawKey>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Postgres,
    Dynamodb,
}

impl Engine {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Postgres => "postgres",
            Self::Dynamodb => "dynamodb",
        }
    }
}

/// Postgres instance size.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Size {
    /// `db.t4g.micro` on AWS.
    #[default]
    Small,
    /// `db.t4g.small` on AWS.
    Medium,
}

impl Size {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
        }
    }
}

/// A DynamoDB table key.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RawKey {
    /// Partition (hash) key attribute name.
    pub partition: String,
    /// Optional sort (range) key attribute name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<String>,
}

/// Overrides for one environment. Only fields that can safely differ between environments
/// may be overridden: a service's `build`, `runtime` and `uses` and a database's `engine` and
/// `key` stay the same everywhere, so what works locally works in every environment.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RawEnvironment {
    /// AWS settings for this environment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aws: Option<RawAws>,
    /// Service overrides, by service name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub services: BTreeMap<String, ServicePatch>,
    /// Queue overrides, by queue name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub queues: BTreeMap<String, QueuePatch>,
    /// Database overrides, by database name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub databases: BTreeMap<String, DatabasePatch>,
}

/// Service fields an environment may override. `scale` and `env` merge key by key; the other
/// fields replace the base value.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ServicePatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 65535))]
    pub port: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(regex(pattern = r"^/"))]
    pub health: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<RawScale>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, EnvValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secrets: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
}

/// Queue fields an environment may override.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueuePatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dlq: Option<RawDlq>,
}

/// Database fields an environment may override.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DatabasePatch {
    /// Postgres only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<Size>,
}

/// A generator plugin, run as the program `shipfile-gen-<name>`.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RawPlugin {
    /// Plugin name; the program `shipfile-gen-<name>` is looked up on `PATH`.
    pub name: String,
    /// Path to the plugin program, relative to `shipfile.yaml`, instead of looking it up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Options passed to the plugin as JSON.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, serde_json::Value>,
}
