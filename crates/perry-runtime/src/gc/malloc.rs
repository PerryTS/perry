use super::*;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct MallocKindTelemetry {
    pub(super) allocated_count: u64,
    pub(super) allocated_bytes: u64,
    pub(super) realloc_count: u64,
    pub(super) realloc_old_bytes: u64,
    pub(super) realloc_new_bytes: u64,
    pub(super) freed_count: u64,
    pub(super) freed_bytes: u64,
    pub(super) survivor_count: u64,
    pub(super) survivor_bytes: u64,
    pub(super) copied_minor_validation_lookups: u64,
}

impl MallocKindTelemetry {
    pub(super) const fn zero() -> Self {
        Self {
            allocated_count: 0,
            allocated_bytes: 0,
            realloc_count: 0,
            realloc_old_bytes: 0,
            realloc_new_bytes: 0,
            freed_count: 0,
            freed_bytes: 0,
            survivor_count: 0,
            survivor_bytes: 0,
            copied_minor_validation_lookups: 0,
        }
    }

    #[cfg(feature = "diagnostics")]
    pub(super) fn reset_cycle_deltas(&mut self) {
        self.allocated_count = 0;
        self.allocated_bytes = 0;
        self.realloc_count = 0;
        self.realloc_old_bytes = 0;
        self.realloc_new_bytes = 0;
        self.freed_count = 0;
        self.freed_bytes = 0;
        self.copied_minor_validation_lookups = 0;
    }
}

#[inline]
pub(super) fn malloc_kind_index(obj_type: u8) -> usize {
    if gc_type_info(obj_type).is_some() {
        obj_type as usize
    } else {
        MALLOC_KIND_UNKNOWN_INDEX
    }
}

