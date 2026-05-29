use super::*;

thread_local! {
    pub(super) static MARK_SEEDS: std::cell::UnsafeCell<Vec<*mut GcHeader>> =
        std::cell::UnsafeCell::new(Vec::new());
}

pub(crate) struct ValidPointerSet {
    /// Insertion-side staging for arena entries — filled in ascending
    /// order by the address-sorted arena walk. Swapped into
    /// `merged_sorted` in `finalize()` so `enclosing_object` can do
    /// its interior-pointer floor-search. Malloc entries are *not*
    /// staged here: they are inserted directly into `lookup_set`
    /// during the malloc walk, bypassing the per-cycle
    /// `sort_unstable` + merge that dominated `build_valid_pointer_set`
    /// on promise-heavy kernels (5-6 % of total kernel time).
    pub(super) arena_sorted: Vec<usize>,
    /// Arena-only sorted vec, populated in `finalize()` by swapping
    /// `arena_sorted` in. Kept for `enclosing_object`'s
    /// interior-pointer floor-search (a lookup the hashset can't
    /// answer). Malloc objects (Closure, Promise, String, Map, Error,
    /// BigInt, Symbol) are deliberately omitted — every Perry runtime
    /// function that holds an interior pointer across user callbacks
    /// (`js_array_reduce`'s `elements_ptr = arr + 8`, etc.) does so
    /// against an arena-allocated array/buffer; malloc-tracked types
    /// are always accessed via their start (user pointer) and never
    /// give rise to interior-pointer probes. If that invariant ever
    /// changes, the malloc walk in `build_valid_pointer_set` must
    /// also populate `arena_sorted` (or a separate sorted vec).
    pub(super) merged_sorted: Vec<usize>,
    /// O(1) hash set for the hot `contains` path. Built from
    /// `merged_sorted` in `finalize()` with `PtrHasher` (Fibonacci-
    /// multiplicative on `usize`) — pointer keys are already well-
    /// distributed, so SipHash buys nothing and a single `mul` per
    /// lookup keeps the hash step out of the cache-miss budget. One
    /// cache miss per lookup (the bucket group) replaces the 17 cache
    /// misses of the binary-search path.
    pub(super) lookup_set: crate::fast_hash::PtrHashSet<usize>,
    // Min/max heap-pointer range across the merged set. Populated in
    // `finalize()`. The conservative stack scan calls `contains` once per
    // 8-byte stack word (~1024 calls per scanned KB of stack) and
    // `try_mark_value` calls it once per scanned root and once per
    // traced reference field. Most candidates that pass the NaN-tag
    // check are real heap pointers and DO fall inside the range,
    // so the prefilter mostly helps for the raw-pointer fallback path
    // where stack words may be return addresses / plain ints / spilled
    // function pointers. Cheap to maintain regardless.
    pub(super) range_min: usize,
    pub(super) range_max: usize,
    /// Bytes of logically tenured objects that are still physically
    /// resident in nursery blocks at collection entry. Populated while
    /// building the pointer set so evacuation policy Stage 1 doesn't
    /// need a second full arena walk on low-pressure cycles.
    pub(super) tenured_nursery_bytes: usize,
}

