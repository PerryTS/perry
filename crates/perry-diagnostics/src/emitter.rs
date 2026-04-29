//! Diagnostic emitters for different output formats.

use crate::diagnostic::{Diagnostic, Diagnostics, Severity};
use crate::source_cache::SourceCache;
use crate::span::LabelStyle;
use std::io::Write;

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
