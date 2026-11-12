//! Spec types, parsing, validation, environment overlay and the resolved Model for Shipfile.
//!
//! The pipeline (`docs/PLAN.md` §5.3, §6.1) is pure: [`load`] turns the spec text, the chosen
//! environment and the project files into a [`Model`], or into positioned [`Diagnostics`].
//!
//! 1. [`parse`]: YAML with the position of every key and value.
//! 2. [`schema`]: JSON Schema generated from [`spec::RawSpec`].
//! 3. [`validate`]: semantic checks, with "did you mean" suggestions.
//! 4. [`resolve`]: environment overlay, defaults and runtime detection.

pub mod contract;
pub mod diagnostics;
pub mod model;
pub mod name;
pub mod overlay;
pub mod parse;
pub mod project;
pub mod resolve;
pub mod schema;
pub mod spec;
pub mod validate;

pub use contract::EnvContract;
pub use diagnostics::{Diagnostic, Diagnostics};
pub use model::Model;
pub use name::Name;
pub use parse::Document;
pub use project::{DirProject, MemProject, Project};
pub use spec::RawSpec;

/// A spec that passed every check.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub doc: Document,
    pub spec: RawSpec,
    pub model: Model,
}

/// A spec that failed: the diagnostics, plus the normalised source they point into.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{diagnostics}")]
pub struct Failed {
    pub text: String,
    pub diagnostics: Diagnostics,
}

/// The source text the way spans see it: LF line endings and no byte-order mark.
pub fn normalize_source(source: &str) -> String {
    let source = source.replace("\r\n", "\n");
    source.strip_prefix('\u{feff}').map(str::to_owned).unwrap_or(source)
}

/// Runs the whole pipeline on one spec.
pub fn load(source: &str, env: Option<&str>, project: &dyn Project) -> Result<Loaded, Box<Failed>> {
    let fail = |source: String, items: Vec<Diagnostic>| {
        Box::new(Failed { text: source, diagnostics: Diagnostics::new(items) })
    };
    let doc = Document::parse(source).map_err(|d| fail(normalize_source(source), vec![*d]))?;
    let errors = schema::validate(&doc);
    if !errors.is_empty() {
        return Err(fail(doc.source, errors));
    }
    let spec: RawSpec = match serde_json::from_value(doc.to_json()) {
        Ok(spec) => spec,
        Err(e) => {
            let diag =
                Diagnostic::error(diagnostics::Code::WrongType, format!("the spec doesn't match its schema: {e}"));
            return Err(fail(doc.source, vec![diag]));
        }
    };
    let errors = validate::validate(&doc, &spec);
    if !errors.is_empty() {
        return Err(fail(doc.source, errors));
    }
    match resolve::resolve(&doc, &spec, env, project) {
        Ok(model) => Ok(Loaded { doc, spec, model }),
        Err(errors) => Err(fail(doc.source, errors)),
    }
}