/// `gc_malloc` touched four separate thread-local slots (`GC_IN_ALLOC`,
/// `MALLOC_OBJECTS`, `MALLOC_SET`, `GC_IN_ALLOC` again) plus two RefCell
/// panic-check borrows. Each TLS lookup on macOS/ARM costs ~30-40ns because it
/// goes through `pthread_getspecific`, so per-allocation overhead was dominated
/// by dispatch, not the actual tracking work. Bundling the two tracked
/// collections into one `RefCell<MallocState>` (and `GC_IN_ALLOC` /
/// `GC_SUPPRESSED` into a single `Cell<u8>` below) collapses the hot path from
/// 4 TLS + 2 borrow_mut to 3 TLS + 1 borrow_mut, with the adjacent `objects`
/// and `set` fields sharing a single cacheline for better locality.
pub(crate) struct MallocState {
    /// Malloc-allocated objects tracked for GC (closures/promises/maps/errors/compatibility residents/…)
    pub(crate) objects: Vec<*mut GcHeader>,
    /// O(1) exact header registry for validating malloc pointers. It starts
    /// inactive so malloc-heavy workloads that never need pointer validation
    /// pay only the `objects.push` cost. The first caller that needs exact
    /// validation (`gc_realloc`, tests, or future non-copying validation paths)
    /// activates the registry by rebuilding it from `objects`; after that,
    /// allocation, realloc, and sweep keep it synchronized inline.
    pub(crate) set: crate::fast_hash::PtrHashSet<usize>,
    /// Headers moved by `gc_realloc` after a malloc sweep snapshot starts.
    /// Incremental malloc sweep resolves snapshot headers through this map
    /// before dereferencing so a paused sweep never follows a freed realloc
    /// source pointer.
    pub(crate) realloc_forwarding: crate::fast_hash::PtrHashMap<usize, usize>,
    /// Original headers in the currently active malloc sweep snapshot.
    /// This prevents reallocs of new allocations that reuse old addresses from
    /// replacing a snapshot object's forwarding entry.
    pub(crate) realloc_snapshot_headers: crate::fast_hash::PtrHashSet<usize>,
    /// Registry availability/consistency. Copied-minor GC may consult an
    /// already-active exact registry, but must never rebuild it on the fast
    /// path because that would scale with total malloc churn.
    pub(super) registry_state: MallocRegistryState,
    /// One-shot latch for the small-start → heavy-capacity growth (see
    /// `MALLOC_STATE_INITIAL_CAPACITY`). Stays latched even if a sweep
    /// later drops `objects.len()` back under the threshold — a thread
    /// that was malloc-heavy once keeps the reserved capacity.
    pub(super) heavy_capacity_reserved: bool,
    pub(super) kind_telemetry: [MallocKindTelemetry; MALLOC_KIND_BUCKET_COUNT],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MallocRegistryState {
    Inactive,
    ActiveConsistent,
}

impl MallocState {
    /// Reclaim every malloc-tracked object block this state still owns.
    ///
    /// Runs at thread exit via `Drop`. Returns the number of bytes freed
    /// (surfaced for tests). Empties `objects` and `set` so the drop is
    /// idempotent.
    ///
    /// # Why this is sound at thread exit (2026-07-09 GC audit §6 / #6185)
    /// `MALLOC_STATE` is `thread_local!`, so this only runs while the owning
    /// thread is being torn down:
    /// - **Worker threads** (`spawn` / `parallelMap` / `parallelFilter`): by
    ///   the time TLS is destroyed the worker's result has already crossed the
    ///   boundary as an owned `SerializedValue` deep-copy — no other thread
    ///   holds a raw pointer into this heap — so freeing these blocks cannot
    ///   create a dangling reference elsewhere. Without this, every
    ///   promise/map/error/large-closure a worker allocated leaked at exit (the
    ///   first malloc-count GC needs 100k objects, so per-request workers never
    ///   collected once).
    /// - **The main thread**: its `MALLOC_STATE` is not dropped until process
    ///   teardown, never mid-program, so this cannot yank live objects out from
    ///   under running code.
    ///
    /// # Why finalizers are deliberately NOT run here
    /// The sweep path pairs `dealloc` with `gc_type_finalize_unmarked_payload` /
    /// `layout_clear_for_ptr`, which reach into *other* thread-locals
    /// (`MAP_REGISTRY`, `MAP_INDEX`, `PROMISE_CONTEXTS`, the async-hooks queues,
    /// …). Thread-local destruction order is unspecified, so touching one of
    /// those during this Drop could hit an already-destroyed TLS and panic
    /// ("cannot access a Thread Local Storage value during or after
    /// destruction"), aborting the thread. We therefore reclaim only the object
    /// blocks themselves — the audit's primary worker-exit leak. Any external
    /// side-allocations those objects own (a Map's entry table, an error's side
    /// tables) are out of scope for this mechanical fix. This also avoids the
    /// re-entrant `MALLOC_STATE.with(...)` the sweep bookkeeping performs.
    ///
    /// Pinned objects are skipped, mirroring `process_sweep_header`, so a
    /// cross-thread promise pinned for an in-flight result is never yanked.
    fn free_all_tracked_objects(&mut self) -> u64 {
        let mut freed_bytes: u64 = 0;
        for header in self.objects.drain(..) {
            if header.is_null() {
                continue;
            }
            // SAFETY: every entry in `objects` is a live `gc_malloc` header
            // (GcHeader-prefixed block) until freed here; this loop frees each
            // exactly once and the thread is exiting, so no concurrent access.
            unsafe {
                if (*header).gc_flags & GC_FLAG_PINNED != 0 {
                    continue;
                }
                let total_size = (*header).size as usize;
                if total_size == 0 {
                    continue;
                }
                let layout = Layout::from_size_align(total_size, 8).unwrap();
                dealloc(header as *mut u8, layout);
                freed_bytes = freed_bytes.saturating_add(total_size as u64);
            }
        }
        self.set.clear();
        freed_bytes
    }
}

impl Drop for MallocState {
    fn drop(&mut self) {
        // Free the worker thread's malloc objects instead of leaking them at
        // exit (audit §6 / #6185). See `free_all_tracked_objects` for the
        // thread-exit soundness and TLS-destruction-order argument.
        self.free_all_tracked_objects();
    }
}

/// Pre-allocated capacity for `MallocState.objects` and `.set`.
///
/// History: this used to be a flat 256 k (`MALLOC_STATE_HEAVY_CAPACITY`
/// below), sized for promise-heavy kernels (`promise_all_chains`
/// allocates ~200 k strings/closures/promises before the first GC)
/// where hashbrown's doubling rehashes at the ~100 k mark were the
/// single hottest leaf in the profile (15.6 % self-time on
/// `gc_malloc`'s caller chain). But that pre-sized a ~2 MB Vec plus a
/// ~4 MB PtrHashSet on EVERY JS-touching thread — spawn workers and
/// tokio callers that allocate a few hundred malloc objects paid a
/// ~6 MB floor for a benchmark they never run. Now: start small, and
/// the first time a thread's tracked-object count crosses
/// `MALLOC_STATE_HEAVY_LEN_THRESHOLD` (a genuinely malloc-heavy
/// thread), reserve straight to the heavy capacity in one step —
/// preserving the rehash-amortization rationale exactly where it
/// mattered while cutting the per-thread floor everywhere else.
pub(super) const MALLOC_STATE_INITIAL_CAPACITY: usize = 4096;
/// One-time growth trigger: a thread whose live tracked-object count
/// reaches this is on the promise-heavy profile — reserve the rest.
pub(super) const MALLOC_STATE_HEAVY_LEN_THRESHOLD: usize = 64 * 1024;
/// Capacity reserved once the heavy threshold trips (the old flat
/// pre-size; covers the 200 k-entry pre-GC working set at hashbrown's
/// 7/8 load factor without further rehashes).
pub(super) const MALLOC_STATE_HEAVY_CAPACITY: usize = 256 * 1024;

thread_local! {
    pub(crate) static MALLOC_STATE: RefCell<MallocState> = RefCell::new(MallocState {
        objects: Vec::with_capacity(MALLOC_STATE_INITIAL_CAPACITY),
        set: crate::fast_hash::PtrHashSet::with_capacity_and_hasher(
            MALLOC_STATE_INITIAL_CAPACITY,
            crate::fast_hash::PtrHasher,
        ),
        realloc_forwarding: crate::fast_hash::new_ptr_hash_map(),
        realloc_snapshot_headers: crate::fast_hash::new_ptr_hash_set(),
        registry_state: MallocRegistryState::Inactive,
        heavy_capacity_reserved: false,
        kind_telemetry: [MallocKindTelemetry::zero(); MALLOC_KIND_BUCKET_COUNT],
    });

    pub(crate) static ARENA_FREE_LIST: RefCell<Vec<(*mut u8, usize)>> = const { RefCell::new(Vec::new()) };
    pub(crate) static ARENA_FREE_LIST_NONEMPTY: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

pub fn gc_malloc(size: usize, obj_type: u8) -> *mut u8 {
    let total = GC_HEADER_SIZE + size;
    let layout = Layout::from_size_align(total, 8).unwrap();

    // Issue #34: malloc-heavy workloads that don't push arena blocks
    // (e.g. the `n = n * 10n + digit` bigint accumulator inside
    // @perry/postgres's `parseBigIntDecimal`, or a decode loop producing
    // many short-lived strings) never trigger GC via the arena slow path.
    // Without this call MALLOC_OBJECTS grows unboundedly.
    //
    // We run the check BEFORE `alloc` so the sweep can't free the about-
    // to-be-returned pointer — after `alloc` the fresh user pointer lives
    // only in a caller-saved register and the conservative stack scan
    // (`setjmp` only captures callee-saved regs) can't see it as a root.
    // Running before means the fresh allocation simply doesn't exist yet
    // during the GC cycle.
    gc_check_trigger();

    unsafe {
        let mut raw = alloc(layout);
        if raw.is_null() && super::gc_try_emergency_reclaim() {
            raw = alloc(layout);
        }
        if raw.is_null() {
            panic!(
                "gc_malloc: failed to allocate {} bytes (heap exhausted after emergency GC)",
                total
            );
        }

        let header = raw as *mut GcHeader;
        (*header).obj_type = obj_type;
        (*header).gc_flags = super::barrier::gc_birth_extra_flags(); // not arena; allocate-black while a budgeted cycle marks
        super::barrier::gc_note_black_birth(header);
        (*header)._reserved = 0;
        (*header).size = total as u32;

        let user_ptr = raw.add(GC_HEADER_SIZE);

        GC_FLAGS.with(|f| f.set(f.get() | GC_FLAG_IN_ALLOC));
        MALLOC_STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.objects.push(header);
            s.record_malloc_alloc(obj_type, 1, total as u64);
            if s.malloc_registry_available() {
                s.set.insert(header as usize);
            }
            s.maybe_reserve_heavy_capacity();
        });
        GC_FLAGS.with(|f| f.set(f.get() & !GC_FLAG_IN_ALLOC));

        user_ptr
    }
}