impl ValidPointerSet {
    pub(super) fn new(arena_capacity: usize, malloc_capacity: usize) -> Self {
        // Pre-size the hashset to the expected entry count so finalize
        // doesn't pay any rehash cost. hashbrown's growth threshold is
        // 7/8 of capacity, so multiplying by 2 leaves comfortable
        // headroom for both arena + malloc estimates.
        let est = arena_capacity + malloc_capacity;
        Self {
            arena_sorted: Vec::with_capacity(arena_capacity),
            merged_sorted: Vec::new(),
            lookup_set: std::collections::HashSet::with_capacity_and_hasher(
                est * 2,
                crate::fast_hash::PtrHasher,
            ),
            range_min: usize::MAX,
            range_max: 0,
            tenured_nursery_bytes: 0,
        }
    }
    /// Caller must guarantee that pushes happen in ascending address
    /// order — `ValidPointerSetBuilder` does so via `ArenaObjectCursor`
    /// in address order.
    pub(super) fn push_arena(&mut self, ptr: usize) {
        self.arena_sorted.push(ptr);
    }
    pub(super) fn record_tenured_nursery_bytes(&mut self, bytes: usize) {
        self.tenured_nursery_bytes += bytes;
    }
    pub(super) fn tenured_nursery_bytes(&self) -> usize {
        self.tenured_nursery_bytes
    }
    pub(super) fn finalize(&mut self) {
        // `merged_sorted` is arena-only — `build_valid_pointer_set`
        // direct-inserts malloc entries into `lookup_set`, so the
        // expensive `malloc_sorted.sort_unstable()` + merge pass that
        // dominated `build_valid_pointer_set` on
        // `promise_all_chains` (~30 ms × 3 cycles = ~90 ms total,
        // 5.78 % of kernel time) is gone. `enclosing_object` uses
        // `merged_sorted` for interior-pointer floor-search — see
        // `build_valid_pointer_set` for the correctness note that
        // restricts that lookup to arena objects.
        std::mem::swap(&mut self.merged_sorted, &mut self.arena_sorted);

        // Compute the `merged_sorted` (arena) range first, then
        // extend with the malloc range that was tracked separately
        // in `range_min` / `range_max` via the
        // `record_malloc_for_range` calls during the build. The
        // final `[range_min, range_max]` covers BOTH regions so
        // `maybe_contains` still prefilters correctly for malloc
        // pointers (closures/promises) that fall outside the
        // arena address span.
        if let (Some(&first), Some(&last)) = (self.merged_sorted.first(), self.merged_sorted.last())
        {
            if first < self.range_min {
                self.range_min = first;
            }
            if last > self.range_max {
                self.range_max = last;
            }
        }

        // Insert the arena entries into the unified `lookup_set`.
        // Malloc entries are already in there (inserted directly by
        // `build_valid_pointer_set`'s malloc walk). The hashset was
        // sized in `new()` to hold both regions without rehashing.
        self.lookup_set.extend(self.merged_sorted.iter().copied());
    }
    /// Track the address span of malloc entries so `maybe_contains`'s
    /// `[range_min, range_max]` prefilter still rejects out-of-range
    /// pointers correctly. `build_valid_pointer_set` calls this once
    /// per malloc user pointer alongside the direct `lookup_set.insert`.
    /// Cheap branch-free min/max update; no Vec materialization.
    #[inline(always)]
    pub(super) fn record_malloc_for_range(&mut self, ptr: usize) {
        if ptr < self.range_min {
            self.range_min = ptr;
        }
        if ptr > self.range_max {
            self.range_max = ptr;
        }
    }
    /// Cheap O(1) range-rejection prefilter. Most stack words and
    /// register spills are not heap pointers; if the candidate falls
    /// outside `[range_min, range_max]` it cannot match either region
    /// and we skip the binary search.
    #[inline(always)]
    pub(crate) fn maybe_contains(&self, ptr: usize) -> bool {
        ptr >= self.range_min && ptr <= self.range_max
    }
    #[inline]
    pub(crate) fn contains(&self, ptr: &usize) -> bool {
        if !self.maybe_contains(*ptr) {
            return false;
        }
        // O(1) hashset lookup. `lookup_set` is built in `finalize()`
        // with the same `PtrHasher` as the malloc-state registry, so a
        // single multiplicative mix + bucket probe replaces the
        // O(log n) binary search through `merged_sorted`. On
        // promise-heavy kernels this cuts `try_mark_value` from ~28 %
        // self-time to ~5–10 % — each call pays 1 cache miss for the
        // bucket group instead of ~log2(100k)=17 random misses through
        // the sorted Vec.
        self.lookup_set.contains(ptr)
    }

