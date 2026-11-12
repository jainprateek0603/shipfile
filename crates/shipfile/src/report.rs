//! Diagnostics rendering: core diagnostics become `miette` diagnostics, drawn by a
//! `miette::ReportHandler` in the style of `docs/PLAN.md` §4.3:
//!
//! ```text
//! error[E012]: unknown resource `orders-qeue` in `uses`
//!   ┌─ shipfile.yaml:14:12
//!   │
//! 14│     uses: [orders-qeue, main-db]
//!   │            ^^^^^^^^^^^ not defined under `queues` or `databases`
//!   │
//!   = help: did you mean `orders-queue`?
//! ```

use std::fmt::{self, Write as _};
use std::sync::Arc;

use miette::{Diagnostic, LabeledSpan, NamedSource, ReportHandler, Severity, SourceCode};
use owo_colors::OwoColorize;
use serde::Serialize;
use shipfile_core::diagnostics::{self, Severity as CoreSeverity};

/// A core diagnostic plus the file it points into.
#[derive(Debug)]
pub(crate) struct SpecDiagnostic {
    diag: shipfile_core::Diagnostic,
    source: Arc<NamedSource<String>>,
}

impl SpecDiagnostic {
    pub(crate) fn new(diag: shipfile_core::Diagnostic, source: Arc<NamedSource<String>>) -> Self {
        Self { diag, source }
    }
}

impl fmt::Display for SpecDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.diag.message)
    }
}

impl std::error::Error for SpecDiagnostic {}

impl Diagnostic for SpecDiagnostic {
    fn code<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        Some(Box::new(self.diag.code))
    }

    fn severity(&self) -> Option<Severity> {
        Some(match self.diag.severity {
            CoreSeverity::Error => Severity::Error,
            CoreSeverity::Warning => Severity::Warning,
        })
    }

    fn help<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        self.diag.help.as_ref().map(|h| Box::new(h) as Box<dyn fmt::Display>)
    }

    fn source_code(&self) -> Option<&dyn SourceCode> {
        Some(&*self.source)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        Some(Box::new(
            self.diag.labels.iter().map(|l| LabeledSpan::new(Some(l.message.clone()), l.span.offset, l.span.len)),
        ))
    }
}

/// Draws diagnostics in the §4.3 style. Colour is decided once, up front.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Handler {
    pub(crate) color: bool,
}

impl ReportHandler for Handler {
    fn debug(&self, diagnostic: &dyn Diagnostic, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            return fmt::Debug::fmt(diagnostic, f);
        }
        self.render(diagnostic, f)
    }
}

impl Handler {
    fn paint(self, text: &str, style: fn(&str) -> String) -> String {
        if self.color { style(text) } else { text.to_owned() }
    }

    pub(crate) fn render(self, diagnostic: &dyn Diagnostic, f: &mut impl fmt::Write) -> fmt::Result {
        let warning = diagnostic.severity() == Some(Severity::Warning);
        let level = if warning { "warning" } else { "error" };
        let code = diagnostic.code().map(|c| format!("[{c}]")).unwrap_or_default();
        let head = format!("{level}{code}");
        let head = if warning {
            self.paint(&head, |t| t.yellow().bold().to_string())
        } else {
            self.paint(&head, |t| t.red().bold().to_string())
        };
        writeln!(f, "{head}: {}", self.paint(&diagnostic.to_string(), |t| t.bold().to_string()))?;

        let snippet = diagnostic
            .labels()
            .and_then(|mut labels| labels.next())
            .zip(diagnostic.source_code())
            .and_then(|(label, source)| Snippet::read(&label, source));
        let width = snippet.as_ref().map_or(2, |s| s.line.to_string().len().max(2));
        let pad = " ".repeat(width);
        let bar = self.paint("│", |t| t.blue().to_string());
        if let Some(s) = &snippet {
            let arrow = self.paint("┌─", |t| t.blue().to_string());
            let name = s.name.as_deref().unwrap_or("<spec>");
            writeln!(f, "{pad}{arrow} {name}:{}:{}", s.line, s.column)?;
            writeln!(f, "{pad}{bar}")?;
            let number = self.paint(&format!("{:>width$}", s.line), |t| t.blue().to_string());
            writeln!(f, "{number}{bar} {}", s.text)?;
            let carets = self.paint(&"^".repeat(s.len), |t| t.red().bold().to_string());
            let indent = " ".repeat(s.column - 1);
            match &s.label {
                Some(label) if !label.is_empty() => writeln!(f, "{pad}{bar} {indent}{carets} {label}")?,
                _ => writeln!(f, "{pad}{bar} {indent}{carets}")?,
            }
            writeln!(f, "{pad}{bar}")?;
        }
        if let Some(help) = diagnostic.help() {
            writeln!(f, "{pad}= {} {help}", self.paint("help:", |t| t.bold().to_string()))?;
        }
        Ok(())
    }

