//! The never-match placeholder program and the compiled-program cache key.
//!
//! Split out of `regex.rs` to keep that file under the 2000-line size gate.

use std::sync::Arc;

/// The pattern of the never-match program `compile_and_cache_regex_checked`
/// installs in `REGEX_CACHE` for a pattern only `fancy-regex` accepts, so a
/// caller reaching for the standard program does not crash.
///
/// Named because `lazy::build_and_install_programs` has to RECOGNISE it: a
/// header whose standard program is this placeholder is usable only through
/// its fancy fallback, so the fallback has to exist beside it.
#[cfg(feature = "regex-engine")]
pub(crate) const NEVER_MATCH_PATTERN: &str = r"[^\s\S]";

/// Key of the three compiled-program caches.
///
/// `(String, String)` looks harmless and is not: `HashMap::get` needs a
/// `&(String, String)`, so **every probe materialised the key** — two heap
/// allocations and two copies of the pattern text, on a path that runs once
/// per RegExp OBJECT, and a JS regex literal evaluates to a fresh object every
/// time it is reached. A native-churn census of the claude-code binary
/// (2026-09-05) put `js_regexp_test` → `lookup_repeat_matcher` →
/// `build_and_install_programs` at **6,044 MB of 8,334 MB of estimated
/// allocation with zero live bytes** — 73 % of all remaining native churn —
/// split across the three probe sites: the `get_or_compile_regex` probe
/// (2,071 MB) and two `core::fmt::Formatter::pad` frames (1,989 MB and
/// 1,984 MB), which is what `.to_string()` on an `Arc<str>` lowers to.
///
/// Keying by `Arc<str>` makes a probe two refcount increments and no
/// allocation: every caller that matters already holds those `Arc`s, because
/// `REGEX_SOURCE_TABLE` and `regex::site_cache` share one allocation of a
/// literal's text with every header built from it. Hashing still walks the
/// pattern bytes — the allocation is what the census measured, and what this
/// removes.
#[cfg(feature = "regex-engine")]
pub(crate) type ProgramKey = (Arc<str>, Arc<str>);
