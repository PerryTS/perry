//! Young-entry logs: per-side-table remembered sets for the copying minor
//! and the budgeted minor-only trace.
//!
//! # The problem this solves
//!
//! Every registered mutable-root scanner walks its whole table on every
//! collection. On the compiled claude-code TUI that is ~35k shape families,
//! ~120k descriptors and ~13k closure-prop owners per walk, three walks per
//! copying minor and two per budgeted minor (initial root scan + final
//! remark), 41+ minors per streamed reply — and every one of those walks found
//! `slots=0`: 34–56 ms per minor of scanner time discovering that nothing in
//! the table pointed at the nursery (`[gc-scanner-profile]`, 2026-09-04).
//!
//! The previous attempt (2026-09-01, reverted) skipped a walk when the
//! VISITOR was inert. That is unsound for the three expensive tables: they
//! ROOT (`visit_nanbox_u64_slot` on accessor get/set, `visit_nanbox_f64_slot`
//! on closure prop values, `visit_usize_slot` on carried keys arrays), and
//! rooting marks in Mark/Copy, so skipping drops live objects.
//!
//! # What a minor can act on
//!
//! A minor-scoped pass — the copying minor's preflight/mark/rewrite passes and
//! the budgeted cycle's `GcCollectionKind::Minor` root scan — neither moves
//! nor sweeps an OLD-generation object. `CopyingNurseryCollector::mark_addr`
//! returns an old address unchanged; `rewrite_raw_addr` follows forwarding
//! records that only a moved (young) object can carry; a minor-only trace's
//! marks on old headers are consumed by nothing (the minor sweep frees young
//! objects only, `PostTraceProbe::owner_is_dead` refuses old owners, and the
//! full path's remembered-set rebuild does not run for minors). So an entry
//! whose key AND every value are old is a provable no-op for every
//! minor-scoped visit. The set of addresses a minor CAN act on is the
//! complement: nursery, longlived (traced through, never swept, not barriered —
//! `barrier_parent_needs_remembering` is false for it) and malloc-GC objects
//! (swept by minors). [`addr_is_minor_relevant`] is that predicate.
//!
//! # The log
//!
//! [`YoungLog`] holds the KEYS of entries that may hold a minor-relevant
//! pointer. It is a log, not an index: duplicates and stale keys are allowed
//! (a stale key is a lookup miss and is dropped; a duplicate is visited twice,
//! idempotently). A minor-scoped scanner walks only the logged keys, visits
//! each entry exactly as the full walk would, and re-logs the entry iff it is
//! still relevant after the visit (a to-space survivor stays young; a promoted
//! object is old and drops out). A full-scope scanner walks the whole table as
//! before and REBUILDS the log from what it found.
//!
//! # The three rules (from `perry-young-gc-fixed-cost.md`)
//!
//! 1. **Arm before publish.** Every writer notes the key BEFORE the entry
//!    becomes findable. A note is one page-map probe (`classify_heap_generation`,
//!    hot-TLS cached) per pointer; an old key with old values notes nothing.
//! 2. **Machine-check the writer set.** Under `debug_assertions` a minor-scoped
//!    walk first re-derives the relevant set from the authoritative table and
//!    panics on any relevant entry the log does not name
//!    ([`debug_assert_logged`]). The `gc/tests` copying-minor fixtures run this
//!    on every collection, so deleting one `note` site is a red test, not a
//!    silent leak.
//! 3. **A skip needs a counter.** [`note_walk`] records, per walk, how many
//!    entries the log named, how many were visited, how many stayed relevant
//!    and how big the table was; `[gc-young-log]` prints them per collection
//!    under `PERRY_GC_DIAG=1`, and the tests read them back.
//!
//! Thread model: the shape and descriptor tables are agent-local
//! (`state()`), so their logs live beside them. The closure side tables are
//! process-global mutexed maps; their log is thread-local on purpose — an
//! entry's addresses belong to the heap of the thread that inserted it, and
//! only that thread's minors can act on them, so a foreign thread's walk must
//! neither visit nor drop them.

use std::cell::RefCell;

use super::GC_HEADER_SIZE;
use crate::arena::HeapGeneration;
use crate::value::{BIGINT_TAG, POINTER_MASK, POINTER_TAG, STRING_TAG, TAG_MASK};

