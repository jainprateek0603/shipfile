//! JSON Schema generated from the spec types (ADR-0005), and validation against it.

use std::sync::OnceLock;

use jsonschema::error::ValidationErrorKind;
use serde_json::Value as Json;

use crate::diagnostics::{Code, Diagnostic, SourceMap, did_you_mean, dotted};
use crate::spec::Spec;

/// Where editors find the published schema.
pub const SCHEMA_ID: &str = "https://shipfile.dev/schema/v1.json";

/// The schema as JSON.
pub fn schema() -> &'static Json {
    static SCHEMA: OnceLock<Json> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        let settings = schemars::generate::SchemaSettings::draft2020_12();
        let schema = settings.into_generator().into_root_schema_for::<Spec>();
        let mut json = schema.to_value();
        strip_null(&mut json);
        if let Json::Object(obj) = &mut json {
            obj.insert("$id".to_owned(), Json::String(SCHEMA_ID.to_owned()));
        }
        json
    })
}

/// The schema file contents: pretty JSON, LF line endings, trailing newline.
pub fn schema_file() -> String {
    let mut out = serde_json::to_string_pretty(schema()).unwrap_or_default();
    out.push('\n');
    out
}

/// `Option<T>` fields are optional, not nullable: drop the `null` alternatives schemars adds.
fn strip_null(value: &mut Json) {
    match value {
        Json::Object(obj) => {
            if let Some(Json::Array(types)) = obj.get_mut("type") {
                types.retain(|t| t != "null");
                if types.len() == 1 {
                    let only = types.remove(0);
                    obj.insert("type".to_owned(), only);
                }
            }
            for key in ["anyOf", "oneOf"] {
                if let Some(Json::Array(options)) = obj.get_mut(key) {
                    options.retain(|o| o.get("type").is_none_or(|t| t != "null"));
                    if options.len() == 1 {
                        let only = options.remove(0);
                        obj.remove(key);
                        if let Json::Object(inner) = only {
                            for (k, v) in inner {
                                obj.entry(k).or_insert(v);
                            }
                        }
                    }
                }
            }
            if let Some(Json::Array(options)) = obj.get_mut("enum") {
                options.retain(|o| !o.is_null());
            }
            obj.values_mut().for_each(strip_null);
        }
        Json::Array(items) => items.iter_mut().for_each(strip_null),
        _ => {}
    }
}

fn validator() -> &'static jsonschema::Validator {
    static VALIDATOR: OnceLock<jsonschema::Validator> = OnceLock::new();
    VALIDATOR.get_or_init(|| {
        jsonschema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .build(schema())
            .unwrap_or_else(|e| unreachable!("generated schema is valid: {e}"))
    })
}

/// Validates `instance` against the schema and maps each error to a positioned diagnostic.
pub fn validate(instance: &Json, map: &SourceMap) -> Vec<Diagnostic> {
    let root = schema();
    validator()
        .iter_errors(instance)
        .flat_map(|err| {
            let ptr = err.instance_path().as_str().to_owned();
            to_diagnostics(root, &ptr, err.kind(), err.instance(), map, &err.to_string())
        })
        .collect()
}

