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
    assert_eq!(report["version"], env!("CARGO_PKG_VERSION"));
    let checks = report["checks"].as_array().expect("checks array");
    let names: Vec<_> = checks.iter().map(|c| c["name"].as_str().expect("name")).collect();
    assert_eq!(names, ["docker", "docker compose", "aws credentials", "aws region"]);
    assert_eq!(output.status.success(), report["ok"].as_bool().expect("ok flag"));
}

mod spec {
    use assert_fs::TempDir;
    use assert_fs::prelude::*;
    use predicates::prelude::*;

    use super::shipfile;

    const ORDERS: &str = "version: 1
name: orders
services:
  api:
    build: ./api
    port: 3000
    public: true
    health: /healthz
    uses: [orders-qeue]
queues:
  orders-queue: {}
";

    fn project(spec: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        dir.child("shipfile.yaml").write_str(spec).unwrap();
        dir
    }

    #[test]
    fn init_yes_writes_a_spec_that_passes_check() {
        let dir = TempDir::new().unwrap();
        dir.child("api/package.json").write_str("{}").unwrap();
        dir.child("worker/go.mod").write_str("module worker").unwrap();
        shipfile()
            .current_dir(&dir)
            .args(["init", "--yes"])
            .env_remove("AWS_REGION")
            .env_remove("AWS_DEFAULT_REGION")
            .assert()
            .success()
            .stdout(predicate::str::contains("wrote shipfile.yaml with 2 service(s): api, worker"));
        let text = std::fs::read_to_string(dir.child("shipfile.yaml").path()).unwrap();
        assert!(!text.contains('\r'), "LF only");
        assert!(text.contains("region: us-east-1"));
        shipfile().current_dir(&dir).arg("check").assert().success();
        // Never overwrite.
        shipfile()
            .current_dir(&dir)
            .args(["init", "--yes"])
            .assert()
            .code(1)
            .stderr(predicate::str::contains("already exists"));
    }

    #[test]
    fn check_reports_errors_on_stderr_with_exit_1() {
        let dir = project(ORDERS);
        shipfile()
            .current_dir(&dir)
            .args(["check", "--no-color"])
            .assert()
            .code(1)
            .stdout("")
            .stderr(predicate::str::contains("error[E012]: unknown resource `orders-qeue` in `uses`"))
            .stderr(predicate::str::contains("shipfile.yaml:9:12"))
            .stderr(predicate::str::contains("did you mean `orders-queue`?"));
    }

    #[test]
    fn check_json_is_machine_readable() {
        let dir = project(ORDERS);
        let output = shipfile().current_dir(&dir).args(["check", "--json"]).output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
        assert_eq!(report["ok"], false);
        assert_eq!(report["errors"], 1);
        assert_eq!(report["warnings"], 1);
        let first = &report["diagnostics"][0];
        assert_eq!(first["code"], "E012");
        assert_eq!((first["line"].as_u64(), first["column"].as_u64()), (Some(9), Some(12)));
        assert_eq!(first["file"], "shipfile.yaml");
    }

    #[test]
    fn check_honours_file_and_env_flags() {
        let dir = TempDir::new().unwrap();
        dir.child("specs/app.yaml")
            .write_str("version: 1\nname: app\nservices:\n  web:\n    build: .\nenvironments:\n  prod: {}\n")
            .unwrap();
        shipfile()
            .current_dir(&dir)
            .args(["check", "-f", "specs/app.yaml", "-e", "prod"])
            .assert()
            .success()
            .stdout(predicate::str::contains("specs/app.yaml is valid").and(predicate::str::contains("env `prod`")));
        shipfile()
            .current_dir(&dir)
            .args(["check", "-f", "specs/app.yaml", "-e", "prd", "--no-color"])
            .assert()
            .code(1)
            .stderr(predicate::str::contains("E022").and(predicate::str::contains("did you mean `prod`?")));
        shipfile().current_dir(&dir).arg("check").assert().code(1).stderr(predicate::str::contains("shipfile init"));
    }

    #[test]
    fn graph_formats() {
        let dir = project(&ORDERS.replace("orders-qeue", "orders-queue"));
        shipfile()
            .current_dir(&dir)
            .arg("graph")
            .assert()
            .success()
            .stdout(predicate::str::contains("uses queues.orders-queue"));
        shipfile()
            .current_dir(&dir)
            .args(["graph", "--format", "dot"])
            .assert()
            .success()
            .stdout(predicate::str::contains("\"services.api\" -> \"queues.orders-queue\";"));
        let output = shipfile().current_dir(&dir).args(["graph", "--json"]).output().unwrap();
        assert!(output.status.success());
        let model: serde_json::Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
        assert_eq!(model["uses"][0]["kind"], "queue");
        assert_eq!(model["services"]["api"]["scale"]["replicas"], 1);
    }

    #[test]
    fn graph_refuses_an_invalid_spec() {
        let dir = project(ORDERS);
        shipfile()
            .current_dir(&dir)
            .args(["graph", "--no-color"])
            .assert()
            .code(1)
            .stdout("")
            .stderr(predicate::str::contains("E012"));
    }
}