    /// Issue #73: interior-pointer lookup. Given a scanned word, find
    /// the heap object that encloses it (if any) and return its user
    /// pointer. This matters for runtime functions that derive
    /// `elements_ptr = arr + 8` or `data = buf + 8` and hold only the
    /// interior pointer while calling into user code. The conservative
    /// scan would otherwise see `arr + 8`, miss it (it's not at an
    /// object start), and let the GC sweep the backing object mid-
    /// iteration. Find the largest entry `<= query`, then validate via
    /// the GcHeader's size field.
    pub(crate) fn enclosing_object(&self, ptr: usize) -> Option<usize> {
        let candidate = Self::find_floor(&self.merged_sorted, ptr)?;
        unsafe {
            let header = (candidate as *const u8).sub(GC_HEADER_SIZE) as *const GcHeader;
            let total = (*header).size as usize;
            let payload_end = candidate + total.saturating_sub(GC_HEADER_SIZE);
            if ptr >= candidate && ptr < payload_end {
                Some(candidate)
            } else {
                None
            }
        }
    }

    pub(super) fn find_floor(sorted: &[usize], ptr: usize) -> Option<usize> {
        if sorted.is_empty() {
            return None;
        }
        let idx = sorted.partition_point(|&p| p <= ptr);
        if idx == 0 {
            return None;
        }
        Some(sorted[idx - 1])
    }
}

/// Build a set of all valid user-space pointers (pointers returned to callers).
/// Used to validate candidates found during conservative stack scanning.
pub(crate) fn build_valid_pointer_set() -> ValidPointerSet {
    let mut builder = ValidPointerSetBuilder::new();
    while !builder.step(usize::MAX) {}
    builder.finish()
}

pub(super) struct ValidPointerSetBuilder {
    set: ValidPointerSet,
    arena_cursor: Option<crate::arena::ArenaObjectCursor>,
    malloc_index: usize,
    arena_done: bool,
    finalized: bool,
}

impl ValidPointerSetBuilder {
    pub(super) fn new() -> Self {
        let malloc_count = MALLOC_STATE.with(|s| s.borrow().objects.len());
        // 48 bytes is a conservative under-estimate (smaller than the
        // typical 96-byte class instance) so the Vec doesn't realloc.
        let arena_estimate = crate::arena::arena_total_bytes() / 48;
        Self {
            set: ValidPointerSet::new(arena_estimate + 64, malloc_count + 64),
            arena_cursor: Some(crate::arena::ArenaObjectCursor::new(
                crate::arena::ArenaWalkOrder::Address,
            )),
            malloc_index: 0,
            arena_done: false,
            finalized: false,
        }
    }

    pub(super) fn step(&mut self, budget: usize) -> bool {
        if self.finalized {
            return true;
        }

        let mut remaining = budget;
        while remaining > 0 && !self.arena_done {
            let next = self
                .arena_cursor
                .as_mut()
                .and_then(crate::arena::ArenaObjectCursor::next);
            let Some((header_ptr, _block_idx)) = next else {
                self.arena_done = true;
                self.arena_cursor = None;
                break;
            };
            let user_ptr = unsafe { header_ptr.add(GC_HEADER_SIZE) };
            self.set.push_arena(user_ptr as usize);
            unsafe {
                let header = header_ptr as *const GcHeader;
                let flags = (*header).gc_flags;
                if flags & GC_FLAG_TENURED != 0
                    && flags & GC_FLAG_FORWARDED == 0
                    && crate::arena::pointer_in_nursery(user_ptr as usize)
                {
                    self.set
                        .record_tenured_nursery_bytes((*header).size as usize);
                }
            }
            remaining -= 1;
        }

        while remaining > 0 && self.arena_done {
            let maybe_header = MALLOC_STATE.with(|s| {
                let s = s.borrow();
                s.objects.get(self.malloc_index).copied()
            });
            let Some(header) = maybe_header else {
                self.set.finalize();
                self.finalized = true;
                return true;
            };
            let user_ptr = unsafe { (header as *mut u8).add(GC_HEADER_SIZE) };
            let addr = user_ptr as usize;
            self.set.lookup_set.insert(addr);
            self.set.record_malloc_for_range(addr);
            self.malloc_index += 1;
            remaining -= 1;
        }

        if self.arena_done {
            let malloc_len = MALLOC_STATE.with(|s| s.borrow().objects.len());
            if self.malloc_index >= malloc_len {
                self.set.finalize();
                self.finalized = true;
            }
        }

        self.finalized
    }

