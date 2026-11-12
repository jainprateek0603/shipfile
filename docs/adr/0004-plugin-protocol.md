# ADR-0004: Plugins are external programs that talk JSON over stdin/stdout

**Status:** accepted · November 2026

## Context
Teams need to add their own conventions (tags, extra files, other languages' bindings) without forking. Rust has no stable plugin ABI, and dynamic loading is fragile across platforms. WASM would work but makes writing a plugin harder.

## Decision
A plugin is any executable named `shipfile-gen-<name>`, found on `PATH` or through `path:` in the spec, the same way `protoc` plugins work. Shipfile writes a versioned request (`protocol`, `env`, `model`, `options`) to stdin and reads `files`, `template_patch` (RFC 6902) and `diagnostics` from stdout.

## Consequences
- Plugins can be written in any language. The example plugins are in Rust and Python.
- Safety rules: plugins run only when listed in the spec, have a 30-second timeout, and may write files only inside the project. The plan names every plugin that patched the template.
- Plugins don't inherit Shipfile's environment: the process starts with `env_clear()` and gets back only `PATH`, `HOME`/`USERPROFILE`, `SYSTEMROOT`, `TEMP`/`TMP`, `LANG` and `SHIPFILE_*`. AWS credentials never reach a plugin; everything it needs comes on stdin.
- Changes to the protocol need a version bump. Shipfile refuses plugins that speak a protocol version it doesn't know.
