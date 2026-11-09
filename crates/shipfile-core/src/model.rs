//! The resolved resource graph: defaults applied, environment overlay merged, runtimes detected.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use crate::spec::{self, Engine, Runtime, Size, Spec, TableKey};

/// Default CPU units for a service.
pub const DEFAULT_CPU: u32 = 256;
/// Default memory (MiB) for a service.
pub const DEFAULT_MEMORY: u32 = 512;
/// Default replica count for a service.
pub const DEFAULT_REPLICAS: u32 = 1;
/// Default receive count before a message moves to the dead-letter queue.
pub const DEFAULT_MAX_RECEIVES: u32 = 5;

/// The resolved spec for one environment.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Model {
    pub app: App,
    pub services: BTreeMap<String, Service>,
    pub queues: BTreeMap<String, Queue>,
    pub databases: BTreeMap<String, Database>,
    /// Service → resource edges, sorted by service then resource.
    pub uses: Vec<Use>,
    pub plugins: Vec<spec::Plugin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct App {
    pub name: String,
    /// The environment overlay applied, if any.
    pub env: Option<String>,
    pub region: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Service {
    pub build: String,
    /// Declared or detected runtime; `None` if neither.
    pub runtime: Option<Runtime>,
    pub command: Option<Vec<String>>,
    pub port: Option<u16>,
    pub public: bool,
    pub health: Option<String>,
    pub scale: Scale,
    pub env: BTreeMap<String, String>,
    pub secrets: Vec<String>,
    pub uses: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Scale {
    pub cpu: u32,
    pub memory: u32,
    pub replicas: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Queue {
    pub max_receives: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "engine", rename_all = "lowercase")]
pub enum Database {
    Postgres { size: Size },
    Dynamodb { key: TableKey },
}

impl Database {
    pub fn engine(&self) -> Engine {
        match self {
            Self::Postgres { .. } => Engine::Postgres,
            Self::Dynamodb { .. } => Engine::Dynamodb,
        }
    }
}

/// What a `uses` edge points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceKind {
    Queue,
    Postgres,
    Dynamodb,
}

impl ResourceKind {
    /// The spec section the resource lives in.
    pub fn section(self) -> &'static str {
        match self {
            Self::Queue => "queues",
            Self::Postgres | Self::Dynamodb => "databases",
        }
    }
}

/// One `uses` edge.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Use {
    pub service: String,
    pub resource: String,
    pub kind: ResourceKind,
}

