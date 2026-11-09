//! Diagnostics: stable error codes, source positions, rendering (human and JSON), the JSON
//! pointer → YAML position map, and "did you mean" suggestions.

use std::collections::HashMap;
use std::fmt;

use codespan_reporting::diagnostic as cs;
use codespan_reporting::files::SimpleFile;
use codespan_reporting::term::{self, termcolor};
use serde::Serialize;

use crate::parse::{Node, Span, Value};

/// Stable diagnostic codes. Never renumber; retire codes instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Code {
    /// YAML syntax error.
    E001,
    /// The same key appears twice in a mapping.
    E002,
    /// Unknown field.
    E003,
    /// Required field missing.
    E004,
    /// Value has the wrong type.
    E005,
    /// Value is not one of the allowed values.
    E006,
    /// Number out of range.
    E007,
    /// Other schema violation.
    E008,
    /// Name is not DNS-safe.
    E010,
    /// Name used twice across services, queues and databases.
    E011,
    /// `uses` names something that is not a queue or database.
    E012,
    /// `uses` names a service.
    E013,
    /// Same entry listed twice.
    E014,
    /// Two things want the same local port.
    E015,
    /// Public service without a `port`.
    E016,
    /// Public service without a `health` path.
    E017,
    /// `health` must be an absolute path.
    E018,
    /// DynamoDB table without a `key`.
    E019,
    /// Field does not apply to this database engine.
    E020,
    /// Environment override names something that does not exist.
    E021,
    /// `-e/--env` names an environment that is not declared.
    E022,
    /// Invalid environment-variable or secret name.
    E023,
    /// A variable is defined twice for one service.
    E024,
    /// Plugin listed twice.
    E025,
    /// Region is not a valid AWS region name.
    E026,
    /// Queue or database that no service uses.
    W001,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::E001 => "E001",
            Self::E002 => "E002",
            Self::E003 => "E003",
            Self::E004 => "E004",
            Self::E005 => "E005",
            Self::E006 => "E006",
            Self::E007 => "E007",
            Self::E008 => "E008",
            Self::E010 => "E010",
            Self::E011 => "E011",
            Self::E012 => "E012",
            Self::E013 => "E013",
            Self::E014 => "E014",
            Self::E015 => "E015",
            Self::E016 => "E016",
            Self::E017 => "E017",
            Self::E018 => "E018",
            Self::E019 => "E019",
            Self::E020 => "E020",
            Self::E021 => "E021",
            Self::E022 => "E022",
            Self::E023 => "E023",
            Self::E024 => "E024",
            Self::E025 => "E025",
            Self::E026 => "E026",
            Self::W001 => "W001",
        }
    }

    pub fn severity(self) -> Severity {
        if matches!(self, Self::W001) { Severity::Warning } else { Severity::Error }
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for Code {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

/// A label pointing at part of the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub span: Span,
    pub message: String,
}

/// One problem found in a spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: Code,
    pub message: String,
    /// Where the problem is. `None` when it is not tied to one place.
    pub primary: Option<Label>,
    /// Related places, such as the first definition of a duplicate.
    pub secondary: Vec<Label>,
    /// What to do next, including "did you mean" suggestions.
    pub help: Vec<String>,
}

impl Diagnostic {
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), primary: None, secondary: Vec::new(), help: Vec::new() }
    }

    #[must_use]
    pub fn at(mut self, span: Option<Span>, label: impl Into<String>) -> Self {
        self.primary = span.map(|span| Label { span, message: label.into() });
        self
    }

    #[must_use]
    pub fn also(mut self, span: Option<Span>, label: impl Into<String>) -> Self {
        if let Some(span) = span {
            self.secondary.push(Label { span, message: label.into() });
        }
        self
    }

    #[must_use]
    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help.push(help.into());
        self
    }

    /// Adds a "did you mean" help line when a close candidate exists.
    #[must_use]
    pub fn suggest<'a>(self, input: &str, candidates: impl IntoIterator<Item = &'a str>) -> Self {
        match did_you_mean(input, candidates) {
            Some(best) => self.help(format!("did you mean `{best}`?")),
            None => self,
        }
    }

    pub fn severity(&self) -> Severity {
        self.code.severity()
    }

    pub fn is_error(&self) -> bool {
        self.severity() == Severity::Error
    }

    fn sort_key(&self) -> (usize, usize, Code) {
        self.primary.as_ref().map_or((0, 0, self.code), |l| (l.span.start, l.span.end, self.code))
    }
}

