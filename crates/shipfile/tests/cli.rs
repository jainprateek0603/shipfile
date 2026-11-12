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