    pub(super) fn finish(mut self) -> ValidPointerSet {
        if !self.finalized {
            while !self.step(usize::MAX) {}
        }
        self.set
    }
}

pub(super) fn push_mark_seed(header: *mut GcHeader) {
    MARK_SEEDS.with(|cell| unsafe {
        (*cell.get()).push(header);
    });
}

#[inline]
pub(super) fn take_mark_seeds() -> Vec<*mut GcHeader> {
    MARK_SEEDS.with(|cell| unsafe { std::mem::take(&mut *cell.get()) })
}

#[inline]
pub(super) fn clear_mark_seeds() {
    MARK_SEEDS.with(|cell| unsafe {
        (*cell.get()).clear();
    });
}

#[inline]
pub(super) fn try_mark_value(value_bits: u64, valid_ptrs: &ValidPointerSet) -> bool {
    let tag = value_bits & TAG_MASK;
    // Hot-path tag rejection. POINTER_TAG / STRING_TAG / BIGINT_TAG are
    // the only NaN-tags that wrap a heap pointer; everything else
    // (UNDEFINED, NULL, FALSE, TRUE, INT32, SHORT_STRING, plain f64s,
    // raw integers) is rejected with a single non-equality cascade
    // that LLVM lowers to a switch.
    let is_heap_ptr = tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG;
    if !is_heap_ptr {
        return false;
    }
    let ptr_val = (value_bits & POINTER_MASK) as usize;
    if ptr_val == 0 {
        return false;
    }

    // Range short-circuit before paying for the binary search. Most
    // calls reject here on miss-prone inputs (e.g. NaN-boxed pointers
    // from objects allocated by previous test runs in the same process,
    // dead-store stack words pointing at freed regions). Saves ~2×
    // O(log n) per non-matching candidate.
    if !valid_ptrs.maybe_contains(ptr_val) {
        return false;
    }

    // Validate against known heap pointers. NaN-boxed pointers always
    // point at object starts (POINTER_TAG is stamped at box time on
    // the user pointer, never at an interior offset), so a direct
    // lookup suffices. The enclosing-object fallback lives on the
    // raw-pointer path (`try_mark_value_or_raw`) where interior
    // pointers actually occur.
    if !valid_ptrs.contains(&ptr_val) {
        return false;
    }

    // Mark it
    unsafe {
        let header = header_from_user_ptr(ptr_val as *const u8);
        if (*header).gc_flags & GC_FLAG_MARKED != 0 {
            return false; // Already marked
        }
        if (*header).gc_flags & GC_FLAG_PINNED != 0 {
            return false; // Pinned objects are always live
        }
        (*header).gc_flags |= GC_FLAG_MARKED;
        push_mark_seed(header);
        true
    }
}

#[inline]
pub(super) fn try_mark_raw_root_addr(addr: usize, valid_ptrs: &ValidPointerSet) -> bool {
    if addr == 0 || !valid_ptrs.contains(&addr) {
        return false;
    }
    unsafe {
        let header = header_from_user_ptr(addr as *const u8);
        if (*header).gc_flags & GC_FLAG_MARKED != 0 {
            return false;
        }
        if (*header).gc_flags & GC_FLAG_PINNED != 0 {
            return false;
        }
        (*header).gc_flags |= GC_FLAG_MARKED;
        push_mark_seed(header);
        true
    }
}

/// Conservative stack scan policy wrapper. In default `auto` mode,
/// compiled frames that have a precise shadow-stack frame skip this
/// native stack/register scan. Runtime-only frames without shadow roots
/// still get the legacy fallback; `PERRY_CONSERVATIVE_STACK_SCAN=full`
/// forces that legacy path for debugging.

