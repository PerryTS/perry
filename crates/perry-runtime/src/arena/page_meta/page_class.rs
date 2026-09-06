//! The direct-indexed page-class table and the 4-way generation cache it
//! replaced (#9853), split out of `page_meta` for the 2000-line file cap.
//!
//! A child module, so `use super::*` sees the parent's private items
//! (`PageGenerationRange`, `PageGenerationSlot`, `HeapGeneration`, …)
//! without widening any visibility.

use super::*;

/// One entry of the direct-indexed table: the range last confirmed for this
/// 1 MiB class, stamped with the invalidation epoch it was confirmed under.
#[derive(Clone, Copy)]
struct PageClassEntry {
    range: PageGenerationRange,
    epoch: u64,
}

impl PageClassEntry {
    /// The filler every unwritten slot holds. `epoch: 0` is the sentinel no
    /// live epoch ever takes (`epoch` starts at 1 and [`PageGenerationCacheSet::
    /// invalidate`] steps over 0 on wrap), so a freshly allocated table is
    /// entirely dead without a second "valid" flag to keep coherent.
    const DEAD: Self = Self {
        range: PageGenerationCache::empty().range,
        epoch: 0,
    };
}

/// Initial span of the direct table, in 1 MiB classes, and how far below the
/// first registered key the base is placed.
///
/// Measured on the compiled claude-code TUI: the live span is **1,018-1,021
/// classes** at ~40 % density, with the base moving per process (ASLR). The
/// base is `first_registered_key - SLACK`, and the first registration can fall
/// anywhere in the eventual span, so both ends have to be covered by the two
/// constants alone. Writing `S = SLACK`, `N = SPAN` and `W = 1,021` for the
/// measured width, a table that covers every case without rebasing needs
///
/// * `S >= W - 1` — otherwise a first registration at the TOP of the span
///   leaves the classes below the base uncovered; and
/// * `N > W - 1 + S` — otherwise a first registration at the BOTTOM leaves the
///   classes above `base + N` uncovered.
///
/// Both must hold, so the width actually covered is
/// `W <= min(S + 1, N - S)` — **maximised at `S = N / 2`**, where it is `N / 2`.
/// That is the whole of the sizing argument, and it is worth writing down
/// because the obvious pairing gets it wrong: `N = 4096, S = 1024` pays for
/// 4,096 entries and covers a span of only **1,025** — four classes above the
/// measured 1,021, which is not a margin. `S = N / 2` covers **2,048** for the
/// same 4,096 entries: **twice the measured span at identical cost**.
///
/// So `N = 4096, S = 2048`: 4,096 x 40 B = **160 KB** on each thread that
/// classifies, allocated only on that thread's first insert, covering any span
/// up to 2,048 classes wherever the first registration falls within it.
///
/// Exceeding it is not a correctness problem — [`PageGenerationCacheSet::
/// rebase_to_cover`] widens the table and the `rebases` counter says how often
/// that happened — so these are sized to make the rebase rare, not to make it
/// impossible.
const PAGE_CLASS_TABLE_INITIAL_SPAN: usize = 4096;
const PAGE_CLASS_TABLE_BASE_SLACK: usize = PAGE_CLASS_TABLE_INITIAL_SPAN / 2;

/// The span these two constants actually cover, `min(S + 1, N - S)`, and the
/// compile-time guard that keeps the derivation above load-bearing rather than
/// decorative. The pairing this replaced (`N = 4096, S = 1024`) covers 1,025 —
/// four classes above the measured span — and fails this assert, which is the
/// point: the sizing is not obvious and a plausible-looking edit gets it wrong.
const PAGE_CLASS_TABLE_COVERED_SPAN: usize = {
    let below = PAGE_CLASS_TABLE_BASE_SLACK + 1;
    let above = PAGE_CLASS_TABLE_INITIAL_SPAN - PAGE_CLASS_TABLE_BASE_SLACK;
    if below < above {
        below
    } else {
        above
    }
};
/// Measured live span on the compiled claude-code TUI, worst of two runs.
const PAGE_CLASS_TABLE_MEASURED_SPAN: usize = 1021;
const _: () = assert!(
    PAGE_CLASS_TABLE_COVERED_SPAN >= 2 * PAGE_CLASS_TABLE_MEASURED_SPAN,
    "the initial table must cover at least twice the measured span, wherever \
     the first registration falls in it — otherwise the common case rebases"
);
/// Above this span the table stops growing and out-of-span keys simply fall
/// through to the authoritative map uncached. 16 GiB of address span is far
/// past any arena this runtime places; the cap exists so a stray registration
/// at a wild address cannot allocate an unbounded table.
const PAGE_CLASS_TABLE_MAX_SPAN: usize = 16 * 1024;

