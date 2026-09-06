//! Content-keyed construction cache for `RegExp`.
//!
//! # Why
//!
//! `js_regexp_new` runs once per EVALUATION of a regex literal (ECMA-262: a
//! literal is a new object every time), and TUI code evaluates literals inside
//! hot functions: `string-width`'s `emojiRegex()` returns a fresh ~12 KB
//! `/…/g` on every call, once per text segment per layout pass, and
//! `ansi-regex` builds the same `new RegExp(parts.join("|"), "g")` per call.
//! Each construction used to copy the pattern three times (the
//! `VALIDATED_PATTERNS` probe key, `owned_pattern`, the `REGEX_SOURCE_TABLE`
//! entry) and SipHash all of it once; the first operation on each header then
//! did the same three more times — `build_and_install_programs` probes the
//! three `(String, String)`-keyed program caches — and, for the common
//! no-fallback pattern, `lookup_fancy_regex` / `lookup_repeat_matcher`
//! re-probed two of them on EVERY exec. On the claude-code keystroke profile
//! SipHash over pattern text was 31 % of the post-turn window (regex 38 %
//! inclusive), all of it under these five functions.
//!
//! # What
//!
//! A direct-mapped, thread-local table keyed by a cheap CONTENT fingerprint
//! (length, first / middle / last 8 bytes, canonical flags) and verified by a
//! full byte compare — identity never depends on an address, so nothing is
//! rekeyed on a GC move and a dynamic `new RegExp(sameText)` hits too; a hit
//! costs one `memcmp` instead of a hash plus three copies. An entry owns the
//! pattern and canonical flags as `Arc<str>` (shared into
//! `REGEX_SOURCE_TABLE`, so a header costs two refcount bumps instead of two
//! `String`s) and, once the first header built from it has been executed, the
//! compiled programs: a later construction installs those eagerly, so the
//! header is born built and never touches the `(pattern, flags)` caches.
//!
//! Validity is a pure function of `(pattern, flags)`, so a hit legitimately
//! skips validation: an entry is only ever written on the validated path, and
//! the programs it hands out were built for exactly this text.
//!
//! Kill switch: `PERRY_REGEX_SITE_CACHE=0` (lookups miss, nothing is stored).

use std::cell::RefCell;
use std::sync::Arc;

use regex::Regex;

/// The compiled programs a header owns, in the form `lazy` installs them.
pub(super) struct Programs {
    pub(super) std: Arc<Regex>,
    pub(super) fancy: Option<Arc<fancy_regex::Regex>>,
    pub(super) repeat: Option<Arc<super::repeat_matcher::RepeatMatcherRegex>>,
}

impl Clone for Programs {
    fn clone(&self) -> Self {
        Self {
            std: self.std.clone(),
            fancy: self.fancy.clone(),
            repeat: self.repeat.clone(),
        }
    }
}

/// What a construction gets back on a hit.
pub(super) struct Hit {
    pub(super) pattern: Arc<str>,
    pub(super) flags: Arc<str>,
    pub(super) programs: Option<Programs>,
}

struct Entry {
    fp: u64,
    pattern: Arc<str>,
    flags: Arc<str>,
    programs: Option<Programs>,
}

/// Direct-mapped slots (2-way: a fingerprint may live in `slot` or
/// `slot ^ 1`). Sized for a bundle's live literal working set; the
/// claude-code TUI cycles through a few dozen per render.
const SLOTS: usize = 1024;

crate::perry_thread_local! {
    static SITE_CACHE: RefCell<Vec<Option<Entry>>> = RefCell::new(Vec::new());
}

fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        crate::gc::env_default_on_from_value(
            std::env::var("PERRY_REGEX_SITE_CACHE").ok().as_deref(),
        )
    })
}