/// Batch-allocate multiple GC-tracked malloc objects in one go.
/// Amortises overhead: one `gc_check_trigger` call, one `MALLOC_OBJECTS`
/// extend, one `MALLOC_SET` extend — instead of N of each.
/// `sizes` contains the *payload* size for each object (excluding GcHeader).
/// Returns a Vec of user pointers (past the header), one per entry.
pub fn gc_malloc_batch(sizes: &[usize], obj_type: u8) -> Vec<*mut u8> {
    gc_check_trigger(); // once, not N times

    let n = sizes.len();
    let mut results = Vec::with_capacity(n);
    let mut headers = Vec::with_capacity(n);
    let mut allocated_bytes: u64 = 0;

    unsafe {
        GC_FLAGS.with(|f| f.set(f.get() | GC_FLAG_IN_ALLOC));

        for &size in sizes {
            let total = GC_HEADER_SIZE + size;
            let layout = Layout::from_size_align(total, 8).unwrap();
            let raw = alloc(layout);
            if raw.is_null() {
                // Inside the IN_ALLOC window the emergency reclaim refuses
                // to run (re-entrancy); batch callers are rare and small,
                // so just report exhaustion.
                panic!(
                    "gc_malloc_batch: failed to allocate {} bytes (heap exhausted)",
                    total
                );
            }
            let header = raw as *mut GcHeader;
            (*header).obj_type = obj_type;
            (*header).gc_flags = super::barrier::gc_birth_extra_flags();
            super::barrier::gc_note_black_birth(header);
            (*header)._reserved = 0;
            (*header).size = total as u32;

            allocated_bytes = allocated_bytes.saturating_add(total as u64);
            headers.push(header);
            results.push(raw.add(GC_HEADER_SIZE));
        }

        MALLOC_STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.objects.extend_from_slice(&headers);
            s.record_malloc_alloc(obj_type, headers.len() as u64, allocated_bytes);
            if s.malloc_registry_available() {
                s.set.extend(headers.iter().map(|&h| h as usize));
            }
            s.maybe_reserve_heavy_capacity();
        });

        GC_FLAGS.with(|f| f.set(f.get() & !GC_FLAG_IN_ALLOC));
    }

    results
}