/// Which arm [`PageGenerationCacheSet`] is running, resolved once per thread on
/// the first insert and then read as a PLAIN FIELD in the hot path.
///
/// Not `page_class_table_enabled()` on the lookup path, deliberately: that is a
/// `OnceLock` and a `OnceLock` read is an ACQUIRE load. This path runs
/// **440 M times per turn** — an `ldar` plus a branch on every one of them is a
/// cost the table is supposed to be removing, and it would land on BOTH arms,
/// so the A/B would have hidden it while the comparison against main paid it.
/// The field shares the first cache line with `base`/`epoch`/`table`, which a
/// lookup loads anyway, so the arm test is free.
///
/// `ARM_UNRESOLVED` behaves as the table arm and is CORRECT for both: before
/// the first insert the table is empty and every way is invalid, so either arm
/// answers "miss" for every key.
const ARM_UNRESOLVED: u8 = 0;
const ARM_TABLE: u8 = 1;
const ARM_WAYS: u8 = 2;

/// The cache in front of [`PageGenerationMap`]: a **direct-indexed table** keyed
/// by `addr >> GENERATION_CLASS_SHIFT`, with the previous 4-way set retained
/// behind `PERRY_GC_PAGE_CLASS_TABLE=0` as the control arm.
///
/// # Why a table and not a bigger cache
/// The 4-way set was measured (`PERRY_CLASSIFY_DIAG`, 3300-char claude-code
/// reply) at **440 M lookups per turn, 20 % miss, 60 % of those misses on a key
/// evicted within the last 64 evictions** — pure capacity, against a working
/// set of **402-432 registered classes**. All four ways were in use
/// (`ways_distinct_max = 4`), so the shortfall is ~120x, which no associativity
/// reaches; #7469 already measured 16 ways as an 8.6 % regression for 1.5 %
/// fewer misses, and five further associativity changes measured flat. The
/// registered classes sit in a span of **1,018-1,021** at ~40 % density, so a
/// table over the span holds every one of them in ~33 KB and answers a lookup
/// with one bounds compare and one load. The same bounds check rejects the
/// ~8,000 candidate addresses per turn that are in no registered block — the
/// other 22 % of misses — without a separate filter.
///
/// # What it is not
/// A cache, not the truth. `PageGenerationMap` stays authoritative: every miss
/// falls through to it exactly as before, and every registration, unregistration
/// and retag invalidates the whole table by bumping `epoch` (O(1), and the same
/// "clear everything" contract the 4-way set had, for the same reason: a stale
/// entry is exactly what this guards against). A hit still requires
/// `range.contains(addr)` — a key match at a range boundary is not an address
/// match.
///
/// # The one place the table is WEAKER than the set it replaces
/// A class can hold more than one range (`PageGenerationSlot::Multiple`). The
/// 4-way set could hold two of them at once, in two ways under the same key,
/// and hit on both; the table has one slot per class, so ranges sharing a
/// class evict each other and alternate accesses miss. This is a real
/// regression in kind, bounded by how many classes are `Multiple` — and it is
/// what the `[gc-page-class]` miss rate would show if the collapse predicted
/// below fails to appear. Registered blocks are `BLOCK_SIZE`-sized and
/// `BLOCK_SIZE == 1 << GENERATION_CLASS_SHIFT`, so one block is exactly one
/// class and the multi-range case is the sub-block registration, not the norm.
///
/// # The two things measurement did not settle, handled explicitly
/// * **The base moves per process** (observed: `0x43daa2` vs `0x57e3c2` on two
///   runs). It is taken from the first insert, minus slack — never compiled in.
/// * **The span can grow** (observed: 1,018 vs 1,021 on two runs of one
///   binary). An insert outside `[base, base + len)` rebases the table to cover
///   it, up to `PAGE_CLASS_TABLE_MAX_SPAN`; past the cap the key is left
///   uncached and falls through. Both paths are pinned by tests that fail when
///   the fallback is removed, because a wrong answer here is a misclassified
///   pointer — a collector that moves the wrong thing.
///
/// Stored behind an `UnsafeCell`, not a `Cell`, for the reason recorded on the
/// 4-way set when it was switched: `Cell::get` returns a **copy**, and copying
/// the set on every classification cost more than the map lookup the cache
/// exists to avoid (a ~2 % regression on `retain.ts`). That argument is
/// stronger here, not weaker — the table is far larger than the set was.
/// Access is single-threaded by construction: the cell is thread-local and no
/// path holds a reference across a call that could re-enter classification.
// `repr(C)` for field ORDER, not for FFI: the four fields a lookup touches are
// declared first so they share one cache line. Under `repr(Rust)` the layout is
// unspecified and the 192-byte `ways` array — dead weight in the table arm —
// may be placed in front of them, which would make the spec's "one bounds
// compare and one load" two lines' worth of traffic. `align(64)` is what makes
// that claim true rather than likely: at the struct's natural 8-byte alignment
// the hot group could straddle two lines depending on where the thread-local
// block lands.
#[repr(C, align(64))]
pub(super) struct PageGenerationCacheSet {
    // ---- the table: everything `lookup` reads, in one line ----
    /// `ARM_UNRESOLVED` / `ARM_TABLE` / `ARM_WAYS`. See the constants above for
    /// why the arm is a field and not the `OnceLock` read.
    arm: u8,
    /// First class covered. Meaningful only when `table` is non-empty.
    base: usize,
    /// Bumped on every invalidation; an entry is live only if its `epoch`
    /// matches. Starts at 1 so a zeroed entry is never live.
    epoch: u64,
    /// Entries for classes `base .. base + table.len()`.
    table: Vec<PageClassEntry>,
    /// Counted unconditionally (a field increment on a `&mut` we already hold)
    /// and reported only under `PERRY_GC_DIAG`. This is the falsifier: the
    /// table's whole claim is that the miss rate collapses. Both arms count,
    /// so the control arm carries the same increment and the comparison is
    /// symmetric.
    hits: u64,
    misses: u64,
    // ---- cold: written on the miss path or rarer ----
    /// Misses the authoritative map could answer, i.e. misses that cached
    /// something. `misses - inserts` is the population that is in no
    /// registered block at all — the 22 % the bounds check is supposed to
    /// reject for free.
    inserts: u64,
    /// Rebases performed and inserts refused past the cap — the two paths the
    /// span measurement could not rule out.
    rebases: u64,
    refused: u64,
    /// Lookups that missed because the key was OUTSIDE `[base, base + len)`.
    /// See the increment site for why this is the counter that matters.
    oos: u64,
    // ---- control arm: the 4-way round-robin set, unchanged ----
    ways: [PageGenerationCache; PAGE_GENERATION_CACHE_WAYS],
    /// Round-robin victim for the next insert.
    next: usize,
}