/// Sorts diagnostics by position and removes exact duplicates.
pub fn normalize(diagnostics: &mut Vec<Diagnostic>) {
    diagnostics.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()).then_with(|| a.message.cmp(&b.message)));
    diagnostics.dedup();
}

/// The closest candidate by edit distance, if it is close enough to be a likely typo.
pub fn did_you_mean<'a>(input: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let input_lower = input.to_ascii_lowercase();
    candidates
        .into_iter()
        .filter(|c| *c != input)
        .map(|c| (strsim::damerau_levenshtein(&input_lower, &c.to_ascii_lowercase()), c))
        .filter(|(d, c)| *d <= (input.len().max(c.len()) / 3).max(1))
        .min_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)))
        .map(|(_, c)| c)
}

// ---------------------------------------------------------------------------------------------
// JSON pointer → YAML position

/// Maps JSON pointers (RFC 6901) to the YAML positions of keys and values.
#[derive(Debug, Clone, Default)]
pub struct SourceMap {
    entries: HashMap<String, Position>,
}

#[derive(Debug, Clone, Copy)]
struct Position {
    key: Option<Span>,
    value: Span,
}

impl SourceMap {
    pub fn new(root: &Node) -> Self {
        let mut map = Self::default();
        map.walk(root, String::new(), None);
        map
    }

    fn walk(&mut self, node: &Node, pointer: String, key: Option<Span>) {
        match &node.value {
            Value::Seq(items) => {
                for (i, item) in items.iter().enumerate() {
                    self.walk(item, format!("{pointer}/{i}"), None);
                }
            }
            Value::Map(entries) => {
                for entry in entries {
                    let child = format!("{pointer}/{}", escape(&entry.key));
                    // Keep the first occurrence of a duplicate key (that is what E002 points at).
                    if !self.entries.contains_key(&child) {
                        self.walk(&entry.value, child, Some(entry.key_span));
                    }
                }
            }
            _ => {}
        }
        self.entries.entry(pointer).or_insert(Position { key, value: node.span });
    }

    /// Span of the value at `pointer`.
    pub fn value(&self, pointer: &str) -> Option<Span> {
        self.entries.get(pointer).map(|p| p.value)
    }

    /// Span of the key at `pointer`, falling back to the value.
    pub fn key(&self, pointer: &str) -> Option<Span> {
        self.entries.get(pointer).map(|p| p.key.unwrap_or(p.value))
    }

    /// The closest known position for `pointer`: itself, or its nearest ancestor's key.
    pub fn nearest_key(&self, pointer: &str) -> Option<Span> {
        let mut p = pointer;
        loop {
            if let Some(span) = self.key(p) {
                return Some(span);
            }
            p = &p[..p.rfind('/')?];
        }
    }

    /// The closest known position for `pointer`'s value, or its nearest ancestor's key.
    pub fn nearest_value(&self, pointer: &str) -> Option<Span> {
        self.value(pointer).or_else(|| self.nearest_key(pointer))
    }
}

/// Builds a JSON pointer from segments.
pub fn pointer<I, S>(segments: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    segments.into_iter().fold(String::new(), |mut acc, s| {
        acc.push('/');
        acc.push_str(&escape(s.as_ref()));
        acc
    })
}

