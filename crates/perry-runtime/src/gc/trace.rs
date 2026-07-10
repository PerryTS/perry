use super::*;

thread_local! {
    /// Set by test-only helpers that wipe page metadata for isolation
    /// (`old_arena_page_index_clear_for_tests`): real objects become
    /// unclassifiable in that synthetic state, so the differential verifier
    /// must stand down for the rest of the thread's test.
    pub(crate) static CLASSIFIER_VERIFY_SUPPRESSED: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

/// #6179 membership classifier: is `addr` a plausible live GC object start?
/// UNION of the two backends — exact membership in the malloc registry OR a
/// plausible arena header on an arena-classified page — deliberately NOT the
/// exclusive generation branch of `current_heap_header_for_user_ptr`: malloc
/// allocations can sit inside address ranges the page metadata attributes to
/// an arena generation, and an exclusive branch then never consults the
/// (exact) malloc registry, leaving live malloc roots unmarked.
#[inline]
pub(super) fn classifier_valid_object_start(addr: usize) -> bool {
    if addr < GC_HEADER_SIZE + 0x1000 {
        return false;
    }
    let header = unsafe { header_from_user_ptr(addr as *const u8) };
    if super::gc_malloc_header_is_tracked(header) {
        return true;
    }
    !matches!(
        crate::arena::classify_heap_generation(addr),
        crate::arena::HeapGeneration::Unknown
    ) && unsafe { super::barrier::plausible_arena_user_ptr_header(header).is_some() }
}

/// #6179: differential-verification mode for the page-metadata classifier.
pub(super) fn classifier_verify_enabled() -> bool {
    if CLASSIFIER_VERIFY_SUPPRESSED.with(|c| c.get()) {
        return false;
    }
    static CACHED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CACHED.get_or_init(|| {
        matches!(
            std::env::var("PERRY_GC_VERIFY_CLASSIFIER").as_deref(),
            Ok("1") | Ok("on") | Ok("true")
        )
    })
}

thread_local! {
    pub(super) static MARK_SEEDS: std::cell::UnsafeCell<Vec<*mut GcHeader>> =
        const { std::cell::UnsafeCell::new(Vec::new()) };
}

const VALID_POINTER_ARENA_RUN_CAPACITY: usize = 1024;

pub(crate) struct ValidPointerSet {
    /// Arena-only start pointers in address-ordered runs. `lookup_set`
    /// handles exact pointer membership; these runs support
    /// `enclosing_object` floor lookups for arena interior pointers without
    /// a final heap-sized merge.
    pub(super) arena_runs: Vec<Vec<usize>>,
    pub(super) current_arena_run: Vec<usize>,
    /// Exact pointer membership filled incrementally as arena and malloc
    /// entries are discovered. A B-tree avoids hash-table rebuilds in tiny
    /// budget steps; insertion may split one fixed-size node but never
    /// rehashes all previously discovered pointers.
    pub(super) lookup_set: std::collections::BTreeSet<usize>,
    // Min/max heap-pointer range across the valid set. Updated as entries
    // are inserted. The conservative stack scan calls `contains` once per
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
    /// True only for sets produced by the production census walk
    /// (`ValidPointerSetBuilder::finish`). The #6179 differential verifier
    /// runs only on census-built sets: tests fabricate sets with synthetic
    /// addresses, and classifying those dereferences headers that don't
    /// exist (panics inside no-unwind scanner contexts → poisoned globals).
    pub(super) built_by_census: bool,
    /// #6179: budgeted (precise, non-moving, no-conservative-scan) cycles
    /// skip the O(heap) exact census entirely; membership routes through the
    /// page-metadata classifier (differentially verified as a superset of
    /// the census across the whole suite). Classifier-mode cycles must NEVER
    /// run a conservative stack scan: conservative words are traced, and a
    /// heuristic false positive would trace garbage.
    pub(super) classifier_mode: bool,
}

impl ValidPointerSet {
    pub(super) fn new() -> Self {
        Self {
            arena_runs: Vec::new(),
            current_arena_run: Vec::with_capacity(VALID_POINTER_ARENA_RUN_CAPACITY),
            lookup_set: std::collections::BTreeSet::new(),
            range_min: usize::MAX,
            range_max: 0,
            tenured_nursery_bytes: 0,
            built_by_census: false,
            classifier_mode: false,
        }
    }

    /// Caller must guarantee that pushes happen in ascending address
    /// order — `ValidPointerSetBuilder` does so via `ArenaObjectCursor`
    /// in address order.
    pub(super) fn push_arena(&mut self, ptr: usize) {
        if self.classifier_mode {
            return; // #6179: no exact census in classifier mode
        }
        if let Some(previous) = self
            .current_arena_run
            .last()
            .copied()
            .or_else(|| self.arena_runs.last().and_then(|run| run.last()).copied())
        {
            debug_assert!(previous <= ptr);
        }

        self.lookup_set.insert(ptr);
        self.record_pointer_range(ptr);
        self.current_arena_run.push(ptr);
        if self.current_arena_run.len() >= VALID_POINTER_ARENA_RUN_CAPACITY {
            self.seal_current_arena_run();
        }
    }

    pub(super) fn push_malloc(&mut self, ptr: usize) {
        if self.classifier_mode {
            return; // #6179: no exact census in classifier mode
        }
        self.lookup_set.insert(ptr);
        self.record_pointer_range(ptr);
    }

    pub(super) fn record_tenured_nursery_bytes(&mut self, bytes: usize) {
        self.tenured_nursery_bytes += bytes;
    }
    pub(super) fn tenured_nursery_bytes(&self) -> usize {
        self.tenured_nursery_bytes
    }
    pub(super) fn finalize(&mut self) {
        self.seal_current_arena_run();
    }

    #[inline(always)]
    fn record_pointer_range(&mut self, ptr: usize) {
        if ptr < self.range_min {
            self.range_min = ptr;
        }
        if ptr > self.range_max {
            self.range_max = ptr;
        }
    }

    fn seal_current_arena_run(&mut self) {
        if self.current_arena_run.is_empty() {
            return;
        }
        let sealed = std::mem::take(&mut self.current_arena_run);
        self.arena_runs.push(sealed);
    }

    /// Cheap O(1) range-rejection prefilter. Most stack words and
    /// register spills are not heap pointers; if the candidate falls
    /// outside `[range_min, range_max]` it cannot match either region
    /// and we skip the binary search.
    #[inline(always)]
    pub(crate) fn maybe_contains(&self, ptr: usize) -> bool {
        // #6179 classifier mode: no census, so no address range — the
        // prefilter must pass everything through to `contains()` (call sites
        // check maybe_contains SEPARATELY before contains and would
        // otherwise reject every root).
        if self.classifier_mode {
            return true;
        }
        ptr >= self.range_min && ptr <= self.range_max
    }
    #[inline]
    pub(crate) fn contains(&self, ptr: &usize) -> bool {
        if self.classifier_mode {
            // No census was built: classify live. FORWARDED rejects are
            // correct here (budgeted cycles are non-moving; a forwarded
            // header can only be pre-existing stale state).
            return classifier_valid_object_start(*ptr);
        }
        if !self.maybe_contains(*ptr) {
            // Range-rejected candidates skip the differential check: the
            // classifier legitimately accepts objects born after the census
            // (allocate-black keeps them safe), which can lie outside the
            // censused address range.
            return false;
        }
        // Exact lookup. The B-tree insert path is bounded during
        // `BuildValidPointerSet`, so a tiny GC step cannot trigger a
        // heap-sized hash-table rebuild.
        let exact = self.lookup_set.contains(ptr);
        // #6179 differential verification (PERRY_GC_VERIFY_CLASSIFIER=1):
        // before the exact set can be replaced by page-metadata
        // classification on precise cycles, the classifier must be proven a
        // SUPERSET of the census on every query — a censused object the
        // classifier rejects would be un-markable, i.e. swept live. The
        // other direction (classifier accepts, census lacks) is expected:
        // objects allocated after the census walk; over-approximation is
        // safe (bounded floating garbage under allocate-black).
        if exact && self.built_by_census && classifier_verify_enabled() {
            let heur = classifier_valid_object_start(*ptr);
            // Censused-then-MOVED is a legitimate divergence: the plausible-
            // header check rejects FORWARDED headers by design (the object
            // lives at its new address; the copying machinery owns the
            // redirect).
            let forwarded = !heur
                && unsafe {
                    let header = header_from_user_ptr(*ptr as *const u8);
                    (*header).gc_flags & GC_FLAG_FORWARDED != 0
                };
            assert!(
                heur || forwarded,
                "classifier rejected censused object {ptr:#x} — page metadata / header heuristic disagrees with the exact valid-pointer set"
            );
        }
        exact
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
        let candidate = self.find_arena_floor(ptr)?;
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

    fn find_arena_floor(&self, ptr: usize) -> Option<usize> {
        let idx = self
            .arena_runs
            .partition_point(|run| run.first().copied().is_some_and(|first| first <= ptr));
        if idx == 0 {
            return None;
        }
        Self::find_floor(&self.arena_runs[idx - 1], ptr)
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
    phase: ValidPointerSetBuildPhase,
    arena_cursor_builder: Option<crate::arena::ArenaObjectCursorBuilder>,
    arena_cursor: Option<crate::arena::ArenaObjectCursor>,
    malloc_index: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ValidPointerSetBuildPhase {
    ArenaCursorSetup,
    ArenaWalk,
    MallocWalk,
    Finalize,
    Done,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ValidPointerSetBuilderSnapshot {
    pub(super) phase: ValidPointerSetBuildPhase,
    pub(super) arena_setup_blocks: usize,
    pub(super) arena_run_count: usize,
    pub(super) current_arena_run_len: usize,
    pub(super) lookup_count: usize,
    pub(super) malloc_index: usize,
}

impl ValidPointerSetBuilder {
    /// #6179: budgeted-cycle flavor — the census walk still runs (it feeds
    /// tenured_nursery_bytes for evacuation policy) but nothing is inserted
    /// into the exact set; membership resolves via the classifier. Saves the
    /// O(heap) BTreeSet held across the whole sliced cycle.
    pub(super) fn new_classifier() -> Self {
        let mut b = Self::new();
        b.set.classifier_mode = true;
        b
    }

    pub(super) fn new() -> Self {
        Self {
            set: ValidPointerSet::new(),
            phase: ValidPointerSetBuildPhase::ArenaCursorSetup,
            arena_cursor_builder: Some(crate::arena::ArenaObjectCursorBuilder::new(
                crate::arena::ArenaWalkOrder::Address,
            )),
            arena_cursor: None,
            malloc_index: 0,
        }
    }

    pub(super) fn step(&mut self, budget: usize) -> bool {
        if self.phase == ValidPointerSetBuildPhase::Done {
            return true;
        }

        let mut remaining = budget;
        let unbounded = budget == usize::MAX;

        loop {
            match self.phase {
                ValidPointerSetBuildPhase::ArenaCursorSetup => {
                    if remaining == 0 {
                        return false;
                    }
                    let cursor = self
                        .arena_cursor_builder
                        .as_mut()
                        .and_then(|builder| builder.step(&mut remaining));
                    let Some(cursor) = cursor else {
                        return false;
                    };
                    self.arena_cursor_builder = None;
                    self.arena_cursor = Some(cursor);
                    self.phase = ValidPointerSetBuildPhase::ArenaWalk;
                    if !unbounded {
                        return false;
                    }
                }
                ValidPointerSetBuildPhase::ArenaWalk => {
                    if !self.step_arena_walk(&mut remaining) {
                        return false;
                    }
                    self.phase = ValidPointerSetBuildPhase::MallocWalk;
                    if !unbounded {
                        return false;
                    }
                }
                ValidPointerSetBuildPhase::MallocWalk => {
                    if !self.step_malloc_walk(&mut remaining) {
                        return false;
                    }
                    self.phase = ValidPointerSetBuildPhase::Finalize;
                    if !unbounded {
                        return false;
                    }
                }
                ValidPointerSetBuildPhase::Finalize => {
                    if remaining == 0 {
                        return false;
                    }
                    self.set.finalize();
                    self.phase = ValidPointerSetBuildPhase::Done;
                    return true;
                }
                ValidPointerSetBuildPhase::Done => return true,
            }
        }
    }

    pub(super) fn finish(mut self) -> ValidPointerSet {
        self.set.built_by_census = true;
        if self.phase != ValidPointerSetBuildPhase::Done {
            while !self.step(usize::MAX) {}
        }
        self.set
    }

    fn step_arena_walk(&mut self, remaining: &mut usize) -> bool {
        while *remaining > 0 {
            let next = {
                let cursor = self
                    .arena_cursor
                    .as_mut()
                    .expect("arena cursor exists during arena walk");
                cursor.next_budgeted(remaining)
            };
            let Some((header_ptr, _block_idx)) = next else {
                let finished = self
                    .arena_cursor
                    .as_ref()
                    .expect("arena cursor exists during arena walk")
                    .is_finished();
                if finished {
                    self.arena_cursor = None;
                    return true;
                }
                return false;
            };
            self.record_arena_header(header_ptr);
        }
        false
    }

    fn record_arena_header(&mut self, header_ptr: *mut u8) {
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
    }

    fn step_malloc_walk(&mut self, remaining: &mut usize) -> bool {
        while *remaining > 0 {
            let maybe_header = MALLOC_STATE.with(|s| {
                let s = s.borrow();
                s.objects.get(self.malloc_index).copied()
            });
            let Some(header) = maybe_header else {
                return true;
            };
            let user_ptr = unsafe { (header as *mut u8).add(GC_HEADER_SIZE) };
            self.set.push_malloc(user_ptr as usize);
            self.malloc_index += 1;
            *remaining -= 1;
        }
        false
    }

    #[cfg(test)]
    pub(super) fn snapshot_for_tests(&self) -> ValidPointerSetBuilderSnapshot {
        ValidPointerSetBuilderSnapshot {
            phase: self.phase,
            arena_setup_blocks: self
                .arena_cursor_builder
                .as_ref()
                .map_or(0, crate::arena::ArenaObjectCursorBuilder::inspected_blocks),
            arena_run_count: self.set.arena_runs.len(),
            current_arena_run_len: self.set.current_arena_run.len(),
            lookup_count: self.set.lookup_set.len(),
            malloc_index: self.malloc_index,
        }
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

/// Conservative stack scan policy wrapper. In default `auto` mode, native
/// stack/register scanning is skipped so copied-minor eligibility only depends
/// on exact mutable roots. `PERRY_CONSERVATIVE_STACK_SCAN=full` forces the
/// legacy path for debugging and makes copied-minor ineligible.

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

    // Range gate + exact lookup. No enclosing_object fallback:
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
    match crate::arena::classify_heap_generation(ptr_val) {
        crate::arena::HeapGeneration::Nursery => {}
        // Non-arena member of the valid-pointer set = malloc-GC object.
        // Minors sweep the malloc registry, and malloc objects are NOT
        // black-leafed in minor traces (they can hold nursery children),
        // so an RS-reachable malloc child must be seeded and traced
        // exactly like a nursery one — otherwise the malloc sweep frees
        // it while its only referrer is a clean old parent.
        crate::arena::HeapGeneration::Unknown => {}
        crate::arena::HeapGeneration::Old | crate::arena::HeapGeneration::Longlived => {
            return false;
        }
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
        if crate::weakref::is_weak_target_trace_slot(header, slot.slot) {
            return;
        }
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