impl PageGenerationCacheSet {
    pub(super) const fn empty() -> Self {
        Self {
            arm: ARM_UNRESOLVED,
            base: 0,
            epoch: 1,
            table: Vec::new(),
            hits: 0,
            misses: 0,
            inserts: 0,
            rebases: 0,
            refused: 0,
            oos: 0,
            ways: [PageGenerationCache::empty(); PAGE_GENERATION_CACHE_WAYS],
            next: 0,
        }
    }

    #[inline(always)]
    pub(super) fn lookup(&mut self, key: usize, addr: usize) -> Option<PageGenerationRange> {
        if self.arm != ARM_WAYS {
            // `wrapping_sub` folds `key < base` into the same out-of-range
            // check as `key >= base + len`: a key below the base wraps to a
            // huge index and fails `< len`.
            let idx = key.wrapping_sub(self.base);
            if idx < self.table.len() {
                let e = &self.table[idx];
                if e.epoch == self.epoch && e.range.contains(addr) {
                    self.hits += 1;
                    return Some(e.range);
                }
            } else {
                // Miss-path only, so it costs nothing on a hit — and it is the
                // counter that decides between the two explanations for a
                // residual miss rate. Out of span: the key is a candidate
                // address in no registered block (the population the table was
                // never able to hold, since the map has no answer to cache
                // either). In span: the table itself failed — a class holding
                // more than one range, or invalidation churn.
                self.oos += 1;
            }
            self.misses += 1;
            return None;
        }
        for way in self.ways.iter() {
            if way.valid && way.key == key && way.range.contains(addr) {
                self.hits += 1;
                return Some(way.range);
            }
        }
        self.misses += 1;
        None
    }

