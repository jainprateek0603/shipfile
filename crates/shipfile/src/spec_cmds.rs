//! `shipfile check` and `shipfile graph`.

use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};

use miette::{IntoDiagnostic, miette};
use owo_colors::{OwoColorize, Stream};
use serde::Serialize;
use shipfile_core::diagnostics::{Diagnostic, JsonDiagnostic, render};
use shipfile_core::model::detect_runtime;
use shipfile_core::{Model, graph};

use crate::Exit;
use crate::cli::{GlobalArgs, GraphFormat};

/// Output settings shared by the commands.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Ui {
    pub(crate) json: bool,
    pub(crate) quiet: bool,
    pub(crate) verbose: u8,
    /// Colour for diagnostics on stderr.
    pub(crate) color: bool,
}

/// The spec file as given, with `/` separators.
pub(crate) fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Reads the spec, normalising line endings so positions are the same on every OS.
fn read_spec(path: &Path) -> miette::Result<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text.trim_start_matches('\u{feff}').replace("\r\n", "\n")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(miette!(
            help = "run `shipfile init` to create one, or pass `-f <path>`",
            "no spec found at {}",
            display_path(path)
        )),
        Err(e) => Err(miette!("cannot read {}: {e}", display_path(path))),
    }
}

/// Resolves `build` paths against the folder that holds the spec.
fn spec_dir(path: &Path) -> PathBuf {
    path.parent().filter(|p| !p.as_os_str().is_empty()).map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 { format!("1 {word}") } else { format!("{n} {word}s") }
}

#[derive(Serialize)]
struct CheckJson<'a> {
    file: &'a str,
    env: Option<&'a str>,
    ok: bool,
    errors: usize,
    warnings: usize,
    diagnostics: Vec<JsonDiagnostic<'a>>,
}

pub(crate) fn check(global: &GlobalArgs, ui: Ui, out: &mut dyn Write, err: &mut dyn Write) -> miette::Result<Exit> {
    let file = display_path(&global.file);
    let source = read_spec(&global.file)?;
    let env = global.env.as_deref();
    let checked = shipfile_core::check(&source, env);
    let errors = checked.diagnostics.iter().filter(|d| d.is_error()).count();
    let warnings = checked.diagnostics.len() - errors;
    let ok = errors == 0;

    if ui.json {
        let report = CheckJson {
            file: &file,
            env,
            ok,
            errors,
            warnings,
            diagnostics: checked.diagnostics.iter().map(|d| d.to_json(&file)).collect(),
        };
        serde_json::to_writer_pretty(&mut *out, &report).into_diagnostic()?;
        writeln!(out).into_diagnostic()?;
        return Ok(if ok { Exit::Ok } else { Exit::Error });
    }

    print_diagnostics(&file, &source, &checked.diagnostics, ui, err)?;
    if !ok {
        writeln!(err, "{} {file}: {}, {}", cross(), plural(errors, "error"), plural(warnings, "warning"))
            .into_diagnostic()?;
        return Ok(Exit::Error);
    }
    if !ui.quiet
        && let Some(spec) = &checked.spec
    {
        let mut summary = format!(
            "{} {file} is valid: {}, {}, {}",
            tick(),
            plural(spec.services.len(), "service"),
            plural(spec.queues.len(), "queue"),
            plural(spec.databases.len(), "database"),
        );
        if let Some(env) = env {
            let _ = write!(summary, " (env `{env}`)");
        }
        if warnings > 0 {
            let _ = write!(summary, ", {}", plural(warnings, "warning"));
        }
        writeln!(out, "{summary}").into_diagnostic()?;
        if ui.verbose > 0 {
            let dir = spec_dir(&global.file);
            let model = Model::resolve(spec, env, &|build| detect_runtime(&dir.join(build)));
            for name in model.services.keys() {
                if let Some(contract) = model.contract(name) {
                    let vars: Vec<&str> = contract.vars.iter().map(|v| v.name.as_str()).collect();
                    writeln!(out, "  {name}: {}", vars.join(" ")).into_diagnostic()?;
                }
            }
        }
    }
    Ok(Exit::Ok)
}

fn print_diagnostics(
    file: &str,
    source: &str,
    diagnostics: &[Diagnostic],
    ui: Ui,
    err: &mut dyn Write,
) -> miette::Result<()> {
    let shown: Vec<Diagnostic> = diagnostics.iter().filter(|d| d.is_error() || !ui.quiet).cloned().collect();
    if !shown.is_empty() {
        write!(err, "{}", render(file, source, &shown, ui.color)).into_diagnostic()?;
    }
    Ok(())
}

fn tick() -> String {
    "✓".if_supports_color(Stream::Stdout, |t| t.green()).to_string()
}

fn cross() -> String {
    "✗".if_supports_color(Stream::Stderr, |t| t.red()).to_string()
}

pub(crate) fn graph(
    global: &GlobalArgs,
    format: GraphFormat,
    ui: Ui,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> miette::Result<Exit> {
    let file = display_path(&global.file);
    let source = read_spec(&global.file)?;
    let dir = spec_dir(&global.file);
    let detect = |build: &str| detect_runtime(&dir.join(build));
    let (model, warnings) = match shipfile_core::load(&source, global.env.as_deref(), &detect) {
        Ok(loaded) => loaded,
        Err(diagnostics) => {
            print_diagnostics(&file, &source, &diagnostics, ui, err)?;
            let errors = diagnostics.iter().filter(|d| d.is_error()).count();
            writeln!(err, "{} {file}: {}; run `shipfile check` for details", cross(), plural(errors, "error"))
                .into_diagnostic()?;
            return Ok(Exit::Error);
        }
    };
    let format = if ui.json { GraphFormat::Json } else { format };
    match format {
        GraphFormat::Tree => {
            print_diagnostics(&file, &source, &warnings, ui, err)?;
            write!(out, "{}", graph::tree(&model)).into_diagnostic()?;
        }
        GraphFormat::Dot => write!(out, "{}", graph::dot(&model)).into_diagnostic()?,
        GraphFormat::Json => {
            serde_json::to_writer_pretty(&mut *out, &model).into_diagnostic()?;
            writeln!(out).into_diagnostic()?;
        }
    }
    Ok(Exit::Ok)
}
