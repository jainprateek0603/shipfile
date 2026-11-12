//! Turning a valid spec into a [`Model`]: environment overlay, defaults and runtime detection.

use std::collections::BTreeMap;

use crate::diagnostics::{Code, Diagnostic, did_you_mean};
use crate::model::{
    App, DEFAULT_CPU, DEFAULT_MAX_RECEIVES, DEFAULT_REPLICAS, Database, Dlq, Key, Model, Plugin, Queue, ResourceKind,
    Scale, Service, Use,
};
use crate::name::Name;
use crate::overlay;
use crate::parse::Document;
use crate::project::{self, Project, Runtime};
use crate::spec::{Engine, RawService, RawSpec};
use crate::validate::default_memory;

/// Resolves a spec that has passed the schema and semantic checks.
pub fn resolve(
    doc: &Document,
    spec: &RawSpec,
    env: Option<&str>,
    project: &dyn Project,
) -> Result<Model, Vec<Diagnostic>> {
    let (spec, env, guarded) = match env {
        None => (spec.clone(), None, false),
        Some(env) => match spec.environments.get(env) {
            Some(overrides) => {
                let overrides = overrides.clone().unwrap_or_default();
                let guarded = overrides.protect.unwrap_or(env == "prod");
                (overlay::apply(spec, &overrides), Some(name(env)?), guarded)
            }
            None => return Err(vec![unknown_environment(doc, spec, env)]),
        },
    };
    let mut errors = Vec::new();
    let mut services = BTreeMap::new();
    for (svc_name, raw) in &spec.services {
        match service(doc, svc_name, raw, project) {
            Ok(svc) => {
                services.insert(name(svc_name)?, svc);
            }
            Err(err) => errors.push(err),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    let mut queues = BTreeMap::new();
    for (queue_name, raw) in &spec.queues {
        let max_receives = raw.and_then(|q| q.dlq).and_then(|d| d.max_receives).unwrap_or(DEFAULT_MAX_RECEIVES);
        queues.insert(name(queue_name)?, Queue { dlq: Dlq { max_receives } });
    }
    let mut databases = BTreeMap::new();
    for (db_name, raw) in &spec.databases {
        let db = match (raw.engine, &raw.key) {
            (Engine::Postgres, _) => Database::Postgres { size: raw.size.unwrap_or_default() },
            (Engine::Dynamodb, Some(key)) => {
                Database::Dynamodb { key: Key { partition: key.partition.clone(), sort: key.sort.clone() } }
            }
            // Reported by validation (E017); resolve is only called on valid specs.
            (Engine::Dynamodb, None) => continue,
        };
        databases.insert(name(db_name)?, db);
    }

    let mut model = Model {
        app: App {
            name: name(&spec.name)?,
            env,
            region: spec.aws.as_ref().and_then(|a| a.region.clone()),
            arch: spec.aws.as_ref().and_then(|a| a.arch).unwrap_or_default(),
            protect: guarded,
        },
        services,
        queues,
        databases,
        uses: Vec::new(),
        plugins: spec
            .plugins
            .iter()
            .map(|p| Ok(Plugin { name: name(&p.name)?, path: p.path.clone(), options: p.options.clone() }))
            .collect::<Result<_, Vec<Diagnostic>>>()?,
    };
    let mut uses = Vec::new();
    for (svc_name, svc) in &model.services {
        for resource in &svc.uses {
            let kind = model.resource_kind(resource).unwrap_or(ResourceKind::Queue);
            uses.push(Use { service: svc_name.clone(), resource: resource.clone(), kind });
        }
    }
    uses.sort();
    model.uses = uses;
    Ok(model)
}

fn name(text: &str) -> Result<Name, Vec<Diagnostic>> {
    // Names were checked by validation; this only guards against calling resolve on an
    // unvalidated spec.
    Name::new(text).map_err(|e| vec![Diagnostic::error(Code::InvalidName, format!("invalid name `{text}`: {e}"))])
}

fn unknown_environment(doc: &Document, spec: &RawSpec, env: &str) -> Diagnostic {
    let known: Vec<&str> = spec.environments.keys().map(String::as_str).collect();
    let help = did_you_mean(env, known.iter().copied()).unwrap_or_else(|| {
        if known.is_empty() {
            format!("add `environments:` with `{env}: {{}}` to the spec")
        } else {
            format!("declared environments: {}; add `{env}:` under `environments`", backticked(&known))
        }
    });
    Diagnostic::error(Code::UnknownEnvironment, format!("unknown environment `{env}`"))
        .at(doc.key_span(&["environments"]), format!("`{env}` is not declared here"))
        .with_help(help)
}

fn backticked(items: &[&str]) -> String {
    items.iter().map(|i| format!("`{i}`")).collect::<Vec<_>>().join(", ")
}

fn service(doc: &Document, svc_name: &str, raw: &RawService, project: &dyn Project) -> Result<Service, Diagnostic> {
    let build = project::normalize_path(&raw.build);
    let build_span = doc.span(&["services", svc_name, "build"]);
    if !project.is_dir(&build) {
        return Err(Diagnostic::error(Code::BuildDirMissing, format!("build folder `{build}` does not exist"))
            .at(build_span, "folder not found")
            .with_help("`build` is a folder relative to shipfile.yaml that holds the service's code"));
    }
    let (runtime, runtime_source) = match raw.runtime.as_deref().and_then(Runtime::parse) {
        Some(runtime) => (Some(runtime), Some("spec".to_owned())),
        None => match project::detect_runtime(project, &build) {
            Some((runtime, manifest)) => (Some(runtime), Some(manifest)),
            None if project.is_file(&project::join(&build, "Dockerfile")) => (None, None),
            None => {
                return Err(Diagnostic::error(
                    Code::RuntimeNotDetected,
                    format!("can't tell which runtime service `{svc_name}` uses"),
                )
                .at(build_span, format!("no {} here", project::MANIFESTS.join(", ")))
                .with_help(
                    "set `runtime:` (for example `node22` or `python3.13`), or add a Dockerfile to the folder",
                ));
            }
        },
    };
    let cpu = raw.scale.and_then(|s| s.cpu).unwrap_or(DEFAULT_CPU);
    let scale = Scale {
        cpu,
        memory: raw.scale.and_then(|s| s.memory).unwrap_or_else(|| default_memory(cpu)),
        replicas: raw.scale.and_then(|s| s.replicas).unwrap_or(DEFAULT_REPLICAS),
    };
    let mut secrets = raw.secrets.clone();
    secrets.sort();
    secrets.dedup();
    let mut uses: Vec<Name> = raw.uses.iter().filter_map(|u| Name::new(u).ok()).collect();
    uses.sort();
    uses.dedup();
    Ok(Service {
        build,
        runtime,
        runtime_source,
        port: raw.port.and_then(|p| u16::try_from(p).ok()),
        public: raw.public.unwrap_or(false),
        health: raw.health.clone(),
        scale,
        env: raw.env.iter().map(|(k, v)| (k.clone(), v.to_string())).collect(),
        secrets,
        uses,
        command: raw.command.clone(),
        migrate: raw.migrate.clone(),
        health_cmd: raw.health_cmd.clone(),
    })
}
