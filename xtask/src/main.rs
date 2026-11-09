//! Repository chores. Run with `cargo xtask <task>`.
//!
//! - `schema`: write `schema/shipfile.v1.json` from the spec types.
//! - `schema --check`: fail if the committed schema is stale.

use std::path::PathBuf;
use std::process::ExitCode;

#[expect(clippy::print_stderr, reason = "a dev tool that reports to the terminal")]
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["schema"] | ["schema", "--check"] => {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../schema/shipfile.v1.json");
            let fresh = shipfile_core::schema::schema_file();
            if args.len() == 2 {
                let committed = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
                if committed == fresh {
                    eprintln!("schema/shipfile.v1.json is up to date");
                    return ExitCode::SUCCESS;
                }
                eprintln!("schema/shipfile.v1.json is stale: run `cargo xtask schema` and commit the result");
                return ExitCode::FAILURE;
            }
            if let Some(dir) = path.parent()
                && let Err(e) = std::fs::create_dir_all(dir)
            {
                eprintln!("cannot create {}: {e}", dir.display());
                return ExitCode::FAILURE;
            }
            if let Err(e) = std::fs::write(&path, fresh) {
                eprintln!("cannot write {}: {e}", path.display());
                return ExitCode::FAILURE;
            }
            eprintln!("wrote schema/shipfile.v1.json");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("usage: cargo xtask schema [--check]");
            ExitCode::FAILURE
        }
    }
}
