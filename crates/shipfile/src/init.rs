//! `shipfile init`: find service folders and write a starter spec.

use std::fmt::Write as _;
use std::io::IsTerminal;
use std::path::Path;

use miette::{IntoDiagnostic, miette};
use shipfile_core::project::{self, Discovered};
use shipfile_core::{DirProject, Name};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Answers {
    pub(crate) app: String,
    pub(crate) services: Vec<ServiceAnswer>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServiceAnswer {
    pub(crate) name: String,
    pub(crate) dir: String,
    pub(crate) runtime: String,
    pub(crate) manifest: String,
    pub(crate) port: Option<u16>,
    pub(crate) public: bool,
}

/// A valid name from free text such as a folder name, if one can be made.
fn to_name(text: &str) -> Option<String> {
    if Name::new(text).is_ok() { Some(text.to_owned()) } else { shipfile_core::name::suggest_fix(text) }
}

/// Answers without asking anything: every detected folder becomes a private service.
pub(crate) fn defaults(folder_name: &str, found: &[Discovered]) -> Answers {
    let app = to_name(folder_name).unwrap_or_else(|| "app".to_owned());
    let mut services: Vec<ServiceAnswer> = Vec::new();
    for d in found {
        let base = if d.dir == "." { app.clone() } else { d.dir.rsplit('/').next().unwrap_or(&d.dir).to_owned() };
        let Some(name) = to_name(&base) else { continue };
        if services.iter().any(|s| s.name == name) {
            continue;
        }
        services.push(ServiceAnswer {
            name,
            dir: d.dir.clone(),
            runtime: d.runtime.to_string(),
            manifest: d.manifest.clone(),
            port: None,
            public: false,
        });
    }
    services.sort_by(|a, b| a.name.cmp(&b.name));
    Answers { app, services }
}

/// The spec text for a set of answers. Deterministic, LF line endings.
pub(crate) fn render(answers: &Answers) -> String {
    let mut out = String::new();
    out.push_str("# yaml-language-server: $schema=https://shipfile.dev/schema/v1.json\n");
    out.push_str("# Written by `shipfile init`. Run `shipfile check` after editing.\n");
    let _ = writeln!(out, "version: 1\nname: {}\n", answers.app);
    if answers.services.is_empty() {
        out.push_str("# No service folders found (package.json, pyproject.toml, go.mod or Cargo.toml).\n");
        out.push_str("services: {}\n");
        return out;
    }
    out.push_str("services:\n");
    for (i, svc) in answers.services.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let build = if svc.dir == "." { ".".to_owned() } else { format!("./{}", svc.dir) };
        let _ = writeln!(out, "  {}:", svc.name);
        let _ = writeln!(out, "    build: {build}");
        let _ = writeln!(out, "    runtime: {}  # detected from {}", svc.runtime, svc.manifest);
        if let Some(port) = svc.port {
            let _ = writeln!(out, "    port: {port}");
        }
        if svc.public {
            out.push_str("    public: true\n    health: /healthz\n");
        }
    }
    out
}

fn ask(mut answers: Answers) -> miette::Result<Answers> {
    let validate = |text: &str| -> Result<inquire::validator::Validation, inquire::CustomUserError> {
        Ok(match Name::new(text) {
            Ok(_) => inquire::validator::Validation::Valid,
            Err(e) => inquire::validator::Validation::Invalid(format!("the name {e}").into()),
        })
    };
    answers.app = inquire::Text::new("App name:")
        .with_default(&answers.app)
        .with_validator(validate)
        .prompt()
        .into_diagnostic()?;
    let mut kept = Vec::new();
    for mut svc in answers.services {
        let include =
            inquire::Confirm::new(&format!("Add `{}` ({}, {}) as a service?", svc.dir, svc.runtime, svc.name))
                .with_default(true)
                .prompt()
                .into_diagnostic()?;
        if !include {
            continue;
        }
        let port = inquire::Text::new(&format!("Port `{}` listens on (empty for a worker):", svc.name))
            .with_validator(|t: &str| {
                Ok(if t.trim().is_empty() || t.trim().parse::<u16>().is_ok_and(|p| p > 0) {
                    inquire::validator::Validation::Valid
                } else {
                    inquire::validator::Validation::Invalid("enter a port from 1 to 65535".into())
                })
            })
            .prompt()
            .into_diagnostic()?;
        svc.port = port.trim().parse().ok();
        if svc.port.is_some() {
            svc.public = inquire::Confirm::new(&format!("Put a load balancer in front of `{}`?", svc.name))
                .with_default(false)
                .prompt()
                .into_diagnostic()?;
        }
        kept.push(svc);
    }
    answers.services = kept;
    Ok(answers)
}

/// Writes the spec at `file`. Returns the path written, for the summary line.
pub(crate) fn run(file: &Path, yes: bool) -> miette::Result<String> {
    if file.exists() {
        return Err(miette!(
            help = "edit it, or delete it first to start over",
            "{} already exists; `shipfile init` never overwrites a spec",
            file.display()
        ));
    }
    let root = match file.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => std::env::current_dir().into_diagnostic()?,
    };
    let folder_name = std::fs::canonicalize(&root)
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "app".to_owned());
    let found = project::discover(&DirProject::new(&root));
    let mut answers = defaults(&folder_name, &found);
    if !yes && std::io::stdin().is_terminal() {
        answers = ask(answers)?;
    }
    let text = render(&answers);
    if let Err(failed) = shipfile_core::load(&text, None, &DirProject::new(&root)) {
        return Err(miette!("the generated spec doesn't pass `shipfile check` (a bug): {}", failed.diagnostics));
    }
    std::fs::write(file, &text).into_diagnostic()?;
    Ok(format!(
        "wrote {} with {} service{}",
        file.display(),
        answers.services.len(),
        if answers.services.len() == 1 { "" } else { "s" }
    ))
}

#[cfg(test)]
mod tests {
    use shipfile_core::MemProject;

    use super::*;

    #[test]
    fn defaults_name_services_after_folders() {
        let project = MemProject::new().with_file("api/package.json", "{}").with_file("Worker_1/go.mod", "go 1.23\n");
        let answers = defaults("My Shop", &project::discover(&project));
        assert_eq!(answers.app, "my-shop");
        let names: Vec<_> = answers.services.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["api", "worker-1"]);
    }

    #[test]
    fn rendered_spec_passes_check() {
        let project = MemProject::new().with_file("api/package.json", "{}");
        let mut answers = defaults("shop", &project::discover(&project));
        answers.services[0].port = Some(3000);
        answers.services[0].public = true;
        let text = render(&answers);
        assert!(text.contains("    port: 3000\n    public: true\n    health: /healthz\n"), "{text}");
        shipfile_core::load(&text, None, &project).unwrap();
    }

    #[test]
    fn empty_project_still_gives_a_valid_spec() {
        let text = render(&defaults("x", &[]));
        shipfile_core::load(&text, None, &MemProject::new()).unwrap();
    }
}
