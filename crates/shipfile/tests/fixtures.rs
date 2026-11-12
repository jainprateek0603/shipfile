//! Every broken spec in `tests/fixtures` gives the expected diagnostics (golden snapshots).

use std::path::Path;

use assert_cmd::Command;

fn check(path: &Path) -> (Option<i32>, String, serde_json::Value) {
    let dir = path.parent().expect("fixture folder");
    let file = path.file_name().expect("fixture name");
    let run = |json: bool| {
        let mut cmd = Command::cargo_bin("shipfile").expect("binary builds");
        cmd.current_dir(dir).arg("-f").arg(file).arg("check").arg("--no-color");
        if json {
            cmd.arg("--json");
        }
        cmd.output().expect("runs")
    };
    let human = run(false);
    let json = run(true);
    let report = serde_json::from_slice(&json.stdout).expect("--json prints JSON");
    (human.status.code(), String::from_utf8(human.stderr).expect("utf-8"), report)
}

#[test]
fn broken_fixtures_render_diagnostics() {
    insta::glob!("../../../tests/fixtures", "*.yaml", |path| {
        let (code, stderr, report) = check(path);
        assert_eq!(code, Some(1), "{}: exit code", path.display());
        assert_eq!(report["ok"], false);
        let first = &report["diagnostics"][0];
        // The comment on the first line of every fixture names the code it expects.
        let source = std::fs::read_to_string(path).expect("fixture");
        if let Some(expected) = source.lines().next().and_then(|l| l.strip_prefix("# E")).map(|l| &l[..3]) {
            assert_eq!(first["code"], format!("E{expected}"), "{}", path.display());
        }
        insta::assert_snapshot!(stderr);
    });
}

#[test]
fn crlf_gives_the_same_positions() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let raw = std::fs::read(dir.join("unknown-use-crlf.yaml")).expect("fixture");
    assert!(raw.windows(2).any(|w| w == b"\r\n"), "the CRLF fixture must keep CRLF line endings");
    let (_, _, lf) = check(&dir.join("unknown-use.yaml"));
    let (_, _, crlf) = check(&dir.join("unknown-use-crlf.yaml"));
    for key in ["code", "line", "column", "help"] {
        assert_eq!(lf["diagnostics"][0][key], crlf["diagnostics"][0][key], "{key}");
    }
}
