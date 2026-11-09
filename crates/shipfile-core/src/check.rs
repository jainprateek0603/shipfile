//! Validation in the order of PLAN §5.3: parse → schema → semantics (with suggestions).

use std::collections::{BTreeMap, BTreeSet};

use crate::contract::{PLATFORM_VARS, resource_var_names};
use crate::diagnostics::{Code, Diagnostic, SourceMap, normalize, pointer};
use crate::model::{ResourceKind, overlay};
use crate::parse::{self, Node, Value};
use crate::schema;
use crate::spec::{Engine, Spec};

/// Longest allowed name for the app, services, resources and environments.
pub const MAX_NAME_LEN: usize = 32;

/// The result of checking a spec.
#[derive(Debug, Clone)]
pub struct Checked {
    /// The typed spec, when it got past the schema stage.
    pub spec: Option<Spec>,
    /// Errors and warnings, sorted by position.
    pub diagnostics: Vec<Diagnostic>,
}

impl Checked {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(Diagnostic::is_error)
    }
}

/// Runs every check on a spec's source. `env` is the `-e/--env` the user selected; when it is
/// `None`, every declared environment's merged result is checked.
pub fn check(source: &str, env: Option<&str>) -> Checked {
    let fail = |diagnostics| Checked { spec: None, diagnostics };

    // 1. Parse.
    let root = match parse::parse(source) {
        Ok(root) => root,
        Err(e) => return fail(vec![Diagnostic::new(Code::E001, "invalid YAML").at(Some(e.span), e.message)]),
    };
    let map = SourceMap::new(&root);
    let mut diagnostics = Vec::new();
    duplicate_keys(&root, &mut diagnostics);

    // 2. Schema.
    let json = root.to_json();
    diagnostics.extend(schema::validate(&json, &map));
    if diagnostics.iter().any(Diagnostic::is_error) {
        normalize(&mut diagnostics);
        return fail(diagnostics);
    }
    let spec: Spec = match serde_json::from_value(json) {
        Ok(spec) => spec,
        Err(e) => {
            return fail(vec![Diagnostic::new(Code::E008, format!("spec does not match its schema: {e}"))]);
        }
    };

    // 3–4. Semantics, with suggestions.
    diagnostics.extend(semantics(&spec, &map, env));
    normalize(&mut diagnostics);
    Checked { spec: Some(spec), diagnostics }
}

fn duplicate_keys(node: &Node, out: &mut Vec<Diagnostic>) {
    match &node.value {
        Value::Map(entries) => {
            for (i, e) in entries.iter().enumerate() {
                if let Some(first) = entries[..i].iter().find(|f| f.key == e.key) {
                    out.push(
                        Diagnostic::new(Code::E002, format!("duplicate key `{}`", e.key))
                            .at(Some(e.key_span), "defined again here")
                            .also(Some(first.key_span), "first defined here")
                            .help("remove one of them; YAML keys must be unique"),
                    );
                }
                duplicate_keys(&e.value, out);
            }
        }
        Value::Seq(items) => items.iter().for_each(|i| duplicate_keys(i, out)),
        _ => {}
    }
}

/// Finds positions, preferring the selected environment's override over the base spec.
struct Loc<'a> {
    map: &'a SourceMap,
    env: Option<&'a str>,
}

impl Loc<'_> {
    fn field(&self, section: &str, name: &str, field: &[&str], key: bool) -> Option<crate::parse::Span> {
        let lookup = |p: &str| if key { self.map.key(p) } else { self.map.value(p) };
        let tail = || [section, name].into_iter().chain(field.iter().copied());
        self.env
            .and_then(|env| lookup(&pointer(["environments", env].into_iter().chain(tail()))))
            .or_else(|| lookup(&pointer(tail())))
            .or_else(|| self.map.key(&pointer([section, name])))
    }
}

