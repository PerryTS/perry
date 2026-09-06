//! Program compilation and the program caches, split out of `regex.rs`
//! for the 2000-line file cap: size limit, std/fancy builders, cache
//! eviction, and the checked compile-and-cache entry point.
//!
//! A child module, so `use super::*` reaches the parent's private items.

use super::*;

/// Compiled-program size budget handed to both regex engines.
///
/// The `regex` crate (and the `regex-automata` backend `fancy-regex`
/// delegates to) caps a compiled program at 10 MiB by default and rejects
/// anything larger with `CompiledTooBig` / `ExceededSizeLimit` — which our
/// callers surface as a bogus `SyntaxError: invalid pattern`. JS itself has
/// no such limit, so a *valid* pattern with large bounded repetitions is
/// wrongly rejected. semver's ReDoS-hardened `safeRe` rewrites (`\s{0,1}`,
/// `\d{1,256}`, `[…]{0,250}`, …) blow well past 10 MiB; raise the budget so
/// these legitimate patterns compile. 64 MiB comfortably fits semver's full
/// range regex while still bounding pathological input.
pub(crate) const REGEX_SIZE_LIMIT: usize = 64 * 1024 * 1024;

/// Build a `regex` crate `Regex` with the raised [`REGEX_SIZE_LIMIT`] so that
/// large-but-valid bounded-quantifier patterns aren't rejected as
/// `CompiledTooBig`. Drop-in replacement for `regex::Regex::new`.
#[cfg(feature = "regex-engine")]
pub(crate) fn build_std_regex(pattern: &str) -> Result<Regex, regex::Error> {
    // Collapse ReDoS-guard bounded quantifiers (`{m,N}`, large N) to unbounded before
    // compiling. The linear `regex` engine expands `x{0,N}` into N states, so the semver
    // package's `\d{0,256}` patterns became 8–16 MB automata each (~183 MB in a large
    // bundle). This engine can't ReDoS, so the bound is safely removable here. See
    // `grammar::collapse_redos_guard_quantifiers`.
    let collapsed = collapse_redos_guard_quantifiers(pattern);
    regex::RegexBuilder::new(&collapsed)
        .size_limit(REGEX_SIZE_LIMIT)
        .build()
}

/// The ASCII word atom the boundary spellings below share. `(?-i:…)` keeps
/// the class exact under an outer `(?i)` — ECMAScript's non-Unicode word set
/// is pure ASCII even case-insensitively (no LONG S / KELVIN SIGN), and the
/// class is already case-closed so disabling the fold changes nothing else.
#[cfg(feature = "regex-engine")]
pub(crate) const FANCY_ASCII_WORD: &str = r"(?-i:[0-9A-Za-z_])";

/// Rewrite the translator's ASCII word-boundary markers into a form
/// `fancy-regex` parses (#9305 fallout, unmasked by the transport fix).
///
/// `js_regex_to_rust` spells ECMAScript's ASCII `\b`/`\B` as `(?-iu:\b)` /
/// `(?-iu:\B)` (#9263). The `regex` crate accepts that scoped flag group,
/// but `fancy-regex`'s own parser rejects the `u` flag outright
/// (`NonUnicodeUnsupported`) — so every pattern that must run on this
/// engine (lookarounds, backreferences) and also contains a word boundary
/// failed to compile as a `SyntaxError`. cli.js's `marked` html-block
/// regex is exactly that shape, which is the throw-in-a-microtask that
/// #9305's setjmp miscompile turned into a segfault.
///
/// The markers can only come from our own translator — `(?-iu:` is itself
/// a SyntaxError in a JS pattern, so no user input survives translation
/// with that byte sequence outside a character class — making a textual
/// substitution exact. The replacement spells the boundary with
/// one-code-point lookarounds, the same technique
/// `push_unicode_ignore_case_word_boundary` already relies on fancy-regex
/// for: a boundary is "exactly one side is a word char", a non-boundary
/// "both sides agree".
#[cfg(feature = "regex-engine")]
pub(crate) fn fancy_compatible_word_boundaries(pattern: &str) -> String {
    if !pattern.contains("(?-iu:") {
        return pattern.to_string();
    }
    let w = FANCY_ASCII_WORD;
    let boundary = format!("(?:(?<={w})(?!{w})|(?<!{w})(?={w}))");
    let non_boundary = format!("(?:(?<={w})(?={w})|(?<!{w})(?!{w}))");
    pattern
        .replace(r"(?-iu:\b)", &boundary)
        .replace(r"(?-iu:\B)", &non_boundary)
}

