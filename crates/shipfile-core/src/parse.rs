//! Positioned YAML parsing.
//!
//! This is the only module that knows about the YAML library (`serde-saphyr`), so it
//! can be swapped without touching the rest of the crate. It turns the source into a
//! [`Node`] tree where every key and value carries its [`Span`].

use std::fmt;

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_saphyr::Spanned;

/// A location in the source file.
///
/// `start`/`end` are byte offsets; `line` and `column` are 1-based (column counts characters).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
}

impl Span {
    /// Builds a span for `len` bytes starting at byte `start`, computing line and column.
    pub fn at(source: &str, start: usize, len: usize) -> Self {
        let start = start.min(source.len());
        let before = &source[..start];
        let line = before.matches('\n').count() + 1;
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        let column = source[line_start..start].chars().count() + 1;
        Self { start, end: (start + len).min(source.len()), line, column }
    }
}

/// A parsed YAML value with its position.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub span: Span,
    pub value: Value,
}

/// The shape of a YAML value. Mappings keep their source order, including duplicate keys.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Seq(Vec<Node>),
    Map(Vec<Entry>),
}

/// One `key: value` pair of a mapping.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub key: String,
    pub key_span: Span,
    pub value: Node,
}

/// A YAML syntax error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxError {
    pub message: String,
    pub span: Span,
}

impl Node {
    /// Looks up a key in a mapping node.
    pub fn get(&self, key: &str) -> Option<&Entry> {
        match &self.value {
            Value::Map(entries) => entries.iter().rev().find(|e| e.key == key),
            _ => None,
        }
    }

    /// Converts the tree to JSON. For duplicate keys the last one wins, as in most YAML loaders;
    /// duplicates are reported separately.
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::Value as J;
        match &self.value {
            Value::Null => J::Null,
            Value::Bool(b) => J::Bool(*b),
            Value::Int(i) => J::from(*i),
            Value::Float(f) => serde_json::Number::from_f64(*f).map_or(J::Null, J::Number),
            Value::Str(s) => J::String(s.clone()),
            Value::Seq(items) => J::Array(items.iter().map(Node::to_json).collect()),
            Value::Map(entries) => J::Object(entries.iter().map(|e| (e.key.clone(), e.value.to_json())).collect()),
        }
    }

    /// Short name of the value's type, for messages.
    pub fn type_name(&self) -> &'static str {
        match self.value {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Int(_) | Value::Float(_) => "number",
            Value::Str(_) => "string",
            Value::Seq(_) => "list",
            Value::Map(_) => "mapping",
        }
    }
}

/// Parses one YAML document.
pub fn parse(source: &str) -> Result<Node, SyntaxError> {
    if source.trim().is_empty() {
        return Ok(Node { span: Span::at(source, 0, 0), value: Value::Null });
    }
    let options = serde_saphyr::options! {
        duplicate_keys: serde_saphyr::options::DuplicateKeyPolicy::PassThrough,
    };
    let raw: Spanned<RawValue> = serde_saphyr::from_str_with_options(source, options).map_err(|err| {
        let span = err.location().map_or_else(|| Span::at(source, 0, 0), |loc| span_of(source, loc));
        let message = err.without_snippet().to_string();
        let message = message.lines().next().unwrap_or("invalid YAML").trim().to_owned();
        SyntaxError { message, span }
    })?;
    Ok(build(source, raw))
}

fn span_of(source: &str, loc: serde_saphyr::Location) -> Span {
    let span = loc.span();
    if let Some(start) = span.byte_offset().and_then(|o| usize::try_from(o).ok()) {
        let len = span.byte_len().and_then(|l| usize::try_from(l).ok()).unwrap_or(0);
        return Span::at(source, start, len);
    }
    // Fall back to line/column.
    let line = usize::try_from(loc.line()).unwrap_or(1).max(1);
    let column = usize::try_from(loc.column()).unwrap_or(1).max(1);
    let line_start: usize = source.split_inclusive('\n').take(line - 1).map(str::len).sum();
    let col_bytes: usize = source[line_start..].chars().take(column - 1).map(char::len_utf8).sum();
    Span::at(source, line_start + col_bytes, 0)
}

/// Trims a span to the token on its first line, so a label on a scalar never spills over.
fn tidy(source: &str, mut span: Span) -> Span {
    let text = &source[span.start..span.end];
    if let Some(nl) = text.find('\n') {
        span.end = span.start + text[..nl].trim_end().len();
    }
    if span.end == span.start {
        // Zero-width: widen to the token under the cursor.
        let rest = &source[span.start..];
        let len = rest.find(|c: char| c.is_whitespace() || matches!(c, ',' | ']' | '}')).unwrap_or(rest.len());
        span.end = span.start + len;
    }
    span
}

