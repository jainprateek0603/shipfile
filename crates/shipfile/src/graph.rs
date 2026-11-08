//! `shipfile graph`: the resource graph as a tree, Graphviz dot, or JSON.

use std::fmt::Write as _;

use shipfile_core::Model;
use shipfile_core::model::{Database, Service};

/// One line in the tree plus the lines nested under it.
type Item = (String, Vec<String>);

pub(crate) fn tree(model: &Model) -> String {
    let mut out = String::new();
    let mut header = model.app.name.to_string();
    let mut facts = Vec::new();
    if let Some(env) = &model.app.env {
        facts.push(format!("env {env}"));
    }
    if let Some(region) = &model.app.region {
        facts.push(region.clone());
    }
    if !facts.is_empty() {
        let _ = write!(header, "  ({})", facts.join(" · "));
    }
    out.push_str(&header);
    out.push('\n');

    let mut sections: Vec<(&str, Vec<Item>)> = Vec::new();
    sections.push((
        "services",
        model
            .services
            .iter()
            .map(|(name, svc)| {
                let edges = model.uses_of(name).map(|u| format!("uses {} {}", u.kind.as_str(), u.resource)).collect();
                (format!("{name}  {}", describe_service(svc)), edges)
            })
            .collect(),
    ));
    sections.push((
        "queues",
        model
            .queues
            .iter()
            .map(|(name, q)| (format!("{name}  dead-letter queue after {} receives", q.dlq.max_receives), Vec::new()))
            .collect(),
    ));
    sections.push((
        "databases",
        model
            .databases
            .iter()
            .map(|(name, db)| {
                let what = match db {
                    Database::Postgres { size } => format!("postgres · {}", size.as_str()),
                    Database::Dynamodb { key } => match &key.sort {
                        Some(sort) => format!("dynamodb · key {} + {sort}", key.partition),
                        None => format!("dynamodb · key {}", key.partition),
                    },
                };
                (format!("{name}  {what}"), Vec::new())
            })
            .collect(),
    ));
    sections.retain(|(_, items)| !items.is_empty());

    for (i, (title, items)) in sections.iter().enumerate() {
        let last_section = i + 1 == sections.len();
        let (branch, indent) = if last_section { ("└─ ", "   ") } else { ("├─ ", "│  ") };
        let _ = writeln!(out, "{branch}{title}");
        for (j, (line, edges)) in items.iter().enumerate() {
            let last_item = j + 1 == items.len();
            let (branch, inner) = if last_item { ("└─ ", "   ") } else { ("├─ ", "│  ") };
            let _ = writeln!(out, "{indent}{branch}{line}");
            for (k, edge) in edges.iter().enumerate() {
                let branch = if k + 1 == edges.len() { "└─ " } else { "├─ " };
                let _ = writeln!(out, "{indent}{inner}{branch}{edge}");
            }
        }
    }
    out
}

fn describe_service(svc: &Service) -> String {
    let mut parts = Vec::new();
    parts.push(svc.runtime.as_ref().map_or_else(|| "Dockerfile".to_owned(), ToString::to_string));
    if let Some(port) = svc.port {
        parts.push(format!(":{port}"));
    }
    if svc.public {
        parts.push(format!("public {}", svc.health.as_deref().unwrap_or("")).trim_end().to_owned());
    }
    let s = svc.scale;
    parts.push(format!("{} cpu / {} MiB × {}", s.cpu, s.memory, s.replicas));
    parts.join(" · ")
}

pub(crate) fn dot(model: &Model) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "digraph \"{}\" {{", model.app.name);
    out.push_str("  rankdir=LR;\n  node [fontname=\"Helvetica\"];\n");
    for (name, svc) in &model.services {
        let shape = if svc.public { "doubleoctagon" } else { "box" };
        let _ = writeln!(out, "  \"services.{name}\" [label=\"{name}\", shape={shape}];");
    }
    for name in model.queues.keys() {
        let _ = writeln!(out, "  \"queues.{name}\" [label=\"{name}\", shape=cds];");
    }
    for (name, db) in &model.databases {
        let engine = match db {
            Database::Postgres { .. } => "postgres",
            Database::Dynamodb { .. } => "dynamodb",
        };
        let _ = writeln!(out, "  \"databases.{name}\" [label=\"{name}\\n{engine}\", shape=cylinder];");
    }
    for edge in &model.uses {
        let section = if model.queues.contains_key(&edge.resource) { "queues" } else { "databases" };
        let _ = writeln!(out, "  \"services.{}\" -> \"{section}.{}\";", edge.service, edge.resource);
    }
    out.push_str("}\n");
    out
}