    #[inline]
    pub(super) fn insert(&mut self, key: usize, range: PageGenerationRange) {
        if self.arm == ARM_UNRESOLVED {
            // The one env read, on the cold path, once per thread.
            self.arm = if page_class_table_enabled() {
                ARM_TABLE
            } else {
                ARM_WAYS
            };
        }
        if self.arm == ARM_TABLE {
            if self.table.is_empty() {
                // The base is taken from the FIRST insert, minus slack. Never a
                // constant: the arena's placement moves with ASLR.
                self.base = key.saturating_sub(PAGE_CLASS_TABLE_BASE_SLACK);
                self.table = vec![PageClassEntry::DEAD; PAGE_CLASS_TABLE_INITIAL_SPAN];
            }
            let mut idx = key.wrapping_sub(self.base);
            if idx >= self.table.len() {
                if !self.rebase_to_cover(key) {
                    // Past the cap: leave it uncached. The caller already has
                    // the authoritative answer and returns it; only the
                    // acceleration is forgone.
                    self.refused += 1;
                    return;
                }
                idx = key - self.base;
            }
            self.table[idx] = PageClassEntry {
                range,
                epoch: self.epoch,
            };
            self.inserts += 1;
            return;
        }
        self.inserts += 1;
        let slot = self.next % PAGE_GENERATION_CACHE_WAYS;
        self.ways[slot] = PageGenerationCache {
            key,
            range,
            valid: true,
        };
        self.next = slot.wrapping_add(1);
    }

    /// Grow the table so that `key` is inside it, keeping every class it
    /// already covered. Returns false — and changes nothing — if the resulting
    /// span would exceed the cap.
    #[cold]
    #[inline(never)]
    fn rebase_to_cover(&mut self, key: usize) -> bool {
        let old_lo = self.base;
        let old_hi = self.base + self.table.len(); // exclusive
        let new_lo = old_lo.min(key.saturating_sub(PAGE_CLASS_TABLE_BASE_SLACK));
        let new_hi = old_hi.max(key.saturating_add(1 + PAGE_CLASS_TABLE_BASE_SLACK));
        let span = new_hi - new_lo;
        if span > PAGE_CLASS_TABLE_MAX_SPAN {
            return false;
        }
        // Entries are a cache; dropping them is always correct. Rebasing by
        // bumping the epoch rather than copying keeps this simple and it is
        // rare — measured span growth was 1,018 -> 1,021 over two whole runs.
        self.epoch = self.epoch.wrapping_add(1);
        self.table = vec![PageClassEntry::DEAD; span];
        self.base = new_lo;
        self.rebases += 1;
        true
    }