/// Keys of side-table entries that may hold a pointer a minor can act on.
pub(crate) struct YoungLog<K> {
    keys: Vec<K>,
    /// A recycled buffer, so the log does not grow from ZERO CAPACITY every
    /// collection. `take_sorted` hands `keys` out and installs this in its
    /// place, so the notes made while the caller walks the batch land in a Vec
    /// that already has room; `extend` and `take_spare` round the buffers
    /// back. Without it every cycle re-grew a 20k-entry Vec from empty — the
    /// same allocate-from-scratch shape these logs exist to remove from the
    /// scanners, reintroduced one level down.
    spare: Vec<K>,
}

impl<K: Copy + Ord> YoungLog<K> {
    pub(crate) const fn new() -> Self {
        Self {
            keys: Vec::new(),
            spare: Vec::new(),
        }
    }

    /// Record `key` as possibly minor-relevant. MUST run before the entry it
    /// describes becomes findable (rule 1). Adjacent duplicates — a hot
    /// `fn.x = …` loop — are collapsed; other duplicates are harmless.
    #[inline]
    pub(crate) fn note(&mut self, key: K) {
        if self.keys.last() != Some(&key) {
            self.keys.push(key);
        }
    }

    /// Take the logged keys, sorted and deduplicated, leaving the log empty.
    /// Notes made while the caller walks the batch (owner-move hooks fire
    /// from inside a visit) land in the emptied log and are picked up by the
    /// caller's next `take_sorted` round.
    pub(crate) fn take_sorted(&mut self) -> Vec<K> {
        // Swap the recycled buffer in rather than leaving a zero-capacity Vec
        // behind: the notes made while the caller walks the batch land here.
        let mut keys = std::mem::replace(&mut self.keys, std::mem::take(&mut self.spare));
        self.keys.clear();
        keys.sort_unstable();
        keys.dedup();
        keys
    }

    /// A buffer for a walk's `kept` list, reusing the log's spare capacity.
    /// Hand it back through [`extend`](Self::extend).
    pub(crate) fn take_spare(&mut self) -> Vec<K> {
        let mut buf = std::mem::take(&mut self.spare);
        buf.clear();
        buf
    }

    /// Keep the larger of the two drained buffers for the next cycle.
    fn stash_spare(&mut self, mut buf: Vec<K>) {
        buf.clear();
        if buf.capacity() > self.spare.capacity() {
            self.spare = buf;
        }
    }

    /// Re-log the keys a walk found still relevant.
    pub(crate) fn extend(&mut self, kept: Vec<K>) {
        if self.keys.is_empty() {
            let drained = std::mem::replace(&mut self.keys, kept);
            self.stash_spare(drained);
        } else {
            self.keys.extend_from_slice(&kept);
            self.stash_spare(kept);
        }
    }

    /// Test-only: the table resets (`test_clear_*`) clear their log with them.
    #[cfg(test)]
    pub(crate) fn clear(&mut self) {
        self.keys.clear();
        self.spare.clear();
    }

    /// Rule 2: the log must name every key in `relevant`. `relevant` is the
    /// set the caller re-derived from the authoritative table under
    /// `debug_assertions`; a miss is a writer that publishes without noting.
    #[cfg(debug_assertions)]
    pub(crate) fn debug_assert_logged(&self, table: &'static str, relevant: &[K])
    where
        K: std::fmt::Debug,
    {
        if relevant.is_empty() {
            return;
        }
        let mut logged = self.keys.clone();
        logged.sort_unstable();
        logged.dedup();
        for key in relevant {
            assert!(
                logged.binary_search(key).is_ok(),
                "young log for {table} does not name {key:?}, which holds a \
                 minor-relevant pointer: a writer of that table publishes \
                 without `note`-ing the key first (see gc/young_log.rs rule 1)"
            );
        }
    }
}

/// Can a minor-scoped pass act on the object at `addr`?
///
/// `false` is authoritative for the old generation only: an old object is
/// neither moved nor swept by any minor, and it never becomes young again.
/// Everything a minor moves, marks-through or sweeps answers `true` —
/// nursery (eden + both survivor halves), longlived, and malloc-GC objects.
/// A non-heap word (handle id, foreign-thread address, integer) answers
/// `false` through the exact malloc-registry probe, never a header sniff.
#[inline]
pub(crate) fn addr_is_minor_relevant(addr: usize) -> bool {
    if addr == 0 {
        return false;
    }
    match crate::arena::classify_heap_generation(addr) {
        HeapGeneration::Old => false,
        HeapGeneration::Nursery | HeapGeneration::Longlived => true,
        HeapGeneration::Unknown => {
            addr > GC_HEADER_SIZE
                && super::malloc::gc_malloc_header_is_tracked(
                    (addr - GC_HEADER_SIZE) as *const super::GcHeader,
                )
        }
    }
}

/// [`addr_is_minor_relevant`] for a NaN-boxed value: only the three
/// pointer-carrying tags decode to an address; numbers, booleans, short
/// strings and `undefined` are never relevant.
#[inline]
pub(crate) fn bits_are_minor_relevant(bits: u64) -> bool {
    let tag = bits & TAG_MASK;
    if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
        addr_is_minor_relevant((bits & POINTER_MASK) as usize)
    } else {
        false
    }
}