fn semantics(spec: &Spec, map: &SourceMap, env: Option<&str>) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    names(spec, map, &mut out);
    uses(spec, map, &mut out);
    databases(spec, map, &mut out);
    environments(spec, map, env, &mut out);
    plugins(spec, map, &mut out);
    unused(spec, map, &mut out);

    // Checks on fields an environment can override run on the base spec and on each
    // environment's merged spec; identical findings are de-duplicated later.
    services(spec, &Loc { map, env: None }, &mut out);
    let selected: Vec<&str> = match env {
        Some(e) if spec.environments.contains_key(e) => vec![e],
        Some(_) => vec![],
        None => spec.environments.keys().map(String::as_str).collect(),
    };
    for e in selected {
        services(&overlay(spec, e), &Loc { map, env: Some(e) }, &mut out);
    }
    out
}

fn dns_problem(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        Some("names cannot be empty")
    } else if name.len() > MAX_NAME_LEN {
        Some("names are at most 32 characters")
    } else if !name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
        Some("use only lowercase letters, digits and `-`")
    } else if !name.as_bytes()[0].is_ascii_lowercase() {
        Some("names start with a lowercase letter")
    } else if name.ends_with('-') {
        Some("names cannot end with `-`")
    } else if name.contains("--") {
        Some("names cannot contain `--`")
    } else {
        None
    }
}

/// A DNS-safe spelling of `name`, for the help line.
pub fn dns_safe(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_start_matches(|c: char| c.is_ascii_digit() || c == '-');
    let mut out: String = out.chars().take(MAX_NAME_LEN).collect();
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() { "app".to_owned() } else { out }
}

fn check_name(what: &str, name: &str, span: Option<crate::parse::Span>, out: &mut Vec<Diagnostic>) {
    if let Some(problem) = dns_problem(name) {
        let fixed = dns_safe(name);
        out.push(
            Diagnostic::new(Code::E010, format!("{what} `{name}` is not DNS-safe"))
                .at(span, problem)
                .help(format!("names become AWS resource names; try `{fixed}`")),
        );
    }
}

fn names(spec: &Spec, map: &SourceMap, out: &mut Vec<Diagnostic>) {
    check_name("app name", &spec.name, map.value("/name"), out);
    let mut seen: BTreeMap<&str, &str> = BTreeMap::new();
    for (section, what, names) in [
        ("services", "service name", spec.services.keys().collect::<Vec<_>>()),
        ("queues", "queue name", spec.queues.keys().collect()),
        ("databases", "database name", spec.databases.keys().collect()),
        ("environments", "environment name", spec.environments.keys().collect()),
    ] {
        for name in names {
            let span = map.key(&pointer([section, name]));
            check_name(what, name, span, out);
            if section == "environments" {
                continue;
            }
            if let Some(first) = seen.get(name.as_str()) {
                out.push(
                    Diagnostic::new(Code::E011, format!("`{name}` is defined under both `{first}` and `{section}`"))
                        .at(span, "second definition")
                        .also(map.key(&pointer([*first, name])), "first definition")
                        .help("names must be unique across services, queues and databases; rename one of them"),
                );
            } else {
                seen.insert(name, section);
            }
        }
    }
    let regions = std::iter::once((pointer(["aws", "region"]), spec.aws.as_ref().and_then(|a| a.region.as_ref())))
        .chain(spec.environments.iter().map(|(e, env)| {
            (pointer(["environments", e, "aws", "region"]), env.aws.as_ref().and_then(|a| a.region.as_ref()))
        }));
    for (ptr, region) in regions {
        if let Some(region) = region
            && !is_region(region)
        {
            out.push(
                Diagnostic::new(Code::E026, format!("`{region}` is not an AWS region name"))
                    .at(map.value(&ptr), "expected something like `ap-south-1`"),
            );
        }
    }
}

