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
//! `VALIDATED_PATTERNS` probe key and `owned_pattern`) and SipHash all of it
//! once; the first operation on each header then did the same three more times.
//! On the claude-code keystroke profile SipHash over pattern text was 31 % of
//! the post-turn window (regex 38 % inclusive).
//!
//! # What
//!
//! A bounded, thread-local table keyed by a cheap CONTENT fingerprint (length,
//! first / middle / last 8 bytes, canonical flags) and verified by a full byte
//! compare. Identity never depends on an address, so nothing is rekeyed on a GC
//! move and a dynamic `new RegExp(sameText)` hits too; a hit costs one short
//! integer hash plus one `memcmp` instead of hashing all pattern bytes.
//!
//! An entry owns the pattern and canonical flags as `Arc<str>` and, once the
//! first header built from it has executed, the compiled programs: a later
//! construction is born built. At capacity, entries referenced by a recorded
//! literal site are pinned and only dynamic or displaced-site entries are
//! evictable. Thus a live literal cannot rebuild, while programs for dead sites
//! can still leave the fixed-size table.
//!
//! Kill switch: `PERRY_REGEX_SITE_CACHE=0` (lookups miss, nothing is stored).

use std::cell::RefCell;
use std::sync::Arc;

use regex::Regex;

type ContentMap = crate::fast_hash::PtrHashMap<u64, Vec<Entry>>;

/// The compiled programs a header owns, in the form `lazy` installs them.
pub(super) struct Programs {
    pub(super) std: Arc<Regex>,
    pub(super) fancy: Option<Arc<fancy_regex::Regex>>,
    pub(super) repeat: Option<Arc<super::repeat_matcher::RepeatMatcherRegex>>,
}

impl Programs {
    pub(super) fn matcher_kind(&self) -> super::MatcherKind {
        if self.repeat.is_some() {
            super::MatcherKind::Repeat
        } else if self.fancy.is_some() {
            super::MatcherKind::Fancy
        } else {
            super::MatcherKind::Standard
        }
    }
}

/// What a construction gets back on a hit.
pub(super) struct Hit {
    pub(super) pattern: Arc<str>,
    pub(super) flags: Arc<str>,
    pub(super) programs: Option<Arc<Programs>>,
}

struct Entry {
    fp: u64,
    pattern: Arc<str>,
    flags: Arc<str>,
    programs: Option<Arc<Programs>>,
}

/// Sized for the recorded literal-site table. The table never exceeds this
/// bound: if all entries are pinned, a dynamic miss remains uncached.
pub(super) const MAX_ENTRIES: usize = 1024;