/// One scanner walk's accounting (rule 3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct YoungLogWalk {
    /// The walk was minor-scoped and visited only the logged keys.
    pub(crate) partial: bool,
    /// Keys the log named when the walk started (after dedup).
    pub(crate) logged: u64,
    /// Entries actually visited (a full walk visits the whole table).
    pub(crate) visited: u64,
    /// Entries still relevant after the visit, i.e. the log size afterwards.
    pub(crate) kept: u64,
    /// Table size at walk time — `table_len - visited` is the work skipped.
    pub(crate) table_len: u64,
}

crate::perry_thread_local! {
    /// Per-table rows since the last `report_and_reset`, keyed by table name.
    static WALK_ROWS: RefCell<Vec<(&'static str, YoungLogWalk, u32)>> =
        const { RefCell::new(Vec::new()) };
    /// The most recent walk per table, for the tests (never reset).
    static LAST_WALK: RefCell<Vec<(&'static str, YoungLogWalk)>> =
        const { RefCell::new(Vec::new()) };
}

/// Record one walk. Cheap (one push per scanner per pass), so it is not
/// gated; printing is.
pub(crate) fn note_walk(table: &'static str, walk: YoungLogWalk) {
    LAST_WALK.with(|rows| {
        let mut rows = rows.borrow_mut();
        if let Some(row) = rows.iter_mut().find(|(name, _)| *name == table) {
            row.1 = walk;
        } else {
            rows.push((table, walk));
        }
    });
    if !super::gc_diag_enabled() {
        return;
    }
    WALK_ROWS.with(|rows| {
        let mut rows = rows.borrow_mut();
        if let Some(row) = rows.iter_mut().find(|(name, _, _)| *name == table) {
            row.1.partial &= walk.partial;
            row.1.logged += walk.logged;
            row.1.visited += walk.visited;
            row.1.kept += walk.kept;
            row.1.table_len = row.1.table_len.max(walk.table_len);
            row.2 += 1;
        } else {
            rows.push((table, walk, 1));
        }
    });
}

/// The most recent walk recorded for `table` on this thread.
#[cfg(test)]
pub(crate) fn last_walk(table: &'static str) -> Option<YoungLogWalk> {
    LAST_WALK.with(|rows| {
        rows.borrow()
            .iter()
            .find(|(name, _)| *name == table)
            .map(|(_, walk)| *walk)
    })
}

/// Print the rows accumulated since the last report and clear them. Called
/// beside `scanner_profile::report_and_reset` so the two read together.
pub(super) fn report_and_reset(cycle_label: &str) {
    if !super::gc_diag_enabled() {
        return;
    }
    let rows = WALK_ROWS.with(|rows| std::mem::take(&mut *rows.borrow_mut()));
    for (table, walk, passes) in rows {
        eprintln!(
            "[gc-young-log] {cycle_label} table={table} mode={} passes={passes} logged={} visited={} kept={} table_len={} skipped={}",
            if walk.partial { "young" } else { "full" },
            walk.logged,
            walk.visited,
            walk.kept,
            walk.table_len,
            (walk.table_len * u64::from(passes)).saturating_sub(walk.visited),
        );
    }
}
