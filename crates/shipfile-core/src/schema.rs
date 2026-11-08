//! The spec's JSON Schema, generated from [`crate::spec::RawSpec`] (ADR-0005), and validation
//! against it with errors mapped back to YAML positions.

use std::sync::OnceLock;

use jsonschema::error::{TypeKind, ValidationErrorKind};
use jsonschema::{ValidationError, Validator};
use serde_json::Value as Json;

use crate::diagnostics::{Code, Diagnostic, did_you_mean};
use crate::parse::Document;
use crate::spec::RawSpec;

/// The JSON Schema for `shipfile.yaml`, as committed in `schema/shipfile.v1.json`.
pub fn schema() -> Json {
    serde_json::to_value(schemars::schema_for!(RawSpec)).unwrap_or(Json::Null)
}

/// The schema as pretty JSON with LF line endings and a final newline.
pub fn schema_text() -> String {
    let mut text = serde_json::to_string_pretty(&schema()).unwrap_or_default();
    text.push('\n');
    text
}

fn compiled() -> &'static (Json, Validator) {
    static COMPILED: OnceLock<(Json, Validator)> = OnceLock::new();
    COMPILED.get_or_init(|| {
        let schema = schema();
        // The schema comes from our own types; failing to compile it is a bug caught by tests.
        let validator =
            jsonschema::validator_for(&schema).unwrap_or_else(|e| panic!("generated schema is invalid: {e}"));
        (schema, validator)
    })
}

/// Validates the document against the schema. Returns one diagnostic per problem.
pub fn validate(doc: &Document) -> Vec<Diagnostic> {
    let (schema, validator) = compiled();
    let instance = doc.to_json();
    let mut out = Vec::new();
    for error in validator.iter_errors(&instance) {
        explain(doc, schema, &error, &mut out);
    }
    // A value of the wrong type also fails its enum, pattern or range; only the type matters.
    let wrong_type: Vec<_> = out.iter().filter(|d| d.code == Code::WrongType).map(Diagnostic::primary_span).collect();
    out.retain(|d| d.code != Code::InvalidValue || !wrong_type.contains(&d.primary_span()));
    out
}

fn segments(location: &jsonschema::paths::Location) -> Vec<String> {
    location.as_str().split('/').skip(1).map(|s| s.replace("~1", "/").replace("~0", "~")).collect()
}

fn dotted(path: &[String]) -> String {
    if path.is_empty() { "the top level".to_owned() } else { format!("`{}`", path.join(".")) }
}

fn json_type(value: &Json) -> &'static str {
    match value {
        Json::Null => "nothing (null)",
        Json::Bool(_) => "a boolean",
        Json::Number(n) if n.is_f64() => "a decimal number",
        Json::Number(_) => "a number",
        Json::String(_) => "a string",
        Json::Array(_) => "a list",
        Json::Object(_) => "a mapping",
    }
}

fn type_name(kind: &TypeKind) -> String {
    let name = |t: &jsonschema::JsonType| match t.to_string().as_str() {
        "integer" => "a whole number",
        "number" => "a number",
        "string" => "a string",
        "boolean" => "`true` or `false`",
        "array" => "a list",
        "object" => "a mapping",
        _ => "nothing (null)",
    };
    match kind {
        TypeKind::Single(t) => name(t).to_owned(),
        TypeKind::Multiple(types) => {
            let names: Vec<_> = types.iter().filter(|t| t.to_string() != "null").map(|t| name(&t)).collect();
            names.join(" or ")
        }
    }
}

/// Field-specific help for pattern errors.
fn pattern_help(field: &str) -> &'static str {
    match field {
        "runtime" => "use one of `node22`, `python3.13`, `go1.23` or `rust` (other versions work the same way)",
        "health" => "the health path must start with `/`, for example `/healthz`",
        "region" => "use an AWS region name such as `ap-south-1` or `us-east-1`",
        _ => "check the value against the documentation",
    }
}

