# Security policy

Shipfile handles AWS credentials and deploys infrastructure, so security reports are welcome and taken seriously.

## Supported versions

| Version | Supported |
|---|---|
| Latest `0.x` release | Yes |
| Older releases | No; please upgrade |

From `1.0.0` the latest minor release of the current major version gets security fixes.

## Reporting a vulnerability

Please **do not** open a public issue. Use GitHub's private vulnerability reporting instead:
[Report a vulnerability](https://github.com/jainprateek0603/shipfile/security/advisories/new).

Include the version (`shipfile --version`), your OS, steps to reproduce and the impact you expect.
You should get a reply within 7 days. Once a fix is released, the advisory is published with credit, unless you ask not to be named.

## Scope

In scope: the `shipfile` binary, the templates and workflows it generates, the plugin protocol, and the release artifacts.
Out of scope: vulnerabilities in AWS, Docker or third-party plugins themselves.
