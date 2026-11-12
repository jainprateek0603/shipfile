//! `shipfile doctor`: checks the machine is ready and says how to fix what isn't.
//!
//! Probing (running programs, reading env and files) is kept apart from the
//! evaluation of what was found, so the evaluation is unit-testable.

use std::io::{self, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

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
    /// Shape of this JSON; raised only on breaking changes. See `docs/json-output.md`.
    pub(crate) schema_version: u32,
    pub(crate) version: &'static str,
    pub(crate) ok: bool,
    pub(crate) checks: Vec<Check>,
}

impl Report {
    fn new(checks: Vec<Check>) -> Self {
        let ok = checks.iter().all(|c| c.status != Status::Fail);
        Self { schema_version: 1, version: env!("CARGO_PKG_VERSION"), ok, checks }
    }
}

/// Result of running an external program.
#[derive(Debug)]
pub(crate) enum Probe {
    NotFound,
    /// Ran but exited non-zero, or could not be started.
    Failed,
    /// Still running at the deadline, so it was killed.
    TimedOut,
    /// Ran successfully; holds trimmed stdout.
    Ran(String),
}

/// How long one probe may run; `docker version` stalls while Docker Desktop starts.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

fn probe(program: &str, args: &[&str]) -> Probe {
    let started = Instant::now();
    let result = probe_inner(program, args);
    tracing::debug!(program, ?args, elapsed = ?started.elapsed(), ?result, "probe");
    result
}

fn probe_inner(program: &str, args: &[&str]) -> Probe {
    let mut child = match Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Probe::NotFound,
        Err(_) => return Probe::Failed,
        Ok(child) => child,
    };
    // Drain stdout on a thread so a chatty program can't block on a full pipe.
    let mut stdout = child.stdout.take();
    let reader = thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(out) = stdout.as_mut() {
            let _ = out.read_to_end(&mut buf);
        }
        buf
    });
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Probe::TimedOut;
            }
            Err(_) => break None,
        }
    };
    let out = reader.join().unwrap_or_default();
    match status {
        Some(s) if s.success() => Probe::Ran(String::from_utf8_lossy(&out).trim().to_owned()),
        _ => Probe::Failed,
    }
}

pub(crate) fn run() -> Report {
    let env = |key: &str| std::env::var(key).ok().filter(|v| !v.is_empty());
    let aws_config = read_aws_file(&env, "AWS_CONFIG_FILE", "config");
    let aws_credentials = read_aws_file(&env, "AWS_SHARED_CREDENTIALS_FILE", "credentials");
    // Probes are independent and each may wait up to the timeout, so run them together.
    let [docker, compose, buildx, git, ssm] = thread::scope(|s| {
        [
            s.spawn(|| probe("docker", &["version", "--format", "{{.Server.Version}}"])),
            s.spawn(|| probe("docker", &["compose", "version", "--short"])),
            s.spawn(|| probe("docker", &["buildx", "version"])),
            s.spawn(|| probe("git", &["--version"])),
            s.spawn(|| probe("session-manager-plugin", &["--version"])),
        ]
        .map(|h| h.join().unwrap_or(Probe::Failed))
    });
    let busy: Vec<u16> = LOCAL_PORTS.into_iter().filter(|p| TcpListener::bind(("127.0.0.1", *p)).is_err()).collect();
    let free_bytes = fs4::available_space(std::env::current_dir().unwrap_or_else(|_| ".".into())).ok();
    Report::new(vec![
        check_docker(&docker),
        check_compose(&compose),
        check_buildx(&buildx),
        check_git(&git),
        check_ports(&busy),
        check_disk(free_bytes),
        check_aws_credentials(&env, aws_config.as_deref(), aws_credentials.as_deref()),
        check_aws_region(&env, aws_config.as_deref()),
        check_session_manager(&ssm),
    ])
}

/// Ports the local stack binds: Postgres, the default web port, ElasticMQ (SQS).
const LOCAL_PORTS: [u16; 3] = [5432, 8000, 9324];
const MIN_FREE_BYTES: u64 = 5 * 1024 * 1024 * 1024;

pub(crate) fn check_buildx(probe: &Probe) -> Check {
    const NAME: &str = "docker buildx";
    match probe {
        Probe::Ran(v) => Check::ok(NAME, v.split_whitespace().nth(1).unwrap_or(v).to_owned()),
        _ => Check::problem(
            NAME,
            Status::Warn,
            "not available",
            "install the buildx plugin (ships with Docker Desktop); needed to build linux/arm64 images for AWS",
        ),
    }
}

