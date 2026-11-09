//! Text renderings of the resource graph for `shipfile graph`.

use std::fmt::Write as _;

use crate::model::{Database, Model};

/// An indented tree, for people.
pub fn tree(model: &Model) -> String {
    let mut out = String::new();
    let mut header = model.app.name.clone();
    if let Some(env) = &model.app.env {
        let _ = write!(header, " ({env})");
    }
    if let Some(region) = &model.app.region {
        let _ = write!(header, " · {region}");
    }
    out.push_str(&header);
    out.push('\n');

    let mut sections: Vec<(&str, Vec<(String, String, Vec<String>)>)> = Vec::new();
    sections.push((
        "services",
        model
            .services
            .iter()
            .map(|(name, s)| {
                let mut facts = vec![s.runtime.map_or("runtime unknown", |r| r.as_str()).to_owned()];
                if let Some(port) = s.port {
                    facts.push(format!(":{port}"));
                }
                facts.push(if s.public { "public".to_owned() } else { "private".to_owned() });
                facts.push(format!("{}×{}cpu/{}MiB", s.scale.replicas, s.scale.cpu, s.scale.memory));
                let mut children: Vec<String> =
                    model.uses_of(name).map(|u| format!("uses {}.{}", u.kind.section(), u.resource)).collect();
                children.extend(s.secrets.iter().map(|k| format!("secret {k}")));
                (name.clone(), facts.join(" · "), children)
            })
            .collect(),
    ));
    sections.push((
        "queues",
        model
            .queues
            .iter()
            .map(|(name, q)| (name.clone(), format!("sqs · dlq after {} receives", q.max_receives), vec![]))
            .collect(),
    ));
    sections.push((
        "databases",
        model
            .databases
            .iter()
            .map(|(name, db)| {
                let facts = match db {
                    Database::Postgres { size } => format!("postgres · {}", size.as_str()),
                    Database::Dynamodb { key } => match &key.sort {
                        Some(sort) => format!("dynamodb · key {}/{sort}", key.partition),
                        None => format!("dynamodb · key {}", key.partition),
                    },
                };
                (name.clone(), facts, vec![])
            })
            .collect(),
    ));
    sections.retain(|(_, items)| !items.is_empty());

    let last_section = sections.len().saturating_sub(1);
    for (si, (title, items)) in sections.iter().enumerate() {
        let (branch, indent) = if si == last_section { ("└─ ", "   ") } else { ("├─ ", "│  ") };
        let _ = writeln!(out, "{branch}{title}");
        for (ii, (name, facts, children)) in items.iter().enumerate() {
            let last_item = ii + 1 == items.len();
            let (b2, i2) = if last_item { ("└─ ", "   ") } else { ("├─ ", "│  ") };
            let _ = writeln!(out, "{indent}{b2}{name}  {facts}");
            for (ci, child) in children.iter().enumerate() {
                let b3 = if ci + 1 == children.len() { "└─ " } else { "├─ " };
                let _ = writeln!(out, "{indent}{i2}{b3}{child}");
            }
        }
    }
    out
}

/// Graphviz DOT.
pub fn dot(model: &Model) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "digraph \"{}\" {{", model.app.name);
    out.push_str("  rankdir=LR;\n  node [fontname=\"Helvetica\"];\n");
    for (name, s) in &model.services {
        let shape = if s.public { "doubleoctagon" } else { "box" };
        let _ = writeln!(out, "  \"services.{name}\" [label=\"{name}\", shape={shape}];");
    }
    for name in model.queues.keys() {
        let _ = writeln!(out, "  \"queues.{name}\" [label=\"{name}\\n(sqs)\", shape=cds];");
    }
    for (name, db) in &model.databases {
        let _ = writeln!(out, "  \"databases.{name}\" [label=\"{name}\\n({})\", shape=cylinder];", db.engine().as_str());
    }
    for u in &model.uses {
        let _ = writeln!(out, "  \"services.{}\" -> \"{}.{}\";", u.service, u.kind.section(), u.resource);
    }
    out.push_str("}\n");
    out
}
