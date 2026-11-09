//! Spec types, parsing, validation, environment overlay and the resolved Model for Shipfile.
//!
//! The pipeline (PLAN §6.1): [`parse`] the YAML keeping positions → validate against the
//! generated [`schema`] → semantic [`check`]s → [`Model::resolve`] (defaults, env overlay,
//! runtime detection) → [`Model::contract`] per service.

pub mod check;
pub mod contract;
pub mod diagnostics;
pub mod graph;
pub mod model;
pub mod parse;
pub mod schema;
pub mod spec;

pub use check::{Checked, check};
pub use contract::EnvContract;
pub use diagnostics::{Code, Diagnostic, Severity};
pub use model::Model;
pub use spec::Spec;

/// Checks a spec and, if it has no errors, resolves it. Warnings are returned alongside the model.
pub fn load(
    source: &str,
    env: Option<&str>,
    detect: &dyn Fn(&str) -> Option<spec::Runtime>,
) -> Result<(Model, Vec<Diagnostic>), Vec<Diagnostic>> {
    let checked = check(source, env);
    match checked.spec {
        Some(spec) if !checked.has_errors() => Ok((Model::resolve(&spec, env, detect), checked.diagnostics)),
        _ => Err(checked.diagnostics),
    }
}
