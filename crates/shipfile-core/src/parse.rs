//! YAML parsing that keeps the position of every key and value.
//!
//! This is the only module that knows about `serde-saphyr`, so the YAML library can be swapped
//! without touching the rest of the crate (see ADR-0007). It turns the source into a [`Node`]
//! tree where every key and value carries a byte [`Span`]. The rest of the pipeline works on
//! JSON (`to_json`) and asks the tree for positions through JSON-pointer-like paths.

use std::fmt;

use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_saphyr::Spanned;

use crate::diagnostics::{Code, Diagnostic, line_col};

/// A byte range in the (LF-normalised) source text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
pub struct Span {
    pub offset: usize,
    pub len: usize,
}

impl Span {
    pub const fn new(offset: usize, len: usize) -> Self {
        Self { offset, len }
    }
}

/// A parsed YAML value with its position.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub value: Value,
    pub span: Span,
}

/// A mapping key with its position.
#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    String(String),
    Seq(Vec<Node>),
    /// Entries in source order. Duplicate keys are rejected while parsing.
    Map(Vec<(Key, Node)>),
}

/// A parsed spec file: the normalised source text plus the positioned tree.
#[derive(Debug, Clone)]
pub struct Document {
    /// The source with CRLF line endings turned into LF. All spans index into this text.
    pub source: String,
    pub root: Node,
}

impl Document {
    /// Parses YAML text. CRLF input is normalised to LF first, so spans, lines and columns
    /// are the same whichever line endings the file was saved with.
    pub fn parse(source: &str) -> Result<Self, Box<Diagnostic>> {
        let source = crate::normalize_source(source);
        if source.trim().is_empty() {
            return Err(Box::new(
                Diagnostic::error(Code::Syntax, "the spec file is empty")
                    .with_help("run `shipfile init` to write a starter shipfile.yaml"),
            ));
        }
        // Duplicates are passed through and reported by `check_duplicates`, which knows both keys.
        let options = serde_saphyr::options! { duplicate_keys: serde_saphyr::DuplicateKeyPolicy::PassThrough };
        match serde_saphyr::from_str_with_options::<SpannedNode>(&source, options) {
            Ok(root) => {
                let root = root.0;
                check_duplicates(&source, &root)?;
                Ok(Self { source, root })
            }
            Err(err) => Err(Box::new(syntax_error(&source, &err))),
        }
    }

    /// The node at `path`, where each segment is a map key or a sequence index.
    pub fn get(&self, path: &[&str]) -> Option<&Node> {
        let mut node = &self.root;
        for segment in path {
            node = match &node.value {
                Value::Map(entries) => &entries.iter().find(|(k, _)| k.name == *segment)?.1,
                Value::Seq(items) => items.get(segment.parse::<usize>().ok()?)?,
                _ => return None,
            };
        }
        Some(node)
    }

    /// The span of the map key that ends `path`.
    pub fn key_span(&self, path: &[&str]) -> Option<Span> {
        let (last, parent) = path.split_last()?;
        match &self.get(parent)?.value {
            Value::Map(entries) => entries.iter().find(|(k, _)| k.name == *last).map(|(k, _)| k.span),
            _ => None,
        }
    }

    /// The best span to point at for `path`: the value itself for scalars, the key for
    /// maps and sequences (whose own span may cover many lines). Falls back to the closest
    /// ancestor that exists.
    pub fn span(&self, path: &[&str]) -> Option<Span> {
        for len in (0..=path.len()).rev() {
            let prefix = &path[..len];
            let Some(node) = self.get(prefix) else { continue };
            let is_scalar = !matches!(node.value, Value::Map(_) | Value::Seq(_));
            if is_scalar && node.span.len > 0 {
                return Some(node.span);
            }
            if let Some(key) = self.key_span(prefix) {
                return Some(key);
            }
            if node.span.len > 0 {
                return Some(node.span);
            }
        }
        None
    }

    /// The tree as plain JSON, for schema validation and typed deserialisation.
    pub fn to_json(&self) -> serde_json::Value {
        to_json(&self.root)
    }
}

fn to_json(node: &Node) -> serde_json::Value {
    use serde_json::Value as J;
    match &node.value {
        Value::Null => J::Null,
        Value::Bool(b) => J::Bool(*b),
        Value::Int(i) => J::from(*i),
        Value::UInt(u) => J::from(*u),
        Value::Float(f) => serde_json::Number::from_f64(*f).map_or(J::Null, J::Number),
        Value::String(s) => J::String(s.clone()),
        Value::Seq(items) => J::Array(items.iter().map(to_json).collect()),
        Value::Map(entries) => J::Object(entries.iter().map(|(k, v)| (k.name.clone(), to_json(v))).collect()),
    }
}

fn check_duplicates(source: &str, node: &Node) -> Result<(), Box<Diagnostic>> {
    match &node.value {
        Value::Map(entries) => {
            for (i, (key, value)) in entries.iter().enumerate() {
                if let Some((first, _)) = entries[..i].iter().find(|(k, _)| k.name == key.name) {
                    return Err(Box::new(
                        Diagnostic::error(Code::DuplicateKey, format!("duplicate key `{}`", key.name))
                            .with_label(key.span, "defined again here")
                            .with_help(format!(
                                "`{}` is first defined on line {}; remove one of them",
                                key.name,
                                line_col(source, first.span.offset).0
                            )),
                    ));
                }
                check_duplicates(source, value)?;
            }
            Ok(())
        }
        Value::Seq(items) => items.iter().try_for_each(|item| check_duplicates(source, item)),
        _ => Ok(()),
    }
}

