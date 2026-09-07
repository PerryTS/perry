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
use std::sync::{Arc, Weak};

use super::site_cache::Programs;

/// The site entry's view of a pattern's compiled programs: **weak**, because
/// the pinned content-cache entry owns the bundle.
///
/// Measured cost of holding them strongly (cc, one 3300-char reply): settled
/// footprint 478/474 MB → 500/527 MB and idle CPU 2.37 → 2.68 s. The site
/// table is 1,024 entries and a compiled program is ~19 KB, so a table that
/// outlives the content cache's own eviction retains programs nothing else
/// wants. The campaign's directive is both metrics together, and a CPU win
/// bought with resident memory does not land.
///
/// Strong references remain where they belong: the content cache and every
/// live header that installed them via `Arc::into_raw`. When this bounded site
/// entry is displaced, the content entry becomes eligible for eviction.
struct WeakPrograms(Weak<Programs>);

impl WeakPrograms {
    fn downgrade(programs: &Arc<Programs>) -> Self {
        Self(Arc::downgrade(programs))
    }

    /// ALL-OR-NOTHING. A header must carry **every** program its pattern needs
    /// — that is #9801's coherence rule, and a partial upgrade is exactly the
    /// incoherent triple it fixed: a standard program installed beside a
    /// missing fancy fallback silently never-matches instead of falling back.
    /// One weak pointer to the bundle makes partial upgrade unrepresentable.
    fn upgrade(&self) -> Option<Arc<Programs>> {
        self.0.upgrade()
    }
}

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
    programs: Option<WeakPrograms>,
}

/// What a construction gets back on a site hit.
pub(super) struct SiteHit {
    pub(super) pattern: Arc<str>,
    pub(super) flags: Arc<str>,
    pub(super) flags_are_canonical: bool,
    pub(super) bits: FlagBits,
    pub(super) programs: Option<Arc<Programs>>,
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
                        programs: entry.programs.as_ref().and_then(WeakPrograms::upgrade),
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
    programs: Option<Arc<Programs>>,
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
                    // Refresh a reference whose programs have been dropped,
                    // rather than only filling an empty one: a dead weak and
                    // an absent entry mean the same thing here, and the
                    // former must be able to heal.
                    if let Some(programs) = &programs {
                        if entry
                            .programs
                            .as_ref()
                            .and_then(WeakPrograms::upgrade)
                            .is_none()
                        {
                            entry.programs = Some(WeakPrograms::downgrade(programs));
                        }
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
            programs: programs.as_ref().map(WeakPrograms::downgrade),
        });
    });
}

/// Attach the programs the first execution built, so later constructions at
/// this site are born built. A no-op when the site was evicted meanwhile.
pub(super) fn install_programs(key: usize, programs: Arc<Programs>) {
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
                if entry.key == key
                    && entry
                        .programs
                        .as_ref()
                        .and_then(WeakPrograms::upgrade)
                        .is_none()
                {
                    entry.programs = Some(WeakPrograms::downgrade(&programs));
                    return;
                }
            }
        }
    });
}

/// Whether the bounded literal-site table still records this content. The
/// content cache consults this only on a collision miss, never on a site hit.
pub(super) fn references_content(pattern: &str, flags: &str) -> bool {
    SITE_KEY_TABLE.with(|table| {
        table
            .borrow()
            .iter()
            .flatten()
            .any(|entry| &*entry.pattern == pattern && &*entry.flags == flags)
    })
}

/// Publish a freshly built bundle to every literal site for this content. The
/// content cache owns it; sites observe the complete bundle through one weak
/// reference, preserving the all-or-nothing matcher rule.
pub(super) fn install_programs_for_content(pattern: &str, flags: &str, programs: &Arc<Programs>) {
    SITE_KEY_TABLE.with(|table| {
        for entry in table.borrow_mut().iter_mut().flatten() {
            if &*entry.pattern == pattern && &*entry.flags == flags {
                entry.programs = Some(WeakPrograms::downgrade(programs));
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

#[cfg(test)]
mod tests {
    use super::*;

    /// **The all-or-nothing rule, made able to fail.**
    ///
    /// #9801 fixed an incoherent triple — a standard program memoized beside a
    /// missing fancy fallback — which does not error: it silently never
    /// matches. Holding the site entry's programs weakly reintroduces exactly
    /// that shape if it weakens each matcher separately. A single weak pointer
    /// to the program bundle makes an incoherent partial upgrade impossible.
    #[test]
    fn the_program_set_weak_reference_expires_atomically() {
        let std_program = Arc::new(::regex::Regex::new("a(b)c").expect("linear program"));
        let fancy_program = Arc::new(::fancy_regex::Regex::new("a(?=b)c").expect("fancy program"));
        let programs = Arc::new(Programs {
            std: std_program.clone(),
            fancy: Some(fancy_program.clone()),
            repeat: None,
        });
        let weak = WeakPrograms::downgrade(&programs);

        let upgraded = weak
            .upgrade()
            .expect("the shared program set is still held here");
        assert!(
            upgraded.fancy.is_some(),
            "the fancy fallback must survive the round trip while its Arc is alive"
        );
        drop(upgraded);

        // Individual matcher Arcs do not keep the SET alive. Once the bundle
        // is gone the weak entry expires all three lanes together, which is
        // the coherence property a triple of independent Weak pointers had
        // to implement manually.
        drop(programs);
        assert!(
            weak.upgrade().is_none(),
            "the site entry must never upgrade only part of a program set"
        );
        drop(fancy_program);
        drop(std_program);
        assert!(weak.upgrade().is_none());
    }

    /// The table must not be the reason a program stays alive: once nothing
    /// else holds it, a recorded entry reports "not built yet" and the next
    /// construction re-picks it up from the content cache.
    #[test]
    fn the_site_table_does_not_keep_a_program_alive() {
        test_reset();
        let key = 0x5171_E000_usize;
        let std_program = Arc::new(::regex::Regex::new("keepalive").expect("linear program"));
        let programs = Arc::new(Programs {
            std: std_program.clone(),
            fancy: None,
            repeat: None,
        });
        record(
            key,
            Arc::from("g"),
            Arc::from("keepalive"),
            Arc::from("g"),
            true,
            FlagBits {
                case_insensitive: false,
                global: true,
                multiline: false,
                sticky: false,
                dot_all: false,
                unicode: false,
                has_indices: false,
            },
            Some(programs.clone()),
        );
        assert!(
            lookup(key, "g")
                .expect("the entry was just recorded")
                .programs
                .is_some(),
            "precondition: the entry answers with its programs while they are alive"
        );

        drop(programs);
        drop(std_program);
        let hit = lookup(key, "g").expect("the entry itself survives");
        assert!(
            hit.programs.is_none(),
            "the site table holds programs WEAKLY: with every other reference gone the entry must \
             report unbuilt rather than keeping ~19 KB per slot alive on its own"
        );
        assert_eq!(
            &*hit.pattern, "keepalive",
            "the entry's identity is unaffected — only its programs expire"
        );
        test_reset();
    }
}
