//! Semantic checks (`docs/PLAN.md` §5.3 step 3), run after the schema has passed.

use std::collections::BTreeMap;

use crate::contract::{BASE_VARS, resource_var_names};
use crate::diagnostics::{Code, Diagnostic, did_you_mean};
use crate::model::ResourceKind;
use crate::name::{self, Name};
use crate::overlay;
use crate::parse::{Document, Span};
use crate::spec::{Engine, RawSpec};

/// Where to point: the base spec, or an environment whose overrides win when present.
struct Ctx<'a> {
    doc: &'a Document,
    env: Option<&'a str>,
}

impl Ctx<'_> {
    fn span(&self, path: &[&str]) -> Option<Span> {
        if let Some(env) = self.env {
            let mut overlay = vec!["environments", env];
            overlay.extend_from_slice(path);
            if self.doc.get(&overlay).is_some() {
                return self.doc.span(&overlay);
            }
        }
        self.doc.span(path)
    }

    fn key_span(&self, path: &[&str]) -> Option<Span> {
        self.doc.key_span(path).or_else(|| self.doc.span(path))
    }

    fn suffix(&self) -> String {
        self.env.map(|e| format!(" in environment `{e}`")).unwrap_or_default()
    }
}

/// Runs every semantic check on the base spec and on each environment.
pub fn validate(doc: &Document, spec: &RawSpec) -> Vec<Diagnostic> {
    let base = Ctx { doc, env: None };
    let mut out = Vec::new();
    check_version(&base, spec, &mut out);
    check_names(&base, spec, &mut out);
    check_uses(&base, spec, &mut out);
    check_engines(&base, spec, &mut out);
    check_overlays(&base, spec, &mut out);
    check_rules(&base, spec, &mut out);
    // Rules that an environment's overrides can break are checked again with them applied.
    // Only new problems are kept, so one mistake in the base isn't reported once per environment.
    for (env, overrides) in &spec.environments {
        let Some(overrides) = overrides else { continue };
        let merged = overlay::apply(spec, overrides);
        let mut found = Vec::new();
        check_rules(&Ctx { doc, env: Some(env) }, &merged, &mut found);
        for diag in found {
            if !out.iter().any(|d| d.code == diag.code && d.primary_span() == diag.primary_span()) {
                out.push(diag);
            }
        }
    }
    out
}

fn check_version(cx: &Ctx, spec: &RawSpec, out: &mut Vec<Diagnostic>) {
    if spec.version != 1 {
        out.push(
            Diagnostic::error(Code::UnsupportedVersion, format!("unsupported spec version {}", spec.version))
                .at(cx.span(&["version"]), "this shipfile only understands version 1")
                .with_help("set `version: 1`, or install a newer shipfile"),
        );
    }
}

fn name_error(what: &str, name: &str, err: &name::NameError, span: Option<Span>) -> Diagnostic {
    let help = match name::suggest_fix(name) {
        Some(fixed) => format!("names must be DNS-safe; try `{fixed}`"),
        None => "names must be DNS-safe: lowercase letters, digits and single `-`, starting with a letter".to_owned(),
    };
    Diagnostic::error(Code::InvalidName, format!("invalid {what} name `{name}`"))
        .at(span, err.to_string())
        .with_help(help)
}

const KINDS: [(&str, &str); 3] = [("services", "service"), ("queues", "queue"), ("databases", "database")];

fn kind_names<'a>(spec: &'a RawSpec, section: &str) -> Vec<&'a str> {
    match section {
        "services" => spec.services.keys().map(String::as_str).collect(),
        "queues" => spec.queues.keys().map(String::as_str).collect(),
        _ => spec.databases.keys().map(String::as_str).collect(),
    }
}

fn check_names(cx: &Ctx, spec: &RawSpec, out: &mut Vec<Diagnostic>) {
    if let Err(err) = Name::new(&spec.name) {
        out.push(name_error("app", &spec.name, &err, cx.span(&["name"])));
    }
    let mut seen: BTreeMap<&str, &str> = BTreeMap::new();
    for (section, what) in KINDS {
        for name in kind_names(spec, section) {
            let span = cx.key_span(&[section, name]);
            if let Err(err) = Name::new(name) {
                out.push(name_error(what, name, &err, span));
            }
            if let Some(first) = seen.insert(name, what) {
                out.push(
                    Diagnostic::error(Code::DuplicateName, format!("the name `{name}` is used twice"))
                        .at(span, format!("already used by a {first}"))
                        .with_help("services, queues and databases share one set of names; rename one of them"),
                );
            }
        }
    }
    for env in spec.environments.keys() {
        if let Err(err) = Name::new(env) {
            out.push(name_error("environment", env, &err, cx.key_span(&["environments", env])));
        }
    }
    for (i, plugin) in spec.plugins.iter().enumerate() {
        if let Err(err) = Name::new(&plugin.name) {
            out.push(name_error("plugin", &plugin.name, &err, cx.span(&["plugins", &i.to_string(), "name"])));
        }
    }
}