    /// The diagnostic as text, ending with a newline.
    pub(crate) fn to_text(self, diagnostic: &dyn Diagnostic) -> String {
        let mut out = String::new();
        // Writing into a String can't fail.
        let _ = self.render(diagnostic, &mut out);
        out
    }
}

/// The source line under a label.
struct Snippet {
    name: Option<String>,
    /// 1-based.
    line: usize,
    /// 1-based, in characters.
    column: usize,
    text: String,
    /// Underline length in characters, at least 1, cut at the end of the line.
    len: usize,
    label: Option<String>,
}

impl Snippet {
    fn read(label: &LabeledSpan, source: &dyn SourceCode) -> Option<Self> {
        // One line of context either side makes miette return whole lines; then find the
        // line that holds the label's start.
        let contents = source.read_span(label.inner(), 1, 1).ok()?;
        let data = std::str::from_utf8(contents.data()).ok()?;
        let mut start = label.offset().checked_sub(contents.span().offset())?;
        let mut line = contents.line();
        let mut text = "";
        for candidate in data.split_inclusive('\n') {
            if start < candidate.len() || !candidate.ends_with('\n') {
                text = candidate;
                break;
            }
            start -= candidate.len();
            line += 1;
        }
        let text = text.trim_end_matches(['\n', '\r']);
        let before = text.get(..start.min(text.len()))?;
        let column = before.chars().count() + 1;
        let rest_of_line = text.get(before.len()..).unwrap_or("");
        let spanned = rest_of_line.get(..label.len().min(rest_of_line.len())).map_or(1, |s| s.chars().count());
        let len = spanned.clamp(1, rest_of_line.chars().count().max(1));
        Some(Self {
            name: contents.name().map(str::to_owned),
            line: line + 1,
            column,
            text: text.to_owned(),
            len,
            label: label.label().map(str::to_owned),
        })
    }
}

/// A diagnostic in `--json` output.
#[derive(Debug, Serialize)]
pub(crate) struct JsonDiagnostic {
    pub(crate) code: String,
    pub(crate) severity: CoreSeverity,
    pub(crate) message: String,
    pub(crate) file: String,
    pub(crate) line: Option<usize>,
    pub(crate) column: Option<usize>,
    pub(crate) label: Option<String>,
    pub(crate) help: Option<String>,
}

impl JsonDiagnostic {
    pub(crate) fn new(diag: &shipfile_core::Diagnostic, file: &str, source: &str) -> Self {
        let position = diag.primary_span().map(|s| diagnostics::line_col(source, s.offset));
        Self {
            code: diag.code.to_string(),
            severity: diag.severity,
            message: diag.message.clone(),
            file: file.to_owned(),
            line: position.map(|p| p.0),
            column: position.map(|p| p.1),
            label: diag.labels.first().map(|l| l.message.clone()),
            help: diag.help.clone(),
        }
    }
}

/// Renders every diagnostic of a failed spec, separated by blank lines.
pub(crate) fn render_failed(failed: &shipfile_core::Failed, file: &str, handler: Handler) -> String {
    let source = Arc::new(NamedSource::new(file, failed.text.clone()));
    let mut out = String::new();
    for diag in &failed.diagnostics.items {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&handler.to_text(&SpecDiagnostic::new(diag.clone(), Arc::clone(&source))));
    }
    let count = failed.diagnostics.errors().count();
    let noun = if count == 1 { "error" } else { "errors" };
    let _ = write!(out, "\n{}: {file} has {count} {noun}\n", handler.paint("error", |t| t.red().bold().to_string()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_the_plan_example() {
        let source = "services:\n  api:\n    build: .\n    uses: [orders-qeue, main-db]\n";
        let failed = shipfile_core::load(
            &format!("version: 1\nname: orders\n{source}queues:\n  orders-queue:\ndatabases:\n  main-db: {{ engine: postgres }}\n"),
            None,
            &shipfile_core::MemProject::new(),
        )
        .unwrap_err();
        let text = render_failed(&failed, "shipfile.yaml", Handler { color: false });
        let expected = "\
error[E012]: unknown resource `orders-qeue` in `uses`
  ┌─ shipfile.yaml:6:12
  │
 6│     uses: [orders-qeue, main-db]
  │            ^^^^^^^^^^^ not defined under `queues` or `databases`
  │
  = help: did you mean `orders-queue`?
";
        assert!(text.starts_with(expected), "{text}");
    }
}
