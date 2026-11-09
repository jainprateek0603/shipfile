//! The `shipfile` command-line tool.

mod cli;
mod doctor;
mod init;
mod spec_cmds;

use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;

use clap::Parser;
use miette::{IntoDiagnostic, MietteHandlerOpts};

use crate::cli::{Cli, Command};
use crate::spec_cmds::Ui;

/// Process exit codes; see `docs/PLAN.md` §4.2.
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub(crate) enum Exit {
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
    let cli = Cli::parse();
    let no_color = cli.global.no_color || std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    if no_color {
        owo_colors::set_override(false);
        let _ = miette::set_hook(Box::new(|_| Box::new(MietteHandlerOpts::new().color(false).build())));
    }
    let ui = Ui {
        json: cli.global.json,
        quiet: cli.global.quiet,
        verbose: cli.global.verbose,
        color: !no_color && io::stderr().is_terminal(),
    };
    match run(&cli, ui) {
        Ok(exit) => exit.into(),
        Err(report) => {
            let _ = writeln!(io::stderr(), "{report:?}");
            Exit::Error.into()
        }
    }
}

fn run(cli: &Cli, ui: Ui) -> miette::Result<Exit> {
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    match &cli.command {
        Command::Init { yes } => init::run(&cli.global, *yes, ui, &mut stdout, &mut stderr),
        Command::Check => spec_cmds::check(&cli.global, ui, &mut stdout, &mut stderr),
        Command::Graph { format } => spec_cmds::graph(&cli.global, *format, ui, &mut stdout, &mut stderr),
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
