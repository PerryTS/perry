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
    /// Registry availability/consistency. Copied-minor GC may consult an
    /// already-active exact registry, but must never rebuild it on the fast
    /// path because that would scale with total malloc churn.
    pub(super) registry_state: MallocRegistryState,
    pub(super) kind_telemetry: [MallocKindTelemetry; MALLOC_KIND_BUCKET_COUNT],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MallocRegistryState {
    Inactive,
    ActiveConsistent,
}

/// Pre-allocated capacity for `MallocState.objects` and `.set`.
///
/// On promise-heavy kernels (`promise_all_chains` allocates ~200 k
/// strings/closures/promises before the first GC) the set grows from
/// 0 → 128 → … → 256 k buckets across the allocation history. Each
/// hashbrown doubling re-inserts every existing key, and at the
/// ~100 k mark those rehashes were the single hottest leaf in the
/// profile (15.6 % self-time on `gc_malloc`'s caller chain). Starting
/// at 256 k buckets covers the kernel's full pre-GC working set
/// (200 k entries at hashbrown's 7/8 load factor) in one allocation —
/// subsequent kernel iterations re-use the slots that sweep re-emptied,
/// so we never pay the rehash tax. Cost: one upfront ~4 MB allocation
/// per thread (vs ~2 MB at 128 k); pays for itself on the first 100
/// allocations.
pub(super) const MALLOC_STATE_INITIAL_CAPACITY: usize = 256 * 1024;

thread_local! {
    pub(crate) static MALLOC_STATE: RefCell<MallocState> = RefCell::new(MallocState {
        objects: Vec::with_capacity(MALLOC_STATE_INITIAL_CAPACITY),
        set: crate::fast_hash::PtrHashSet::with_capacity_and_hasher(
            MALLOC_STATE_INITIAL_CAPACITY,
            crate::fast_hash::PtrHasher,
        ),
        registry_state: MallocRegistryState::Inactive,
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
        let raw = alloc(layout);
        if raw.is_null() {
            panic!("gc_malloc: failed to allocate {} bytes", total);
        }

        let header = raw as *mut GcHeader;
        (*header).obj_type = obj_type;
        (*header).gc_flags = 0; // not arena
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
                panic!("gc_malloc_batch: failed to allocate {} bytes", total);
            }
            let header = raw as *mut GcHeader;
            (*header).obj_type = obj_type;
            (*header).gc_flags = 0;
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

    pub(super) fn take_kind_telemetry(
        &mut self,
    ) -> [MallocKindTelemetry; MALLOC_KIND_BUCKET_COUNT] {
        let snapshot = self.kind_telemetry;
        for counters in &mut self.kind_telemetry {
            counters.reset_cycle_deltas();
        }
        snapshot
    }
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