crate::perry_thread_local! {
    static SITE_CACHE: RefCell<ContentMap> = RefCell::new(crate::fast_hash::new_ptr_hash_map());
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
/// the (≤ 8 byte) canonical flags. Collisions are harmless because every hit
/// is verified by a full compare and colliding entries share one small bucket.
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

fn entry_matches(entry: &Entry, fp: u64, pattern: &str, flags: &str) -> bool {
    entry.fp == fp && &*entry.flags == flags && &*entry.pattern == pattern
}

fn entry_count(cache: &ContentMap) -> usize {
    cache.values().map(Vec::len).sum()
}

/// Remove one entry that has no recorded literal site. The scan happens only
/// on a distinct-content miss at capacity; literal-site hits never reach it.
fn evict_one_dynamic(cache: &mut ContentMap) -> bool {
    let victim = cache.iter().find_map(|(&fp, bucket)| {
        bucket
            .iter()
            .position(|entry| !super::site_key::references_content(&entry.pattern, &entry.flags))
            .map(|index| (fp, index))
    });
    let Some((fp, index)) = victim else {
        return false;
    };
    let bucket = cache.get_mut(&fp).expect("the selected bucket exists");
    bucket.swap_remove(index);
    if bucket.is_empty() {
        cache.remove(&fp);
    }
    true
}

fn make_room(cache: &mut ContentMap) -> bool {
    if entry_count(cache) < MAX_ENTRIES {
        return true;
    }
    if evict_one_dynamic(cache) {
        if crate::hot_diag::regex_on() {
            crate::hot_diag::regex_counters(|d| d.cache_evictions += 1);
        }
        return true;
    }
    false
}

/// Find the verified entry for `(pattern, canonical flags)`.
pub(super) fn lookup(pattern: &str, flags: &str) -> Option<Hit> {
    if !enabled() {
        return None;
    }
    let fp = fingerprint(pattern.as_bytes(), flags.as_bytes());
    SITE_CACHE.with(|cache| {
        let cache = cache.borrow();
        for entry in cache.get(&fp)? {
            if entry_matches(entry, fp, pattern, flags) {
                // Count only the construction probe's full byte compare, not
                // the cold insert/install verification.
                if crate::hot_diag::regex_on() {
                    crate::hot_diag::regex_counters(|d| {
                        d.new_site_verify_bytes += pattern.len() as u64
                    });
                }
                return Some(Hit {
                    pattern: entry.pattern.clone(),
                    flags: entry.flags.clone(),
                    programs: entry.programs.clone(),
                });
            }
        }
        None
    })
}

/// Record a validated `(pattern, canonical flags)`, returning the shared
/// owned copies a header should keep. An existing verified entry is reused.
pub(super) fn insert(pattern: &str, flags: &str) -> (Arc<str>, Arc<str>) {
    if !enabled() {
        return (Arc::from(pattern), Arc::from(flags));
    }
    let fp = fingerprint(pattern.as_bytes(), flags.as_bytes());
    SITE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(bucket) = cache.get(&fp) {
            for entry in bucket {
                if entry_matches(entry, fp, pattern, flags) {
                    return (entry.pattern.clone(), entry.flags.clone());
                }
            }
        }
        let pattern: Arc<str> = Arc::from(pattern);
        let flags: Arc<str> = Arc::from(flags);
        if make_room(&mut cache) {
            cache.entry(fp).or_default().push(Entry {
                fp,
                pattern: pattern.clone(),
                flags: flags.clone(),
                programs: None,
            });
        }
        (pattern, flags)
    })
}

/// Attach the programs the first execution built to the content entry and
/// publish a weak view to every recorded literal site for this exact content.
pub(super) fn install_programs(pattern: &str, flags: &str, programs: Arc<Programs>) {
    if !enabled() {
        return;
    }
    let fp = fingerprint(pattern.as_bytes(), flags.as_bytes());
    let content_owned = SITE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(bucket) = cache.get_mut(&fp) {
            for entry in bucket {
                if entry_matches(entry, fp, pattern, flags) {
                    if entry.programs.is_none() {
                        entry.programs = Some(programs.clone());
                    }
                    return true;
                }
            }
        }
        if !make_room(&mut cache) {
            return false;
        }
        cache.entry(fp).or_default().push(Entry {
            fp,
            pattern: Arc::from(pattern),
            flags: Arc::from(flags),
            programs: Some(programs.clone()),
        });
        true
    });
    if content_owned {
        super::site_key::install_programs_for_content(pattern, flags, &programs);
    }
}

#[cfg(test)]
pub(super) fn test_reset() {
    SITE_CACHE.with(|cache| cache.borrow_mut().clear());
}

#[cfg(test)]
pub(super) fn test_has_programs(pattern: &str, flags: &str) -> Option<bool> {
    let fp = fingerprint(pattern.as_bytes(), flags.as_bytes());
    SITE_CACHE.with(|cache| {
        let cache = cache.borrow();
        for entry in cache.get(&fp)? {
            if entry_matches(entry, fp, pattern, flags) {
                return Some(entry.programs.is_some());
            }
        }
        None
    })
}

#[cfg(test)]
pub(super) fn test_len() -> usize {
    SITE_CACHE.with(|cache| entry_count(&cache.borrow()))
}

#[cfg(test)]
pub(super) fn test_try_evict_one_dynamic() -> bool {
    SITE_CACHE.with(|cache| evict_one_dynamic(&mut cache.borrow_mut()))
}
