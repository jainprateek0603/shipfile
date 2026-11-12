//! Repository tasks: `cargo xtask <task>`.
//!
//! - `schema`: write `schema/shipfile.v1.json` from the spec types.
//! - `schema --check`: fail if the committed schema is out of date.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

fn schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("schema").join("shipfile.v1.json")
}

fn schema(check: bool) -> io::Result<bool> {
    let path = schema_path();
    let fresh = shipfile_core::schema::schema_text();
    let mut err = io::stderr().lock();
    if check {
        let committed = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
        if committed == fresh {
            writeln!(err, "schema/shipfile.v1.json is up to date")?;
            return Ok(true);
        }
        writeln!(err, "schema/shipfile.v1.json is out of date; run `cargo xtask schema` and commit the result")?;
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, fresh)?;
    writeln!(err, "wrote schema/shipfile.v1.json")?;
    Ok(true)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
        ["schema"] => schema(false),
        ["schema", "--check"] => schema(true),
        _ => {
            let _ = writeln!(io::stderr(), "usage: cargo xtask schema [--check]");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            let _ = writeln!(io::stderr(), "error: {e}");
            ExitCode::FAILURE
        }
    }
}
