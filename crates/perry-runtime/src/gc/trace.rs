use super::*;

#[path = "trace/block_skip.rs"]
pub(super) mod block_skip;
pub(super) use block_skip::BlockCensus;

#[path = "trace/adopt_census.rs"]
pub(crate) mod adopt_census;
pub(crate) use adopt_census::AdoptableBlockBuilder;

crate::perry_thread_local! {
    /// Set by test-only helpers that wipe page metadata for isolation
    /// (`old_arena_page_index_clear_for_tests`): real objects become
    /// unclassifiable in that synthetic state, so the differential verifier
    /// must stand down for the rest of the thread's test. Read only by the
    /// verifier, which `gc-instruments` serves.
    #[cfg_attr(not(feature = "gc-instruments"), allow(dead_code))]
    pub(crate) static CLASSIFIER_VERIFY_SUPPRESSED: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

crate::perry_thread_local! {
    /// #9717: array-growth forwarding stubs the classifier admitted into a
    /// budgeted-cycle valid-pointer set that `plausible_arena_user_ptr_header`
    /// would have rejected. A non-zero count is a POSITIVE report that a live
    /// slot pointed at a growth stub during a budgeted full trace — the exact
    /// edge whose loss swept a private-field array on the idle reclaim. Zero on
    /// a run with no such edge, so it never perturbs a log a gate parses.
    static FORWARDED_STUB_MEMBERSHIP_RECOVERIES: std::cell::Cell<u64> =
        const { std::cell::Cell::new(0) };
}

#[cold]
fn note_forwarded_stub_membership_recovery() {
    FORWARDED_STUB_MEMBERSHIP_RECOVERIES.with(|c| c.set(c.get().saturating_add(1)));
}

/// Running count of array-growth forwarding stubs the classifier recovered into
/// a budgeted valid-pointer set (#9717). A test that plants a stub-only-
/// referenced array can assert this moved.
pub(crate) fn forwarded_stub_membership_recoveries() -> u64 {
    FORWARDED_STUB_MEMBERSHIP_RECOVERIES.with(std::cell::Cell::get)
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
    if matches!(
        crate::arena::classify_heap_generation(addr),
        crate::arena::HeapGeneration::Unknown
    ) {
        return false;
    }
    if unsafe { super::barrier::plausible_arena_user_ptr_header(header).is_some() } {
        return true;
    }
    // #9717: an array-growth forwarding stub is a real censused arena object a
    // live slot can still point directly at (references are never rewritten,
    // #6228). The census path admits it (record_arena_header pushes every arena
    // object), so this classifier -- which contains() uses for a budgeted,
    // non-moving cycle and which must be a census SUPERSET -- has to admit it
    // too. plausible_arena_user_ptr_header rejects FORWARDED headers by design
    // (a metadata key whose object may have died and been recycled with the bit
    // set), so the stub was silently dropped: mark_field_into_worklist failed
    // membership, never marked the stub, and the FORWARDED-follow in
    // trace_one_worklist_header never ran -- so the live post-growth array,
    // reachable only through the field to stub edge, was swept. That is the
    // idle-time (budgeted full) reclaim turning a private-field array empty.
    if unsafe { super::barrier::plausible_forwarded_arena_stub(header).is_some() } {
        note_forwarded_stub_membership_recovery();
        return true;
    }
    false
}

/// #6179: differential-verification mode for the page-metadata classifier.
/// A `gc-instruments` knob (#10572): constant `false` without the feature.
pub(super) fn classifier_verify_enabled() -> bool {
    #[cfg(not(feature = "gc-instruments"))]
    return false;
    // The cached process-wide switch first: this runs on every census hit, and
    // the suppression flag is a thread-local (#10182).
    #[cfg(feature = "gc-instruments")]
    {
        static CACHED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *crate::once_init::get_or_init(&CACHED, || {
            super::env_flag_enabled("PERRY_GC_VERIFY_CLASSIFIER")
        }) && !CLASSIFIER_VERIFY_SUPPRESSED.with(|c| c.get())
    }
}

crate::perry_thread_local! {
    pub(super) static MARK_SEEDS: std::cell::UnsafeCell<Vec<*mut GcHeader>> =
        const { std::cell::UnsafeCell::new(Vec::new()) };
}

/// Census blocks whose walked extent exceeds this keep a sorted start list
/// instead of a start bitmap: an oversized block (one large allocation rounded
/// up to a `BLOCK_SIZE` multiple) holds a handful of objects, and a bitmap over
/// its whole extent would be mostly zero words.
const CENSUS_BITMAP_MAX_EXTENT: usize = crate::arena::BLOCK_SIZE;

/// Start bitmaps are allocated in chunks of this many words (8 KiB), one or two
/// per block. A bitmap kept in one contiguous vector grew past the allocator's
/// small and medium size classes and made it commit a fresh large page:
/// `records_array_1m:sparse` read +4.8 MiB peak RSS for a ~100 KB index. The
/// start runs this replaces were 8 KiB vectors too.
const CENSUS_BITMAP_CHUNK_WORDS: usize = 1024;
const CENSUS_BITMAP_CHUNK_WORD_SHIFT: u32 = 10;
/// Chunks a bitmap block can need: `CENSUS_BITMAP_MAX_EXTENT` bytes at one bit
/// per 8 bytes is `2 * CENSUS_BITMAP_CHUNK_WORDS` words.
const CENSUS_BITMAP_MAX_CHUNKS: usize = 2;

/// Arena object starts sit on 8-byte boundaries relative to their block's
/// `data` pointer: `ArenaObjectCursor::next_budgeted` rounds every header
/// offset up to a multiple of 8 before reading it, and the census consumes
/// exactly the headers that cursor yields.
const CENSUS_START_ALIGN_SHIFT: u32 = 3;

/// Window of the direct-mapped block index (#10182): one arena block (1 MiB).
/// Distinct arena blocks are at least this large and never overlap, so no two
/// census block bases fall into one window.
const CENSUS_BLOCK_WINDOW_SHIFT: u32 = 20;
/// Largest direct-mapped block index built: 16 GiB of address span, 128 KiB of
/// table. A census spread wider keeps the binary search.
const CENSUS_BLOCK_WINDOW_MAX: usize = 1 << 14;

/// One censused arena block, in address order (#10182).
#[derive(Clone, Copy, Debug)]
pub(super) struct CensusStartBlock {
    /// The block's `data` address as the census cursor snapshotted it.
    pub(super) base: usize,
    /// Bytes the census walked (`offset` in the snapshot). No censused header
    /// starts at or past `base + extent`.
    pub(super) extent: usize,
    /// Global arena block index (`u32::MAX` when unknown).
    pub(super) block_idx: u32,
    /// Bitmap blocks: the block's bitmap chunks (owned by
    /// `ValidPointerSet::start_bitmap_chunks`); word `w` is word
    /// `w & (CENSUS_BITMAP_CHUNK_WORDS - 1)` of chunk `w >> 10`. Null past the
    /// block's word count.
    pub(super) chunks: [*mut u64; CENSUS_BITMAP_MAX_CHUNKS],
    /// Sorted blocks: index of the block's first start in `large_starts`.
    pub(super) first: usize,
    /// Bitmap blocks: word count. Sorted blocks: start count.
    pub(super) len: usize,
    pub(super) sorted: bool,
}

impl CensusStartBlock {
    /// Word `word_idx` of this bitmap block's start bitmap.
    ///
    /// # Safety
    /// `self` is a bitmap block of a live `ValidPointerSet` and
    /// `word_idx < self.len`.
    #[inline(always)]
    unsafe fn word_ptr(&self, word_idx: usize) -> *mut u64 {
        debug_assert!(!self.sorted && word_idx < self.len);
        self.chunks[word_idx >> CENSUS_BITMAP_CHUNK_WORD_SHIFT]
            .add(word_idx & (CENSUS_BITMAP_CHUNK_WORDS - 1))
    }
}

pub(crate) struct ValidPointerSet {
    /// **The exact arena membership set**, one entry per censused arena block
    /// in ascending address order. `ArenaObjectCursorBuilder::new(
    /// ArenaWalkOrder::Address)` hands the census the blocks in address order
    /// and each block's headers in address order, so the table is sorted by
    /// construction and a block never appears twice.
    ///
    /// Membership used to answer from address-ordered runs of user pointers
    /// (1024 per run, sealed at block boundaries): a binary search over every
    /// run's first key, then a second binary search inside the run — about 21
    /// cache-missing probes per traced pointer field, which was ~46 % of a full
    /// mark on a live 20 MB JSON tree. The runs replaced a shadow `BTreeSet`
    /// (#7592) and cost 8 bytes per censused object.
    ///
    /// Now each block carries an **object-start bitmap** (1 bit per 8-byte
    /// alignment unit of its walked extent, 16 KB for a full 1 MB block) and a
    /// query is a search over the block fences (`arena_block_bases`, one entry
    /// per block) plus one bit test. Oversized blocks keep a sorted start list
    /// (`large_starts`), since they hold a few objects over many megabytes.
    pub(super) arena_blocks: Vec<CensusStartBlock>,
    /// `arena_blocks[i].base`, mirrored into one contiguous vector so the
    /// block-level binary search reads 8-byte fences only.
    pub(super) arena_block_bases: Vec<usize>,
    /// Storage of the bitmap blocks' start bitmaps, in 8 KiB chunks (see
    /// `CENSUS_BITMAP_CHUNK_WORDS`). Bit `k` of a block's bitmap is set iff a
    /// censused header starts at `base + (k << 3)`. A chunk's heap buffer never
    /// moves, so `CensusStartBlock::chunks` may point into it.
    pub(super) start_bitmap_chunks: Vec<Vec<u64>>,
    /// Concatenated ascending start lists (user pointers) of the sorted blocks.
    pub(super) large_starts: Vec<usize>,
    /// Per-block census facts and trace reachability (#10182). Disarmed
    /// unless this set was built by the production census walk.
    pub(super) block_census: BlockCensus,
    /// Direct-mapped block index (#10182): one entry per 1 MiB window from
    /// `block_windows_lo`, holding the index of the greatest census block whose
    /// base is at or below the window start and the index of the census block,
    /// at most one, whose base lies inside the window (`u32::MAX` for none).
    /// Empty means `census_block_at` binary-searches `arena_block_bases`.
    pub(super) block_windows: Vec<(u32, u32)>,
    pub(super) block_windows_lo: usize,
    /// Live count of pushed arena starts, kept so `lookup_count` stays O(1).
    pub(super) arena_count: usize,
    /// Exact membership for **malloc-tracked** objects only, which have no
    /// address order to exploit. A B-tree avoids hash-table rebuilds in tiny
    /// budget steps; insertion may split one fixed-size node but never
    /// rehashes all previously discovered pointers.
    pub(super) malloc_lookup: std::collections::BTreeSet<usize>,
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
            arena_blocks: Vec::new(),
            arena_block_bases: Vec::new(),
            start_bitmap_chunks: Vec::new(),
            large_starts: Vec::new(),
            block_census: BlockCensus::disarmed(),
            block_windows: Vec::new(),
            block_windows_lo: 0,
            arena_count: 0,
            malloc_lookup: std::collections::BTreeSet::new(),
            range_min: usize::MAX,
            range_max: 0,
            tenured_nursery_bytes: 0,
            built_by_census: false,
            classifier_mode: false,
        }
    }

    /// Open census block `block_idx`: `data`/`offset` exactly as the census
    /// cursor snapshotted it. Every start pushed until the next call belongs to
    /// this block. Blocks must be opened in ascending address order.
    pub(super) fn begin_arena_block(&mut self, block_idx: u32, data: usize, offset: usize) {
        if self.classifier_mode {
            return; // #6179: no exact census in classifier mode
        }
        if let Some(previous) = self.arena_blocks.last() {
            assert!(
                previous.base.saturating_add(previous.extent) <= data,
                "census blocks must arrive in ascending, non-overlapping address order: \
                 {:#x}+{:#x} then {data:#x}",
                previous.base,
                previous.extent
            );
        }
        let sorted = offset > CENSUS_BITMAP_MAX_EXTENT;
        let mut chunks = [std::ptr::null_mut(); CENSUS_BITMAP_MAX_CHUNKS];
        let (first, len) = if sorted {
            (self.large_starts.len(), 0)
        } else {
            let bits = offset.div_ceil(1 << CENSUS_START_ALIGN_SHIFT);
            let words = bits.div_ceil(64);
            for (index, chunk) in chunks.iter_mut().enumerate() {
                let start = index * CENSUS_BITMAP_CHUNK_WORDS;
                if start >= words {
                    break;
                }
                let mut storage = vec![0u64; (words - start).min(CENSUS_BITMAP_CHUNK_WORDS)];
                *chunk = storage.as_mut_ptr();
                self.start_bitmap_chunks.push(storage);
            }
            (0, words)
        };
        self.arena_block_bases.push(data);
        self.arena_blocks.push(CensusStartBlock {
            base: data,
            extent: offset,
            block_idx,
            chunks,
            first,
            len,
            sorted,
        });
    }

    /// Open census block `block_idx` with start-bitmap storage built by someone
    /// else (`adopt_census`): `chunks` hold `words` words in the census layout
    /// for a block whose walked extent is `offset`.
    pub(super) fn begin_arena_block_with_chunks(
        &mut self,
        block_idx: u32,
        data: usize,
        offset: usize,
        words: usize,
        mut chunks: Vec<Vec<u64>>,
    ) {
        debug_assert!(!self.classifier_mode && offset <= CENSUS_BITMAP_MAX_EXTENT);
        if let Some(previous) = self.arena_blocks.last() {
            assert!(
                previous.base.saturating_add(previous.extent) <= data,
                "census blocks must arrive in ascending, non-overlapping address order: \
                 {:#x}+{:#x} then {data:#x}",
                previous.base,
                previous.extent
            );
        }
        assert_eq!(
            words,
            offset.div_ceil(1 << CENSUS_START_ALIGN_SHIFT).div_ceil(64),
            "adopted bitmap must cover exactly the block's walked extent"
        );
        let mut pointers = [std::ptr::null_mut(); CENSUS_BITMAP_MAX_CHUNKS];
        for (index, chunk) in chunks.iter_mut().enumerate().take(CENSUS_BITMAP_MAX_CHUNKS) {
            pointers[index] = chunk.as_mut_ptr();
        }
        self.start_bitmap_chunks.extend(chunks);
        self.arena_block_bases.push(data);
        self.arena_blocks.push(CensusStartBlock {
            base: data,
            extent: offset,
            block_idx,
            chunks: pointers,
            first: 0,
            len: words,
            sorted: false,
        });
    }

    /// Record a censused arena start (user pointer) in the block opened by the
    /// last `begin_arena_block`. Starts arrive in ascending address order —
    /// `ValidPointerSetBuilder` feeds them from `ArenaObjectCursor` in address
    /// order.
    pub(super) fn push_arena(&mut self, ptr: usize) {
        if self.classifier_mode {
            return; // #6179: no exact census in classifier mode
        }
        let block = self
            .arena_blocks
            .last_mut()
            .expect("an arena start is pushed only inside an opened census block");
        let header_offset = ptr.wrapping_sub(block.base).wrapping_sub(GC_HEADER_SIZE);
        // A start the bitmap cannot represent would be a silent false negative
        // (swept live), so the cursor's alignment contract is checked, not
        // assumed. One predictable compare per censused object.
        assert!(
            header_offset < block.extent
                && header_offset & ((1 << CENSUS_START_ALIGN_SHIFT) - 1) == 0,
            "census start {ptr:#x} is outside or misaligned in its block \
             {:#x}+{:#x}",
            block.base,
            block.extent
        );
        if block.sorted {
            debug_assert!(self.large_starts.last().is_none_or(|&last| last < ptr));
            self.large_starts.push(ptr);
            block.len += 1;
        } else {
            let bit = header_offset >> CENSUS_START_ALIGN_SHIFT;
            // SAFETY: `header_offset < extent` (asserted above), so the word
            // index is below the block's word count.
            unsafe {
                *block.word_ptr(bit >> 6) |= 1u64 << (bit & 63);
            }
        }
        self.arena_count += 1;
        self.record_pointer_range(ptr);
    }

    /// Build the direct-mapped block index once the census is complete. It is
    /// left empty — `census_block_at` keeps the binary search — when two block
    /// bases share a window (only fabricated test sets do) or the blocks span
    /// more than `CENSUS_BLOCK_WINDOW_MAX` windows.
    pub(super) fn build_block_windows(&mut self) {
        self.block_windows.clear();
        let (Some(first), Some(last)) = (self.arena_blocks.first(), self.arena_blocks.last())
        else {
            return;
        };
        let lo = first.base >> CENSUS_BLOCK_WINDOW_SHIFT;
        let hi = last.base.saturating_add(last.extent.max(1) - 1) >> CENSUS_BLOCK_WINDOW_SHIFT;
        let windows = hi - lo + 1;
        if windows > CENSUS_BLOCK_WINDOW_MAX || self.arena_blocks.len() >= u32::MAX as usize {
            return;
        }
        let mut table = vec![(u32::MAX, u32::MAX); windows];
        let mut previous = usize::MAX;
        for (idx, block) in self.arena_blocks.iter().enumerate() {
            let window = (block.base >> CENSUS_BLOCK_WINDOW_SHIFT) - lo;
            if window == previous {
                return;
            }
            table[window].1 = idx as u32;
            previous = window;
        }
        let mut greatest = u32::MAX;
        let mut next = 0usize;
        for (window, entry) in table.iter_mut().enumerate() {
            let window_start = (lo + window) << CENSUS_BLOCK_WINDOW_SHIFT;
            while next < self.arena_blocks.len() && self.arena_blocks[next].base <= window_start {
                greatest = next as u32;
                next += 1;
            }
            entry.0 = greatest;
        }
        self.block_windows = table;
        self.block_windows_lo = lo;
    }

    pub(super) fn push_malloc(&mut self, ptr: usize) {
        if self.classifier_mode {
            return; // #6179: no exact census in classifier mode
        }
        self.malloc_lookup.insert(ptr);
        self.record_pointer_range(ptr);
    }

    /// Total censused entries (arena starts + malloc starts).
    #[cfg(test)]
    pub(super) fn lookup_count(&self) -> usize {
        self.arena_count + self.malloc_lookup.len()
    }

    pub(super) fn record_tenured_nursery_bytes(&mut self, bytes: usize) {
        self.tenured_nursery_bytes += bytes;
    }
    pub(super) fn tenured_nursery_bytes(&self) -> usize {
        self.tenured_nursery_bytes
    }
    /// Transient heap bytes the arena membership index holds (fences, block
    /// table, bitmaps, oversized-block start lists).
    #[cfg(test)]
    pub(super) fn arena_index_bytes(&self) -> usize {
        self.arena_blocks.capacity() * std::mem::size_of::<CensusStartBlock>()
            + self.arena_block_bases.capacity() * std::mem::size_of::<usize>()
            + self
                .start_bitmap_chunks
                .iter()
                .map(|chunk| chunk.capacity() * std::mem::size_of::<u64>())
                .sum::<usize>()
            + self.start_bitmap_chunks.capacity() * std::mem::size_of::<Vec<u64>>()
            + self.large_starts.capacity() * std::mem::size_of::<usize>()
    }

    #[inline(always)]
    pub(super) fn record_pointer_range(&mut self, ptr: usize) {
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
        // Exact lookup. Arena starts answer from the per-block start bitmaps;
        // only malloc-tracked starts, which have no usable order, need the
        // B-tree. Arena first because arena hits dominate every workload that
        // reaches here — a malloc pointer pays one extra fence search.
        let exact = self.arena_start_censused(*ptr)
            || (!self.malloc_lookup.is_empty() && self.malloc_lookup.contains(ptr));
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
        let block = self.census_block_at(ptr)?;
        let candidate = self.floor_start_in_block(block, ptr)?;
        unsafe {
            let header = (candidate as *const u8).sub(GC_HEADER_SIZE) as *const GcHeader;
            let total = (*header).size as usize;
            let payload_end = candidate + total.saturating_sub(GC_HEADER_SIZE);
            if ptr >= candidate && ptr < payload_end {
                self.block_census.note_reached(block.block_idx);
                Some(candidate)
            } else {
                None
            }
        }
    }

    /// The census block whose base is the greatest one `<= ptr`, if any.
    /// `ptr` may still lie past that block's walked extent.
    ///
    /// #10182: answered from the direct-mapped block index when one was built.
    /// The window holding `ptr` names the greatest base at or below the window
    /// start and the one base, if any, inside the window, so the greatest base
    /// at or below `ptr` is the inside one when `ptr` has reached it. Past the
    /// last window every base is below `ptr`; before the first none is.
    #[inline(always)]
    fn census_block_at(&self, ptr: usize) -> Option<&CensusStartBlock> {
        if !self.block_windows.is_empty() {
            let window = ptr >> CENSUS_BLOCK_WINDOW_SHIFT;
            let idx = match self
                .block_windows
                .get(window.wrapping_sub(self.block_windows_lo))
            {
                Some(&(below, inside)) => {
                    #[cfg(test)]
                    if block_window_sabotage::ignoring_inside() {
                        return self.arena_blocks.get(below as usize);
                    }
                    if inside != u32::MAX && ptr >= self.arena_block_bases[inside as usize] {
                        inside
                    } else {
                        below
                    }
                }
                None if window < self.block_windows_lo => return None,
                None => return self.arena_blocks.last(),
            };
            return self.arena_blocks.get(idx as usize);
        }
        self.census_block_at_by_search(ptr)
    }

    #[inline(always)]
    fn census_block_at_by_search(&self, ptr: usize) -> Option<&CensusStartBlock> {
        let idx = self.arena_block_bases.partition_point(|&base| base <= ptr);
        if idx == 0 {
            return None;
        }
        self.arena_blocks.get(idx - 1)
    }

    /// `(direct-mapped answer, binary-search answer)` as block bases, for the
    /// block-index test.
    #[cfg(test)]
    pub(super) fn census_block_base_both_ways(&self, ptr: usize) -> (Option<usize>, Option<usize>) {
        (
            self.census_block_at(ptr).map(|b| b.base),
            self.census_block_at_by_search(ptr).map(|b| b.base),
        )
    }

    /// Exact arena membership: a census hit also records that the trace
    /// reached the hit's block (#10182; see `block_skip`'s module doc for why
    /// every mark passes through here).
    #[inline]
    fn arena_start_censused(&self, ptr: usize) -> bool {
        let Some(block) = self.census_block_at(ptr) else {
            return false;
        };
        let hit = if block.sorted {
            self.large_starts[block.first..block.first + block.len]
                .binary_search(&ptr)
                .is_ok()
        } else {
            // Below `base + GC_HEADER_SIZE` the subtraction wraps past every
            // extent, so one compare rejects both ends of the block.
            let header_offset = ptr.wrapping_sub(block.base).wrapping_sub(GC_HEADER_SIZE);
            if header_offset >= block.extent
                || header_offset & ((1 << CENSUS_START_ALIGN_SHIFT) - 1) != 0
            {
                false
            } else {
                let bit = header_offset >> CENSUS_START_ALIGN_SHIFT;
                #[cfg(test)]
                let bit = start_bitmap_sabotage::shift_probe(bit);
                // SAFETY: `header_offset < extent`, so the word index is below
                // the block's word count (the sabotaged probe in test builds
                // may step one bit past it, still inside the last word).
                let word_idx = bit >> 6;
                word_idx < block.len
                    && unsafe { (*block.word_ptr(word_idx) >> (bit & 63)) & 1 != 0 }
            }
        };
        if hit {
            self.block_census.note_reached(block.block_idx);
        }
        hit
    }

    /// Greatest censused start (user pointer) in `block` that is `<= ptr`.
    fn floor_start_in_block(&self, block: &CensusStartBlock, ptr: usize) -> Option<usize> {
        if block.sorted {
            let starts = &self.large_starts[block.first..block.first + block.len];
            return Self::find_floor(starts, ptr);
        }
        if block.extent == 0 {
            return None;
        }
        // A start `s <= ptr` has its header at `s - GC_HEADER_SIZE`, so the
        // highest candidate header offset is `ptr - base - GC_HEADER_SIZE`,
        // clamped to the last walked byte.
        let header_offset = ptr.checked_sub(block.base)?.checked_sub(GC_HEADER_SIZE)?;
        let bit = header_offset.min(block.extent - 1) >> CENSUS_START_ALIGN_SHIFT;
        let mut word_idx = bit >> 6;
        let top = bit & 63;
        let keep = if top == 63 {
            u64::MAX
        } else {
            (1u64 << (top + 1)) - 1
        };
        // SAFETY: `bit` derives from an offset below `extent`, and every
        // later index is smaller.
        let mut word = unsafe { *block.word_ptr(word_idx) } & keep;
        loop {
            if word != 0 {
                let floor_bit = word_idx * 64 + (63 - word.leading_zeros() as usize);
                return Some(block.base + (floor_bit << CENSUS_START_ALIGN_SHIFT) + GC_HEADER_SIZE);
            }
            if word_idx == 0 {
                return None;
            }
            word_idx -= 1;
            word = unsafe { *block.word_ptr(word_idx) };
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

/// Sabotage switch for the block-index test: a lookup ignores the base inside
/// the window and answers with the block below it. Test builds only.
#[cfg(test)]
pub(crate) mod block_window_sabotage {
    use std::cell::Cell;

    thread_local! {
        static IGNORE_INSIDE: Cell<bool> = const { Cell::new(false) };
    }

    #[inline]
    pub(crate) fn ignoring_inside() -> bool {
        IGNORE_INSIDE.with(Cell::get)
    }

    pub(crate) struct Guard(bool);

    impl Guard {
        pub(crate) fn arm() -> Self {
            Self(IGNORE_INSIDE.with(|s| s.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            IGNORE_INSIDE.with(|s| s.set(self.0));
        }
    }
}

/// Sabotage switch for the whole-block census test: the one-pass walk skips
/// recording every start bit. Test builds only.
#[cfg(test)]
pub(crate) mod whole_block_census_sabotage {
    use std::cell::Cell;

    thread_local! {
        static DROP_STARTS: Cell<bool> = const { Cell::new(false) };
    }

    #[inline]
    pub(crate) fn dropping_starts() -> bool {
        DROP_STARTS.with(Cell::get)
    }

    /// Arms the dropped starts until the guard drops.
    pub(crate) struct Guard(bool);

    impl Guard {
        pub(crate) fn arm() -> Self {
            Self(DROP_STARTS.with(|s| s.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            DROP_STARTS.with(|s| s.set(self.0));
        }
    }
}

/// Sabotage switch for the start-bitmap tests: shifts every bitmap probe by
/// one alignment unit, so a test can show its membership oracle notices a
/// bitmap that answers for the wrong address. Test builds only.
#[cfg(test)]
pub(crate) mod start_bitmap_sabotage {
    use std::cell::Cell;

    thread_local! {
        static SHIFT_PROBE: Cell<bool> = const { Cell::new(false) };
    }

    #[inline]
    pub(crate) fn shift_probe(bit: usize) -> usize {
        if SHIFT_PROBE.with(Cell::get) {
            bit + 1
        } else {
            bit
        }
    }

    /// Arms the shifted probe until the guard drops.
    pub(crate) struct Guard(bool);

    impl Guard {
        pub(crate) fn arm() -> Self {
            Self(SHIFT_PROBE.with(|s| s.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            SHIFT_PROBE.with(|s| s.set(self.0));
        }
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
    /// #10182: census the per-block facts `block_skip` needs (exact census
    /// only), and the block the walk is currently inside.
    census_armed: bool,
    census_block_idx: usize,
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
    pub(super) arena_block_count: usize,
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
        b.set.block_census = BlockCensus::disarmed();
        b.census_armed = false;
        b
    }

    pub(super) fn new() -> Self {
        let mut set = ValidPointerSet::new();
        set.block_census = BlockCensus::armed();
        Self {
            set,
            census_armed: true,
            census_block_idx: usize::MAX,
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
                    if unbounded && self.walk_whole_blocks() {
                        self.phase = ValidPointerSetBuildPhase::MallocWalk;
                        continue;
                    }
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
                    self.set.block_census.flush_block();
                    self.set.build_block_windows();
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
            let Some((header_ptr, block_idx)) = next else {
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
            if block_idx != self.census_block_idx {
                self.census_block_idx = block_idx;
                if let Some((_, data, offset)) = self
                    .arena_cursor
                    .as_ref()
                    .and_then(crate::arena::ArenaObjectCursor::current_block_extent)
                {
                    self.set.begin_arena_block(
                        u32::try_from(block_idx).unwrap_or(u32::MAX),
                        data,
                        offset,
                    );
                    if self.census_armed {
                        self.set.block_census.begin_block(block_idx, data, offset);
                    }
                }
            }
            if self.census_armed {
                unsafe {
                    self.set
                        .block_census
                        .note_header(header_ptr as *const GcHeader);
                }
            }
            self.record_arena_header(header_ptr);
        }
        false
    }

    /// An unbudgeted census walks each block in one pass
    /// ([`Self::census_whole_block`]) instead of one cursor call per object:
    /// the census reads every header of the arena, and on a pacing full over
    /// two promoted 20 MB JSON trees its per-object overhead, not the memory
    /// traffic, was three quarters of the phase (#10182). Returns false, and
    /// walks nothing, when the per-object path must be kept: classifier mode
    /// records no starts, and a cursor a budgeted step left inside a block
    /// resumes mid-block.
    fn walk_whole_blocks(&mut self) -> bool {
        if self.set.classifier_mode
            || !self
                .arena_cursor
                .as_ref()
                .is_some_and(crate::arena::ArenaObjectCursor::at_block_boundary)
        {
            return false;
        }
        loop {
            let next = self
                .arena_cursor
                .as_mut()
                .expect("arena cursor exists during arena walk")
                .next_whole_block();
            let Some((block_idx, data, offset, size)) = next else {
                self.arena_cursor = None;
                return true;
            };
            // SAFETY: the cursor snapshotted this block for this census, and
            // nothing runs between that snapshot and this walk.
            unsafe { self.census_whole_block(block_idx, data, offset, size) };
        }
    }

    /// Census one whole arena block in a single pass. It yields exactly the
    /// headers `ArenaObjectCursor::next_budgeted` yields for the block (same
    /// alignment, stop conditions and walkability filter) and records exactly
    /// what `step_arena_walk` records for each of them; the per-block constants
    /// (the bitmap chunks, the nursery classification, the pointer range) are
    /// hoisted out of the per-object loop.
    ///
    /// # Safety
    /// `data`/`offset`/`size` are an arena block as the census cursor
    /// snapshotted it.
    unsafe fn census_whole_block(
        &mut self,
        block_idx: usize,
        data: usize,
        offset: usize,
        size: usize,
    ) {
        // #10182: a block the in-place promotion walk of this very safepoint
        // recorded is adopted instead of walked (see `adopt_census`).
        if self.adopt_promoted_block(block_idx, data, offset, size) {
            return;
        }
        self.walk_census_block(block_idx, data, offset, size);
    }

    /// The census walk of one whole block (see [`Self::census_whole_block`]).
    ///
    /// # Safety
    /// As `census_whole_block`.
    pub(super) unsafe fn walk_census_block(
        &mut self,
        block_idx: usize,
        data: usize,
        offset: usize,
        size: usize,
    ) {
        let mut cursor = 0usize;
        let mut begun = false;
        let mut bitmap: Option<[*mut u64; CENSUS_BITMAP_MAX_CHUNKS]> = None;
        let mut nursery = false;
        let mut first_start = 0usize;
        let mut last_start = 0usize;
        let mut bitmap_starts = 0usize;
        let mut tenured_bytes = 0usize;
        let mut non_walkable_before_first_object = false;
        while cursor < offset {
            let aligned = (cursor + 7) & !7;
            if aligned >= offset {
                break;
            }
            let header = (data + aligned) as *const GcHeader;
            let total_size = (*header).size as usize;
            if total_size == 0 || total_size > size {
                break;
            }
            cursor = aligned + total_size;
            if !crate::gc::gc_type_is_arena_walkable((*header).obj_type) {
                if begun {
                    self.set.block_census.note_non_walkable();
                } else {
                    non_walkable_before_first_object = true;
                }
                continue;
            }
            let user_ptr = data + aligned + GC_HEADER_SIZE;
            if !begun {
                begun = true;
                self.census_block_idx = block_idx;
                self.set.begin_arena_block(
                    u32::try_from(block_idx).unwrap_or(u32::MAX),
                    data,
                    offset,
                );
                if self.census_armed {
                    self.set.block_census.begin_block(block_idx, data, offset);
                    self.set.block_census.note_whole_block_walk();
                    if non_walkable_before_first_object {
                        self.set.block_census.note_non_walkable();
                    }
                }
                let block = self
                    .set
                    .arena_blocks
                    .last()
                    .expect("census block just opened");
                if !block.sorted {
                    bitmap = Some(block.chunks);
                }
                // Every object of the block has the block's classification.
                nursery = crate::arena::pointer_in_nursery(user_ptr);
                first_start = user_ptr;
            }
            if self.census_armed {
                self.set.block_census.note_header(header);
            }
            match bitmap {
                Some(chunks) => {
                    #[cfg(test)]
                    if whole_block_census_sabotage::dropping_starts() {
                        last_start = user_ptr;
                        continue;
                    }
                    // `aligned` is a multiple of 8 below `offset`, the block's
                    // extent: the bit and its word are inside the bitmap.
                    let bit = aligned >> CENSUS_START_ALIGN_SHIFT;
                    let word = bit >> 6;
                    *chunks[word >> CENSUS_BITMAP_CHUNK_WORD_SHIFT]
                        .add(word & (CENSUS_BITMAP_CHUNK_WORDS - 1)) |= 1u64 << (bit & 63);
                    bitmap_starts += 1;
                }
                None => self.set.push_arena(user_ptr),
            }
            last_start = user_ptr;
            let flags = (*header).gc_flags;
            if nursery && flags & GC_FLAG_TENURED != 0 && flags & GC_FLAG_FORWARDED == 0 {
                tenured_bytes += total_size;
            }
        }
        if begun {
            if bitmap.is_some() {
                self.set.arena_count += bitmap_starts;
                self.set.record_pointer_range(first_start);
                self.set.record_pointer_range(last_start);
            }
            self.set.record_tenured_nursery_bytes(tenured_bytes);
        }
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
            arena_block_count: self.set.arena_blocks.len(),
            lookup_count: self.set.lookup_count(),
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
    // An unfinished budgeted cycle is owned by another TLS value. During
    // thread teardown its Drop may run after MARK_SEEDS has already been
    // destroyed; at that point there is no surviving mutator that could
    // consume these seeds. `try_with` keeps normal cleanup identical while
    // making the teardown path order-independent.
    let _ = MARK_SEEDS.try_with(|cell| unsafe {
        (*cell.get()).clear();
    });
}

#[inline]
pub(crate) fn try_mark_value(value_bits: u64, valid_ptrs: &ValidPointerSet) -> bool {
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

    if super::full_trace::handle_trace_active()
        && super::full_trace::observe_handle(value_bits, valid_ptrs)
    {
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
    if super::full_trace::handle_trace_active()
        && super::full_trace::observe_handle(addr as u64, valid_ptrs)
    {
        return false;
    }
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

#[inline(always)]
pub(super) unsafe fn mark_field_into_worklist(
    val_bits: u64,
    valid_ptrs: &ValidPointerSet,
    worklist: &mut Vec<*mut GcHeader>,
    proxy_trace_active: bool,
) -> bool {
    // `proxy_trace_active` is `super::full_trace::handle_trace_active()`, read by
    // the caller once for the whole object being traced (#10182).
    if proxy_trace_active && super::full_trace::observe_handle(val_bits, valid_ptrs) {
        return false;
    }
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
    // #10182: tracing a pointer-free object that is not a forwarding stub does
    // nothing — `trace_one_worklist_header` would only follow a FORWARDED hop,
    // and a leaf descriptor visits no slot — so it is marked and not queued.
    // Strings are half the objects of a JSON tree.
    #[cfg(not(test))]
    let forwarded = flags & GC_FLAG_FORWARDED != 0;
    #[cfg(test)]
    let forwarded = flags & GC_FLAG_FORWARDED != 0 && !leaf_mark_sabotage::ignoring_forwarding();
    // #10362: a pointer-free array yields no slot either, and on a chain-node
    // heap it is half the traced objects — the obj_type-keyed leaf test above
    // cannot see them, because they are arrays and not the strings it was
    // written for.
    //
    // ONLY WHEN NO PROXY TRACE IS ACTIVE. `trace_heap_rewrite_slots` reads
    // every word of a POINTER-FREE payload through `gc_observe_traced_value`
    // when `proxy_trace_active`, because a proxy id is a POINTER_TAG value in
    // the proxy-id band and not a heap pointer — which is exactly why the
    // layout mask calls that payload pointer free. Skipping the object would
    // leave the entry unobserved, `gc_finish_full_trace` would prune it, and a
    // LIVE proxy's target and handler would be collected. The other two
    // consumers of this predicate ignore `PointerFreeRange` and carry no such
    // term; the asymmetry is deliberate.
    #[cfg(not(test))]
    let proxy_gate = proxy_trace_active;
    #[cfg(test)]
    let proxy_gate = proxy_trace_active && !zero_slot_skip_sabotage::respecting_proxy_gate();
    if !forwarded
        && (gc_type_rewrite_descriptor_kind((*header).obj_type) == GcRewriteDescriptorKind::Leaf
            || (!proxy_gate && gc_object_yields_no_child_slots(header)))
    {
        return true;
    }
    // Push directly onto the caller's worklist. No MARK_SEEDS push —
    // that's only needed for root-phase callers that don't own a
    // worklist (mark_mutable_root_slots, mark_registered_roots,
    // mark_remembered_set_roots, mark_stack_roots). The trace drain
    // already owns and consumes this worklist.
    worklist.push(header);
    true
}

/// Sabotage switches for the zero-slot skip (#10362). Test builds only.
///
/// `respecting_proxy_gate` DISARMS the `!proxy_trace_active` term, i.e. makes
/// the full mark skip a pointer-free array even while a proxy trace is running.
/// That is the defect the gate exists to prevent, and
/// `gc::tests::zero_slot_skip` requires it to strand a live proxy's target.
#[cfg(test)]
pub(crate) mod zero_slot_skip_sabotage {
    use std::cell::Cell;

    thread_local! {
        static IGNORE_PROXY_GATE: Cell<bool> = const { Cell::new(false) };
    }

    #[inline]
    pub(crate) fn respecting_proxy_gate() -> bool {
        IGNORE_PROXY_GATE.with(Cell::get)
    }

    pub(crate) struct Guard(bool);

    impl Guard {
        pub(crate) fn arm() -> Self {
            Self(IGNORE_PROXY_GATE.with(|s| s.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            let prior = self.0;
            IGNORE_PROXY_GATE.with(|s| s.set(prior));
        }
    }
}

/// Sabotage switch for the leaf-mark test: a forwarded pointer-free object is
/// not queued either, so its forwarding hop is never followed. Test builds
/// only.
#[cfg(test)]
pub(crate) mod leaf_mark_sabotage {
    use std::cell::Cell;

    thread_local! {
        static IGNORE_FORWARDING: Cell<bool> = const { Cell::new(false) };
    }

    #[inline]
    pub(crate) fn ignoring_forwarding() -> bool {
        IGNORE_FORWARDING.with(Cell::get)
    }

    pub(crate) struct Guard(bool);

    impl Guard {
        pub(crate) fn arm() -> Self {
            Self(IGNORE_FORWARDING.with(|s| s.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            IGNORE_FORWARDING.with(|s| s.set(self.0));
        }
    }
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
        // #10182: the drain visits headers in queue order and each one is a
        // cold DRAM read on a heap larger than the cache (a 20 MB JSON tree);
        // start the read of the entry a few places ahead, as the copying
        // minor's drain already does. A prefetch cannot fault.
        if let Some(&ahead) = worklist.get(*cursor + super::prefetch::PREFETCH_DISTANCE) {
            super::prefetch::prefetch_read(ahead as usize);
        }
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
        // #6228: a FORWARDED header (array growth installs PERMANENT
        // forwarding stubs — types.rs set_forwarding_address — so stale
        // pre-growth pointers keep resolving for reads) must propagate
        // liveness to its target instead of tracing as zero-children. The
        // deforestation pass manufactures exactly such a stale pointer as
        // the ONLY reference (direct-call `var b = build(n)`), and without
        // this hop the live post-growth array was swept: length 0 / NaN
        // reads past 32 MiB. Mirrors the (previously dead-code) trace_array
        // path, plus MARKING the target — worklist membership alone does
        // not protect it from the sweep.
        if (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
            let new_user = forwarding_address(header) as usize;
            if new_user >= 0x1000 && valid_ptrs.contains(&new_user) {
                let new_header = header_from_user_ptr(new_user as *const u8);
                if (*new_header).gc_flags & GC_FLAG_MARKED == 0 {
                    (*new_header).gc_flags |= GC_FLAG_MARKED;
                    worklist.push(new_header);
                }
            }
            return;
        }
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
#[allow(dead_code)] // test scaffolding: exercised only by gc::tests::layout_trace under cfg(test)
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

crate::perry_thread_local! {
    /// Objects this thread's block-persistence pass has FORCE-MARKED since
    /// process start — i.e. kept alive for no reason other than sharing a
    /// block with something reachable.
    ///
    /// The census exists for the same reason `gc::scan_fallback`'s does
    /// (#7148): force-marking is a correctness measure with a *semantic* side
    /// effect — an unrooted object in a persisting block is not dead, so every
    /// death-keyed consumer (the `dead_owner` side-table prunes, the Map/Set
    /// registry sweeps) legitimately declines to act on it. A test that
    /// asserts "this unrooted owner is dead" is therefore asserting something
    /// about the tenancy of its arena block, and without this counter it has no
    /// way to say so: the mark is cleared again by the sweep, so nothing
    /// observable survives the collection. #7975 is what that costs — two
    /// `dead_owner_side_tables` cases failed 200/200 when the lazy `globalThis`
    /// bootstrap landed in their block, and the failure named the prune.
    ///
    /// Updated once per pass (per fixed-point round), never per object.
    static BLOCK_PERSIST_FORCE_MARKS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Running count of [`BLOCK_PERSIST_FORCE_MARKS`] on this thread. Compare
/// across a collection: an unchanged value means block persistence force-marked
/// nothing, so an object left unmarked by that collection was genuinely
/// unreachable rather than merely a neighbour of something reachable.
#[cfg(test)]
pub(crate) fn block_persist_force_mark_count() -> u64 {
    BLOCK_PERSIST_FORCE_MARKS.with(std::cell::Cell::get)
}

/// Record one pass's force-mark count. BOTH arms call this: the whole-cycle
/// pass below and the budgeted `BlockPersistCycleState` in `gc::cycle`, which
/// force-marks through its own loop. A census that only counted one of them
/// would read zero on exactly the cycles that are hardest to reason about.
pub(super) fn note_block_persist_force_marks(marked: usize) {
    if marked != 0 {
        BLOCK_PERSIST_FORCE_MARKS
            .with(|count| count.set(count.get().saturating_add(marked as u64)));
    }
}

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
        note_block_persist_force_marks(newly_marked);

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
    // #10182: two per-object facts read once instead of once per slot —
    // whether the proxy registry observes this trace (it changes only when a
    // proxy is created, and none is created inside one object's visit), and
    // whether the object is one of the weak-holder classes whose weak slots
    // the trace skips (its class cannot change while it is traced). Range
    // descriptors are walked here directly rather than through a per-slot
    // dynamic callback.
    let proxy_trace_active = super::full_trace::handle_trace_active();
    #[cfg(not(test))]
    let weak_holder = crate::weakref::is_weak_holder_header(header);
    #[cfg(test)]
    let weak_holder =
        crate::weakref::is_weak_holder_header(header) && !mark_hoist_sabotage::forgetting_weak();
    visit_gc_rewrite_slot_descriptors(header, |descriptor| unsafe {
        let mut visit_slot = |slot: *mut u64, layout_kind: Option<HeapChildSlotReadKind>| {
            if weak_holder && crate::weakref::is_weak_target_trace_slot(header, slot) {
                return;
            }
            if let Some(kind) = layout_kind {
                record_layout_child_slot_read(kind);
                record_trace_slot_read();
            }
            mark_field_into_worklist(*slot, valid_ptrs, worklist, proxy_trace_active);
        };
        match descriptor {
            GcMutableSlotDescriptor::PointerFreeRange(range) => {
                if proxy_trace_active {
                    for i in 0..range.slot_count() {
                        super::full_trace::observe_handle(*range.slot(i), valid_ptrs);
                    }
                }
            }
            GcMutableSlotDescriptor::Slot(slot) => visit_slot(slot.slot, slot.layout_kind),
            GcMutableSlotDescriptor::Range { range, layout_kind } => {
                // Start the header reads of the range's pointer children
                // before marking any of them: each is a cold DRAM read the
                // mark would otherwise take one at a time. A prefetch cannot
                // fault, so the candidate need not be proven a pointer yet.
                for i in 0..range.slot_count() {
                    let bits = *range.slot(i);
                    let tag = bits & TAG_MASK;
                    if tag == POINTER_TAG || tag == STRING_TAG {
                        super::prefetch::prefetch_read(
                            ((bits & POINTER_MASK) as usize).wrapping_sub(GC_HEADER_SIZE),
                        );
                    }
                }
                for i in 0..range.slot_count() {
                    visit_slot(range.slot(i), layout_kind);
                }
            }
        }
    });
}

/// Sabotage switch for the mark-hoist test: the per-object weak-holder fact
/// reads false, so a weak holder's weak slots are traced strongly. Test builds
/// only.
#[cfg(test)]
pub(crate) mod mark_hoist_sabotage {
    use std::cell::Cell;

    thread_local! {
        static FORGET_WEAK: Cell<bool> = const { Cell::new(false) };
    }

    #[inline]
    pub(crate) fn forgetting_weak() -> bool {
        FORGET_WEAK.with(Cell::get)
    }

    pub(crate) struct Guard(bool);

    impl Guard {
        pub(crate) fn arm() -> Self {
            Self(FORGET_WEAK.with(|s| s.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            FORGET_WEAK.with(|s| s.set(self.0));
        }
    }
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
