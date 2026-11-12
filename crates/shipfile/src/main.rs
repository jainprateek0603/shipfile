//! The `shipfile` command-line tool.

mod cli;
mod doctor;
mod panic;

use std::io::{self, Write};
use std::process::ExitCode;

use clap::{CommandFactory as _, Parser as _};
use miette::{IntoDiagnostic, MietteHandlerOpts};

use crate::cli::{Cli, Command};

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
    init_logging(cli.global.verbose, cli.global.quiet, cli.global.no_color);
    if cli.global.no_color {
        owo_colors::set_override(false);
        let _ = miette::set_hook(Box::new(|_| Box::new(MietteHandlerOpts::new().color(false).build())));
    }
    match run(&cli) {
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
    use std::io::IsTerminal as _;
    use tracing_subscriber::EnvFilter;
    let level = match (quiet, verbose) {
        (true, _) => "error",
        (false, 0) => "warn",
        (false, 1) => "debug",
        (false, _) => "trace",
    };
    let filter =
        EnvFilter::try_from_env("SHIPFILE_LOG").unwrap_or_else(|_| EnvFilter::new(format!("shipfile={level}")));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(io::stderr)
        .with_ansi(!no_color && std::env::var_os("NO_COLOR").is_none() && io::stderr().is_terminal())
        .without_time()
        .try_init();
}

fn run(cli: &Cli) -> miette::Result<Exit> {
    let mut stdout = io::stdout().lock();
    match &cli.command {
        Command::Completions { shell } => {
            clap_complete::generate(*shell, &mut Cli::command(), "shipfile", &mut stdout);
            Ok(Exit::Ok)
        }
        Command::Doctor => {
            let report = doctor::run();
            if cli.global.json {
                serde_json::to_writer_pretty(&mut stdout, &report).into_diagnostic()?;
                writeln!(stdout).into_diagnostic()?;
            } else {
                doctor::render(&report, doctor::Marks::detect(cli.global.ascii), &mut stdout).into_diagnostic()?;
            }
            Ok(if report.ok { Exit::Ok } else { Exit::Error })
        }
    }
}
