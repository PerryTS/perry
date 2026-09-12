//! An allocating convenience API for build tooling, over the same engine.
//!
//! The runtime does not use this. It owns its memory, carries its own budgets
//! and reports every limit as an outcome; none of that is useful to a CLI
//! scanning a source file, which wants `is_match` and a slice back. This wraps
//! [`crate::Regex`] with ordinary `String`/`&str` ergonomics.
//!
//! Patterns are written in the **`regex` crate's dialect**, because that is
//! what every pattern in the CLI was written and reviewed against, and
//! [`dialect`] translates each one into ECMAScript that means the same thing --
//! including where the two spell something identically and mean something
//! different. Anything it cannot carry across intact is a construction error,
//! never a silently different pattern. Offsets are UTF-8 bytes into the `&str`
//! searched, as in `regex`, although the engine answers in UTF-16 units.
//!
//! `tests/dialect_parity.rs` holds this to that: every pattern the CLI compiles,
//! and a battery of general ones, answered by both engines and compared.

use crate::{Budget, Error, Limits, Regex as Engine, SearchStats, Span};
use std::borrow::Cow;
use std::ops::Index;

mod dialect;
pub use dialect::DialectError;

/// Work one search may spend. Build tooling runs on files it is compiling, not
/// on input arriving over a socket, so this is "large enough not to think
/// about" rather than tuned; exhausting it is a panic, not a no-match.
const WORK: usize = 1 << 40;

/// Limits sized for tooling. Scratch grows on demand, so a high ceiling costs
/// nothing until a search needs it -- and a lazy `.*?` over a multi-megabyte
/// bundle does need it.
fn limits() -> Limits {
    Limits {
        program_bytes: 32 * 1024 * 1024,
        scratch_bytes: 4 * 1024 * 1024 * 1024,
        input_bytes: 4 * 1024 * 1024 * 1024,
    }
}

/// Maps UTF-16 unit offsets, which the engine answers in, onto UTF-8 byte
/// offsets, which a `&str` is indexed by. Wholly ASCII text needs no map at
/// all, and build tooling reads mostly ASCII source; otherwise the map is built
/// the first time a match needs converting, so a search that finds nothing
/// never pays for it.
struct Offsets<'t> {
    text: &'t str,
    map: std::cell::OnceCell<Option<Vec<usize>>>,
}

impl<'t> Offsets<'t> {
    fn for_text(text: &'t str) -> Self {
        Self {
            text,
            map: std::cell::OnceCell::new(),
        }
    }

    fn byte(&self, unit: usize) -> usize {
        let map = self.map.get_or_init(|| {
            if self.text.is_ascii() {
                return None;
            }
            // One entry per UTF-16 unit, plus the end, each holding the byte
            // offset that unit's character begins at.
            let mut map = Vec::with_capacity(self.text.len() + 1);
            for (byte, ch) in self.text.char_indices() {
                for _ in 0..ch.len_utf16() {
                    map.push(byte);
                }
            }
            map.push(self.text.len());
            Some(map)
        });
        match map {
            None => unit,
            Some(map) => map[unit],
        }
    }
}

/// One matched region of the subject.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Match<'t> {
    text: &'t str,
    start: usize,
    end: usize,
}

impl<'t> Match<'t> {
    pub fn as_str(&self) -> &'t str {
        &self.text[self.start..self.end]
    }
    pub fn start(&self) -> usize {
        self.start
    }
    pub fn end(&self) -> usize {
        self.end
    }
    pub fn range(&self) -> std::ops::Range<usize> {
        self.start..self.end
    }
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
    pub fn len(&self) -> usize {
        self.end - self.start
    }
}

/// The groups one match captured. Group 0 is the whole match.
#[derive(Clone, Debug)]
pub struct Captures<'r, 't> {
    text: &'t str,
    spans: Vec<Option<(usize, usize)>>,
    names: &'r [(String, Vec<usize>)],
}

impl<'t> Captures<'_, 't> {
    pub fn get(&self, index: usize) -> Option<Match<'t>> {
        let (start, end) = (*self.spans.get(index)?)?;
        Some(Match {
            text: self.text,
            start,
            end,
        })
    }
    /// The group declared under `name`, if it participated in the match.
    pub fn name(&self, name: &str) -> Option<Match<'t>> {
        let (_, indices) = self.names.iter().find(|(n, _)| n == name)?;
        indices.iter().find_map(|&index| self.get(index))
    }
    pub fn len(&self) -> usize {
        self.spans.len()
    }
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }
}

/// `caps[1]`, as the `regex` crate allows. Panics on an unset group, which is
/// the same contract.
impl Index<usize> for Captures<'_, '_> {
    type Output = str;
    fn index(&self, index: usize) -> &str {
        self.get(index)
            .map(|m| m.as_str())
            .unwrap_or_else(|| panic!("no group at index '{index}'"))
    }
}

/// A compiled pattern.
pub struct Regex {
    pattern: String,
    engine: Engine,
    names: Vec<(String, Vec<usize>)>,
}

impl std::fmt::Debug for Regex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Regex").field(&self.pattern).finish()
    }
}

/// One search's answer, before offsets are converted: every group's UTF-16
/// span, and where the whole match ended in units.
struct Found {
    spans: Vec<Option<Span>>,
}

impl Regex {
    /// Compile a pattern written in the `regex` crate's dialect. Returns an
    /// error, with the offset and the reason, for anything whose meaning would
    /// not survive translation into ECMAScript.
    pub fn new(pattern: &str) -> Result<Self, Error> {
        let translated = dialect::translate(pattern).map_err(Error::Dialect)?;
        let mut budget = Budget::new(WORK);
        let engine = Engine::compile(
            translated.source.as_bytes(),
            translated.flags,
            limits(),
            &mut budget,
        )?;
        let names = engine.named_groups();
        Ok(Self {
            pattern: pattern.to_string(),
            engine,
            names,
        })
    }

