//! Lazy compilation of RegExp programs.
//!
//! # Why
//!
//! Constructing a RegExp used to BUILD it. `js_regexp_new` ran
//! `compile_and_cache_regex_checked`, which is a full `regex::Regex::new` —
//! `regex_syntax` parse + HIR translate (Unicode case folding) + a Thompson
//! NFA build + the meta engine's strategy selection. That is the single most
//! expensive thing a JS program can do per regex literal, and a bundle
//! evaluates hundreds of literals at module-init time whether or not the run
//! ever matches with them: a symbolized `perf` profile of the claude-code
//! bundle's `--help` (a run that prints text and exits) put ~14% of ALL
//! retired instructions inside `regex_syntax`/`regex_automata` compilation,
//! against 0.11% for the whole of the compiled JavaScript.
//!
//! Measured on that bundle's own 2,378 distinct regex literals:
//!
//! | step | cost |
//! |---|---|
//! | `regex::Regex::new` (what construction used to do) | 82 µs/pattern |
//! | `regex_syntax::Parser::parse` (syntax only) | 4.6 µs/pattern |
//!
//! and a fixture of 200 literals where exactly ONE is ever executed spent
//! 73 ms of its 79 ms wall clock inside construction (Node: 1 ms).
//!
//! # What is lazy and what is not
//!
//! Only the *program build* moves. Everything observable at construction
//! stays at construction:
//!
//! * a syntactically invalid pattern still throws `SyntaxError` from
//!   `js_regexp_new` / `RegExp.prototype.compile`, at the same point in the
//!   program — [`std_engine_syntax_ok`] runs the SAME parser
//!   `build_std_regex` would run, on the SAME translated + flag-prefixed +
//!   REDoS-collapsed string, and anything it rejects falls through to the
//!   unchanged both-engines check (so the fancy-regex fallback for
//!   lookbehind/backreferences still decides, and still throws when both
//!   engines refuse);
//! * `.source` / `.flags` / `.global` / `.sticky` / `lastIndex` are header
//!   and side-table reads that never touched the compiled program;
//! * identity is untouched — `js_regexp_new` still allocates a fresh header
//!   per evaluation.
//!
//! The build itself happens on the first operation that needs a matcher,
//! through [`ensure_regex_compiled`], and installs exactly the pointers
//! `js_regexp_new` used to install eagerly (`regex_ptr`, `fancy_ptr`,
//! `repeat_matcher_ptr`), each a leaked `Arc` the header owns.

use std::sync::Arc;

use regex::Regex;

use super::grammar::{collapse_redos_guard_quantifiers, js_regex_to_rust_with_flags};
use super::{
    evict_regex_cache_if_full, get_or_compile_regex, is_valid_ptr, is_valid_regex_ptr,
    string_as_str, RegExpHeader, FANCY_CACHE, REGEX_SOURCE_TABLE, REPEAT_MATCHER_CACHE,
    VALIDATED_PATTERNS,
};

/// The exact string `build_std_regex` is handed for `(pattern, flags)`: the
/// JS→Rust translation with the inline `(?ims)` mode prefix the flags imply.
///
/// Extracted so the eager syntax check and the lazy build cannot drift — a
/// validator that inspects a DIFFERENT string than the builder would either
/// throw on a pattern that compiles or accept one that does not.
pub(super) fn flag_prefixed_pattern(pattern: &str, flags: &str) -> String {
    let translated = js_regex_to_rust_with_flags(pattern, flags);
    let case_insensitive = flags.contains('i');
    let multiline = flags.contains('m');
    // #2828: the `s` (dotAll) flag maps directly onto the Rust `regex`
    // crate's `(?s)` inline mode, so `.` matches newlines.
    let dot_all = flags.contains('s');
    if !(case_insensitive || multiline || dot_all) {
        return translated;
    }
    let mut prefix = String::from("(?");
    if case_insensitive {
        prefix.push('i');
    }
    if multiline {
        prefix.push('m');
    }
    if dot_all {
        prefix.push('s');
    }
    prefix.push(')');
    format!("{}{}", prefix, translated)
}

