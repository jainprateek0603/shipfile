use assert_cmd::Command;
use predicates::prelude::*;

fn shipfile() -> Command {
    Command::cargo_bin("shipfile").expect("binary builds")
}

#[test]
fn version_prints_crate_version() {
    shipfile()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(concat!("shipfile ", env!("CARGO_PKG_VERSION"))));
}

#[test]
fn no_subcommand_is_a_usage_error() {
    shipfile().assert().failure().stderr(predicate::str::contains("Usage"));
}

#[test]
fn doctor_json_has_stable_shape() {
    // Exit code depends on the machine (Docker may be missing in CI), so only the shape is checked.
    let output = shipfile().args(["doctor", "--json"]).env("NO_COLOR", "1").output().expect("runs");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["version"], env!("CARGO_PKG_VERSION"));
    let checks = report["checks"].as_array().expect("checks array");
    let names: Vec<_> = checks.iter().map(|c| c["name"].as_str().expect("name")).collect();
    assert_eq!(
        names,
        [
            "docker",
            "docker compose",
            "docker buildx",
            "git",
            "local ports",
            "disk space",
            "aws credentials",
            "aws region",
            "session manager",
        ]
    );
    assert_eq!(output.status.success(), report["ok"].as_bool().expect("ok flag"));
}

#[test]
fn usage_error_does_not_exit_2() {
    // 2 means "plan has changes"; a typo must not look like that.
    shipfile().arg("--bogus").assert().code(1).stderr(predicate::str::contains("--bogus"));
    shipfile().assert().code(1);
}

#[test]
fn help_exits_0() {
    shipfile().arg("--help").assert().code(0).stdout(predicate::str::contains("Usage"));
}

#[test]
fn completions_for_each_shell() {
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        shipfile().args(["completions", shell]).assert().success().stdout(predicate::str::contains("shipfile"));
    }
}

#[test]
fn doctor_ascii_output_is_ascii() {
    let output = shipfile().args(["doctor", "--ascii", "--no-color"]).output().expect("runs");
    assert!(output.stdout.is_ascii(), "{}", String::from_utf8_lossy(&output.stdout));
}

#[test]
fn verbose_logs_go_to_stderr() {
    let output = shipfile().args(["doctor", "--json", "-vv"]).output().expect("runs");
    let _: serde_json::Value = serde_json::from_slice(&output.stdout).expect("stdout stays pure JSON");
    assert!(String::from_utf8_lossy(&output.stderr).contains("probe"));
}

mod spec {
    use std::path::{Path, PathBuf};

    use assert_fs::TempDir;
    use assert_fs::prelude::*;
    use predicates::prelude::*;

    use super::shipfile;