    /// Invalidate everything, both arms. O(1) for the table: an epoch bump
    /// makes every entry stale at once, which is the same contract the 4-way
    /// set met by being reset wholesale — and the reason the table can meet it
    /// without touching ~2,000 entries.
    ///
    /// The bump is the whole of the table's correctness. Without it a retagged
    /// block keeps answering with its previous generation, which is a
    /// misclassified pointer: the collector treats an old object as young, or
    /// declines to trace a young one. `a_registration_change_invalidates_every_entry`
    /// is the standing guard.
    #[inline]
    pub(super) fn invalidate(&mut self) {
        self.ways = [PageGenerationCache::empty(); PAGE_GENERATION_CACHE_WAYS];
        self.next = 0;
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            // Wrapped: 0 is the "never live" sentinel a zeroed entry carries,
            // so step past it. Reaching this needs 2^64 invalidations; the
            // branch is here so the sentinel cannot be forged rather than
            // because the wrap is expected.
            self.epoch = 1;
        }
    }

    /// `(hits, misses, inserts, span, rebases, refused)` for the diagnostic
    /// line and for the tests.
    fn stats(&self) -> PageClassStats {
        PageClassStats {
            arm: self.arm,
            hits: self.hits,
            misses: self.misses,
            inserts: self.inserts,
            span: self.table.len(),
            rebases: self.rebases,
            refused: self.refused,
            oos: self.oos,
        }
    }
}

/// What [`PageGenerationCacheSet::stats`] reports. A named struct rather than a
/// tuple because the report and four tests read different fields of it and a
/// six-tuple's positions are not self-describing at the call site.
#[derive(Clone, Copy)]
struct PageClassStats {
    arm: u8,
    hits: u64,
    misses: u64,
    inserts: u64,
    span: usize,
    rebases: u64,
    refused: u64,
    oos: u64,
}

/// `PERRY_GC_PAGE_CLASS_TABLE=0` restores the 4-way set. The kill switch, and
/// the positive control: both arms live in ONE binary so no build difference
/// can be confounded with the change.
#[inline(always)]
pub(super) fn page_class_table_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| crate::gc::env_default_on_enabled("PERRY_GC_PAGE_CLASS_TABLE"))
}

/// One line under `PERRY_GC_DIAG=1`, emitted per copying minor from the
/// collector (never at exit — the rig SIGKILLs the process).
pub(crate) fn page_class_table_report() {
    if !crate::gc::gc_diag_enabled() {
        return;
    }
    // SAFETY: thread-local, single-threaded, shared borrow ends here.
    let st = unsafe { (*hot_page_generation_cache()).stats() };
    let tot = st.hits + st.misses;
    if tot == 0 {
        return;
    }
    // `misses - inserts` is the population in no registered block at all: the
    // map had no answer either, so nothing was cached. Reported apart because
    // the two halves are removed by different properties of the table — the
    // first by capacity, the second by the bounds check.
    let unregistered = st.misses.saturating_sub(st.inserts);
    let arm_name = match st.arm {
        ARM_WAYS => "4way",
        ARM_TABLE => "table",
        // Never inserted, so never resolved: report what it WOULD pick.
        _ if page_class_table_enabled() => "table(unresolved)",
        _ => "4way(unresolved)",
    };
    eprintln!(
        "[gc-page-class] arm={} lookups={tot} hit={} ({:.3}%) miss={} ({:.3}%) \
miss_registered={} miss_unregistered={} miss_out_of_span={} span={} rebases={} refused={}",
        arm_name,
        st.hits,
        100.0 * st.hits as f64 / tot as f64,
        st.misses,
        100.0 * st.misses as f64 / tot as f64,
        st.inserts,
        unregistered,
        st.oos,
        st.span,
        st.rebases,
        st.refused,
    );
}

#[cfg(test)]
mod page_class_table_tests {
    //! The direct-indexed page-class table, pinned at the two points the span
    //! measurement could not settle. A wrong answer from this structure is a
    //! misclassified pointer — a collector that moves the wrong thing — so
    //! each path has a test that fails when its fallback is removed.
    use super::*;