fn resource_kind(spec: &RawSpec, name: &str) -> Option<ResourceKind> {
    if spec.queues.contains_key(name) {
        return Some(ResourceKind::Queue);
    }
    spec.databases.get(name).map(|db| match db.engine {
        Engine::Postgres => ResourceKind::Postgres,
        Engine::Dynamodb => ResourceKind::Dynamodb,
    })
}

fn check_uses(cx: &Ctx, spec: &RawSpec, out: &mut Vec<Diagnostic>) {
    let resources: Vec<&str> = spec.queues.keys().chain(spec.databases.keys()).map(String::as_str).collect();
    for (service, svc) in &spec.services {
        for (i, target) in svc.uses.iter().enumerate() {
            let index = i.to_string();
            let span = cx.span(&["services", service, "uses", &index]);
            if svc.uses[..i].contains(target) {
                out.push(
                    Diagnostic::error(Code::DuplicateEntry, format!("`{target}` is listed twice in `uses`"))
                        .at(span, "listed again here")
                        .with_help("remove the repeated entry"),
                );
            } else if resource_kind(spec, target).is_none() {
                let (label, help) = if spec.services.contains_key(target) {
                    (
                        format!("`{target}` is a service"),
                        Some("`uses` lists queues and databases; services call each other over HTTP".to_owned()),
                    )
                } else {
                    (
                        "not defined under `queues` or `databases`".to_owned(),
                        did_you_mean(target, resources.iter().copied()),
                    )
                };
                let diag = Diagnostic::error(Code::UnknownUse, format!("unknown resource `{target}` in `uses`"))
                    .at(span, label);
                out.push(match help {
                    Some(help) => diag.with_help(help),
                    None => diag.with_help(format!("define `{target}` under `queues` or `databases`")),
                });
            }
        }
    }
}

fn check_engines(cx: &Ctx, spec: &RawSpec, out: &mut Vec<Diagnostic>) {
    for (name, db) in &spec.databases {
        match db.engine {
            Engine::Dynamodb => {
                if db.key.is_none() {
                    out.push(
                        Diagnostic::error(Code::DynamodbNeedsKey, format!("DynamoDB table `{name}` has no `key`"))
                            .at(cx.key_span(&["databases", name]), "needs a partition key")
                            .with_help("add `key: { partition: id }` (a `sort` key is optional)"),
                    );
                }
                if db.size.is_some() {
                    out.push(field_not_for_engine(cx.span(&["databases", name, "size"]), "size", name, "postgres"));
                }
            }
            Engine::Postgres => {
                if db.key.is_some() {
                    out.push(field_not_for_engine(cx.span(&["databases", name, "key"]), "key", name, "dynamodb"));
                }
            }
        }
    }
}

fn field_not_for_engine(span: Option<Span>, field: &str, db: &str, engine: &str) -> Diagnostic {
    Diagnostic::error(Code::FieldNotForEngine, format!("`{field}` can't be set on database `{db}`"))
        .at(span, format!("`{field}` is only for {engine} databases"))
        .with_help(format!("remove `{field}`, or set `engine: {engine}`"))
}

fn check_overlays(cx: &Ctx, spec: &RawSpec, out: &mut Vec<Diagnostic>) {
    for (env, overrides) in &spec.environments {
        let Some(overrides) = overrides else { continue };
        let sections: [(&str, &str, Vec<&str>); 3] = [
            ("services", "service", overrides.services.keys().map(String::as_str).collect()),
            ("queues", "queue", overrides.queues.keys().map(String::as_str).collect()),
            ("databases", "database", overrides.databases.keys().map(String::as_str).collect()),
        ];
        for (section, what, names) in sections {
            let known = kind_names(spec, section);
            for name in names.into_iter().filter(|n| !known.contains(n)) {
                let help = did_you_mean(name, known.iter().copied())
                    .unwrap_or_else(|| format!("environments can only override {section} defined at the top level"));
                out.push(
                    Diagnostic::error(
                        Code::OverlayUnknownResource,
                        format!("environment `{env}` overrides unknown {what} `{name}`"),
                    )
                    .at(cx.key_span(&["environments", env, section, name]), format!("no {what} called `{name}`"))
                    .with_help(help),
                );
            }
        }
        for (name, patch) in &overrides.databases {
            if patch.size.is_some() && spec.databases.get(name).is_some_and(|db| db.engine == Engine::Dynamodb) {
                out.push(field_not_for_engine(
                    cx.span(&["environments", env, "databases", name, "size"]),
                    "size",
                    name,
                    "postgres",
                ));
            }
        }
    }
}