pub(crate) fn check_git(probe: &Probe) -> Check {
    const NAME: &str = "git";
    match probe {
        Probe::Ran(v) => Check::ok(NAME, v.trim_start_matches("git version ").to_owned()),
        _ => Check::problem(NAME, Status::Warn, "not found", "install git (https://git-scm.com/downloads)"),
    }
}

pub(crate) fn check_ports(busy: &[u16]) -> Check {
    const NAME: &str = "local ports";
    let list = |ports: &[u16]| ports.iter().map(u16::to_string).collect::<Vec<_>>().join(", ");
    if busy.is_empty() {
        Check::ok(NAME, format!("{} free", list(&LOCAL_PORTS)))
    } else {
        Check::problem(
            NAME,
            Status::Warn,
            format!("{} already in use", list(busy)),
            "stop whatever listens there (or another `shipfile up`) before `shipfile up`",
        )
    }
}

#[expect(clippy::cast_precision_loss, reason = "display only")]
pub(crate) fn check_disk(free_bytes: Option<u64>) -> Check {
    const NAME: &str = "disk space";
    let gb = |b: u64| format!("{:.1} GB free", b as f64 / 1e9);
    match free_bytes {
        Some(b) if b >= MIN_FREE_BYTES => Check::ok(NAME, gb(b)),
        Some(b) => Check::problem(NAME, Status::Warn, gb(b), "free up space; images and volumes need about 5 GB"),
        None => {
            Check::problem(NAME, Status::Warn, "could not be read", "check free space by hand; about 5 GB is needed")
        }
    }
}

pub(crate) fn check_session_manager(probe: &Probe) -> Check {
    const NAME: &str = "session manager";
    match probe {
        Probe::Ran(v) => Check::ok(NAME, v.clone()),
        _ => Check::problem(
            NAME,
            Status::Warn,
            "plugin not found",
            "install the AWS Session Manager plugin; only needed for `shipfile shell`",
        ),
    }
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
        Probe::TimedOut => Check::problem(
            NAME,
            Status::Fail,
            "did not answer within 5 s",
            "Docker is starting or stuck; retry in a moment, or restart Docker",
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
        Probe::TimedOut => Check::problem(
            NAME,
            Status::Fail,
            "did not answer within 5 s",
            "Docker is starting or stuck; retry in a moment",
        ),
    }
}

fn compose_major(version: &str) -> Option<u32> {
    version.trim().trim_start_matches('v').split('.').next()?.parse().ok()
}

pub(crate) fn check_aws_credentials(
    env: &dyn Fn(&str) -> Option<String>,
    config: Option<&str>,
    credentials: Option<&str>,
) -> Check {
    const NAME: &str = "aws credentials";
    let fix = "run `aws login`, `aws configure` or `aws sso login`; only needed for bootstrap, plan and apply";
    let explicit_profile = env("AWS_PROFILE");
    let profile = explicit_profile.clone().unwrap_or_else(|| "default".to_owned());
    let source = if env("AWS_ACCESS_KEY_ID").is_some() {
        Some("environment variables".to_owned())
    } else if env("AWS_WEB_IDENTITY_TOKEN_FILE").is_some() {
        Some("web identity token".to_owned())
    } else if env("AWS_CONTAINER_CREDENTIALS_FULL_URI").is_some()
        || env("AWS_CONTAINER_CREDENTIALS_RELATIVE_URI").is_some()
    {
        Some("container credentials endpoint".to_owned())
    } else {
        profile_source(config, credentials, &profile).map(|kind| format!("profile `{profile}`, {kind}"))
    };
    match (source, explicit_profile) {
        // Whether they are valid is checked with STS from phase 5.
        (Some(source), _) => Check::ok(NAME, format!("found ({source}); not yet verified")),
        (None, Some(p)) if !profile_exists(config, credentials, &p) => Check::problem(
            NAME,
            Status::Warn,
            format!("AWS_PROFILE is `{p}`, but no such profile exists"),
            "fix the name, or create it with `aws configure --profile <name>`",
        ),
        (None, Some(p)) => Check::problem(NAME, Status::Warn, format!("profile `{p}` has no credential settings"), fix),
        (None, None) => Check::problem(NAME, Status::Warn, "none found (an EC2 instance role is not detected)", fix),
    }
}

/// Section header for `profile` in the shared config file.
fn config_header(profile: &str) -> String {
    if profile == "default" { "default".to_owned() } else { format!("profile {profile}") }
}