fn to_diagnostics(
    root: &Json,
    ptr: &str,
    kind: &ValidationErrorKind,
    instance: &Json,
    map: &SourceMap,
    fallback: &str,
) -> Vec<Diagnostic> {
    let sub = subschema_at(root, ptr);
    let field = dotted(ptr);
    match kind {
        ValidationErrorKind::AdditionalProperties { unexpected } => unexpected
            .iter()
            .map(|name| {
                let allowed = property_names(root, sub);
                let child = format!("{ptr}/{}", name.replace('~', "~0").replace('/', "~1"));
                let mut d = Diagnostic::new(Code::E003, format!("unknown field `{name}` in {}", where_(&field)))
                    .at(map.key(&child), "not a known field");
                if let Some(best) = did_you_mean(name, allowed.iter().map(String::as_str)) {
                    d = d.help(format!("did you mean `{best}`?"));
                } else if !allowed.is_empty() {
                    d = d.help(format!("known fields: {}", allowed.join(", ")));
                }
                d
            })
            .collect(),
        ValidationErrorKind::Required { property } => {
            let name = property.as_str().unwrap_or_default();
            vec![
                Diagnostic::new(Code::E004, format!("missing required field `{name}` in {}", where_(&field)))
                    .at(map.nearest_key(ptr), format!("`{name}` is required here")),
            ]
        }
        ValidationErrorKind::Type { .. } => {
            let expected = sub.and_then(|s| s.get("type")).map_or_else(|| "another type".to_owned(), type_words);
            vec![
                Diagnostic::new(Code::E005, format!("`{field}` must be {expected}"))
                    .at(map.nearest_value(ptr), format!("this is {}", json_type_words(instance))),
            ]
        }
        ValidationErrorKind::Enum { options } => {
            let options: Vec<String> =
                options.as_array().map(|o| o.iter().map(scalar_text).collect()).unwrap_or_default();
            let got = scalar_text(instance);
            let mut d = Diagnostic::new(Code::E006, format!("`{got}` is not a valid value for `{field}`"))
                .at(map.nearest_value(ptr), "not allowed here");
            if let Some(best) = did_you_mean(&got, options.iter().map(String::as_str)) {
                d = d.help(format!("did you mean `{best}`?"));
            }
            vec![d.help(format!("allowed values: {}", options.join(", ")))]
        }
        ValidationErrorKind::Constant { expected_value } => vec![
            Diagnostic::new(Code::E006, format!("`{}` is not a valid value for `{field}`", scalar_text(instance)))
                .at(map.nearest_value(ptr), format!("must be `{}`", scalar_text(expected_value))),
        ],
        ValidationErrorKind::Minimum { limit } | ValidationErrorKind::ExclusiveMinimum { limit } => vec![
            Diagnostic::new(Code::E007, format!("`{field}` is too small"))
                .at(map.nearest_value(ptr), format!("must be at least {limit}")),
        ],
        ValidationErrorKind::Maximum { limit } | ValidationErrorKind::ExclusiveMaximum { limit } => vec![
            Diagnostic::new(Code::E007, format!("`{field}` is too large"))
                .at(map.nearest_value(ptr), format!("must be at most {limit}")),
        ],
        _ => vec![Diagnostic::new(Code::E008, format!("`{field}`: {fallback}")).at(map.nearest_value(ptr), "here")],
    }
}

fn where_(field: &str) -> String {
    if field == "the top level" { field.to_owned() } else { format!("`{field}`") }
}

fn resolve<'a>(root: &'a Json, mut schema: &'a Json) -> &'a Json {
    for _ in 0..16 {
        match schema.get("$ref").and_then(Json::as_str) {
            Some(r) => match root.pointer(r.trim_start_matches('#')) {
                Some(target) => schema = target,
                None => break,
            },
            None => break,
        }
    }
    schema
}

/// Walks the schema along an instance pointer.
fn subschema_at<'a>(root: &'a Json, ptr: &str) -> Option<&'a Json> {
    let mut schema = resolve(root, root);
    for seg in ptr.split('/').skip(1) {
        let seg = seg.replace("~1", "/").replace("~0", "~");
        let next = schema
            .get("properties")
            .and_then(|p| p.get(&seg))
            .or_else(|| schema.get("additionalProperties").filter(|a| a.is_object()))
            .or_else(|| schema.get("items").filter(|a| a.is_object()))?;
        schema = resolve(root, next);
    }
    Some(schema)
}

fn property_names(root: &Json, schema: Option<&Json>) -> Vec<String> {
    schema
        .map(|s| resolve(root, s))
        .and_then(|s| s.get("properties"))
        .and_then(Json::as_object)
        .map(|p| p.keys().cloned().collect())
        .unwrap_or_default()
}

fn type_words(t: &Json) -> String {
    let one = |t: &str| match t {
        "object" => "a mapping",
        "array" => "a list",
        "integer" => "a whole number",
        "number" => "a number",
        "string" => "a string",
        "boolean" => "`true` or `false`",
        "null" => "empty",
        _ => "a value",
    };
    match t {
        Json::String(s) => one(s).to_owned(),
        Json::Array(ts) => ts.iter().filter_map(Json::as_str).map(one).collect::<Vec<_>>().join(" or "),
        _ => "another type".to_owned(),
    }
}

fn json_type_words(v: &Json) -> &'static str {
    match v {
        Json::Null => "empty",
        Json::Bool(_) => "a boolean",
        Json::Number(n) if n.is_i64() || n.is_u64() => "a whole number",
        Json::Number(_) => "a number",
        Json::String(_) => "a string",
        Json::Array(_) => "a list",
        Json::Object(_) => "a mapping",
    }
}

fn scalar_text(v: &Json) -> String {
    match v {
        Json::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_has_no_nullable_options() {
        let text = schema_file();
        assert!(!text.contains("\"null\""), "Option fields must not be nullable");
        assert!(text.contains("\"$id\": \"https://shipfile.dev/schema/v1.json\""));
    }

    #[test]
    fn committed_schema_is_up_to_date() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../schema/shipfile.v1.json");
        let committed = std::fs::read_to_string(path).unwrap_or_default().replace("\r\n", "\n");
        assert!(committed == schema_file(), "schema/shipfile.v1.json is stale: run `cargo xtask schema`");
    }
}
