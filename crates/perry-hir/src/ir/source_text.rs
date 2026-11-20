//! Immutable views of retained JavaScript source.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::{Deref, Range};
use std::sync::Arc;

/// Several nested functions can retain overlapping ranges of one module
/// buffer. Cloning a source view does not copy the function's body text.
#[derive(Clone)]
pub struct SourceText {
    source: Arc<String>,
    range: Range<usize>,
}

impl SourceText {
    pub(crate) fn shared(source: Arc<String>, range: Range<usize>) -> Option<Self> {
        source.get(range.clone())?;
        Some(Self { source, range })
    }

    /// Source coordinates for observational diagnostics. A constructed string
    /// has a range relative to itself; declaration producers retain real spans.
    pub fn source_range(&self) -> Range<usize> {
        self.range.clone()
    }

    pub fn as_str(&self) -> &str {
        &self.source[self.range.clone()]
    }

    /// Slice relative to this view, preserving the original backing buffer.
    pub fn slice(&self, range: Range<usize>) -> Option<Self> {
        self.as_str().get(range.clone())?;
        Self::shared(
            Arc::clone(&self.source),
            self.range.start + range.start..self.range.start + range.end,
        )
    }
}

impl From<String> for SourceText {
    fn from(source: String) -> Self {
        let range = 0..source.len();
        Self {
            source: Arc::new(source),
            range,
        }
    }
}

impl From<&str> for SourceText {
    fn from(source: &str) -> Self {
        source.to_owned().into()
    }
}

impl Deref for SourceText {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for SourceText {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Debug for SourceText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl fmt::Display for SourceText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.as_str(), f)
    }
}

impl PartialEq for SourceText {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}
impl Eq for SourceText {}

impl Hash for SourceText {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl PartialEq<str> for SourceText {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for SourceText {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::hash_map::DefaultHasher;

    fn hash(value: &impl Hash) -> u64 {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn nested_views_survive_owner_release_and_hash_like_owned_strings() {
        let buffer = Arc::new("prefix function inner() { return 7; } suffix".to_owned());
        let source = SourceText::shared(Arc::clone(&buffer), 7..37).unwrap();
        let nested = source.slice(9..14).unwrap();
        drop(buffer);
        drop(source);
        assert_eq!(nested.as_str(), "inner");
        assert_eq!(hash(&nested), hash(&"inner".to_owned()));
        assert_eq!(format!("{nested:?}"), format!("{:?}", "inner"));
        assert_eq!(nested, SourceText::from("inner"));
    }

    #[test]
    fn ranges_validate_utf8_and_cannot_escape_the_view() {
        let source = SourceText::from("π😀");
        assert_eq!(source.slice(0..2).unwrap().as_str(), "π");
        assert_eq!(source.slice(2..6).unwrap().as_str(), "😀");
        assert!(source.slice(1..2).is_none());
        assert!(source.slice(0..7).is_none());
        assert!(source.slice(2..6).unwrap().slice(0..5).is_none());
    }
}