fn syntax_error(source: &str, err: &serde_saphyr::Error) -> Diagnostic {
    let message = first_line(&err.to_string());
    let mut diag = Diagnostic::error(Code::Syntax, format!("invalid YAML: {message}"));
    if let Some(loc) = err.location()
        && loc.line() > 0
    {
        let offset = match loc.span().byte_offset() {
            Some(offset) => usize::try_from(offset).unwrap_or(0),
            None => offset_of(source, loc.line(), loc.column()),
        };
        diag = diag.with_label(Span::new(offset.min(source.len()), 1), "here");
    }
    diag.with_help("fix the YAML syntax; a YAML linter or your editor's YAML support can help")
}

/// Strips serde-saphyr's own position suffix and snippet so only the reason is left.
fn first_line(message: &str) -> String {
    let line = message.lines().next().unwrap_or(message).trim();
    let line = line.split(" at line ").next().unwrap_or(line);
    line.trim_end_matches('.').to_owned()
}

/// Byte offset of a 1-based (line, character column) position.
fn offset_of(source: &str, line: u64, column: u64) -> usize {
    let skip = |n: u64| usize::try_from(n.saturating_sub(1)).unwrap_or(usize::MAX);
    let line_start: usize = source.split_inclusive('\n').take(skip(line)).map(str::len).sum();
    let rest = &source[line_start.min(source.len())..];
    let col_bytes: usize = rest.chars().take(skip(column)).map(char::len_utf8).sum();
    line_start + col_bytes
}

fn span_of(loc: &serde_saphyr::Location) -> Span {
    let span = loc.span();
    match (span.byte_offset(), span.byte_len()) {
        (Some(offset), Some(len)) => Span::new(usize::try_from(offset).unwrap_or(0), usize::try_from(len).unwrap_or(0)),
        _ => Span::default(),
    }
}

/// `Spanned<NodeValue>` turned into a [`Node`].
struct SpannedNode(Node);

impl<'de> Deserialize<'de> for SpannedNode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let spanned = Spanned::<RawValue>::deserialize(deserializer)?;
        Ok(Self(Node { value: spanned.value.0, span: span_of(&spanned.referenced) }))
    }
}

struct RawValue(Value);

impl<'de> Deserialize<'de> for RawValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ValueVisitor).map(RawValue)
    }
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("any YAML value")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Int(v))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Value, E> {
        Ok(Value::UInt(v))
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Value, E> {
        Ok(Value::Float(v))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.to_owned()))
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut items = Vec::new();
        while let Some(SpannedNode(node)) = seq.next_element()? {
            items.push(node);
        }
        Ok(Value::Seq(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut entries = Vec::new();
        while let Some(key) = map.next_key::<Spanned<String>>()? {
            let SpannedNode(value) = map.next_value()?;
            entries.push((Key { name: key.value, span: span_of(&key.referenced) }, value));
        }
        Ok(Value::Map(entries))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(doc: &Document, span: Span) -> &str {
        &doc.source[span.offset..span.offset + span.len]
    }

    const SPEC: &str =
        "version: 1\nname: orders\nservices:\n  api:\n    port: 3000\n    uses: [orders-queue, main-db]\n";

    #[test]
    fn keeps_value_and_key_spans() {
        let doc = Document::parse(SPEC).unwrap();
        let span = doc.span(&["services", "api", "uses", "1"]).unwrap();
        assert_eq!(text(&doc, span), "main-db");
        let key = doc.key_span(&["services", "api"]).unwrap();
        assert_eq!(text(&doc, key), "api");
        assert_eq!(text(&doc, doc.span(&["services", "api", "port"]).unwrap()), "3000");
    }

    #[test]
    fn crlf_gives_the_same_spans() {
        let lf = Document::parse(SPEC).unwrap();
        let crlf = Document::parse(&SPEC.replace('\n', "\r\n")).unwrap();
        let path = ["services", "api", "uses", "0"];
        assert_eq!(lf.span(&path), crlf.span(&path));
        assert_eq!(lf.source, crlf.source);
    }

    #[test]
    fn missing_paths_fall_back_to_the_closest_ancestor() {
        let doc = Document::parse(SPEC).unwrap();
        let span = doc.span(&["services", "api", "health"]).unwrap();
        assert_eq!(text(&doc, span), "api");
    }

    #[test]
    fn to_json_round_trips_scalars() {
        let doc = Document::parse("a: 1\nb: -2\nc: 1.5\nd: true\ne: ~\nf: x\n").unwrap();
        assert_eq!(doc.to_json(), serde_json::json!({"a": 1, "b": -2, "c": 1.5, "d": true, "e": null, "f": "x"}));
    }

    #[test]
    fn duplicate_keys_are_errors() {
        let err = Document::parse("a: 1\nb: 2\na: 3\n").unwrap_err();
        assert_eq!(err.code, Code::DuplicateKey);
        assert_eq!(err.labels[0].span, Span::new(10, 1));
    }

    #[test]
    fn syntax_errors_have_a_position() {
        let err = Document::parse("a: [1, 2\nb: 3\n").unwrap_err();
        assert_eq!(err.code, Code::Syntax);
        assert_ne!(err.labels.len(), 0);
    }

    #[test]
    fn non_ascii_offsets_are_bytes() {
        let doc = Document::parse("name: \"ünï\"\nport: 1\n").unwrap();
        assert_eq!(text(&doc, doc.span(&["port"]).unwrap()), "1");
    }
}