/// Build a `fancy_regex` `Regex` with the raised delegate size limit (see
/// [`REGEX_SIZE_LIMIT`]). `fancy-regex` delegates non-fancy subpatterns to the
/// `regex` crate, so the same 10 MiB cap applies there; raise it in lockstep.
#[cfg(feature = "regex-engine")]
pub(crate) fn build_fancy_regex(pattern: &str) -> Result<fancy_regex::Regex, fancy_regex::Error> {
    let pattern = fancy_compatible_word_boundaries(pattern);
    fancy_regex::RegexBuilder::new(&pattern)
        .delegate_size_limit(REGEX_SIZE_LIMIT)
        .build()
}

/// Entry cap for the compiled-regex caches (2026-07-09 GC audit: one entry
/// per distinct `(pattern, flags)` ever compiled, no cap of any kind, entries
/// up to [`REGEX_SIZE_LIMIT`] — `new RegExp(userInput)` was an attacker-driven
/// OOM). When an insert would exceed the cap the whole map is cleared — the
/// `PARSE_KEY_CACHE` precedent: cheap, no LRU bookkeeping, recompilation is
/// the fallback. Live `RegExpHeader`s are unaffected: each header OWNS a raw
/// `Arc` reference to its compiled program(s), released by its GC finalizer,
/// so dropping the cache's references cannot free a program still in use.
#[cfg(feature = "regex-engine")]
pub(crate) const REGEX_CACHE_MAX_ENTRIES: usize = 512;

/// Clear-on-overflow guard shared by the compiled-program caches and the
/// validated-pattern set: make room for one more entry, wiping the map when it
/// is at capacity.
#[cfg(feature = "regex-engine")]
pub(crate) fn evict_regex_cache_if_full<K, V>(cache: &mut HashMap<K, V>) {
    if cache.len() >= REGEX_CACHE_MAX_ENTRIES {
        cache.clear();
        if crate::hot_diag::regex_on() {
            crate::hot_diag::regex_with(|d| d.cache_clears += 1);
        }
    }
}

/// Compile `(pattern, flags)` into the caches if absent, reporting whether
/// SOME engine accepted the flag-prefixed pattern. One NFA build total.
///
/// This is the expensive path — the emoji-regex class of pattern costs
/// milliseconds per build. It no longer runs at construction: `js_regexp_new`
/// validates with the parser alone and `regex::lazy` calls this (through
/// `get_or_compile_regex`) on the first operation that needs a matcher. It is
/// still reached from construction for the patterns the linear engine's parser
/// rejects, where only a build can tell a fancy-regex pattern from a
/// `SyntaxError`.
///
/// Returns `true` when the pattern is usable: compiled by the `regex` crate
/// (cached in `REGEX_CACHE`), or by `fancy-regex` (cached in `FANCY_CACHE`,
/// with the never-match placeholder in `REGEX_CACHE` so non-fancy callers
/// don't crash — the fancy fallback is handled in `js_regexp_exec_fancy`).
/// Returns `false` when BOTH engines reject it — nothing is cached and the
/// caller decides whether that is a SyntaxError (see `js_regexp_new`'s
/// bare-pattern fallback for the flag-prefix size edge).
/// One shared never-match program per thread.
///
/// Only used by the `PERRY_REGEX_ENGINE=regress` measurement path, where every
/// pattern needs a value in `regex_ptr` (the built/not-built flag) but no NFA:
/// building a fresh one per pattern would be exactly the compile cost the
/// experiment exists to remove from the measurement.
#[cfg(feature = "regex-engine")]
pub(crate) fn shared_never_match_program() -> Arc<Regex> {
    crate::perry_thread_local! {
        static NEVER_MATCH: RefCell<Option<Arc<Regex>>> = const { RefCell::new(None) };
    }
    NEVER_MATCH.with(|slot| {
        slot.borrow_mut()
            .get_or_insert_with(|| Arc::new(Regex::new(NEVER_MATCH_PATTERN).unwrap()))
            .clone()
    })
}

