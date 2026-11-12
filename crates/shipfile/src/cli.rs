use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// One file in, a whole service out.
///
/// Shipfile reads one `shipfile.yaml`, runs your stack locally with Docker,
/// then deploys the same stack to AWS with a plan you review first.
#[derive(Debug, Parser)]
#[command(name = "shipfile", version, about, long_about)]
pub(crate) struct Cli {
    #[command(flatten)]
    pub(crate) global: GlobalArgs,

    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Args)]
#[expect(clippy::struct_excessive_bools, reason = "independent CLI flags")]
pub(crate) struct GlobalArgs {
    /// Path to the spec file.
    #[arg(short = 'f', long = "file", global = true, default_value = "shipfile.yaml")]
    #[allow(dead_code, reason = "read by spec commands from phase 1; `expect` breaks on the MSRV, which does not flag it")]
    pub(crate) file: PathBuf,

    /// Environment overlay to apply (for example `dev` or `prod`).
    #[arg(short = 'e', long = "env", global = true, env = "SHIPFILE_ENV")]
    #[allow(dead_code, reason = "read by spec commands from phase 1; `expect` breaks on the MSRV, which does not flag it")]
    pub(crate) env: Option<String>,

    /// Print machine-readable JSON on stdout.
    #[arg(long, global = true)]
    pub(crate) json: bool,

    /// Disable coloured output (also honours `NO_COLOR`).
    #[arg(long, global = true)]
    pub(crate) no_color: bool,

    /// Use ASCII status marks instead of Unicode symbols.
    #[arg(long, global = true)]
    pub(crate) ascii: bool,

    /// More output; repeat for more detail.
    #[arg(short, long, global = true, action = clap::ArgAction::Count, conflicts_with = "quiet")]
    pub(crate) verbose: u8,

    /// Only print results and errors.
    #[arg(short, long, global = true)]
    pub(crate) quiet: bool,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Check Docker, Compose and AWS setup, and explain how to fix anything missing.
    Doctor,
    /// Print a shell completion script, e.g. `shipfile completions bash > /etc/bash_completion.d/shipfile`.
    Completions {
        /// Shell to generate for.
        shell: clap_complete::Shell,
    },
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::Cli;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