impl MallocState {
    #[inline]
    pub(super) fn malloc_registry_available(&self) -> bool {
        self.registry_state == MallocRegistryState::ActiveConsistent
    }

    /// One-time jump from the small per-thread start to the heavy
    /// pre-size once this thread proves malloc-heavy. Called after the
    /// tracked-object push on the allocation paths; `>=` (rather than
    /// an exact-crossing check) keeps `gc_malloc_batch`'s multi-entry
    /// extends from skipping over the threshold.
    #[inline]
    pub(super) fn maybe_reserve_heavy_capacity(&mut self) {
        if self.heavy_capacity_reserved || self.objects.len() < MALLOC_STATE_HEAVY_LEN_THRESHOLD {
            return;
        }
        self.heavy_capacity_reserved = true;
        self.objects
            .reserve(MALLOC_STATE_HEAVY_CAPACITY.saturating_sub(self.objects.len()));
        self.set
            .reserve(MALLOC_STATE_HEAVY_CAPACITY.saturating_sub(self.set.len()));
    }

    #[inline]
    pub(super) fn record_malloc_alloc(&mut self, obj_type: u8, count: u64, bytes: u64) {
        let counters = &mut self.kind_telemetry[malloc_kind_index(obj_type)];
        counters.allocated_count = counters.allocated_count.saturating_add(count);
        counters.allocated_bytes = counters.allocated_bytes.saturating_add(bytes);
        counters.survivor_count = counters.survivor_count.saturating_add(count);
        counters.survivor_bytes = counters.survivor_bytes.saturating_add(bytes);
    }

    #[inline]
    pub(super) fn record_malloc_realloc(&mut self, obj_type: u8, old_bytes: u64, new_bytes: u64) {
        let counters = &mut self.kind_telemetry[malloc_kind_index(obj_type)];
        counters.realloc_count = counters.realloc_count.saturating_add(1);
        counters.realloc_old_bytes = counters.realloc_old_bytes.saturating_add(old_bytes);
        counters.realloc_new_bytes = counters.realloc_new_bytes.saturating_add(new_bytes);
        if new_bytes >= old_bytes {
            counters.survivor_bytes = counters
                .survivor_bytes
                .saturating_add(new_bytes.saturating_sub(old_bytes));
        } else {
            counters.survivor_bytes = counters
                .survivor_bytes
                .saturating_sub(old_bytes.saturating_sub(new_bytes));
        }
    }