    fn fresh<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        // Thread-local table, thread-local map: a fresh thread is a fresh world.
        std::thread::spawn(f)
            .join()
            .expect("page-class table test panicked")
    }

    fn table_stats() -> PageClassStats {
        // SAFETY: thread-local, single-threaded, borrow ends here.
        unsafe { (*hot_page_generation_cache()).stats() }
    }

    const MB: usize = 1 << GENERATION_CLASS_SHIFT;

    /// The base is taken from the FIRST registration, wherever it is — not
    /// from a constant. An arena that starts at a high address (ASLR moved the
    /// base by 0x142920 classes between two measured runs) must hit the table,
    /// not fall through to the map forever.
    ///
    /// Sabotage: hard-wire `self.base = 0` in `insert` — the classification
    /// still returns the right generation (the map is authoritative) but every
    /// lookup misses, and this test fails on the hit counter.
    #[test]
    fn base_is_taken_from_the_first_registration_not_a_constant() {
        if !page_class_table_enabled() {
            return;
        }
        fresh(|| {
            // Far from zero, and not 1 MiB-aligned so the key math is exercised.
            let base = 0x5f0_0000_0000usize + 0x3_8000;
            register_block_space(base, MB, HeapGeneration::Old, HeapSpace::Old);
            let inside = base + 0x1234;
            // First classification: a miss that fills the entry.
            assert_eq!(classify_heap_generation(inside), HeapGeneration::Old);
            let before = table_stats();
            assert!(before.span > 0, "the first insert must allocate the table");
            assert_eq!(
                (before.rebases, before.refused),
                (0, 0),
                "a base derived from the first registration must cover that \
                 registration in the initial table — no rebase, no refusal"
            );
            // Second: MUST be a table hit.
            assert_eq!(classify_heap_generation(inside), HeapGeneration::Old);
            let after = table_stats();
            assert_eq!(
                after.hits,
                before.hits + 1,
                "a re-classification of a registered address must hit the table; \
                 a base that is not derived from the first registration leaves \
                 every key out of span and the table permanently cold"
            );
        });
    }

    /// A key OUTSIDE the current span must still classify correctly, through
    /// the authoritative map — either by rebasing the table to cover it or, past
    /// the cap, by falling through uncached. Both are exercised.
    ///
    /// Sabotage: in `insert`, replace the out-of-span branch with an unchecked
    /// `self.table[idx]` — the first assertion below panics on the bounds
    /// check, and a release build without bounds checks would write past the
    /// allocation. Or make `lookup` return the entry without `idx < len` — the
    /// far address then reads a garbage entry and this test's generation
    /// assertion fails.
    #[test]
    fn a_key_outside_the_span_still_classifies_correctly() {
        if !page_class_table_enabled() {
            return;
        }
        fresh(|| {
            let near = 0x6a0_0000_0000usize;
            register_block_space(near, MB, HeapGeneration::Old, HeapSpace::Old);
            assert_eq!(classify_heap_generation(near + 8), HeapGeneration::Old);
            let s0 = table_stats();
            assert_eq!(s0.span, PAGE_CLASS_TABLE_INITIAL_SPAN);

            // 1. Within the cap: a block 4,000 classes away. Must rebase and
            //    then hit.
            let far = near + 4_000 * MB;
            register_block_space(far, MB, HeapGeneration::Nursery, HeapSpace::NurseryEden);
            assert_eq!(
                classify_heap_generation(far + 8),
                HeapGeneration::Nursery,
                "an out-of-span key must classify through the map"
            );
            let s1 = table_stats();
            assert_eq!(
                s1.rebases,
                s0.rebases + 1,
                "a key inside the cap must rebase the table"
            );
            assert!(s1.span > s0.span, "rebasing must widen the span");
            assert_eq!(s1.refused, 0);
            // And the ORIGINAL block is still answered correctly after rebase.
            assert_eq!(classify_heap_generation(near + 8), HeapGeneration::Old);
            let h_before = table_stats().hits;
            assert_eq!(classify_heap_generation(far + 8), HeapGeneration::Nursery);
            assert_eq!(
                table_stats().hits,
                h_before + 1,
                "after rebase the far key must hit"
            );

            // 2. Past the cap: 40,000 classes away. Must NOT rebase (the cap
            //    bounds the allocation) and must STILL classify correctly,
            //    uncached.
            let wild = near + 40_000 * MB;
            register_block_space(wild, MB, HeapGeneration::Longlived, HeapSpace::Old);
            assert_eq!(
                classify_heap_generation(wild + 8),
                HeapGeneration::Longlived,
                "a key past the cap must fall through to the map, not be dropped"
            );
            let s2 = table_stats();
            assert_eq!(
                s2.rebases, s1.rebases,
                "a key past the cap must not grow the table"
            );
            assert_eq!(s2.span, s1.span);
            assert!(s2.refused >= 1, "the refusal must be counted, not silent");
            // Classify it again: still correct, still uncached.
            assert_eq!(
                classify_heap_generation(wild + 8),
                HeapGeneration::Longlived
            );
        });
    }

    /// A key match is NOT an address match. Two ranges can share a 1 MiB class
    /// (`PageGenerationSlot::Multiple`); an entry confirmed for one must not
    /// answer for an address in the other.
    ///
    /// Sabotage: drop `e.range.contains(addr)` from `lookup` — the second
    /// classification returns the first range's generation for an address that
    /// is not in it.
    ///
    /// Deliberately NOT gated on the arm: it asserts only on classification
    /// results, which must hold whichever structure answers, so a run with
    /// `PERRY_GC_PAGE_CLASS_TABLE=0` exercises the 4-way control arm through
    /// this test. (The 4-way set can hold both ranges at once, in two ways
    /// under one key; the table holds the last-confirmed one and misses to the
    /// map for the other. Both are correct, which is what is pinned here.)
    #[test]
    fn a_hit_requires_range_containment_not_just_key_equality() {
        fresh(|| {
            // Two half-class ranges in the SAME class, different generations.
            let class_base = 0x7b0_0000_0000usize;
            let half = MB / 2;
            register_block_space(class_base, half, HeapGeneration::Old, HeapSpace::Old);
            register_block_space(
                class_base + half,
                half,
                HeapGeneration::Nursery,
                HeapSpace::NurseryEden,
            );
            assert_eq!(
                classify_heap_generation(class_base + 8),
                HeapGeneration::Old
            );
            // Same key, other half: the cached entry (Old) must NOT answer.
            assert_eq!(
                classify_heap_generation(class_base + half + 8),
                HeapGeneration::Nursery,
                "an entry for another range in the same class answered for this address"
            );
            assert_eq!(
                classify_heap_generation(class_base + 8),
                HeapGeneration::Old
            );
        });
    }

    /// Registration invalidates: a retagged block must never be answered from
    /// a stale entry. This is the 4-way set's original contract carried over.
    ///
    /// Sabotage: make `invalidate` a no-op for the table — the second
    /// classification returns the pre-retag generation.
    ///
    /// Also ungated on the arm: "a retag is never answered from a stale entry"
    /// is the contract of BOTH structures, and running it under
    /// `PERRY_GC_PAGE_CLASS_TABLE=0` is what keeps the control arm from
    /// rotting untested while the table is the default.
    #[test]
    fn a_registration_change_invalidates_every_entry() {
        fresh(|| {
            let base = 0x8c0_0000_0000usize;
            register_block_space(base, MB, HeapGeneration::Nursery, HeapSpace::NurseryEden);
            assert_eq!(classify_heap_generation(base + 8), HeapGeneration::Nursery);
            assert_eq!(classify_heap_generation(base + 8), HeapGeneration::Nursery); // cached
            unregister_block_generation(base, MB);
            register_block_space(base, MB, HeapGeneration::Old, HeapSpace::Old);
            assert_eq!(
                classify_heap_generation(base + 8),
                HeapGeneration::Old,
                "a stale table entry answered after the block was retagged"
            );
        });
    }
}
