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
pub(crate) struct GlobalArgs {
    /// Path to the spec file.
    #[arg(short = 'f', long = "file", global = true, default_value = "shipfile.yaml")]
    #[expect(dead_code, reason = "read by spec commands from phase 1")]
    pub(crate) file: PathBuf,

    /// Environment overlay to apply (for example `dev` or `prod`).
    #[arg(short = 'e', long = "env", global = true, env = "SHIPFILE_ENV")]
    #[expect(dead_code, reason = "read by spec commands from phase 1")]
    pub(crate) env: Option<String>,

    /// Print machine-readable JSON on stdout.
    #[arg(long, global = true)]
    pub(crate) json: bool,

    /// Disable coloured output (also honours `NO_COLOR`).
    #[arg(long, global = true)]
    pub(crate) no_color: bool,

    /// More output; repeat for more detail.
    #[arg(short, long, global = true, action = clap::ArgAction::Count, conflicts_with = "quiet")]
    #[expect(dead_code, reason = "wired to logging once there is something to log")]
    pub(crate) verbose: u8,

    /// Only print results and errors.
    #[arg(short, long, global = true)]
    #[expect(dead_code, reason = "wired to progress output from phase 3")]
    pub(crate) quiet: bool,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Check Docker, Compose and AWS setup, and explain how to fix anything missing.
    Doctor,
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