fn build(source: &str, raw: Spanned<RawValue>) -> Node {
    let span = tidy(source, span_of(source, raw.referenced));
    let value = match raw.value {
        RawValue::Null => Value::Null,
        RawValue::Bool(b) => Value::Bool(b),
        RawValue::Int(i) => Value::Int(i),
        RawValue::Float(f) => Value::Float(f),
        RawValue::Str(s) => Value::Str(s),
        RawValue::Seq(items) => Value::Seq(items.into_iter().map(|i| build(source, i)).collect()),
        RawValue::Map(entries) => Value::Map(
            entries
                .into_iter()
                .map(|(k, v)| Entry {
                    key: k.value.0,
                    key_span: tidy(source, span_of(source, k.referenced)),
                    value: build(source, v),
                })
                .collect(),
        ),
    };
    Node { span, value }
}

enum RawValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Seq(Vec<Spanned<RawValue>>),
    Map(Vec<(Spanned<Key>, Spanned<RawValue>)>),
}

impl<'de> Deserialize<'de> for RawValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(RawVisitor)
    }
}

struct RawVisitor;

impl<'de> Visitor<'de> for RawVisitor {
    type Value = RawValue;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any YAML value")
    }

    fn visit_unit<E>(self) -> Result<RawValue, E> {
        Ok(RawValue::Null)
    }
    fn visit_none<E>(self) -> Result<RawValue, E> {
        Ok(RawValue::Null)
    }
    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<RawValue, D::Error> {
        RawValue::deserialize(d)
    }
    fn visit_bool<E>(self, v: bool) -> Result<RawValue, E> {
        Ok(RawValue::Bool(v))
    }
    fn visit_i64<E>(self, v: i64) -> Result<RawValue, E> {
        Ok(RawValue::Int(v))
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<RawValue, E> {
        i64::try_from(v).map(RawValue::Int).map_err(|_| E::custom("integer is too large"))
    }
    fn visit_i128<E: de::Error>(self, v: i128) -> Result<RawValue, E> {
        i64::try_from(v).map(RawValue::Int).map_err(|_| E::custom("integer is too large"))
    }
    fn visit_u128<E: de::Error>(self, v: u128) -> Result<RawValue, E> {
        i64::try_from(v).map(RawValue::Int).map_err(|_| E::custom("integer is too large"))
    }
    fn visit_f64<E>(self, v: f64) -> Result<RawValue, E> {
        Ok(RawValue::Float(v))
    }
    fn visit_str<E>(self, v: &str) -> Result<RawValue, E> {
        Ok(RawValue::Str(v.to_owned()))
    }
    fn visit_string<E>(self, v: String) -> Result<RawValue, E> {
        Ok(RawValue::Str(v))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<RawValue, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(RawValue::Seq(items))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<RawValue, A::Error> {
        let mut entries = Vec::new();
        while let Some(key) = map.next_key::<Spanned<Key>>()? {
            entries.push((key, map.next_value()?));
        }
        Ok(RawValue::Map(entries))
    }
}

/// A mapping key; scalar keys of any type are read as their text.
struct Key(String);

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct KeyVisitor;
        impl Visitor<'_> for KeyVisitor {
            type Value = Key;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a scalar mapping key")
            }
            fn visit_str<E>(self, v: &str) -> Result<Key, E> {
                Ok(Key(v.to_owned()))
            }
            fn visit_bool<E>(self, v: bool) -> Result<Key, E> {
                Ok(Key(v.to_string()))
            }
            fn visit_i64<E>(self, v: i64) -> Result<Key, E> {
                Ok(Key(v.to_string()))
            }
            fn visit_u64<E>(self, v: u64) -> Result<Key, E> {
                Ok(Key(v.to_string()))
            }
            fn visit_f64<E>(self, v: f64) -> Result<Key, E> {
                Ok(Key(v.to_string()))
            }
            fn visit_unit<E>(self) -> Result<Key, E> {
                Ok(Key("null".to_owned()))
            }
        }
        deserializer.deserialize_any(KeyVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_positions_of_keys_and_values() {
        let src = "name: orders\nservices:\n  api:\n    uses: [orders-qeue, main-db]\n";
        let root = parse(src).unwrap();
        let services = &root.get("services").unwrap().value;
        let api = services.get("api").unwrap();
        assert_eq!((api.key_span.line, api.key_span.column), (3, 3));
        let uses = api.value.get("uses").unwrap();
        let Value::Seq(items) = &uses.value.value else { panic!("seq") };
        assert_eq!((items[0].span.line, items[0].span.column), (4, 12));
        assert_eq!(&src[items[0].span.start..items[0].span.end], "orders-qeue");
        assert_eq!(&src[items[1].span.start..items[1].span.end], "main-db");
    }

    #[test]
    fn keeps_duplicate_keys() {
        let root = parse("a: 1\na: 2\n").unwrap();
        let Value::Map(entries) = &root.value else { panic!("map") };
        assert_eq!(entries.len(), 2);
        assert_eq!(root.to_json(), serde_json::json!({ "a": 2 }));
    }

    #[test]
    fn syntax_error_has_position() {
        let err = parse("a: [1, 2\nb: 3\n").unwrap_err();
        assert!(err.span.line >= 1, "{err:?}");
    }

    #[test]
    fn scalars() {
        let root = parse("a: true\nb: 3\nc: 1.5\nd: ~\ne: '3'\n").unwrap();
        assert_eq!(root.to_json(), serde_json::json!({ "a": true, "b": 3, "c": 1.5, "d": null, "e": "3" }));
    }
}
