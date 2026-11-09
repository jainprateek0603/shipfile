//! `shipfile init`: finds service folders, asks a few questions and writes a starter spec.
//!
//! Detection and rendering are pure functions of the folder listing and the answers, so they
//! are unit-tested; only [`run`] touches the terminal and the file system.

use std::fmt::Write as _;
use std::io::{IsTerminal, Write};
use std::path::Path;

use miette::{IntoDiagnostic, miette};
use owo_colors::{OwoColorize, Stream};
use serde::Serialize;
use shipfile_core::check::{dns_problem, dns_safe};
use shipfile_core::model::detect_runtime;
use shipfile_core::spec::Runtime;

use crate::Exit;
use crate::cli::GlobalArgs;
use crate::spec_cmds::{Ui, display_path};

/// Folders never treated as services.
const SKIP_DIRS: &[&str] = &["node_modules", "target", "vendor", "dist", "build", "venv", "__pycache__"];
/// Service names that are public by default.
const PUBLIC_NAMES: &[&str] = &["api", "web", "app", "frontend", "server", "site"];

/// A service in the starter spec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Draft {
    pub(crate) name: String,
    pub(crate) build: String,
    pub(crate) runtime: Option<Runtime>,
    /// The file the runtime was detected from.
    #[serde(skip)]
    pub(crate) marker: Option<&'static str>,
    pub(crate) public: bool,
    pub(crate) port: Option<u16>,
}

fn marker_of(dir: &Path) -> Option<(Runtime, &'static str)> {
    let runtime = detect_runtime(dir)?;
    let marker = ["package.json", "pyproject.toml", "requirements.txt", "go.mod", "Cargo.toml"]
        .into_iter()
        .find(|m| dir.join(m).is_file())?;
    Some((runtime, marker))
}

pub(crate) fn default_port(runtime: Option<Runtime>) -> u16 {
    match runtime {
        Some(Runtime::Node20 | Runtime::Node22 | Runtime::Node24) => 3000,
        Some(Runtime::Python312 | Runtime::Python313) => 8000,
        _ => 8080,
    }
}

/// Finds services: each immediate sub-folder with a marker file, or else the root itself.
pub(crate) fn detect(root: &Path, app: &str) -> Vec<Draft> {
    let mut dirs: Vec<(String, std::path::PathBuf)> = std::fs::read_dir(root)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
                .filter(|(name, _)| !name.starts_with('.') && !SKIP_DIRS.contains(&name.as_str()))
                .collect()
        })
        .unwrap_or_default();
    dirs.sort();
    let mut found: Vec<Draft> = Vec::new();
    for (folder, path) in dirs {
        if let Some((runtime, marker)) = marker_of(&path) {
            let mut name = dns_safe(&folder);
            let base = name.clone();
            let mut n = 2;
            while found.iter().any(|d| d.name == name) {
                name = format!("{base}-{n}");
                n += 1;
            }
            found.push(Draft {
                name,
                build: format!("./{folder}"),
                runtime: Some(runtime),
                marker: Some(marker),
                public: false,
                port: None,
            });
        }
    }
    if found.is_empty() {
        let (runtime, marker) = marker_of(root).unzip();
        found.push(Draft { name: app.to_owned(), build: ".".to_owned(), runtime, marker, public: false, port: None });
    }
    // One public service by default: a conventional name, or the only service.
    let public = found.iter().position(|d| PUBLIC_NAMES.contains(&d.name.as_str())).or(if found.len() == 1 {
        Some(0)
    } else {
        None
    });
    if let Some(i) = public {
        found[i].public = true;
        found[i].port = Some(default_port(found[i].runtime));
    }
    found
}

/// Renders the starter spec.
pub(crate) fn render(app: &str, region: &str, services: &[Draft]) -> String {
    let mut out = String::new();
    out.push_str("# yaml-language-server: $schema=https://shipfile.dev/schema/v1.json\n");
    out.push_str("# Starter spec written by `shipfile init`. Run `shipfile check` after editing.\n");
    let _ = writeln!(out, "version: 1\nname: {app}\naws:\n  region: {region}\n\nservices:");
    for s in services {
        let build = serde_json::to_string(&s.build).unwrap_or_else(|_| "\".\"".to_owned());
        let _ = writeln!(out, "  {}:\n    build: {build}", s.name);
        match (s.runtime, s.marker) {
            (Some(r), Some(m)) => {
                let _ = writeln!(out, "    runtime: {}  # detected from {m}", r.as_str());
            }
            (Some(r), None) => {
                let _ = writeln!(out, "    runtime: {}", r.as_str());
            }
            (None, _) => {
                out.push_str("    # runtime: node22  # not detected; set it if the build folder has no marker file\n");
            }
        }
        if let Some(port) = s.port {
            let _ = writeln!(out, "    port: {port}");
        }
        if s.public {
            out.push_str("    public: true\n    health: /healthz\n");
        }
    }
    out.push_str(
        "\n# Add queues and databases, then list them under a service's `uses`:\n\
         #\n\
         # queues:\n\
         #   jobs:\n\
         #     dlq: { max_receives: 5 }\n\
         #\n\
         # databases:\n\
         #   main-db:\n\
         #     engine: postgres\n\
         #     size: small\n\
         #\n\
         # services:\n\
         #   api:\n\
         #     uses: [jobs, main-db]\n\
         \n\
         environments:\n  prod: {}\n",
    );
    out
}

fn default_region() -> String {
    ["AWS_REGION", "AWS_DEFAULT_REGION"]
        .into_iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.trim().is_empty()))
        .unwrap_or_else(|| "us-east-1".to_owned())
}

