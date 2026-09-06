//! Literal-site keyed construction cache for `RegExp` — O(1), no hashing, no
//! byte compare.
//!
//! # Why this exists next to `site_cache`
//!
//! [`super::site_cache`] answers "have I seen this pattern TEXT before?" and
//! is what a dynamic `new RegExp(s)` needs. It is keyed by a content
//! fingerprint and, because a fingerprint can collide, every hit is verified
//! by a **full byte compare of the pattern**. That verify is linear in the
//! pattern, and a regex literal evaluates to a fresh object every time it is
//! reached: on claude-code the segment loop constructs `string-width`'s
//! ~12,807-character `/…/g` once per grapheme, so the verify alone is ~2.0 GB
//! of `memcmp` per 400-character reply and 39.6 % of `js_regexp_new`'s own
//! profile subtree.
//!
//! A literal does not need to be identified by its text. It is one source
//! site, and its pattern and flags are fixed at compile time. The compiler now
//! says so: `Expr::RegExp` emits an 8-byte private global per literal site and
//! passes its ADDRESS as `site_key` (`expr/logical_collections.rs`), and
//! [`js_regexp_new_site`](super::js_regexp_new_site) probes this table with
//! it.
//!
//! # Why the key is sound, and why the string handles are not
//!
//! Identity by address is only sound while the address cannot be reused for
//! something else. A `StringHeader` address fails that twice over — headers
//! are GC-managed, so an address is freed and reused, and a moving collector
//! relocates them — which is why the earlier analysis of this problem
//! concluded no sound string identity was available and left the content
//! compare in place.
//!
//! A per-site global has neither problem: it is emitted by the compiler into
//! the image, never freed, never moved, and distinct sites are distinct
//! globals and therefore distinct addresses. So an entry is verified by
//! comparing ONE WORD, and the pattern is never read at all — not hashed, not
//! fingerprinted, not compared.
//!
//! What that leaves per construction on a hit: two `Arc` refcount bumps for
//! the shared `(pattern, flags)` text, the program handles if the site has
//! been executed once, and the header allocation itself. No validation (the
//! first construction at this site did it, and validity is a pure function of
//! the pair), no flag canonicalization, no fingerprint, no `memcmp`.
//!
//! Kill switch: `PERRY_REGEX_SITE_KEY=0` (every probe misses and nothing is
//! recorded, so the construction falls through to the content-keyed path
//! exactly as before this existed).

use std::cell::RefCell;
use std::sync::Arc;

use super::site_cache::Programs;

/// The flag bits `js_regexp_new` derives from the canonical flags text. They
/// are a pure function of the site's flags literal, so a hit reads them
/// instead of re-scanning the string seven times.
#[derive(Clone, Copy)]
pub(super) struct FlagBits {
    pub(super) case_insensitive: bool,
    pub(super) global: bool,
    pub(super) multiline: bool,
    pub(super) sticky: bool,
    pub(super) dot_all: bool,
    pub(super) unicode: bool,
    pub(super) has_indices: bool,
}

struct Entry {
    key: usize,
    /// The caller's flags text VERBATIM, as the site spells it. Compared on
    /// every probe: flags are at most eight bytes, so the check is free, and
    /// it makes the entry exact for a caller that is not the emitted lowering
    /// (`/x/ig` and `/x/gi` are two spellings of one canonical form and must
    /// not answer for each other's `flags_are_canonical`).
    raw_flags: Arc<str>,
    pattern: Arc<str>,
    flags: Arc<str>,
    /// The caller's flags string already IS the canonical text, so the header
    /// can share it instead of materializing a GC string (#9819). A property
    /// of the site: the author wrote `/x/gi` or `/x/ig` once.
    flags_are_canonical: bool,
    bits: FlagBits,
    programs: Option<Programs>,
}

/// What a construction gets back on a site hit.
pub(super) struct SiteHit {
    pub(super) pattern: Arc<str>,
    pub(super) flags: Arc<str>,
    pub(super) flags_are_canonical: bool,
    pub(super) bits: FlagBits,
    pub(super) programs: Option<Programs>,
}

/// Direct-mapped, 2-way (a key may live in `slot` or `slot ^ 1`). A bundle's
/// live literal working set is small — claude-code holds 2,935 distinct
/// patterns across ~2,378 literal sites and a render cycles through a few
/// dozen.
const SLOTS: usize = 1024;

crate::perry_thread_local! {
    static SITE_KEY_TABLE: RefCell<Vec<Option<Entry>>> = RefCell::new(Vec::new());
}

fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        crate::gc::env_default_on_from_value(std::env::var("PERRY_REGEX_SITE_KEY").ok().as_deref())
    })
}

/// The site global is 8-byte aligned, so the low three bits carry no
/// information; shift them out before masking. No hash — the key is already a
/// unique identity, and hashing it would be the cost this table exists to
/// remove.
#[inline]
fn slot_of(key: usize) -> usize {
    (key >> 3) & (SLOTS - 1)
}

/// The entry recorded for `key`, or `None`.
pub(super) fn lookup(key: usize, raw_flags: &str) -> Option<SiteHit> {
    if !enabled() || key == 0 {
        return None;
    }
    let slot = slot_of(key);
    SITE_KEY_TABLE.with(|table| {
        let table = table.borrow();
        if table.is_empty() {
            return None;
        }
        for s in [slot, slot ^ 1] {
            if let Some(entry) = &table[s] {
                if entry.key == key && &*entry.raw_flags == raw_flags {
                    return Some(SiteHit {
                        pattern: entry.pattern.clone(),
                        flags: entry.flags.clone(),
                        flags_are_canonical: entry.flags_are_canonical,
                        bits: entry.bits,
                        programs: entry.programs.clone(),
                    });
                }
            }
        }
        None
    })
}

/// Record what the first construction at `key` established. Callers must pass
/// the validated, canonical values — an entry is only ever written on the path
/// that has already validated the pair.
pub(super) fn record(
    key: usize,
    raw_flags: Arc<str>,
    pattern: Arc<str>,
    flags: Arc<str>,
    flags_are_canonical: bool,
    bits: FlagBits,
    programs: Option<Programs>,
) {
    if !enabled() || key == 0 {
        return;
    }
    let slot = slot_of(key);
    SITE_KEY_TABLE.with(|table| {
        let mut table = table.borrow_mut();
        if table.is_empty() {
            table.resize_with(SLOTS, || None);
        }
        for s in [slot, slot ^ 1] {
            if let Some(entry) = &mut table[s] {
                if entry.key == key && entry.raw_flags == raw_flags {
                    if entry.programs.is_none() {
                        entry.programs = programs;
                    }
                    return;
                }
            }
        }
        let victim = if table[slot].is_none() {
            slot
        } else if table[slot ^ 1].is_none() {
            slot ^ 1
        } else {
            // Both ways taken by other sites: evict the primary. A site whose
            // entry is evicted simply falls back to the content-keyed path,
            // which is correct and merely slower.
            slot
        };
        table[victim] = Some(Entry {
            key,
            raw_flags,
            pattern,
            flags,
            flags_are_canonical,
            bits,
            programs,
        });
    });
}

/// Attach the programs the first execution built, so later constructions at
/// this site are born built. A no-op when the site was evicted meanwhile.
pub(super) fn install_programs(key: usize, programs: Programs) {
    if !enabled() || key == 0 {
        return;
    }
    let slot = slot_of(key);
    SITE_KEY_TABLE.with(|table| {
        let mut table = table.borrow_mut();
        if table.is_empty() {
            return;
        }
        for s in [slot, slot ^ 1] {
            if let Some(entry) = &mut table[s] {
                if entry.key == key && entry.programs.is_none() {
                    entry.programs = Some(programs);
                    return;
                }
            }
        }
    });
}

#[cfg(test)]
pub(super) fn test_reset() {
    SITE_KEY_TABLE.with(|table| table.borrow_mut().clear());
}

/// The pattern text this site is recorded under, or `None`. Test-only: the
/// probe that lets a sabotage of `slot_of`/the key comparison be caught by a
/// test that constructs two different literals at two colliding sites.
///
/// Takes the key in the **emitted lowering's type** (`i64`, what
/// `Expr::RegExp`'s `ptrtoint` produces and what the `js_regexp_new_site`
/// extern declares) and narrows it here, so a test holds exactly the value the
/// compiler passes and crosses the same `as usize` boundary the product entry
/// point does. The table itself is keyed by `usize` because the key IS an
/// address; the two spellings meet at the FFI edge and nowhere else.
#[cfg(test)]
pub(super) fn test_recorded_pattern(key: i64, raw_flags: &str) -> Option<String> {
    lookup(key as usize, raw_flags).map(|hit| hit.pattern.to_string())
}

/// How many slots hold an entry. Test-only: proves a dynamic
/// `new RegExp(str)` did NOT record anything.
#[cfg(test)]
pub(super) fn test_occupied_slots() -> usize {
    SITE_KEY_TABLE.with(|table| table.borrow().iter().filter(|e| e.is_some()).count())
}
