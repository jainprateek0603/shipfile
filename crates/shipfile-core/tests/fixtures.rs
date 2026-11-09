//! Golden tests: every fixture spec → rendered diagnostics (invalid) or graph + contracts (valid).
//!
//! A fixture may start with `# check-env: <name>` to check it as `shipfile check -e <name>`.

use std::fmt::Write as _;
use std::path::Path;

use shipfile_core::diagnostics::render;
use shipfile_core::spec::Runtime;
use shipfile_core::{Model, check, graph};

fn env_of(source: &str) -> Option<&str> {
    source.lines().next()?.strip_prefix("# check-env: ").map(str::trim)
}

fn read(path: &Path) -> (String, String) {
    let source = std::fs::read_to_string(path).expect("fixture is readable").replace("\r\n", "\n");
    let name = path.file_name().expect("fixture has a name").to_string_lossy().into_owned();
    (name, source)
}

#[test]
fn invalid_fixtures() {
    insta::glob!("fixtures/invalid/*.yaml", |path| {
        let (name, source) = read(path);
        let checked = check(&source, env_of(&source));
        assert!(checked.has_errors(), "{name} should fail");
        insta::assert_snapshot!(render(&name, &source, &checked.diagnostics, false));
    });
}

#[test]
fn invalid_fixtures_json() {
    insta::glob!("fixtures/invalid/*.yaml", |path| {
        let (name, source) = read(path);
        let checked = check(&source, env_of(&source));
        let json: Vec<_> = checked.diagnostics.iter().map(|d| d.to_json(&name)).collect();
        insta::assert_json_snapshot!(json);
    });
}

#[test]
fn valid_fixtures() {
    insta::glob!("fixtures/valid/*.yaml", |path| {
        let (name, source) = read(path);
        let checked = check(&source, env_of(&source));
        assert!(!checked.has_errors(), "{name}:\n{}", render(&name, &source, &checked.diagnostics, false));
        let spec = checked.spec.unwrap();
        let mut envs: Vec<Option<&str>> = vec![None];
        envs.extend(spec.environments.keys().map(|e| Some(e.as_str())));
        let mut out = String::new();
        for env in envs {
            let model = Model::resolve(&spec, env, &|_| Some(Runtime::Node22));
            writeln!(out, "== env: {} ==", env.unwrap_or("(none)")).unwrap();
            out.push_str(&graph::tree(&model));
            for svc in model.services.keys() {
                let contract = model.contract(svc).unwrap();
                out.push_str(&serde_json::to_string_pretty(&contract).unwrap());
                out.push('\n');
            }
        }
        if !checked.diagnostics.is_empty() {
            out.push_str(&render(&name, &source, &checked.diagnostics, false));
        }
        insta::assert_snapshot!(out);
    });
}

#[test]
fn enough_broken_fixtures() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/invalid");
    assert!(std::fs::read_dir(dir).unwrap().count() >= 15);
}