#[expect(clippy::too_many_lines, reason = "one arm per schema error kind")]
fn explain(doc: &Document, schema: &Json, error: &ValidationError<'_>, out: &mut Vec<Diagnostic>) {
    let path = segments(error.instance_path());
    let refs: Vec<&str> = path.iter().map(String::as_str).collect();
    let field = path.last().map_or("", String::as_str);
    let instance = error.instance();
    match error.kind() {
        ValidationErrorKind::AdditionalProperties { unexpected } => {
            let allowed = allowed_properties(schema, error.schema_path());
            for name in unexpected {
                let mut key_path = refs.clone();
                key_path.push(name);
                let help = did_you_mean(name, allowed.iter().map(String::as_str))
                    .unwrap_or_else(|| format!("allowed fields here: {}", backticked(&allowed)));
                out.push(
                    Diagnostic::error(Code::UnknownField, format!("unknown field `{name}` in {}", dotted(&path)))
                        .at(doc.key_span(&key_path), "not a known field")
                        .with_help(help),
                );
            }
        }
        ValidationErrorKind::Required { property } => {
            let name = property.as_str().unwrap_or_default();
            out.push(
                Diagnostic::error(Code::MissingField, format!("missing field `{name}` in {}", dotted(&path)))
                    .at(doc.span(&refs), format!("`{name}` is required here"))
                    .with_help(format!("add `{name}:` under {}", dotted(&path))),
            );
        }
        ValidationErrorKind::Type { kind } => {
            out.push(
                Diagnostic::error(
                    Code::WrongType,
                    format!("{} must be {}, found {}", dotted(&path), type_name(kind), json_type(instance)),
                )
                .at(doc.span(&refs), format!("expected {}", type_name(kind))),
            );
        }
        ValidationErrorKind::Enum { options } => {
            let options: Vec<String> = options
                .as_array()
                .into_iter()
                .flatten()
                .map(|o| o.as_str().map_or(o.to_string(), str::to_owned))
                .collect();
            let help = instance
                .as_str()
                .and_then(|v| did_you_mean(v, options.iter().map(String::as_str)))
                .unwrap_or_else(|| format!("use one of {}", backticked(&options)));
            out.push(
                Diagnostic::error(Code::InvalidValue, format!("invalid value {instance} for {}", dotted(&path)))
                    .at(doc.span(&refs), format!("not one of {}", backticked(&options)))
                    .with_help(help),
            );
        }
        ValidationErrorKind::Pattern { .. } => {
            out.push(
                Diagnostic::error(Code::InvalidValue, format!("invalid value {instance} for {}", dotted(&path)))
                    .at(doc.span(&refs), "not in the expected format")
                    .with_help(pattern_help(field)),
            );
        }
        ValidationErrorKind::Minimum { limit } | ValidationErrorKind::ExclusiveMinimum { limit } => {
            out.push(
                Diagnostic::error(Code::InvalidValue, format!("{} is too small", dotted(&path)))
                    .at(doc.span(&refs), format!("the minimum is {limit}")),
            );
        }
        ValidationErrorKind::Maximum { limit } | ValidationErrorKind::ExclusiveMaximum { limit } => {
            out.push(
                Diagnostic::error(Code::InvalidValue, format!("{} is too large", dotted(&path)))
                    .at(doc.span(&refs), format!("the maximum is {limit}")),
            );
        }
        ValidationErrorKind::AnyOf { context } | ValidationErrorKind::OneOfNotValid { context } => {
            // A branch whose errors are deeper than this value (or not about its type) is the one
            // the user meant, for example the mapping branch of an optional mapping.
            let depth = path.len();
            let meant = context.iter().find(|branch| {
                branch.iter().any(|e| {
                    segments(e.instance_path()).len() > depth || !matches!(e.kind(), ValidationErrorKind::Type { .. })
                })
            });
            if let Some(branch) = meant {
                for inner in branch {
                    explain(doc, schema, inner, out);
                }
            } else {
                let expected: Vec<String> = context
                    .iter()
                    .flatten()
                    .filter_map(|e| match e.kind() {
                        ValidationErrorKind::Type { kind } => Some(type_name(kind)),
                        _ => None,
                    })
                    .filter(|t| !t.starts_with("nothing"))
                    .collect();
                let expected = if expected.is_empty() { "a different value".to_owned() } else { expected.join(" or ") };
                out.push(
                    Diagnostic::error(
                        Code::WrongType,
                        format!("{} must be {expected}, found {}", dotted(&path), json_type(instance)),
                    )
                    .at(doc.span(&refs), format!("expected {expected}")),
                );
            }
        }
        other => {
            out.push(
                Diagnostic::error(Code::InvalidValue, format!("invalid value for {}: {other:?}", dotted(&path)))
                    .at(doc.span(&refs), "invalid value"),
            );
        }
    }
}

fn backticked(items: &[String]) -> String {
    items.iter().map(|i| format!("`{i}`")).collect::<Vec<_>>().join(", ")
}

/// The property names allowed next to the `additionalProperties` keyword that failed.
fn allowed_properties(schema: &Json, schema_path: &jsonschema::paths::Location) -> Vec<String> {
    let pointer = schema_path.as_str();
    let parent = pointer.strip_suffix("/additionalProperties").unwrap_or(pointer);
    let mut names: Vec<String> = schema
        .pointer(parent)
        .and_then(|s| s.get("properties"))
        .and_then(Json::as_object)
        .map(|props| props.keys().cloned().collect())
        .unwrap_or_default();
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn errors(yaml: &str) -> Vec<Diagnostic> {
        validate(&Document::parse(yaml).unwrap())
    }

    #[test]
    fn schema_compiles_and_accepts_a_minimal_spec() {
        assert_eq!(errors("version: 1\nname: x\n"), Vec::new());
    }

    #[test]
    fn unknown_fields_suggest_the_closest_name() {
        let errs = errors("version: 1\nname: x\nservices:\n  api:\n    build: .\n    prot: 3000\n");
        assert_eq!(errs.len(), 1, "{errs:#?}");
        assert_eq!(errs[0].code, Code::UnknownField);
        assert_eq!(errs[0].help.as_deref(), Some("did you mean `port`?"));
    }

    #[test]
    fn optional_mappings_report_the_inner_problem() {
        let errs = errors("version: 1\nname: x\nservices:\n  api:\n    build: .\n    scale: { cpu: lots }\n");
        assert_eq!(errs.len(), 1, "{errs:#?}");
        assert!(errs[0].message.contains("services.api.scale.cpu"), "{errs:#?}");
    }

    #[test]
    fn env_values_must_be_scalars() {
        let errs = errors("version: 1\nname: x\nservices:\n  api:\n    build: .\n    env: { A: [1] }\n");
        assert_eq!(errs.len(), 1, "{errs:#?}");
        assert_eq!(errs[0].code, Code::WrongType);
    }
}
