use super::*;

pub(crate) const GENERATION_PAGE_SHIFT: usize = 12;
// Generation classification wants exact range answers, but it does
// not need a separate hash entry for every 4 KiB remembered-set card.
// A 1 MiB bucket matches the arena block scale, keeps lookup bounded,
// and avoids thousands of metadata entries for low-pressure nursery
// churn before the first GC.
pub(crate) const GENERATION_CLASS_SHIFT: usize = 20;
pub(crate) const GENERATION_PAGE_SIZE: usize = 1 << GENERATION_PAGE_SHIFT;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HeapGeneration {
    Unknown,
    Nursery,
    Longlived,
    Old,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HeapSpace {
    Unknown,
    NurseryEden,
    Survivor0,
    Survivor1,
    Longlived,
    Old,
    /// A young block that a copying minor decided to promote **whole, in
    /// place** (`arena/promote.rs`, #7742). Its generation is already `Old` —
    /// the write barrier, `remembered_child_needs_tracking` and
    /// `barrier_parent_needs_remembering` all see old-gen semantics from the
    /// instant the retag lands — but the space stays distinguishable for the
    /// duration of that one cycle so the copier can tell "must still be
    /// traced once, because it was young when the cycle began" from a
    /// genuinely old object it may skip. The finish walk retags it to
    /// [`HeapSpace::Old`] before the mutator runs again, so no mutator-visible
    /// classification ever observes this variant.
    PromotedYoung,
}

impl HeapSpace {
    #[inline]
    pub(crate) fn is_nursery(self) -> bool {
        matches!(
            self,
            HeapSpace::NurseryEden | HeapSpace::Survivor0 | HeapSpace::Survivor1
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PageGenerationRange {
    base: usize,
    end: usize,
    generation: HeapGeneration,
    space: HeapSpace,
    object_starts: *mut u64,
}

impl PageGenerationRange {
    #[inline]
    fn contains(self, addr: usize) -> bool {
        addr >= self.base && addr < self.end
    }

    /// The marker a NEGATIVE table entry carries: "no registered range covers
    /// any address in this 1 MiB class" (#9852).
    ///
    /// `base = usize::MAX, end = 0` makes [`contains`] false for every address
    /// without a branch of its own, so the hit arm is byte-for-byte the code it
    /// was before negatives existed and the marker test is reached only after
    /// `contains` has already failed — i.e. on the miss path only.
    ///
    /// [`contains`]: PageGenerationRange::contains
    const ABSENT: Self = Self {
        base: usize::MAX,
        end: 0,
        generation: HeapGeneration::Unknown,
        space: HeapSpace::Unknown,
        object_starts: std::ptr::null_mut(),
    };

    #[inline]
    fn is_absent(self) -> bool {
        self.base == usize::MAX && self.end == 0
    }
}

/// What a table probe concluded. Three states, not two, because "the table
/// knows there is nothing here" and "the table does not know" lead to
/// different code: the first answers the caller, the second falls through to
/// the authoritative map.
#[derive(Clone, Copy)]
enum ClassLookup {
    Hit(PageGenerationRange),
    /// A live NEGATIVE entry: no registered range covers this class, so the
    /// map has no answer either and does not need to be asked (#9852).
    KnownAbsent,
    Unknown,
}

#[derive(Clone, Debug)]
enum PageGenerationSlot {
    Single(PageGenerationRange),
    Multiple(Vec<PageGenerationRange>),
}

impl PageGenerationSlot {
    #[inline]
    fn find(&self, addr: usize) -> Option<PageGenerationRange> {
        match self {
            PageGenerationSlot::Single(range) => range.contains(addr).then_some(*range),
            PageGenerationSlot::Multiple(ranges) => {
                ranges.iter().copied().find(|range| range.contains(addr))
            }
        }
    }

    fn insert(&mut self, range: PageGenerationRange) {
        match self {
            PageGenerationSlot::Single(existing) => {
                if *existing == range {
                    return;
                }
                *self = PageGenerationSlot::Multiple(vec![*existing, range]);
            }
            PageGenerationSlot::Multiple(ranges) => {
                if !ranges.contains(&range) {
                    ranges.push(range);
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
struct PageGenerationCache {
    key: usize,
    range: PageGenerationRange,
    valid: bool,
}

impl PageGenerationCache {
    const fn empty() -> Self {
        Self {
            key: 0,
            range: PageGenerationRange {
                base: 0,
                end: 0,
                generation: HeapGeneration::Unknown,
                space: HeapSpace::Unknown,
                object_starts: std::ptr::null_mut(),
            },
            valid: false,
        }
    }
}

/// Ways in the [`PageGenerationCacheSet`] below.
///
/// #7469: this was a **one**-entry cache, and the write barrier classifies at
/// least two unrelated addresses per store — the child being written and the
/// object being written into. On `churn.ts` those live in different 1 MiB
/// generation classes, so consecutive classifications evicted each other and
/// the "cache" missed on essentially every call: 71 self samples in the
/// authoritative map lookup that the cache exists to avoid. Four ways covers
/// the barrier's working set (child, parent, array payload, header) with room
/// to spare; it is still a fixed-size array probed linearly, so a hit is a few
/// compares off one cache line.
// Four ways, measured: widening to 16 removed the 1.5% of misses the ECS
// command path took (`classify_heap_generation_uncached`) but the longer
// linear scan cost every barrier and array-receiver classification more than
// that — an 8.6% regression on the same row (0/7 pairs). Keep the scan short.
const PAGE_GENERATION_CACHE_WAYS: usize = 4;

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

/// The negative cache's arm (#9852), resolved separately from `arm` because it
/// is a separate kill switch: the table can be on with negatives off, which is
/// the control arm for this change. `NEG_UNRESOLVED` stores nothing, which is
/// correct for both settings.
const NEG_UNRESOLVED: u8 = 0;
const NEG_ON: u8 = 1;
const NEG_OFF: u8 = 2;

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
struct PageGenerationCacheSet {
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
    /// `NEG_UNRESOLVED` / `NEG_ON` / `NEG_OFF` — the negative cache's own kill
    /// switch (`PERRY_GC_PAGE_CLASS_NEGATIVE`), resolved once per thread on the
    /// cold store path for the same reason `arm` is a field and not an
    /// `OnceLock` read: this must not put an acquire load on a path that runs
    /// hundreds of millions of times per turn.
    neg_arm: u8,
    /// Lookups answered `KnownAbsent` from a live negative entry — map lookups
    /// that did not happen. This is the counter the change is judged on.
    neg_hits: u64,
    /// Negative entries written.
    neg_inserts: u64,
    /// MEASUREMENT ONLY (#9852). Splits the miss population the map could not
    /// answer into the two cases `pages.get(&key).and_then(|s| s.find(addr))`
    /// currently collapses:
    /// * `neg_class_absent` — `pages.get(&key)` is itself `None`: no
    ///   registered range covers this 1 MiB class. This is the ONLY sound
    ///   negative to cache at class granularity.
    /// * `neg_class_present` — the class exists in the map but no range in it
    ///   contains `addr`. Caching "absent" for the class would be WRONG for
    ///   the other addresses in it, so this population is not cacheable by the
    ///   proposed design.
    neg_class_absent: u64,
    neg_class_present: u64,
    // ---- control arm: the 4-way round-robin set, unchanged ----
    ways: [PageGenerationCache; PAGE_GENERATION_CACHE_WAYS],
    /// Round-robin victim for the next insert.
    next: usize,
}

impl PageGenerationCacheSet {
    const fn empty() -> Self {
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
            neg_arm: NEG_UNRESOLVED,
            neg_hits: 0,
            neg_inserts: 0,
            neg_class_absent: 0,
            neg_class_present: 0,
            ways: [PageGenerationCache::empty(); PAGE_GENERATION_CACHE_WAYS],
            next: 0,
        }
    }

    #[inline(always)]
    fn lookup(&mut self, key: usize, addr: usize) -> ClassLookup {
        if self.arm != ARM_WAYS {
            // `wrapping_sub` folds `key < base` into the same out-of-range
            // check as `key >= base + len`: a key below the base wraps to a
            // huge index and fails `< len`.
            let idx = key.wrapping_sub(self.base);
            if idx < self.table.len() {
                let e = &self.table[idx];
                if e.epoch == self.epoch {
                    if e.range.contains(addr) {
                        self.hits += 1;
                        return ClassLookup::Hit(e.range);
                    }
                    // Reached only after `contains` has failed, so the hit arm
                    // is unchanged and this costs nothing on a hit (#9852).
                    // A negative entry is stamped with the same epoch as a
                    // positive one, so the registration that would make it
                    // wrong invalidates it by the same bump.
                    if e.range.is_absent() {
                        self.neg_hits += 1;
                        return ClassLookup::KnownAbsent;
                    }
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
            return ClassLookup::Unknown;
        }
        for way in self.ways.iter() {
            if way.valid && way.key == key && way.range.contains(addr) {
                self.hits += 1;
                return ClassLookup::Hit(way.range);
            }
        }
        self.misses += 1;
        ClassLookup::Unknown
    }

    #[inline]
    fn insert(&mut self, key: usize, range: PageGenerationRange) {
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
            if self.place(key, range) {
                self.inserts += 1;
            }
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

    /// Write `range` into `key`'s slot, rebasing if the key is outside the
    /// current span. Returns whether it was stored; `false` means the span cap
    /// refused the key and the caller must fall through to the map, which it
    /// already can — only the acceleration is forgone. **Requires a non-empty
    /// table**: allocating one here would take the base from whatever key
    /// arrived first, and only a REGISTERED key is guaranteed to sit inside the
    /// arena's eventual span.
    #[inline]
    fn place(&mut self, key: usize, range: PageGenerationRange) -> bool {
        debug_assert!(!self.table.is_empty(), "place on an unallocated table");
        let mut idx = key.wrapping_sub(self.base);
        if idx >= self.table.len() {
            if !self.rebase_to_cover(key) {
                self.refused += 1;
                return false;
            }
            idx = key - self.base;
        }
        self.table[idx] = PageClassEntry {
            range,
            epoch: self.epoch,
        };
        true
    }

    /// Remember that NO registered range covers `key`'s 1 MiB class (#9852).
    ///
    /// Called only where the caller has just read `pages.get(&key)` as `None`.
    /// That is the whole soundness argument and it has to stay exact: a class
    /// that holds a range but does not contain this particular address is a
    /// DIFFERENT case, and caching "absent" for it would answer wrongly for
    /// every other address in the class. Measured on the compiled claude-code
    /// TUI, that second case is 16-29 % of the population, so this is not a
    /// theoretical distinction.
    ///
    /// Three preconditions, each one a way this could be wrong rather than
    /// merely slow:
    /// * **the table arm only** — the 4-way set has no negative concept and its
    ///   `lookup` does not test for one;
    /// * **a table that already exists** — see [`place`]: a negative key must
    ///   never be the one that fixes the base;
    /// * **the kill switch** — `PERRY_GC_PAGE_CLASS_NEGATIVE=0` stores nothing,
    ///   which turns the feature off completely because `lookup` can only
    ///   answer `KnownAbsent` from an entry this function wrote.
    ///
    /// Invalidation needs no new rule: the entry carries the current epoch, and
    /// all three `PageGenerationMap` mutation sites already end with
    /// `invalidate_generation_cache()`, which bumps it. A registration into
    /// this class is exactly the event that would make the entry wrong, and it
    /// is exactly the event that kills it.
    ///
    /// [`place`]: PageGenerationCacheSet::place
    #[inline]
    fn insert_negative(&mut self, key: usize) {
        if self.arm != ARM_TABLE || self.table.is_empty() {
            return;
        }
        if self.neg_arm == NEG_UNRESOLVED {
            // The one env read, on the cold path, once per thread.
            self.neg_arm = if page_class_negative_enabled() {
                NEG_ON
            } else {
                NEG_OFF
            };
        }
        if self.neg_arm != NEG_ON {
            return;
        }
        if self.place(key, PageGenerationRange::ABSENT) {
            self.neg_inserts += 1;
        }
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
    fn invalidate(&mut self) {
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

    /// MEASUREMENT ONLY (#9852). Called from the two `_uncached` arms when the
    /// authoritative map had no answer, with whether the CLASS was absent.
    #[inline]
    fn note_negative(&mut self, class_absent: bool) {
        if class_absent {
            self.neg_class_absent += 1;
        } else {
            self.neg_class_present += 1;
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
            neg_hits: self.neg_hits,
            neg_inserts: self.neg_inserts,
            neg_class_absent: self.neg_class_absent,
            neg_class_present: self.neg_class_present,
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
    neg_hits: u64,
    neg_inserts: u64,
    neg_class_absent: u64,
    neg_class_present: u64,
}

/// `PERRY_GC_PAGE_CLASS_TABLE=0` restores the 4-way set. The kill switch, and
/// the positive control: both arms live in ONE binary so no build difference
/// can be confounded with the change.
#[inline(always)]
fn page_class_table_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| crate::gc::env_default_on_enabled("PERRY_GC_PAGE_CLASS_TABLE"))
}

/// `PERRY_GC_PAGE_CLASS_NEGATIVE=0` stops the table remembering that a class
/// holds nothing (#9852), leaving every such lookup to the authoritative map
/// exactly as before. Mirrors `PERRY_GC_PAGE_CLASS_TABLE` deliberately: both
/// arms live in ONE binary, so the A/B has no build difference to confound it.
#[inline(always)]
fn page_class_negative_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| crate::gc::env_default_on_enabled("PERRY_GC_PAGE_CLASS_NEGATIVE"))
}

/// One line under `PERRY_GC_DIAG=1`, emitted per copying minor from the
/// collector (never at exit — the rig SIGKILLs the process).
pub(crate) fn page_class_table_report() {
    if !crate::gc::gc_diag_enabled() {
        return;
    }
    // SAFETY: thread-local, single-threaded, shared borrow ends here.
    let st = unsafe { (*hot_page_generation_cache()).stats() };
    // `neg_hits` are lookups too: they are answered without touching the map,
    // so counting them anywhere but the denominator would make the miss RATE
    // improve by shrinking what it is a rate of.
    let tot = st.hits + st.neg_hits + st.misses;
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
miss_registered={} miss_unregistered={} miss_out_of_span={} span={} rebases={} refused={} \
neg_hit={} ({:.3}%) neg_stored={} neg_class_absent={} neg_class_present={}",
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
        st.neg_hits,
        100.0 * st.neg_hits as f64 / tot as f64,
        st.neg_inserts,
        st.neg_class_absent,
        st.neg_class_present,
    );
}

/// #7187: this map used to carry a bespoke identity hasher (`write_usize`
/// stored the key verbatim). `HashMap` is hashbrown, which takes the bucket
/// index from the hash's LOW bits and the SIMD control byte from
/// `hash >> 57`. Keys here are `addr >> GENERATION_CLASS_SHIFT` — around 2^26
/// for a typical heap address — so the top seven bits were zero for **every**
/// entry in the table, and every group probe matched every occupied slot in
/// the group. Each of those matches costs a real key comparison (a scattered
/// load into a bucket) before the right one is found, on a lookup that the
/// write barrier performs several times per heap store.
///
/// `fast_hash::PtrHasher` is the project's existing answer to exactly this —
/// see its module doc, and the `mix(h) = h ^ (h >> 32)` avalanche step whose
/// comment records a 455 ms → 830 ms regression from omitting it. The map is
/// only ever point-queried (`get` / `get_mut` / `insert` / `remove`; the
/// `first_key..=last_key` loops walk key *ranges*, not the map), so iteration
/// order is not observable and this carries no determinism exposure.
type PageGenerationMap = crate::fast_hash::PtrHashMap<usize, PageGenerationSlot>;
type OldGenPageObjectMap = crate::fast_hash::PtrHashMap<usize, Vec<usize>>;
type OldGenPageMetaMap = crate::fast_hash::PtrHashMap<usize, OldPageMeta>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OldPageMeta {
    pub(crate) page_base: usize,
    pub(crate) page_end: usize,
    pub(crate) allocated_bytes: usize,
    pub(crate) live_bytes: usize,
    pub(crate) dead_bytes: usize,
    pub(crate) object_count: usize,
    pub(crate) live_object_count: usize,
    pub(crate) dead_object_count: usize,
    pub(crate) pinned_bytes: usize,
    pub(crate) pinned_object_count: usize,
    pub(crate) dirty_slots: usize,
    /// Epoch stamp for `dirty_slots` (#6181). `dirty_slots` is a per-cycle
    /// count that used to be zeroed for every old page at the start of each
    /// GC cycle by `old_pages_begin_gc_cycle` — an O(number of old pages)
    /// walk on every minor whose cost grows with old-gen size. Instead of
    /// the eager reset, each page records the epoch its `dirty_slots` was
    /// last stamped in; the cycle-start "reset" is now a single O(1) bump of
    /// the thread-local `OLD_GEN_PAGE_DIRTY_EPOCH`, and a page whose stamp is
    /// stale reads as zero (`effective_dirty_slots`). Mirrors the
    /// generation-counter lazy-invalidation pattern in `native_arena.rs`.
    pub(crate) dirty_slots_epoch: u64,
    pub(crate) dirty: bool,
    pub(crate) evacuation_eligible: bool,
}

impl OldPageMeta {
    #[inline]
    fn zero_for_page(page: usize) -> Self {
        let page_base = generation_page_base(page);
        Self {
            page_base,
            page_end: page_base + GENERATION_PAGE_SIZE,
            allocated_bytes: 0,
            live_bytes: 0,
            dead_bytes: 0,
            object_count: 0,
            live_object_count: 0,
            dead_object_count: 0,
            pinned_bytes: 0,
            pinned_object_count: 0,
            dirty_slots: 0,
            // Epoch 0 never matches a live cycle epoch (which starts at 1), so
            // a freshly materialized page reads zero dirty slots until it is
            // stamped by the remembered-set scan (#6181).
            dirty_slots_epoch: 0,
            dirty: false,
            evacuation_eligible: false,
        }
    }

    /// `dirty_slots` scoped to `current_epoch`: a page last stamped in an
    /// earlier cycle (its `dirty_slots_epoch` is stale) has no dirty slots
    /// this cycle. This is what makes the O(1) epoch bump in
    /// `old_pages_begin_gc_cycle` equivalent to the old per-page reset (#6181).
    #[inline]
    pub(crate) fn effective_dirty_slots(&self, current_epoch: u64) -> usize {
        if self.dirty_slots_epoch == current_epoch {
            self.dirty_slots
        } else {
            0
        }
    }

    #[inline]
    fn reset_cycle_sweep_accounting(&mut self) {
        self.live_bytes = 0;
        self.dead_bytes = 0;
        self.pinned_bytes = 0;
        self.live_object_count = 0;
        self.dead_object_count = 0;
        self.pinned_object_count = 0;
        self.evacuation_eligible = false;
    }

    #[inline]
    fn refresh_policy_bits(&mut self) {
        self.evacuation_eligible = self.allocated_bytes > 0
            && self.live_bytes > 0
            && self.dead_bytes > 0
            && self.pinned_bytes == 0;
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct OldPageSummary {
    pub(crate) pages: usize,
    pub(crate) allocated_bytes: usize,
    pub(crate) live_bytes: usize,
    pub(crate) dead_bytes: usize,
    pub(crate) reusable_bytes: usize,
    pub(crate) pooled_bytes: usize,
    pub(crate) returned_bytes: usize,
    pub(crate) pinned_bytes: usize,
    pub(crate) object_count: usize,
    pub(crate) live_object_count: usize,
    pub(crate) dead_object_count: usize,
    pub(crate) pinned_object_count: usize,
    pub(crate) dirty_pages: usize,
    pub(crate) dirty_slots: usize,
    pub(crate) fragmented_pages: usize,
    pub(crate) evacuation_eligible_pages: usize,
}

#[derive(Default)]
pub(crate) struct OldArenaSourceBlockSelection {
    pub(crate) block_indices: crate::fast_hash::PtrHashSet<usize>,
    pub(crate) pages: crate::fast_hash::PtrHashSet<usize>,
}

// `PAGE_GENERATIONS` and `PAGE_GENERATION_CACHE` stay raw because they are
// NAMED fields of `HotTls` (`page_generations`, `page_generation_cache`), whose
// providers below hand `tls_hot::fill` their addresses. A named field is one
// dependent load cheaper than a claimed slot, which is why the closed set
// exists; everything else in this file was simply never migrated.
thread_local! {
    static PAGE_GENERATIONS: RefCell<PageGenerationMap> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());

    static PAGE_GENERATION_CACHE: UnsafeCell<PageGenerationCacheSet> =
        const { UnsafeCell::new(PageGenerationCacheSet::empty()) };
}

crate::perry_thread_local! {
    static OLD_GEN_PAGE_OBJECTS: RefCell<OldGenPageObjectMap> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());

    static OLD_GEN_PAGE_META: RefCell<OldGenPageMetaMap> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());

    /// Promoted-block pages whose object list is DESCRIBED rather than stored —
    /// see [`register_promoted_page_run`].
    static OLD_GEN_PAGE_PROMOTED_RUNS: RefCell<crate::fast_hash::PtrHashMap<usize, PromotedPageRun>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());

    /// Monotone-within-a-window latch so the common case — a program that never
    /// promotes a block in place — pays one `Cell` read per reader instead of a
    /// hash probe per page. Same pattern as `PER_OBJECT_LAYOUTS_NONEMPTY`.
    static OLD_GEN_PAGE_PROMOTED_RUNS_NONEMPTY: Cell<bool> = const { Cell::new(false) };

    pub(crate) static OLD_GEN_RECLAIM_REUSABLE_BYTES: Cell<usize> = const { Cell::new(0) };
    pub(crate) static OLD_GEN_RECLAIM_POOLED_BYTES: Cell<usize> = const { Cell::new(0) };
    pub(crate) static OLD_GEN_RECLAIM_RETURNED_BYTES: Cell<usize> = const { Cell::new(0) };

    /// Monotonic per-cycle epoch for old-page `dirty_slots` (#6181). Bumped
    /// once per GC cycle by `old_pages_begin_gc_cycle` instead of walking every
    /// old page to zero its `dirty_slots`. Starts at 1 so a freshly allocated
    /// page (stamp 0, see `OldPageMeta::zero_for_page`) reads as having no
    /// dirty slots. `u64` never wraps in practice (one bump per collection).
    static OLD_GEN_PAGE_DIRTY_EPOCH: Cell<u64> = const { Cell::new(1) };
}

#[cfg(test)]
thread_local! {
    static OLD_PAGE_META_SNAPSHOT_CALLS: Cell<usize> = const { Cell::new(0) };
}

// --- #7469 hot-TLS address providers. See `crate::tls_hot`. ---

/// Address of this thread's `PAGE_GENERATION_CACHE`.
pub(crate) fn page_generation_cache_hot_addr() -> *mut u8 {
    PAGE_GENERATION_CACHE.with(|c| c.get() as *mut u8)
}

/// Address of this thread's `PAGE_GENERATIONS`.
pub(crate) fn page_generations_hot_addr() -> *mut u8 {
    PAGE_GENERATIONS.with(|p| p as *const _ as *mut u8)
}

/// [`PAGE_GENERATION_CACHE`] without a TLS resolution — see `crate::tls_hot`.
#[inline(always)]
fn hot_page_generation_cache() -> *mut PageGenerationCacheSet {
    // SAFETY: the slot is filled from `page_generation_cache_hot_addr` above,
    // and `tls_hot::tests::cached_addresses_match_thread_locals` asserts the
    // pairing.
    crate::tls_hot::hot().page_generation_cache as *mut PageGenerationCacheSet
}

/// [`PAGE_GENERATIONS`] without a TLS resolution — see `crate::tls_hot`.
#[inline(always)]
fn hot_page_generations() -> &'static RefCell<PageGenerationMap> {
    // SAFETY: as above, paired with `page_generations_hot_addr`.
    unsafe { &*(crate::tls_hot::hot().page_generations as *const RefCell<PageGenerationMap>) }
}

#[inline]
fn old_gen_page_dirty_epoch() -> u64 {
    OLD_GEN_PAGE_DIRTY_EPOCH.with(|epoch| epoch.get())
}

#[inline]
pub(crate) fn generation_page_for_addr(addr: usize) -> usize {
    addr >> GENERATION_PAGE_SHIFT
}

#[inline]
fn generation_class_key_for_addr(addr: usize) -> usize {
    addr >> GENERATION_CLASS_SHIFT
}

#[inline]
pub(crate) fn generation_page_base(page: usize) -> usize {
    page << GENERATION_PAGE_SHIFT
}

#[inline]
fn invalidate_generation_cache() {
    // Every way, not one — a stale way is exactly what this guards against.
    // SAFETY: thread-local, single-threaded.
    PAGE_GENERATION_CACHE.with(|cache| unsafe { (*cache.get()).invalidate() });
}

fn register_old_block_pages(base: usize, size: usize) {
    if base == 0 || size == 0 {
        return;
    }
    let end = base + size;
    let first_page = generation_page_for_addr(base);
    let last_page = generation_page_for_addr(end - 1);
    OLD_GEN_PAGE_META.with(|meta| {
        let mut meta = meta.borrow_mut();
        for page in first_page..=last_page {
            meta.entry(page)
                .or_insert_with(|| OldPageMeta::zero_for_page(page));
        }
    });
}

pub(crate) fn unregister_old_block_pages(pages: &[usize]) {
    if pages.is_empty() {
        return;
    }
    // #7624 REMOVER: a deferred entry for one of these pages must be folded in
    // BEFORE the page is dropped, or the flush would re-add it afterwards and
    // hand a later walk a header inside a recycled block.
    flush_deferred_old_page_registrations();
    OLD_GEN_PAGE_META.with(|meta| {
        let mut meta = meta.borrow_mut();
        for &page in pages {
            meta.remove(&page);
        }
    });
    OLD_GEN_PAGE_OBJECTS.with(|index| {
        let mut index = index.borrow_mut();
        for &page in pages {
            index.remove(&page);
        }
    });
    // RUN REMOVER: DISCARD, never expand — the block backing these pages is
    // going away, so a run's `first_header` no longer points at memory this
    // arena owns. Expanding would parse freed pages.
    if OLD_GEN_PAGE_PROMOTED_RUNS_NONEMPTY.with(Cell::get) {
        OLD_GEN_PAGE_PROMOTED_RUNS.with(|runs| {
            let mut runs = runs.borrow_mut();
            for &page in pages {
                runs.remove(&page);
            }
        });
    }
    // #7187 Phase B: the other place a page's dirty stamp stops existing — the
    // metadata entry itself is gone. A cached page whose metadata was dropped
    // is no longer a complete recording, so drop the cache.
    crate::gc::dirty_page_cache_invalidate();
}

#[inline]
pub(crate) fn address_span_overlaps_pages(
    start: usize,
    size: usize,
    pages: &crate::fast_hash::PtrHashSet<usize>,
) -> bool {
    if start == 0 || size == 0 || pages.is_empty() {
        return false;
    }
    let Some(end) = start.checked_add(size) else {
        return true;
    };
    let first_page = generation_page_for_addr(start);
    let last_page = generation_page_for_addr(end - 1);
    (first_page..=last_page).any(|page| pages.contains(&page))
}

#[cfg(test)]
pub(crate) fn register_block_space(
    base: usize,
    size: usize,
    generation: HeapGeneration,
    space: HeapSpace,
) {
    register_block_space_with_object_starts(base, size, generation, space, std::ptr::null_mut());
}

pub(crate) fn register_block_space_with_object_starts(
    base: usize,
    size: usize,
    generation: HeapGeneration,
    space: HeapSpace,
    object_starts: *mut u64,
) {
    if base == 0 || size == 0 || matches!(generation, HeapGeneration::Unknown) {
        return;
    }
    let end = base + size;
    let range = PageGenerationRange {
        base,
        end,
        generation,
        space,
        object_starts,
    };
    let first_key = generation_class_key_for_addr(base);
    let last_key = generation_class_key_for_addr(end - 1);
    PAGE_GENERATIONS.with(|pages| {
        let mut pages = pages.borrow_mut();
        for key in first_key..=last_key {
            match pages.entry(key) {
                Entry::Occupied(mut entry) => entry.get_mut().insert(range),
                Entry::Vacant(entry) => {
                    entry.insert(PageGenerationSlot::Single(range));
                }
            }
        }
    });
    if matches!(generation, HeapGeneration::Old) {
        register_old_block_pages(base, size);
    }
    invalidate_generation_cache();
}

/// Change the generation/space a block already registered at `base..base+size`
/// reports, **without** disturbing the old-page metadata that
/// [`unregister_block_generation`] would tear down.
///
/// Whole-block promotion (#7742) needs exactly this: the block's bytes do not
/// move, only their generation label does, and the sequence
/// `unregister_block_generation` + `register_block_space` is *not* equivalent —
/// the unregister half drops every `OLD_GEN_PAGE_META` / `OLD_GEN_PAGE_OBJECTS`
/// entry on those pages, which for the second (PromotedYoung → Old) retag would
/// throw away the object index the finish walk just built.
///
/// Only ranges whose `base`/`end` match exactly are retagged, so a block that
/// shares a 1 MiB generation class with its neighbours cannot relabel them.
pub(crate) fn retag_block_space(
    base: usize,
    size: usize,
    generation: HeapGeneration,
    space: HeapSpace,
) {
    retag_block_space_inner(base, size, generation, space, true);
}

/// Relabel a block WITHOUT minting the old-gen page-metadata entries an
/// `Old` retag normally mints (#7937).
///
/// For a retag that may be UNDONE — the speculative first-cycle promotion,
/// which decides from its own trace and rolls back if the trace disagrees —
/// eager registration is both wasted and expensive: it mints one zeroed
/// `OldPageMeta` per 4 KB of the whole young generation, and a `HashMap` never
/// gives its capacity back, so removing the entries afterwards does not undo
/// the footprint. Measured: `tree_wide` +6 MB, `tree` +3 MB, `churn` +1 MB of
/// peak RSS for blocks that were handed straight back to the nursery.
///
/// Deferring it is sound for that caller specifically because a speculative
/// attempt requires `skip_remembering` (see `gc::copying`), which is the proof
/// that NO remembered-set insertion can happen between the retag and the
/// finish — and the remembered set is the only reader that would want a
/// promoted page's metadata before it exists. If the attempt commits,
/// `finish_in_place_promotion` mints the entries on the way past: both
/// `register_promoted_page_headers`/`_run` (per live page) and its closing
/// `retag_block_space(.., Old, HeapSpace::Old)` (every page) create them.
pub(crate) fn retag_block_space_deferring_old_page_registration(
    base: usize,
    size: usize,
    generation: HeapGeneration,
    space: HeapSpace,
) {
    retag_block_space_inner(base, size, generation, space, false);
}

fn retag_block_space_inner(
    base: usize,
    size: usize,
    generation: HeapGeneration,
    space: HeapSpace,
    register_old_pages: bool,
) {
    if base == 0 || size == 0 || matches!(generation, HeapGeneration::Unknown) {
        return;
    }
    let end = base + size;
    let first_key = generation_class_key_for_addr(base);
    let last_key = generation_class_key_for_addr(end - 1);
    PAGE_GENERATIONS.with(|pages| {
        let mut pages = pages.borrow_mut();
        for key in first_key..=last_key {
            let Some(slot) = pages.get_mut(&key) else {
                continue;
            };
            match slot {
                PageGenerationSlot::Single(range) => {
                    if range.base == base && range.end == end {
                        range.generation = generation;
                        range.space = space;
                    }
                }
                PageGenerationSlot::Multiple(ranges) => {
                    for range in ranges.iter_mut() {
                        if range.base == base && range.end == end {
                            range.generation = generation;
                            range.space = space;
                        }
                    }
                }
            }
        }
    });
    if register_old_pages && matches!(generation, HeapGeneration::Old) {
        register_old_block_pages(base, size);
    }
    invalidate_generation_cache();
}

/// Bulk registration of a run of freshly-promoted old-gen objects that share
/// one 4 KiB page, in **address order**, onto a page whose object list this
/// promotion is the sole author of.
///
/// This is the whole-block-promotion twin of
/// [`flush_deferred_old_page_registrations_batch`]. It exists because that path
/// still pays, per object, an `entry(page)` hash lookup, a `contains` scan of
/// the page's pre-batch list and a `refresh_policy_bits` recompute — costs that
/// are amortisable when the caller knows, as here, that it is filling one page
/// once from a linear walk. `headers` is appended wholesale and the meta is
/// updated once for the whole run.
///
/// `bytes` is the number of bytes of the run that fall inside `page` (an object
/// straddling a page boundary contributes its overlap to each page it touches),
/// matching `update_old_page_meta_for_object`'s accounting exactly.
///
/// # The list is DESCRIBED, not stored
///
/// The run is recorded as `(first_header, last_header, count)` and expanded into
/// [`OLD_GEN_PAGE_OBJECTS`] only if some reader actually asks for this page —
/// see [`PromotedPageRun`]. The header addresses are recoverable by the same
/// linear parse `old_arena_walk_objects` already performs over every old block,
/// so storing them is storing a derivable fact.
///
/// Measured on `gc-handoff/bench/retain.ts` (2.11 M promoted objects): building
/// the per-object list was **6.7 ms of the 18.2 ms** `in_place_promotion` phase
/// and **20 MB of permanent RSS**, and `retain` never reads one of those pages.
/// On `retain_wide` (2.94 M) it was 22.9 ms of 34.1 and 28 MB.
pub(crate) fn register_promoted_page_run(
    page: usize,
    first_header: usize,
    last_header: usize,
    count: usize,
    bytes: usize,
) {
    if count == 0 {
        return;
    }
    // One promoted block authors a given page exactly once per cycle, but two
    // blocks can share a page when block bases are not page aligned. Expanding
    // the incumbent keeps "at most one pending run per page" true by
    // construction rather than by an alignment argument.
    //
    // Taken and expanded OUTSIDE the borrow: `expand_promoted_run` touches a
    // different thread-local today, and this keeps that from being a fact the
    // next edit has to re-derive before it can add one line to the expansion.
    let previous = OLD_GEN_PAGE_PROMOTED_RUNS.with(|runs| runs.borrow_mut().remove(&page));
    if let Some(previous) = previous {
        expand_promoted_run(page, previous);
    }
    OLD_GEN_PAGE_PROMOTED_RUNS.with(|runs| {
        runs.borrow_mut().insert(
            page,
            PromotedPageRun {
                first_header,
                last_header,
                count,
            },
        );
    });
    OLD_GEN_PAGE_PROMOTED_RUNS_NONEMPTY.with(|flag| flag.set(true));
    OLD_GEN_PAGE_META.with(|meta| {
        let mut meta = meta.borrow_mut();
        let page_meta = meta
            .entry(page)
            .or_insert_with(|| OldPageMeta::zero_for_page(page));
        page_meta.allocated_bytes = page_meta.allocated_bytes.saturating_add(bytes);
        page_meta.object_count = page_meta.object_count.saturating_add(count);
        page_meta.live_bytes = page_meta.live_bytes.saturating_add(bytes);
        page_meta.live_object_count = page_meta.live_object_count.saturating_add(count);
        page_meta.refresh_policy_bits();
    });
}

/// A promoted-block page whose object list has not been built.
///
/// `first_header`/`last_header` are INCLUSIVE header addresses of the first and
/// last object overlapping the page — the first may sit below the page base
/// (an object straddling in from the previous page), which is exactly the
/// population the eager list used to hold. Objects on an old block are
/// contiguous and ascending, so every object between those two also overlaps
/// the page: the pair is an exact description of the set, not an approximation.
///
/// # Only [`PromotionLiveness::AssumeAllLive`] pages may be described
///
/// The parse cannot reconstruct which objects were MARKED — `clear_marks` runs
/// after the promotion, so by expansion time the marks are gone. On a TRACED
/// promoting cycle the eager list therefore stays: it is the only place that
/// liveness exists. Restricting the description to the untraced path is what
/// makes "the parse yields exactly `count`" an exact claim rather than an
/// approximation, and it is the path that carries the cost anyway (4 of
/// `retain`'s 5 cycles, all of its promoted objects).
#[derive(Clone, Copy, Debug)]
struct PromotedPageRun {
    first_header: usize,
    last_header: usize,
    count: usize,
}

/// Expand one run into [`OLD_GEN_PAGE_OBJECTS`]. Caller must have already
/// removed it from [`OLD_GEN_PAGE_PROMOTED_RUNS`], so this cannot recurse and
/// cannot double-append.
///
/// The parse is the one `old_arena_walk_objects` performs — hop by
/// `GcHeader::size`, stop on an implausible one. That walker is the old-gen
/// sweep's own enumerator, so "an old block parses linearly by header size" is
/// not a new invariant this introduces; it is the invariant the sweep already
/// rests on. What this adds is a bound taken at promotion time, which is why
/// runs are expanded before anything can reshape the block — see
/// [`materialize_all_promoted_page_runs`].
fn expand_promoted_run(page: usize, run: PromotedPageRun) {
    use crate::gc::GcHeader;

    let mut headers = Vec::with_capacity(run.count);
    let mut addr = run.first_header;
    while addr <= run.last_header {
        let header = addr as *const GcHeader;
        let total = unsafe { (*header).size } as usize;
        if total < crate::gc::GC_HEADER_SIZE {
            break;
        }
        // Same filter the producer applied: a non-arena-walkable object is
        // hopped OVER, not indexed. Without this the expansion would hand
        // readers headers the eager list never contained.
        if crate::gc::gc_type_is_arena_walkable(unsafe { (*header).obj_type }) {
            headers.push(addr);
        }
        addr += total;
    }
    debug_assert_eq!(
        headers.len(),
        run.count,
        "a promoted page run did not re-parse to the object count recorded at \
         promotion (page {page:#x}, {:#x}..={:#x}). The block was reshaped while \
         its run was still pending, which means some path that frees, moves or \
         resizes an old-gen object reached it without expanding the run first.",
        run.first_header,
        run.last_header,
    );
    OLD_GEN_PAGE_OBJECTS.with(|index| {
        let mut index = index.borrow_mut();
        let slot = index.entry(page).or_insert_with(Vec::new);
        if slot.is_empty() {
            *slot = headers;
        } else {
            slot.extend_from_slice(&headers);
        }
    });
}

/// Expand any pending run covering one of `pages`.
///
/// Every reader of [`OLD_GEN_PAGE_OBJECTS`] must call this for the pages it is
/// about to read, in the same way #7624 makes every reader flush the deferred
/// registration buffer. `promoted_page_run_materialization_sites` enumerates
/// the obligation from the source so a new reader cannot silently skip it.
pub(crate) fn materialize_promoted_page_runs(pages: impl IntoIterator<Item = usize>) {
    if !OLD_GEN_PAGE_PROMOTED_RUNS_NONEMPTY.with(Cell::get) {
        return;
    }
    for page in pages {
        let run = OLD_GEN_PAGE_PROMOTED_RUNS.with(|runs| runs.borrow_mut().remove(&page));
        if let Some(run) = run {
            expand_promoted_run(page, run);
        }
    }
}

/// Expand every pending run.
///
/// Called before anything that can reshape an old-gen block — the full and
/// budgeted cycle constructors, whose sweep frees objects in place and whose
/// holes are then refilled by `old_free` with objects of a different size. A
/// run's `last_header` is an address remembered at promotion time; once
/// boundaries inside it can move, that address stops being a header boundary.
/// Expanding first keeps the run representation confined to the window in which
/// promoted blocks are immutable: between the promotion and the next old-gen
/// sweep.
pub(crate) fn materialize_all_promoted_page_runs() {
    if !OLD_GEN_PAGE_PROMOTED_RUNS_NONEMPTY.with(Cell::get) {
        return;
    }
    let pending: Vec<(usize, PromotedPageRun)> =
        OLD_GEN_PAGE_PROMOTED_RUNS.with(|runs| runs.borrow_mut().drain().collect());
    OLD_GEN_PAGE_PROMOTED_RUNS_NONEMPTY.with(|flag| flag.set(false));
    for (page, run) in pending {
        expand_promoted_run(page, run);
    }
}

/// Eager per-object registration for a promoted page whose liveness is
/// [`PromotionLiveness::Marked`] — a TRACED promoting cycle, where the marks
/// are the only record of which objects are live and they are cleared before
/// anything could re-derive them. Unchanged from the pre-description path.
pub(crate) fn register_promoted_page_headers(page: usize, headers: &[usize], bytes: usize) {
    if headers.is_empty() {
        return;
    }
    OLD_GEN_PAGE_OBJECTS.with(|index| {
        let mut index = index.borrow_mut();
        index
            .entry(page)
            .or_insert_with(Vec::new)
            .extend_from_slice(headers);
    });
    OLD_GEN_PAGE_META.with(|meta| {
        let mut meta = meta.borrow_mut();
        let page_meta = meta
            .entry(page)
            .or_insert_with(|| OldPageMeta::zero_for_page(page));
        page_meta.allocated_bytes = page_meta.allocated_bytes.saturating_add(bytes);
        page_meta.object_count = page_meta.object_count.saturating_add(headers.len());
        page_meta.live_bytes = page_meta.live_bytes.saturating_add(bytes);
        page_meta.live_object_count = page_meta.live_object_count.saturating_add(headers.len());
        page_meta.refresh_policy_bits();
    });
}

/// Pending runs, for tests that must prove the subject actually ran.
#[cfg(test)]
pub(crate) fn pending_promoted_page_runs() -> usize {
    OLD_GEN_PAGE_PROMOTED_RUNS.with(|runs| runs.borrow().len())
}

pub(crate) fn unregister_block_generation(base: usize, size: usize) {
    if base == 0 || size == 0 {
        return;
    }
    let end = base + size;
    let first_key = generation_class_key_for_addr(base);
    let last_key = generation_class_key_for_addr(end - 1);
    let mut removed_old_block = false;
    PAGE_GENERATIONS.with(|pages| {
        let mut pages = pages.borrow_mut();
        for key in first_key..=last_key {
            let mut remove_page = false;
            let mut replacement = None;
            if let Some(slot) = pages.get_mut(&key) {
                match slot {
                    PageGenerationSlot::Single(range) => {
                        if range.base == base && range.end == end {
                            removed_old_block |= matches!(range.generation, HeapGeneration::Old);
                            remove_page = true;
                        }
                    }
                    PageGenerationSlot::Multiple(ranges) => {
                        ranges.retain(|range| {
                            let remove = range.base == base && range.end == end;
                            if remove && matches!(range.generation, HeapGeneration::Old) {
                                removed_old_block = true;
                            }
                            !remove
                        });
                        if ranges.is_empty() {
                            remove_page = true;
                        } else if ranges.len() == 1 {
                            replacement = Some(PageGenerationSlot::Single(ranges[0]));
                        }
                    }
                }
            }
            if remove_page {
                pages.remove(&key);
            } else if let Some(slot) = replacement {
                pages.insert(key, slot);
            }
        }
    });
    if removed_old_block {
        let first_page = generation_page_for_addr(base);
        let last_page = generation_page_for_addr(end - 1);
        let old_pages_to_unregister: Vec<usize> = (first_page..=last_page).collect();
        unregister_old_block_pages(&old_pages_to_unregister);
    }
    invalidate_generation_cache();
}

/// #7469: split so the **cache-hit** arm is small enough to actually inline
/// into its callers. A single `js_write_barrier_slot` classifies twice (child
/// then parent) and `write_barrier_decoded_parent` classifies again; with the
/// miss path inlined alongside, the whole thing stayed out of line and each
/// call paid its own `_tlv_get_addr`. Out-of-lining the miss lets the hit arm
/// inline, and LLVM then CSEs the one remaining hot-TLS resolution across
/// every classification in the barrier.
#[inline(always)]
pub(crate) fn classify_heap_generation(addr: usize) -> HeapGeneration {
    if addr == 0 {
        return HeapGeneration::Unknown;
    }
    let key = generation_class_key_for_addr(addr);
    // Both tables come off the one cached hot-TLS base — this runs on every
    // `decode_heap_addr`, i.e. every heap store the write barrier sees, and was
    // the single largest `_tlv_get_addr` caller on `churn.ts` (116 of 653
    // attributed samples) precisely because it resolved two distinct
    // thread-locals per call.
    // SAFETY: thread-local, single-threaded, and the borrow ends here.
    match unsafe { (*hot_page_generation_cache()).lookup(key, addr) } {
        ClassLookup::Hit(range) => return range.generation,
        // The map's answer for a class with no registered range IS `Unknown`
        // (`pages.get(&key)` is `None` -> `HeapGeneration::Unknown`), so this
        // returns what the fall-through would have returned, without it.
        ClassLookup::KnownAbsent => return HeapGeneration::Unknown,
        ClassLookup::Unknown => {}
    }
    classify_heap_generation_uncached(addr, key)
}

/// Cache-miss arm of [`classify_heap_generation`]: consult the page map and
/// re-prime the one-entry cache.
#[inline(never)]
fn classify_heap_generation_uncached(addr: usize, key: usize) -> HeapGeneration {
    // MEASUREMENT ONLY (#9852): the `class_absent` flag splits the two cases
    // `pages.get(&key).and_then(...)` collapses. Computed inside the borrow,
    // counted after it drops.
    let (found, class_absent) = {
        let pages = hot_page_generations().borrow();
        match pages.get(&key) {
            None => (None, true),
            Some(slot) => (slot.find(addr), false),
        }
    };
    if found.is_none() {
        // SAFETY: thread-local, single-threaded, and the borrow ends here.
        unsafe {
            let set = &mut *hot_page_generation_cache();
            set.note_negative(class_absent);
            if class_absent {
                set.insert_negative(key);
            }
        }
    }
    if let Some(range) = found {
        // SAFETY: as above.
        unsafe { (*hot_page_generation_cache()).insert(key, range) };
        range.generation
    } else {
        HeapGeneration::Unknown
    }
}

#[inline]
pub(crate) fn classify_heap_space(addr: usize) -> HeapSpace {
    classify_heap_space_in_range(addr).map_or(HeapSpace::Unknown, |(space, _, _)| space)
}

/// [`classify_heap_space`] plus the base of the registered range `addr` fell
/// in.
///
/// The base is what lets a caller answer a SECOND classification — the one for
/// `addr - GC_HEADER_SIZE`, which every object-classifying path needs — with a
/// bounds compare instead of a second map lookup. A header is on the same
/// registered range as its user pointer for every real object (a block always
/// begins with a header, so a user pointer is never within `GC_HEADER_SIZE` of
/// a range base); the base is precisely the guard that keeps a *garbage*
/// candidate address at the very start of a range from turning into a read of
/// the unmapped page below it (#7742).
/// Split hit/miss exactly like [`classify_heap_generation`] above, and for the
/// same reason (#7469): with the map-lookup arm inlined alongside it, the whole
/// function stayed out of line and every call paid its own `_tlv_get_addr` for
/// the cache base. This one is the copying minor's inner loop —
/// `CopyingPointerSet::classify_arena` calls it once per visited slot — so on a
/// promotion-heavy cycle it runs millions of times per collection.
#[inline(always)]
pub(crate) fn classify_heap_space_in_range(addr: usize) -> Option<(HeapSpace, usize, *mut u64)> {
    if addr == 0 {
        return None;
    }
    let key = generation_class_key_for_addr(addr);
    // SAFETY: thread-local, single-threaded, and the borrow ends here.
    match unsafe { (*hot_page_generation_cache()).lookup(key, addr) } {
        ClassLookup::Hit(range) => return Some((range.space, range.base, range.object_starts)),
        // As above: `None` is what the map returns for a class it does not
        // hold, so the negative answers with the same value.
        ClassLookup::KnownAbsent => return None,
        ClassLookup::Unknown => {}
    }
    classify_heap_space_in_range_uncached(addr, key)
}

/// Cache-miss arm of [`classify_heap_space_in_range`].
#[inline(never)]
fn classify_heap_space_in_range_uncached(
    addr: usize,
    key: usize,
) -> Option<(HeapSpace, usize, *mut u64)> {
    // MEASUREMENT ONLY (#9852) — see `classify_heap_generation_uncached`.
    let (found, class_absent) = {
        let pages = hot_page_generations().borrow();
        match pages.get(&key) {
            None => (None, true),
            Some(slot) => (slot.find(addr), false),
        }
    };
    if found.is_none() {
        // SAFETY: thread-local, single-threaded, and the borrow ends here.
        unsafe {
            let set = &mut *hot_page_generation_cache();
            set.note_negative(class_absent);
            if class_absent {
                set.insert_negative(key);
            }
        }
    }
    let range = found?;
    // SAFETY: thread-local, single-threaded, and the borrow ends here.
    unsafe { (*hot_page_generation_cache()).insert(key, range) };
    Some((range.space, range.base, range.object_starts))
}

/// Record a newly initialized Map header in its owning block's exact-start
/// bitmap. Map is the only arena type whose tag can be fabricated by an
/// 8-aligned interior pointer and whose rewrite descriptor follows an external
/// payload pointer. Keeping all other allocations off this path avoids a
/// metadata read-modify-write on every bump allocation.
#[inline(always)]
pub(crate) fn record_arena_object_start(header_addr: usize, obj_type: u8) {
    if obj_type != crate::gc::GC_TYPE_MAP {
        return;
    }
    let Some((_space, range_base, bitmap)) = classify_heap_space_in_range(header_addr) else {
        debug_assert!(false, "arena allocation was not in a registered block");
        return;
    };
    debug_assert!(
        !bitmap.is_null(),
        "registered arena block has no object-start bitmap"
    );
    if bitmap.is_null() || header_addr < range_base {
        return;
    }
    let slot = (header_addr - range_base) >> super::OBJECT_START_SHIFT;
    let word = slot / u64::BITS as usize;
    let bit = slot % u64::BITS as usize;
    unsafe {
        *bitmap.add(word) |= 1u64 << bit;
    }
}

/// True only when `header_addr` is a recorded allocation boundary in the
/// registered block beginning at `range_base`.
#[inline(always)]
pub(crate) fn arena_header_is_object_start(
    header_addr: usize,
    range_base: usize,
    bitmap: *mut u64,
) -> bool {
    if bitmap.is_null() || header_addr < range_base {
        return false;
    }
    let relative = header_addr - range_base;
    if relative & ((1 << super::OBJECT_START_SHIFT) - 1) != 0 {
        return false;
    }
    let slot = relative >> super::OBJECT_START_SHIFT;
    let word = slot / u64::BITS as usize;
    let bit = slot % u64::BITS as usize;
    unsafe { *bitmap.add(word) & (1u64 << bit) != 0 }
}

pub(crate) fn old_object_page_overlaps(
    header_addr: usize,
    total_size: usize,
) -> Vec<(usize, usize)> {
    if header_addr == 0 || total_size == 0 {
        return Vec::new();
    }
    let object_end = header_addr + total_size;
    let first_page = generation_page_for_addr(header_addr);
    let last_page = generation_page_for_addr(object_end - 1);
    let mut overlaps = Vec::with_capacity(last_page - first_page + 1);
    for page in first_page..=last_page {
        let page_base = generation_page_base(page);
        let page_end = page_base + GENERATION_PAGE_SIZE;
        let overlap_start = header_addr.max(page_base);
        let overlap_end = object_end.min(page_end);
        if overlap_start < overlap_end {
            overlaps.push((page, overlap_end - overlap_start));
        }
    }
    overlaps
}

fn update_old_page_meta_for_object(page_updates: &[(usize, usize)], adding: bool) {
    if page_updates.is_empty() {
        return;
    }
    OLD_GEN_PAGE_META.with(|meta| {
        let mut meta = meta.borrow_mut();
        for &(page, bytes) in page_updates {
            let page_meta = meta
                .entry(page)
                .or_insert_with(|| OldPageMeta::zero_for_page(page));
            if adding {
                page_meta.allocated_bytes = page_meta.allocated_bytes.saturating_add(bytes);
                page_meta.object_count = page_meta.object_count.saturating_add(1);
            } else {
                page_meta.allocated_bytes = page_meta.allocated_bytes.saturating_sub(bytes);
                page_meta.object_count = page_meta.object_count.saturating_sub(1);
                if page_meta.allocated_bytes == 0 && page_meta.object_count == 0 {
                    page_meta.reset_cycle_sweep_accounting();
                }
            }
            page_meta.refresh_policy_bits();
        }
    });
}

pub(crate) fn register_old_object_pages(header_addr: usize, total_size: usize) {
    if header_addr == 0 || total_size == 0 {
        return;
    }
    let overlaps = old_object_page_overlaps(header_addr, total_size);
    let mut added_pages = Vec::with_capacity(overlaps.len());
    OLD_GEN_PAGE_OBJECTS.with(|index| {
        let mut index = index.borrow_mut();
        for &(page, bytes) in &overlaps {
            let headers = index.entry(page).or_insert_with(Vec::new);
            if !headers.contains(&header_addr) {
                headers.push(header_addr);
                added_pages.push((page, bytes));
            }
        }
    });
    update_old_page_meta_for_object(&added_pages, true);
}

// ---------------------------------------------------------------------------
// #7624: deferred old-object page registration.
//
// `register_old_object_pages` above is written for the OCCASIONAL old-gen
// birth it was introduced for. Per call it pays two `RefCell` borrows, two
// `Vec` allocations, a hash lookup, and a **linear `contains` scan of the
// page's object list** — and that scan grows as the page fills, so a burst of
// births into one 4 KiB page is quadratic in the objects it lands there.
//
// #7613's promote-on-first-copy made that path hot on ordinary workloads: a
// copying minor promotes straight into old-gen (`gc/copying.rs`'s `move_young`
// → `arena_alloc_gc_old`), so json_pipeline pushes ~113 MB of promotions per
// run through it. Deferring lets the whole burst be registered in ONE batch,
// where the per-page list length is captured once and the dedup scan only has
// to cover the entries that predate the batch — zero comparisons for the fresh
// pages a bump-allocated promotion burst actually fills.
//
// SOUNDNESS. The deferral is invisible because **every reader and every
// remover of `OLD_GEN_PAGE_OBJECTS`/`OLD_GEN_PAGE_META` flushes first**, so
// the index is exactly what eager registration would have left at each
// observation point. Ordering matters in both directions: a remover that ran
// before the flush would be a no-op and the flush would then RESURRECT a dead
// entry, which is why removers flush too and not only readers. The flush sites
// are enumerated in `deferred_registration_flush_sites` in `arena/tests.rs`,
// which fails if a new toucher of either table appears without one.
// ---------------------------------------------------------------------------

crate::perry_thread_local! {
    /// Old-object page registrations not yet folded into `OLD_GEN_PAGE_OBJECTS`.
    /// Entries are `(header_addr, total_size)`; nothing here is dereferenced, so
    /// a deferred entry never keeps an object alive and is not a GC root — and
    /// the flush discipline above means the buffer is provably EMPTY at every
    /// point a collector could observe it (asserted by
    /// `deferred_buffer_is_empty_after_every_cycle_constructor`).
    static DEFERRED_OLD_PAGE_REGISTRATIONS: RefCell<Vec<(usize, usize)>> =
        const { RefCell::new(Vec::new()) };
}

/// Bound on the buffer between flushes, and therefore on its resident
/// footprint: 16 B/entry × 8k = **128 KB**.
///
/// This started at 64k entries (1 MB), inherited from #7623 where the buffer
/// backed a different shape. Nothing here wanted 64k: the cap exists to amortise
/// the per-batch loop, 8k already does that ~8,000×, and since the flush is
/// allocation-free the extra batches cost only the loop entry. So it is sized
/// for the footprint it has to justify.
///
/// It was reduced while chasing a `gc-ratchet` `pinned_host` row —
/// `11_collect_at_depth.rss_bytes`, ~+1.07 MB above the pinned artifact — on the
/// theory that the row WAS this buffer. Two measurements later that theory is
/// dead twice over, and the cap had nothing to do with it:
///
/// 1. Shrinking the cap 8× (1 MB → 128 KB) moved the cell +16 KB in the WRONG
///    direction when it should have shed ~0.9 MB. The probe also promotes
///    **zero** objects, so this path is inert on it.
/// 2. Measuring **`origin/main` itself** on the same idle host settled it:
///    base reads 35,651,584 on that cell (+2.88% over the pinned artifact, just
///    under the band) against fix's 35,749,888. **fix is +98 KB over base, not
///    +1.07 MB** — the row is ~91% pre-existing drift between the artifact
///    (pinned at 0.5.1346) and current `main`, and base fails ten other
///    `pinned_host` RSS cells on its own.
///
/// The smaller cap is kept because 128 KB beats 1 MB on its own terms, not
/// because it fixed anything.
pub(crate) const DEFERRED_OLD_PAGE_REGISTRATION_CAP: usize = 8_192;

/// Record `header_addr`'s page registration for the next flush instead of
/// performing it now. Callers must be old-gen births; see the module comment
/// for why this is invisible to every consumer of the index.
#[inline]
pub(crate) fn defer_old_object_page_registration(header_addr: usize, total_size: usize) {
    if header_addr == 0 || total_size == 0 {
        return;
    }
    let full = DEFERRED_OLD_PAGE_REGISTRATIONS.with(|buf| {
        let mut buf = buf.borrow_mut();
        buf.push((header_addr, total_size));
        buf.len() >= DEFERRED_OLD_PAGE_REGISTRATION_CAP
    });
    if full {
        flush_deferred_old_page_registrations_batch();
    }
}

/// Make the page-objects index complete. Cheap (one thread-local read) when
/// nothing is pending, which is the case at all but a handful of GC-time calls.
#[inline]
pub(crate) fn flush_deferred_old_page_registrations() {
    if DEFERRED_OLD_PAGE_REGISTRATIONS.with(|buf| buf.borrow().is_empty()) {
        return;
    }
    flush_deferred_old_page_registrations_batch();
}

/// The batched drain. Equivalent to `register_old_object_pages` per entry, but
/// holding one borrow of each table for the whole batch and — the part that
/// removes the quadratic term — scanning only the portion of a page's object
/// list that PREDATES this batch.
///
/// Skipping the in-batch entries is sound because they are pairwise distinct:
/// each comes from a live allocation, and an address cannot be handed out twice
/// without an intervening free, which cannot happen without a flush (every
/// remover flushes). Hole reuse — the reason the dedup exists at all — hands
/// back an address registered BEFORE the batch, so it is still covered.
/// The batch also holds BOTH table borrows at once and applies the
/// `OLD_GEN_PAGE_META` update inline rather than staging it in a `Vec`. The two
/// thread-locals are distinct cells, so there is no aliasing; the payoff is that
/// a flush allocates nothing at all. That is not a micro-optimisation: measured
/// on the pinned host, staging the updates and re-growing the pending buffer
/// once per batch cost **+31 MB peak RSS** on json_pipeline 500k (63 flushes,
/// each re-growing a ~1 MB `Vec` from empty and freeing a ~1 MB staging `Vec`),
/// which is a regression the deferral does not need to pay.
#[cold]
fn flush_deferred_old_page_registrations_batch() {
    let mut pending =
        DEFERRED_OLD_PAGE_REGISTRATIONS.with(|buf| std::mem::take(&mut *buf.borrow_mut()));
    if pending.is_empty() {
        return;
    }
    OLD_GEN_PAGE_OBJECTS.with(|index| {
        let mut index = index.borrow_mut();
        OLD_GEN_PAGE_META.with(|meta| {
            let mut meta = meta.borrow_mut();
            // Entries arrive in allocation order, so consecutive ones share a
            // page; cache that page's pre-batch length across the run.
            let mut run_page: Option<usize> = None;
            let mut run_base_len: usize = 0;
            for &(header_addr, total_size) in &pending {
                let object_end = header_addr + total_size;
                let first_page = generation_page_for_addr(header_addr);
                let last_page = generation_page_for_addr(object_end - 1);
                for page in first_page..=last_page {
                    let page_base = generation_page_base(page);
                    let page_end = page_base + GENERATION_PAGE_SIZE;
                    let overlap_start = header_addr.max(page_base);
                    let overlap_end = object_end.min(page_end);
                    if overlap_start >= overlap_end {
                        continue;
                    }
                    let headers = index.entry(page).or_insert_with(Vec::new);
                    if run_page != Some(page) {
                        run_page = Some(page);
                        run_base_len = headers.len();
                    }
                    if headers[..run_base_len.min(headers.len())].contains(&header_addr) {
                        continue;
                    }
                    headers.push(header_addr);
                    // Identical to `update_old_page_meta_for_object(.., true)`
                    // for this one page, applied here so no staging Vec exists.
                    let page_meta = meta
                        .entry(page)
                        .or_insert_with(|| OldPageMeta::zero_for_page(page));
                    page_meta.allocated_bytes = page_meta
                        .allocated_bytes
                        .saturating_add(overlap_end - overlap_start);
                    page_meta.object_count = page_meta.object_count.saturating_add(1);
                    page_meta.refresh_policy_bits();
                }
            }
        });
    });
    // Hand the allocation back rather than dropping it, so the next burst
    // refills a buffer that is already 64k entries wide.
    pending.clear();
    DEFERRED_OLD_PAGE_REGISTRATIONS.with(|buf| {
        let mut buf = buf.borrow_mut();
        if buf.capacity() < pending.capacity() {
            *buf = pending;
        }
    });
}

/// Entries awaiting a flush. Tests only — the buffer is an implementation
/// detail everywhere else.
#[cfg(test)]
pub(crate) fn deferred_old_page_registrations_len() -> usize {
    DEFERRED_OLD_PAGE_REGISTRATIONS.with(|buf| buf.borrow().len())
}

#[allow(dead_code)]
pub(crate) fn unregister_old_object_pages(header_addr: usize, total_size: usize) {
    if header_addr == 0 || total_size == 0 {
        return;
    }
    // #7624 REMOVER: see `unregister_old_block_pages`. Removing before the
    // flush would leave the flush to resurrect this object.
    flush_deferred_old_page_registrations();
    let overlaps = old_object_page_overlaps(header_addr, total_size);
    // RUN REMOVER: as `old_arena_page_index_remove_object`.
    materialize_promoted_page_runs(overlaps.iter().map(|&(page, _)| page));
    let mut removed_pages = Vec::with_capacity(overlaps.len());
    OLD_GEN_PAGE_OBJECTS.with(|index| {
        let mut index = index.borrow_mut();
        for &(page, bytes) in &overlaps {
            let mut remove_page = false;
            if let Some(headers) = index.get_mut(&page) {
                if let Some(pos) = headers.iter().position(|&addr| addr == header_addr) {
                    headers.swap_remove(pos);
                    removed_pages.push((page, bytes));
                }
                remove_page = headers.is_empty();
            }
            if remove_page {
                index.remove(&page);
            }
        }
    });
    update_old_page_meta_for_object(&removed_pages, false);
}

pub(crate) fn old_pages_begin_gc_cycle() {
    // #7624 CYCLE START: all three cycle constructors route through here
    // (`gc/mod.rs`'s minor, `gc/cycle.rs`'s `new_full`, `gc/policy.rs`'s
    // budgeted), so every collection begins with a complete page-objects index
    // and an EMPTY deferral buffer.
    flush_deferred_old_page_registrations();
    // #6181: the per-page `dirty_slots` reset used to iterate every old page
    // here (O(old pages) on every minor, growing with old-gen size). It is now
    // a single epoch bump — a page whose `dirty_slots_epoch` predates the new
    // epoch reads as zero via `effective_dirty_slots`, and the scan re-stamps
    // it on first touch this cycle (`old_page_account_dirty_slot`).
    OLD_GEN_PAGE_DIRTY_EPOCH.with(|epoch| epoch.set(epoch.get().wrapping_add(1)));
    OLD_GEN_RECLAIM_REUSABLE_BYTES.with(|bytes| bytes.set(0));
    OLD_GEN_RECLAIM_POOLED_BYTES.with(|bytes| bytes.set(0));
    OLD_GEN_RECLAIM_RETURNED_BYTES.with(|bytes| bytes.set(0));
}

pub(crate) fn old_pages_reset_sweep_accounting() {
    // #7624 READER (`OLD_GEN_PAGE_META`): closes the promote → sweep window
    // inside a full cycle. The per-object accounting that follows calls
    // `refresh_policy_bits`, which reads `allocated_bytes`; flushing here means
    // it never recomputes a page's bits from a count that is missing this
    // cycle's promotions.
    flush_deferred_old_page_registrations();
    OLD_GEN_PAGE_META.with(|meta| {
        for page_meta in meta.borrow_mut().values_mut() {
            page_meta.reset_cycle_sweep_accounting();
        }
    });
}

pub(crate) fn old_page_account_swept_object(
    header_addr: usize,
    total_size: usize,
    live: bool,
    pinned: bool,
) {
    if header_addr == 0 || total_size == 0 {
        return;
    }
    let overlaps = old_object_page_overlaps(header_addr, total_size);
    if overlaps.is_empty() {
        return;
    }
    OLD_GEN_PAGE_META.with(|meta| {
        let mut meta = meta.borrow_mut();
        for (page, bytes) in overlaps {
            let page_meta = meta
                .entry(page)
                .or_insert_with(|| OldPageMeta::zero_for_page(page));
            if live {
                page_meta.live_bytes = page_meta.live_bytes.saturating_add(bytes);
                page_meta.live_object_count = page_meta.live_object_count.saturating_add(1);
                if pinned {
                    page_meta.pinned_bytes = page_meta.pinned_bytes.saturating_add(bytes);
                    page_meta.pinned_object_count = page_meta.pinned_object_count.saturating_add(1);
                }
            } else {
                page_meta.dead_bytes = page_meta.dead_bytes.saturating_add(bytes);
                page_meta.dead_object_count = page_meta.dead_object_count.saturating_add(1);
            }
            page_meta.refresh_policy_bits();
        }
    });
}

pub(crate) fn old_page_account_promoted_object(
    header_addr: usize,
    total_size: usize,
    pinned: bool,
) {
    if header_addr == 0 || total_size == 0 {
        return;
    }
    let overlaps = old_object_page_overlaps(header_addr, total_size);
    if overlaps.is_empty() {
        return;
    }
    OLD_GEN_PAGE_META.with(|meta| {
        let mut meta = meta.borrow_mut();
        for (page, bytes) in overlaps {
            let page_meta = meta
                .entry(page)
                .or_insert_with(|| OldPageMeta::zero_for_page(page));
            page_meta.live_bytes = page_meta.live_bytes.saturating_add(bytes);
            page_meta.live_object_count = page_meta.live_object_count.saturating_add(1);
            if pinned {
                page_meta.pinned_bytes = page_meta.pinned_bytes.saturating_add(bytes);
                page_meta.pinned_object_count = page_meta.pinned_object_count.saturating_add(1);
            }
            page_meta.refresh_policy_bits();
        }
    });
}

pub(crate) fn old_page_account_dirty_slot(slot_addr: usize) {
    if slot_addr == 0 {
        return;
    }
    old_page_account_dirty_slots(generation_page_for_addr(slot_addr), 1);
}

/// Batched form of [`old_page_account_dirty_slot`] for a run of `count` slots
/// already known to lie on `page`.
///
/// The dirty scan walks contiguous, ascending slot ranges, so ~512 consecutive
/// slots share one 4 KiB page. Per-slot this was one hash-map probe each; the
/// counter it maintains is per-page, so the run can be folded into a single
/// probe. Same epoch semantics as the per-slot form — the first run seen this
/// cycle re-stamps and starts the count over, later runs on the same page
/// accumulate (#6181).
pub(crate) fn old_page_account_dirty_slots(page: usize, count: usize) {
    if count == 0 {
        return;
    }
    let current_epoch = old_gen_page_dirty_epoch();
    OLD_GEN_PAGE_META.with(|meta| {
        if let Some(page_meta) = meta.borrow_mut().get_mut(&page) {
            if page_meta.dirty_slots_epoch == current_epoch {
                page_meta.dirty_slots = page_meta.dirty_slots.saturating_add(count);
            } else {
                page_meta.dirty_slots_epoch = current_epoch;
                page_meta.dirty_slots = count;
            }
        }
    });
}

pub(crate) fn old_page_summary() -> OldPageSummary {
    // #7624 READER (`OLD_GEN_PAGE_META`): a deferred registration also owes
    // this table an `allocated_bytes`/`object_count` update, so the summary
    // would under-report a mid-cycle promotion burst without the flush.
    flush_deferred_old_page_registrations();
    let current_epoch = old_gen_page_dirty_epoch();
    OLD_GEN_PAGE_META.with(|meta| {
        let meta = meta.borrow();
        let mut summary = OldPageSummary {
            pages: meta.len(),
            ..OldPageSummary::default()
        };
        for page_meta in meta.values() {
            summary.allocated_bytes = summary
                .allocated_bytes
                .saturating_add(page_meta.allocated_bytes);
            summary.live_bytes = summary.live_bytes.saturating_add(page_meta.live_bytes);
            summary.dead_bytes = summary.dead_bytes.saturating_add(page_meta.dead_bytes);
            summary.pinned_bytes = summary.pinned_bytes.saturating_add(page_meta.pinned_bytes);
            summary.object_count = summary.object_count.saturating_add(page_meta.object_count);
            summary.live_object_count = summary
                .live_object_count
                .saturating_add(page_meta.live_object_count);
            summary.dead_object_count = summary
                .dead_object_count
                .saturating_add(page_meta.dead_object_count);
            summary.pinned_object_count = summary
                .pinned_object_count
                .saturating_add(page_meta.pinned_object_count);
            let dirty_slots = page_meta.effective_dirty_slots(current_epoch);
            if page_meta.dirty || dirty_slots > 0 {
                summary.dirty_pages = summary.dirty_pages.saturating_add(1);
            }
            summary.dirty_slots = summary.dirty_slots.saturating_add(dirty_slots);
            if page_meta.live_bytes > 0 && page_meta.dead_bytes > 0 {
                summary.fragmented_pages = summary.fragmented_pages.saturating_add(1);
            }
            if page_meta.evacuation_eligible {
                summary.evacuation_eligible_pages =
                    summary.evacuation_eligible_pages.saturating_add(1);
            }
        }
        summary.reusable_bytes = OLD_GEN_RECLAIM_REUSABLE_BYTES.with(|bytes| bytes.get());
        summary.pooled_bytes = OLD_GEN_RECLAIM_POOLED_BYTES.with(|bytes| bytes.get());
        summary.returned_bytes = OLD_GEN_RECLAIM_RETURNED_BYTES.with(|bytes| bytes.get());
        summary
    })
}

pub(crate) fn old_page_meta_snapshot() -> Vec<OldPageMeta> {
    #[cfg(test)]
    OLD_PAGE_META_SNAPSHOT_CALLS.with(|calls| calls.set(calls.get().saturating_add(1)));
    // #7624 READER (`OLD_GEN_PAGE_META`): this one drives real policy —
    // `gc/oldgen_defrag.rs` selects evacuation pages from it.
    flush_deferred_old_page_registrations();
    let current_epoch = old_gen_page_dirty_epoch();
    OLD_GEN_PAGE_META.with(|meta| {
        let mut snapshot = meta
            .borrow()
            .values()
            .copied()
            .map(|page_meta| normalize_dirty_slots_for_epoch(page_meta, current_epoch))
            .collect::<Vec<_>>();
        snapshot.sort_unstable_by_key(|page_meta| page_meta.page_base);
        snapshot
    })
}

#[cfg(test)]
pub(crate) fn old_page_meta_snapshot_calls_for_tests() -> usize {
    OLD_PAGE_META_SNAPSHOT_CALLS.with(Cell::get)
}

#[cfg(test)]
pub(crate) fn reset_old_page_meta_snapshot_calls_for_tests() {
    OLD_PAGE_META_SNAPSHOT_CALLS.with(|calls| calls.set(0));
}

/// Fold a stale `dirty_slots` stamp down to the effective value so a copied
/// `OldPageMeta` handed to a caller always reports this cycle's dirty-slot
/// count directly in `dirty_slots`, without the caller needing the epoch (#6181).
#[inline]
fn normalize_dirty_slots_for_epoch(mut page_meta: OldPageMeta, current_epoch: u64) -> OldPageMeta {
    page_meta.dirty_slots = page_meta.effective_dirty_slots(current_epoch);
    page_meta.dirty_slots_epoch = current_epoch;
    page_meta
}

/// Address ranges of the live old-generation blocks, as
/// `(base, end_exclusive, global_block_index, size)` sorted by base.
///
/// Old-gen memory is released a BLOCK at a time (`old_arena_reclaim_*`), so a
/// pass that wants its bytes back has to reason in blocks. `OldPageMeta` is
/// page-granular and carries no block identity, which is why #9772's selection
/// could predict 44 MB of "releasable block bytes" from page granules and
/// release nothing.
pub(crate) fn old_arena_block_ranges() -> Vec<(usize, usize, usize, usize)> {
    let old_block_start = longlived_end();
    OLD_ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        let mut out: Vec<(usize, usize, usize, usize)> = arena
            .blocks
            .iter()
            .enumerate()
            .filter_map(|(i, block)| {
                if block.data.is_null() || block.size == 0 {
                    return None;
                }
                let base = block.data as usize;
                Some((base, base + block.size, old_block_start + i, block.size))
            })
            .collect();
        out.sort_unstable_by_key(|r| r.0);
        out
    })
}

/// Index into [`old_arena_block_ranges`] output for the block containing
/// `addr`, or `None` when the address is not in a live old-gen block.
pub(crate) fn old_arena_block_range_index(
    ranges: &[(usize, usize, usize, usize)],
    addr: usize,
) -> Option<usize> {
    let idx = ranges.partition_point(|r| r.0 <= addr).checked_sub(1)?;
    (addr < ranges[idx].1).then_some(idx)
}

pub(crate) fn old_arena_source_blocks_for_pages(
    selected_pages: &crate::fast_hash::PtrHashSet<usize>,
) -> OldArenaSourceBlockSelection {
    let mut selection = OldArenaSourceBlockSelection::default();
    if selected_pages.is_empty() {
        return selection;
    }

    let old_block_start = longlived_end();
    OLD_ARENA.with(|arena| {
        let arena = unsafe { &*arena.get() };
        for (i, block) in arena.blocks.iter().enumerate() {
            if block.data.is_null() || block.size == 0 {
                continue;
            }
            let base = block.data as usize;
            let first_page = generation_page_for_addr(base);
            let last_page = generation_page_for_addr(base + block.size - 1);
            if !(first_page..=last_page).any(|page| selected_pages.contains(&page)) {
                continue;
            }

            selection.block_indices.insert(old_block_start + i);
            for page in first_page..=last_page {
                selection.pages.insert(page);
            }
        }
    });
    selection
}

pub(crate) fn old_arena_walk_objects_on_pages(
    pages: &crate::fast_hash::PtrHashSet<usize>,
    mut callback: impl FnMut(*mut u8),
) -> usize {
    if pages.is_empty() {
        return 0;
    }

    // #7624 READER: promotions land in old-gen mid-cycle (a copying minor's
    // root scan runs before the remembered-set walk), so this cannot rely on
    // the cycle-start flush alone.
    flush_deferred_old_page_registrations();
    // RUN READER: a promoted page's list is built on demand.
    materialize_promoted_page_runs(pages.iter().copied());

    let mut headers = Vec::new();
    let mut seen = crate::fast_hash::new_ptr_hash_set();
    OLD_GEN_PAGE_OBJECTS.with(|index| {
        let index = index.borrow();
        for page in pages {
            if let Some(page_headers) = index.get(page) {
                for &header_addr in page_headers {
                    if seen.insert(header_addr) {
                        headers.push(header_addr);
                    }
                }
            }
        }
    });

    let count = headers.len();
    for header_addr in headers {
        callback(header_addr as *mut u8);
    }
    count
}

pub(crate) struct OldArenaPageObjectCursor {
    pages: Vec<usize>,
    page_cursor: usize,
    header_cursor: usize,
}

impl OldArenaPageObjectCursor {
    pub(crate) fn new(pages: &crate::fast_hash::PtrHashSet<usize>) -> Self {
        // #7624 READER: same obligation as `old_arena_walk_objects_on_pages`.
        // The cursor is stepped incrementally by the budgeted cycle, which
        // marks but never allocates into old-gen, so nothing can accumulate
        // between `new` and the last `next`; `next` debug-asserts that rather
        // than paying a thread-local check per object.
        flush_deferred_old_page_registrations();
        // RUN READER: same obligation, same window — the stepping window
        // promotes nothing, so expanding every page's run once here covers
        // every `next`.
        materialize_promoted_page_runs(pages.iter().copied());
        Self {
            pages: pages.iter().copied().collect(),
            page_cursor: 0,
            header_cursor: 0,
        }
    }

    pub(crate) fn next(&mut self) -> Option<usize> {
        // #7624: `new` flushed; the stepping window must not re-dirty the
        // buffer, or this reader would be walking a stale index. Debug-only so
        // the per-object read costs nothing in a shipped collector.
        debug_assert!(
            DEFERRED_OLD_PAGE_REGISTRATIONS.with(|buf| buf.borrow().is_empty()),
            "an old-gen birth happened while a page-object cursor was stepping; \
             this reader is now walking a stale index (#7624)"
        );
        loop {
            let page = *self.pages.get(self.page_cursor)?;
            let header = OLD_GEN_PAGE_OBJECTS.with(|index| {
                index
                    .borrow()
                    .get(&page)
                    .and_then(|headers| headers.get(self.header_cursor).copied())
            });
            if let Some(header) = header {
                self.header_cursor += 1;
                return Some(header);
            }
            self.page_cursor += 1;
            self.header_cursor = 0;
        }
    }

    pub(crate) fn is_done(&self) -> bool {
        self.page_cursor >= self.pages.len()
    }
}

pub(crate) fn old_arena_page_index_remove_object(header_addr: usize, total_size: usize) {
    if header_addr == 0 || total_size == 0 {
        return;
    }
    // #7624 REMOVER: see `unregister_old_block_pages`.
    flush_deferred_old_page_registrations();
    let overlaps = old_object_page_overlaps(header_addr, total_size);
    if overlaps.is_empty() {
        return;
    }
    // RUN REMOVER: a removal against a page whose list is still described
    // would silently no-op, and the later expansion would resurrect the
    // object. Expand first, then remove from the real list.
    materialize_promoted_page_runs(overlaps.iter().map(|&(page, _)| page));
    OLD_GEN_PAGE_OBJECTS.with(|index| {
        let mut index = index.borrow_mut();
        for (page, _) in overlaps {
            let mut remove_page = false;
            if let Some(headers) = index.get_mut(&page) {
                headers.retain(|&addr| addr != header_addr);
                remove_page = headers.is_empty();
            }
            if remove_page {
                index.remove(&page);
            }
        }
    });
}

/// Stamp `page`'s metadata dirty. Returns whether a metadata entry existed to
/// stamp: #7187 Phase B's "already dirty" cache may only remember a page whose
/// recording is complete in BOTH the modbuf and here, so it has to know.
pub(crate) fn old_page_mark_dirty(page: usize) -> bool {
    OLD_GEN_PAGE_META.with(|meta| {
        if let Some(page_meta) = meta.borrow_mut().get_mut(&page) {
            page_meta.dirty = true;
            true
        } else {
            false
        }
    })
}

pub(crate) fn old_page_clear_dirty(page: usize) {
    OLD_GEN_PAGE_META.with(|meta| {
        if let Some(page_meta) = meta.borrow_mut().get_mut(&page) {
            page_meta.dirty = false;
        }
    });
    // #7187 Phase B: one of the two places a page's `dirty` stamp can go false,
    // so one of the places the barrier's cached page can stop being a complete
    // recording. Invalidating here rather than at the callers covers the GC's
    // own clear loop and the tests that reach for this directly.
    crate::gc::dirty_page_cache_invalidate();
}

#[cfg(test)]
pub(crate) fn old_arena_page_index_clear_for_tests() {
    // Wiping page metadata makes real old-arena objects unclassifiable —
    // stand the #6179 differential verifier down for this thread's test.
    crate::gc::CLASSIFIER_VERIFY_SUPPRESSED.with(|c| c.set(true));
    // #7624: DISCARD rather than flush — a caller asking for an empty index
    // would get a repopulated one if the pending burst were folded in first.
    DEFERRED_OLD_PAGE_REGISTRATIONS.with(|buf| buf.borrow_mut().clear());
    OLD_GEN_PAGE_OBJECTS.with(|index| index.borrow_mut().clear());
    // DISCARD pending runs for the same reason the deferral buffer is
    // discarded: a caller asking for an empty index must not get a
    // repopulated one at the next read.
    OLD_GEN_PAGE_PROMOTED_RUNS.with(|runs| runs.borrow_mut().clear());
    OLD_GEN_PAGE_PROMOTED_RUNS_NONEMPTY.with(|flag| flag.set(false));
}

#[cfg(test)]
pub(crate) fn old_page_meta_for_tests(page: usize) -> Option<OldPageMeta> {
    // #7624 READER: same rule as `old_page_summary`/`old_page_meta_snapshot`,
    // so a test that allocates and then inspects a page sees what eager
    // registration would have left.
    flush_deferred_old_page_registrations();
    let current_epoch = old_gen_page_dirty_epoch();
    OLD_GEN_PAGE_META.with(|meta| {
        meta.borrow()
            .get(&page)
            .copied()
            .map(|page_meta| normalize_dirty_slots_for_epoch(page_meta, current_epoch))
    })
}

#[cfg(test)]
mod page_generation_hasher_tests {
    use super::*;
    use std::collections::HashSet;
    use std::hash::BuildHasher;

    /// #7187 regression guard for `PageGenerationMap`'s hasher.
    ///
    /// `HashMap` is hashbrown: the bucket index comes from the hash's low bits,
    /// but the SIMD control byte — the filter that decides whether a group
    /// probe needs a real key comparison — is `hash >> 57`. Generation class
    /// keys are `addr >> GENERATION_CLASS_SHIFT`, so an identity hasher (which
    /// this map carried until #7187) produces a value around 2^26 whose top
    /// seven bits are zero for **every** key in the table. Every occupied slot
    /// in a probed group then matches, and each match costs a scattered load
    /// plus a key comparison — on a lookup the write barrier performs several
    /// times per heap store.
    ///
    /// This asserts the property directly rather than asserting "we call
    /// `PtrHasher`": reinstating any non-mixing hasher collapses the control
    /// byte to a single value and fails here.
    #[test]
    fn control_byte_is_spread_across_generation_class_keys() {
        let map = PageGenerationMap::default();
        let build = map.hasher();

        // Realistic 48-bit heap addresses, one per 1 MiB generation bucket —
        // the exact key population `classify_heap_generation` looks up.
        let base: usize = 0x0000_7f31_0000_0000;
        let control_bytes: HashSet<u64> = (0..64)
            .map(|i| {
                let addr = base + i * (1usize << GENERATION_CLASS_SHIFT);
                (build.hash_one(generation_class_key_for_addr(addr)) >> 57) & 0x7f
            })
            .collect();

        assert!(
            control_bytes.len() >= 32,
            "hashbrown control byte must vary across generation class keys, got {} \
             distinct values from 64 consecutive buckets (an identity hasher yields 1)",
            control_bytes.len()
        );
    }

    /// The bucket index (low bits) must stay well spread too — mixing that put
    /// all the entropy in the high bits and left the low bits constant would
    /// trade a control-byte collision for a far worse bucket collision. This is
    /// the failure `fast_hash`'s `mix` step exists for.
    #[test]
    fn bucket_index_is_spread_across_generation_class_keys() {
        let map = PageGenerationMap::default();
        let build = map.hasher();

        let base: usize = 0x0000_7f31_0000_0000;
        let low_bits: HashSet<u64> = (0..64)
            .map(|i| {
                let addr = base + i * (1usize << GENERATION_CLASS_SHIFT);
                build.hash_one(generation_class_key_for_addr(addr)) & 0x3f
            })
            .collect();

        assert!(
            low_bits.len() >= 32,
            "bucket index must vary across generation class keys, got {} distinct \
             values from 64 consecutive buckets",
            low_bits.len()
        );
    }

    /// The map must still answer correctly after the hasher change — a
    /// point-query round trip over many buckets, which is the only way this map
    /// is ever used.
    #[test]
    fn point_queries_round_trip_across_many_buckets() {
        let mut map = PageGenerationMap::default();
        let base: usize = 0x0000_7f31_0000_0000;
        for i in 0..256usize {
            let addr = base + i * (1usize << GENERATION_CLASS_SHIFT);
            map.insert(
                generation_class_key_for_addr(addr),
                PageGenerationSlot::Single(PageGenerationRange {
                    base: addr,
                    end: addr + (1 << GENERATION_CLASS_SHIFT),
                    generation: HeapGeneration::Old,
                    space: HeapSpace::Old,
                    object_starts: std::ptr::null_mut(),
                }),
            );
        }
        for i in 0..256usize {
            let addr = base + i * (1usize << GENERATION_CLASS_SHIFT);
            let found = map
                .get(&generation_class_key_for_addr(addr))
                .and_then(|slot| slot.find(addr + 0x40))
                .expect("every inserted bucket must be found by point query");
            assert_eq!(found.generation, HeapGeneration::Old);
            assert_eq!(found.base, addr);
        }
        assert_eq!(map.len(), 256);
    }
}

/// `PERRY_GC_CENSUS`: estimated bytes held by the per-page side tables.
pub(crate) fn page_meta_census() -> Vec<crate::gc::census::SideTableRow> {
    use crate::gc::census::{hash_table_bytes, vec_bytes};
    let mut rows = Vec::new();
    PAGE_GENERATIONS.with(|m| {
        let m = m.borrow();
        rows.push((
            "arena.page_generations",
            m.len(),
            hash_table_bytes(
                m.capacity(),
                std::mem::size_of::<(usize, PageGenerationSlot)>(),
            ),
        ));
    });
    OLD_GEN_PAGE_OBJECTS.with(|m| {
        let m = m.borrow();
        let inner: usize = m.values().map(vec_bytes).sum();
        rows.push((
            "arena.old_gen_page_objects",
            m.len(),
            hash_table_bytes(m.capacity(), std::mem::size_of::<(usize, Vec<usize>)>()) + inner,
        ));
    });
    OLD_GEN_PAGE_META.with(|m| {
        let m = m.borrow();
        rows.push((
            "arena.old_gen_page_meta",
            m.len(),
            hash_table_bytes(m.capacity(), std::mem::size_of::<(usize, OldPageMeta)>()),
        ));
    });
    OLD_GEN_PAGE_PROMOTED_RUNS.with(|m| {
        let m = m.borrow();
        rows.push((
            "arena.old_gen_page_promoted_runs",
            m.len(),
            hash_table_bytes(
                m.capacity(),
                std::mem::size_of::<(usize, PromotedPageRun)>(),
            ),
        ));
    });
    rows
}

#[cfg(test)]
mod block_range_tests {
    use super::old_arena_block_range_index;

    /// `old_arena_block_range_index` is the whole reason #9772's selection can
    /// group pages by block, so it gets a test that can fail: gaps between
    /// blocks must not be attributed to the block below them.
    #[test]
    fn block_range_lookup_respects_gaps_and_ends() {
        // Two 1 MiB blocks with a 1 MiB hole between them.
        let ranges = vec![
            (0x1000_0000, 0x1010_0000, 7, 0x10_0000),
            (0x1020_0000, 0x1030_0000, 9, 0x10_0000),
        ];
        assert_eq!(old_arena_block_range_index(&ranges, 0x1000_0000), Some(0));
        assert_eq!(old_arena_block_range_index(&ranges, 0x100F_FFFF), Some(0));
        // One past the end of block 0 is the gap, not block 0.
        assert_eq!(old_arena_block_range_index(&ranges, 0x1010_0000), None);
        assert_eq!(old_arena_block_range_index(&ranges, 0x1018_0000), None);
        assert_eq!(old_arena_block_range_index(&ranges, 0x1020_0000), Some(1));
        assert_eq!(old_arena_block_range_index(&ranges, 0x102F_FFFF), Some(1));
        // Above every block, and below every block.
        assert_eq!(old_arena_block_range_index(&ranges, 0x1030_0000), None);
        assert_eq!(old_arena_block_range_index(&ranges, 0x0FFF_FFFF), None);
        assert_eq!(old_arena_block_range_index(&[], 0x1000_0000), None);
    }
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

    /// #9852. A repeated classification of an address whose 1 MiB class holds
    /// no registered range must be answered from the table, not from the map.
    ///
    /// Asserted on `neg_hits`, not on the returned value: the value is
    /// `Unknown` whether or not the negative was consulted, so a test that
    /// checked only the answer would pass with the whole feature deleted.
    ///
    /// Sabotage: drop the `set.insert_negative(key)` call in
    /// `classify_heap_generation_uncached` — the second classification is a
    /// miss again and `neg_hits` stays 0, failing here.
    #[test]
    fn a_class_with_no_registered_range_is_answered_from_the_negative_entry() {
        if !page_class_table_enabled() || !page_class_negative_enabled() {
            return;
        }
        fresh(|| {
            // A registration first: the base must come from a REGISTERED key.
            let base = 0x9d0_0000_0000usize;
            register_block_space(base, MB, HeapGeneration::Old, HeapSpace::Old);
            assert_eq!(classify_heap_generation(base + 8), HeapGeneration::Old);
            // An address in a neighbouring class that holds nothing at all.
            let absent = base + 4 * MB + 0x40;
            assert_eq!(classify_heap_generation(absent), HeapGeneration::Unknown);
            let after_first = table_stats();
            assert_eq!(
                after_first.neg_inserts, 1,
                "a class the map does not hold must leave a negative entry"
            );
            let before = after_first.neg_hits;
            assert_eq!(classify_heap_generation(absent), HeapGeneration::Unknown);
            assert_eq!(
                table_stats().neg_hits,
                before + 1,
                "the second classification of an absent class went to the map; \
                 the negative entry was not consulted"
            );
            // And the same class answers `classify_heap_space_in_range` too.
            assert_eq!(classify_heap_space_in_range(absent + 8), None);
            assert_eq!(
                table_stats().neg_hits,
                before + 2,
                "the space classifier did not consult the negative entry"
            );
        });
    }

    /// #9852, the unsound case. A class that DOES hold a range, probed at an
    /// address outside it, must not be remembered as absent — the negative is
    /// class-granular and would then answer wrongly for the addresses the
    /// class really covers. Measured on cc, this case is 16-29 % of the
    /// population, so it is the common case and not a corner.
    ///
    /// Sabotage: call `insert_negative` unconditionally instead of under
    /// `if class_absent` — the final assertion reads `Unknown` for an address
    /// in a registered half-block.
    #[test]
    fn a_partially_registered_class_is_never_remembered_as_absent() {
        if !page_class_table_enabled() || !page_class_negative_enabled() {
            return;
        }
        fresh(|| {
            let class_base = 0xae0_0000_0000usize;
            let half = MB / 2;
            // Only the FIRST half of the class is registered.
            register_block_space(class_base, half, HeapGeneration::Old, HeapSpace::Old);
            // Probe the unregistered half: the class exists, the address is
            // not in its range.
            assert_eq!(
                classify_heap_generation(class_base + half + 8),
                HeapGeneration::Unknown
            );
            let st = table_stats();
            assert_eq!(
                st.neg_class_present, 1,
                "the probe must be counted as class-present, not class-absent"
            );
            assert_eq!(
                st.neg_inserts, 0,
                "a class that holds a range must never get a negative entry"
            );
            // The registered half must still classify correctly.
            assert_eq!(
                classify_heap_generation(class_base + 8),
                HeapGeneration::Old,
                "a negative cached for a partially registered class answered \
                 for an address the class really covers"
            );
        });
    }

    /// #9852. A negative is invalidated by the registration that makes it
    /// wrong, through the epoch bump every mutation site already performs. A
    /// stale negative is the worst failure this structure can produce: the
    /// collector declines to trace a live young object.
    ///
    /// Sabotage: stamp `epoch: 0` on the negative entry and it can never be
    /// live (this test still passes but the previous one fails); stamp a
    /// constant epoch instead of `self.epoch` and this one fails.
    #[test]
    fn a_negative_entry_is_killed_by_a_later_registration_in_that_class() {
        if !page_class_table_enabled() || !page_class_negative_enabled() {
            return;
        }
        fresh(|| {
            let base = 0xbf0_0000_0000usize;
            register_block_space(base, MB, HeapGeneration::Old, HeapSpace::Old);
            assert_eq!(classify_heap_generation(base + 8), HeapGeneration::Old);
            let later = base + 8 * MB;
            // Absent now, and remembered as absent.
            assert_eq!(classify_heap_generation(later + 8), HeapGeneration::Unknown);
            assert_eq!(table_stats().neg_inserts, 1);
            // Now that class really is registered.
            register_block_space(later, MB, HeapGeneration::Nursery, HeapSpace::NurseryEden);
            assert_eq!(
                classify_heap_generation(later + 8),
                HeapGeneration::Nursery,
                "a stale negative answered after the class was registered — \
                 the collector would decline to trace a live young object"
            );
        });
    }

    /// #9852. A negative must never be the insert that allocates the table:
    /// the base is derived from the first key stored, and only a REGISTERED
    /// key is guaranteed to sit inside the arena's eventual span. An
    /// unregistered candidate address can be anywhere at all.
    ///
    /// Sabotage: drop the `self.table.is_empty()` guard in `insert_negative` —
    /// the first assertion fails (a table exists with a base taken from a
    /// garbage address) and the registered address that follows is out of span.
    #[test]
    fn a_negative_never_fixes_the_tables_base() {
        if !page_class_table_enabled() || !page_class_negative_enabled() {
            return;
        }
        fresh(|| {
            // A wild candidate address, classified before anything is
            // registered — exactly what a conservative scan produces.
            let wild = 0x1_0000_0000_0000usize;
            assert_eq!(classify_heap_generation(wild), HeapGeneration::Unknown);
            let st = table_stats();
            assert_eq!(
                (st.span, st.neg_inserts),
                (0, 0),
                "a negative allocated the table and fixed its base from an \
                 unregistered address"
            );
            // The real arena, far away, must still be covered without a rebase.
            let base = 0xc10_0000_0000usize;
            register_block_space(base, MB, HeapGeneration::Old, HeapSpace::Old);
            assert_eq!(classify_heap_generation(base + 8), HeapGeneration::Old);
            assert_eq!(classify_heap_generation(base + 8), HeapGeneration::Old);
            let st = table_stats();
            assert!(st.span > 0, "the first registered insert must allocate");
            assert_eq!(
                (st.rebases, st.refused),
                (0, 0),
                "the base came from somewhere other than the first registration"
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
