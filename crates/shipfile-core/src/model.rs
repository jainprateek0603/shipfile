//! The resolved model: the spec with defaults filled in, the environment applied and runtimes
//! detected. Every generator reads this, never the raw spec.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::name::Name;
use crate::project::Runtime;
use crate::spec::Size;

pub const DEFAULT_CPU: u32 = 256;
pub const DEFAULT_MEMORY: u32 = 512;
pub const DEFAULT_REPLICAS: u32 = 1;
pub const DEFAULT_MAX_RECEIVES: u32 = 5;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Model {
    pub app: App,
    pub services: BTreeMap<Name, Service>,
    pub queues: BTreeMap<Name, Queue>,
    pub databases: BTreeMap<Name, Database>,
    /// Service → resource edges, sorted by service then resource.
    pub uses: Vec<Use>,
    pub plugins: Vec<Plugin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct App {
    pub name: Name,
    /// The environment applied with `-e`, if any.
    pub env: Option<Name>,
    pub region: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Service {
    /// `/`-separated folder relative to the spec; `.` for the spec's own folder.
    pub build: String,
    /// `None` when the folder has its own Dockerfile and no runtime was given or detected.
    pub runtime: Option<Runtime>,
    /// Where the runtime came from: `spec`, or the manifest it was detected from.
    pub runtime_source: Option<String>,
    pub port: Option<u16>,
    pub public: bool,
    pub health: Option<String>,
    pub scale: Scale,
    pub env: BTreeMap<String, String>,
    /// Secret names, sorted.
    pub secrets: Vec<String>,
    /// Resources this service uses, sorted.
    pub uses: Vec<Name>,
    pub command: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Scale {
    pub cpu: u32,
    pub memory: u32,
    pub replicas: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Queue {
    pub dlq: Dlq,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Dlq {
    pub max_receives: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "engine", rename_all = "lowercase")]
pub enum Database {
    Postgres { size: Size },
    Dynamodb { key: Key },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Key {
    pub partition: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceKind {
    Queue,
    Postgres,
    Dynamodb,
}

impl ResourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queue => "queue",
            Self::Postgres => "postgres",
            Self::Dynamodb => "dynamodb",
        }
    }
}

/// One `uses` edge.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Use {
    pub service: Name,
    pub resource: Name,
    pub kind: ResourceKind,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Plugin {
    pub name: Name,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub options: BTreeMap<String, serde_json::Value>,
}

impl Model {
    /// The kind of the queue or database called `name`.
    pub fn resource_kind(&self, name: &Name) -> Option<ResourceKind> {
        if self.queues.contains_key(name) {
            return Some(ResourceKind::Queue);
        }
        self.databases.get(name).map(|db| match db {
            Database::Postgres { .. } => ResourceKind::Postgres,
            Database::Dynamodb { .. } => ResourceKind::Dynamodb,
        })
    }

    /// The edges leaving `service`.
    pub fn uses_of<'a>(&'a self, service: &'a Name) -> impl Iterator<Item = &'a Use> + 'a {
        self.uses.iter().filter(move |u| &u.service == service)
    }

    /// The services that use `resource`.
    pub fn users_of<'a>(&'a self, resource: &'a Name) -> impl Iterator<Item = &'a Name> + 'a {
        self.uses.iter().filter(move |u| &u.resource == resource).map(|u| &u.service)
    }
}