impl Model {
    /// Resolves a spec that passed [`crate::check`]: applies the `env` overlay and defaults, and
    /// fills in runtimes with `detect` (given each service's `build` path).
    pub fn resolve(spec: &Spec, env: Option<&str>, detect: &dyn Fn(&str) -> Option<Runtime>) -> Self {
        let spec = match env {
            Some(env) => overlay(spec, env),
            None => spec.clone(),
        };
        let queues: BTreeMap<_, _> = spec
            .queues
            .iter()
            .map(|(name, q)| {
                let max_receives = q.dlq.as_ref().and_then(|d| d.max_receives).unwrap_or(DEFAULT_MAX_RECEIVES);
                (name.clone(), Queue { max_receives })
            })
            .collect();
        let databases: BTreeMap<_, _> = spec
            .databases
            .iter()
            .map(|(name, db)| {
                let resolved = match db.engine {
                    Engine::Postgres => Database::Postgres { size: db.size.unwrap_or(Size::Small) },
                    Engine::Dynamodb => Database::Dynamodb {
                        key: db.key.clone().unwrap_or_else(|| TableKey { partition: "id".to_owned(), sort: None }),
                    },
                };
                (name.clone(), resolved)
            })
            .collect();
        let services: BTreeMap<_, _> = spec
            .services
            .iter()
            .map(|(name, s)| {
                let scale = s.scale.clone().unwrap_or_default();
                let mut uses = s.uses.clone();
                uses.sort();
                uses.dedup();
                let service = Service {
                    build: s.build.clone(),
                    runtime: s.runtime.or_else(|| detect(&s.build)),
                    command: s.command.clone(),
                    port: s.port,
                    public: s.public.unwrap_or(false),
                    health: s.health.clone(),
                    scale: Scale {
                        cpu: scale.cpu.unwrap_or(DEFAULT_CPU),
                        memory: scale.memory.unwrap_or(DEFAULT_MEMORY),
                        replicas: scale.replicas.unwrap_or(DEFAULT_REPLICAS),
                    },
                    env: s.env.clone(),
                    secrets: {
                        let mut s = s.secrets.clone();
                        s.sort();
                        s.dedup();
                        s
                    },
                    uses,
                };
                (name.clone(), service)
            })
            .collect();
        let mut uses: Vec<Use> = services
            .iter()
            .flat_map(|(svc, s)| {
                s.uses.iter().filter_map(|r| {
                    let kind = if queues.contains_key(r) {
                        ResourceKind::Queue
                    } else {
                        match databases.get(r)?.engine() {
                            Engine::Postgres => ResourceKind::Postgres,
                            Engine::Dynamodb => ResourceKind::Dynamodb,
                        }
                    };
                    Some(Use { service: svc.clone(), resource: r.clone(), kind })
                })
            })
            .collect();
        uses.sort();
        Model {
            app: App {
                name: spec.name.clone(),
                env: env.map(str::to_owned),
                region: spec.aws.as_ref().and_then(|a| a.region.clone()),
            },
            services,
            queues,
            databases,
            uses,
            plugins: spec.plugins.clone(),
        }
    }

    /// The `uses` edges of one service.
    pub fn uses_of<'a>(&'a self, service: &'a str) -> impl Iterator<Item = &'a Use> + 'a {
        self.uses.iter().filter(move |u| u.service == service)
    }
}

/// Merges an environment's overrides into the base spec. Scalars replace, maps merge by key,
/// lists replace. Unknown names are ignored (they are reported by the checks).
pub fn overlay(spec: &Spec, env: &str) -> Spec {
    let mut out = spec.clone();
    let Some(patch) = spec.environments.get(env) else { return out };
    if let Some(region) = patch.aws.as_ref().and_then(|a| a.region.clone()) {
        out.aws.get_or_insert_with(Default::default).region = Some(region);
    }
    for (name, p) in &patch.services {
        let Some(s) = out.services.get_mut(name) else { continue };
        if let Some(v) = &p.command {
            s.command = Some(v.clone());
        }
        if p.port.is_some() {
            s.port = p.port;
        }
        if p.public.is_some() {
            s.public = p.public;
        }
        if let Some(v) = &p.health {
            s.health = Some(v.clone());
        }
        if let Some(scale) = &p.scale {
            let base = s.scale.get_or_insert_with(Default::default);
            base.cpu = scale.cpu.or(base.cpu);
            base.memory = scale.memory.or(base.memory);
            base.replicas = scale.replicas.or(base.replicas);
        }
        s.env.extend(p.env.iter().map(|(k, v)| (k.clone(), v.clone())));
        if let Some(v) = &p.secrets {
            s.secrets.clone_from(v);
        }
    }
    for (name, p) in &patch.queues {
        let Some(q) = out.queues.get_mut(name) else { continue };
        if let Some(max) = p.dlq.as_ref().and_then(|d| d.max_receives) {
            q.dlq.get_or_insert_with(Default::default).max_receives = Some(max);
        }
    }
    for (name, p) in &patch.databases {
        let Some(db) = out.databases.get_mut(name) else { continue };
        if p.size.is_some() {
            db.size = p.size;
        }
    }
    out
}

