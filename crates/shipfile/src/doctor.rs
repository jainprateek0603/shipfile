//! `shipfile doctor`: checks the machine is ready and says how to fix what isn't.
//!
//! Probing (running programs, reading env and files) is kept apart from the
//! evaluation of what was found, so the evaluation is unit-testable.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;

use owo_colors::{OwoColorize, Stream};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Status {
    Ok,
    /// Missing, but only needed by later commands.
    Warn,
    /// Missing and needed now.
    Fail,
}

#[derive(Debug, Serialize)]
pub(crate) struct Check {
    pub(crate) name: &'static str,
    pub(crate) status: Status,
    pub(crate) detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) fix: Option<String>,
}

impl Check {
    fn ok(name: &'static str, detail: impl Into<String>) -> Self {
        Self { name, status: Status::Ok, detail: detail.into(), fix: None }
    }

    fn problem(name: &'static str, status: Status, detail: impl Into<String>, fix: impl Into<String>) -> Self {
        Self { name, status, detail: detail.into(), fix: Some(fix.into()) }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct Report {
    pub(crate) version: &'static str,
    pub(crate) ok: bool,
    pub(crate) checks: Vec<Check>,
}

impl Report {
    fn new(checks: Vec<Check>) -> Self {
        let ok = checks.iter().all(|c| c.status != Status::Fail);
        Self { version: env!("CARGO_PKG_VERSION"), ok, checks }
    }
}

/// Result of running an external program.
#[derive(Debug)]
pub(crate) enum Probe {
    NotFound,
    /// Ran but exited non-zero, or could not be started.
    Failed,
    /// Ran successfully; holds trimmed stdout.
    Ran(String),
}

fn probe(program: &str, args: &[&str]) -> Probe {
    match Command::new(program).args(args).output() {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Probe::NotFound,
        Ok(out) if out.status.success() => Probe::Ran(String::from_utf8_lossy(&out.stdout).trim().to_owned()),
        Err(_) | Ok(_) => Probe::Failed,
    }
}

pub(crate) fn run() -> Report {
    let env = |key: &str| std::env::var(key).ok().filter(|v| !v.is_empty());
    let aws_config = read_aws_file(&env, "AWS_CONFIG_FILE", "config");
    Report::new(vec![
        check_docker(&probe("docker", &["version", "--format", "{{.Server.Version}}"])),
        check_compose(&probe("docker", &["compose", "version", "--short"])),
        check_aws_credentials(&env, aws_config.as_deref(), aws_credentials_file_exists(&env)),
        check_aws_region(&env, aws_config.as_deref()),
    ])
}

pub(crate) fn check_docker(probe: &Probe) -> Check {
    const NAME: &str = "docker";
    match probe {
        Probe::Ran(version) => Check::ok(NAME, format!("{version} (daemon running)")),
        Probe::NotFound => Check::problem(
            NAME,
            Status::Fail,
            "not installed",
            "install Docker Desktop (https://docs.docker.com/get-docker/) or Docker Engine",
        ),
        Probe::Failed => Check::problem(
            NAME,
            Status::Fail,
            "installed, but the daemon is not reachable",
            "start Docker Desktop, or run `sudo systemctl start docker` on Linux",
        ),
    }
}

pub(crate) fn check_compose(probe: &Probe) -> Check {
    const NAME: &str = "docker compose";
    let fix = "install Docker Compose v2 (it ships with Docker Desktop; on Linux install the `docker-compose-plugin` package)";
    match probe {
        Probe::Ran(version) => match compose_major(version) {
            Some(major) if major >= 2 => Check::ok(NAME, version.trim_start_matches('v')),
            Some(_) => Check::problem(NAME, Status::Fail, format!("{version} is too old; v2 or later is needed"), fix),
            None => Check::problem(NAME, Status::Warn, format!("could not read the version from `{version}`"), fix),
        },
        Probe::NotFound | Probe::Failed => Check::problem(NAME, Status::Fail, "not available", fix),
    }
}

fn compose_major(version: &str) -> Option<u32> {
    version.trim().trim_start_matches('v').split('.').next()?.parse().ok()
}

pub(crate) fn check_aws_credentials(
    env: &dyn Fn(&str) -> Option<String>,
    config: Option<&str>,
    credentials_file: bool,
) -> Check {
    const NAME: &str = "aws credentials";
    let source = if env("AWS_ACCESS_KEY_ID").is_some() {
        Some("environment variables".to_owned())
    } else if env("AWS_WEB_IDENTITY_TOKEN_FILE").is_some() {
        Some("web identity token".to_owned())
    } else if let Some(profile) = env("AWS_PROFILE") {
        Some(format!("profile `{profile}`"))
    } else if credentials_file {
        Some("shared credentials file".to_owned())
    } else if config.is_some_and(|c| c.contains("sso_")) {
        Some("SSO in the shared config file".to_owned())
    } else {
        None
    };
    match source {
        // Whether they are valid is checked with STS from phase 5.
        Some(source) => Check::ok(NAME, format!("found ({source}); not yet verified")),
        None => Check::problem(
            NAME,
            Status::Warn,
            "none found",
            "run `aws configure` or `aws sso login`; only needed for bootstrap, plan and apply",
        ),
    }
}

pub(crate) fn check_aws_region(env: &dyn Fn(&str) -> Option<String>, config: Option<&str>) -> Check {
    const NAME: &str = "aws region";
    let profile = env("AWS_PROFILE").unwrap_or_else(|| "default".to_owned());
    let found = env("AWS_REGION")
        .map(|r| (r, "AWS_REGION".to_owned()))
        .or_else(|| env("AWS_DEFAULT_REGION").map(|r| (r, "AWS_DEFAULT_REGION".to_owned())))
        .or_else(|| config.and_then(|c| region_from_config(c, &profile)).map(|r| (r, format!("profile `{profile}`"))));
    match found {
        Some((region, source)) => Check::ok(NAME, format!("{region} ({source})")),
        None => Check::problem(
            NAME,
            Status::Warn,
            "not set",
            "set `aws.region` in shipfile.yaml, or `AWS_REGION`; only needed for bootstrap, plan and apply",
        ),
    }
}

/// Finds `region` in the given profile's section of an AWS shared config file.
pub(crate) fn region_from_config(config: &str, profile: &str) -> Option<String> {
    let wanted = if profile == "default" { "default".to_owned() } else { format!("profile {profile}") };
    let mut in_section = false;
    for line in config.lines().map(str::trim) {
        if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_section = header.trim() == wanted;
        } else if in_section
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == "region"
        {
            return Some(value.trim().to_owned()).filter(|v| !v.is_empty());
        }
    }
    None
}

fn aws_file_path(env: &dyn Fn(&str) -> Option<String>, override_var: &str, name: &str) -> Option<PathBuf> {
    env(override_var).map(PathBuf::from).or_else(|| std::env::home_dir().map(|h| h.join(".aws").join(name)))
}

fn read_aws_file(env: &dyn Fn(&str) -> Option<String>, override_var: &str, name: &str) -> Option<String> {
    std::fs::read_to_string(aws_file_path(env, override_var, name)?).ok()
}

fn aws_credentials_file_exists(env: &dyn Fn(&str) -> Option<String>) -> bool {
    aws_file_path(env, "AWS_SHARED_CREDENTIALS_FILE", "credentials").is_some_and(|p| p.is_file())
}

pub(crate) fn render(report: &Report, out: &mut impl Write) -> io::Result<()> {
    let width = report.checks.iter().map(|c| c.name.len()).max().unwrap_or(0);
    writeln!(out)?;
    for check in &report.checks {
        let mark = match check.status {
            Status::Ok => "✓".if_supports_color(Stream::Stdout, |t| t.green()).to_string(),
            Status::Warn => "!".if_supports_color(Stream::Stdout, |t| t.yellow()).to_string(),
            Status::Fail => "✗".if_supports_color(Stream::Stdout, |t| t.red()).to_string(),
        };
        writeln!(out, "  {mark} {:<width$}  {}", check.name, check.detail)?;
        if let Some(fix) = &check.fix {
            writeln!(out, "    {:<width$}  {} {fix}", "", "fix:".if_supports_color(Stream::Stdout, |t| t.dimmed()))?;
        }
    }
    writeln!(out)?;
    if !report.ok {
        writeln!(out, "  Not ready: fix the items marked ✗ and run `shipfile doctor` again.")?;
    } else if report.checks.iter().any(|c| c.status == Status::Warn) {
        writeln!(out, "  Ready for local work. Items marked ! are only needed for the AWS commands.")?;
    } else {
        writeln!(out, "  Ready.")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |key| pairs.iter().find(|(k, _)| *k == key).map(|(_, v)| (*v).to_owned())
    }

    fn rendered(checks: Vec<Check>) -> String {
        let mut out = Vec::new();
        render(&Report::new(checks), &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn docker_states() {
        assert_eq!(check_docker(&Probe::Ran("29.7.2".into())).status, Status::Ok);
        assert_eq!(check_docker(&Probe::NotFound).status, Status::Fail);
        let down = check_docker(&Probe::Failed);
        assert_eq!(down.status, Status::Fail);
        assert!(down.detail.contains("daemon"));
    }

    #[test]
    fn compose_needs_v2() {
        assert_eq!(check_compose(&Probe::Ran("2.29.1".into())).status, Status::Ok);
        assert_eq!(check_compose(&Probe::Ran("v2.40.0-desktop.1".into())).detail, "2.40.0-desktop.1");
        assert_eq!(check_compose(&Probe::Ran("1.29.2".into())).status, Status::Fail);
        assert_eq!(check_compose(&Probe::Ran("garbage".into())).status, Status::Warn);
        assert_eq!(check_compose(&Probe::Failed).status, Status::Fail);
    }

    #[test]
    fn missing_aws_setup_is_only_a_warning() {
        let env = env_of(&[]);
        assert_eq!(check_aws_credentials(&env, None, false).status, Status::Warn);
        assert_eq!(check_aws_region(&env, None).status, Status::Warn);
    }

    #[test]
    fn credentials_sources() {
        let env = env_of(&[("AWS_ACCESS_KEY_ID", "AKIAEXAMPLE")]);
        let check = check_aws_credentials(&env, None, false);
        assert_eq!(check.status, Status::Ok);
        assert!(!check.detail.contains("AKIA"), "never print key material");
        assert_eq!(check_aws_credentials(&env_of(&[]), None, true).status, Status::Ok);
        let sso = "[profile dev]\nsso_session = corp\n";
        assert_eq!(check_aws_credentials(&env_of(&[]), Some(sso), false).status, Status::Ok);
    }

    #[test]
    fn region_precedence() {
        let config = "[default]\nregion = eu-west-1\n";
        let env = env_of(&[("AWS_REGION", "ap-south-1"), ("AWS_DEFAULT_REGION", "us-east-1")]);
        assert_eq!(check_aws_region(&env, Some(config)).detail, "ap-south-1 (AWS_REGION)");
        let env = env_of(&[("AWS_DEFAULT_REGION", "us-east-1")]);
        assert_eq!(check_aws_region(&env, Some(config)).detail, "us-east-1 (AWS_DEFAULT_REGION)");
        assert_eq!(check_aws_region(&env_of(&[]), Some(config)).detail, "eu-west-1 (profile `default`)");
    }

    #[test]
    fn region_from_named_profile() {
        let config = "[default]\nregion = eu-west-1\n\n[profile dev]\noutput = json\nregion=ap-south-1\n";
        assert_eq!(region_from_config(config, "dev").as_deref(), Some("ap-south-1"));
        assert_eq!(region_from_config(config, "default").as_deref(), Some("eu-west-1"));
        assert_eq!(region_from_config(config, "missing"), None);
    }

    #[test]
    fn render_summary_lines() {
        let failing = rendered(vec![check_docker(&Probe::NotFound)]);
        assert!(failing.contains("fix:") && failing.contains("Not ready"));
        assert!(rendered(vec![check_aws_region(&env_of(&[]), None)]).contains("Ready for local work"));
        assert!(rendered(vec![check_compose(&Probe::Ran("2.29.1".into()))]).trim_end().ends_with("Ready."));
    }
}