/// Valid Fargate memory sizes (MiB) for a CPU size.
pub fn fargate_memory(cpu: u32) -> Vec<u32> {
    match cpu {
        256 => vec![512, 1024, 2048],
        512 => (1..=4).map(|g| g * 1024).collect(),
        1024 => (2..=8).map(|g| g * 1024).collect(),
        2048 => (4..=16).map(|g| g * 1024).collect(),
        4096 => (8..=30).map(|g| g * 1024).collect(),
        _ => Vec::new(),
    }
}

/// The memory used when only `cpu` is given: the smallest size Fargate allows for it.
pub fn default_memory(cpu: u32) -> u32 {
    fargate_memory(cpu).first().copied().unwrap_or(crate::model::DEFAULT_MEMORY)
}

fn describe_sizes(sizes: &[u32]) -> String {
    match sizes {
        [] => "nothing".to_owned(),
        [a, b, c] => format!("{a}, {b} or {c}"),
        [first, .., last] => format!("{first} to {last} in steps of 1024"),
        [only] => only.to_string(),
    }
}

/// Checks that environments can break, so they run on the base spec and on each environment.
fn check_rules(cx: &Ctx, spec: &RawSpec, out: &mut Vec<Diagnostic>) {
    let suffix = cx.suffix();
    let mut ports: BTreeMap<u32, &str> = BTreeMap::new();
    for (name, svc) in &spec.services {
        let path = |field: &'static str| ["services", name.as_str(), field];
        if svc.public == Some(true) {
            if svc.port.is_none() {
                out.push(
                    Diagnostic::error(Code::PublicNeedsPort, format!("public service `{name}` has no `port`{suffix}"))
                        .at(cx.span(&path("public")), "public services need a port for the load balancer")
                        .with_help("add `port:` with the port the service listens on"),
                );
            }
            if svc.health.is_none() {
                out.push(
                    Diagnostic::error(
                        Code::PublicNeedsHealth,
                        format!("public service `{name}` has no `health` path{suffix}"),
                    )
                    .at(cx.span(&path("public")), "public services need a health check")
                    .with_help("add `health: /healthz`, a path that returns 200 when the service is ready"),
                );
            }
        }
        if let Some(port) = svc.port
            && let Some(first) = ports.insert(port, name)
        {
            out.push(
                Diagnostic::error(Code::PortClash, format!("port {port} is used by `{first}` and `{name}`{suffix}"))
                    .at(cx.span(&path("port")), format!("already used by `{first}`"))
                    .with_help("services run side by side locally, so each needs its own port"),
            );
        }
        if let Some(scale) = svc.scale {
            let cpu = scale.cpu.unwrap_or(crate::model::DEFAULT_CPU);
            let memory = scale.memory.unwrap_or_else(|| default_memory(cpu));
            let valid = fargate_memory(cpu);
            if !valid.contains(&memory) {
                let field = if scale.memory.is_some() { "memory" } else { "cpu" };
                out.push(
                    Diagnostic::error(
                        Code::InvalidScale,
                        format!("service `{name}` can't have {memory} MiB with cpu {cpu}{suffix}"),
                    )
                    .at(cx.span(&["services", name, "scale", field]), "not a Fargate size")
                    .with_help(format!("with cpu {cpu}, memory can be {}", describe_sizes(&valid))),
                );
            }
        }
        check_env_vars(cx, spec, name, out);
    }
}

