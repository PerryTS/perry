//! Diagnostic infrastructure for the Perry TypeScript compiler.
//!
//! This crate provides structured error reporting with:
//! - Source location tracking (file, line, column)
//! - Rich diagnostic types with error codes
//! - Multiple output formats (terminal, JSON, simple text)
//! - Suggestions for fixes
//!
//! # Example
//!
//! ```
//! use perry_diagnostics::{
//!     Diagnostic, DiagnosticCode, Severity,
//!     Span, FileId, SourceCache,
//!     TerminalEmitter, DiagnosticEmitter,
//! };
//!
//! // Create a source cache and add a file
//! let mut cache = SourceCache::new();
//! let file_id = cache.add_file("test.ts", "let x: any = 42;".to_string());
//!
//! // Create a diagnostic
//! let diag = Diagnostic::warning(DiagnosticCode::AnyTypeUsage, "'any' type detected")
//!     .with_span(Span::new(file_id, 7, 10))
//!     .with_help("Consider using a more specific type")
//!     .build();
//!
//! // Emit to stderr
//! let stderr = std::io::stderr();
//! let mut emitter = TerminalEmitter::new(stderr.lock(), true);
//! emitter.emit(&diag, &cache).unwrap();
//! ```

pub mod compat_report;
pub mod diagnostic;
pub mod emitter;
pub mod source_cache;
pub mod span;

// Re-export commonly used types
pub use compat_report::{
    category_for_code, enqueue_report, has_report_sink, is_reportable_code,
    maybe_enqueue_for_diagnostic, redact_snippet, set_report_sink, sha256_hash,
    CompatibilityReport, PendingReport, RedactionError, ReportCategory, ReportSink, ReportStage,
    MAX_SNIPPET_LEN,
};
pub use diagnostic::{
    Applicability, Diagnostic, DiagnosticBuilder, DiagnosticCode, Diagnostics, RelatedInfo,
    Severity, Suggestion,
};
pub use emitter::{DiagnosticEmitter, JsonEmitter, SimpleEmitter, TerminalEmitter};
pub use source_cache::{SourceCache, SourceFile};
pub use span::{FileId, Label, LabelStyle, Location, Span};