    #[inline]
    pub(super) fn record_malloc_free(&mut self, obj_type: u8, bytes: u64) {
        let counters = &mut self.kind_telemetry[malloc_kind_index(obj_type)];
        counters.freed_count = counters.freed_count.saturating_add(1);
        counters.freed_bytes = counters.freed_bytes.saturating_add(bytes);
        counters.survivor_count = counters.survivor_count.saturating_sub(1);
        counters.survivor_bytes = counters.survivor_bytes.saturating_sub(bytes);
    }

    #[inline]
    pub(super) fn record_copied_minor_validation_lookup(&mut self, obj_type: Option<u8>) {
        let index = obj_type
            .map(malloc_kind_index)
            .unwrap_or(MALLOC_KIND_UNKNOWN_INDEX);
        let counters = &mut self.kind_telemetry[index];
        counters.copied_minor_validation_lookups =
            counters.copied_minor_validation_lookups.saturating_add(1);
    }

    #[cfg(feature = "diagnostics")]
    pub(super) fn take_kind_telemetry(
        &mut self,
    ) -> [MallocKindTelemetry; MALLOC_KIND_BUCKET_COUNT] {
        let snapshot = self.kind_telemetry;
        for counters in &mut self.kind_telemetry {
            counters.reset_cycle_deltas();
        }
        snapshot
    }

    #[inline]
    pub(super) fn record_realloc_forwarding(
        &mut self,
        old_header: *mut GcHeader,
        new_header: *mut GcHeader,
    ) {
        if old_header == new_header {
            return;
        }
        let old_addr = old_header as usize;
        let new_addr = new_header as usize;
        let mut updated_existing_snapshot = false;
        for current in self.realloc_forwarding.values_mut() {
            if *current == old_addr {
                *current = new_addr;
                updated_existing_snapshot = true;
            }
        }
        if !updated_existing_snapshot
            && self.realloc_snapshot_headers.contains(&old_addr)
            && !self.realloc_forwarding.contains_key(&old_addr)
        {
            self.realloc_forwarding.insert(old_addr, new_addr);
        }
        self.realloc_forwarding
            .retain(|&snapshot_header, current_header| snapshot_header != *current_header);
    }

    pub(super) fn resolve_realloc_forwarding(&self, header: *mut GcHeader) -> *mut GcHeader {
        let start = header as usize;
        self.realloc_forwarding
            .get(&start)
            .copied()
            .unwrap_or(start) as *mut GcHeader
    }
}

pub(super) fn malloc_sweep_snapshot_headers() -> Vec<*mut GcHeader> {
    MALLOC_STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.realloc_forwarding.clear();
        let headers = s.objects.clone();
        s.realloc_snapshot_headers.clear();
        s.realloc_snapshot_headers
            .extend(headers.iter().map(|&header| header as usize));
        headers
    })
}

pub(super) fn malloc_sweep_clear_snapshot_tracking() {
    MALLOC_STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.realloc_forwarding.clear();
        s.realloc_snapshot_headers.clear();
    });
}

pub(super) fn malloc_sweep_revalidate_header(
    snapshot_header: *mut GcHeader,
    expected_idx: usize,
) -> Option<(*mut GcHeader, usize)> {
    MALLOC_STATE.with(|s| {
        let s = s.borrow();
        let current_header = s.resolve_realloc_forwarding(snapshot_header);
        if expected_idx < s.objects.len() && s.objects[expected_idx] == current_header {
            return Some((current_header, expected_idx));
        }
        s.objects
            .iter()
            .position(|&candidate| candidate == current_header)
            .map(|idx| (current_header, idx))
    })
}

thread_local! {
    pub(super) static MALLOC_REGISTRY_REBUILD_COUNT: Cell<u64> = const { Cell::new(0) };
}

/// Lazily activate `MallocState.set` from `MallocState.objects`.
///
/// Once activated, the registry stays exact: `gc_malloc`,
/// `gc_malloc_batch`, `gc_realloc`, and `sweep_malloc_objects` update it
/// inline. This preserves the malloc hot path for workloads that never need
/// exact validation, while keeping copied-minor from rebuilding the registry
/// during nursery collection.
#[inline]
pub(super) fn ensure_set_built(s: &mut MallocState) {
    if s.malloc_registry_available() {
        return;
    }
    s.set.clear();
    s.set.extend(s.objects.iter().map(|&h| h as usize));
    s.registry_state = MallocRegistryState::ActiveConsistent;
    MALLOC_REGISTRY_REBUILD_COUNT.with(|c| c.set(c.get().saturating_add(1)));
}