fn check_env_vars(cx: &Ctx, spec: &RawSpec, service: &str, out: &mut Vec<Diagnostic>) {
    let svc = &spec.services[service];
    let suffix = cx.suffix();
    let mut generated: BTreeMap<String, String> =
        BASE_VARS.iter().map(|v| ((*v).to_owned(), "shipfile".to_owned())).collect();
    for target in &svc.uses {
        if let (Ok(name), Some(kind)) = (Name::new(target), resource_kind(spec, target)) {
            for var in resource_var_names(&name, kind) {
                generated.insert(var, format!("`{target}`"));
            }
        }
    }
    let clash = |var: &str, span: Option<Span>, by: &str| {
        Diagnostic::error(Code::EnvVarCollision, format!("`{var}` is already set by {by}{suffix}"))
            .at(span, "set again here")
            .with_help("rename this variable; shipfile sets it for you")
    };
    for key in svc.env.keys() {
        let span = cx.key_span(&["services", service, "env", key]);
        if !name::is_env_var(key) {
            out.push(env_var_name_error(key, span));
        } else if let Some(by) = generated.get(key) {
            out.push(clash(key, span, by));
        }
    }
    for (i, key) in svc.secrets.iter().enumerate() {
        let span = cx.span(&["services", service, "secrets", &i.to_string()]);
        if !name::is_env_var(key) {
            out.push(env_var_name_error(key, span));
        } else if svc.secrets[..i].contains(key) {
            out.push(
                Diagnostic::error(Code::DuplicateEntry, format!("secret `{key}` is listed twice{suffix}"))
                    .at(span, "listed again here")
                    .with_help("remove the repeated entry"),
            );
        } else if let Some(by) = generated.get(key) {
            out.push(clash(key, span, by));
        } else if svc.env.contains_key(key) {
            out.push(
                Diagnostic::error(Code::EnvVarCollision, format!("`{key}` is both a secret and an env var{suffix}"))
                    .at(span, "also set under `env`")
                    .with_help("keep it in `secrets` only, so its value never lands in shipfile.yaml"),
            );
        }
    }
}

fn env_var_name_error(key: &str, span: Option<Span>) -> Diagnostic {
    Diagnostic::error(Code::InvalidName, format!("invalid environment variable name `{key}`"))
        .at(span, "not a valid variable name")
        .with_help("use letters, digits and `_`, not starting with a digit, for example `LOG_LEVEL`")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(yaml: &str) -> Vec<Diagnostic> {
        let doc = Document::parse(yaml).unwrap();
        assert!(crate::schema::validate(&doc).is_empty(), "schema errors in test input");
        let spec: RawSpec = serde_json::from_value(doc.to_json()).unwrap();
        validate(&doc, &spec)
    }

    fn codes(yaml: &str) -> Vec<&'static str> {
        check(yaml).iter().map(|d| d.code.as_str()).collect()
    }

    const BASE: &str = "version: 1\nname: shop\n";

    #[test]
    fn uses_typo_suggests_the_resource() {
        let errs = check(&format!(
            "{BASE}services:\n  api:\n    build: .\n    uses: [orders-qeue]\nqueues:\n  orders-queue:\n"
        ));
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, Code::UnknownUse);
        assert_eq!(errs[0].help.as_deref(), Some("did you mean `orders-queue`?"));
    }

    #[test]
    fn names_are_unique_across_kinds() {
        let yaml = format!("{BASE}services:\n  jobs:\n    build: .\nqueues:\n  jobs:\n");
        assert_eq!(codes(&yaml), ["E011"]);
    }

    #[test]
    fn environment_can_break_rules_and_is_reported_once() {
        let yaml = format!(
            "{BASE}services:\n  api:\n    build: .\n    port: 80\nenvironments:\n  prod:\n    services:\n      api: {{ public: true }}\n"
        );
        let errs = check(&yaml);
        assert_eq!(errs.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(), ["E016"]);
        assert!(errs[0].message.ends_with("in environment `prod`"));
    }

    #[test]
    fn base_errors_are_not_repeated_per_environment() {
        let yaml = format!(
            "{BASE}services:\n  api:\n    build: .\n    public: true\n    port: 80\nenvironments:\n  dev:\n  prod: {{}}\n"
        );
        assert_eq!(codes(&yaml), ["E016"]);
    }

    #[test]
    fn env_vars_cannot_shadow_generated_ones() {
        let yaml = format!(
            "{BASE}services:\n  api:\n    build: .\n    uses: [q]\n    env: {{ Q_URL: x, AWS_REGION: y }}\nqueues:\n  q:\n"
        );
        assert_eq!(codes(&yaml), ["E022", "E022"]);
    }

    #[test]
    fn fargate_sizes() {
        assert_eq!(describe_sizes(&fargate_memory(256)), "512, 1024 or 2048");
        assert_eq!(describe_sizes(&fargate_memory(1024)), "2048 to 8192 in steps of 1024");
        let yaml = format!("{BASE}services:\n  api:\n    build: .\n    scale: {{ cpu: 1024, memory: 512 }}\n");
        assert_eq!(codes(&yaml), ["E019"]);
    }
}