/// The `key = value` lines of one `[header]` section of an AWS ini file, if present.
pub(crate) fn profile_section<'a>(file: &'a str, header: &str) -> Option<Vec<(&'a str, &'a str)>> {
    let mut found = None::<Vec<_>>;
    let mut in_section = false;
    for line in file.lines().map(str::trim) {
        if let Some(h) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_section = h.trim() == header;
            if in_section {
                found.get_or_insert_with(Vec::new);
            }
        } else if in_section && let Some((k, v)) = line.split_once('=') {
            found.get_or_insert_with(Vec::new).push((k.trim(), v.trim()));
        }
    }
    found
}

fn profile_exists(config: Option<&str>, credentials: Option<&str>, profile: &str) -> bool {
    config.and_then(|c| profile_section(c, &config_header(profile))).is_some()
        || credentials.and_then(|c| profile_section(c, profile)).is_some()
}

/// How the active profile gets credentials, looking only inside its own section.
fn profile_source(config: Option<&str>, credentials: Option<&str>, profile: &str) -> Option<&'static str> {
    let has = |section: &[(&str, &str)], key: &str| section.iter().any(|(k, _)| *k == key);
    if credentials.and_then(|c| profile_section(c, profile)).is_some_and(|s| has(&s, "aws_access_key_id")) {
        return Some("credentials file");
    }
    let section = config.and_then(|c| profile_section(c, &config_header(profile)))?;
    if section.iter().any(|(k, _)| k.starts_with("sso_")) {
        Some("SSO")
    } else if has(&section, "login_session") {
        Some("`aws login` session")
    } else if has(&section, "credential_process") {
        Some("credential_process")
    } else if has(&section, "role_arn") {
        Some("assumed role")
    } else if has(&section, "aws_access_key_id") {
        Some("config file")
    } else {
        None
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
    profile_section(config, &config_header(profile))?
        .into_iter()
        .find(|(k, _)| *k == "region")
        .map(|(_, v)| v.to_owned())
        .filter(|v| !v.is_empty())
}

fn aws_file_path(env: &dyn Fn(&str) -> Option<String>, override_var: &str, name: &str) -> Option<PathBuf> {
    env(override_var).map(PathBuf::from).or_else(|| std::env::home_dir().map(|h| h.join(".aws").join(name)))
}

fn read_aws_file(env: &dyn Fn(&str) -> Option<String>, override_var: &str, name: &str) -> Option<String> {
    std::fs::read_to_string(aws_file_path(env, override_var, name)?).ok()
}

/// Status marks; ASCII for consoles that can't show Unicode (legacy Windows, `TERM=dumb`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Marks {
    ok: &'static str,
    warn: &'static str,
    fail: &'static str,
}

impl Marks {
    pub(crate) const UNICODE: Self = Self { ok: "✓", warn: "!", fail: "✗" };
    pub(crate) const ASCII: Self = Self { ok: "[ok]", warn: "[!]", fail: "[x]" };

    pub(crate) fn detect(force_ascii: bool) -> Self {
        let dumb = std::env::var("TERM").is_ok_and(|t| t == "dumb");
        if force_ascii || dumb || !supports_unicode::on(supports_unicode::Stream::Stdout) {
            Self::ASCII
        } else {
            Self::UNICODE
        }
    }
}

