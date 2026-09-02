//! ECMAScript's global-scan loop, shared by every operation that walks a
//! subject with a `g` regex: `String.prototype.match`, `matchAll`, `replace`
//! and `replaceAll`.
//!
//! **Why this exists (#9430).** ECMAScript's `RegExpExec` loop and Rust's
//! match iterators disagree about one position. Where a zero-width match
//! lands exactly at the end of the previous match, Rust
//! (`regex_automata::util::iter::Searcher::try_advance`, and `fancy_regex`'s
//! `Matches::next_with`, which is documented as "adapted from the `regex`
//! crate … ignores empty matches immediately after a match") throws that
//! match away and re-searches one character to the right. ECMAScript keeps
//! it and *then* advances one code unit — the `matchStr is ""` branch of
//! `RegExp.prototype [ @@match ]`. So `"a".match(/a*/g)` is `["a", ""]` in
//! JavaScript and `["a"]` under a Rust iterator; and because the rule fires
//! at every such position, not only the last one, `"aXa".match(/a*/g)` is
//! `["a", "", "a", ""]` in JavaScript and `["a", "a"]` under a Rust iterator.
//!
//! `regress` — the third engine, reached for quantified captures — already
//! implements the ECMAScript rule (`next_start` steps one position right
//! only when `end == pos`), so its iterators are used unchanged. This module
//! exists to give the other two the same semantics, and its walk starts at a
//! caller-supplied byte offset so `matchAll` can honour `lastIndex` without
//! slicing the subject (#9429).

/// `AdvanceStringIndex` for a zero-width match that ended at `end`.
///
/// ECMAScript advances one UTF-16 code unit; Rust `&str` offsets can only
/// name whole scalars, so an astral character is stepped over in one go
/// rather than in two halves. That is the same non-`u` code-unit gap #9218
/// and #9409 record for `.` and `split(/(?:)/)` — the engines match Unicode
/// scalars, and no position exists here for the second half of a surrogate
/// pair. Stepping by a whole scalar keeps the walk on boundaries the engines
/// can be re-entered at, which is what the previous Rust-iterator behaviour
/// did as well, so nothing regresses on that axis.
pub(super) fn advance_past_empty(haystack: &str, end: usize) -> usize {
    let mut next = end + 1;
    while next < haystack.len() && !haystack.is_char_boundary(next) {
        next += 1;
    }
    next
}

/// Walk `haystack` from `start` the way `RegExpExec` does, collecting one `T`
/// per match. `find_at` must return the first match at or after the offset it
/// is given, as `(match start, match end, payload)`, or `None` to end the
/// scan.
///
/// The cursor advances to the match end, or one code unit past it when the
/// match is empty, so it is strictly increasing and the loop terminates. The
/// `start > haystack.len()` bound is what makes a trailing empty match the
/// LAST one rather than the first of an infinite series.
pub(super) fn scan<T, F>(haystack: &str, start: usize, mut find_at: F) -> Vec<T>
where
    F: FnMut(usize) -> Option<(usize, usize, T)>,
{
    let mut out = Vec::new();
    let mut cursor = start;
    while cursor <= haystack.len() {
        let Some((match_start, match_end, item)) = find_at(cursor) else {
            break;
        };
        out.push(item);
        cursor = if match_end == match_start {
            advance_past_empty(haystack, match_end)
        } else {
            match_end
        };
    }
    out
}

/// Full-match byte ranges for the linear `regex` engine.
pub(super) fn std_ranges(re: &regex::Regex, haystack: &str, start: usize) -> Vec<(usize, usize)> {
    scan(haystack, start, |cursor| {
        re.find_at(haystack, cursor).map(|matched| {
            (
                matched.start(),
                matched.end(),
                (matched.start(), matched.end()),
            )
        })
    })
}

/// Captures for the linear `regex` engine.
pub(super) fn std_captures<'h>(
    re: &regex::Regex,
    haystack: &'h str,
    start: usize,
) -> Vec<regex::Captures<'h>> {
    scan(haystack, start, |cursor| {
        let caps = re.captures_at(haystack, cursor)?;
        let full = caps.get(0).expect("capture zero is the full match");
        Some((full.start(), full.end(), caps))
    })
}

/// Full-match byte ranges for the `fancy_regex` fallback (lookaround /
/// backreferences). A scan error ends the walk, matching the
/// `while let Some(Ok(..))` shape these call sites used before.
pub(super) fn fancy_ranges(
    re: &fancy_regex::Regex,
    haystack: &str,
    start: usize,
) -> Vec<(usize, usize)> {
    scan(haystack, start, |cursor| {
        match re.find_from_pos(haystack, cursor) {
            Ok(Some(matched)) => Some((
                matched.start(),
                matched.end(),
                (matched.start(), matched.end()),
            )),
            Ok(None) | Err(_) => None,
        }
    })
}

/// Captures for the `fancy_regex` fallback.
pub(super) fn fancy_captures<'h>(
    re: &fancy_regex::Regex,
    haystack: &'h str,
    start: usize,
) -> Vec<fancy_regex::Captures<'h>> {
    scan(haystack, start, |cursor| {
        match re.captures_from_pos(haystack, cursor) {
            Ok(Some(caps)) => {
                let full = caps.get(0).expect("capture zero is the full match");
                Some((full.start(), full.end(), caps))
            }
            Ok(None) | Err(_) => None,
        }
    })
}