#[derive(Serialize)]
struct InitJson<'a> {
    file: &'a str,
    name: &'a str,
    region: &'a str,
    services: &'a [Draft],
}

pub(crate) fn run(
    global: &GlobalArgs,
    yes: bool,
    ui: Ui,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> miette::Result<Exit> {
    let path = &global.file;
    let file = display_path(path);
    if path.exists() {
        return Err(miette!(
            help = "edit it, delete it first, or write somewhere else with `-f <path>`",
            "{file} already exists"
        ));
    }
    let root = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let root_name = std::fs::canonicalize(root)
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let mut app = dns_safe(&root_name);
    let mut region = default_region();
    let mut services = detect(root, &app);

    let interactive = !yes && !ui.json && std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    if !yes && !interactive && !ui.quiet && !ui.json {
        writeln!(err, "not a terminal; using the defaults (pass --yes to skip this note)").into_diagnostic()?;
    }
    if interactive {
        let names: Vec<&str> = services.iter().map(|s| s.name.as_str()).collect();
        writeln!(err, "Found {} service(s): {}", services.len(), names.join(", ")).into_diagnostic()?;
        app = inquire::Text::new("App name:")
            .with_default(&app)
            .with_validator(|v: &str| {
                Ok(match dns_problem(v) {
                    Some(p) => inquire::validator::Validation::Invalid(p.into()),
                    None => inquire::validator::Validation::Valid,
                })
            })
            .prompt()
            .into_diagnostic()?;
        region = inquire::Text::new("AWS region:").with_default(&region).prompt().into_diagnostic()?;
        for s in &mut services {
            s.public = inquire::Confirm::new(&format!("Put `{}` behind a public load balancer?", s.name))
                .with_default(s.public)
                .prompt()
                .into_diagnostic()?;
            if s.public {
                let default = s.port.unwrap_or_else(|| default_port(s.runtime)).to_string();
                let port = inquire::Text::new(&format!("Port `{}` listens on:", s.name))
                    .with_default(&default)
                    .with_validator(|v: &str| {
                        Ok(match v.parse::<u16>() {
                            Ok(p) if p > 0 => inquire::validator::Validation::Valid,
                            _ => inquire::validator::Validation::Invalid("enter a port between 1 and 65535".into()),
                        })
                    })
                    .prompt()
                    .into_diagnostic()?;
                s.port = port.parse().ok();
            } else {
                s.port = None;
            }
        }
        if services.len() == 1 && services[0].build == "." && services[0].name != app {
            services[0].name.clone_from(&app);
        }
    }

    let text = render(&app, &region, &services);
    let checked = shipfile_core::check(&text, None);
    if checked.has_errors() {
        let detail = shipfile_core::diagnostics::render(&file, &text, &checked.diagnostics, false);
        return Err(miette!("the starter spec did not pass `check`; this is a bug:\n{detail}"));
    }
    std::fs::write(path, &text).into_diagnostic()?;

    if ui.json {
        let report = InitJson { file: &file, name: &app, region: &region, services: &services };
        serde_json::to_writer_pretty(&mut *out, &report).into_diagnostic()?;
        writeln!(out).into_diagnostic()?;
    } else if !ui.quiet {
        let names: Vec<&str> = services.iter().map(|s| s.name.as_str()).collect();
        let tick = "✓".if_supports_color(Stream::Stdout, |t| t.green()).to_string();
        writeln!(out, "{tick} wrote {file} with {} service(s): {}", services.len(), names.join(", "))
            .into_diagnostic()?;
        writeln!(out, "  next: `shipfile check`, then `shipfile graph`").into_diagnostic()?;
    }
    Ok(Exit::Ok)
}

#[cfg(test)]
mod tests {
    use assert_fs::TempDir;
    use assert_fs::prelude::*;

    use super::*;

    #[test]
    fn detects_sub_folders() {
        let dir = TempDir::new().unwrap();
        dir.child("api/package.json").write_str("{}").unwrap();
        dir.child("worker/pyproject.toml").write_str("").unwrap();
        dir.child("node_modules/x/package.json").write_str("{}").unwrap();
        dir.child("docs/readme.md").write_str("").unwrap();
        let found = detect(dir.path(), "shop");
        let names: Vec<_> = found.iter().map(|d| (d.name.as_str(), d.build.as_str(), d.public, d.port)).collect();
        assert_eq!(names, [("api", "./api", true, Some(3000)), ("worker", "./worker", false, None)]);
        assert_eq!(found[1].runtime, Some(Runtime::Python313));
    }

    #[test]
    fn falls_back_to_the_root() {
        let dir = TempDir::new().unwrap();
        dir.child("go.mod").write_str("module x").unwrap();
        let found = detect(dir.path(), "shop");
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].name.as_str(), found[0].build.as_str()), ("shop", "."));
        assert_eq!((found[0].runtime, found[0].port), (Some(Runtime::Go125), Some(8080)));
    }

    #[test]
    fn starter_specs_pass_check() {
        let dir = TempDir::new().unwrap();
        for case in [vec![], vec!["api/package.json", "Worker Svc/Cargo.toml"], vec!["a/go.mod", "b/go.mod"]] {
            let sub = dir.child(format!("case{}", case.len()));
            for f in case {
                sub.child(f).write_str("").unwrap();
            }
            sub.create_dir_all().unwrap();
            let text = render("shop", "ap-south-1", &detect(sub.path(), "shop"));
            let checked = shipfile_core::check(&text, None);
            assert!(checked.diagnostics.is_empty(), "{text}\n{:?}", checked.diagnostics);
        }
    }
}