/// Does the standard engine's PARSER accept this pattern?
///
/// This is the cheap half of `build_std_regex`: `regex::RegexBuilder::build`
/// parses and then builds an NFA, and only the parse can report a syntax
/// error. Asking the parse alone answers "is this a `SyntaxError`?" without
/// building any automaton.
///
/// The parse itself has two halves, and the cheap one is enough almost
/// always. `regex_syntax`'s AST parse is pure grammar — unbalanced groups,
/// `a{2,1}`, `[z-a]`, dangling `)` all fail there. Its HIR *translate* pass is
/// where Unicode class expansion and, under `i`, `case_fold_simple` run:
/// `ClassUnicodeRange::case_fold_simple` is 3.53% of a claude-code `--help`
/// profile on its own, and on a corpus of case-folding-heavy literals translate
/// costs 138 µs/pattern against the AST parse's 3.4 µs — 40x.
///
/// Exactly one class of diagnostic is translate-only for the strings perry
/// produces: an unknown Unicode property name (`\p{Bogus}`). Perry never emits
/// the other translate-only errors' triggers — they all require `(?-u)` /
/// non-UTF-8 matching, and `js_regex_to_rust` always emits Unicode-mode
/// patterns (`\x{…}` escapes, never raw bytes). So: AST-parse everything, and
/// pay for the full translate only when the translated pattern mentions a
/// property. That is 0.7% of the claude-code bundle's literals (16 of 2,378);
/// the substring test deliberately over-triggers (a literal backslash followed
/// by `p` also matches), because over-triggering only costs time.
///
/// `tests::syntax_check_agrees_with_full_build` pins the whole thing against
/// `build_std_regex` on a corpus, so neither this split nor a `regex` upgrade
/// can silently move where a `SyntaxError` is raised.
///
/// `false` is NOT a verdict of "invalid": it only means the linear engine
/// refused, which is also how every lookbehind/backreference pattern answers.
/// The caller falls back to the unchanged both-engines path, which owns the
/// `SyntaxError` decision.
pub(super) fn std_engine_syntax_ok(pattern: &str, flags: &str) -> bool {
    // `build_std_regex` collapses ReDoS-guard bounded quantifiers before
    // building; validate the same post-collapse string.
    let collapsed = collapse_redos_guard_quantifiers(&flag_prefixed_pattern(pattern, flags));
    if collapsed.contains("\\p") || collapsed.contains("\\P") {
        return regex_syntax::Parser::new().parse(&collapsed).is_ok();
    }
    regex_syntax::ast::parse::Parser::new()
        .parse(&collapsed)
        .is_ok()
}

/// Has `(pattern, flags)` already cleared validation on this thread?
///
/// Validity is a pure function of `(pattern, flags)`, so re-deriving it is
/// pure cost. Construction used to reach this conclusion via a `REGEX_CACHE`
/// hit, which only worked because construction also compiled; with the build
/// deferred, the cache can be empty for a pattern that has been constructed a
/// thousand times (`string-width`/`emoji-regex` build a fresh ~12,807-char
/// literal on every measurement), so the fact is recorded separately.
pub(super) fn pattern_already_validated(pattern: &str, flags: &str) -> bool {
    VALIDATED_PATTERNS.with(|set| {
        set.borrow()
            .contains_key(&(pattern.to_string(), flags.to_string()))
    })
}

/// Record that `(pattern, flags)` passed validation. Capped and
/// cleared-on-overflow exactly like the compiled-program caches — the
/// consequence of a clear is a repeated parse, not a wrong answer.
pub(super) fn mark_pattern_validated(pattern: &str, flags: &str) {
    VALIDATED_PATTERNS.with(|set| {
        let mut set = set.borrow_mut();
        evict_regex_cache_if_full(&mut set);
        set.insert((pattern.to_string(), flags.to_string()), ());
    });
}

