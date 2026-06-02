//! Diagnostic types for compiler errors, warnings, and hints.

use crate::span::{Label, Span};
use serde::{Deserialize, Serialize};

/// Severity level of a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Severity {
    /// Informational hint (suggestions for improvement)
    Hint,
    /// Warning (compiles but may cause issues)
    Warning,
    /// Error (blocks compilation)
    Error,
}

impl Severity {
    /// Get the string representation for display.
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Hint => "hint",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Diagnostic error codes organized by category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DiagnosticCode {
    // Parse errors (P001-P099)
    /// Syntax error during parsing
    ParseError,

    // Type errors (T001-T099)
    /// Type mismatch between expected and actual types
    TypeMismatch,
    /// Missing required type annotation
    MissingTypeAnnotation,
    /// Explicit 'any' type usage
    AnyTypeUsage,
    /// Implicit 'any' due to missing inference
    ImplicitAny,
    /// Unsupported type construct
    UnsupportedType,

    // Unsupported features (U001-U099)
    /// Unsupported binary operator
    UnsupportedBinaryOp,
    /// Unsupported unary operator
    UnsupportedUnaryOp,
    /// Unsupported expression type
    UnsupportedExpression,
    /// Unsupported statement type
    UnsupportedStatement,
    /// Unsupported pattern (destructuring, etc.)
    UnsupportedPattern,
    /// Generic unsupported feature
    UnsupportedFeature,
    /// Unsupported property key
    UnsupportedPropertyKey,
    /// Unsupported assignment target
    UnsupportedAssignmentTarget,
    /// Unsupported callee type
    UnsupportedCalleeType,

    // Dynamic code (D001-D099)
    /// Dynamic property access with non-constant key
    DynamicPropertyAccess,
    /// eval() usage
    EvalUsage,
    /// new Function() usage
    NewFunctionUsage,
    /// Reflection API usage (Object.keys, Reflect.*, etc.)
    ReflectionUsage,
    /// Dynamic import() usage
    DynamicImport,

    // Compatibility warnings (C001-C099)
    /// Implicit type coercion
    ImplicitCoercion,
    /// Loose equality (== instead of ===)
    LooseEquality,
    /// Non-deterministic code patterns
    NonDeterministicCode,

    // Resolution errors (R001-R099)
    /// Undefined variable reference
    UndefinedVariable,
    /// Undefined function reference
    UndefinedFunction,
    /// Unresolved import
    UnresolvedImport,
    /// Reference to a no-op stub symbol — code links and runs but
    /// the call silently does nothing on the chosen target (#464).
    NoOpStub,
    /// Reference to a stdlib symbol Perry doesn't implement (#463).
    /// Distinct from `NoOpStub`: stubs link to real but no-op code on
    /// some targets; this fires when the symbol isn't backed by any
    /// implementation at all (e.g. `crypto.subtle.encrypt`). Manifest
    /// lookup happens at HIR lowering time, so the user sees the error
    /// before the binary is built — no more silent wrong output.
    UnimplementedApi,

    // Internal errors (I001-I099)
    /// Internal compiler error
    InternalError,
}

impl DiagnosticCode {
    /// Get the error code string (e.g., "U001").
    pub fn as_str(&self) -> &'static str {
        match self {
            // Parse errors
            Self::ParseError => "P001",

            // Type errors
            Self::TypeMismatch => "T001",
            Self::MissingTypeAnnotation => "T002",
            Self::AnyTypeUsage => "T003",
            Self::ImplicitAny => "T004",
            Self::UnsupportedType => "T005",

            // Unsupported features
            Self::UnsupportedBinaryOp => "U001",
            Self::UnsupportedUnaryOp => "U002",
            Self::UnsupportedExpression => "U003",
            Self::UnsupportedStatement => "U004",
            Self::UnsupportedPattern => "U005",
            Self::UnsupportedFeature => "U006",
            Self::UnsupportedPropertyKey => "U007",
            Self::UnsupportedAssignmentTarget => "U008",
            Self::UnsupportedCalleeType => "U009",

            // Dynamic code
            Self::DynamicPropertyAccess => "D001",
            Self::EvalUsage => "D002",
            Self::NewFunctionUsage => "D003",
            Self::ReflectionUsage => "D004",
            Self::DynamicImport => "D005",

            // Compatibility warnings
            Self::ImplicitCoercion => "C001",
            Self::LooseEquality => "C002",
            Self::NonDeterministicCode => "C003",

            // Resolution errors
            Self::UndefinedVariable => "R001",
            Self::UndefinedFunction => "R002",
            Self::UnresolvedImport => "R003",
            Self::NoOpStub => "R004",
            Self::UnimplementedApi => "R005",

            // Internal errors
            Self::InternalError => "I001",
        }
    }

