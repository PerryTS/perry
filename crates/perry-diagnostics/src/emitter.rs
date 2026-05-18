//! Diagnostic emitters for different output formats.

use crate::compat_report::{is_reportable_code, maybe_enqueue_for_diagnostic, ReportStage};
use crate::diagnostic::{Diagnostic, Diagnostics, Severity};
use crate::source_cache::SourceCache;
use crate::span::LabelStyle;
use std::io::Write;

/// #849 chokepoint: extract the raw snippet for `diagnostic` from the
/// `SourceCache` and forward it to the compat-report queue if the code
/// is in the targeted set AND a sink is installed.
///
/// All three emitters call this from their `emit` implementation. It's
/// the single place in the codebase where a diagnostic's source snippet
/// becomes a candidate report — keeps emission policy out of the
/// per-crate diagnostic-construction sites.
pub fn forward_to_compat_report_channel(diagnostic: &Diagnostic, cache: &SourceCache) {
    if !is_reportable_code(diagnostic.code) {
        return;
    }
    let span = diagnostic.span;
    if span.is_dummy() {
        return;
    }
    let Some(file) = cache.get_file(span.file_id) else {
        return;
    };
    // Pull the snippet (offset slice) from the file. Stay within bounds.
    let src = &file.source;
    let start = (span.start as usize).min(src.len());
    let end = (span.end as usize).min(src.len()).max(start);
    if start == end {
        return;
    }
    // Find a UTF-8 char boundary at or after `start` and at or before `end`.
    let start = (start..=end)
        .find(|&i| src.is_char_boundary(i))
        .unwrap_or(start);
    let end = (start..=end)
        .rev()
        .find(|&i| src.is_char_boundary(i))
        .unwrap_or(end);
    let snippet = &src[start..end];
    maybe_enqueue_for_diagnostic(diagnostic.code, snippet, ReportStage::HirLower, None, None);
}

/// Trait for emitting diagnostics in various formats.
pub trait DiagnosticEmitter {
    /// Emit a single diagnostic.
    fn emit(&mut self, diagnostic: &Diagnostic, cache: &SourceCache) -> std::io::Result<()>;

    /// Emit multiple diagnostics.
    fn emit_all(&mut self, diagnostics: &Diagnostics, cache: &SourceCache) -> std::io::Result<()> {
        for diag in diagnostics.iter() {
            self.emit(diag, cache)?;
        }
        Ok(())
    }

    /// Emit a summary line.
    fn emit_summary(&mut self, diagnostics: &Diagnostics) -> std::io::Result<()>;
}

/// Rich terminal output with colors and code snippets.
pub struct TerminalEmitter<W: Write> {
    writer: W,
    colored: bool,
}

impl<W: Write> TerminalEmitter<W> {
    /// Create a new terminal emitter.
    pub fn new(writer: W, colored: bool) -> Self {
        Self { writer, colored }
    }

    /// Get ANSI color code for severity.
    fn severity_color(&self, severity: Severity) -> &'static str {
        if !self.colored {
            return "";
        }
        match severity {
            Severity::Error => "\x1b[31m",   // Red
            Severity::Warning => "\x1b[33m", // Yellow
            Severity::Hint => "\x1b[34m",    // Blue
        }
    }

    /// Get ANSI reset code.
    fn reset(&self) -> &'static str {
        if self.colored {
            "\x1b[0m"
        } else {
            ""
        }
    }

    /// Get bold ANSI code.
    fn bold(&self) -> &'static str {
        if self.colored {
            "\x1b[1m"
        } else {
            ""
        }
    }

    /// Get cyan ANSI code (for line numbers).
    fn cyan(&self) -> &'static str {
        if self.colored {
            "\x1b[36m"
        } else {
            ""
        }
    }
}

impl<W: Write> DiagnosticEmitter for TerminalEmitter<W> {
    fn emit(&mut self, diagnostic: &Diagnostic, cache: &SourceCache) -> std::io::Result<()> {
        // #849: single chokepoint — enqueue a compatibility report when
        // the diagnostic code is in the targeted set. No-op when no
        // sink is installed.
        forward_to_compat_report_channel(diagnostic, cache);

        let color = self.severity_color(diagnostic.severity);
        let reset = self.reset();
        let bold = self.bold();
        let cyan = self.cyan();

        // Header: error[U001]: message
        writeln!(
            self.writer,
            "{}{}{}[{}]{}: {}",
            bold,
            color,
            diagnostic.severity.as_str(),
            diagnostic.code.as_str(),
            reset,
            diagnostic.message
        )?;

        // Location: --> file:line:column
        if let Some(loc) = cache.location(diagnostic.span) {
            writeln!(
                self.writer,
                "  {}-->{} {}:{}:{}",
                cyan, reset, loc.file, loc.line, loc.column
            )?;

            // Code snippet
            if let Some(file) = cache.get_file(diagnostic.span.file_id) {
                let (line_num, start_col) = file.line_column(diagnostic.span.start);
                if let Some(line_text) = file.line_text(line_num) {
                    let line_str = format!("{}", line_num);
                    let padding = " ".repeat(line_str.len());

                    writeln!(self.writer, "{} {}|{}", padding, cyan, reset)?;
                    writeln!(self.writer, "{}{} |{} {}", cyan, line_str, reset, line_text)?;

                    // Underline
                    let underline_padding = " ".repeat((start_col - 1) as usize);
                    let span_len = diagnostic.span.len().max(1) as usize;
                    // Cap the underline length to not exceed the line
                    let max_underline = line_text.len().saturating_sub((start_col - 1) as usize);
                    let underline_len = span_len.min(max_underline).max(1);
                    let underline = "^".repeat(underline_len);

                    writeln!(
                        self.writer,
                        "{} {}|{} {}{}{}{}",
                        padding, cyan, reset, underline_padding, color, underline, reset
                    )?;
                }
            }
        }

        // Additional labels
        for label in &diagnostic.labels {
            if let Some(loc) = cache.location(label.span) {
                let label_color = match label.style {
                    LabelStyle::Primary => color,
                    LabelStyle::Secondary => self.cyan(),
                };
                writeln!(
                    self.writer,
                    "  {}note{}: {} ({}:{}:{})",
                    label_color, reset, label.message, loc.file, loc.line, loc.column
                )?;
            }
        }

        // Help text
        if let Some(ref explanation) = diagnostic.explanation {
            writeln!(self.writer, "  {}= help:{} {}", cyan, reset, explanation)?;
        }

        // Suggestions
        for suggestion in &diagnostic.suggestions {
            writeln!(
                self.writer,
                "  {}= suggestion:{} {}",
                cyan, reset, suggestion.message
            )?;
            if !suggestion.replacement.is_empty() {
                writeln!(
                    self.writer,
                    "                  replace with: `{}`",
                    suggestion.replacement
                )?;
            }
        }

        writeln!(self.writer)?;
        Ok(())
    }