/// True when `header` is an exact malloc-tracked GC allocation header.
///
/// This is for validation paths that need to reject forged pointer-tagged JS
/// values before reading a candidate header.
pub(crate) fn gc_malloc_header_is_tracked(header: *const GcHeader) -> bool {
    if header.is_null() {
        return false;
    }
    MALLOC_STATE.with(|s| {
        let mut s = s.borrow_mut();
        ensure_set_built(&mut s);
        s.set.contains(&(header as usize))
    })
}

/// Reallocate a malloc-tracked object, preserving GcHeader.
/// `old_user_ptr` is the pointer previously returned by gc_malloc.
/// Returns new user pointer (after header).
///
/// Safety: validates the pointer is actually tracked before dereferencing.
/// If the pointer was freed by GC or is arena-allocated, falls back to
/// fresh allocation to prevent SIGABRT from invalid realloc.
pub fn gc_realloc(old_user_ptr: *mut u8, new_payload_size: usize) -> *mut u8 {
    if old_user_ptr.is_null() {
        // Graceful fallback: allocate fresh instead of panicking
        return gc_malloc(new_payload_size, GC_TYPE_STRING);
    }

    let old_header = unsafe { old_user_ptr.sub(GC_HEADER_SIZE) as *mut GcHeader };

    // Validate the pointer is in our tracked set before dereferencing the header.
    // This prevents SIGABRT when gc_realloc is called on a pointer that was
    // freed by GC (use-after-free) or was never allocated by gc_malloc.
    // Set is built lazily on first realloc — most allocation-heavy
    // workloads never enter this branch so the build cost is amortized
    // away from `gc_malloc`'s hot path.
    let is_tracked = MALLOC_STATE.with(|s| {
        let mut s = s.borrow_mut();
        ensure_set_built(&mut s);
        s.set.contains(&(old_header as usize))
    });

    if !is_tracked {
        // Pointer is not tracked — it was freed by GC, is arena-allocated,
        // or was never allocated by gc_malloc. Allocate fresh.
        eprintln!(
            "[perry] gc_realloc: untracked pointer {:p}, allocating fresh ({} bytes)",
            old_user_ptr, new_payload_size
        );
        return gc_malloc(new_payload_size, GC_TYPE_STRING);
    }

    // Also check arena flag — arena objects must not be passed to system realloc
    unsafe {
        if (*old_header).gc_flags & GC_FLAG_ARENA != 0 {
            eprintln!(
                "[perry] gc_realloc: arena pointer {:p}, allocating fresh",
                old_user_ptr
            );
            let new_ptr = gc_malloc(new_payload_size, (*old_header).obj_type);
            let old_payload_size = (*old_header).size as usize - GC_HEADER_SIZE;
            let copy_size = old_payload_size.min(new_payload_size);
            std::ptr::copy_nonoverlapping(old_user_ptr, new_ptr, copy_size);
            return new_ptr;
        }
    }

    let old_total = unsafe { (*old_header).size as usize };
    let obj_type = unsafe { (*old_header).obj_type };
    let new_total = GC_HEADER_SIZE + new_payload_size;

    let old_layout = Layout::from_size_align(old_total, 8).unwrap();

    unsafe {
        let new_raw = realloc(old_header as *mut u8, old_layout, new_total);
        if new_raw.is_null() {
            panic!("gc_realloc: failed to reallocate to {} bytes", new_total);
        }

        let new_header = new_raw as *mut GcHeader;
        (*new_header).size = new_total as u32;

        let prev_in_alloc = GC_FLAGS.with(|f| {
            let prev = f.get();
            f.set(prev | GC_FLAG_IN_ALLOC);
            prev & GC_FLAG_IN_ALLOC
        });
        MALLOC_STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.record_malloc_realloc(obj_type, old_total as u64, new_total as u64);
            // Update pointer in MALLOC_STATE (objects + set) if it changed.
            if new_header != old_header {
                for ptr in s.objects.iter_mut() {
                    if *ptr == old_header {
                        *ptr = new_header;
                        break;
                    }
                }
                // Keep the lazy-built set in sync. We already built it
                // above for the `is_tracked` check, so it's currently
                // consistent with `objects` — patch in place.
                s.set.remove(&(old_header as usize));
                s.set.insert(new_header as usize);
                s.record_realloc_forwarding(old_header, new_header);
            }
        });
        GC_FLAGS.with(|f| {
            let cur = f.get();
            if prev_in_alloc != 0 {
                f.set(cur | GC_FLAG_IN_ALLOC);
            } else {
                f.set(cur & !GC_FLAG_IN_ALLOC);
            }
        });

        new_raw.add(GC_HEADER_SIZE)
    }
}