pub(crate) fn render(report: &Report, marks: Marks, out: &mut impl Write) -> io::Result<()> {
    let width = report.checks.iter().map(|c| c.name.len()).max().unwrap_or(0);
    let mark_width = marks.ok.chars().count().max(marks.fail.chars().count());
    writeln!(out)?;
    for check in &report.checks {
        let raw = match check.status {
            Status::Ok => marks.ok,
            Status::Warn => marks.warn,
            Status::Fail => marks.fail,
        };
        let padded = format!("{raw:<mark_width$}");
        let mark = match check.status {
            Status::Ok => padded.if_supports_color(Stream::Stdout, |t| t.green()).to_string(),
            Status::Warn => padded.if_supports_color(Stream::Stdout, |t| t.yellow()).to_string(),
            Status::Fail => padded.if_supports_color(Stream::Stdout, |t| t.red()).to_string(),
        };
        writeln!(out, "  {mark} {:<width$}  {}", check.name, check.detail)?;
        if let Some(fix) = &check.fix {
            let indent = mark_width + 1;
            writeln!(
                out,
                "  {:indent$}{:<width$}  {} {fix}",
                "",
                "",
                "fix:".if_supports_color(Stream::Stdout, |t| t.dimmed())
            )?;
        }
    }
    writeln!(out)?;
    if !report.ok {
        writeln!(out, "  Not ready: fix the items marked {} and run `shipfile doctor` again.", marks.fail)?;
    } else if report.checks.iter().any(|c| c.status == Status::Warn) {
        writeln!(
            out,
            "  Ready for local work. Items marked {} are optional for now; see their fix lines.",
            marks.warn
        )?;
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
        render(&Report::new(checks), Marks::UNICODE, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn docker_states() {
        assert_eq!(check_docker(&Probe::Ran("29.7.2".into())).status, Status::Ok);
        assert_eq!(check_docker(&Probe::NotFound).status, Status::Fail);
        let down = check_docker(&Probe::Failed);
        assert_eq!(down.status, Status::Fail);
        assert!(down.detail.contains("daemon"));
        assert_eq!(check_docker(&Probe::TimedOut).status, Status::Fail);
        assert_eq!(check_compose(&Probe::TimedOut).status, Status::Fail);
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
        assert_eq!(check_aws_credentials(&env, None, None).status, Status::Warn);
        assert_eq!(check_aws_region(&env, None).status, Status::Warn);
    }

    #[test]
    fn credentials_sources() {
        let none = env_of(&[]);
        let env = env_of(&[("AWS_ACCESS_KEY_ID", "AKIAEXAMPLE")]);
        let check = check_aws_credentials(&env, None, None);
        assert_eq!(check.status, Status::Ok);
        assert!(!check.detail.contains("AKIA"), "never print key material");
        let creds = "[default]\naws_access_key_id = AKIAEXAMPLE\n";
        assert!(check_aws_credentials(&none, None, Some(creds)).detail.contains("credentials file"));
        let container = env_of(&[("AWS_CONTAINER_CREDENTIALS_RELATIVE_URI", "/v2/creds")]);
        assert!(check_aws_credentials(&container, None, None).detail.contains("container"));
        let process = "[default]\ncredential_process = /bin/creds\n";
        assert!(check_aws_credentials(&none, Some(process), None).detail.contains("credential_process"));
        let login = "[default]
login_session = arn:aws:iam::1:user/me
";
        assert!(check_aws_credentials(&none, Some(login), None).detail.contains("aws login"));
    }

    #[test]
    fn credentials_only_from_active_profile() {
        // SSO on `dev` must not count when the active profile is `default`.
        let config = "[default]\nregion = eu-west-1\n[profile dev]\nsso_session = corp\n";
        assert_eq!(check_aws_credentials(&env_of(&[]), Some(config), None).status, Status::Warn);
        let dev = env_of(&[("AWS_PROFILE", "dev")]);
        assert!(check_aws_credentials(&dev, Some(config), None).detail.contains("SSO"));
    }

    #[test]
    fn unknown_profile_is_reported() {
        let env = env_of(&[("AWS_PROFILE", "typo")]);
        let check = check_aws_credentials(&env, Some("[default]\nregion = x\n"), None);
        assert_eq!(check.status, Status::Warn);
        assert!(check.detail.contains("no such profile"));
    }

    #[test]
    fn extra_checks() {
        assert_eq!(check_buildx(&Probe::Ran("github.com/docker/buildx v0.20.0 abc".into())).detail, "v0.20.0");
        assert_eq!(check_buildx(&Probe::NotFound).status, Status::Warn);
        assert_eq!(check_git(&Probe::Ran("git version 2.50.1".into())).detail, "2.50.1");
        assert_eq!(check_ports(&[]).status, Status::Ok);
        assert!(check_ports(&[5432]).detail.contains("5432"));
        assert_eq!(check_disk(Some(1)).status, Status::Warn);
        assert_eq!(check_disk(Some(MIN_FREE_BYTES)).status, Status::Ok);
        assert_eq!(check_session_manager(&Probe::NotFound).status, Status::Warn);
    }

    #[test]
    fn json_shape_is_stable() {
        // Golden test for docs/json-output.md: a change here is a `schema_version` question.
        let report = Report::new(vec![
            check_docker(&Probe::NotFound),
            check_aws_region(&env_of(&[("AWS_REGION", "eu-west-1")]), None),
        ]);
        let mut json = serde_json::to_value(&report).unwrap();
        json["version"] = "[version]".into();
        insta::assert_json_snapshot!(json);
    }

    #[test]
    fn ascii_render() {
        let mut out = Vec::new();
        render(&Report::new(vec![check_docker(&Probe::NotFound)]), Marks::ASCII, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.is_ascii(), "{text}");
        assert!(text.contains("[x]  docker") && text.contains("marked [x]"));
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