pub(super) unsafe fn mark_field_into_worklist(
    val_bits: u64,
    valid_ptrs: &ValidPointerSet,
    worklist: &mut Vec<*mut GcHeader>,
) -> bool {
    let tag = val_bits & TAG_MASK;
    let ptr_val: usize = if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
        let p = (val_bits & POINTER_MASK) as usize;
        if p == 0 {
            return false;
        }
        p
    } else {
        // Possible raw-I64 pointer. Reject anything with NaN-tag bits
        // (already handled above) or anything outside the 48-bit
        // user-address range. f64 numbers have the exponent bits set,
        // which puts them well above 0x0000_FFFF_FFFF_FFFF — they're
        // rejected here.
        if !(0x1000..=0x0000_FFFF_FFFF_FFFF).contains(&val_bits) {
            return false;
        }
        val_bits as usize
    };

    // Range gate + hashset lookup. No enclosing_object fallback:
    // trace-phase field words always store user pointers at object
    // starts, not interior pointers (those only arise in conservative
    // stack scanning, which uses `try_mark_value_or_raw`).
    if !valid_ptrs.contains(&ptr_val) {
        return false;
    }

    let header = header_from_user_ptr(ptr_val as *const u8);
    let flags = (*header).gc_flags;
    if flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) != 0 {
        return false;
    }
    (*header).gc_flags = flags | GC_FLAG_MARKED;
    // Push directly onto the caller's worklist. No MARK_SEEDS push —
    // that's only needed for root-phase callers that don't own a
    // worklist (mark_mutable_root_slots, mark_registered_roots,
    // mark_remembered_set_roots, mark_stack_roots). The trace drain
    // already owns and consumes this worklist.
    worklist.push(header);
    true
}

pub(super) fn try_mark_young_value_as_seed(value_bits: u64, valid_ptrs: &ValidPointerSet) -> bool {
    let ptr = decode_heap_addr(value_bits);
    try_mark_young_user_ptr_as_seed(ptr, valid_ptrs)
}

pub(super) fn try_mark_young_user_ptr_as_seed(
    ptr_val: usize,
    valid_ptrs: &ValidPointerSet,
) -> bool {
    if ptr_val == 0 || !valid_ptrs.contains(&ptr_val) {
        return false;
    }
    if !matches!(
        crate::arena::classify_heap_generation(ptr_val),
        crate::arena::HeapGeneration::Nursery
    ) {
        return false;
    }
    unsafe {
        let header = header_from_user_ptr(ptr_val as *const u8);
        let flags = (*header).gc_flags;
        if flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) != 0 {
            return false;
        }
        (*header).gc_flags = flags | GC_FLAG_MARKED;
        push_mark_seed(header);
    }
    true
}

/// Process a worklist of already-marked headers: follow references iteratively,
/// marking newly-reached objects and pushing them onto the worklist.
///
/// Gen-GC Phase C3b: when `minor_only` is true, skip tracing the
/// fields of objects whose user address is in the old-gen arena.
/// The RS already records every old→young edge written since the
/// last collection, and `mark_remembered_set_roots` enqueued the
/// relevant old-parents — they're marked live but their children
/// are NOT recursively traced. This is the time-win core of the
/// generational design: minor GC's transitive closure is bounded
/// by `O(young live set + RS roots)` instead of `O(all live)`.
pub(super) fn drain_trace_worklist(
    worklist: &mut Vec<*mut GcHeader>,
    valid_ptrs: &ValidPointerSet,
) {
    drain_trace_worklist_inner(worklist, valid_ptrs, false);
}

pub(super) fn drain_trace_worklist_inner(
    worklist: &mut Vec<*mut GcHeader>,
    valid_ptrs: &ValidPointerSet,
    minor_only: bool,
) {
    let mut cursor = 0;
    while !drain_trace_worklist_step(worklist, &mut cursor, valid_ptrs, minor_only, usize::MAX) {}
}

