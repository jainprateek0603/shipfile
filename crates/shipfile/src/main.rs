//! The `shipfile` command-line tool.

mod cli;
mod doctor;

use std::io::{self, Write};
use std::process::ExitCode;

use clap::Parser as _;
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

fn run(cli: &Cli) -> miette::Result<Exit> {
    let mut stdout = io::stdout().lock();
    match cli.command {
        Command::Doctor => {
            let report = doctor::run();
            if cli.global.json {
                serde_json::to_writer_pretty(&mut stdout, &report).into_diagnostic()?;
                writeln!(stdout).into_diagnostic()?;
            } else {
                doctor::render(&report, &mut stdout).into_diagnostic()?;
            }
            Ok(if report.ok { Exit::Ok } else { Exit::Error })
        }
    }
}
