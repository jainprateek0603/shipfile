//! Positioned problems found in a spec, with stable error codes.
//!
//! This crate only produces plain data. The binary renders it with `miette`.

use serde::Serialize;

use crate::parse::Span;

/// Stable error codes. Never renumber or reuse one; add new codes at the end of a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Code {
    // Parsing.
    Syntax,
    DuplicateKey,
    // Schema.
    UnknownField,
    WrongType,
    MissingField,
    InvalidValue,
    UnsupportedVersion,
    // Names and references.
    InvalidName,
    DuplicateName,
    UnknownUse,
    DuplicateEntry,
    // Service and resource rules.
    PortClash,
    PublicNeedsPort,
    PublicNeedsHealth,
    DynamodbNeedsKey,
    FieldNotForEngine,
    InvalidScale,
    // Environments.
    UnknownEnvironment,
    OverlayUnknownResource,
    EnvVarCollision,
    // Project files.
    RuntimeNotDetected,
    BuildDirMissing,
}

impl Code {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Syntax => "E001",
            Self::DuplicateKey => "E002",
            Self::UnknownField => "E003",
            Self::WrongType => "E004",
            Self::MissingField => "E005",
            Self::InvalidValue => "E006",
            Self::UnsupportedVersion => "E007",
            Self::InvalidName => "E010",
            Self::DuplicateName => "E011",
            Self::UnknownUse => "E012",
            Self::DuplicateEntry => "E013",
            Self::PortClash => "E014",
            Self::PublicNeedsPort => "E015",
            Self::PublicNeedsHealth => "E016",
            Self::DynamodbNeedsKey => "E017",
            Self::FieldNotForEngine => "E018",
            Self::InvalidScale => "E019",
            Self::UnknownEnvironment => "E020",
            Self::OverlayUnknownResource => "E021",
            Self::EnvVarCollision => "E022",
            Self::RuntimeNotDetected => "E030",
            Self::BuildDirMissing => "E031",
        }
    }
}

impl std::fmt::Display for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Label {
    pub span: Span,
    pub message: String,
}

/// One problem: what went wrong, where, and what to do next.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[error("{message}")]
pub struct Diagnostic {
    pub code: Code,
    pub severity: Severity,
    pub message: String,
    /// The first label is the primary location.
    pub labels: Vec<Label>,
    pub help: Option<String>,
}

impl Diagnostic {
    pub fn error(code: Code, message: impl Into<String>) -> Self {
        Self { code, severity: Severity::Error, message: message.into(), labels: Vec::new(), help: None }
    }

    #[must_use]
    pub fn with_label(mut self, span: Span, message: impl Into<String>) -> Self {
        self.labels.push(Label { span, message: message.into() });
        self
    }

    #[must_use]
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Adds a label when a span is known.
    #[must_use]
    pub fn at(self, span: Option<Span>, message: impl Into<String>) -> Self {
        match span {
            Some(span) => self.with_label(span, message),
            None => self,
        }
    }

    pub fn primary_span(&self) -> Option<Span> {
        self.labels.first().map(|l| l.span)
    }
}

/// Every problem found in one spec, sorted by position so output is deterministic.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the spec has {} error(s)", self.errors().count())]
pub struct Diagnostics {
    pub items: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn new(mut items: Vec<Diagnostic>) -> Self {
        items.sort_by(|a, b| {
            (a.primary_span().map(|s| s.offset), a.code, &a.message).cmp(&(
                b.primary_span().map(|s| s.offset),
                b.code,
                &b.message,
            ))
        });
        items.dedup();
        Self { items }
    }

    pub fn errors(&self) -> impl Iterator<Item = &Diagnostic> {
        self.items.iter().filter(|d| d.severity == Severity::Error)
    }

    pub fn has_errors(&self) -> bool {
        self.errors().next().is_some()
    }
}

impl From<Diagnostic> for Diagnostics {
    fn from(d: Diagnostic) -> Self {
        Self::new(vec![d])
    }
}

/// 1-based line and character column of a byte offset.
pub fn line_col(source: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(source.len());
    let before = &source[..offset];
    let line = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let column = source[line_start..offset].chars().count() + 1;
    (line, column)
}

/// The closest candidate by edit distance, if it is close enough to be a likely typo.
pub fn suggest<'a, I>(input: &str, candidates: I) -> Option<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    candidates
        .into_iter()
        .filter(|c| *c != input)
        .map(|c| (strsim::damerau_levenshtein(input, c), c))
        .filter(|(distance, c)| *distance <= (input.chars().count().max(c.chars().count()) / 3).max(1))
        .min_by_key(|(distance, c)| (*distance, *c))
        .map(|(_, c)| c)
}

/// `help: did you mean `x`?` text for a typo, if there is a close candidate.
pub fn did_you_mean<'a, I>(input: &str, candidates: I) -> Option<String>
where
    I: IntoIterator<Item = &'a str>,
{
    suggest(input, candidates).map(|s| format!("did you mean `{s}`?"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions_catch_typos() {
        let names = ["orders-queue", "main-db", "sessions"];
        assert_eq!(suggest("orders-qeue", names), Some("orders-queue"));
        assert_eq!(suggest("main_db", names), Some("main-db"));
        assert_eq!(suggest("sesions", names), Some("sessions"));
        assert_eq!(suggest("billing", names), None);
        assert_eq!(suggest("x", names), None);
    }

    #[test]
    fn suggestions_are_deterministic_on_ties() {
        assert_eq!(suggest("ab", ["ac", "aa"]), Some("aa"));
    }

    #[test]
    fn short_inputs_allow_one_edit() {
        assert_eq!(suggest("prt", ["port", "public"]), Some("port"));
    }

    #[test]
    fn line_col_counts_chars() {
        let src = "a: 1\nnäme: x\n";
        assert_eq!(line_col(src, 0), (1, 1));
        assert_eq!(line_col(src, 5), (2, 1));
        let x = src.find('x').unwrap();
        assert_eq!(line_col(src, x), (2, 7));
    }

    #[test]
    fn codes_are_stable() {
        assert_eq!(Code::Syntax.as_str(), "E001");
        assert_eq!(Code::UnknownUse.as_str(), "E012");
        assert_eq!(Code::BuildDirMissing.as_str(), "E031");
    }
}