/// The `(source, flags)` a header was built from.
///
/// Prefers the GC-survivable side table (issue #637) and falls back to the
/// header's own string payloads, which — unlike the thread-local table — are
/// readable from a second statically-linked copy of the runtime (Wall 18).
pub(super) fn source_and_flags(re: *const RegExpHeader) -> (Arc<str>, Arc<str>) {
    if let Some(source) =
        REGEX_SOURCE_TABLE.with(|table| table.borrow().get(&(re as usize)).cloned())
    {
        return source;
    }
    unsafe {
        let pattern: Arc<str> = if is_valid_ptr((*re).pattern_ptr) {
            Arc::from(string_as_str((*re).pattern_ptr))
        } else {
            Arc::from("")
        };
        let flags: Arc<str> = if is_valid_ptr((*re).flags_ptr) {
            Arc::from(string_as_str((*re).flags_ptr))
        } else {
            Arc::from("")
        };
        (pattern, flags)
    }
}

/// Build this header's compiled program(s) if it has none yet.
///
/// `regex_ptr == null` is the "not built yet" state. It is published LAST so
/// a header is never observable as built while `fancy_ptr` /
/// `repeat_matcher_ptr` are still stale — every reader that consults those
/// two goes through [`lookup_fancy_regex`](super::lookup_fancy_regex) /
/// `lookup_repeat_matcher`, which call this first.
///
/// The header OWNS a leaked `Arc` reference to each program (mirroring what
/// `js_regexp_new` used to do inline), so the capped `REGEX_CACHE` /
/// `FANCY_CACHE` / `REPEAT_MATCHER_CACHE` can evict without invalidating a
/// live receiver.
///
/// Contains no JS allocation and cannot re-enter the interpreter, so it is
/// safe to call from inside a phase that holds a borrow of a GC string.
///
/// # Preconditions
/// Every call site has already established `is_valid_regex_ptr(re)` — this is
/// reached only from the guarded entry points (`js_regexp_test`,
/// `js_regexp_exec`, the `String.prototype` regex methods, …). That matters
/// for cost, not just for tidiness: `is_valid_regex_ptr` reaches
/// `try_read_gc_header` and heap-space classification, which is far too
/// expensive to repeat on the already-built path, and this runs up to three
/// times per match operation. So the hot path is two loads — is the pointer
/// plausible, is the program already there — and the full validation lives in
/// the cold builder.
#[inline]
pub(crate) fn ensure_regex_compiled(re: *const RegExpHeader) {
    if !is_valid_ptr(re) {
        return;
    }
    if unsafe { !(*re).regex_ptr.is_null() } {
        return;
    }
    build_and_install_programs(re);
}

