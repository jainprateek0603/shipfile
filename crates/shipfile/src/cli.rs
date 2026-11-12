use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

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
    pub(crate) file: PathBuf,

    /// Environment overlay to apply (for example `dev` or `prod`).
    #[arg(short = 'e', long = "env", global = true, env = "SHIPFILE_ENV")]
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
    /// Find service folders and write a starter shipfile.yaml.
    Init {
        /// Accept the detected defaults without asking (also when stdin is not a terminal).
        #[arg(short, long)]
        yes: bool,
    },
    /// Validate the spec: schema first, then semantic checks. Errors point to the exact line.
    Check,
    /// Print the resource graph.
    Graph {
        /// Output format. `--json` is the same as `--format json`.
        #[arg(long, value_enum, default_value_t = GraphFormat::Tree)]
        format: GraphFormat,
    },
    /// Print the spec's JSON Schema (for editors and other tools).
    Schema,
    /// Explain an error code in detail, for example `shipfile explain E012`.
    Explain {
        /// The error code, such as `E012`.
        code: String,
    },
    /// Check Docker, Compose and AWS setup, and explain how to fix anything missing.
    Doctor,
    /// Print a shell completion script, e.g. `shipfile completions bash > /etc/bash_completion.d/shipfile`.
    Completions {
        /// Shell to generate for.
        shell: clap_complete::Shell,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum GraphFormat {
    Tree,
    Dot,
    Json,
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