    fn emit_summary(&mut self, diagnostics: &Diagnostics) -> std::io::Result<()> {
        let errors = diagnostics.error_count();
        let warnings = diagnostics.warning_count();

        let color = if errors > 0 {
            self.severity_color(Severity::Error)
        } else if warnings > 0 {
            self.severity_color(Severity::Warning)
        } else {
            ""
        };
        let reset = self.reset();

        if errors > 0 || warnings > 0 {
            write!(self.writer, "{}", color)?;
            if errors > 0 {
                write!(
                    self.writer,
                    "{} error{}",
                    errors,
                    if errors == 1 { "" } else { "s" }
                )?;
            }
            if errors > 0 && warnings > 0 {
                write!(self.writer, " and ")?;
            }
            if warnings > 0 {
                write!(
                    self.writer,
                    "{} warning{}",
                    warnings,
                    if warnings == 1 { "" } else { "s" }
                )?;
            }
            writeln!(self.writer, " emitted{}", reset)?;
        }

        Ok(())
    }
}

/// JSON output for tooling integration.
pub struct JsonEmitter<W: Write> {
    writer: W,
}

impl<W: Write> JsonEmitter<W> {
    /// Create a new JSON emitter.
    pub fn new(writer: W) -> Self {
        Self { writer }
    }
}

impl<W: Write> DiagnosticEmitter for JsonEmitter<W> {
    fn emit(&mut self, diagnostic: &Diagnostic, cache: &SourceCache) -> std::io::Result<()> {
        // #849 chokepoint — see TerminalEmitter::emit comment.
        forward_to_compat_report_channel(diagnostic, cache);

        let loc = cache.location(diagnostic.span);

        let json = serde_json::json!({
            "code": diagnostic.code.as_str(),
            "severity": diagnostic.severity.as_str(),
            "message": diagnostic.message,
            "location": loc.map(|l| serde_json::json!({
                "file": l.file,
                "line": l.line,
                "column": l.column,
            })),
            "span": if diagnostic.span.is_dummy() {
                serde_json::Value::Null
            } else {
                serde_json::json!({
                    "start": diagnostic.span.start,
                    "end": diagnostic.span.end,
                })
            },
            "help": diagnostic.explanation,
            "suggestions": diagnostic.suggestions.iter().map(|s| {
                serde_json::json!({
                    "message": s.message,
                    "replacement": s.replacement,
                })
            }).collect::<Vec<_>>(),
        });

        serde_json::to_writer(&mut self.writer, &json)?;
        writeln!(self.writer)?;
        Ok(())
    }

    fn emit_summary(&mut self, diagnostics: &Diagnostics) -> std::io::Result<()> {
        let summary = serde_json::json!({
            "type": "summary",
            "errors": diagnostics.error_count(),
            "warnings": diagnostics.warning_count(),
            "hints": diagnostics.hint_count(),
            "total": diagnostics.len(),
        });
        serde_json::to_writer(&mut self.writer, &summary)?;
        writeln!(self.writer)?;
        Ok(())
    }
}

/// Simple text output (no colors, minimal formatting).
pub struct SimpleEmitter<W: Write> {
    writer: W,
}

impl<W: Write> SimpleEmitter<W> {
    /// Create a new simple emitter.
    pub fn new(writer: W) -> Self {
        Self { writer }
    }
}

impl<W: Write> DiagnosticEmitter for SimpleEmitter<W> {
    fn emit(&mut self, diagnostic: &Diagnostic, cache: &SourceCache) -> std::io::Result<()> {
        // #849 chokepoint — see TerminalEmitter::emit comment.
        forward_to_compat_report_channel(diagnostic, cache);

        let loc = cache.location(diagnostic.span);

        if let Some(loc) = loc {
            writeln!(
                self.writer,
                "{}:{}:{}: {}: {} [{}]",
                loc.file,
                loc.line,
                loc.column,
                diagnostic.severity.as_str(),
                diagnostic.message,
                diagnostic.code.as_str()
            )?;
        } else {
            writeln!(
                self.writer,
                "{}: {} [{}]",
                diagnostic.severity.as_str(),
                diagnostic.message,
                diagnostic.code.as_str()
            )?;
        }

        Ok(())
    }

    fn emit_summary(&mut self, diagnostics: &Diagnostics) -> std::io::Result<()> {
        writeln!(
            self.writer,
            "{} error(s), {} warning(s)",
            diagnostics.error_count(),
            diagnostics.warning_count()
        )
    }
}
