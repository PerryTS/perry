//! Source span types for tracking locations in source code.

use serde::{Deserialize, Serialize};

/// Unique identifier for a source file in the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileId(pub u32);

impl FileId {
    /// A dummy file ID for spans without a known file.
    pub const DUMMY: FileId = FileId(u32::MAX);
}

/// A span in source code with file and byte offset information.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Span {
    /// File ID (index into source cache)
    pub file_id: FileId,
    /// Byte offset of start (inclusive)
    pub start: u32,
    /// Byte offset of end (exclusive)
    pub end: u32,
}

impl Span {
    /// A dummy span for cases where no location is available.
    pub const DUMMY: Span = Span {
        file_id: FileId::DUMMY,
        start: 0,
        end: 0,
    };

    /// Create a new span.
    pub fn new(file_id: FileId, start: u32, end: u32) -> Self {
        Self {
            file_id,
            start,
            end,
        }
    }

    /// Check if this is a dummy/unknown span.
    pub fn is_dummy(&self) -> bool {
        self.file_id == FileId::DUMMY
    }

    /// Merge two spans into one that covers both.
    /// Both spans must be from the same file.
    pub fn merge(self, other: Span) -> Span {
        debug_assert!(
            self.file_id == other.file_id || self.is_dummy() || other.is_dummy(),
            "Cannot merge spans from different files"
        );

        if self.is_dummy() {
            return other;
        }
        if other.is_dummy() {
            return self;
        }

        Span {
            file_id: self.file_id,
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    /// Get the length of this span in bytes.
    pub fn len(&self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    /// Check if this span is empty.
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

impl Default for Span {
    fn default() -> Self {
        Self::DUMMY
    }
}

/// Resolved location with file path, line, and column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    /// File path
    pub file: String,
    /// Line number (1-indexed)
    pub line: u32,
    /// Column number (1-indexed)
    pub column: u32,
}

impl std::fmt::Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{}", self.file, self.line, self.column)
    }
}

/// A labeled span for multi-span diagnostics.
#[derive(Debug, Clone)]
pub struct Label {
    /// The span to highlight
    pub span: Span,
    /// Message to display at this location
    pub message: String,
    /// Style of the label (primary or secondary)
    pub style: LabelStyle,
}

impl Label {
    /// Create a primary label (main error location).
    pub fn primary(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
            style: LabelStyle::Primary,
        }
    }

    /// Create a secondary label (related location).
    pub fn secondary(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
            style: LabelStyle::Secondary,
        }
    }
}

/// Style for diagnostic labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelStyle {
    /// Primary label - the main error location (typically red underline)
    Primary,
    /// Secondary label - related locations (typically blue underline)
    Secondary,
}