/// Cheap content fingerprint: length, three 8-byte windows of the pattern,
/// the (≤ 8 byte) canonical flags. Collisions are harmless — every hit is
/// verified by a full compare — they only cost the verify and a re-insert.
fn fingerprint(pattern: &[u8], flags: &[u8]) -> u64 {
    #[inline]
    fn window(bytes: &[u8], at: usize) -> u64 {
        let mut w = [0u8; 8];
        let end = (at + 8).min(bytes.len());
        if at < end {
            w[..end - at].copy_from_slice(&bytes[at..end]);
        }
        u64::from_le_bytes(w)
    }
    #[inline]
    fn mix(h: u64, w: u64) -> u64 {
        (h ^ w).wrapping_mul(0xC6BC_2796_92B5_C323).rotate_left(29)
    }
    let n = pattern.len();
    let mut h = (n as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h = mix(h, window(pattern, 0));
    h = mix(h, window(pattern, n / 2));
    h = mix(h, window(pattern, n.saturating_sub(8)));
    h = mix(h, window(flags, 0));
    h
}

#[inline]
fn slot_of(fp: u64) -> usize {
    (fp as usize) & (SLOTS - 1)
}

fn entry_matches(entry: &Entry, fp: u64, pattern: &str, flags: &str) -> bool {
    entry.fp == fp && &*entry.flags == flags && &*entry.pattern == pattern
}

/// Find the verified entry for `(pattern, canonical flags)`.
pub(super) fn lookup(pattern: &str, flags: &str) -> Option<Hit> {
    if !enabled() {
        return None;
    }
    let fp = fingerprint(pattern.as_bytes(), flags.as_bytes());
    let slot = slot_of(fp);
    SITE_CACHE.with(|cache| {
        let cache = cache.borrow();
        if cache.is_empty() {
            return None;
        }
        for s in [slot, slot ^ 1] {
            if let Some(entry) = &cache[s] {
                if entry_matches(entry, fp, pattern, flags) {
                    // The verify is a FULL byte compare, so its cost is
                    // linear in the pattern and this counter — not
                    // `pattern_bytes`, which counts every construction
                    // whether it probed or not — is the `memcmp` volume.
                    // Counted at the construction probe only; `insert` and
                    // `install_programs` verify too and are not counted here.
                    if crate::hot_diag::regex_on() {
                        let n = pattern.len() as u64;
                        crate::hot_diag::regex_counters(|d| d.new_site_verify_bytes += n);
                    }
                    return Some(Hit {
                        pattern: entry.pattern.clone(),
                        flags: entry.flags.clone(),
                        programs: entry.programs.clone(),
                    });
                }
            }
        }
        None
    })
}

/// Record a validated `(pattern, canonical flags)`, returning the shared
/// owned copies a header should keep. An existing verified entry is reused
/// (its programs are kept); otherwise the fresh entry has none yet.
pub(super) fn insert(pattern: &str, flags: &str) -> (Arc<str>, Arc<str>) {
    if !enabled() {
        return (Arc::from(pattern), Arc::from(flags));
    }
    let fp = fingerprint(pattern.as_bytes(), flags.as_bytes());
    let slot = slot_of(fp);
    SITE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.is_empty() {
            cache.resize_with(SLOTS, || None);
        }
        for s in [slot, slot ^ 1] {
            if let Some(entry) = &cache[s] {
                if entry_matches(entry, fp, pattern, flags) {
                    return (entry.pattern.clone(), entry.flags.clone());
                }
            }
        }
        let victim = if cache[slot].is_none() {
            slot
        } else if cache[slot ^ 1].is_none() {
            slot ^ 1
        } else {
            slot ^ ((fp >> 11) as usize & 1)
        };
        let pattern: Arc<str> = Arc::from(pattern);
        let flags: Arc<str> = Arc::from(flags);
        cache[victim] = Some(Entry {
            fp,
            pattern: pattern.clone(),
            flags: flags.clone(),
            programs: None,
        });
        (pattern, flags)
    })
}

/// Attach the programs the first execution built to the entry for
/// `(pattern, canonical flags)`, so every later construction of the same
/// text is born built. Inserts the entry if it was evicted meanwhile.
pub(super) fn install_programs(pattern: &str, flags: &str, programs: Programs) {
    if !enabled() {
        return;
    }
    let fp = fingerprint(pattern.as_bytes(), flags.as_bytes());
    let slot = slot_of(fp);
    SITE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.is_empty() {
            cache.resize_with(SLOTS, || None);
        }
        for s in [slot, slot ^ 1] {
            if let Some(entry) = &mut cache[s] {
                if entry_matches(entry, fp, pattern, flags) {
                    if entry.programs.is_none() {
                        entry.programs = Some(programs);
                    }
                    return;
                }
            }
        }
        let victim = if cache[slot].is_none() {
            slot
        } else if cache[slot ^ 1].is_none() {
            slot ^ 1
        } else {
            slot ^ ((fp >> 11) as usize & 1)
        };
        cache[victim] = Some(Entry {
            fp,
            pattern: Arc::from(pattern),
            flags: Arc::from(flags),
            programs: Some(programs),
        });
    });
}

#[cfg(test)]
pub(super) fn test_reset() {
    SITE_CACHE.with(|cache| cache.borrow_mut().clear());
}

#[cfg(test)]
pub(super) fn test_has_programs(pattern: &str, flags: &str) -> Option<bool> {
    let fp = fingerprint(pattern.as_bytes(), flags.as_bytes());
    let slot = slot_of(fp);
    SITE_CACHE.with(|cache| {
        let cache = cache.borrow();
        if cache.is_empty() {
            return None;
        }
        for s in [slot, slot ^ 1] {
            if let Some(entry) = &cache[s] {
                if entry_matches(entry, fp, pattern, flags) {
                    return Some(entry.programs.is_some());
                }
            }
        }
        None
    })
}