pub(super) fn drain_trace_worklist_step(
    worklist: &mut Vec<*mut GcHeader>,
    cursor: &mut usize,
    valid_ptrs: &ValidPointerSet,
    minor_only: bool,
    budget: usize,
) -> bool {
    let mut remaining = budget;
    while remaining > 0 && *cursor < worklist.len() {
        let header = worklist[*cursor];
        *cursor += 1;
        trace_one_worklist_header(header, valid_ptrs, worklist, minor_only);
        remaining -= 1;
    }
    *cursor >= worklist.len()
}

pub(super) fn trace_one_worklist_header(
    header: *mut GcHeader,
    valid_ptrs: &ValidPointerSet,
    worklist: &mut Vec<*mut GcHeader>,
    minor_only: bool,
) {
    unsafe {
        let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
        // C3b/C4 generational skip: in minor mode, an object
        // is treated as a black leaf when it lives in OLD_ARENA
        // (Phase B physical region) OR carries GC_FLAG_TENURED
        // (Phase C4 logical promotion — non-moving generational).
        // Either way its fields aren't recursively visited;
        // young children it holds reach the worklist via the
        // remembered set scan from C3a. False-positive RS
        // entries (parent whose write has since been overwritten)
        // are correctness-safe — extra young objects stay alive
        // for one cycle, swept on the next.
        if minor_only {
            // Skip tracing only when the object is BOTH tenured AND
            // physically in old-gen arena. Tenured-in-nursery
            // objects (until the evacuation policy moves them) still
            // hold pointers to young-gen children, and skipping their
            // fields without a write barrier on every store leaves those
            // children unmarked.
            let is_old_arena = crate::arena::pointer_in_old_gen(user_ptr as usize);
            let is_tenured = (*header).gc_flags & GC_FLAG_TENURED != 0;
            if is_tenured && is_old_arena {
                return;
            }
        }
        trace_heap_rewrite_slots(header, valid_ptrs, worklist);
    }
}

/// Trace from marked objects: follow references iteratively using a worklist.
pub(super) fn trace_marked_objects(valid_ptrs: &ValidPointerSet) {
    // Same MARK_SEEDS-based approach as the minor variant — root scans
    // populated `MARK_SEEDS` via `try_mark_value`, no need to walk arena
    // here just to gather them.
    let mut worklist = take_mark_seeds();
    drain_trace_worklist(&mut worklist, valid_ptrs);
}

/// Block-persistence pass: arena block reset is all-or-nothing, so any arena
/// object in a block that has at least one reachable object will persist in
/// memory whether or not the object itself was reached from a root. Any
/// malloc children referenced by those persisting arena objects must therefore
/// be kept alive — otherwise they get freed by sweep and the persisting arena
/// object holds dangling pointers.
///
/// Why this matters: during `arr.push(new_obj)`, the new object is in a
/// caller-saved register between its allocation and the write into `arr`.
/// If array growth triggers GC in that window, conservative stack scanning
/// (setjmp only captures callee-saved regs) doesn't see the new object as a
/// root. The arena block containing the new object still survives (other
/// objects in that block are reachable from `arr`), so the new object's
/// memory is intact. But its malloc-allocated string fields ("Record X",
/// email, etc.) get swept, and JSON.stringify later reads freed memory.
/// Repro: issues #43 / #44.
///
/// Issue #179: the force-mark-every-adjacent-object behavior cascades
/// catastrophically when a long-lived root (e.g. a caller-level
/// 10k-record array) pins an old block: the dead iter-0 neighbors get
/// resurrected, their fields trace into later blocks, and the "live
/// set" snowballs. The register-holding scenario above is inherently
/// *recent* — by the time an object is a few GC cycles old, its register
/// has been repurposed and any surviving handle has been re-loaded from
/// a stable stack slot, so block-persist on old blocks provides no
/// additional safety. Restrict Pass 2 to the last `BLOCK_PERSIST_WINDOW`
/// general-arena blocks (matching the `keep_low = current - 4` window
/// that `arena_reset_empty_blocks` already uses — same reasoning).
/// Longlived-arena blocks (indices `>= general_block_count()`) never
/// get block-persisted either: every object in that arena is kept alive
/// by an explicit root scanner (`scan_parse_roots`,
/// `scan_shape_cache_roots`, `scan_transition_cache_roots`), so any
/// unmarked object there is genuinely unreachable — its malloc
/// children can safely be swept.
///
/// Iterates until fixed point because marking an arena object may trace a
/// child in a previously-dead block, making it live in the next round.
/// The fixed-point loop terminates faster with the restricted window
/// because cross-block trace expansion can no longer pull in dead
/// old-block neighbors as new block-persist candidates.
pub(super) const BLOCK_PERSIST_WINDOW: usize = 5;