    /// Get the default severity for this error code.
    pub fn default_severity(&self) -> Severity {
        match self {
            // Errors
            Self::ParseError
            | Self::TypeMismatch
            | Self::UnsupportedBinaryOp
            | Self::UnsupportedUnaryOp
            | Self::UnsupportedExpression
            | Self::UnsupportedStatement
            | Self::UnsupportedPattern
            | Self::UnsupportedFeature
            | Self::UnsupportedPropertyKey
            | Self::UnsupportedAssignmentTarget
            | Self::UnsupportedCalleeType
            | Self::EvalUsage
            | Self::NewFunctionUsage
            | Self::UndefinedVariable
            | Self::UndefinedFunction
            | Self::UnresolvedImport
            | Self::UnimplementedApi
            | Self::InternalError => Severity::Error,

            // Warnings
            Self::AnyTypeUsage
            | Self::ImplicitAny
            | Self::UnsupportedType
            | Self::DynamicPropertyAccess
            | Self::ReflectionUsage
            | Self::DynamicImport
            | Self::ImplicitCoercion
            | Self::LooseEquality
            | Self::NonDeterministicCode
            | Self::NoOpStub => Severity::Warning,

            // Hints
            Self::MissingTypeAnnotation => Severity::Hint,
        }
    }
}

impl std::fmt::Display for DiagnosticCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// How applicable a suggested fix is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applicability {
    /// Can be applied automatically with high confidence
    MachineApplicable,
    /// Might need manual review
    MaybeIncorrect,
    /// Just a hint with placeholders, user must decide
    HasPlaceholders,
}

/// A suggested fix for a diagnostic.
#[derive(Debug, Clone)]
pub struct Suggestion {
    /// Description of what this fix does
    pub message: String,
    /// The span to replace
    pub span: Span,
    /// The replacement text
    pub replacement: String,
    /// How confident we are in this fix
    pub applicability: Applicability,
}

impl Suggestion {
    /// Create a new suggestion.
    pub fn new(
        message: impl Into<String>,
        span: Span,
        replacement: impl Into<String>,
        applicability: Applicability,
    ) -> Self {
        Self {
            message: message.into(),
            span,
            replacement: replacement.into(),
            applicability,
        }
    }

    /// Create a machine-applicable suggestion.
    pub fn replace(span: Span, replacement: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(message, span, replacement, Applicability::MachineApplicable)
    }
}

/// Related information for a diagnostic.
#[derive(Debug, Clone)]
pub struct RelatedInfo {
    /// Location of related information
    pub span: Span,
    /// Message explaining the relation
    pub message: String,
}

/// A compiler diagnostic with rich information.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    /// Unique error code
    pub code: DiagnosticCode,
    /// Severity level
    pub severity: Severity,
    /// Short message (single line)
    pub message: String,
    /// Longer explanation (optional)
    pub explanation: Option<String>,
    /// Primary span (where the error is)
    pub span: Span,
    /// Additional labels (related locations)
    pub labels: Vec<Label>,
    /// Help/fix suggestions
    pub suggestions: Vec<Suggestion>,
    /// Related diagnostics
    pub related: Vec<RelatedInfo>,
}

impl Diagnostic {
    /// Create a new error diagnostic.
    pub fn error(code: DiagnosticCode, message: impl Into<String>) -> DiagnosticBuilder {
        DiagnosticBuilder::new(code, Severity::Error, message)
    }

    /// Create a new warning diagnostic.
    pub fn warning(code: DiagnosticCode, message: impl Into<String>) -> DiagnosticBuilder {
        DiagnosticBuilder::new(code, Severity::Warning, message)
    }