    fn orders() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/orders")
    }

    /// Runs a spec command against `examples/orders` from inside that folder.
    fn in_orders(args: &[&str]) -> assert_cmd::assert::Assert {
        shipfile().current_dir(orders()).args(args).arg("--no-color").assert()
    }

    fn stdout_of(args: &[&str]) -> String {
        String::from_utf8(in_orders(args).success().get_output().stdout.clone()).expect("utf-8")
    }

    #[test]
    fn example_passes_check() {
        in_orders(&["check"]).success().stdout("✓ shipfile.yaml is valid: 2 services, 1 queue, 2 databases\n");
        in_orders(&["-e", "prod", "check"]).success().stdout(predicate::str::contains("(environment `prod`)"));
    }

    #[test]
    fn check_json_shape() {
        let report: serde_json::Value = serde_json::from_str(&stdout_of(&["check", "--json"])).expect("JSON");
        assert_eq!(
            report,
            serde_json::json!({
                "ok": true, "file": "shipfile.yaml", "env": null, "diagnostics": [],
                "resources": {"services": 2, "queues": 1, "databases": 2}
            })
        );
    }

    #[test]
    fn graph_golden_tree_dot_json() {
        insta::assert_snapshot!("graph-tree", stdout_of(&["graph"]));
        insta::assert_snapshot!("graph-tree-prod", stdout_of(&["-e", "prod", "graph"]));
        insta::assert_snapshot!("graph-dot", stdout_of(&["graph", "--format", "dot"]));
        let json = stdout_of(&["graph", "--format", "json"]);
        assert_eq!(json, stdout_of(&["graph", "--json"]), "--json is --format json");
        let value: serde_json::Value = serde_json::from_str(&json).expect("JSON");
        insta::assert_json_snapshot!("graph-json", value);
    }

    #[test]
    fn unknown_environment_suggests_one() {
        in_orders(&["-e", "prd", "check"])
            .code(1)
            .stderr(predicate::str::contains("error[E020]: unknown environment `prd`"))
            .stderr(predicate::str::contains("did you mean `prod`?"));
    }

    #[test]
    fn missing_spec_says_what_to_do() {
        let dir = TempDir::new().expect("temp dir");
        shipfile()
            .current_dir(dir.path())
            .args(["check", "--no-color"])
            .assert()
            .code(1)
            .stderr(predicate::str::contains("no spec found at shipfile.yaml"))
            .stderr(predicate::str::contains("shipfile init"));
    }

    #[test]
    fn failed_check_json_has_positions() {
        let dir = TempDir::new().expect("temp dir");
        dir.child("shipfile.yaml")
            .write_str("version: 1\nname: x\nservices:\n  api:\n    build: .\n    prot: 1\n")
            .unwrap();
        let out = shipfile().current_dir(dir.path()).args(["check", "--json"]).assert().code(1).get_output().clone();
        assert!(out.stderr.is_empty(), "--json keeps stderr quiet");
        let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("JSON");
        assert_eq!(report["ok"], false);
        let diag = &report["diagnostics"][0];
        assert_eq!(diag["code"], "E003");
        assert_eq!((diag["line"].as_u64(), diag["column"].as_u64()), (Some(6), Some(5)));
        assert_eq!(diag["help"], "did you mean `port`?");
    }

    #[test]
    fn init_yes_writes_a_spec_that_checks() {
        let dir = TempDir::new().expect("temp dir");
        dir.child("api/package.json").write_str(r#"{"engines": {"node": "20"}}"#).unwrap();
        dir.child("worker/pyproject.toml").write_str("[project]\nrequires-python = \">=3.12\"\n").unwrap();
        dir.child("node_modules/x/package.json").write_str("{}").unwrap();
        shipfile()
            .current_dir(dir.path())
            .args(["init", "--yes"])
            .assert()
            .success()
            .stdout(predicate::str::contains("wrote shipfile.yaml with 2 services"));
        let spec = std::fs::read_to_string(dir.path().join("shipfile.yaml")).unwrap();
        assert!(spec.contains("  api:\n    build: ./api\n    runtime: node20"), "{spec}");
        assert!(spec.contains("  worker:\n    build: ./worker\n    runtime: python3.12"), "{spec}");
        assert!(!spec.contains('\r'));
        shipfile().current_dir(dir.path()).arg("check").assert().success();
    }

    #[test]
    fn init_without_a_tty_does_not_prompt() {
        let dir = TempDir::new().expect("temp dir");
        dir.child("go.mod").write_str("module x\n\ngo 1.23\n").unwrap();
        // assert_cmd gives the child no terminal on stdin, so init must not wait for answers.
        shipfile().current_dir(dir.path()).arg("init").write_stdin("").assert().success();
        dir.child("shipfile.yaml").assert(predicate::str::contains("build: .\n"));
    }

    #[test]
    fn init_refuses_to_overwrite() {
        let dir = TempDir::new().expect("temp dir");
        dir.child("shipfile.yaml").write_str("keep me\n").unwrap();
        shipfile()
            .current_dir(dir.path())
            .args(["init", "--yes", "--no-color"])
            .assert()
            .code(1)
            .stderr(predicate::str::contains("already exists"));
        dir.child("shipfile.yaml").assert("keep me\n");
    }

    #[test]
    fn file_flag_points_elsewhere() {
        shipfile().arg("-f").arg(orders().join("shipfile.yaml")).arg("check").assert().success();
    }
}