/// Detects a runtime from the marker files in a build folder.
pub fn detect_runtime(dir: &Path) -> Option<Runtime> {
    [
        ("package.json", Runtime::Node22),
        ("pyproject.toml", Runtime::Python313),
        ("requirements.txt", Runtime::Python313),
        ("go.mod", Runtime::Go125),
        ("Cargo.toml", Runtime::Rust),
    ]
    .into_iter()
    .find(|(file, _)| dir.join(file).is_file())
    .map(|(_, runtime)| runtime)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::spec::{Environment, Scale as SpecScale, Service as SpecService, ServicePatch};

    fn base() -> Spec {
        let mut services = BTreeMap::new();
        services.insert(
            "api".to_owned(),
            SpecService {
                build: "./api".to_owned(),
                runtime: None,
                command: None,
                port: Some(3000),
                public: Some(true),
                health: Some("/healthz".to_owned()),
                scale: Some(SpecScale { cpu: None, memory: Some(1024), replicas: None }),
                env: BTreeMap::from([("A".to_owned(), "1".to_owned()), ("B".to_owned(), "2".to_owned())]),
                secrets: vec!["S".to_owned()],
                uses: vec![],
            },
        );
        Spec {
            version: 1,
            name: "orders".to_owned(),
            aws: None,
            services,
            queues: BTreeMap::new(),
            databases: BTreeMap::new(),
            environments: BTreeMap::new(),
            plugins: vec![],
        }
    }

    #[test]
    fn defaults_fill_gaps() {
        let model = Model::resolve(&base(), None, &|_| Some(Runtime::Node22));
        let api = &model.services["api"];
        assert_eq!(api.scale, Scale { cpu: 256, memory: 1024, replicas: 1 });
        assert_eq!(api.runtime, Some(Runtime::Node22));
    }

    #[test]
    fn overlay_merges_maps_and_scale_fields() {
        let mut spec = base();
        spec.environments.insert(
            "prod".to_owned(),
            Environment {
                services: BTreeMap::from([(
                    "api".to_owned(),
                    ServicePatch {
                        scale: Some(SpecScale { replicas: Some(2), ..Default::default() }),
                        env: BTreeMap::from([("B".to_owned(), "x".to_owned())]),
                        ..Default::default()
                    },
                )]),
                ..Default::default()
            },
        );
        let model = Model::resolve(&spec, Some("prod"), &|_| None);
        let api = &model.services["api"];
        assert_eq!(api.scale, Scale { cpu: 256, memory: 1024, replicas: 2 });
        assert_eq!(api.env["A"], "1");
        assert_eq!(api.env["B"], "x");
        assert_eq!(model.app.env.as_deref(), Some("prod"));
    }

    proptest! {
        #[test]
        fn overlay_is_idempotent(
            replicas in proptest::option::of(0u32..100),
            port in proptest::option::of(1u16..),
            public in proptest::option::of(any::<bool>()),
            env in proptest::collection::btree_map("[A-Z]{1,3}", "[a-z]{0,3}", 0..4),
        ) {
            let mut spec = base();
            spec.environments.insert("prod".to_owned(), Environment {
                services: BTreeMap::from([("api".to_owned(), ServicePatch {
                    port, public, env: env.clone(),
                    scale: Some(SpecScale { replicas, ..Default::default() }),
                    ..Default::default()
                })]),
                ..Default::default()
            });
            let once = overlay(&spec, "prod");
            let twice = overlay(&once, "prod");
            prop_assert_eq!(&once, &twice);
            // Untouched fields keep their base values; patched ones take the patch.
            let api = &once.services["api"];
            prop_assert_eq!(api.port, port.or(Some(3000)));
            prop_assert_eq!(&api.build, "./api");
            for (k, v) in &env {
                prop_assert_eq!(&api.env[k], v);
            }
        }

        #[test]
        fn empty_overlay_is_identity(name in "[a-z]{1,8}") {
            let mut spec = base();
            spec.environments.insert(name.clone(), Environment::default());
            prop_assert_eq!(overlay(&spec, &name), spec);
        }
    }
}
