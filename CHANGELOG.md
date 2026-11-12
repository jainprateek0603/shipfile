# Changelog

All notable changes to this project are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- `doctor` checks docker buildx, git, local ports, disk space and the Session Manager plugin.
- `doctor` detects AWS credentials per active profile, including `aws login`, `credential_process`, assumed roles and container credentials.
- `--ascii` flag, with automatic ASCII marks on consoles without Unicode.
- `schema_version` in `--json` output.
- `-v`/`-vv`/`-q` logging to stderr, with `SHIPFILE_LOG`.
- `shipfile completions <shell>`.
- A readable crash report that saves the backtrace to a file.

### Fixed

- Usage errors exit with `1`, so they can't be mistaken for "plan has changes" (`2`).
- `doctor` no longer hangs when Docker doesn't answer; each probe times out after 5 s.

## [0.0.1] - 2026-11-12

### Added

- Workspace, `shipfile doctor`, CI and release setup.