#[cfg(test)]
impl MallocState {
    /// Build an empty, TLS-independent `MallocState` for unit tests.
    fn new_empty_for_test() -> Self {
        MallocState {
            objects: Vec::new(),
            set: crate::fast_hash::PtrHashSet::with_capacity_and_hasher(
                0,
                crate::fast_hash::PtrHasher,
            ),
            realloc_forwarding: crate::fast_hash::new_ptr_hash_map(),
            realloc_snapshot_headers: crate::fast_hash::new_ptr_hash_set(),
            registry_state: MallocRegistryState::Inactive,
            heavy_capacity_reserved: false,
            kind_telemetry: [MallocKindTelemetry::zero(); MALLOC_KIND_BUCKET_COUNT],
        }
    }

    /// Allocate a raw GcHeader-prefixed block (as `gc_malloc` would) and push
    /// it into this state's tracking, WITHOUT touching the thread-local
    /// `MALLOC_STATE`. Returns the header so a test can assert on / manually
    /// reclaim it. `flags` seeds `gc_flags` (e.g. `GC_FLAG_PINNED`).
    unsafe fn push_test_object(&mut self, payload: usize, flags: u8) -> *mut GcHeader {
        let total = GC_HEADER_SIZE + payload;
        let layout = Layout::from_size_align(total, 8).unwrap();
        let raw = alloc(layout);
        assert!(!raw.is_null(), "test allocation failed");
        let header = raw as *mut GcHeader;
        (*header).obj_type = GC_TYPE_STRING;
        (*header).gc_flags = flags;
        (*header)._reserved = 0;
        (*header).size = total as u32;
        self.objects.push(header);
        self.set.insert(header as usize);
        header
    }
}

#[cfg(test)]
mod drop_tests {
    use super::*;

    /// `free_all_tracked_objects` (the body of `Drop for MallocState`) reclaims
    /// every tracked block and reports the exact byte total — the worker-exit
    /// leak fix (#6185 / GC audit §6). TLS-Drop itself can't be exercised
    /// directly from a unit test (it fires only at real thread teardown), so we
    /// drive the shared free path on a TLS-independent `MallocState`.
    #[test]
    fn free_all_tracked_objects_reclaims_every_block() {
        let mut state = MallocState::new_empty_for_test();
        let payloads = [8usize, 24, 100, 4096];
        let expected: u64 = payloads.iter().map(|&p| (GC_HEADER_SIZE + p) as u64).sum();

        unsafe {
            for &p in &payloads {
                state.push_test_object(p, 0);
            }
        }
        assert_eq!(state.objects.len(), payloads.len());

        let freed = state.free_all_tracked_objects();
        assert_eq!(
            freed, expected,
            "must free the exact total size of every tracked block"
        );
        assert!(
            state.objects.is_empty(),
            "tracked-object list must be drained after free"
        );

        // Idempotent: the subsequent real Drop must be a no-op (no double free).
        let freed_again = state.free_all_tracked_objects();
        assert_eq!(freed_again, 0);
    }

    /// Pinned objects are skipped (mirrors `process_sweep_header`) so a
    /// cross-thread promise pinned for an in-flight result is never yanked.
    #[test]
    fn free_all_tracked_objects_skips_pinned() {
        let mut state = MallocState::new_empty_for_test();
        let (pinned, unpinned_size);
        unsafe {
            pinned = state.push_test_object(64, GC_FLAG_PINNED);
            state.push_test_object(32, 0);
        }
        unpinned_size = (GC_HEADER_SIZE + 32) as u64;

        let freed = state.free_all_tracked_objects();
        assert_eq!(
            freed, unpinned_size,
            "only the unpinned block's bytes are reclaimed"
        );

        // The pinned block was skipped, not freed — reclaim it manually so the
        // test itself doesn't leak.
        unsafe {
            let total = (*pinned).size as usize;
            let layout = Layout::from_size_align(total, 8).unwrap();
            dealloc(pinned as *mut u8, layout);
        }
    }
}
