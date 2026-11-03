//! Source file cache for diagnostic rendering.

use crate::span::{FileId, Location, Span};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// A cached source file with line information.
#[derive(Debug, Clone)]
pub struct SourceFile {
    /// Unique identifier
    pub id: FileId,
    /// File path
    pub path: PathBuf,
    /// Source code content
    pub source: String,
    /// Byte offsets where each line starts, built by the first lookup that
    /// needs a line: a file parsed without any diagnostic never pays for it.
    line_starts: OnceLock<Vec<u32>>,
}

impl SourceFile {
    /// Create a new source file.
    fn new(id: FileId, path: PathBuf, source: String) -> Self {
        Self {
            id,
            path,
            source,
            line_starts: OnceLock::new(),
        }
    }

    fn line_starts(&self) -> &[u32] {
        self.line_starts
            .get_or_init(|| compute_line_starts(&self.source))
    }

    /// Get the line and column for a byte offset.
    pub fn line_column(&self, offset: u32) -> (u32, u32) {
        let offset = offset.min(self.source.len() as u32);

        // Binary search for the line containing this offset
        let line_idx = match self.line_starts().binary_search(&offset) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        };

        let line_start = self.line_starts()[line_idx];
        let line = (line_idx + 1) as u32;
        let column = (offset - line_start + 1).max(1);

        (line, column)
    }

    /// Get the text of a specific line (1-indexed).
    pub fn line_text(&self, line: u32) -> Option<&str> {
        if line == 0 {
            return None;
        }

        let idx = (line - 1) as usize;
        let start = *self.line_starts().get(idx)? as usize;
        let end = self
            .line_starts()
            .get(idx + 1)
            .map(|&e| e as usize)
            .unwrap_or(self.source.len());

        let text = &self.source[start..end];
        // Trim trailing newline but keep other whitespace
        Some(text.trim_end_matches('\n').trim_end_matches('\r'))
    }

    /// Get the number of lines in this file.
    pub fn line_count(&self) -> usize {
        self.line_starts().len()
    }

    /// Get a slice of the source code.
    pub fn slice(&self, start: u32, end: u32) -> Option<&str> {
        let start = start as usize;
        let end = (end as usize).min(self.source.len());
        if start <= end && end <= self.source.len() {
            Some(&self.source[start..end])
        } else {
            None
        }
    }
}

/// Compute the byte offset where each line starts.
fn compute_line_starts(source: &str) -> Vec<u32> {
    let mut starts = vec![0];
    for (i, c) in source.char_indices() {
        if c == '\n' {
            starts.push((i + 1) as u32);
        }
    }
    starts
}

/// Cache of source files for diagnostic rendering.
#[derive(Debug, Default)]
pub struct SourceCache {
    /// Source files indexed by FileId
    files: HashMap<FileId, SourceFile>,
    /// Map from path to FileId
    path_to_id: HashMap<PathBuf, FileId>,
    /// Next file ID to assign
    next_id: u32,
}

impl SourceCache {
    /// Create a new empty source cache.
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
            path_to_id: HashMap::new(),
            next_id: 0,
        }
    }

    /// Add a file to the cache, returning its FileId.
    /// If the file was already added, returns the existing FileId.
    pub fn add_file(&mut self, path: impl AsRef<Path>, source: String) -> FileId {
        let path = path.as_ref().to_path_buf();

        // Check if already cached
        if let Some(&id) = self.path_to_id.get(&path) {
            return id;
        }

        let id = FileId(self.next_id);
        self.next_id += 1;

        let file = SourceFile::new(id, path.clone(), source);
        self.files.insert(id, file);
        self.path_to_id.insert(path, id);

        id
    }

    /// Get a source file by ID.
    pub fn get_file(&self, id: FileId) -> Option<&SourceFile> {
        self.files.get(&id)
    }

    /// Get a source file by path.
    pub fn get_file_by_path(&self, path: impl AsRef<Path>) -> Option<&SourceFile> {
        let id = self.path_to_id.get(path.as_ref())?;
        self.files.get(id)
    }

    /// Get the FileId for a path, if it exists.
    pub fn get_id(&self, path: impl AsRef<Path>) -> Option<FileId> {
        self.path_to_id.get(path.as_ref()).copied()
    }

    /// Resolve a span to a Location with file path, line, and column.
    pub fn location(&self, span: Span) -> Option<Location> {
        if span.is_dummy() {
            return None;
        }

        let file = self.files.get(&span.file_id)?;
        let (line, column) = file.line_column(span.start);

        Some(Location {
            file: file.path.to_string_lossy().into_owned(),
            line,
            column,
        })
    }

    /// Get the source text for a span.
    pub fn source_text(&self, span: Span) -> Option<&str> {
        if span.is_dummy() {
            return None;
        }

        let file = self.files.get(&span.file_id)?;
        file.slice(span.start, span.end)
    }

    /// Get the line text containing a span.
    pub fn line_text(&self, span: Span) -> Option<&str> {
        if span.is_dummy() {
            return None;
        }

        let file = self.files.get(&span.file_id)?;
        let (line, _) = file.line_column(span.start);
        file.line_text(line)
    }

    /// Get the number of files in the cache.
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Check if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_starts() {
        let source = "line1\nline2\nline3";
        let starts = compute_line_starts(source);
        assert_eq!(starts, vec![0, 6, 12]);
    }

    #[test]
    fn test_line_column() {
        let mut cache = SourceCache::new();
        let source = "hello\nworld\n".to_string();
        let id = cache.add_file("test.ts", source);

        let file = cache.get_file(id).unwrap();

        // First character of first line
        assert_eq!(file.line_column(0), (1, 1));
        // Last character of first line
        assert_eq!(file.line_column(4), (1, 5));
        // First character of second line
        assert_eq!(file.line_column(6), (2, 1));
        // 'o' in "world"
        assert_eq!(file.line_column(7), (2, 2));
    }

    #[test]
    fn test_line_text() {
        let mut cache = SourceCache::new();
        let source = "line one\nline two\nline three".to_string();
        let id = cache.add_file("test.ts", source);

        let file = cache.get_file(id).unwrap();

        assert_eq!(file.line_text(1), Some("line one"));
        assert_eq!(file.line_text(2), Some("line two"));
        assert_eq!(file.line_text(3), Some("line three"));
        assert_eq!(file.line_text(4), None);
        assert_eq!(file.line_text(0), None);
    }

    #[test]
    fn test_location() {
        let mut cache = SourceCache::new();
        let source = "let x = 42;\nlet y = 100;".to_string();
        let id = cache.add_file("test.ts", source);

        let span = Span::new(id, 4, 5); // 'x'
        let loc = cache.location(span).unwrap();
        assert_eq!(loc.line, 1);
        assert_eq!(loc.column, 5);

        let span = Span::new(id, 16, 17); // 'y'
        let loc = cache.location(span).unwrap();
        assert_eq!(loc.line, 2);
        assert_eq!(loc.column, 5);
    }
}