#[cfg(feature = "regex-engine")]
pub(crate) fn compile_and_cache_regex_checked(pattern: &Arc<str>, flags: &Arc<str>) -> bool {
    let already = REGEX_CACHE.with(|cache| {
        cache
            .borrow()
            .contains_key(&(pattern.clone(), flags.clone()))
    });
    if already {
        return true;
    }
    let regress_covers = if let Some(repeat_matcher) = repeat_matcher::compile(pattern, flags) {
        if crate::hot_diag::regex_on() {
            crate::hot_diag::regex_with(|d| d.compiles_repeat += 1);
        }
        REPEAT_MATCHER_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            evict_regex_cache_if_full(&mut cache);
            cache.insert((pattern.clone(), flags.clone()), Arc::new(repeat_matcher));
        });
        true
    } else {
        false
    };
    // `PERRY_REGEX_ENGINE=regress` (measurement only — see
    // `repeat_matcher::regress_first`): the ECMAScript backtracker is the
    // primary engine, so stop here. Every exec-family entry point consults the
    // repeat matcher first, and the shared never-match placeholder gives the
    // header's `regex_ptr` built-flag a value WITHOUT building an NFA — which
    // is the whole point of the experiment (the linear engine's program is
    // ~12.5 KB median against regress's 512 B, measured over 4,463 literals
    // from seven real bundles).
    if regress_covers && repeat_matcher::regress_first() {
        REGEX_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            evict_regex_cache_if_full(&mut cache);
            cache.insert(
                (pattern.clone(), flags.clone()),
                shared_never_match_program(),
            );
        });
        return true;
    }
    // Translate JS regex to Rust-compatible pattern, with the inline mode
    // prefix the flags imply. Shared with `lazy::std_engine_syntax_ok` so the
    // eager syntax check and this build can never inspect different strings.
    let regex_pattern = lazy::flag_prefixed_pattern(pattern, flags);
    let regex = match build_std_regex(&regex_pattern) {
        Ok(re) => re,
        Err(_) => {
            // Pattern has features regex crate doesn't support
            // (lookbehind, lookahead). Try fancy-regex which supports
            // the full JS regex feature set, and if it compiles, wrap
            // the result via a find-and-replace approach at the exec
            // call sites. Store a never-matching pattern so existing
            // callers don't crash.
            let fancy_ok = FANCY_CACHE.with(|fc| {
                if let Ok(fre) = build_fancy_regex(&regex_pattern) {
                    if crate::hot_diag::regex_on() {
                        crate::hot_diag::regex_with(|d| d.compiles_fancy += 1);
                    }
                    let mut fc = fc.borrow_mut();
                    evict_regex_cache_if_full(&mut fc);
                    fc.insert((pattern.clone(), flags.clone()), std::sync::Arc::new(fre));
                    true
                } else {
                    false
                }
            });
            if !fancy_ok {
                return false;
            }
            Regex::new(NEVER_MATCH_PATTERN).unwrap()
        }
    };
    if crate::hot_diag::regex_on() {
        crate::hot_diag::regex_with(|d| d.compiles_std += 1);
    }
    REGEX_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        evict_regex_cache_if_full(&mut cache);
        cache.insert((pattern.clone(), flags.clone()), Arc::new(regex));
    });
    true
}

#[cfg(feature = "regex-engine")]
pub(crate) fn get_or_compile_regex(pattern: &Arc<str>, flags: &Arc<str>) -> Arc<Regex> {
    let hit = REGEX_CACHE.with(|cache| {
        cache
            .borrow()
            .get(&(pattern.clone(), flags.clone()))
            .cloned()
    });
    if let Some(re) = hit {
        return re;
    }
    let _ = compile_and_cache_regex_checked(pattern, flags);
    REGEX_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(re) = cache.get(&(pattern.clone(), flags.clone())) {
            return re.clone();
        }
        // Both engines rejected it (validation normally throws before this
        // point) — keep the historical behavior: cache + return never-match.
        let arc = Arc::new(Regex::new(NEVER_MATCH_PATTERN).unwrap());
        evict_regex_cache_if_full(&mut cache);
        cache.insert((pattern.clone(), flags.clone()), arc.clone());
        arc
    })
}