    /// The pattern as written, before translation.
    pub fn as_str(&self) -> &str {
        &self.pattern
    }

    pub fn is_match(&self, text: &str) -> bool {
        let mut budget = Budget::new(WORK);
        match self.engine.is_match(text.as_bytes(), limits(), &mut budget) {
            Ok(found) => found,
            Err(error) => self.failed(error),
        }
    }

    pub fn find<'t>(&self, text: &'t str) -> Option<Match<'t>> {
        self.captures(text)?.get(0)
    }

    pub fn captures<'r, 't>(&'r self, text: &'t str) -> Option<Captures<'r, 't>> {
        let offsets = Offsets::for_text(text);
        let found = self.search(text, 0)?;
        Some(self.convert(text, &offsets, found))
    }

    pub fn find_iter<'r, 't>(&'r self, text: &'t str) -> Matches<'r, 't> {
        Matches {
            inner: self.captures_iter(text),
        }
    }

    pub fn captures_iter<'r, 't>(&'r self, text: &'t str) -> CaptureMatches<'r, 't> {
        CaptureMatches {
            regex: self,
            text,
            offsets: Offsets::for_text(text),
            at: 0,
            last_end: None,
        }
    }

    /// Replace every match with `replacement`, inserted literally. `$1`-style
    /// expansion is not implemented: nothing here needs it, and a replacement
    /// that happened to contain `$` would otherwise change meaning silently.
    pub fn replace_all<'t>(&self, text: &'t str, replacement: &str) -> Cow<'t, str> {
        let mut matches = self.find_iter(text).peekable();
        if matches.peek().is_none() {
            return Cow::Borrowed(text);
        }
        let mut out = String::with_capacity(text.len());
        let mut at = 0;
        for m in matches {
            out.push_str(&text[at..m.start()]);
            out.push_str(replacement);
            at = m.end();
        }
        out.push_str(&text[at..]);
        Cow::Owned(out)
    }

    /// One search from `start`, a UTF-16 unit offset.
    fn search(&self, text: &str, start: usize) -> Option<Found> {
        let mut spans = vec![None::<Span>; self.engine.capture_count().max(1)];
        let mut budget = Budget::new(WORK);
        match self.engine.find(
            text.as_bytes(),
            start,
            Some(&mut spans),
            limits(),
            &mut budget,
            &mut SearchStats::default(),
        ) {
            Ok(true) => Some(Found { spans }),
            Ok(false) => None,
            Err(error) => self.failed(error),
        }
    }

    fn convert<'r, 't>(
        &'r self,
        text: &'t str,
        offsets: &Offsets<'t>,
        found: Found,
    ) -> Captures<'r, 't> {
        Captures {
            text,
            spans: found
                .spans
                .iter()
                .map(|span| span.map(|s| (offsets.byte(s.start()), offsets.byte(s.end()))))
                .collect(),
            names: &self.names,
        }
    }

    /// A search that could not finish has no answer, and reporting it as a
    /// no-match would be a wrong one -- for the install scanner, a missed
    /// detection. The `regex` crate cannot fail here, so no caller has an error
    /// path to give this to; stopping loudly is the honest remaining option.
    fn failed(&self, error: Error) -> ! {
        panic!(
            "regex search for `{}` did not finish: {error:?}",
            self.pattern
        )
    }
}

pub struct CaptureMatches<'r, 't> {
    regex: &'r Regex,
    text: &'t str,
    offsets: Offsets<'t>,
    /// Where the next search starts, in UTF-16 units.
    at: usize,
    /// Where the previous match ended, in UTF-16 units.
    last_end: Option<usize>,
}

impl<'r, 't> Iterator for CaptureMatches<'r, 't> {
    type Item = Captures<'r, 't>;
    fn next(&mut self) -> Option<Captures<'r, 't>> {
        let mut found = self.regex.search(self.text, self.at)?;
        let (start, end) = whole(&found);
        // The `regex` crate's rule: an empty match that ends where the previous
        // match ended is not reported, and the search resumes one character on.
        // ECMAScript's `matchAll` would report it, so this is the step that
        // keeps iteration answering as `regex` does.
        if start == end && Some(end) == self.last_end {
            let next = self.next_character(end)?;
            found = self.regex.search(self.text, next)?;
        }
        let (_, end) = whole(&found);
        self.at = end;
        self.last_end = Some(end);
        Some(self.regex.convert(self.text, &self.offsets, found))
    }
}

impl CaptureMatches<'_, '_> {
    /// The unit offset one whole character after `unit`, so a search never
    /// starts between the halves of a surrogate pair.
    fn next_character(&self, unit: usize) -> Option<usize> {
        let byte = self.offsets.byte(unit);
        let ch = self.text[byte..].chars().next()?;
        Some(unit + ch.len_utf16())
    }
}

fn whole(found: &Found) -> (usize, usize) {
    let span = found.spans[0].expect("a match always sets group 0");
    (span.start(), span.end())
}

pub struct Matches<'r, 't> {
    inner: CaptureMatches<'r, 't>,
}

impl<'t> Iterator for Matches<'_, 't> {
    type Item = Match<'t>;
    fn next(&mut self) -> Option<Match<'t>> {
        self.inner.next()?.get(0)
    }
}

/// Quote every character the `regex` dialect could read as syntax, so the
/// result can be interpolated into a pattern passed to [`Regex::new`]. The set
/// is `regex::escape`'s.
pub fn escape(text: &str) -> String {
    const META: &str = r"\.+*?()|[]{}^$#&-~";
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if META.contains(ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests;