fn escape(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

/// Turns a JSON pointer into a readable path such as `services.api.scale.cpu`.
pub fn dotted(pointer: &str) -> String {
    if pointer.is_empty() {
        return "the top level".to_owned();
    }
    pointer.trim_start_matches('/').split('/').map(|s| s.replace("~1", "/").replace("~0", "~")).collect::<Vec<_>>().join(".")
}

// ---------------------------------------------------------------------------------------------
// Rendering

/// Renders diagnostics in the PLAN §4.3 style.
pub fn render(file: &str, source: &str, diagnostics: &[Diagnostic], color: bool) -> String {
    let files = SimpleFile::new(file, source);
    let config = term::Config::default();
    let mut buffer = if color { termcolor::Buffer::ansi() } else { termcolor::Buffer::no_color() };
    for d in diagnostics {
        let severity = match d.severity() {
            Severity::Error => cs::Severity::Error,
            Severity::Warning => cs::Severity::Warning,
        };
        let mut labels = Vec::new();
        if let Some(l) = &d.primary {
            labels.push(cs::Label::primary((), l.span.start..l.span.end).with_message(&l.message));
        }
        for l in &d.secondary {
            labels.push(cs::Label::secondary((), l.span.start..l.span.end).with_message(&l.message));
        }
        let mut notes: Vec<String> = Vec::new();
        if d.primary.is_none() {
            notes.push(format!("in {file}"));
        }
        notes.extend(d.help.iter().map(|h| format!("help: {h}")));
        let diag = cs::Diagnostic::new(severity)
            .with_code(d.code.as_str())
            .with_message(&d.message)
            .with_labels(labels)
            .with_notes(notes);
        // Writing to an in-memory buffer only fails if a span is out of bounds, which would be a bug.
        let _ = term::emit_to_write_style(&mut buffer, &config, &files, &diag);
    }
    String::from_utf8_lossy(buffer.as_slice()).replace("\r\n", "\n")
}

/// The `--json` form of one diagnostic.
#[derive(Debug, Clone, Serialize)]
pub struct JsonDiagnostic<'a> {
    pub code: Code,
    pub severity: Severity,
    pub message: &'a str,
    pub file: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<&'a str>,
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    pub help: &'a [String],
}

impl Diagnostic {
    pub fn to_json<'a>(&'a self, file: &'a str) -> JsonDiagnostic<'a> {
        JsonDiagnostic {
            code: self.code,
            severity: self.severity(),
            message: &self.message,
            file,
            line: self.primary.as_ref().map(|l| l.span.line),
            column: self.primary.as_ref().map(|l| l.span.column),
            label: self.primary.as_ref().map(|l| l.message.as_str()),
            help: &self.help,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;

    #[test]
    fn suggestions_need_to_be_close() {
        assert_eq!(did_you_mean("orders-qeue", ["main-db", "orders-queue"]), Some("orders-queue"));
        assert_eq!(did_you_mean("replcas", ["cpu", "memory", "replicas"]), Some("replicas"));
        assert_eq!(did_you_mean("zzz", ["cpu", "memory", "replicas"]), None);
        assert_eq!(did_you_mean("Postgres", ["postgres", "dynamodb"]), Some("postgres"));
    }

    #[test]
    fn pointers_map_to_positions() {
        let src = "services:\n  api:\n    uses: [a, b]\n  a/b: {}\n";
        let map = SourceMap::new(&parse(src).unwrap());
        let span = map.value("/services/api/uses/1").unwrap();
        assert_eq!((span.line, span.column), (3, 15));
        let span = map.key("/services/api").unwrap();
        assert_eq!((span.line, span.column), (2, 3));
        assert!(map.key(&pointer(["services", "a/b"])).is_some());
        assert_eq!(map.nearest_key("/services/api/nope/deeper").unwrap().line, 2);
        assert_eq!(dotted("/services/api/scale"), "services.api.scale");
    }

    #[test]
    fn renders_like_the_plan() {
        let src = "services:\n  api:\n    uses: [orders-qeue, main-db]\n";
        let map = SourceMap::new(&parse(src).unwrap());
        let d = Diagnostic::new(Code::E012, "unknown resource `orders-qeue` in `uses`")
            .at(map.value("/services/api/uses/0"), "not defined under `queues` or `databases`")
            .suggest("orders-qeue", ["orders-queue"]);
        let out = render("shipfile.yaml", src, &[d], false);
        assert!(out.contains("error[E012]: unknown resource `orders-qeue` in `uses`"), "{out}");
        assert!(out.contains("shipfile.yaml:3:12"), "{out}");
        assert!(out.contains("= help: did you mean `orders-queue`?"), "{out}");
    }
}
