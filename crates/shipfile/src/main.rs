//! The `shipfile` command-line tool.

mod cli;
mod doctor;
mod graph;
mod init;
mod panic;
mod report;

use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::process::ExitCode;

use clap::{CommandFactory as _, Parser as _};
use miette::{IntoDiagnostic, miette};
use serde::Serialize;
use shipfile_core::{DirProject, Loaded};

use crate::cli::{Cli, Command, GlobalArgs, GraphFormat};
use crate::report::{Handler, JsonDiagnostic};

/// Process exit codes; see `docs/PLAN.md` §4.2.
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
enum Exit {
    Ok = 0,
    Error = 1,
    #[expect(dead_code, reason = "returned by `plan --detailed-exitcode` from phase 5")]
    PlanHasChanges = 2,
    #[expect(dead_code, reason = "returned by the safety guard from phase 5")]
    Blocked = 3,
}

impl From<Exit> for ExitCode {
    fn from(exit: Exit) -> Self {
        ExitCode::from(exit as u8)
    }
}

fn main() -> ExitCode {
    // Not `Cli::parse()`: clap exits with 2 on usage errors, which would read as
    // "plan has changes" to `plan --detailed-exitcode` callers.
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            let _ = e.print();
            // Help and version go to stdout and are not errors.
            return if e.use_stderr() { Exit::Error } else { Exit::Ok }.into();
        }
    };
    panic::install();
    let no_color = cli.global.no_color || std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    init_logging(cli.global.verbose, cli.global.quiet, no_color);
    if no_color {
        owo_colors::set_override(false);
    }
    let handler = Handler { color: !no_color && io::stderr().is_terminal() };
    let _ = miette::set_hook(Box::new(move |_| Box::new(handler)));
    match run(&cli, handler) {
        Ok(exit) => exit.into(),
        Err(report) => {
            let _ = writeln!(io::stderr(), "{report:?}");
            Exit::Error.into()
        }
    }
}

/// Logs go to stderr. `-v` debug, `-vv` trace, `-q` errors only; `SHIPFILE_LOG` overrides.
/// Never log secret values: types holding them redact themselves in `Debug`.
fn init_logging(verbose: u8, quiet: bool, no_color: bool) {
    use tracing_subscriber::EnvFilter;
    let level = match (quiet, verbose) {
        (true, _) => "error",
        (false, 0) => "warn",
        (false, 1) => "debug",
        (false, _) => "trace",
    };
    let filter = EnvFilter::try_from_env("SHIPFILE_LOG")
        .unwrap_or_else(|_| EnvFilter::new(format!("shipfile={level},shipfile_core={level}")));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(io::stderr)
        .with_ansi(!no_color && io::stderr().is_terminal())
        .without_time()
        .try_init();
}

fn run(cli: &Cli, handler: Handler) -> miette::Result<Exit> {
    let mut stdout = io::stdout().lock();
    match &cli.command {
        Command::Completions { shell } => {
            clap_complete::generate(*shell, &mut Cli::command(), "shipfile", &mut stdout);
            Ok(Exit::Ok)
        }
        Command::Doctor => {
            let report = doctor::run();
            if cli.global.json {
                write_json(&mut stdout, &report)?;
            } else {
                doctor::render(&report, doctor::Marks::detect(cli.global.ascii), &mut stdout).into_diagnostic()?;
            }
            Ok(if report.ok { Exit::Ok } else { Exit::Error })
        }
        Command::Init { yes } => {
            let summary = init::run(&cli.global.file, *yes)?;
            writeln!(stdout, "{summary}. Next: `shipfile check`.").into_diagnostic()?;
            Ok(Exit::Ok)
        }
        Command::Check => check(&cli.global, handler, &mut stdout),
        Command::Graph { format } => {
            let format = if cli.global.json { GraphFormat::Json } else { *format };
            let loaded = match load(&cli.global)? {
                Ok(loaded) => loaded,
                Err(failed) => return report_failure(&cli.global, &failed, handler, &mut stdout),
            };
            match format {
                GraphFormat::Tree => stdout.write_all(graph::tree(&loaded.model).as_bytes()).into_diagnostic()?,
                GraphFormat::Dot => stdout.write_all(graph::dot(&loaded.model).as_bytes()).into_diagnostic()?,
                GraphFormat::Json => write_json(&mut stdout, &loaded.model)?,
            }
            Ok(Exit::Ok)
        }
    }
}

fn write_json(out: &mut impl Write, value: &impl Serialize) -> miette::Result<()> {
    serde_json::to_writer_pretty(&mut *out, value).into_diagnostic()?;
    writeln!(out).into_diagnostic()
}

/// The spec path as shown to users: always with `/` separators.
fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Reads and checks the spec. The outer error is for I/O problems; the inner one for spec errors.
fn load(global: &GlobalArgs) -> miette::Result<Result<Loaded, Box<shipfile_core::Failed>>> {
    let file = &global.file;
    let source = match std::fs::read_to_string(file) {
        Ok(source) => source,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(miette!(
                help = "run `shipfile init` to write one, or pass `-f <path>`",
                "no spec found at {}",
                display_path(file)
            ));
        }
        Err(e) => return Err(miette!("can't read {}: {e}", display_path(file))),
    };
    let root = file.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    Ok(shipfile_core::load(&source, global.env.as_deref(), &DirProject::new(root)))
}

#[derive(Serialize)]
struct CheckReport<'a> {
    ok: bool,
    file: String,
    env: Option<&'a str>,
    diagnostics: Vec<JsonDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resources: Option<Counts>,
}

#[derive(Serialize)]
struct Counts {
    services: usize,
    queues: usize,
    databases: usize,
}

fn report_failure(
    global: &GlobalArgs,
    failed: &shipfile_core::Failed,
    handler: Handler,
    stdout: &mut impl Write,
) -> miette::Result<Exit> {
    let file = display_path(&global.file);
    if global.json {
        let diagnostics =
            failed.diagnostics.items.iter().map(|d| JsonDiagnostic::new(d, &file, &failed.text)).collect();
        write_json(stdout, &CheckReport { ok: false, file, env: global.env.as_deref(), diagnostics, resources: None })?;
    } else {
        io::stderr().lock().write_all(report::render_failed(failed, &file, handler).as_bytes()).into_diagnostic()?;
    }
    Ok(Exit::Error)
}

fn check(global: &GlobalArgs, handler: Handler, stdout: &mut impl Write) -> miette::Result<Exit> {
    let loaded = match load(global)? {
        Ok(loaded) => loaded,
        Err(failed) => return report_failure(global, &failed, handler, stdout),
    };
    let model = &loaded.model;
    let counts =
        Counts { services: model.services.len(), queues: model.queues.len(), databases: model.databases.len() };
    let file = display_path(&global.file);
    if global.json {
        let report = CheckReport {
            ok: true,
            file,
            env: global.env.as_deref(),
            diagnostics: Vec::new(),
            resources: Some(counts),
        };
        write_json(stdout, &report)?;
    } else {
        let plural = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
        let env = global.env.as_deref().map(|e| format!(" (environment `{e}`)")).unwrap_or_default();
        writeln!(
            stdout,
            "✓ {file} is valid{env}: {}, {}, {}",
            plural(counts.services, "service"),
            plural(counts.queues, "queue"),
            plural(counts.databases, "database"),
        )
        .into_diagnostic()?;
    }
    Ok(Exit::Ok)
}