#[cold]
fn build_and_install_programs(re: *const RegExpHeader) {
    // The one place the precondition is re-checked, so a caller that has not
    // validated cannot corrupt an unrelated allocation.
    if !is_valid_regex_ptr(re) {
        return;
    }
    let (pattern, flags) = source_and_flags(re);
    if crate::hot_diag::regex_on() {
        let cache_hit = super::REGEX_CACHE.with(|cache| {
            cache
                .borrow()
                .contains_key(&(pattern.clone(), flags.clone()))
        });
        unsafe {
            let pattern_ptr = (*re).pattern_ptr;
            crate::hot_diag::regex_with(|d| {
                d.note_build(pattern_ptr as usize, pattern.as_bytes(), &flags, cache_hit)
            });
        }
    }
    let std_arc = get_or_compile_regex(&pattern, &flags);
    let fancy_arc: Option<Arc<fancy_regex::Regex>> =
        FANCY_CACHE.with(|fc| fc.borrow().get(&(pattern.clone(), flags.clone())).cloned());
    let repeat_arc: Option<Arc<super::repeat_matcher::RepeatMatcherRegex>> = REPEAT_MATCHER_CACHE
        .with(|cache| {
            cache
                .borrow()
                .get(&(pattern.clone(), flags.clone()))
                .cloned()
        });
    // ── Repair before publishing ──────────────────────────────────────────
    //
    // A built header is treated as AUTHORITATIVE — `lookup_fancy_regex` /
    // `lookup_repeat_matcher` read a null slot beside a non-null `regex_ptr`
    // as "this pattern has no such program" — and `install_programs` below
    // memoizes the triple against the pattern text, so whatever is assembled
    // here becomes the answer for every later construction of the same
    // literal. It therefore has to be complete, and the probes above cannot
    // guarantee that on their own: the three caches are capped independently
    // and each `clear()`s wholesale, while
    // `compile_and_cache_regex_checked` returns early whenever `REGEX_CACHE`
    // already holds the pattern — so it never re-runs the fancy or
    // repeat-matcher build for a pattern whose `REGEX_CACHE` entry survived a
    // clear of one of the others.
    //
    // Both gaps are silent WRONG ANSWERS, not slowdowns: a lookbehind literal
    // whose fancy program is missing matches nothing at all, and a
    // quantified-capture literal whose repeat matcher is missing reports the
    // linear engine's capture assignment instead of ECMA-262's. Re-derive
    // what is missing. Each check costs nothing when the caches are coherent,
    // which is the normal case.
    let mut fancy_arc = fancy_arc;
    if fancy_arc.is_none() && std_arc.as_str() == super::NEVER_MATCH_PATTERN {
        // The standard program matches nothing, so this pattern is only
        // usable through the fancy engine — and it is not there.
        let flag_prefixed = flag_prefixed_pattern(&pattern, &flags);
        if let Ok(fre) = super::build_fancy_regex(&flag_prefixed) {
            let arc = Arc::new(fre);
            FANCY_CACHE.with(|fc| {
                let mut fc = fc.borrow_mut();
                evict_regex_cache_if_full(&mut fc);
                fc.insert((pattern.clone(), flags.clone()), arc.clone());
            });
            fancy_arc = Some(arc);
        }
    }
    let mut repeat_arc = repeat_arc;
    if repeat_arc.is_none() {
        // `compile` is a byte scan that returns `None` immediately unless a
        // capture group sits under a quantifier (or inside a negative
        // lookaround), so this is free for the patterns that do not need it.
        if let Some(matcher) = super::repeat_matcher::compile(&pattern, &flags) {
            let arc = Arc::new(matcher);
            REPEAT_MATCHER_CACHE.with(|cache| {
                let mut cache = cache.borrow_mut();
                evict_regex_cache_if_full(&mut cache);
                cache.insert((pattern.clone(), flags.clone()), arc.clone());
            });
            repeat_arc = Some(arc);
        }
    }

    // Remember the built programs against the pattern text, so the next
    // construction of the same literal is born built (`js_regexp_new`).
    super::site_cache::install_programs(
        &pattern,
        &flags,
        super::site_cache::Programs {
            std: std_arc.clone(),
            fancy: fancy_arc.clone(),
            repeat: repeat_arc.clone(),
        },
    );
    let regex_ptr = Arc::into_raw(std_arc) as *mut Regex;
    let fancy_ptr: *const () =
        fancy_arc.map_or(std::ptr::null(), |arc| Arc::into_raw(arc) as *const ());
    let repeat_matcher_ptr: *const () =
        repeat_arc.map_or(std::ptr::null(), |arc| Arc::into_raw(arc) as *const ());
    unsafe {
        let re = re as *mut RegExpHeader;
        (*re).fancy_ptr = fancy_ptr;
        (*re).repeat_matcher_ptr = repeat_matcher_ptr;
        // Publish last: `regex_ptr` is the built/not-built flag.
        (*re).regex_ptr = regex_ptr;
    }
}

/// The header's standard-engine program, building it on first use.
///
/// Every `&*(*re).regex_ptr` in the tree goes through here — the field is
/// null until something needs a matcher.
///
/// # Safety
/// `re` must be a live `RegExpHeader` (all callers gate on
/// `is_valid_regex_ptr`); the returned reference borrows a raw `Arc` the
/// header owns until its GC finalizer runs.
pub(crate) unsafe fn header_std_regex<'a>(re: *const RegExpHeader) -> &'a Regex {
    ensure_regex_compiled(re);
    &*(*re).regex_ptr
}