pub(super) fn mark_block_persisting_arena_objects(
    valid_ptrs: &ValidPointerSet,
) -> BlockPersistTraceStats {
    let mut worklist: Vec<*mut GcHeader> = Vec::new();
    let mut stats = BlockPersistTraceStats::default();
    loop {
        stats.iterations += 1;
        let n_blocks = crate::arena::arena_block_count();
        let general_n = crate::arena::general_block_count();
        // Recent-window lower bound: same formula as the reset policy's
        // `keep_low` (issue #73) so block-persist and reset operate on
        // the same "registers might still hold handles here" definition
        // of recent.
        let persist_low = general_n.saturating_sub(BLOCK_PERSIST_WINDOW);
        let mut block_has_live: Vec<bool> = vec![false; n_blocks];

        // Pass 1: compute which blocks have any reachable (marked/pinned)
        // object. Restricted to the same recent young-arena window pass 2
        // uses — pass 1 only existed to populate the filter pass 2 reads,
        // and longlived/old/non-recent blocks would never enter pass 2's
        // mark loop anyway. With ~1.6M objects per cycle in
        // perf-comprehensive and only the last 5 general blocks within the
        // window, this collapses pass 1 from a full arena walk to a
        // handful-of-blocks walk.
        crate::arena::arena_walk_objects_filtered(
            |block_idx| block_idx >= persist_low && block_idx < general_n,
            |header_ptr, block_idx| {
                let header = header_ptr as *mut GcHeader;
                unsafe {
                    if (*header).gc_flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) != 0
                        && block_idx < block_has_live.len()
                    {
                        block_has_live[block_idx] = true;
                    }
                }
            },
        );
        let live_blocks_this = block_has_live.iter().filter(|&&live| live).count();
        let candidate_blocks_this = (persist_low..general_n)
            .filter(|&block_idx| block_has_live.get(block_idx).copied().unwrap_or(false))
            .count();
        stats.live_blocks += live_blocks_this;
        stats.candidate_blocks += candidate_blocks_this;

        // Pass 2: mark any unmarked arena object in a live block and enqueue.
        // Block-level pre-filter skips the object loop for dead blocks —
        // post-parse workloads can have 27 of 29 blocks containing 3M dead
        // objects, and the per-object early-return inside the callback still
        // invokes the walker for every header (issue #64 follow-up). The
        // filter drops pass 2 from ~55ms to <1ms on that workload.
        //
        // Issue #179 restriction: only persist recent general-arena blocks.
        // Longlived blocks (block_idx >= general_n) and old general blocks
        // (block_idx < persist_low) are skipped — their dead objects will
        // be naturally unmarked and their malloc children swept.
        let mut newly_marked = 0usize;
        crate::arena::arena_walk_objects_filtered(
            |block_idx| {
                block_idx < block_has_live.len()
                    && block_has_live[block_idx]
                    && block_idx >= persist_low
                    && block_idx < general_n
            },
            |header_ptr, _block_idx| {
                let header = header_ptr as *mut GcHeader;
                unsafe {
                    if (*header).gc_flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) == 0 {
                        (*header).gc_flags |= GC_FLAG_MARKED;
                        worklist.push(header);
                        newly_marked += 1;
                    }
                }
            },
        );
        stats.marked_objects += newly_marked;

        if newly_marked == 0 {
            break;
        }

        // Trace newly marked; may mark children in previously-dead blocks,
        // requiring another round to pick them up (but only within the
        // recent window — old blocks' newly-traced marks don't re-enter
        // the block-persist pump).
        drain_trace_worklist(&mut worklist, valid_ptrs);
    }
    stats
}