    /// Create a new hint diagnostic.
    pub fn hint(code: DiagnosticCode, message: impl Into<String>) -> DiagnosticBuilder {
        DiagnosticBuilder::new(code, Severity::Hint, message)
    }

    /// Create a diagnostic with the code's default severity.
    pub fn new(code: DiagnosticCode, message: impl Into<String>) -> DiagnosticBuilder {
        DiagnosticBuilder::new(code, code.default_severity(), message)
    }

    /// Check if this is an error.
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    /// Check if this is a warning.
    pub fn is_warning(&self) -> bool {
        self.severity == Severity::Warning
    }

    /// Check if this is a hint.
    pub fn is_hint(&self) -> bool {
        self.severity == Severity::Hint
    }
}

/// Builder for constructing diagnostics fluently.
pub struct DiagnosticBuilder {
    inner: Diagnostic,
}

impl DiagnosticBuilder {
    /// Create a new diagnostic builder.
    pub fn new(code: DiagnosticCode, severity: Severity, message: impl Into<String>) -> Self {
        Self {
            inner: Diagnostic {
                code,
                severity,
                message: message.into(),
                explanation: None,
                span: Span::DUMMY,
                labels: Vec::new(),
                suggestions: Vec::new(),
                related: Vec::new(),
            },
        }
    }

    /// Set the primary span.
    pub fn with_span(mut self, span: Span) -> Self {
        self.inner.span = span;
        self
    }

    /// Add a secondary label.
    pub fn with_label(mut self, span: Span, message: impl Into<String>) -> Self {
        self.inner.labels.push(Label::secondary(span, message));
        self
    }

    /// Add a primary label (in addition to the main span).
    pub fn with_primary_label(mut self, span: Span, message: impl Into<String>) -> Self {
        self.inner.labels.push(Label::primary(span, message));
        self
    }

    /// Add a suggestion.
    pub fn with_suggestion(mut self, suggestion: Suggestion) -> Self {
        self.inner.suggestions.push(suggestion);
        self
    }

    /// Add a simple replacement suggestion.
    pub fn suggest_replace(
        mut self,
        span: Span,
        replacement: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        self.inner
            .suggestions
            .push(Suggestion::replace(span, replacement, message));
        self
    }

    /// Add help text.
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.inner.explanation = Some(help.into());
        self
    }

    /// Add related information.
    pub fn with_related(mut self, span: Span, message: impl Into<String>) -> Self {
        self.inner.related.push(RelatedInfo {
            span,
            message: message.into(),
        });
        self
    }

    /// Build the diagnostic.
    pub fn build(self) -> Diagnostic {
        self.inner
    }
}

/// Collection of diagnostics with summary statistics.
#[derive(Debug, Clone, Default)]
pub struct Diagnostics {
    /// All diagnostics
    pub items: Vec<Diagnostic>,
}

impl Diagnostics {
    /// Create a new empty collection.
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Add a diagnostic.
    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.items.push(diagnostic);
    }

    /// Extend with multiple diagnostics.
    pub fn extend(&mut self, diagnostics: impl IntoIterator<Item = Diagnostic>) {
        self.items.extend(diagnostics);
    }

    /// Check if there are any errors.
    pub fn has_errors(&self) -> bool {
        self.items.iter().any(|d| d.is_error())
    }

    /// Count errors.
    pub fn error_count(&self) -> usize {
        self.items.iter().filter(|d| d.is_error()).count()
    }

    /// Count warnings.
    pub fn warning_count(&self) -> usize {
        self.items.iter().filter(|d| d.is_warning()).count()
    }

    /// Count hints.
    pub fn hint_count(&self) -> usize {
        self.items.iter().filter(|d| d.is_hint()).count()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Get the number of diagnostics.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Iterate over diagnostics.
    pub fn iter(&self) -> impl Iterator<Item = &Diagnostic> {
        self.items.iter()
    }
}

impl IntoIterator for Diagnostics {
    type Item = Diagnostic;
    type IntoIter = std::vec::IntoIter<Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<'a> IntoIterator for &'a Diagnostics {
    type Item = &'a Diagnostic;
    type IntoIter = std::slice::Iter<'a, Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}