fn is_region(r: &str) -> bool {
    let parts: Vec<&str> = r.split('-').collect();
    let (geo, rest) = match parts.as_slice() {
        [geo, "gov" | "iso" | "isob", rest @ ..] | [geo, rest @ ..] => (*geo, rest),
        [] => return false,
    };
    matches!(rest, [area, n] if !area.is_empty() && area.bytes().all(|b| b.is_ascii_lowercase())
        && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        && geo.len() == 2
        && geo.bytes().all(|b| b.is_ascii_lowercase())
}

fn kind_of(spec: &Spec, name: &str) -> Option<ResourceKind> {
    if spec.queues.contains_key(name) {
        return Some(ResourceKind::Queue);
    }
    spec.databases.get(name).map(|db| match db.engine {
        Engine::Postgres => ResourceKind::Postgres,
        Engine::Dynamodb => ResourceKind::Dynamodb,
    })
}

fn uses(spec: &Spec, map: &SourceMap, out: &mut Vec<Diagnostic>) {
    let resources: Vec<&str> = spec.queues.keys().chain(spec.databases.keys()).map(String::as_str).collect();
    for (svc, s) in &spec.services {
        let mut seen = BTreeMap::new();
        for (i, name) in s.uses.iter().enumerate() {
            let i = i.to_string();
            let span = map.value(&pointer(["services", svc, "uses", &i]));
            if let Some(first) = seen.insert(name.as_str(), span) {
                out.push(
                    Diagnostic::new(Code::E014, format!("`{name}` is listed twice in `uses`"))
                        .at(span, "listed again here")
                        .also(first, "first listed here"),
                );
                continue;
            }
            if kind_of(spec, name).is_some() {
                continue;
            }
            let d = if spec.services.contains_key(name) {
                Diagnostic::new(Code::E013, format!("`{name}` in `uses` is a service"))
                    .at(span, "services cannot be used like queues or databases")
                    .help("to call another service, use its URL; only queues and databases go in `uses`")
            } else {
                let label = if resources.is_empty() {
                    "no queues or databases are defined".to_owned()
                } else {
                    "not defined under `queues` or `databases`".to_owned()
                };
                Diagnostic::new(Code::E012, format!("unknown resource `{name}` in `uses`"))
                    .at(span, label)
                    .suggest(name, resources.iter().copied())
            };
            out.push(d);
        }
    }
}

fn services(spec: &Spec, loc: &Loc<'_>, out: &mut Vec<Diagnostic>) {
    let mut ports: BTreeMap<u16, (String, Option<crate::parse::Span>)> = BTreeMap::new();
    if spec.databases.values().any(|d| d.engine == Engine::Postgres) {
        ports.insert(5432, ("the local Postgres container".to_owned(), None));
    }
    if !spec.queues.is_empty() {
        ports.insert(9324, ("the local ElasticMQ (SQS) container".to_owned(), None));
    }
    if spec.databases.values().any(|d| d.engine == Engine::Dynamodb) {
        ports.insert(8000, ("the local DynamoDB container".to_owned(), None));
    }

    for (svc, s) in &spec.services {
        let at = |field: &[&str], key: bool| loc.field("services", svc, field, key);
        if s.public == Some(true) {
            if s.port.is_none() {
                out.push(
                    Diagnostic::new(Code::E016, format!("public service `{svc}` has no `port`"))
                        .at(at(&["public"], true), "public services need a port for the load balancer")
                        .help("add `port: <number>` (the port your app listens on)"),
                );
            }
            if s.health.is_none() {
                out.push(
                    Diagnostic::new(Code::E017, format!("public service `{svc}` has no `health` path"))
                        .at(at(&["public"], true), "the load balancer needs a health check")
                        .help("add `health: /healthz` (any path that answers 200 when the app is up)"),
                );
            }
        }
        if let Some(health) = &s.health
            && !health.starts_with('/')
        {
            out.push(
                Diagnostic::new(Code::E018, format!("`health` of `{svc}` must start with `/`"))
                    .at(at(&["health"], false), "not an absolute path")
                    .help(format!("try `/{}`", health.trim_start_matches('/'))),
            );
        }
        if let Some(port) = s.port {
            let span = at(&["port"], false);
            match ports.get(&port) {
                Some((other, other_span)) => out.push(
                    Diagnostic::new(Code::E015, format!("port {port} of `{svc}` clashes with {other}"))
                        .at(span, "already taken locally")
                        .also(*other_span, "first used here")
                        .help("every service needs its own port locally; pick another one"),
                ),
                None => {
                    ports.insert(port, (format!("service `{svc}`"), span));
                }
            }
        }

        // Variable names and collisions with the env contract.
        let mut taken: BTreeMap<String, String> =
            PLATFORM_VARS.iter().map(|v| ((*v).to_owned(), "the platform".to_owned())).collect();
        for r in &s.uses {
            if let Some(kind) = kind_of(spec, r) {
                for v in resource_var_names(kind, r) {
                    taken.insert(v, format!("`uses: {r}`"));
                }
            }
        }
        let mut secrets_seen = BTreeSet::new();
        for (i, key) in s.secrets.iter().enumerate() {
            let span = at(&["secrets", &i.to_string()], false);
            if !secrets_seen.insert(key) {
                out.push(
                    Diagnostic::new(Code::E014, format!("secret `{key}` is listed twice"))
                        .at(span, "listed again here"),
                );
                continue;
            }
            var_name("secret", key, span, &taken, out);
            taken.insert(key.clone(), format!("secret `{key}`"));
        }
        for key in s.env.keys() {
            let span = at(&["env", key], true);
            var_name("environment variable", key, span, &taken, out);
        }
    }
}

fn var_name(
    what: &str,
    key: &str,
    span: Option<crate::parse::Span>,
    taken: &BTreeMap<String, String>,
    out: &mut Vec<Diagnostic>,
) {
    let valid = key.bytes().next().is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    if !valid {
        let mut fixed: String =
            key.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' }).collect();
        if fixed.starts_with(|c: char| c.is_ascii_digit()) {
            fixed.insert(0, '_');
        }
        out.push(
            Diagnostic::new(Code::E023, format!("`{key}` is not a valid {what} name"))
                .at(span, "use letters, digits and `_`, not starting with a digit")
                .help(format!("try `{fixed}`")),
        );
    } else if let Some(owner) = taken.get(key) {
        out.push(
            Diagnostic::new(Code::E024, format!("{what} `{key}` is already set by {owner}"))
                .at(span, "defined twice")
                .help("Shipfile sets this variable for you; remove it or pick another name"),
        );
    }
}

fn databases(spec: &Spec, map: &SourceMap, out: &mut Vec<Diagnostic>) {
    for (name, db) in &spec.databases {
        let p = |f: &str| pointer(["databases", name, f]);
        match db.engine {
            Engine::Dynamodb => {
                if db.key.is_none() {
                    out.push(
                        Diagnostic::new(Code::E019, format!("DynamoDB table `{name}` has no `key`"))
                            .at(map.key(&pointer(["databases", name])), "needs a partition key")
                            .help("add `key: { partition: id }` (a sort key is optional)"),
                    );
                }
                if db.size.is_some() {
                    out.push(engine_field(
                        "size",
                        "dynamodb",
                        map.key(&p("size")),
                        "only Postgres databases have a size",
                    ));
                }
            }
            Engine::Postgres => {
                if db.key.is_some() {
                    out.push(engine_field("key", "postgres", map.key(&p("key")), "only DynamoDB tables have a key"));
                }
            }
        }
    }
}

fn engine_field(field: &str, engine: &str, span: Option<crate::parse::Span>, why: &str) -> Diagnostic {
    Diagnostic::new(Code::E020, format!("`{field}` does not apply to `{engine}` databases"))
        .at(span, why)
        .help(format!("remove `{field}`"))
}

fn environments(spec: &Spec, map: &SourceMap, env: Option<&str>, out: &mut Vec<Diagnostic>) {
    for (e, patch) in &spec.environments {
        let sections: [(&str, Vec<&String>, Vec<&str>); 3] = [
            ("services", patch.services.keys().collect(), spec.services.keys().map(String::as_str).collect()),
            ("queues", patch.queues.keys().collect(), spec.queues.keys().map(String::as_str).collect()),
            ("databases", patch.databases.keys().collect(), spec.databases.keys().map(String::as_str).collect()),
        ];
        for (section, names, known) in sections {
            for name in names {
                if !known.contains(&name.as_str()) {
                    out.push(
                        Diagnostic::new(
                            Code::E021,
                            format!("environment `{e}` overrides unknown {section} entry `{name}`"),
                        )
                        .at(
                            map.key(&pointer(["environments", e, section, name])),
                            format!("not defined under `{section}`"),
                        )
                        .suggest(name, known.iter().copied()),
                    );
                }
            }
        }
        for (name, db) in &patch.databases {
            if db.size.is_some() && spec.databases.get(name).is_some_and(|d| d.engine == Engine::Dynamodb) {
                out.push(engine_field(
                    "size",
                    "dynamodb",
                    map.key(&pointer(["environments", e, "databases", name, "size"])),
                    "only Postgres databases have a size",
                ));
            }
        }
    }
    if let Some(e) = env
        && !spec.environments.contains_key(e)
    {
        let known: Vec<&str> = spec.environments.keys().map(String::as_str).collect();
        let mut d = Diagnostic::new(Code::E022, format!("environment `{e}` is not declared"))
            .at(map.key("/environments"), "declared environments are listed here")
            .suggest(e, known.iter().copied());
        d = if known.is_empty() {
            d.help(format!("add `environments: {{ {e}: {{}} }}` to the spec"))
        } else {
            d.help(format!("declared: {}", known.join(", ")))
        };
        out.push(d);
    }
}

fn plugins(spec: &Spec, map: &SourceMap, out: &mut Vec<Diagnostic>) {
    let mut seen = BTreeMap::new();
    for (i, p) in spec.plugins.iter().enumerate() {
        let span = map.value(&pointer(["plugins", &i.to_string(), "name"]));
        if let Some(first) = seen.insert(p.name.as_str(), span) {
            out.push(
                Diagnostic::new(Code::E025, format!("plugin `{}` is listed twice", p.name))
                    .at(span, "listed again here")
                    .also(first, "first listed here"),
            );
        }
    }
}

fn unused(spec: &Spec, map: &SourceMap, out: &mut Vec<Diagnostic>) {
    let used: BTreeSet<&str> = spec.services.values().flat_map(|s| s.uses.iter().map(String::as_str)).collect();
    for (section, names) in
        [("queues", spec.queues.keys().collect::<Vec<_>>()), ("databases", spec.databases.keys().collect())]
    {
        for name in names {
            if !used.contains(name.as_str()) {
                out.push(
                    Diagnostic::new(Code::W001, format!("`{name}` is not used by any service"))
                        .at(map.key(&pointer([section, name])), "unused")
                        .help(format!("add `{name}` to a service's `uses`, or remove it")),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_rules() {
        assert_eq!(dns_problem("orders-queue"), None);
        assert!(dns_problem("Orders").is_some());
        assert!(dns_problem("1db").is_some());
        assert!(dns_problem("a--b").is_some());
        assert!(dns_problem("a-").is_some());
        assert!(dns_problem(&"a".repeat(33)).is_some());
        assert_eq!(dns_safe("My_App 2"), "my-app-2");
        assert_eq!(dns_safe("__"), "app");
    }

    #[test]
    fn regions() {
        for ok in ["ap-south-1", "us-east-1", "us-gov-west-1", "eu-central-2"] {
            assert!(is_region(ok), "{ok}");
        }
        for bad in ["mumbai", "ap-south", "AP-SOUTH-1", "ap-south-x", "a-b-1"] {
            assert!(!is_region(bad), "{bad}");
        }
    }
}