pub(super) unsafe fn trace_heap_rewrite_slots(
    header: *mut GcHeader,
    valid_ptrs: &ValidPointerSet,
    worklist: &mut Vec<*mut GcHeader>,
) {
    visit_gc_rewrite_slots(header, |slot| unsafe {
        slot.record_layout_read();
        if slot.layout_kind.is_some() {
            record_trace_slot_read();
        }
        mark_field_into_worklist(*slot.slot, valid_ptrs, worklist);
    });
}

/// Trace array elements.
/// Elements may be NaN-boxed JSValues OR raw I64 pointers (codegen stores raw I64 for
/// is_pointer/is_array/is_string typed arrays via js_array_set_jsvalue).
// #854: part of GC fallback/verification trace path (also exercised by gc/tests)
#[allow(dead_code)]
pub(super) unsafe fn trace_array(
    user_ptr: *mut u8,
    valid_ptrs: &ValidPointerSet,
    worklist: &mut Vec<*mut GcHeader>,
) {
    // Issue #233: a runtime-installed FORWARDED flag (from
    // js_array_grow) means this user_ptr's first 8 bytes hold the
    // forwarding pointer instead of length+capacity. Tracing it as
    // an array would either bail (corrupt sanity check) or scan
    // garbage as JSValues. Push the forwarding target on the
    // worklist so the live new array stays marked, and return.
    let header = (user_ptr as *const u8).sub(GC_HEADER_SIZE) as *const GcHeader;
    if (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
        let new_user = forwarding_address(header) as usize;
        if new_user >= 0x1000 {
            let new_header = header_from_user_ptr(new_user as *const u8);
            worklist.push(new_header);
        }
        return;
    }

    trace_heap_rewrite_slots(header as *mut GcHeader, valid_ptrs, worklist);
}

/// Trace object fields and keys array.
/// Fields may be NaN-boxed JSValues OR raw I64 pointers (codegen stores some fields as raw I64).
/// keys_array may be a raw pointer (*mut ArrayHeader) OR NaN-boxed (codegen may NaN-box it).
// #854: part of GC fallback/verification trace path (also exercised by gc/tests)
#[allow(dead_code)]
pub(super) unsafe fn trace_object(
    user_ptr: *mut u8,
    valid_ptrs: &ValidPointerSet,
    worklist: &mut Vec<*mut GcHeader>,
) {
    let header = (user_ptr as *const u8).sub(GC_HEADER_SIZE) as *mut GcHeader;
    trace_heap_rewrite_slots(header, valid_ptrs, worklist);
}

/// Trace closure captures
/// Captures may be NaN-boxed JSValues OR raw I64 pointers bitcast to F64.
/// Perry's codegen stores `is_string`/`is_array`/`is_closure` captures as raw I64 in some paths.
// #854: part of GC fallback/verification trace path (also exercised by gc/tests)
#[allow(dead_code)]
pub(super) unsafe fn trace_closure(
    user_ptr: *mut u8,
    valid_ptrs: &ValidPointerSet,
    worklist: &mut Vec<*mut GcHeader>,
) {
    let header = (user_ptr as *const u8).sub(GC_HEADER_SIZE) as *mut GcHeader;
    trace_heap_rewrite_slots(header, valid_ptrs, worklist);
}

/// Sweep: free unmarked malloc objects; add unmarked arena objects to free list.
/// Returns total bytes freed.
#[cfg(test)]

pub(super) fn clear_marks() {
    // Clear arena objects
    crate::arena::arena_walk_objects(|header_ptr| {
        let header = header_ptr as *mut GcHeader;
        unsafe {
            (*header).gc_flags &= !GC_FLAG_MARKED;
        }
    });

    // Clear malloc objects
    MALLOC_STATE.with(|s| {
        let s = s.borrow();
        for &header in s.objects.iter() {
            unsafe {
                (*header).gc_flags &= !GC_FLAG_MARKED;
            }
        }
    });
}

// ============================================================================
// Root scanner registrations (called during module init)
// ============================================================================
