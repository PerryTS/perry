//! Mark-sweep garbage collector for Perry
//!
//! Design:
//! - 8-byte GcHeader prepended to every heap allocation (invisible to callers)
//! - Arena objects (arrays/objects): discovered by walking arena blocks linearly (zero per-alloc tracking cost)
//! - Malloc objects (strings/closures/promises/bigints/errors): tracked in MALLOC_STATE
//! - Mark phase: conservative stack scan + explicit thread-local root scanning + type-specific tracing
//! - Sweep phase: free malloc objects; arena objects added to free list for reuse
//! - Trigger: only checked on new arena block allocation or explicit gc() call

use std::alloc::{alloc, dealloc, realloc, Layout};
use std::cell::{Cell, RefCell};

/// GC header prepended to every heap allocation.
/// Callers receive a pointer AFTER this header (ptr + 8).
#[repr(C)]
pub struct GcHeader {
    /// GC_TYPE_ARRAY, GC_TYPE_STRING, etc.
    pub obj_type: u8,
    /// GC_FLAG_MARKED | GC_FLAG_ARENA | GC_FLAG_PINNED
    pub gc_flags: u8,
    /// Reserved for future use
    pub _reserved: u16,
    /// Total allocation size (header + payload) for arena block walking
    pub size: u32,
}

pub const GC_HEADER_SIZE: usize = std::mem::size_of::<GcHeader>(); // 8 bytes

// Object type constants
pub const GC_TYPE_ARRAY: u8 = 1;
pub const GC_TYPE_OBJECT: u8 = 2;
pub const GC_TYPE_STRING: u8 = 3;
pub const GC_TYPE_CLOSURE: u8 = 4;
pub const GC_TYPE_PROMISE: u8 = 5;
pub const GC_TYPE_BIGINT: u8 = 6;
pub const GC_TYPE_ERROR: u8 = 7;
pub const GC_TYPE_MAP: u8 = 8;
/// Issue #179 Step 2 Phase 2: lazy JSON-parse top-level array.
/// Arena-allocated, same fast-alloc path as regular arrays.
/// `js_array_length` and `js_json_stringify` recognize this type and
/// serve reads / stringify directly from the tape + blob bytes
/// without materializing the tree. Any other accessor
/// force-materializes (mutates the header's `materialized` field so
/// subsequent accesses hit the tree).
pub const GC_TYPE_LAZY_ARRAY: u8 = 9;

// Flag constants
pub const GC_FLAG_MARKED: u8 = 0x01;
pub const GC_FLAG_ARENA: u8 = 0x02;
pub const GC_FLAG_PINNED: u8 = 0x04;
/// Set on a keys-array that was handed out by `shape_cache_insert`.
/// `js_object_set_field_by_name` reads this bit to decide whether it
/// must clone before mutating (shared arrays can't be mutated in
/// place; fresh arrays allocated in the `keys.is_null()` branch can).
/// Without the bit the clone fires on every property added to every
/// fresh object literal — a 20-property row object allocates 19
/// throwaway keys_array clones per row.
pub const GC_FLAG_SHAPE_SHARED: u8 = 0x08;
/// Set on strings that live in the intern table. Prevents in-place
/// mutation and allows `js_object_set_field_by_name` to skip the
/// FNV-1a hash (pointer identity is sufficient for interned strings).
pub const GC_FLAG_INTERNED: u8 = 0x10;
/// Gen-GC Phase C4: object has survived at least PROMOTION_AGE
/// minor GCs and is now logically tenured — minor GC trace skips
/// recursion into its fields, exactly like an OLD_ARENA-allocated
/// object. Stored on the GcHeader so the per-object check is one
/// byte load + one bit-and. Non-moving generational: tenured
/// objects stay physically in nursery (no copying / forwarding-
/// pointer machinery), but the trace pretends they're old-gen.
/// True compacting evacuation lands in Phase C4b.
pub const GC_FLAG_TENURED: u8 = 0x20;
/// Gen-GC Phase C4: object has survived exactly one minor GC.
/// Set during the post-trace age-bump pass; on the next minor GC,
/// the age-bump pass observes this flag and promotes the object
/// to TENURED. Two-bit aging (HAS_SURVIVED → TENURED) gives
/// PROMOTION_AGE=2 without needing a counter field.
pub const GC_FLAG_HAS_SURVIVED: u8 = 0x40;
/// Gen-GC Phase C4b: object has been evacuated (copied) to a new
/// location. The new address is stored in the **user-payload's
/// first 8 bytes** (immediately after the GcHeader). Walkers that
/// encounter a FORWARDED header read the forwarding address and
/// follow it; ref-rewrite passes update every NaN-boxed pointer
/// they observe to the forwarded address. Conservative-stack
/// scans STILL get the old (now-stale) address; objects that
/// might be conservatively referenced are pinned out of the
/// evacuation set via `GC_FLAG_PINNED` to avoid corrupting reads
/// from those words.
///
/// This is the last bit in the u8 gc_flags. Adding more flags
/// requires extending GcHeader (currently 8 bytes total — extending
/// breaks ABI everywhere; deferred until/unless a future phase
/// genuinely needs more bits).
pub const GC_FLAG_FORWARDED: u8 = 0x80;

/// Read the forwarding address embedded in an evacuated object's
/// user payload. Caller must verify `gc_flags & GC_FLAG_FORWARDED`
/// is set; reading otherwise returns garbage. The forwarded
/// address is the **user pointer** of the new location — i.e.
/// what `arena_alloc_gc_old` returned for the new copy. Callers
/// that need the new GcHeader subtract `GC_HEADER_SIZE` themselves.
///
/// # Safety
/// `header` must point to a valid GcHeader whose user payload is
/// at least 8 bytes (every Perry object's payload is — strings
/// have at least the StringHeader, arrays have ArrayHeader, etc.).
#[inline]
pub unsafe fn forwarding_address(header: *const GcHeader) -> *mut u8 {
    debug_assert!(
        (*header).gc_flags & GC_FLAG_FORWARDED != 0,
        "forwarding_address called on non-forwarded header"
    );
    let user_ptr = (header as *const u8).add(GC_HEADER_SIZE) as *const *mut u8;
    *user_ptr
}

/// Install a forwarding address in an evacuated object's user
/// payload and set `GC_FLAG_FORWARDED` on its header. The first 8
/// bytes of the user payload become the forwarding pointer (the
/// new user address — what `arena_alloc_gc_old` returned).
/// Subsequent reads via `forwarding_address` recover the new
/// location.
///
/// # Safety
/// As `forwarding_address`. The user payload must be at least 8
/// bytes; this is true for every Perry GC type today.
#[inline]
pub unsafe fn set_forwarding_address(header: *mut GcHeader, new_user_addr: *mut u8) {
    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE) as *mut *mut u8;
    *user_ptr = new_user_addr;
    (*header).gc_flags |= GC_FLAG_FORWARDED;
}

// Object flags stored in GcHeader._reserved (u16) for Object.freeze/seal/preventExtensions
pub const OBJ_FLAG_FROZEN: u16 = 0x01;
pub const OBJ_FLAG_SEALED: u16 = 0x02;
pub const OBJ_FLAG_NO_EXTEND: u16 = 0x04;

// NaN-boxing tag constants (duplicated from value.rs to avoid circular deps)
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const BIGINT_TAG: u64 = 0x7FFA_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;

/// GC statistics
pub struct GcStats {
    pub collection_count: u64,
    pub total_freed_bytes: u64,
    pub last_pause_us: u64,
}

/// Issue #62: consolidated malloc-tracking state. Before this, the hot path of
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
    /// Malloc-allocated objects tracked for GC (strings/closures/bigints/…)
    pub(crate) objects: Vec<*mut GcHeader>,
    /// O(1) lookup set for validating malloc pointers — lazily built
    /// from `objects` on first `gc_realloc` call. The vast majority of
    /// `gc_malloc`-heavy workloads (anything that doesn't call
    /// `gc_realloc` directly — promise/closure/Map/etc. allocation
    /// kernels) never touch this. Building it eagerly on every
    /// allocation was the single hottest leaf (~16 % self-time on
    /// `promise_all_chains`: 200 k `set.insert` calls × hashbrown's
    /// group-probe cost). Now `gc_malloc` only pushes to `objects`
    /// (Vec push amortized O(1)), and `gc_realloc` calls
    /// `ensure_set_built` to populate the set on demand.
    /// `set_dirty` is set when an allocation/sweep happens since the
    /// set was built; cleared on rebuild. Empty initially.
    pub(crate) set: crate::fast_hash::PtrHashSet<usize>,
    /// True while `set` does not reflect `objects`. Allocations and
    /// sweep both set this; `ensure_set_built` consumes it.
    pub(crate) set_dirty: bool,
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
const MALLOC_STATE_INITIAL_CAPACITY: usize = 256 * 1024;

thread_local! {
    pub(crate) static MALLOC_STATE: RefCell<MallocState> = RefCell::new(MallocState {
        objects: Vec::with_capacity(MALLOC_STATE_INITIAL_CAPACITY),
        // Set is empty initially. Populated lazily on first
        // `gc_realloc` call via `ensure_set_built`. Pre-allocate the
        // bucket array up front so the lazy build (when it does
        // happen) doesn't have to rehash. Memory cost: ~4 MB upfront
        // (paid only on workloads that allocate enough to need it
        // — Vec/HashSet won't actually fault the pages until written).
        set: crate::fast_hash::PtrHashSet::with_capacity_and_hasher(
            MALLOC_STATE_INITIAL_CAPACITY,
            crate::fast_hash::PtrHasher,
        ),
        // No allocations yet — set is consistent with empty objects.
        set_dirty: false,
    });

    /// Free list of arena slots available for reuse: (user_ptr, payload_size)
    pub(crate) static ARENA_FREE_LIST: RefCell<Vec<(*mut u8, usize)>> = const { RefCell::new(Vec::new()) };

    /// Fast empty-check for `ARENA_FREE_LIST` — kept in sync with the Vec
    /// length. The hot allocation path checks this `Cell` (a single load,
    /// no `RefCell::borrow_mut` cost) and skips the free-list lookup
    /// entirely when it's empty. Maintained by the GC sweep (sets) and
    /// `arena_alloc_gc` (clears when the Vec drains).
    pub(crate) static ARENA_FREE_LIST_NONEMPTY: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };

    /// GC statistics
    static GC_STATS: RefCell<GcStats> = const { RefCell::new(GcStats {
        collection_count: 0,
        total_freed_bytes: 0,
        last_pause_us: 0,
    }) };

    /// Registered root scanner functions (promise queue, timers, etc.)
    static ROOT_SCANNERS: RefCell<Vec<fn(&mut dyn FnMut(f64))>> = RefCell::new(Vec::new());

    /// Module-level global variable addresses (registered by codegen)
    static GLOBAL_ROOTS: RefCell<Vec<*mut u64>> = const { RefCell::new(Vec::new()) };

    /// Bit 0: reentrancy guard (`GC_FLAG_IN_ALLOC`) — set while gc_malloc /
    /// gc_realloc is mutating MALLOC_STATE. Prevents gc_check_trigger() from
    /// running a collection mid-tracking, which would cause RefCell
    /// double-borrow panics (SIGABRT).
    ///
    /// Bit 1: suppression (`GC_FLAG_SUPPRESSED`) — when set, gc_check_trigger()
    /// skips collection entirely. Used by JSON.parse to avoid mid-parse GC
    /// cycles (parse is synchronous and roots intermediate values in
    /// PARSE_ROOTS, so deferring GC until after parse completes is safe and
    /// eliminates O(n*m) GC overhead).
    ///
    /// Issue #62: merged into a single Cell<u8> so the fast path of
    /// `gc_check_trigger` reads both flags with one TLS access + one load.
    static GC_FLAGS: Cell<u8> = const { Cell::new(0) };
}

/// Bit 0 of GC_FLAGS — in_alloc reentrancy guard.
const GC_FLAG_IN_ALLOC: u8 = 0b01;
/// Bit 1 of GC_FLAGS — suppression flag (JSON.parse).
const GC_FLAG_SUPPRESSED: u8 = 0b10;

/// Threshold: run GC when total arena bytes exceed this.
///
/// Issue #179 tier 1 follow-up: lowered from 128 MB to 64 MB. The
/// 128 MB value was tuned so `object_create`'s 96 MB working set would
/// fit under the threshold and pay zero GC cost. That tuning
/// assumption was wrong for any workload with sustained allocation
/// pressure: `bench_json_roundtrip` at 5 MB/iter takes 25 iters to
/// hit 128 MB, and post-v0.5.193's adaptive step can't recover from
/// the single-GC regime because high-productivity collections
/// (>90% freed) double the step back to 256 MB and the bench
/// completes before a second GC. 64 MB fires the first GC at iter
/// ~12 which is early enough to catch the workload's natural rhythm
/// without paying for excess collections.
///
/// Tuning sweep on `bench_json_roundtrip` (Node baseline: 372 ms /
/// 191 MB):
///
/// | Initial | Time | RSS |
/// |---|---:|---:|
/// | 128 MB | 322 ms | 199 MB (+4% vs Node) |
/// | 96 MB  | 353 ms | 178 MB (−7%  vs Node) |
/// | **64 MB** | **364 ms** | **144 MB (−25% vs Node)** |
/// | 48 MB  | 378 ms | 130 MB (−32% vs Node) |
///
/// 64 MB is the sweet spot that wins on both axes vs Node by a
/// comfortable margin. `object_create` / `binary_trees` unaffected —
/// their working sets fit in one 1 MB arena block each, well under
/// the threshold, 0-1 ms as before.
const GC_THRESHOLD_INITIAL_BYTES: usize = 64 * 1024 * 1024; // 64 MB
/// Sanity bound on the adaptive step itself. Step growth past 1 GB is
/// only theoretically possible on multi-day services where GC fires
/// rarely; we keep the cap loose here since the *real* peak-RSS
/// guardrail is `GC_TRIGGER_ABSOLUTE_CEILING` below.
const GC_THRESHOLD_MAX_BYTES: usize = 1024 * 1024 * 1024; // 1 GB

/// Hard ceiling on the next-GC trigger (arena_total bytes), independent
/// of how productive recent sweeps have been. Without this, the
/// >90%-freed branch doubles the step on every productive collection,
/// > and `next_trigger = new_total + step` lets peak nursery occupancy
/// > grow unboundedly even when most of what we collected was garbage.
/// > On `bench_json_roundtrip` direct (50 iters × ~5 MB / iter, GC fires
/// > 3 times), the step doubled from 64 MB → 67 MB → 134 MB and the
/// > trigger followed it, so peak nursery hit 115 MB at GC #3 — the
/// > dealloc pass from C4b-δ then returned 91 MB to the OS, but the
/// > peak-RSS damage was already done. Capping the trigger at the
/// > initial threshold prevents that runaway: after GC, trigger ≤ 128 MB
/// > regardless of how much step adapted, so peak nursery stays bounded
/// > to roughly initial + one iter's allocation buffer + headroom for
/// > non-arena overhead.
///
/// Floor: even if `arena_total` is already near or past the ceiling
/// (large old-gen + longlived combined live set), keep at least the
/// 16 MB step floor as headroom — `next_trigger = max(new_total + 16 MB,
/// min(new_total + step, ceiling))`. This avoids GC thrash when the
/// non-nursery component of arena_total alone exceeds the ceiling.
///
/// 2026-05-02 raise from 64 MB → 128 MB: ECS perf-comprehensive's
/// allocation-heavy benches (10k two-comp + sync, 5k × 3 cmds) hit
/// the 64 MB cap mid-round, then the >25%-freed branch halved the
/// step to 16 MB, so the next trigger landed ~16 MB above the post-
/// GC working set — well within a single round's allocation budget.
/// Result: 1-2 mid-round GCs per bench, the worst of which spent
/// 60 ms inside `mark_block_persisting_arena_objects` force-marking
/// + tracing 40 k newly-allocated objects in the recent window.
/// Doubling the cap lets productive sweeps accumulate full
/// `step` headroom (up to 128 MB) before the next trigger, which
/// shifts those GC events out of the measured rounds entirely.
/// `bench_json_roundtrip`-class workloads still bounded — they
/// finish under 128 MB peak and fire ≤2 GCs total.
///
/// Workloads unaffected: `07_object_create` / `12_binary_trees` /
/// `bench_gc_pressure` all fit their working sets under 64 MB and
/// fire GC at most once. The cap only changes behavior when the step
/// would otherwise have pushed the trigger past the initial threshold,
/// which is exactly the bench-RSS scenario this is targeting.
const GC_TRIGGER_ABSOLUTE_CEILING: usize = 128 * 1024 * 1024;

thread_local! {
    /// Lower bound for the next GC trigger. Bumped after each
    /// `gc_collect_inner` based on collection effectiveness (see the
    /// adaptive logic in `gc_check_trigger`).
    ///
    /// The initial value is `GC_THRESHOLD_INITIAL_BYTES` (128MB —
    /// chosen so that the 96MB working set of a 1M-iter object_create
    /// or binary_trees benchmark fits under the threshold and pays
    /// zero GC cost). After every collection, if the sweep freed >75%
    /// of arena bytes, the per-program "step" is doubled (capped at
    /// 1GB) so subsequent allocation bursts don't pay GC overhead just
    /// because they re-cross the same line. For hot `new ClassName()`
    /// loops where every object dies between GC cycles, this means
    /// the FIRST burst pays for at most one collection and the rest
    /// run GC-free.
    ///
    /// If a sweep frees <25%, the step is halved (down to a 16MB
    /// floor) so live-set-bound programs don't grow their working
    /// set unboundedly between collections.
    static GC_NEXT_TRIGGER_BYTES: std::cell::Cell<usize> =
        const { std::cell::Cell::new(GC_THRESHOLD_INITIAL_BYTES) };

    /// Per-program adaptive GC step. Doubles (up to MAX) when sweeps
    /// are mostly-garbage; halves (down to 16MB) when sweeps reclaim
    /// little. Used to compute the next trigger after each GC as
    /// `post_total + step`.
    static GC_STEP_BYTES: std::cell::Cell<usize> =
        const { std::cell::Cell::new(GC_THRESHOLD_INITIAL_BYTES) };

    /// Lower bound for the next malloc-count-based GC trigger. After each
    /// collection, this is reset to `survivor_count + GC_MALLOC_COUNT_STEP`
    /// so that programs with large legitimate live sets (>10k tracked
    /// malloc objects) don't GC-thrash on every subsequent allocation.
    /// See `gc_check_trigger` for the update rule.
    static GC_NEXT_MALLOC_TRIGGER: std::cell::Cell<usize> =
        const { std::cell::Cell::new(100_000) };

    /// Issue #745: track whether a medium-or-larger parse already
    /// raised `GC_NEXT_TRIGGER_BYTES` this GC cycle. Cleared in
    /// `gc_collect_inner` whenever a real collection runs.
    static GC_TRIGGER_BUMPED: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };

    /// Issue #745: snapshot of `arena_total_bytes()` at the most
    /// recent `gc_suppress` call. Used by `gc_bump_malloc_trigger`
    /// to compute the suppressed window's arena growth.
    static GC_PRE_SUPPRESS_BYTES: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

/// Initial step for the malloc-count-based GC trigger. Adaptive: doubles
/// when >75% of malloc objects are garbage (loop-scoped temporaries),
/// halves when <25% are garbage (large live set). Capped at
/// `GC_MALLOC_COUNT_STEP_MAX` to bound memory between collections.
///
/// Originally a single hardcoded threshold (`GC_MALLOC_COUNT_THRESHOLD`);
/// issue #34 showed that triggering GC from `gc_malloc` (needed for
/// malloc-heavy workloads that don't push arena blocks — e.g.
/// @perry/postgres's `parseBigIntDecimal` bigint chain) combined with a
/// hardcoded threshold would thrash for any program whose live set
/// exceeded the threshold. Making it a per-cycle step fixes that.
///
/// Issue #58: the constant 10k step caused ~100 GC cycles for 500k-iter
/// string-concat loops where almost every object is dead. Adaptive
/// doubling ramps the step to 160k+ after a few mostly-garbage sweeps,
/// cutting GC cycles from ~100 to ~10.
const GC_MALLOC_COUNT_STEP_INITIAL: usize = 100_000;
const GC_MALLOC_COUNT_STEP_MAX: usize = 2_000_000;
const GC_MALLOC_COUNT_STEP_MIN: usize = 10_000;

thread_local! {
    /// Per-program adaptive malloc-count step. Mirrors `GC_STEP_BYTES`
    /// behaviour: doubles when mostly-garbage, halves when mostly-live.
    static GC_MALLOC_COUNT_STEP: std::cell::Cell<usize> =
        const { std::cell::Cell::new(GC_MALLOC_COUNT_STEP_INITIAL) };
}

// ---------------------------------------------------------------------------
// Phase A — precise root tracking via shadow stack
// (docs/generational-gc-plan.md Phase A)
// ---------------------------------------------------------------------------
//
// Each compiled function gets a *shadow-stack frame* that holds the
// currently-live heap-pointer-typed locals. Codegen emits:
//   - push at function entry with a precomputed slot count
//   - slot stores at each safepoint (allocation + runtime-call sites)
//   - pop at every return path
//
// The shadow stack is built but not yet consumed by GC in this phase.
// Phase B+ will teach the GC tracer to walk it as a precise-root source
// in parallel with the existing conservative scanner.
//
// Layout: the shadow stack is a contiguous `Vec<u64>` (per-thread).
// Each frame is:
//   [u64 prev_frame_top, u64 slot_count, u64 slot_0, u64 slot_1, ...]
// `SHADOW_STACK_FRAME_TOP` points at the current frame's slot_0 so
// slot stores are a single indexed write. `prev_frame_top` is the
// saved top from before this frame was pushed — so pop is a single
// load + store.
//
// Slots hold NaN-boxed `JSValue` bits (u64) — same format codegen
// already uses for pointer-typed locals. The GC tracer in Phase B+
// will call `try_mark_value` on each non-zero slot, matching the
// closure-capture tracer's pattern.

pub const SHADOW_STACK_HEADER_SLOTS: usize = 2; // prev_frame_top + slot_count
pub const SHADOW_STACK_GROW_RESERVE: usize = 1024; // initial capacity (slots)

/// Combined shadow-stack state. Holding both fields in one TLS slot
/// halves the macOS `tlv_get_addr` calls in every shadow-stack op
/// (push / pop / slot_set / slot_get / scanner) — those ops fired
/// ~3 M+ times per perf-comprehensive run, and TLS access was the
/// single biggest leaf cost in the post-iter-3 profile (20.9 % leaf
/// samples on `tlv_get_addr`). Replacing `RefCell<Vec<u64>>` with
/// `UnsafeCell<ShadowStackState>` also drops the per-op RefCell
/// borrow accounting.
///
/// Safety: shadow-stack ops are only invoked from compiled JS code
/// (runtime-generated, single-threaded for this TLS) and from GC
/// scanner / rewriter passes. The two never overlap — GC is
/// stop-the-world relative to this TLS, and compiled code can't
/// re-enter the runtime through a path that would touch this state
/// while a GC walk is in progress (no allocation occurs inside the
/// scanner/rewriter, and `GC_FLAG_IN_ALLOC` blocks reentrant GC).
pub(crate) struct ShadowStackState {
    /// `Vec<u64>` instead of `Vec<*mut u8>` because slots hold
    /// NaN-boxed JSValue bits (upper 16 bits are the tag, lower 48
    /// the pointer) — the GC tracer unwraps the NaN-box the same way
    /// it already does for closure captures.
    pub(crate) stack: Vec<u64>,
    /// Index into `stack` where the current frame's slot_0 lives.
    /// `usize::MAX` when no frame is pushed (initial state + after
    /// the outermost function returns).
    pub(crate) frame_top: usize,
}

thread_local! {
    pub(crate) static SHADOW: std::cell::UnsafeCell<ShadowStackState> =
        std::cell::UnsafeCell::new(ShadowStackState {
            stack: Vec::with_capacity(SHADOW_STACK_GROW_RESERVE),
            frame_top: usize::MAX,
        });
}

/// Push a new shadow-stack frame with `slot_count` live-pointer
/// slots. Slots start zero-initialized (codegen fills them with
/// NaN-boxed pointer values via `js_shadow_slot_set`). Returns an
/// opaque `frame_handle` (the pre-push top index) that the matching
/// pop must be passed — lets the GC assert frame balance in debug
/// builds and detects codegen misemission.
///
/// Not marked `#[inline(always)]` because it's called once per
/// function entry; the 3-line body inlines naturally.
#[no_mangle]
pub extern "C" fn js_shadow_frame_push(slot_count: u32) -> u64 {
    SHADOW.with(|cell| unsafe {
        let s = &mut *cell.get();
        let prev_top = s.frame_top;
        let base = s.stack.len();
        // Header: prev_frame_top + slot_count. Slots follow,
        // initialized to 0 (GC_FLAG_NONE + null pointer).
        s.stack.push(prev_top as u64);
        s.stack.push(slot_count as u64);
        let slots_start = s.stack.len();
        s.stack.resize(slots_start + slot_count as usize, 0);
        s.frame_top = slots_start;
        base as u64
    })
}

/// Pop the current shadow-stack frame. `frame_handle` must match
/// the return value of the matching `js_shadow_frame_push` (debug
/// assertion). Restores the prior `SHADOW.frame_top`.
#[no_mangle]
pub extern "C" fn js_shadow_frame_pop(frame_handle: u64) {
    SHADOW.with(|cell| unsafe {
        let s = &mut *cell.get();
        let base = frame_handle as usize;
        debug_assert!(
            base + SHADOW_STACK_HEADER_SLOTS <= s.stack.len(),
            "shadow-stack pop past end (corrupted frame handle)"
        );
        let prev_top = s.stack[base] as usize;
        s.stack.truncate(base);
        s.frame_top = prev_top;
    });
}

/// Update slot `idx` in the current frame with NaN-boxed `value`.
/// Codegen emits this at safepoints for each live pointer-typed
/// local. Hot path — compiled code calls this directly or inlines
/// an equivalent sequence; Rust version exists for runtime tests
/// and debug builds.
#[no_mangle]
pub extern "C" fn js_shadow_slot_set(idx: u32, value: u64) {
    SHADOW.with(|cell| unsafe {
        let s = &mut *cell.get();
        let top = s.frame_top;
        if top == usize::MAX {
            return; // no frame active — no-op
        }
        let slot = top + idx as usize;
        if slot < s.stack.len() {
            s.stack[slot] = value;
        }
    });
}

/// Read the current frame's slot `idx` — test-only; Phase B GC
/// tracer walks the raw Vec directly instead of going through a
/// function call per slot.
#[no_mangle]
pub extern "C" fn js_shadow_slot_get(idx: u32) -> u64 {
    SHADOW.with(|cell| unsafe {
        let s = &*cell.get();
        let top = s.frame_top;
        if top == usize::MAX {
            return 0;
        }
        let slot = top + idx as usize;
        if slot < s.stack.len() {
            s.stack[slot]
        } else {
            0
        }
    })
}

/// Current frame depth — test-only.
pub fn shadow_stack_depth() -> usize {
    SHADOW.with(|cell| unsafe {
        let s = &*cell.get();
        // Count frames by walking prev_frame_top pointers from the
        // top back to the bottom. Depth = number of hops to reach
        // `usize::MAX`.
        let mut top = s.frame_top;
        let mut depth = 0;
        while top != usize::MAX && top >= SHADOW_STACK_HEADER_SLOTS {
            depth += 1;
            let header_base = top - SHADOW_STACK_HEADER_SLOTS;
            if header_base >= s.stack.len() {
                break;
            }
            top = s.stack[header_base] as usize;
        }
        depth
    })
}

/// Allocate memory via malloc with GcHeader prepended.
/// Returns pointer to usable memory AFTER the header.
/// The allocation is tracked in MALLOC_STATE.
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
            // Lazy-set: `gc_realloc` rebuilds the lookup set on demand.
            // Mark dirty so the next `ensure_set_built` rebuilds from
            // the updated `objects` Vec. Skips a ~50 ns hashbrown
            // group-probe per allocation on the hot path.
            s.set_dirty = true;
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

            headers.push(header);
            results.push(raw.add(GC_HEADER_SIZE));
        }

        MALLOC_STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.objects.extend_from_slice(&headers);
            // Lazy-set: see `gc_malloc` for rationale.
            s.set_dirty = true;
        });

        GC_FLAGS.with(|f| f.set(f.get() & !GC_FLAG_IN_ALLOC));
    }

    results
}

/// Lazily build `MallocState.set` from `MallocState.objects` if dirty.
/// `gc_malloc` flips `set_dirty = true` instead of inserting into the
/// hashset; callers that actually need the set (`gc_realloc` and the
/// sweep's removal path) call this first to refresh it. For typical
/// allocation-heavy kernels (`promise_all_chains`, object_create) the
/// set is never built at all because they never call `gc_realloc`.
#[inline]
fn ensure_set_built(s: &mut MallocState) {
    if !s.set_dirty {
        return;
    }
    s.set.clear();
    s.set.extend(s.objects.iter().map(|&h| h as usize));
    s.set_dirty = false;
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
    let new_total = GC_HEADER_SIZE + new_payload_size;

    let old_layout = Layout::from_size_align(old_total, 8).unwrap();

    unsafe {
        let new_raw = realloc(old_header as *mut u8, old_layout, new_total);
        if new_raw.is_null() {
            panic!("gc_realloc: failed to reallocate to {} bytes", new_total);
        }

        let new_header = new_raw as *mut GcHeader;
        (*new_header).size = new_total as u32;

        // Update pointer in MALLOC_STATE (objects + set) if it changed.
        if new_header != old_header {
            GC_FLAGS.with(|f| f.set(f.get() | GC_FLAG_IN_ALLOC));
            MALLOC_STATE.with(|s| {
                let mut s = s.borrow_mut();
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
            });
            GC_FLAGS.with(|f| f.set(f.get() & !GC_FLAG_IN_ALLOC));
        }

        new_raw.add(GC_HEADER_SIZE)
    }
}

/// Register a root scanner function.
/// Each scanner is called during the mark phase to discover roots.
pub fn gc_register_root_scanner(scanner: fn(&mut dyn FnMut(f64))) {
    ROOT_SCANNERS.with(|scanners| {
        scanners.borrow_mut().push(scanner);
    });
}

/// Register a global variable address as a GC root.
/// Called by codegen in module init functions.
#[no_mangle]
pub extern "C" fn js_gc_register_global_root(ptr: i64) {
    GLOBAL_ROOTS.with(|roots| {
        roots.borrow_mut().push(ptr as *mut u64);
    });
}

/// Suppress GC triggers. While suppressed, `gc_check_trigger` is a no-op.
/// Used by JSON.parse to avoid mid-parse GC cycles.
pub fn gc_suppress() {
    // Issue #745: snapshot arena_total at suppress-start so the
    // matching `gc_bump_malloc_trigger` can size the suppressed
    // window's parse growth and gate the bytes-trigger bump on it.
    GC_PRE_SUPPRESS_BYTES.with(|c| c.set(crate::arena::arena_total_bytes()));
    GC_FLAGS.with(|f| f.set(f.get() | GC_FLAG_SUPPRESSED));
}

/// Resume GC triggers after suppression.
pub fn gc_unsuppress() {
    GC_FLAGS.with(|f| f.set(f.get() & !GC_FLAG_SUPPRESSED));
}

/// Rebaseline the malloc-count AND arena-bytes triggers to the current
/// live set so that objects just created during a GC-suppressed window
/// (e.g. JSON.parse) don't immediately trip a collection on the next
/// allocation.
///
/// Pre-fix: only the malloc-count trigger was bumped. JSON.parse on the
/// 108 MB honest_bench fixture lifts arena_total to ~108 MB, the bytes
/// trigger is still at its initial 128 MB threshold, and the iterate+
/// rebuild pass that immediately follows trips bytes-based GC after
/// only ~20 MB of new allocations. The 4 mark/sweep cycles each walk
/// the entire 400 MB live heap (the records tree dominates) and add
/// ~800 ms of overhead to the workload. Bumping the bytes trigger by
/// the per-program step (initially 128 MB, grows up to 1 GB on
/// mostly-garbage sweep evidence) defers the first GC until the
/// post-parse working set itself doubles — for json_pipeline_full
/// that means iterate+rebuild completes inside one GC cycle instead
/// of four.
pub fn gc_bump_malloc_trigger() {
    let current = MALLOC_STATE.with(|s| s.borrow().objects.len());
    let step = GC_MALLOC_COUNT_STEP.with(|c| c.get());
    GC_NEXT_MALLOC_TRIGGER.with(|c| c.set(current + step));

    use crate::arena::arena_total_bytes;
    let bytes_now = arena_total_bytes();
    let pre_suppress = GC_PRE_SUPPRESS_BYTES.with(|c| c.get());
    let parse_growth = bytes_now.saturating_sub(pre_suppress);

    // Issue #745: gate the bytes-trigger bump on the suppressed
    // window's parse size, with two regimes:
    //
    //   * Tiny parses (< 1 MB of arena growth) — the
    //     `test_memory_json_churn` shape: 5 k iters × ~13 KB per
    //     parse into a fragmented arena, where every block holds
    //     both live and dead objects so a GC sweep would find 91 %+
    //     bytes dead but reclaim *zero* blocks, then step-double
    //     and cascade RSS up. Always bump here — the original
    //     bytes-bump (commit 56818086) correctly deferred GC
    //     indefinitely on this shape, and we preserve that.
    //
    //   * Medium-or-larger parses (>= 1 MB) — the
    //     `json_pipeline_full` and `json_polyglot` shapes: once per
    //     GC cycle, bump the trigger to grant the post-parse
    //     workload a `step` of headroom. The flag clears in
    //     `gc_collect_inner` so the next cycle gets its own bump.
    //     This is what was missing in commit 56818086 — each
    //     iteration of `json_polyglot`'s 50-iter loop bumped the
    //     trigger by another `step`, and after productive
    //     step-doubling that grew toward 1 GB the trigger ratcheted
    //     hundreds of MB above the actual live set (~5 MB) and GC
    //     never fired across the entire run. Peak RSS climbed to
    //     254/411 MB on the lazy-tape path.
    //
    // Also cap the effective step at the *initial* value (64 MB) so
    // post-`73a48ced` step-doubling can't make a single bump grant
    // hundreds of MB of headroom. The original optimization measured
    // `step` at INITIAL on the first call (no prior GC), so the cap
    // is a no-op for the `json_pipeline_full` workload.
    const TINY_PARSE_BYTES: usize = 1024 * 1024;
    let is_tiny_parse = parse_growth < TINY_PARSE_BYTES;
    if !is_tiny_parse && GC_TRIGGER_BUMPED.with(|c| c.get()) {
        return;
    }

    let bytes_step = GC_STEP_BYTES
        .with(|c| c.get())
        .min(GC_THRESHOLD_INITIAL_BYTES);
    let bytes_trigger = bytes_now.saturating_add(bytes_step);
    // Only raise — never lower — so this can't accidentally trip a
    // pending collection that the existing trigger had already armed.
    GC_NEXT_TRIGGER_BYTES.with(|c| {
        if bytes_trigger > c.get() {
            c.set(bytes_trigger);
            if !is_tiny_parse {
                GC_TRIGGER_BUMPED.with(|b| b.set(true));
            }
        }
    });
}

/// Check if GC should run. Called only when a new arena block is allocated.
/// Skips collection if we're inside gc_malloc/gc_realloc to prevent
/// RefCell double-borrow panics (reentrancy from allocation → arena grow → GC → sweep).
pub fn gc_check_trigger() {
    // Issue #62: single TLS access covers both `in_alloc` and `suppressed`.
    if GC_FLAGS.with(|f| f.get()) & (GC_FLAG_IN_ALLOC | GC_FLAG_SUPPRESSED) != 0 {
        return;
    }
    use crate::arena::arena_total_bytes;
    let total = arena_total_bytes();
    let next_trigger = GC_NEXT_TRIGGER_BYTES.with(|c| c.get());
    if total >= next_trigger {
        // Snapshot pre-GC in-use bytes to measure collection effectiveness.
        // We also capture `freed_bytes` from the sweep itself (sum of dead
        // object sizes). Issue #179: `pre_in_use - post_in_use` measures
        // only block-reset activity, which is gated by the 2-cycle grace
        // period (Issue #73) — the first productive GC in a series will
        // show (pre - post) = 0 even though the sweep found 60%+ dead
        // objects. Using `freed_bytes` reflects true reclaim potential
        // and lets the adaptive step halve on the cycle that first
        // surfaces the dead working set, rather than deferring until
        // after the grace completes.
        let pre_in_use = crate::arena::arena_in_use_bytes();
        let sweep_freed_bytes = gc_collect_inner();
        let post_in_use = crate::arena::arena_in_use_bytes();

        // Adaptive step:
        //   >90% freed → double (almost all dead — `object_create`-style
        //                        hot loops fit their entire working set
        //                        under the threshold; defer.)
        //   10-90% freed → halve (productive collection — real reclaim
        //                         is possible, so collect again sooner
        //                         to keep the working set bounded;
        //                         16MB floor prevents thrash).
        //   <10% freed → double (live set genuinely large, don't thrash).
        //
        // Issue #179: the halve band was formerly 10-25% only. Before
        // the age-restricted block-persist, collections in the 25-90%
        // band were illusory — block-persist re-marked dead neighbors
        // as live, so "freed" over-counted what was actually reclaimable
        // on subsequent cycles. Keeping step flat there was the correct
        // defensive choice. With v0.5.193's block-persist limited to
        // the last 5 general-arena blocks, "freed" now reflects real
        // sweep effectiveness, and widening the halve band lets the
        // trigger fire often enough for middle blocks to actually
        // reset and RSS to stay bounded. `bench_json_roundtrip` moves
        // into this band: first GC frees ~73% → halve → next trigger
        // ~56MB later → second GC frees more → step halves again →
        // RSS stabilizes instead of growing linearly with iters.
        //
        // The >90% and <10% branches retain the existing "don't thrash"
        // protection (Issue #64 follow-up): both extremes mean the
        // live/garbage ratio is such that collecting sooner is wasted
        // work.
        // Adaptive step, driven by the *larger* of sweep-freed-bytes
        // and the block-reset delta (`pre - post`). `freed_bytes` from
        // the sweep surfaces reclaim potential immediately (before the
        // 2-cycle grace completes); `pre - post` reflects actual block
        // resets landing on subsequent cycles. Using the max keeps the
        // step adaptive to both surfaces of productive collection.
        //
        //   >90% freed → double (near-total sweep; `object_create`-style
        //                        hot loops pay one GC then run free).
        //   25-90% freed → halve (productive — reclaim is meaningful,
        //                         collect again sooner to bound RSS).
        //   10-25% freed → keep (marginal — don't thrash vs. churn).
        //   <10% freed → double (live set genuinely large, defer).
        //
        // Issue #179 driver: formerly the halve band was 10-25% only,
        // which never fired on `bench_json_roundtrip` because typical
        // freed-pct there is 50-80%. With the max-of-two metric AND
        // the age-restricted block-persist (v0.5.193), widening the
        // halve band to 25-90% lets the trigger fire often enough for
        // middle blocks to actually reset, without dropping into the
        // 16MB-floor thrash territory that hurts throughput on
        // moderate workloads. `bench_json_roundtrip` lands here on
        // most cycles (60-80% freed) → step halves → GC fires 3-4×
        // across the 50-iter loop → RSS stabilizes around the live-
        // set size plus the 5-block recent-window headroom.
        //
        // The 16MB floor keeps `object_create`-scale hot loops from
        // thrashing: those workloads land in the >90% band on the
        // first GC and immediately double the step, escaping the
        // halve trajectory after a single cycle.
        let block_reclaim = pre_in_use.saturating_sub(post_in_use);
        let freed = std::cmp::max(block_reclaim, sweep_freed_bytes as usize);
        let mut step = GC_STEP_BYTES.with(|c| c.get());
        let old_step = step;
        if pre_in_use > 0 {
            let pct_freed = (freed * 100) / pre_in_use;
            // 2026-05-02: widen the "double" band from `>90% || <10%` to
            // `>=85% || <10%`. ECS perf-comprehensive's two
            // alloc-heavy benches (10k two-comp, 5k × 3 cmds) sweep
            // at 86-89 % freed, which previously landed in the halve
            // band. Step would shrink 64→32→16 MB across the first
            // two benches, then GC fired every ~16 MB of fresh
            // allocations — a 60 ms `mark_block_persisting_arena_objects`
            // outlier landed mid-measured-round on each refire.
            // Promoting 85-90 % to double lets the step grow to the
            // 128 MB ceiling on the first sweep, the trigger jumps
            // out past the bench's full per-iteration allocation
            // budget, and subsequent GCs fire BETWEEN measured rounds
            // (i.e. invisible to the bench's wall-time counter).
            // `bench_json_roundtrip` lands at 50-80 % freed and is
            // unchanged — it still halves and stabilizes at the floor.
            if !(10..=84).contains(&pct_freed) {
                step = (step * 2).min(GC_THRESHOLD_MAX_BYTES);
            } else if pct_freed >= 25 {
                step = (step / 2).max(16 * 1024 * 1024);
            }
            // 10-25% freed → keep step unchanged (marginal churn).
            GC_STEP_BYTES.with(|c| c.set(step));
            if std::env::var_os("PERRY_GC_DIAG").is_some() {
                eprintln!(
                    "[gc-step] pre_in_use={} post_in_use={} sweep_freed={} block_reclaim={} pct={}% step={}→{}",
                    pre_in_use, post_in_use, sweep_freed_bytes, block_reclaim, pct_freed, old_step, step
                );
            }
        }
        let new_total = arena_total_bytes();
        // C4b-δ-tune: hard cap on next_trigger so the >90%-freed
        // step-doubling can't drive peak nursery past the initial
        // threshold. Floor: at least 16 MB of headroom past
        // `new_total` so a workload whose post-GC live set already
        // approaches the ceiling doesn't thrash on every fresh
        // allocation.
        let stepped = new_total.saturating_add(step);
        let capped = stepped.min(GC_TRIGGER_ABSOLUTE_CEILING);
        let floor = new_total.saturating_add(16 * 1024 * 1024);
        let next_trigger = std::cmp::max(capped, floor);
        GC_NEXT_TRIGGER_BYTES.with(|c| c.set(next_trigger));
        // Rebaseline malloc trigger too — the just-completed collection
        // swept malloc objects, so the next malloc-count trigger should
        // be relative to the new survivor count.
        let survivors = MALLOC_STATE.with(|s| s.borrow().objects.len());
        let mstep = GC_MALLOC_COUNT_STEP.with(|c| c.get());
        GC_NEXT_MALLOC_TRIGGER.with(|c| c.set(survivors + mstep));
        return;
    }
    // Also trigger on malloc object count to bound memory growth for
    // services that stay within a single arena block but produce many
    // short-lived strings/closures/bigints per iteration. Since
    // gc_malloc now calls this (issue #34), the threshold is adaptive
    // — it's always `survivor_count + step` after each cycle, so
    // programs with large legitimate live sets don't thrash.
    //
    // Issue #58: the step is now adaptive — after each malloc-triggered
    // collection, if >75% of objects were garbage, double the step (up
    // to 500k). If <25% were garbage, halve it (down to 5k floor).
    // This lets tight loops that produce tons of dead temporaries
    // (string concat, object creation) ramp the step quickly so they
    // pay only a handful of GC cycles instead of ~100.
    let malloc_count = MALLOC_STATE.with(|s| s.borrow().objects.len());
    let next_malloc_trigger = GC_NEXT_MALLOC_TRIGGER.with(|c| c.get());
    if malloc_count >= next_malloc_trigger {
        let pre_count = malloc_count;
        gc_collect_inner();
        let survivors = MALLOC_STATE.with(|s| s.borrow().objects.len());
        // Adapt the malloc-count step based on collection effectiveness.
        //
        // Issue #58 insight: in tight allocation loops the conservative
        // stack scanner keeps almost everything alive — GC finds <10%
        // garbage and wastes time walking 100k+ objects. In this regime
        // we should BACK OFF (increase the step) so the loop can finish
        // without GC interference. Once control returns to a higher scope
        // the dead objects will fall off the stack and become collectable.
        //
        // Conversely, when GC reclaims >75% it's working well and can
        // afford to stay at the current cadence or even speed up.
        let mut mstep = GC_MALLOC_COUNT_STEP.with(|c| c.get());
        if pre_count > 0 {
            let freed = pre_count.saturating_sub(survivors);
            let pct_freed = (freed * 100) / pre_count;
            if pct_freed < 15 {
                // GC is nearly useless — quadruple the step to back off fast
                mstep = (mstep * 4).min(GC_MALLOC_COUNT_STEP_MAX);
            } else if pct_freed < 50 {
                // GC is partially effective — double the step
                mstep = (mstep * 2).min(GC_MALLOC_COUNT_STEP_MAX);
            } else if pct_freed > 90 {
                // GC is highly effective — halve the step to collect sooner
                mstep = (mstep / 2).max(GC_MALLOC_COUNT_STEP_MIN);
            }
            // 50-90% freed: keep current step (balanced)
            GC_MALLOC_COUNT_STEP.with(|c| c.set(mstep));
        }
        GC_NEXT_MALLOC_TRIGGER.with(|c| c.set(survivors + mstep));
    }
}

/// Counter tracking "worker threads hold JSValue roots we can't scan"
/// state. Incremented by stdlib entry points that spawn tokio tasks which
/// invoke user closures on worker threads (WS server, HTTP server, etc.).
/// When > 0, the conservative main-thread stack scanner can't see all
/// live roots — collecting would free objects still referenced from
/// worker-thread stacks and SEGV on next access.
///
/// Issue #31: gc() from setInterval in a Fastify+WebSocket server crashed
/// within 60s of the first tick because WS worker threads held live refs
/// to message payload strings on their stacks. This counter lets stdlib
/// features signal "please skip user-initiated gc() while I'm running"
/// without a full stop-the-world mutex.
pub static GC_UNSAFE_ZONES: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// One-shot warning so we don't spam stderr on every tick.
static GC_UNSAFE_WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Manual GC trigger (callable from TypeScript as `gc()`). Skipped when
/// worker threads are active (see GC_UNSAFE_ZONES).
#[no_mangle]
pub extern "C" fn js_gc_collect() {
    if GC_UNSAFE_ZONES.load(std::sync::atomic::Ordering::Acquire) > 0 {
        // One-shot warning — user likely has `setInterval(() => gc(), N)`
        // in a server; we don't want to print every 30s.
        if !GC_UNSAFE_WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            eprintln!(
                "perry: gc() skipped — a tokio-based server (WebSocket/HTTP) is active \
                 and may hold JSValue refs on worker threads that the main-thread GC \
                 can't see. Manual gc() is a no-op for the rest of this process."
            );
        }
        return;
    }
    gc_collect_inner();
}

/// Increment GC_UNSAFE_ZONES. Called by stdlib when spawning tokio tasks
/// that invoke user closures on worker threads.
#[no_mangle]
pub extern "C" fn js_gc_enter_unsafe_zone() {
    GC_UNSAFE_ZONES.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
}

/// Decrement GC_UNSAFE_ZONES. Called when a stdlib feature that owns
/// worker threads shuts down (e.g. ws_server_close).
#[no_mangle]
pub extern "C" fn js_gc_exit_unsafe_zone() {
    GC_UNSAFE_ZONES.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
}

/// Threshold-based GC trigger (safe for use from the event loop).
/// Only runs collection if arena or malloc thresholds are exceeded.
#[no_mangle]
pub extern "C" fn gc_check_trigger_export() {
    gc_check_trigger();
}

/// Main GC collection
/// Gen-GC Phase C3b minor-collection entry. Skips old-gen during
/// the trace phase: old-gen objects are marked-and-skipped (their
/// fields aren't recursively visited). Young children held by
/// old-gen parents reach the worklist exclusively via the
/// remembered set, scanned by `mark_remembered_set_roots`.
///
/// **Correctness contract** (per docs/generational-gc-plan.md §C):
/// - Every old→young write since the last collection MUST have
///   recorded the parent in the RS (codegen emits the barrier at
///   every PropertySet / IndexSet / closure-capture-set site —
///   see `crates/perry-codegen/src/expr.rs::emit_write_barrier`).
/// - The conservative C-stack scan still runs; any young pointer
///   reachable via runtime register-roots stays live.
/// - Old-gen objects' MARK bit gets set during the trace step
///   (caller pushes them onto the worklist); the MINOR trace just
///   doesn't recurse through them.
///
/// Sweep is unchanged from full GC — `arena_reset_empty_blocks`
/// already restricts itself to nursery blocks, so old-gen blocks
/// are structurally untouched. The malloc-side sweep walks
/// `MALLOC_STATE.objects`; any unmarked entry there is reclaimed
/// regardless of generation. (Phase C4 will refine this if minor
/// GC begins running on old-gen-heavy workloads.)
///
/// Gated by `PERRY_GEN_GC=1` via `gen_gc_enabled()`. Default OFF —
/// shipping as opt-in until the bench_json_roundtrip ship criterion
/// (RSS ≤70 MB direct path) lands and proves out across the gap +
/// parity test corpus.
pub fn gc_collect_minor() -> u64 {
    // Phase C4b-γ-3: re-entrancy guard. Without this, the evacuation
    // pass's `arena_alloc_gc_old` can trigger `gc_check_trigger` (via
    // `arena.alloc`'s slow-path block-fill) DURING the outer collection
    // cycle. The outer cycle's MARK_SEEDS, CONS_PINNED, and valid_ptrs
    // are all in indeterminate states mid-evac; a recursive
    // `gc_collect_minor` clears them, runs its own mark phase from a
    // mostly-empty C-stack snapshot (we're deep inside the runtime,
    // very few user pointers reachable), evacuates whatever it can find,
    // then returns to the outer cycle which proceeds with corrupt
    // pinning + corrupt seed list. Symptom: bench_evac_heavy's `cache`
    // local gets evacuated by the inner cycle (un-pinned because the
    // inner mark_stack_roots can't see it through the deep-runtime
    // stack), and the outer rewrite walk doesn't update the user's
    // shadow stack slot to point at the new copy → cache.length reads
    // garbage from the FORWARDED slot's first 8 bytes thereafter.
    //
    // Fix: set GC_FLAG_IN_ALLOC for the entire duration of
    // gc_collect_minor. `gc_check_trigger` already early-returns when
    // this bit is set. Any recursive `gc_check_trigger` call from
    // arena_alloc_gc_old / arena_alloc_gc / gc_malloc inside the
    // collection sees the bit and bails. The outer cycle's bookkeeping
    // stays intact.
    let prev_in_alloc = GC_FLAGS.with(|f| {
        let prev = f.get();
        f.set(prev | GC_FLAG_IN_ALLOC);
        prev & GC_FLAG_IN_ALLOC
    });
    let start = std::time::Instant::now();
    // MARK_SEEDS persists across GC cycles. Clear before any try_mark
    // call so trace sees only this cycle's freshly-marked headers.
    clear_mark_seeds();
    let valid_ptrs = build_valid_pointer_set();

    // === MARK PHASE (minor) ===
    // Order matters for the C4b pinning policy. We pin from every
    // root source whose internal data structure isn't yet wired
    // for reference-rewriting (= sources that need their pointer
    // slots updated post-evacuation; if we can't rewrite, we must
    // not move):
    //
    //   1. Conservative C-stack scan (mark_stack_roots) — can't
    //      rewrite stack words discovered by bit-pattern matching.
    //   2. The 9 registered root scanners (mark_registered_roots) —
    //      their callback API exposes only mutable-mark, not
    //      mutable-rewrite. C4b-γ would refactor each scanner; for
    //      now we pin their discoveries.
    //
    // Sources we DO plan to rewrite in a future C4b-γ pass and
    // therefore do NOT pin from:
    //
    //   - Module globals (GLOBAL_ROOTS) — Vec<*mut JSValue>,
    //     iterable mutable.
    //   - Shadow stack — direct Vec<u64> we already walk in
    //     `shadow_stack_root_scanner`.
    //   - Marked-object heap fields (incl. RS-parent fields) —
    //     trace_*-family walks them per obj_type; a parallel
    //     rewrite_*-family will land in C4b-γ-2.
    //
    // The pinning calls fire BEFORE trace so they capture only
    // root-direct discoveries, not transitive heap reachability —
    // transitively-discovered objects came via heap fields that
    // C4b-γ-2 will walk to rewrite.
    mark_stack_roots(&valid_ptrs);
    // CONS_PINNED is only consumed by `evacuate_tenured_nursery_objects`,
    // which is gated on `gen_gc_evacuate_enabled()`. When evacuation is
    // off (the default), each `pin_currently_marked_as_conservative` call
    // is a full arena walk producing a HashSet nobody reads — at ~1.6M
    // objects in perf-comprehensive that's two ~10ms walks per minor GC
    // burned for nothing. Gate both pin calls behind the same flag.
    let evac = gen_gc_evacuate_enabled();
    if evac {
        pin_currently_marked_as_conservative();
    }
    mark_global_roots(&valid_ptrs);
    mark_registered_roots(&valid_ptrs);
    // C4b-γ-1: pin again to capture scanner-discovered objects
    // (objects that mark_registered_roots added since the prior
    // pin call). The HashSet absorbs the redundant inserts of
    // already-pinned conservative discoveries cheaply.
    if evac {
        pin_currently_marked_as_conservative();
    }
    mark_remembered_set_roots(&valid_ptrs);
    trace_marked_objects_minor(&valid_ptrs);
    mark_block_persisting_arena_objects(&valid_ptrs);
    // Phase C4b-γ-2 makes evacuation correctness-safe: the
    // post-evac `rewrite_forwarded_references` walk visits every
    // reference site we own (shadow stack + module globals + every
    // marked heap object's fields) and rewrites pointers to
    // forwarded objects. The transitive-pinning safety valve
    // formerly here is no longer needed — non-pinned tenured
    // objects are now genuine evacuation candidates and the bench
    // RSS win lands accordingly.

    // === AGE-BUMP PASS (gen-GC Phase C4) ===
    // Folded into the sweep walk via `sweep_with_age_bump(true)` below.
    // Each general-arena object header was walked twice per minor GC: once
    // here for HAS_SURVIVED/TENURED bookkeeping, once in sweep for the
    // mark/free decision. With ~1.6M objects per cycle in
    // perf-comprehensive that doubled the per-cycle header-touch cost; the
    // merged walk halves it. Aging applies to nursery only (gated on
    // `block_idx < general_block_count()` inside the merged walk), matching
    // the original `pointer_in_old_gen` skip.
    //
    // Two-bit aging (HAS_SURVIVED → TENURED) gives PROMOTION_AGE=2:
    //   - First survival:  set HAS_SURVIVED.
    //   - Second survival: set TENURED, clear HAS_SURVIVED.
    //
    // Tenured objects are skipped by `drain_trace_worklist_minor` on
    // subsequent minor GCs — bounded by the time-win generational design
    // promises. They stay PHYSICALLY in nursery (no copying) so RSS
    // doesn't drop until Phase C4b lands real evacuation.

    // === EVACUATION PASS (Phase C4b-β + C4b-γ-2, opt-in) ===
    // Copy non-pinned tenured nursery objects into OLD_ARENA and
    // install forwarding pointers in the original nursery slots,
    // then rewrite every reference site we own (shadow stack,
    // module globals, all marked heap objects' fields) to point
    // at the new addresses. After rewriting, the original nursery
    // slots are stale and the sweep that follows reclaims their
    // blocks (their MARKED bit was cleared at evac time).
    //
    // Conservative-stack discoveries are pinned by
    // `pin_currently_marked_as_conservative` above so their
    // referenced objects are not evacuated — we never rewrite
    // C-stack words.
    if gen_gc_evacuate_enabled() {
        let n_evac = evacuate_tenured_nursery_objects();
        rewrite_forwarded_references(&valid_ptrs);
        if std::env::var_os("PERRY_GC_DIAG").is_some() {
            eprintln!(
                "[gc-evac] evacuated={} cons_pinned={}",
                n_evac,
                cons_pinned_count()
            );
        }
    }

    // === SWEEP PHASE ===
    // `do_age_bump = true` folds the per-object HAS_SURVIVED / TENURED
    // update into this same walk (see comment block above the removed
    // dedicated age-bump pass).
    let freed_bytes = sweep_with_age_bump(true);

    // RS clear — see gc_collect_inner for the rationale.
    REMEMBERED_SET.with(|s| s.borrow_mut().clear());
    // Conservative-pinning is per-cycle; clear so next cycle
    // re-discovers fresh.
    CONS_PINNED.with(|s| s.borrow_mut().clear());

    #[cfg(target_env = "gnu")]
    unsafe {
        libc::malloc_trim(0);
    }

    let elapsed_us = start.elapsed().as_micros() as u64;
    GC_STATS.with(|stats| {
        let mut stats = stats.borrow_mut();
        stats.collection_count += 1;
        stats.total_freed_bytes += freed_bytes;
        stats.last_pause_us = elapsed_us;
    });
    // Restore IN_ALLOC to its pre-collection state. Usually this clears
    // the bit (collections fire from contexts where IN_ALLOC was clear);
    // if the outer caller had it set (e.g., we got here via
    // `js_gc()` invoked from a runtime function that already held the
    // flag), preserve their state.
    GC_FLAGS.with(|f| {
        let cur = f.get();
        if prev_in_alloc != 0 {
            f.set(cur | GC_FLAG_IN_ALLOC);
        } else {
            f.set(cur & !GC_FLAG_IN_ALLOC);
        }
    });
    freed_bytes
}

/// Generational GC (minor collection on every trigger) is now the
/// default model as of Phase D (v0.5.237). Set `PERRY_GEN_GC=0`,
/// `=false`, or `=off` to opt out and fall back to the full
/// mark-sweep — kept as an escape hatch for bisecting GC-related
/// regressions in user programs.
///
/// Why generational is the default now: Phase C (v0.5.222-228) wired
/// the nursery / old-gen split, write barriers, remembered set, and
/// non-moving tenuring; Phase C4b (v0.5.229-236) added forwarding
/// pointer infrastructure, conservative-pinning safety, evacuation
/// (still gated `PERRY_GEN_GC_EVACUATE=1`), reference rewriting,
/// idle-block deallocation, and the trigger ceiling that bounds
/// peak nursery occupancy. The minor-GC path has been the proven-
/// equivalent default in every regression suite (168 unit tests,
/// 9 `test_json_*.ts` × 4 mode combos, 18 memory-stability tests)
/// since C3b landed; flipping the default makes those gains apply
/// to user programs without requiring an env-var opt-in.
pub fn gen_gc_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_GEN_GC").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// Gen-GC Phase C4b: `PERRY_GEN_GC_EVACUATE=1` enables the
/// experimental copying evacuation that physically relocates
/// tenured non-pinned nursery objects into OLD_ARENA. Default OFF
/// — opt-in for testing until C4b-γ (reference rewriting) lands
/// and the evacuation is correctness-complete on all reference
/// sites. Without C4b-γ, evacuated objects' nursery slots hold
/// forwarding pointers but references to those slots still point
/// at the old (forwarded) location — reads through those refs
/// get garbage.
pub fn gen_gc_evacuate_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        matches!(
            std::env::var("PERRY_GEN_GC_EVACUATE").as_deref(),
            Ok("1") | Ok("on") | Ok("true")
        )
    })
}

fn gc_collect_inner() -> u64 {
    // Issue #745: clear the per-cycle bytes-bump flag so the next
    // gc-suppressed parse can rebaseline the trigger again. Done at
    // the top so all entry points — full GC, minor GC, manual
    // `gc()`, the malloc-count trigger path — keep the flag in sync.
    GC_TRIGGER_BUMPED.with(|c| c.set(false));
    if gen_gc_enabled() {
        return gc_collect_minor();
    }
    let start = std::time::Instant::now();

    // MARK_SEEDS persists across GC cycles. Clear before any try_mark
    // call so trace sees only this cycle's freshly-marked headers.
    clear_mark_seeds();
    // Build set of valid heap pointers for conservative stack scan validation
    let valid_ptrs = build_valid_pointer_set();

    // === MARK PHASE ===

    // 1. Conservative stack scan
    mark_stack_roots(&valid_ptrs);

    // 2. Scan registered global roots (module-level variables)
    mark_global_roots(&valid_ptrs);

    // 3. Run registered root scanners (promise queues, timers, etc.)
    mark_registered_roots(&valid_ptrs);

    // 3b. Gen-GC Phase C3: scan remembered set as additional roots.
    //     Old-gen objects that wrote young-gen pointers since the
    //     last collection are recorded here by the write barrier
    //     (gen-gc-plan.md §C). For full GC this is redundant with
    //     the conservative+precise scan that already covered them,
    //     but it's cheap and keeps the dispatch path uniform with
    //     the eventual minor-GC entry. RS is cleared at the end of
    //     collection so the next cycle starts coherent.
    mark_remembered_set_roots(&valid_ptrs);

    // 4. Trace from marked roots (iterative worklist)
    trace_marked_objects(&valid_ptrs);

    // 5. Block-persistence pass: arena blocks survive whole or not at all, so
    //    arena objects sharing a block with a root-reachable object persist
    //    even when not themselves reachable. Their malloc children must stay
    //    alive too (issues #43 / #44).
    mark_block_persisting_arena_objects(&valid_ptrs);

    // === SWEEP PHASE ===
    // sweep() now clears mark bits on surviving objects inline,
    // eliminating 2 redundant heap walks (arena + malloc).
    let freed_bytes = sweep();

    // Gen-GC Phase C3: clear the remembered set after sweep. The
    // RS records old→young writes since the previous collection;
    // after a full collection, every young object referenced by
    // an old-gen parent has either been kept alive (via the
    // mark_remembered_set_roots scan above) or is dead and gets
    // swept. Either way the parent's RS entry is no longer
    // load-bearing — the next allocation cycle's barrier emissions
    // will repopulate it as needed.
    REMEMBERED_SET.with(|s| s.borrow_mut().clear());

    // Return released glibc heap pages to the kernel. Without this, glibc
    // keeps freed memory in its arena for reuse but never shrinks RSS, so
    // long-running services show unbounded RSS growth from transient
    // allocations (HTTP buffers, JSON parsers, etc.) even though the
    // Perry GC successfully frees the underlying objects.
    // No-op on non-glibc platforms (macOS, musl).
    #[cfg(target_env = "gnu")]
    unsafe {
        libc::malloc_trim(0);
    }

    let elapsed_us = start.elapsed().as_micros() as u64;

    GC_STATS.with(|stats| {
        let mut stats = stats.borrow_mut();
        stats.collection_count += 1;
        stats.total_freed_bytes += freed_bytes;
        stats.last_pause_us = elapsed_us;
    });
    freed_bytes
}

/// A sorted-`Vec`-backed set of valid user-space heap pointers,
/// used to validate candidate addresses found during the conservative
/// stack scan.
///
/// Two-region layout: arena pointers and malloc pointers are stored
/// in *separate* sorted Vecs. The address-sorted arena walker emits
/// `arena_sorted` already in ascending order with no merge required,
/// so finalize only sorts the small `malloc_sorted` tail (typically a
/// few thousand entries) instead of running driftsort's K-way merge
/// across all 1.6 M arena pointers + the malloc tail. The merge phase
/// of the previous single-Vec implementation cost ~80 ms per GC cycle
/// on perf-comprehensive (1.65 M element memcpy through main memory);
/// keeping the regions separate eliminates it entirely.
///
/// `contains` does two binary searches instead of one (~15 ns extra
/// per call), but contains is only called a few times per traced
/// pointer field — bench profile shows < 500k calls per cycle, so
/// the per-call overhead is dwarfed by the merge savings.
///
/// Profiling background: `HashSet<usize>` with 700 k entries was the
/// dominant GC cost in `object_create` — even after pre-sizing the
/// 700 k inserts were ~10-15 ms per collection because of repeated
/// hash computation + cache misses on the bucket array. Sorted-Vec
/// is ~3× faster on this workload at build time and the O(log n)
/// lookup is fast enough that the few thousand stack-scan candidate
/// validations per GC barely move the total.
pub(crate) struct ValidPointerSet {
    /// Insertion-side staging for arena entries — filled in ascending
    /// order by the address-sorted arena walk. Swapped into
    /// `merged_sorted` in `finalize()` so `enclosing_object` can do
    /// its interior-pointer floor-search. Malloc entries are *not*
    /// staged here: they are inserted directly into `lookup_set`
    /// during the malloc walk, bypassing the per-cycle
    /// `sort_unstable` + merge that dominated `build_valid_pointer_set`
    /// on promise-heavy kernels (5-6 % of total kernel time).
    arena_sorted: Vec<usize>,
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
    merged_sorted: Vec<usize>,
    /// O(1) hash set for the hot `contains` path. Built from
    /// `merged_sorted` in `finalize()` with `PtrHasher` (Fibonacci-
    /// multiplicative on `usize`) — pointer keys are already well-
    /// distributed, so SipHash buys nothing and a single `mul` per
    /// lookup keeps the hash step out of the cache-miss budget. One
    /// cache miss per lookup (the bucket group) replaces the 17 cache
    /// misses of the binary-search path.
    lookup_set: crate::fast_hash::PtrHashSet<usize>,
    // Min/max heap-pointer range across the merged set. Populated in
    // `finalize()`. The conservative stack scan calls `contains` once per
    // 8-byte stack word (~1024 calls per scanned KB of stack) and
    // `try_mark_value` calls it once per scanned root and once per
    // traced reference field. Most candidates that pass the NaN-tag
    // check are real heap pointers and DO fall inside the range,
    // so the prefilter mostly helps for the raw-pointer fallback path
    // where stack words may be return addresses / plain ints / spilled
    // function pointers. Cheap to maintain regardless.
    range_min: usize,
    range_max: usize,
}

impl ValidPointerSet {
    fn new(arena_capacity: usize, malloc_capacity: usize) -> Self {
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
        }
    }
    /// Caller must guarantee that pushes happen in ascending address
    /// order — `build_valid_pointer_set` does so via
    /// `arena_walk_objects_addr_sorted`.
    fn push_arena(&mut self, ptr: usize) {
        self.arena_sorted.push(ptr);
    }
    fn finalize(&mut self) {
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
    fn record_malloc_for_range(&mut self, ptr: usize) {
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

    fn find_floor(sorted: &[usize], ptr: usize) -> Option<usize> {
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
fn build_valid_pointer_set() -> ValidPointerSet {
    let malloc_count = MALLOC_STATE.with(|s| s.borrow().objects.len());
    // 48 bytes is a conservative under-estimate (smaller than the
    // typical 96-byte class instance) so the Vec doesn't realloc.
    let arena_estimate = crate::arena::arena_total_bytes() / 48;
    let mut set = ValidPointerSet::new(arena_estimate + 64, malloc_count + 64);

    // Arena objects: walk arena blocks in ascending data-pointer
    // order so the pushed user_ptrs land in `arena_sorted` already
    // sorted (within each block, offsets only increase, so
    // block-by-block ascending-address yields globally ascending user
    // pointers). No merge needed in finalize for this region.
    crate::arena::arena_walk_objects_addr_sorted(|header_ptr| {
        let user_ptr = unsafe { (header_ptr as *mut u8).add(GC_HEADER_SIZE) };
        set.push_arena(user_ptr as usize);
    });

    // Malloc objects: insert *directly* into the lookup_set,
    // bypassing `malloc_sorted` + the per-cycle
    // `sort_unstable() + merge`. On promise-heavy kernels
    // `MallocState.objects` is millions of entries (~2.1 M on
    // `promise_all_chains`); the per-cycle `sort_unstable` was
    // ~30 ms (5-6 % of total kernel time) and the
    // subsequent `lookup_set.extend(...)` did the same hashset
    // inserts anyway. Cutting straight to `lookup_set.insert` gives
    // us the same hashset content without the Vec materialization,
    // sort, or merge step.
    //
    // Correctness note: this means `merged_sorted` (and therefore
    // `enclosing_object`) contains arena entries only. That is the
    // intended scope: every Perry runtime function known to derive
    // an interior pointer (`js_array_reduce`'s `elements_ptr = arr + 8`,
    // `js_buffer_data = buf + 8`, etc.) holds it across user
    // callbacks for an *arena-allocated* array/buffer; malloc-tracked
    // types (Closure, Promise, String, Map, Error, BigInt, Symbol)
    // are accessed exclusively at their user pointer (object start).
    // If a future runtime function starts holding an interior pointer
    // into a malloc-allocated object, this comment is the place to
    // revisit.
    MALLOC_STATE.with(|s| {
        let s = s.borrow();
        for &header in s.objects.iter() {
            let user_ptr = unsafe { (header as *mut u8).add(GC_HEADER_SIZE) };
            let addr = user_ptr as usize;
            set.lookup_set.insert(addr);
            set.record_malloc_for_range(addr);
        }
    });

    set.finalize();
    set
}

/// Get the GcHeader for a user pointer (pointer returned by gc_malloc or arena_alloc_gc).
/// The header is located GC_HEADER_SIZE bytes before the user pointer.
#[inline]
unsafe fn header_from_user_ptr(user_ptr: *const u8) -> *mut GcHeader {
    (user_ptr as *mut u8).sub(GC_HEADER_SIZE) as *mut GcHeader
}

// Try to mark a value (if it's a heap pointer). Returns true if newly marked.
// === MARK_SEEDS ===
// Per-cycle worklist populated by `try_mark_value` /
// `try_mark_value_or_raw` whenever they newly mark an object. The
// `trace_marked_objects[_minor]` entry points then drain this list
// instead of doing a full arena walk to find marked headers — saving
// ~10 ms per cycle in perf-comprehensive (1.6M objects/cycle).
//
// Re-entrant pushes during trace are fine: trace functions also push
// the newly-marked header onto their LOCAL worklist (the one
// `drain_trace_worklist_inner` is iterating), which is the path the
// drain actually consumes. The seeds list keeps accumulating during
// trace — those duplicate entries are harmless because either the
// next `take_mark_seeds()` call clears them, or the next GC cycle
// starts with `clear_mark_seeds()`.
thread_local! {
    static MARK_SEEDS: std::cell::UnsafeCell<Vec<*mut GcHeader>> =
        std::cell::UnsafeCell::new(Vec::new());
}

#[inline(always)]
fn push_mark_seed(header: *mut GcHeader) {
    MARK_SEEDS.with(|cell| unsafe {
        (*cell.get()).push(header);
    });
}

#[inline]
fn take_mark_seeds() -> Vec<*mut GcHeader> {
    MARK_SEEDS.with(|cell| unsafe { std::mem::take(&mut *cell.get()) })
}

#[inline]
fn clear_mark_seeds() {
    MARK_SEEDS.with(|cell| unsafe {
        (*cell.get()).clear();
    });
}

#[inline]
fn try_mark_value(value_bits: u64, valid_ptrs: &ValidPointerSet) -> bool {
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

/// Conservative stack scan: scan the current thread's stack for heap pointers.
/// Handles BOTH NaN-boxed pointers (POINTER_TAG/STRING_TAG/BIGINT_TAG) AND raw I64 pointers.
/// Raw I64 pointers arise from Perry's `is_array`/`is_string`/`is_pointer`/`is_closure` local
/// variables — codegen stores these as raw I64 words (not NaN-boxed) in registers and on stack.
fn mark_stack_roots(valid_ptrs: &ValidPointerSet) {
    // Capture callee-saved registers into a buffer via setjmp.
    //
    // On Apple platforms the C `setjmp(3)` saves the signal mask via a
    // `sigprocmask` system call, which dominates GC cost (~25 μs per
    // call on arm64). We only need register capture, not signal-state
    // save — switch to `_setjmp(3)` (linker symbol `__setjmp`) on
    // Apple targets. See the matching switch in
    // `promise.rs::js_promise_run_microtasks` for the full rationale.
    let mut jmp_buf = [0u64; 32]; // oversized for safety
    unsafe {
        #[cfg(target_vendor = "apple")]
        extern "C" {
            #[link_name = "_setjmp"]
            fn setjmp(env: *mut u64) -> i32;
        }
        #[cfg(not(target_vendor = "apple"))]
        extern "C" {
            fn setjmp(env: *mut u64) -> i32;
        }
        setjmp(jmp_buf.as_mut_ptr());
    }

    // Scan the register buffer (covers callee-saved regs: x19-x28 on AArch64, rbx/rbp/r12-r15 on x86_64)
    for &word in &jmp_buf {
        try_mark_value_or_raw(word, valid_ptrs);
    }

    // Issue #73: setjmp only captures callee-saved registers. On
    // macOS ARM64 that's x19-x28 + d8-d15 — it misses d0-d7 and
    // d16-d31 (caller-saved FP regs where LLVM may be holding a
    // NaN-boxed pointer across the async poll loop's internal calls,
    // especially under heavy optimization). Capture them explicitly
    // via inline asm so any spilling LLVM hasn't performed is
    // irrelevant — we read the regs directly as they stand at GC
    // entry. A value in d0-d31 ANY of which happens to be a
    // NaN-boxed heap pointer gets marked here.
    #[cfg(target_arch = "aarch64")]
    unsafe {
        let mut fp_regs: [u64; 32] = [0; 32];
        std::arch::asm!(
            "str d0,  [{buf}, #0x00]",
            "str d1,  [{buf}, #0x08]",
            "str d2,  [{buf}, #0x10]",
            "str d3,  [{buf}, #0x18]",
            "str d4,  [{buf}, #0x20]",
            "str d5,  [{buf}, #0x28]",
            "str d6,  [{buf}, #0x30]",
            "str d7,  [{buf}, #0x38]",
            "str d8,  [{buf}, #0x40]",
            "str d9,  [{buf}, #0x48]",
            "str d10, [{buf}, #0x50]",
            "str d11, [{buf}, #0x58]",
            "str d12, [{buf}, #0x60]",
            "str d13, [{buf}, #0x68]",
            "str d14, [{buf}, #0x70]",
            "str d15, [{buf}, #0x78]",
            "str d16, [{buf}, #0x80]",
            "str d17, [{buf}, #0x88]",
            "str d18, [{buf}, #0x90]",
            "str d19, [{buf}, #0x98]",
            "str d20, [{buf}, #0xa0]",
            "str d21, [{buf}, #0xa8]",
            "str d22, [{buf}, #0xb0]",
            "str d23, [{buf}, #0xb8]",
            "str d24, [{buf}, #0xc0]",
            "str d25, [{buf}, #0xc8]",
            "str d26, [{buf}, #0xd0]",
            "str d27, [{buf}, #0xd8]",
            "str d28, [{buf}, #0xe0]",
            "str d29, [{buf}, #0xe8]",
            "str d30, [{buf}, #0xf0]",
            "str d31, [{buf}, #0xf8]",
            buf = in(reg) fp_regs.as_mut_ptr(),
            options(nostack, preserves_flags),
        );
        for &word in &fp_regs {
            try_mark_value_or_raw(word, valid_ptrs);
        }
    }

    // Get stack bounds
    let stack_top: usize;
    #[cfg(target_arch = "aarch64")]
    unsafe {
        let sp: u64;
        std::arch::asm!("mov {}, sp", out(reg) sp);
        stack_top = sp as usize;
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        let sp: u64;
        std::arch::asm!("mov {}, rsp", out(reg) sp);
        stack_top = sp as usize;
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        // Fallback: skip stack scan on unsupported architectures
        return;
    }

    let stack_bottom = get_stack_bottom();
    if stack_bottom == 0 {
        return; // Can't determine stack bounds
    }

    // Walk the stack from current SP to stack bottom.
    // Each 8-byte word may be: NaN-boxed pointer, raw I64 heap pointer, return addr, or plain value.
    let mut addr = stack_top;
    while addr < stack_bottom {
        let word = unsafe { *(addr as *const u64) };
        try_mark_value_or_raw(word, valid_ptrs);
        addr += 8;
    }
}

/// Mark a value if it is a heap pointer — either NaN-boxed OR a raw I64 pointer.
/// Returns true if newly marked.
/// This is used for conservative scanning where Perry stores raw I64 pointers (for is_string/
/// is_array/is_pointer/is_closure vars) alongside NaN-boxed F64 values.
#[inline]
fn try_mark_value_or_raw(word: u64, valid_ptrs: &ValidPointerSet) -> bool {
    // First try NaN-boxed interpretation (POINTER_TAG / STRING_TAG / BIGINT_TAG)
    if try_mark_value(word, valid_ptrs) {
        return true;
    }
    // Fallback: treat as raw (non-NaN-boxed) heap pointer.
    // Perry's is_string/is_array/is_pointer/is_closure locals store raw I64 addresses.
    // Validate against the known-heap-pointer set to avoid false positives from return addresses
    // and plain integers. Valid heap pointers are in the lower 48-bit address space and
    // won't have NaN-boxing tags in upper bits (already rejected above).
    let raw_ptr_u64 = word;
    if !(0x1000..=0x0000_FFFF_FFFF_FFFF).contains(&raw_ptr_u64) {
        return false; // Too small (null/invalid) or has upper bits set (NaN tag or non-address)
    }
    let raw_ptr = raw_ptr_u64 as usize;
    // Heap-range short-circuit: every valid raw heap pointer (object
    // start OR interior) must lie within [range_min, range_max + max
    // object size]. The interior-pointer case can land up to one
    // object-size past `range_max`, so we widen the upper bound by
    // an absolute slack to keep `enclosing_object` reachable for the
    // few real interior pointers that exist (`js_array_reduce`'s
    // `elements_ptr = arr + 8` shape, etc.). The slack is bounded by
    // the largest GcHeader.size field actually used — Perry's biggest
    // legitimate single allocation is a class instance with many
    // string fields, well under 4 KB. Anything larger came from a
    // pinned arena object (rare; doesn't reach this path) so 1 MB
    // gives plenty of headroom while still rejecting the typical
    // mis-tagged stack word.
    if !valid_ptrs.maybe_contains(raw_ptr)
        && raw_ptr.saturating_sub(0x10_0000) > valid_ptrs.range_max
    {
        return false;
    }
    // Try direct match first (pointer to object start).
    let target = if valid_ptrs.contains(&raw_ptr) {
        raw_ptr
    } else {
        // Issue #73: interior-pointer fallback. Runtime functions like
        // `js_array_reduce` derive `elements_ptr = arr + 8` and hold
        // only the interior pointer across user-callback invocations.
        // A conservative scan that only matches object-start addresses
        // would miss this, letting the GC sweep the backing array
        // mid-iteration. Look up the enclosing object and mark that.
        match valid_ptrs.enclosing_object(raw_ptr) {
            Some(start) => start,
            None => return false,
        }
    };
    unsafe {
        let header = header_from_user_ptr(target as *const u8);
        if (*header).gc_flags & GC_FLAG_MARKED != 0 {
            return false; // Already marked
        }
        if (*header).gc_flags & GC_FLAG_PINNED != 0 {
            return false; // Pinned objects are always live
        }
        (*header).gc_flags |= GC_FLAG_MARKED;
        push_mark_seed(header);
    }
    true
}

/// Specialized mark-and-enqueue for trace-phase field walks.
///
/// `trace_closure`, `trace_array`, `trace_object`, `trace_map`,
/// `trace_promise.value/.reason` all share the same pattern: read a
/// heap-field word that is either a NaN-boxed JSValue or a raw I64
/// pointer at an object start, mark it if live, and push the marked
/// header onto the local worklist. The generic
/// `try_mark_value_or_raw` is general enough to also handle
/// conservative stack scans (raw interior pointers via
/// `enclosing_object`) and root scans (push to MARK_SEEDS so the
/// trace-marked-objects entry point can pick them up), but BOTH of
/// those features are pure overhead inside `drain_trace_worklist`:
///
/// 1. Field words never hold interior pointers — they're written via
///    `arr[i] = x` / `obj.f = x` / closure capture stores, all of
///    which use the object-start user pointer. Skipping
///    `enclosing_object` saves a binary-search lookup per field.
///
/// 2. The MARK_SEEDS push happens once per newly-marked object during
///    trace, but the same header is also pushed onto the local
///    worklist by the caller (so the trace drain visits it). The
///    extra MARK_SEEDS push goes onto a TLS vec, gets cleared at the
///    start of the next cycle, and is pure waste while we're already
///    in the trace phase. Skipping it saves a TLS slot deref +
///    Vec::push per marked object.
///
/// 3. The caller-side re-decode of the NaN-tag (to figure out
///    POINTER_MASK extraction vs raw-pointer extraction) is folded
///    into this function, so the caller doesn't pay that switch a
///    second time.
///
/// The valid-pointer hashset check is still load-bearing here — we
/// only elide the secondary `enclosing_object` fallback.
#[inline(always)]
unsafe fn mark_field_into_worklist(
    val_bits: u64,
    valid_ptrs: &ValidPointerSet,
    worklist: &mut Vec<*mut GcHeader>,
) {
    let tag = val_bits & TAG_MASK;
    let ptr_val: usize = if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
        let p = (val_bits & POINTER_MASK) as usize;
        if p == 0 {
            return;
        }
        p
    } else {
        // Possible raw-I64 pointer. Reject anything with NaN-tag bits
        // (already handled above) or anything outside the 48-bit
        // user-address range. f64 numbers have the exponent bits set,
        // which puts them well above 0x0000_FFFF_FFFF_FFFF — they're
        // rejected here.
        if !(0x1000..=0x0000_FFFF_FFFF_FFFF).contains(&val_bits) {
            return;
        }
        val_bits as usize
    };

    // Range gate + hashset lookup. No enclosing_object fallback:
    // trace-phase field words always store user pointers at object
    // starts, not interior pointers (those only arise in conservative
    // stack scanning, which uses `try_mark_value_or_raw`).
    if !valid_ptrs.contains(&ptr_val) {
        return;
    }

    let header = header_from_user_ptr(ptr_val as *const u8);
    let flags = (*header).gc_flags;
    if flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) != 0 {
        return;
    }
    (*header).gc_flags = flags | GC_FLAG_MARKED;
    // Push directly onto the caller's worklist. No MARK_SEEDS push —
    // that's only needed for root-phase callers that don't own a
    // worklist (mark_global_roots, mark_registered_roots,
    // mark_remembered_set_roots, mark_stack_roots). The trace drain
    // already owns and consumes this worklist.
    worklist.push(header);
}

/// Get the bottom (highest address) of the current thread's stack.
#[cfg(target_os = "macos")]
fn get_stack_bottom() -> usize {
    extern "C" {
        fn pthread_self() -> *mut std::ffi::c_void;
        fn pthread_get_stackaddr_np(thread: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    }
    unsafe {
        let thread = pthread_self();
        pthread_get_stackaddr_np(thread) as usize
    }
}

#[cfg(target_os = "linux")]
fn get_stack_bottom() -> usize {
    extern "C" {
        fn pthread_self() -> usize;
        fn pthread_attr_init(attr: *mut [u64; 8]) -> i32;
        fn pthread_getattr_np(thread: usize, attr: *mut [u64; 8]) -> i32;
        fn pthread_attr_getstack(
            attr: *const [u64; 8],
            stackaddr: *mut *mut u8,
            stacksize: *mut usize,
        ) -> i32;
        fn pthread_attr_destroy(attr: *mut [u64; 8]) -> i32;
    }
    unsafe {
        let thread = pthread_self();
        let mut attr = [0u64; 8];
        pthread_attr_init(&mut attr);
        if pthread_getattr_np(thread, &mut attr) != 0 {
            return 0;
        }
        let mut stackaddr: *mut u8 = std::ptr::null_mut();
        let mut stacksize: usize = 0;
        pthread_attr_getstack(&attr, &mut stackaddr, &mut stacksize);
        pthread_attr_destroy(&mut attr);
        stackaddr as usize + stacksize
    }
}

// Windows: read TEB.StackBase. Works on every supported Windows version
// (Windows 7+) without needing GetCurrentThreadStackLimits (Win8+), so it
// stays correct on the `--min-windows-version=7` build path. The TEB lives
// at GS:[0] on x86_64 (FS:[0] on x86); StackBase sits at offset 0x08
// (the highest address — i.e. where the stack starts and grows down from).
// This is the same pointer kernel32!GetCurrentThreadStackLimits returns as
// `HighLimit`, just read directly from the TEB to avoid the kernel32 dep.
//
// Without this, conservative stack scan early-returns with stack_bottom=0,
// the GC sees no stack roots, and any heap pointer that lives only in a
// stack slot during a callback gets swept (issues #385/#386/#387 — the
// `Array.prototype.map` / `JSON.parse(...).property` / supported_features
// segfaults all traced back to here).
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn get_stack_bottom() -> usize {
    let stack_base: usize;
    unsafe {
        std::arch::asm!(
            "mov {out}, gs:[0x08]",
            out = out(reg) stack_base,
            options(nostack, preserves_flags, readonly),
        );
    }
    stack_base
}

#[cfg(all(target_os = "windows", target_arch = "x86"))]
fn get_stack_bottom() -> usize {
    let stack_base: usize;
    unsafe {
        std::arch::asm!(
            "mov {out}, fs:[0x04]",
            out = out(reg) stack_base,
            options(nostack, preserves_flags, readonly),
        );
    }
    stack_base
}

#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
fn get_stack_bottom() -> usize {
    // ARM64 Windows: TEB pointer is in x18; StackBase at offset 0x08.
    let stack_base: usize;
    unsafe {
        let teb: usize;
        std::arch::asm!("mov {}, x18", out(reg) teb, options(nostack, preserves_flags, readonly));
        stack_base = *((teb + 0x08) as *const usize);
    }
    stack_base
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "linux",
    all(
        target_os = "windows",
        any(target_arch = "x86_64", target_arch = "x86", target_arch = "aarch64")
    ),
)))]
fn get_stack_bottom() -> usize {
    0 // Stack scanning not supported on this OS/arch
}

/// Mark global roots (module-level variables registered by codegen).
fn mark_global_roots(valid_ptrs: &ValidPointerSet) {
    GLOBAL_ROOTS.with(|roots| {
        let roots = roots.borrow();
        for &root_ptr in roots.iter() {
            if !root_ptr.is_null() {
                let value = unsafe { *root_ptr };
                // First try NaN-boxed interpretation (exported globals, closures, etc.)
                if !try_mark_value(value, valid_ptrs) {
                    // Module variable globals store raw I64 pointers (not NaN-boxed).
                    // The codegen stores raw pointer values for is_pointer && !is_union types
                    // but the GC needs NaN-box tags to identify heap pointers.
                    // Try the raw value directly as a heap pointer address.
                    // This is safe: we validate against valid_ptrs (known heap allocations),
                    // and f64 number values have upper bits set (exponent) so they won't
                    // falsely match real heap addresses in the lower 48-bit address space.
                    let raw_ptr = value as usize;
                    if raw_ptr != 0 && valid_ptrs.contains(&raw_ptr) {
                        unsafe {
                            let header = header_from_user_ptr(raw_ptr as *const u8);
                            if (*header).gc_flags & GC_FLAG_MARKED == 0
                                && (*header).gc_flags & GC_FLAG_PINNED == 0
                            {
                                (*header).gc_flags |= GC_FLAG_MARKED;
                            }
                        }
                    }
                }
            }
        }
    });
}

/// Run registered root scanners (promise queues, timers, exception state).
fn mark_registered_roots(valid_ptrs: &ValidPointerSet) {
    // Collect scanners first to avoid borrow conflicts
    let scanners: Vec<fn(&mut dyn FnMut(f64))> = ROOT_SCANNERS.with(|s| s.borrow().clone());

    for scanner in scanners {
        scanner(&mut |value: f64| {
            try_mark_value(value.to_bits(), valid_ptrs);
        });
    }

    // Singleton closures (no-capture closures cached by `js_closure_alloc_singleton`)
    // are reachable from emitted code via cached pointers, not via JSValue refs the
    // tracer would otherwise find. Mark them explicitly so the sweep doesn't
    // reclaim a cached entry that's about to be loaded by the next call site.
    for closure_ptr in crate::closure::snapshot_singleton_closures() {
        let user = closure_ptr as usize;
        if user != 0 && valid_ptrs.contains(&user) {
            unsafe {
                let header = header_from_user_ptr(user as *const u8);
                if (*header).gc_flags & GC_FLAG_MARKED == 0
                    && (*header).gc_flags & GC_FLAG_PINNED == 0
                {
                    (*header).gc_flags |= GC_FLAG_MARKED;
                }
            }
        }
    }
}

/// Gen-GC Phase C3: mark the remembered set as roots. Old-gen
/// objects in the RS may hold pointers to young-gen objects that
/// would otherwise be missed by a minor GC. Today the full GC
/// also calls this so the RS stays coherent (nothing gets stuck
/// pointing at swept young objects), and so that the RS clear at
/// the end has clear "consumed" semantics. The actual minor-GC
/// time win lands in C3b when trace-from-RS short-circuits at
/// old-gen boundaries instead of recursing through them.
fn mark_remembered_set_roots(valid_ptrs: &ValidPointerSet) {
    // Snapshot the RS so we can iterate without holding the borrow
    // across `try_mark_value` (which may trigger user-code paths
    // that touch other RefCells).
    let snapshot: Vec<usize> = REMEMBERED_SET.with(|s| s.borrow().iter().copied().collect());
    for header_addr in snapshot {
        // Header sits at GcHeader; user pointer is +GC_HEADER_SIZE.
        // Mark the OLD-gen object itself live (Phase C3b will scan
        // its fields for young pointers without recursing into the
        // old-gen subtree). For this commit we use the existing
        // `try_mark_value` machinery which traces transitively —
        // correct but not yet generationally optimal.
        let user_ptr = header_addr + GC_HEADER_SIZE;
        if !valid_ptrs.contains(&user_ptr) {
            continue;
        }
        // Treat as a NaN-boxed POINTER value; try_mark_value
        // dispatches through the standard mark + worklist path.
        let nanbox = POINTER_TAG | (user_ptr as u64);
        try_mark_value(nanbox, valid_ptrs);
    }
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
fn drain_trace_worklist(worklist: &mut Vec<*mut GcHeader>, valid_ptrs: &ValidPointerSet) {
    drain_trace_worklist_inner(worklist, valid_ptrs, false);
}

fn drain_trace_worklist_minor(worklist: &mut Vec<*mut GcHeader>, valid_ptrs: &ValidPointerSet) {
    drain_trace_worklist_inner(worklist, valid_ptrs, true);
}

fn drain_trace_worklist_inner(
    worklist: &mut Vec<*mut GcHeader>,
    valid_ptrs: &ValidPointerSet,
    minor_only: bool,
) {
    let mut i = 0;
    while i < worklist.len() {
        let header = worklist[i];
        i += 1;

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
                // objects (`PERRY_GEN_GC_EVACUATE` is opt-in default
                // OFF) still hold pointers to young-gen children, and
                // skipping their fields without a write barrier on
                // every store leaves those children unmarked. ECS
                // demo-simple regressed when an archetype that had
                // survived to TENURED still held a `componentData` Map
                // whose value arrays were young-gen — minor GC skipped
                // tracing the archetype, the value arrays got swept,
                // and `pipeline` forEach iterated zero entities.
                // `pointer_in_old_gen` excludes tenured-in-nursery
                // exactly, so the AND form is the correct gate.
                let is_old_arena = crate::arena::pointer_in_old_gen(user_ptr as usize);
                let is_tenured = (*header).gc_flags & GC_FLAG_TENURED != 0;
                if is_tenured && is_old_arena {
                    continue;
                }
            }
            match (*header).obj_type {
                GC_TYPE_ARRAY => trace_array(user_ptr, valid_ptrs, worklist),
                GC_TYPE_OBJECT => trace_object(user_ptr, valid_ptrs, worklist),
                GC_TYPE_CLOSURE => trace_closure(user_ptr, valid_ptrs, worklist),
                GC_TYPE_PROMISE => trace_promise(user_ptr, valid_ptrs, worklist),
                GC_TYPE_ERROR => trace_error(user_ptr, valid_ptrs, worklist),
                GC_TYPE_MAP => trace_map(user_ptr, valid_ptrs, worklist),
                GC_TYPE_LAZY_ARRAY => trace_lazy_array(user_ptr, valid_ptrs, worklist),
                GC_TYPE_STRING | GC_TYPE_BIGINT => {}
                _ => {}
            }
        }
    }
}

/// Trace from marked objects: follow references iteratively using a worklist.
fn trace_marked_objects(valid_ptrs: &ValidPointerSet) {
    // Same MARK_SEEDS-based approach as the minor variant — root scans
    // populated `MARK_SEEDS` via `try_mark_value`, no need to walk arena
    // here just to gather them.
    let mut worklist = take_mark_seeds();
    drain_trace_worklist(&mut worklist, valid_ptrs);
}

/// Gen-GC Phase C3b minor variant of `trace_marked_objects`.
/// Drains the per-cycle MARK_SEEDS worklist that root-marking
/// populated via `try_mark_value` / `try_mark_value_or_raw` —
/// recursion into old-gen objects is skipped. The seeds list
/// avoids the full arena walk that the previous implementation
/// did just to find currently-marked headers; with ~1.6M objects
/// per cycle in perf-comprehensive that walk dominated minor-GC
/// time and produced output containing only the small number of
/// objects the root scan actually touched.
fn trace_marked_objects_minor(valid_ptrs: &ValidPointerSet) {
    let mut worklist = take_mark_seeds();
    drain_trace_worklist_minor(&mut worklist, valid_ptrs);
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
const BLOCK_PERSIST_WINDOW: usize = 5;

fn mark_block_persisting_arena_objects(valid_ptrs: &ValidPointerSet) {
    let mut worklist: Vec<*mut GcHeader> = Vec::new();
    loop {
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

        if newly_marked == 0 {
            break;
        }

        // Trace newly marked; may mark children in previously-dead blocks,
        // requiring another round to pick them up (but only within the
        // recent window — old blocks' newly-traced marks don't re-enter
        // the block-persist pump).
        drain_trace_worklist(&mut worklist, valid_ptrs);
    }
}

/// Trace Map entries — scan all key-value pairs in the Map's entries array.
/// Maps store NaN-boxed JSValues (strings, arrays, objects) as keys and values.
/// Values may also be raw I64 pointers (for typed arrays/maps stored in maps).
unsafe fn trace_map(
    user_ptr: *mut u8,
    valid_ptrs: &ValidPointerSet,
    worklist: &mut Vec<*mut GcHeader>,
) {
    let map = user_ptr as *const crate::map::MapHeader;
    let size = (*map).size;
    let capacity = (*map).capacity;

    // Sanity check
    if size > capacity || size > 100_000 {
        return;
    }

    let entries = (*map).entries as *const u64;
    if entries.is_null() {
        return;
    }

    // Each entry is 2 x f64 (key + value). Specialized field walker
    // for both — see `mark_field_into_worklist`.
    for i in 0..(size as usize) {
        let key_bits = *entries.add(i * 2);
        let val_bits = *entries.add(i * 2 + 1);
        mark_field_into_worklist(key_bits, valid_ptrs, worklist);
        mark_field_into_worklist(val_bits, valid_ptrs, worklist);
    }
}

/// Extract a raw pointer value from NaN-boxed or raw bits.
///
/// Previously called from `trace_map`; now subsumed by
/// `mark_field_into_worklist` which folds the extraction and the
/// mark-and-enqueue dance into one inlined step. Kept for any
/// external callers / future use.
#[allow(dead_code)]
fn extract_ptr_from_bits(bits: u64) -> usize {
    let tag = bits & TAG_MASK;
    match tag {
        t if t == POINTER_TAG || t == STRING_TAG || t == BIGINT_TAG => {
            (bits & POINTER_MASK) as usize
        }
        _ => {
            // Raw pointer (no NaN-boxing tag)
            if (0x1000..=0x0000_FFFF_FFFF_FFFF).contains(&bits) {
                bits as usize
            } else {
                0
            }
        }
    }
}

/// Trace array elements.
/// Elements may be NaN-boxed JSValues OR raw I64 pointers (codegen stores raw I64 for
/// is_pointer/is_array/is_string typed arrays via js_array_set_jsvalue).
unsafe fn trace_array(
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

    let arr = user_ptr as *const crate::array::ArrayHeader;
    let length = (*arr).length;
    let capacity = (*arr).capacity;

    // Sanity check: reject corrupt length/capacity to avoid scanning wild memory.
    // The 16M cap is a garbage-recognition guard (no realistic array exceeds it);
    // real programs routinely push >65k items into arrays (issue #44 repro hits 100k).
    if length > capacity || length > 16_000_000 {
        return;
    }

    let elements =
        (user_ptr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const u64;

    // Specialized field walker — see `mark_field_into_worklist`.
    for i in 0..length as usize {
        let val_bits = *elements.add(i);
        mark_field_into_worklist(val_bits, valid_ptrs, worklist);
    }
}

/// Trace object fields and keys array.
/// Fields may be NaN-boxed JSValues OR raw I64 pointers (codegen stores some fields as raw I64).
/// keys_array may be a raw pointer (*mut ArrayHeader) OR NaN-boxed (codegen may NaN-box it).
unsafe fn trace_object(
    user_ptr: *mut u8,
    valid_ptrs: &ValidPointerSet,
    worklist: &mut Vec<*mut GcHeader>,
) {
    let obj = user_ptr as *const crate::object::ObjectHeader;
    let field_count = (*obj).field_count;

    // Sanity check: reject corrupt field_count to avoid scanning wild memory.
    // 1M is a garbage-recognition guard — legitimate objects never have that many fields.
    if field_count > 1_000_000 {
        return;
    }

    let fields = (user_ptr as *const u8).add(std::mem::size_of::<crate::object::ObjectHeader>())
        as *const u64;

    // Trace each field with the specialized field walker (handles both
    // NaN-boxed JSValues and raw I64 pointers — codegen stores some
    // fields as raw I64 for is_pointer typed variables). See
    // `mark_field_into_worklist` for why this beats `try_mark_value_or_raw`.
    for i in 0..field_count as usize {
        let val_bits = *fields.add(i);
        mark_field_into_worklist(val_bits, valid_ptrs, worklist);
    }

    // Trace keys_array pointer.
    // The codegen may store keys_array as either a raw pointer or a NaN-boxed POINTER_TAG value.
    // Read the raw 64-bit value and handle both cases.
    let keys_raw = (*obj).keys_array as u64;
    if keys_raw != 0 {
        // Extract the actual pointer: strip NaN-boxing tags if present
        let keys_ptr = if keys_raw >> 48 >= 0x7FF8 {
            // NaN-boxed: extract lower 48 bits as pointer
            (keys_raw & POINTER_MASK) as usize
        } else {
            keys_raw as usize
        };
        if keys_ptr != 0 && keys_ptr >= 0x1000 && valid_ptrs.contains(&keys_ptr) {
            let keys_header = header_from_user_ptr(keys_ptr as *const u8);
            if (*keys_header).gc_flags & GC_FLAG_MARKED == 0 {
                (*keys_header).gc_flags |= GC_FLAG_MARKED;
                worklist.push(keys_header);
            }
        }
    }
}

/// Trace a lazy array (Issue #179 Phase 2). The tape bytes live
/// inline in the same arena allocation, so they're reclaimed with
/// the header. We only need to keep two satellite references alive:
///
/// 1. `blob_str` — the input `StringHeader`. Without this the blob
///    data pointer the tape references would dangle after the first
///    post-parse GC cycle. The intern table / other caches may or
///    may not keep it alive; tracing is authoritative.
/// 2. `materialized` — the `ArrayHeader`-backed tree once forced.
///    Null until first non-`.length` access.
unsafe fn trace_lazy_array(
    user_ptr: *mut u8,
    valid_ptrs: &ValidPointerSet,
    worklist: &mut Vec<*mut GcHeader>,
) {
    let lazy = user_ptr as *const crate::json_tape::LazyArrayHeader;
    // Defensive magic check — if somehow mis-tagged, bail.
    if (*lazy).magic != crate::json_tape::LAZY_ARRAY_MAGIC {
        return;
    }

    let blob_ptr = (*lazy).blob_str as usize;
    if blob_ptr != 0 && valid_ptrs.contains(&blob_ptr) {
        let hdr = header_from_user_ptr(blob_ptr as *const u8);
        if (*hdr).gc_flags & GC_FLAG_MARKED == 0 {
            (*hdr).gc_flags |= GC_FLAG_MARKED;
            worklist.push(hdr);
        }
    }

    let mat_ptr = (*lazy).materialized as usize;
    if mat_ptr != 0 && valid_ptrs.contains(&mat_ptr) {
        let hdr = header_from_user_ptr(mat_ptr as *const u8);
        if (*hdr).gc_flags & GC_FLAG_MARKED == 0 {
            (*hdr).gc_flags |= GC_FLAG_MARKED;
            worklist.push(hdr);
        }
    }

    // Phase 5: sparse per-element cache. Both the cache buffer and
    // the bitmap are separate arena allocations that must be marked
    // to survive sweep. The cache's live JSValues (only those with
    // their bitmap bit set) must in turn be traced — their pointees
    // are the real backing objects for `parsed[i]` and must stay
    // alive across GC so identity holds.
    let cache_ptr = (*lazy).materialized_elements as usize;
    if cache_ptr != 0 && valid_ptrs.contains(&cache_ptr) {
        let hdr = header_from_user_ptr(cache_ptr as *const u8);
        if (*hdr).gc_flags & GC_FLAG_MARKED == 0 {
            (*hdr).gc_flags |= GC_FLAG_MARKED;
            // No need to push onto worklist — GC_TYPE_STRING is a
            // leaf, no children to trace through the buffer itself.
        }
    }
    let bitmap_ptr = (*lazy).materialized_bitmap as usize;
    if bitmap_ptr != 0 && valid_ptrs.contains(&bitmap_ptr) {
        let hdr = header_from_user_ptr(bitmap_ptr as *const u8);
        if (*hdr).gc_flags & GC_FLAG_MARKED == 0 {
            (*hdr).gc_flags |= GC_FLAG_MARKED;
        }
    }
    // Walk the cache and trace each set slot's JSValue. Unset slots
    // hold zero bits (positive zero number) which try_mark_value
    // correctly ignores as a non-pointer; safe to walk either way,
    // but checking the bitmap first avoids redundant work.
    let cached_length = (*lazy).cached_length as usize;
    if cache_ptr != 0 && bitmap_ptr != 0 && cached_length > 0 {
        let cache = (*lazy).materialized_elements;
        let bitmap = (*lazy).materialized_bitmap;
        let bitmap_words = cached_length.div_ceil(64);
        for w in 0..bitmap_words {
            let word = *bitmap.add(w);
            if word == 0 {
                continue;
            }
            let base_idx = w * 64;
            for b in 0..64usize {
                if word & (1u64 << b) == 0 {
                    continue;
                }
                let i = base_idx + b;
                if i >= cached_length {
                    break;
                }
                let val_bits = (*cache.add(i)).bits();
                if try_mark_value(val_bits, valid_ptrs) {
                    let tag = val_bits & TAG_MASK;
                    let ptr_val = if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
                        (val_bits & POINTER_MASK) as usize
                    } else {
                        val_bits as usize
                    };
                    if ptr_val != 0 && valid_ptrs.contains(&ptr_val) {
                        let header = header_from_user_ptr(ptr_val as *const u8);
                        worklist.push(header);
                    }
                }
            }
        }
    }
}

/// Trace closure captures
/// Captures may be NaN-boxed JSValues OR raw I64 pointers bitcast to F64.
/// Perry's codegen stores `is_string`/`is_array`/`is_closure` captures as raw I64 in some paths.
unsafe fn trace_closure(
    user_ptr: *mut u8,
    valid_ptrs: &ValidPointerSet,
    worklist: &mut Vec<*mut GcHeader>,
) {
    let closure = user_ptr as *const crate::closure::ClosureHeader;
    let capture_count = crate::closure::real_capture_count((*closure).capture_count);
    let captures = (user_ptr as *const u8).add(std::mem::size_of::<crate::closure::ClosureHeader>())
        as *const u64;

    // Specialized field walker: skips MARK_SEEDS push (caller owns
    // `worklist`) and the interior-pointer fallback (closure capture
    // slots only hold user-pointer object starts). See
    // `mark_field_into_worklist` for why this beats the generic
    // `try_mark_value_or_raw` path on this hot loop.
    for i in 0..capture_count as usize {
        let val_bits = *captures.add(i);
        mark_field_into_worklist(val_bits, valid_ptrs, worklist);
    }
}

/// Trace promise fields
unsafe fn trace_promise(
    user_ptr: *mut u8,
    valid_ptrs: &ValidPointerSet,
    worklist: &mut Vec<*mut GcHeader>,
) {
    let promise = user_ptr as *const crate::promise::Promise;

    // Trace value and reason — may be NaN-boxed JSValues or raw I64 pointers.
    // Specialized field walker — see `mark_field_into_worklist`.
    for &val_bits in &[(*promise).value.to_bits(), (*promise).reason.to_bits()] {
        mark_field_into_worklist(val_bits, valid_ptrs, worklist);
    }

    // Trace on_fulfilled and on_rejected (closure pointers)
    let on_fulfilled = (*promise).on_fulfilled;
    if !on_fulfilled.is_null() {
        let ptr_usize = on_fulfilled as usize;
        if valid_ptrs.contains(&ptr_usize) {
            let header = header_from_user_ptr(on_fulfilled as *const u8);
            if (*header).gc_flags & GC_FLAG_MARKED == 0 {
                (*header).gc_flags |= GC_FLAG_MARKED;
                worklist.push(header);
            }
        }
    }

    let on_rejected = (*promise).on_rejected;
    if !on_rejected.is_null() {
        let ptr_usize = on_rejected as usize;
        if valid_ptrs.contains(&ptr_usize) {
            let header = header_from_user_ptr(on_rejected as *const u8);
            if (*header).gc_flags & GC_FLAG_MARKED == 0 {
                (*header).gc_flags |= GC_FLAG_MARKED;
                worklist.push(header);
            }
        }
    }

    // Trace next promise in chain
    let next = (*promise).next;
    if !next.is_null() {
        let next_usize = next as usize;
        if valid_ptrs.contains(&next_usize) {
            let header = header_from_user_ptr(next as *const u8);
            if (*header).gc_flags & GC_FLAG_MARKED == 0 {
                (*header).gc_flags |= GC_FLAG_MARKED;
                worklist.push(header);
            }
        }
    }
}

/// Trace error fields (message, name, stack are StringHeader pointers; cause is f64; errors is array)
unsafe fn trace_error(
    user_ptr: *mut u8,
    valid_ptrs: &ValidPointerSet,
    worklist: &mut Vec<*mut GcHeader>,
) {
    let error = user_ptr as *const crate::error::ErrorHeader;

    for &str_ptr in &[(*error).message, (*error).name, (*error).stack] {
        if !str_ptr.is_null() {
            let ptr_usize = str_ptr as usize;
            if valid_ptrs.contains(&ptr_usize) {
                let header = header_from_user_ptr(str_ptr as *const u8);
                if (*header).gc_flags & GC_FLAG_MARKED == 0 {
                    (*header).gc_flags |= GC_FLAG_MARKED;
                    worklist.push(header);
                }
            }
        }
    }

    // Trace `cause` if it's a NaN-boxed pointer-like value
    let cause_bits = (*error).cause.to_bits();
    let top16 = (cause_bits >> 48) as u16;
    // POINTER_TAG=0x7FFD, STRING_TAG=0x7FFF, BIGINT_TAG=0x7FFA
    if top16 == 0x7FFD || top16 == 0x7FFF || top16 == 0x7FFA {
        let cause_ptr = (cause_bits & 0x0000_FFFF_FFFF_FFFF) as *const u8;
        if !cause_ptr.is_null() {
            let ptr_usize = cause_ptr as usize;
            if valid_ptrs.contains(&ptr_usize) {
                let header = header_from_user_ptr(cause_ptr);
                if (*header).gc_flags & GC_FLAG_MARKED == 0 {
                    (*header).gc_flags |= GC_FLAG_MARKED;
                    worklist.push(header);
                }
            }
        }
    }

    // Trace `errors` array
    let errors_ptr = (*error).errors;
    if !errors_ptr.is_null() {
        let ptr_usize = errors_ptr as usize;
        if valid_ptrs.contains(&ptr_usize) {
            let header = header_from_user_ptr(errors_ptr as *const u8);
            if (*header).gc_flags & GC_FLAG_MARKED == 0 {
                (*header).gc_flags |= GC_FLAG_MARKED;
                worklist.push(header);
            }
        }
    }
}

/// Sweep: free unmarked malloc objects; add unmarked arena objects to free list.
/// Returns total bytes freed.
fn sweep() -> u64 {
    sweep_with_age_bump(false)
}

/// Sweep variant that folds the minor-GC age-bump pass into the same arena walk.
///
/// `gc_collect_minor` previously did:
///   1. arena_walk_objects to update HAS_SURVIVED/TENURED on marked young objects
///   2. arena_walk_objects_with_block_index in `sweep` to free dead objects and
///      compute block_has_live
///
/// Both walks visit every arena object header. With ~1.6M objects per cycle in
/// perf-comprehensive, removing the dedicated age-bump walk saves ~10ms/cycle
/// and avoids touching every header twice. The age-bump update is folded into
/// the sweep walk's "alive" branches, gated on `block_idx < general_n` so only
/// general-arena (nursery) objects age — longlived and old-gen are skipped, as
/// in the original standalone age-bump pass (which used `pointer_in_old_gen`
/// for the same gate).
fn sweep_with_age_bump(do_age_bump: bool) -> u64 {
    let mut freed_bytes: u64 = 0;

    // Sweep malloc objects.
    //
    // Lazy-set: instead of `set.remove(...)` per freed object (50 ns ×
    // tens of thousands of dead entries = several ms per GC cycle), we
    // flip `set_dirty = true` once and the next `gc_realloc` rebuilds
    // the set from the updated `objects` Vec. Most kernels never call
    // `gc_realloc` between collections, so the rebuild is amortized
    // away entirely. The set IS kept in sync inline for callers that
    // need consistency between sweep and a later mid-cycle realloc —
    // but the common path is just the bool flip.
    MALLOC_STATE.with(|s| {
        let mut s = s.borrow_mut();
        let MallocState { objects, .. } = &mut *s;
        let mut i = 0;
        let mut any_freed = false;
        while i < objects.len() {
            let header = objects[i];
            unsafe {
                if (*header).gc_flags & GC_FLAG_PINNED != 0 {
                    // Pinned objects are always kept alive — clear mark bit inline
                    (*header).gc_flags &= !GC_FLAG_MARKED;
                    i += 1;
                    continue;
                }
                if (*header).gc_flags & GC_FLAG_MARKED == 0 {
                    // Unmarked: free it
                    let total_size = (*header).size as usize;
                    freed_bytes += total_size as u64;

                    // For Maps, also free the separately-allocated entries array
                    // and drop the lookup side-table entry so the next allocation
                    // at this GC slot doesn't inherit stale key→index pairs.
                    if (*header).obj_type == GC_TYPE_MAP {
                        let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
                        let map = user_ptr as *const crate::map::MapHeader;
                        let entries = (*map).entries;
                        if !entries.is_null() {
                            let cap = (*map).capacity as usize;
                            if cap > 0 {
                                let ent_size = (cap * 16).max(8); // ENTRY_SIZE = 16
                                let ent_layout = Layout::from_size_align(ent_size, 8).unwrap();
                                dealloc(entries as *mut u8, ent_layout);
                            }
                        }
                        crate::map::drop_map_index(user_ptr as usize);
                    }
                    if (*header).obj_type == GC_TYPE_PROMISE {
                        let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
                        let promise = user_ptr as *mut crate::promise::Promise;
                        crate::async_hooks::enqueue_gc_destroy((*promise).async_id);
                        crate::promise::clear_promise_context_for_gc(promise);
                    }

                    let layout = Layout::from_size_align(total_size, 8).unwrap();
                    dealloc(header as *mut u8, layout);
                    objects.swap_remove(i);
                    any_freed = true;
                    // Don't increment i — swap_remove moved last element here
                } else {
                    // Surviving object — clear mark bit inline to avoid separate heap walk
                    (*header).gc_flags &= !GC_FLAG_MARKED;
                    i += 1;
                }
            }
        }
        if any_freed {
            s.set_dirty = true;
        }
    });

    // Sweep arena objects. Two-phase strategy:
    //
    //   1. Fast probe pass: walk objects, clear mark bits, count
    //      dead bytes, track whether ANY block has a live object.
    //      If no live anywhere → entire arena is reclaimable. Skip
    //      every per-block tracking structure and reset all blocks
    //      to offset=0 in O(1). This is the common case for tight
    //      `new ClassName()` loops where nothing escapes.
    //
    //   2. Slow tracking pass (only when some block has live objects):
    //      walk again, this time bucketing dead objects per block so
    //      we can decide which blocks are fully empty (reset) vs
    //      partially empty (push their dead objects to the free list
    //      in a single batched extend).
    //
    // The two-pass split avoids the per-object HashMap insert cost
    // (~50ns) on the common all-dead path, where it would account for
    // 700k × 50ns = 35ms per GC cycle.
    // Sweep arena objects with per-block live tracking.
    //
    // For each object, walk and check mark/pinned state:
    //   - live → set `block_has_live[block_idx]` and clear the mark
    //     bit inline so we don't need a separate pass.
    //   - dead → zero its payload memory (so stale pointers don't
    //     retain other objects on the next GC cycle).
    //
    // We deliberately do NOT push dead objects onto the global
    // ARENA_FREE_LIST. The inline bump allocator never reads the
    // free list — it uses the per-block reset instead. Pushing
    // dead objects to the free list would cost ~50ns per object
    // × ~700k objects per GC × ~12 GC cycles per benchmark = 420ms
    // of pure waste in `object_create`. The function-call allocator
    // path (`js_object_alloc_class_inline_keys` → `arena_alloc_gc`)
    // is the only consumer of the free list, and it's only used
    // for shapes the inline path doesn't cover (anonymous classes,
    // closure body new'd from a slot, etc.) — those are rare enough
    // that running them through the slow path is fine.
    //
    // After the walk, `arena_reset_empty_blocks` resets every block
    // with zero live objects to offset=0. This is the load-bearing
    // optimization that lets the inline bump allocator reuse memory
    // across GC cycles instead of page-faulting through fresh blocks.
    let n_blocks = crate::arena::arena_block_count();
    let mut block_has_live: Vec<bool> = vec![false; n_blocks];
    // Inclusive upper bound on indices that age. `general_block_count()`
    // is the first non-general index; objects with `block_idx < general_n`
    // are nursery-resident and need the age-bump update.
    let general_n = if do_age_bump {
        crate::arena::general_block_count()
    } else {
        0
    };

    // Hoist the OVERFLOW_FIELDS empty check out of the per-dead-object
    // loop. perf-comprehensive's sweep walks ~1.6 M dead arena headers
    // per cycle and most workloads never write past the 8 inline object
    // slots, so OVERFLOW_FIELDS stays empty for the whole run. The
    // hoisted bool turns 1.6 M `clear_overflow_for_ptr` calls (each one
    // a TLS-load + RefCell borrow + HashMap remove on a missing key)
    // into a single bool test per object. ~1.4 % leaf samples → 0 on
    // the empty-map path, ~80 ms saved on perf-comprehensive.
    let overflow_active = !crate::object::overflow_fields_is_empty();

    crate::arena::arena_walk_objects_with_block_index(|header_ptr, block_idx| {
        let header = header_ptr as *mut GcHeader;
        unsafe {
            // Age-bump for surviving general-arena (nursery) objects, folded
            // into this walk so the standalone `arena_walk_objects` pass in
            // gc_collect_minor can be eliminated. Mirrors the original
            // age-bump's gate (skip old-gen, skip already-tenured, skip
            // unmarked-and-unpinned) and runs BEFORE the mark bit is
            // cleared so the MARKED check stays meaningful.
            let age_bump_this = do_age_bump && block_idx < general_n;
            let flags = (*header).gc_flags;
            // Fast path: `flags == 0` means the object is dead (MARKED=0)
            // AND has no special bits (PINNED/FORWARDED/HAS_SURVIVED/
            // TENURED). Fresh allocations from the current cycle that
            // never got marked land here — in perf-comprehensive's hot
            // forEach / commandBuffer loops that's the dominant case.
            // Skipping the four flag-bit branches and the age-bump
            // bookkeeping for this common case shaves a measurable amount
            // off the 1.6 M-object-per-cycle sweep walk.
            if flags == 0 {
                let total_size = (*header).size as usize;
                freed_bytes += total_size as u64;
                if overflow_active && (*header).obj_type == GC_TYPE_OBJECT {
                    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
                    crate::object::clear_overflow_for_ptr(user_ptr as usize);
                }
                return;
            }
            if flags & GC_FLAG_PINNED != 0 {
                if block_idx < block_has_live.len() {
                    block_has_live[block_idx] = true;
                }
                if age_bump_this && flags & GC_FLAG_TENURED == 0 {
                    if flags & GC_FLAG_HAS_SURVIVED != 0 {
                        (*header).gc_flags =
                            (flags | GC_FLAG_TENURED) & !GC_FLAG_HAS_SURVIVED & !GC_FLAG_MARKED;
                    } else {
                        (*header).gc_flags = (flags | GC_FLAG_HAS_SURVIVED) & !GC_FLAG_MARKED;
                    }
                } else {
                    (*header).gc_flags = flags & !GC_FLAG_MARKED;
                }
                return;
            }
            // FORWARDED objects must keep their containing block alive.
            // `trace_array` short-circuits on FORWARDED (it pushes the
            // forwarding TARGET onto the worklist instead of marking the
            // OLD itself), so OLD reaches sweep as MARKED == 0 even
            // though its first 8 bytes hold a load-bearing forwarding
            // pointer. If OLD's block ends up with zero MARKED objects,
            // `arena_reset_empty_blocks` wipes it to offset=0, the
            // forwarding chain breaks, and `clean_arr_ptr` on any stale
            // OLD reference returns null. ECS demo-simple's `pipeline`
            // forEach hits this when `archetypesByComponent`'s value
            // array was reached via a forwarded chain — the next query
            // call's Map.get pointed at wiped memory and forEach
            // iterated zero entities. Treat FORWARDED as live for the
            // block-keep gate; the OLD payload is just an 8-byte
            // forwarding pointer, harmless to retain.
            if flags & GC_FLAG_FORWARDED != 0 {
                if block_idx < block_has_live.len() {
                    block_has_live[block_idx] = true;
                }
                (*header).gc_flags = flags & !GC_FLAG_MARKED;
                return;
            }
            if flags & GC_FLAG_MARKED == 0 {
                let total_size = (*header).size as usize;
                let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
                freed_bytes += total_size as u64;

                if overflow_active && (*header).obj_type == GC_TYPE_OBJECT {
                    crate::object::clear_overflow_for_ptr(user_ptr as usize);
                }

                // Note: We deliberately do NOT zero the dead object's
                // payload here. trace_object/trace_array/trace_closure
                // walk objects PRECISELY (only `field_count` /
                // `length` / `capture_count` slots), so unused slots
                // and dead-object payloads are never scanned by the
                // mark phase. The conservative stack scan only walks
                // the C stack, not arbitrary heap memory. So stale
                // pointer-looking bytes inside dead-object payloads
                // can never trigger a false positive — and zeroing
                // them was costing ~2-3ms per `object_create` GC for
                // memory bandwidth (700k × 88 bytes = 62MB written).
            } else {
                if block_idx < block_has_live.len() {
                    block_has_live[block_idx] = true;
                }
                if age_bump_this && flags & GC_FLAG_TENURED == 0 {
                    if flags & GC_FLAG_HAS_SURVIVED != 0 {
                        (*header).gc_flags =
                            (flags | GC_FLAG_TENURED) & !GC_FLAG_HAS_SURVIVED & !GC_FLAG_MARKED;
                    } else {
                        (*header).gc_flags = (flags | GC_FLAG_HAS_SURVIVED) & !GC_FLAG_MARKED;
                    }
                } else {
                    (*header).gc_flags = flags & !GC_FLAG_MARKED;
                }
            }
        }
    });

    // Reset every block that ended up with zero live objects.
    // Diagnostic: PERRY_GC_DIAG=1 reports block-level liveness.
    if std::env::var_os("PERRY_GC_DIAG").is_some() {
        let general_n = crate::arena::general_block_count();
        let live_general = (0..general_n).filter(|&i| block_has_live[i]).count();
        let live_ll = (general_n..n_blocks).filter(|&i| block_has_live[i]).count();
        eprintln!(
            "[gc] blocks: general={} ({} live), longlived={} ({} live), freed_bytes={}",
            general_n,
            live_general,
            n_blocks - general_n,
            live_ll,
            freed_bytes
        );
    }
    crate::arena::arena_reset_empty_blocks(&block_has_live);

    freed_bytes
}

/// Clear mark bits on all surviving objects
#[cfg(test)]
fn clear_marks() {
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

/// Root scanner for promise task queue and scheduled resolves
pub fn promise_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::promise::scan_promise_roots(mark);
}

/// Root scanner for timer callbacks
pub fn timer_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::timer::scan_timer_roots(mark);
}

/// Root scanner for current exception
pub fn exception_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::exception::scan_exception_roots(mark);
}

/// Root scanner for active AsyncLocalStorage context.
pub fn async_context_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::async_context::scan_active_context_roots(mark);
    crate::builtins::scan_queued_microtask_roots(mark);
}

/// Root scanner for async_hooks hook callbacks and user resource references.
pub fn async_hooks_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::async_hooks::scan_async_hooks_roots(mark);
}

/// Root scanner for object shape cache (keys arrays shared across objects with same shape)
pub fn shape_cache_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::object::scan_shape_cache_roots(mark);
}

/// Root scanner for the shape-transition cache used by the dynamic-key
/// write path (`obj[name] = value`). Same role as `shape_cache_root_scanner`
/// — without it, GC would free cached target keys_arrays that no live
/// object currently references directly.
pub fn transition_cache_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::object::scan_transition_cache_roots(mark);
}

/// Root scanner for OVERFLOW_FIELDS (per-object extra properties beyond inline slots)
pub fn overflow_fields_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::object::scan_overflow_fields_roots(mark);
}

/// Root scanner for in-progress JSON.parse frames (issue #46).
/// Without this, GC triggered mid-parse would sweep in-progress arrays/objects
/// and the fresh string/object values about to be pushed into them.
pub fn json_parse_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::json::scan_parse_roots(mark);
}

// ---------------------------------------------------------------------------
// Phase C — write barrier + remembered set
// (docs/generational-gc-plan.md §Phase C)
// ---------------------------------------------------------------------------
//
// Generational GC needs to know which old-gen objects hold
// references to young-gen objects, so a minor GC can scan just
// those (the "remembered set") instead of the entire old-gen.
//
// The write barrier fires on every heap store. Semantics:
//   if parent is OLD and child points to YOUNG, add parent to
//   the remembered set.
//
// Sub-phase C1 (this commit): runtime infrastructure — barrier
// function + remembered set storage + unit tests. No codegen
// emission yet (sub-phase C2). No minor GC consuming the set
// yet (sub-phase C3).
//
// Bounded false-positive policy: the remembered set is a HashSet
// of GcHeader pointers (NOT card-marks). False positives are
// safe (extra scan during minor GC, no correctness impact); false
// negatives would skip a live young-gen object and break
// correctness. The current implementation uses HashSet<usize>
// because pointer-tagged f64 NaN-boxing means the same heap
// pointer can appear via different tag bytes; storing the
// canonicalized GcHeader address dedups across tag variants.

thread_local! {
    /// Set of OLD-gen GcHeader addresses that have been written to
    /// with a YOUNG-gen pointer since the last minor GC. Cleared
    /// by minor GC after the remembered-set scan (Phase C3).
    pub(crate) static REMEMBERED_SET: std::cell::RefCell<std::collections::HashSet<usize>> =
        std::cell::RefCell::new(std::collections::HashSet::new());

    /// Gen-GC Phase C4b: set of GcHeader addresses pinned this
    /// collection cycle because they may be referenced by the
    /// conservative C-stack scan. Conservative scan finds candidate
    /// pointers by bit-pattern matching memory words; we cannot
    /// safely rewrite those words after evacuation because they
    /// might not actually be pointers (false positives). Therefore
    /// any object discovered conservatively is excluded from the
    /// evacuation candidate set.
    ///
    /// Populated by `pin_currently_marked_as_conservative` after
    /// `mark_stack_roots` runs in `gc_collect_minor`. Cleared at
    /// the end of every collection so the next cycle starts fresh.
    pub(crate) static CONS_PINNED: std::cell::RefCell<std::collections::HashSet<usize>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
}

/// Gen-GC Phase C4b: walk the current arena+malloc marked set and
/// record every header address as conservatively pinned. Called
/// AFTER `mark_stack_roots` (the conservative scan) and BEFORE
/// `mark_global_roots` / `mark_registered_roots` / RS scan — so
/// only the conservative-scan results are captured. Subsequently-
/// marked objects (precise sources) get marked but stay out of
/// `CONS_PINNED`, making them eligible for evacuation if they
/// also tenure.
///
/// Called only from the minor-GC path. The full GC path
/// (`gc_collect_inner`) doesn't evacuate so doesn't need pinning.
fn pin_currently_marked_as_conservative() {
    CONS_PINNED.with(|s| {
        let mut pinned = s.borrow_mut();
        crate::arena::arena_walk_objects(|header_ptr| {
            let header = header_ptr as *mut GcHeader;
            unsafe {
                if (*header).gc_flags & GC_FLAG_MARKED != 0 {
                    pinned.insert(header as usize);
                }
            }
        });
        MALLOC_STATE.with(|m| {
            let m = m.borrow();
            for &header in m.objects.iter() {
                unsafe {
                    if (*header).gc_flags & GC_FLAG_MARKED != 0 {
                        pinned.insert(header as usize);
                    }
                }
            }
        });
    });
}

/// Gen-GC Phase C4b-β: walk arena nursery objects and copy
/// non-pinned tenured ones into OLD_ARENA. Install a forwarding
/// pointer at the original nursery slot's user-payload start.
/// Returns the count of evacuated objects (diagnostic only).
///
/// Candidate filter: the object must be
/// - in the nursery arena (not OLD, not LONGLIVED)
/// - MARKED (alive this cycle)
/// - TENURED (survived ≥2 minor GCs)
/// - NOT in CONS_PINNED (no conservative root reaches it)
/// - NOT already FORWARDED (idempotent; duplicate evacuation is
///   safe-skipped)
///
/// Phase C4b-γ-2: this function is paired with
/// `rewrite_forwarded_references` — every reference site (heap
/// fields, shadow stack, global roots) is rewalked AFTER this
/// function returns and any pointer to a forwarded object is
/// updated to the new address. The original's MARKED bit is
/// cleared at evac time so sweep treats the now-stale slot as
/// dead and the nursery block can reset; the new copy is marked
/// MARKED so the rewrite walk picks up its (copied) fields and so
/// sweep keeps it alive.
fn evacuate_tenured_nursery_objects() -> usize {
    let mut evacuated = 0usize;
    crate::arena::arena_walk_objects(|header_ptr| {
        let header = header_ptr as *mut GcHeader;
        unsafe {
            let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
            // Skip if not in nursery (LONGLIVED + OLD have their
            // own arenas).
            if crate::arena::pointer_in_old_gen(user_ptr as usize) {
                return;
            }
            let flags = (*header).gc_flags;
            // Already evacuated (shouldn't happen — caller's filter
            // should prevent — but defend against duplicate calls).
            if flags & GC_FLAG_FORWARDED != 0 {
                return;
            }
            // Must be alive AND tenured.
            if flags & GC_FLAG_MARKED == 0 {
                return;
            }
            if flags & GC_FLAG_TENURED == 0 {
                return;
            }
            // Conservative-pinning blocks evacuation.
            if is_conservatively_pinned(header) {
                return;
            }
            // Allocate the new home in OLD_ARENA. Same size +
            // alignment as the original; same obj_type.
            let total = (*header).size as usize;
            let payload = total - GC_HEADER_SIZE;
            let new_user = crate::arena::arena_alloc_gc_old(payload, 8, (*header).obj_type);
            // Copy the user payload bytes verbatim. The new
            // GcHeader was set up by arena_alloc_gc_old; we don't
            // copy the OLD header (its flags / size match the
            // new alloc by construction).
            std::ptr::copy_nonoverlapping(user_ptr, new_user, payload);
            // Install forwarding pointer at the OLD location.
            set_forwarding_address(header, new_user);
            // Clear MARKED on the original so sweep frees its
            // (now-stale) nursery slot. The block can reset once
            // every object in it is either FORWARDED-cleared or
            // unmarked dead.
            (*header).gc_flags &= !GC_FLAG_MARKED;
            // Mark the new copy so (a) the rewrite walk visits
            // its fields and (b) sweep keeps it alive. The mark
            // bit is cleared inline by sweep on surviving objects.
            let new_header = (new_user as *mut u8).sub(GC_HEADER_SIZE) as *mut GcHeader;
            (*new_header).gc_flags |= GC_FLAG_MARKED;
            // Carry TENURED forward — the new copy is logically
            // the same object, just relocated. Without this the
            // age-bump pass on the next cycle would treat it as
            // a fresh young object.
            (*new_header).gc_flags |= GC_FLAG_TENURED;
            evacuated += 1;
        }
    });
    evacuated
}

/// Gen-GC Phase C4b-γ-2: rewrite a single NaN-boxed (or raw)
/// pointer-bearing word. If `bits` decodes to a heap pointer
/// whose target carries `GC_FLAG_FORWARDED`, return the rewritten
/// bits with the new address (preserving the NaN-boxing tag for
/// tagged inputs). Otherwise return None.
///
/// `valid_ptrs` must be the same set built at minor-GC entry —
/// pointers to NEW evac copies (allocated post-build) are not in
/// it, but those copies are never FORWARDED themselves so this
/// only matters for the initial validation step.
fn try_rewrite_value(bits: u64, valid_ptrs: &ValidPointerSet) -> Option<u64> {
    let tag = bits & TAG_MASK;
    let (ptr_addr, is_nanbox) = match tag {
        t if t == POINTER_TAG || t == STRING_TAG || t == BIGINT_TAG => {
            ((bits & POINTER_MASK) as usize, true)
        }
        _ => {
            // Reject NaN-tagged non-pointer values (numbers,
            // booleans, undefined, null, SSO, INT32, handles).
            if tag >= 0x7FF8_0000_0000_0000 {
                return None;
            }
            // Raw pointer fallback: lower 48 bits valid range.
            if !(0x1000..=0x0000_FFFF_FFFF_FFFF).contains(&bits) {
                return None;
            }
            (bits as usize, false)
        }
    };
    if ptr_addr == 0 || !valid_ptrs.contains(&ptr_addr) {
        return None;
    }
    unsafe {
        let header = (ptr_addr as *const u8).sub(GC_HEADER_SIZE) as *const GcHeader;
        if (*header).gc_flags & GC_FLAG_FORWARDED == 0 {
            return None;
        }
        let new_user = forwarding_address(header) as usize;
        Some(if is_nanbox {
            tag | (new_user as u64)
        } else {
            new_user as u64
        })
    }
}

/// In-place rewrite helper: read `*slot`, run it through
/// `try_rewrite_value`, write back if a rewrite was produced.
#[inline]
unsafe fn rewrite_slot(slot: *mut u64, valid_ptrs: &ValidPointerSet) {
    let bits = *slot;
    if let Some(new_bits) = try_rewrite_value(bits, valid_ptrs) {
        *slot = new_bits;
    }
}

unsafe fn rewrite_array_fields(user_ptr: *mut u8, valid_ptrs: &ValidPointerSet) {
    // Issue #233: skip FORWARDED arrays — their first 8 bytes hold a
    // forwarding pointer, not length+capacity. The forwarder itself
    // has no element fields to rewrite; the new location's fields
    // are handled by its own rewrite_array_fields visit.
    let header = (user_ptr as *const u8).sub(GC_HEADER_SIZE) as *const GcHeader;
    if (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
        return;
    }
    let arr = user_ptr as *const crate::array::ArrayHeader;
    let length = (*arr).length;
    let capacity = (*arr).capacity;
    if length > capacity || length > 16_000_000 {
        return;
    }
    let elements =
        (user_ptr as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64;
    for i in 0..length as usize {
        rewrite_slot(elements.add(i), valid_ptrs);
    }
}

unsafe fn rewrite_object_fields(user_ptr: *mut u8, valid_ptrs: &ValidPointerSet) {
    let obj = user_ptr as *const crate::object::ObjectHeader;
    let field_count = (*obj).field_count;
    if field_count > 1_000_000 {
        return;
    }
    let fields =
        (user_ptr as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>()) as *mut u64;
    for i in 0..field_count as usize {
        rewrite_slot(fields.add(i), valid_ptrs);
    }
    // keys_array — codegen may store either raw or NaN-boxed.
    // try_rewrite_value disambiguates by tag.
    let keys_addr = &(*obj).keys_array as *const _ as *mut u64;
    rewrite_slot(keys_addr, valid_ptrs);
}

unsafe fn rewrite_map_fields(user_ptr: *mut u8, valid_ptrs: &ValidPointerSet) {
    let map = user_ptr as *const crate::map::MapHeader;
    let size = (*map).size;
    let capacity = (*map).capacity;
    if size > capacity || size > 100_000 {
        return;
    }
    let entries = (*map).entries as *mut u64;
    if entries.is_null() {
        return;
    }
    for i in 0..(size as usize) {
        rewrite_slot(entries.add(i * 2), valid_ptrs);
        rewrite_slot(entries.add(i * 2 + 1), valid_ptrs);
    }
}

unsafe fn rewrite_closure_fields(user_ptr: *mut u8, valid_ptrs: &ValidPointerSet) {
    let closure = user_ptr as *const crate::closure::ClosureHeader;
    let capture_count = crate::closure::real_capture_count((*closure).capture_count);
    let captures =
        (user_ptr as *mut u8).add(std::mem::size_of::<crate::closure::ClosureHeader>()) as *mut u64;
    for i in 0..capture_count as usize {
        rewrite_slot(captures.add(i), valid_ptrs);
    }
}

unsafe fn rewrite_promise_fields(user_ptr: *mut u8, valid_ptrs: &ValidPointerSet) {
    let promise = user_ptr as *mut crate::promise::Promise;
    rewrite_slot(&(*promise).value as *const f64 as *mut u64, valid_ptrs);
    rewrite_slot(&(*promise).reason as *const f64 as *mut u64, valid_ptrs);
    rewrite_slot(&(*promise).on_fulfilled as *const _ as *mut u64, valid_ptrs);
    rewrite_slot(&(*promise).on_rejected as *const _ as *mut u64, valid_ptrs);
    rewrite_slot(&(*promise).next as *const _ as *mut u64, valid_ptrs);
}

unsafe fn rewrite_error_fields(user_ptr: *mut u8, valid_ptrs: &ValidPointerSet) {
    let error = user_ptr as *mut crate::error::ErrorHeader;
    rewrite_slot(&(*error).message as *const _ as *mut u64, valid_ptrs);
    rewrite_slot(&(*error).name as *const _ as *mut u64, valid_ptrs);
    rewrite_slot(&(*error).stack as *const _ as *mut u64, valid_ptrs);
    rewrite_slot(&(*error).cause as *const f64 as *mut u64, valid_ptrs);
    rewrite_slot(&(*error).errors as *const _ as *mut u64, valid_ptrs);
}

unsafe fn rewrite_lazy_array_fields(user_ptr: *mut u8, valid_ptrs: &ValidPointerSet) {
    let lazy = user_ptr as *mut crate::json_tape::LazyArrayHeader;
    if (*lazy).magic != crate::json_tape::LAZY_ARRAY_MAGIC {
        return;
    }
    rewrite_slot(&(*lazy).blob_str as *const _ as *mut u64, valid_ptrs);
    rewrite_slot(&(*lazy).materialized as *const _ as *mut u64, valid_ptrs);
    rewrite_slot(
        &(*lazy).materialized_elements as *const _ as *mut u64,
        valid_ptrs,
    );
    rewrite_slot(
        &(*lazy).materialized_bitmap as *const _ as *mut u64,
        valid_ptrs,
    );
    // Walk cached materialized JSValues — each holds a NaN-boxed
    // pointer to a backing object that may itself be forwarded.
    let cached_length = (*lazy).cached_length as usize;
    let cache = (*lazy).materialized_elements;
    let bitmap = (*lazy).materialized_bitmap;
    if !cache.is_null() && !bitmap.is_null() && cached_length > 0 {
        let bitmap_words = cached_length.div_ceil(64);
        for w in 0..bitmap_words {
            let word = *bitmap.add(w);
            if word == 0 {
                continue;
            }
            let base_idx = w * 64;
            for b in 0..64usize {
                if word & (1u64 << b) == 0 {
                    continue;
                }
                let i = base_idx + b;
                if i >= cached_length {
                    break;
                }
                let slot = cache.add(i) as *mut u64;
                rewrite_slot(slot, valid_ptrs);
            }
        }
    }
}

/// Walk every live (MARKED, non-FORWARDED) object on the heap and
/// rewrite any forwarded references in its fields. Includes new
/// evac copies (marked at evac time) and surviving non-evacuated
/// objects.
fn rewrite_heap_objects(valid_ptrs: &ValidPointerSet) {
    let rewrite_one = |header: *mut GcHeader| {
        unsafe {
            let flags = (*header).gc_flags;
            // FORWARDED originals are stale — first 8 bytes of
            // payload now holds the forwarding address, not real
            // field data. Skip them entirely.
            if flags & GC_FLAG_FORWARDED != 0 {
                return;
            }
            // Skip dead objects — sweep is about to free them.
            if flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) == 0 {
                return;
            }
            let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
            match (*header).obj_type {
                GC_TYPE_ARRAY => rewrite_array_fields(user_ptr, valid_ptrs),
                GC_TYPE_OBJECT => rewrite_object_fields(user_ptr, valid_ptrs),
                GC_TYPE_CLOSURE => rewrite_closure_fields(user_ptr, valid_ptrs),
                GC_TYPE_PROMISE => rewrite_promise_fields(user_ptr, valid_ptrs),
                GC_TYPE_ERROR => rewrite_error_fields(user_ptr, valid_ptrs),
                GC_TYPE_MAP => rewrite_map_fields(user_ptr, valid_ptrs),
                GC_TYPE_LAZY_ARRAY => rewrite_lazy_array_fields(user_ptr, valid_ptrs),
                GC_TYPE_STRING | GC_TYPE_BIGINT => {} // leaf
                _ => {}
            }
        }
    };
    crate::arena::arena_walk_objects(|hp| rewrite_one(hp as *mut GcHeader));
    MALLOC_STATE.with(|s| {
        let s = s.borrow();
        for &h in s.objects.iter() {
            rewrite_one(h);
        }
    });
}

/// Walk every shadow-stack slot and rewrite forwarded pointers.
/// Mirrors `shadow_stack_root_scanner` but writes back instead of
/// only reading. Slots hold NaN-boxed `JSValue` bits — same
/// encoding `try_rewrite_value` understands.
fn rewrite_shadow_stack_slots(valid_ptrs: &ValidPointerSet) {
    SHADOW.with(|cell| unsafe {
        let s = &mut *cell.get();
        if s.stack.is_empty() {
            return;
        }
        let mut top = s.frame_top;
        while top != usize::MAX && top >= SHADOW_STACK_HEADER_SLOTS {
            let header_base = top - SHADOW_STACK_HEADER_SLOTS;
            if header_base + 1 >= s.stack.len() {
                break;
            }
            let slot_count = s.stack[header_base + 1] as usize;
            let slots_end = top + slot_count;
            if slots_end > s.stack.len() {
                break;
            }
            for i in 0..slot_count {
                let bits = s.stack[top + i];
                if bits == 0 {
                    continue;
                }
                if let Some(new_bits) = try_rewrite_value(bits, valid_ptrs) {
                    s.stack[top + i] = new_bits;
                }
            }
            top = s.stack[header_base] as usize;
        }
    });
}

/// Walk every registered module-global address and rewrite
/// forwarded pointers. Mirrors `mark_global_roots` but writes
/// back. Both NaN-boxed and raw-pointer encodings are handled by
/// `try_rewrite_value` via tag inspection.
fn rewrite_global_roots(valid_ptrs: &ValidPointerSet) {
    GLOBAL_ROOTS.with(|roots| {
        let roots = roots.borrow();
        for &root_ptr in roots.iter() {
            if root_ptr.is_null() {
                continue;
            }
            unsafe {
                let bits = *root_ptr;
                if let Some(new_bits) = try_rewrite_value(bits, valid_ptrs) {
                    *root_ptr = new_bits;
                }
            }
        }
    });
}

/// Top-level Phase C4b-γ-2 entry: rewrite every reference site we
/// own. Skipped: conservatively-discovered C-stack words (we can't
/// safely overwrite arbitrary stack memory; pinning of conservative-
/// root targets in `gc_collect_minor` keeps those references valid
/// without rewriting). Skipped: registered root scanners (their
/// API is mark-only); the conservative-pinning policy also covers
/// objects they would have discovered.
fn rewrite_forwarded_references(valid_ptrs: &ValidPointerSet) {
    rewrite_shadow_stack_slots(valid_ptrs);
    rewrite_global_roots(valid_ptrs);
    rewrite_heap_objects(valid_ptrs);
}

/// Gen-GC Phase C4b: is `header` pinned this cycle (cannot be
/// evacuated)? Tested by the evacuation candidate filter in
/// `gc_collect_minor` after the age-bump pass.
#[inline]
pub fn is_conservatively_pinned(header: *const GcHeader) -> bool {
    CONS_PINNED.with(|s| s.borrow().contains(&(header as usize)))
}

/// Test-only diagnostic: number of objects pinned this cycle.
pub fn cons_pinned_count() -> usize {
    CONS_PINNED.with(|s| s.borrow().len())
}

/// Gen-GC Phase C1: the write barrier. Called by codegen-emitted
/// store sites (after sub-phase C2 wires the emission).
///
/// Decode the parent + child as raw addresses. If parent's
/// GcHeader sits in the old-gen arena AND child's NaN-boxed
/// pointer (any of POINTER / STRING / BIGINT / SHORT_STRING)
/// resolves to a heap address inside the nursery, record the
/// parent's GcHeader in the remembered set.
///
/// Hot-path constraints: this fires on EVERY heap store in
/// compiled code once C2 lands. Must be cheap. The current
/// O(blocks) range checks via `pointer_in_*` will be optimized
/// to a single bit-test on `GcHeader::gc_flags & GC_FLAG_YOUNG`
/// in sub-phase C3 — this commit's predicate is the simple
/// correct-but-slower form so the C2 codegen wiring can land
/// without a perf cliff (gated behind PERRY_WRITE_BARRIERS=1).
#[no_mangle]
pub extern "C" fn js_write_barrier(parent: u64, child: u64) {
    // Decode the parent — must be a NaN-boxed pointer (POINTER /
    // STRING / BIGINT / SHORT_STRING) or a raw heap address.
    let parent_addr = decode_heap_addr(parent);
    if parent_addr == 0 {
        return;
    }
    // Decode child similarly.
    let child_addr = decode_heap_addr(child);
    if child_addr == 0 {
        return;
    }
    // Old → young check.
    if !crate::arena::pointer_in_old_gen(parent_addr) {
        return;
    }
    if !crate::arena::pointer_in_nursery(child_addr) {
        return;
    }
    // Parent's GcHeader sits at parent_addr - GC_HEADER_SIZE.
    let header = parent_addr.saturating_sub(GC_HEADER_SIZE);
    REMEMBERED_SET.with(|s| {
        s.borrow_mut().insert(header);
    });
}

/// Decode a NaN-boxed value into a heap address. Returns 0 for
/// non-pointer values (numbers / booleans / undefined / null).
/// Accepts POINTER_TAG / STRING_TAG / BIGINT_TAG / SHORT_STRING_TAG;
/// SHORT_STRING values return 0 because they're inline data, not
/// heap pointers.
#[inline]
fn decode_heap_addr(bits: u64) -> usize {
    let tag = bits & TAG_MASK;
    if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
        (bits & POINTER_MASK) as usize
    } else if tag < 0x7FF8_0000_0000_0000 {
        // Plain double — could be a raw bitcast pointer (rare).
        // Treat as non-pointer in the barrier; minor GC's precise
        // root scan still catches anything reachable via the
        // shadow stack.
        0
    } else {
        // SHORT_STRING_TAG (0x7FF9), INT32_TAG (0x7FFE),
        // primitive (0x7FFC), JS_HANDLE (0x7FFB) — none are
        // young-gen pointers.
        0
    }
}

/// Gen-GC Phase C: read the current remembered set size — used
/// by tests and `PERRY_GC_DIAG=1` output to confirm barrier
/// activity. Returns 0 in Phase C1 since no codegen-emitted
/// barrier has fired yet.
pub fn remembered_set_size() -> usize {
    REMEMBERED_SET.with(|s| s.borrow().len())
}

/// Gen-GC Phase C: clear the remembered set. Will be called by
/// minor GC after the rs-scan completes (Phase C3). Test-only
/// for now to enable test isolation.
pub fn remembered_set_clear() {
    REMEMBERED_SET.with(|s| s.borrow_mut().clear());
}

/// Root scanner for the shadow stack (gen-GC Phase A sub-phase 4).
/// Walks every live slot in every pushed frame and invokes `mark`
/// with the slot's NaN-boxed f64 value. The mark callback's
/// `try_mark_value` pipeline already knows how to distinguish
/// plain numbers / undefined / null / booleans (skipped) from
/// POINTER_TAG / STRING_TAG / BIGINT_TAG / SHORT_STRING_TAG
/// values that refer to heap objects.
///
/// Runs IN PARALLEL with the conservative stack scanner — this is
/// the Phase A design: shadow stack adds precise roots that the
/// conservative scanner would also have found via register/stack
/// walk, just as a separate direct source. Correctness-safe
/// overlap: marking an already-marked object is a no-op. Phase B+
/// will start dropping conservative-scanner coverage for stack
/// slots that the shadow stack authoritatively covers, reducing
/// over-promotion in the generational GC.
///
/// Zero-slot frames (functions where no local is pointer-typed)
/// contribute nothing — the inner loop's `slots_count == 0` exits
/// immediately. Empty shadow stack (no function call currently
/// active, or PERRY_SHADOW_STACK=0 at compile time so push/pop
/// never emitted) also contributes nothing.
pub fn shadow_stack_root_scanner(mark: &mut dyn FnMut(f64)) {
    SHADOW.with(|cell| unsafe {
        let s = &*cell.get();
        if s.stack.is_empty() {
            return;
        }
        // Walk every frame by chasing prev_frame_top pointers from
        // the current top. Each frame's layout:
        //   [slot_0 .. slot_{n-1}]  with header at prev
        //     = [prev_frame_top, slot_count] at (top - 2, top - 1)
        let mut top = s.frame_top;
        while top != usize::MAX && top >= SHADOW_STACK_HEADER_SLOTS {
            let header_base = top - SHADOW_STACK_HEADER_SLOTS;
            if header_base + 1 >= s.stack.len() {
                break;
            }
            let slot_count = s.stack[header_base + 1] as usize;
            let slots_end = top + slot_count;
            if slots_end > s.stack.len() {
                break;
            }
            for i in 0..slot_count {
                let bits = s.stack[top + i];
                if bits != 0 {
                    mark(f64::from_bits(bits));
                }
            }
            top = s.stack[header_base] as usize;
        }
    });
}

/// Initialize GC root scanners. Called once at runtime startup.
pub fn gc_init() {
    gc_register_root_scanner(promise_root_scanner);
    gc_register_root_scanner(timer_root_scanner);
    gc_register_root_scanner(exception_root_scanner);
    gc_register_root_scanner(async_context_root_scanner);
    gc_register_root_scanner(async_hooks_root_scanner);
    gc_register_root_scanner(shape_cache_root_scanner);
    gc_register_root_scanner(crate::regex::scan_last_exec_groups_root);
    gc_register_root_scanner(crate::array::scan_template_raw_roots);
    gc_register_root_scanner(transition_cache_root_scanner);
    gc_register_root_scanner(overflow_fields_root_scanner);
    gc_register_root_scanner(json_parse_root_scanner);
    gc_register_root_scanner(intern_table_root_scanner);
    gc_register_root_scanner(shadow_stack_root_scanner);
    gc_register_root_scanner(crate::builtins::scan_console_log_singleton_roots);
    // Box-capture root scanner (mutable closure captures, esp. the
    // generator state-machine's `__iter` and `__step` boxes that hold
    // the iter object + step closure across awaits).
    gc_register_root_scanner(crate::r#box::scan_box_roots);
    // Iter-result scratch slot — the async-step fast path stows the
    // generator's most recent yield value here; it stays live until
    // the step driver reads it back.
    gc_register_root_scanner(crate::promise::scan_iter_result_root);
    // Async-step thunk single-slot cache (build_async_step_thunks).
    gc_register_root_scanner(crate::promise::scan_async_step_thunk_cache);
    // perry/tui hook + state slot pools — they store raw NaN-boxed
    // value bits but the GC has no other way to know which slots hold
    // heap pointers (arrays/objects/strings stashed via setState /
    // useState / useRef). #679 follow-up: pre-fix, an Enter-press in
    // the perry-code demo stored a freshly-concat'd messages array,
    // the next allocation triggered minor GC, and the array was
    // reclaimed because nothing else held it — `messages.map(…)` on
    // the stale pointer produced an empty render.
    gc_register_root_scanner(crate::tui::hooks::scan_hook_slot_roots);
    gc_register_root_scanner(crate::tui::state::scan_state_slot_roots);
    #[cfg(feature = "ohos-napi")]
    gc_register_root_scanner(crate::arkts_callbacks::arkts_callbacks_root_scanner);
}

/// Root scanner for the string intern table.
pub fn intern_table_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::string::scan_intern_table_roots(mark);
}

/// FFI: initialize GC (called from compiled code startup)
#[no_mangle]
pub extern "C" fn js_gc_init() {
    gc_init();
}

/// FFI: get GC stats
#[no_mangle]
pub extern "C" fn js_gc_stats(
    out_collections: *mut u64,
    out_freed: *mut u64,
    out_pause_us: *mut u64,
) {
    GC_STATS.with(|stats| {
        let stats = stats.borrow();
        unsafe {
            if !out_collections.is_null() {
                *out_collections = stats.collection_count;
            }
            if !out_freed.is_null() {
                *out_freed = stats.total_freed_bytes;
            }
            if !out_pause_us.is_null() {
                *out_pause_us = stats.last_pause_us;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gc_malloc_basic() {
        // Allocate a string-type object
        let ptr = gc_malloc(64, GC_TYPE_STRING);
        assert!(!ptr.is_null());

        // Verify header is set correctly
        unsafe {
            let header = header_from_user_ptr(ptr);
            assert_eq!((*header).obj_type, GC_TYPE_STRING);
            assert_eq!((*header).gc_flags, 0); // not arena, not marked
            assert_eq!((*header).size as usize, GC_HEADER_SIZE + 64);
        }

        // Verify it's tracked in MALLOC_OBJECTS (rebuild lazy set first)
        let tracked = MALLOC_STATE.with(|s| {
            let header = unsafe { header_from_user_ptr(ptr) };
            let mut s = s.borrow_mut();
            ensure_set_built(&mut s);
            s.set.contains(&(header as usize))
        });
        assert!(tracked, "allocated object should be tracked in MALLOC_SET");
    }

    #[test]
    fn test_gc_malloc_different_types() {
        let string_ptr = gc_malloc(32, GC_TYPE_STRING);
        let closure_ptr = gc_malloc(48, GC_TYPE_CLOSURE);
        let bigint_ptr = gc_malloc(16, GC_TYPE_BIGINT);

        unsafe {
            assert_eq!((*header_from_user_ptr(string_ptr)).obj_type, GC_TYPE_STRING);
            assert_eq!(
                (*header_from_user_ptr(closure_ptr)).obj_type,
                GC_TYPE_CLOSURE
            );
            assert_eq!((*header_from_user_ptr(bigint_ptr)).obj_type, GC_TYPE_BIGINT);
        }
    }

    #[test]
    fn test_gc_collect_updates_stats() {
        // Get initial stats
        let initial_count = GC_STATS.with(|s| s.borrow().collection_count);

        // Run GC
        gc_collect_inner();

        // Stats should have incremented
        let new_count = GC_STATS.with(|s| s.borrow().collection_count);
        assert_eq!(
            new_count,
            initial_count + 1,
            "collection count should increment"
        );
    }

    #[test]
    fn test_gc_header_size() {
        assert_eq!(GC_HEADER_SIZE, 8, "GC header should be 8 bytes");
    }

    /// Issue #179: block-persist's age window must match the reset
    /// policy's `keep_low` window — both define the set of blocks
    /// where caller-saved-register handles might still be uncaptured.
    /// If the two drift apart, block-persist either over-retains old
    /// blocks (RSS regression) or under-protects recent blocks
    /// (re-opens the issues #43 / #44 dangling-pointer failure mode).
    #[test]
    fn block_persist_window_matches_reset_keep_low() {
        // `keep_low = current.saturating_sub(4)` → 5 blocks
        // (current-4..=current). `BLOCK_PERSIST_WINDOW` gates Pass 2
        // of `mark_block_persisting_arena_objects` via
        // `persist_low = general_n.saturating_sub(BLOCK_PERSIST_WINDOW)`.
        // Both windows must describe the same "register-miss risk"
        // horizon for the correctness invariant to hold.
        assert_eq!(
            BLOCK_PERSIST_WINDOW, 5,
            "block-persist window must match reset's keep_low window (5 blocks)"
        );
    }

    /// Issue #179: `gc_collect_inner` must return the sweep's
    /// freed_bytes so the adaptive step logic can react to
    /// object-reclaim activity immediately, not wait for blocks to
    /// clear the 2-cycle grace and surface as a `pre - post` drop on
    /// the next cycle. The return value drives the `>90% halve /
    /// 10-90% halve / <10% double` classifier in `gc_check_trigger`.
    #[test]
    fn gc_collect_inner_returns_freed_bytes() {
        // Allocate an object that's guaranteed unreachable (no
        // roots hold it — we immediately drop the pointer).
        let _throwaway = gc_malloc(128, GC_TYPE_STRING);
        // freed_bytes is the per-sweep reclaim count; for this
        // tiny test we just assert the signature (returns u64).
        // The exact freed count depends on thread-local state from
        // other tests, so we only assert the type/shape.
        let _freed: u64 = gc_collect_inner();
    }

    #[test]
    fn test_gc_realloc_basic() {
        let ptr = gc_malloc(32, GC_TYPE_STRING);
        assert!(!ptr.is_null());

        // Write some data
        unsafe {
            std::ptr::write_bytes(ptr, 0xAB, 32);
        }

        // Reallocate to larger size
        let new_ptr = gc_realloc(ptr, 128);
        assert!(!new_ptr.is_null());

        // Verify old data preserved (first 32 bytes should still be 0xAB)
        unsafe {
            for i in 0..32 {
                assert_eq!(
                    *new_ptr.add(i),
                    0xAB,
                    "byte {} should be preserved after realloc",
                    i
                );
            }
        }

        // Verify tracking updated (rebuild lazy set first)
        let tracked = MALLOC_STATE.with(|s| {
            let header = unsafe { header_from_user_ptr(new_ptr) };
            let mut s = s.borrow_mut();
            ensure_set_built(&mut s);
            s.set.contains(&(header as usize))
        });
        assert!(tracked, "reallocated object should be tracked");
    }

    #[test]
    fn test_gc_realloc_null_allocates_fresh() {
        let ptr = gc_realloc(std::ptr::null_mut(), 64);
        assert!(!ptr.is_null(), "realloc(null) should allocate fresh");
    }

    #[test]
    fn test_gc_mark_flags() {
        let ptr = gc_malloc(32, GC_TYPE_STRING);
        unsafe {
            let header = header_from_user_ptr(ptr);

            // Initially not marked
            assert_eq!((*header).gc_flags & GC_FLAG_MARKED, 0);

            // Mark it
            (*header).gc_flags |= GC_FLAG_MARKED;
            assert_ne!((*header).gc_flags & GC_FLAG_MARKED, 0);

            // Clear mark
            (*header).gc_flags &= !GC_FLAG_MARKED;
            assert_eq!((*header).gc_flags & GC_FLAG_MARKED, 0);
        }
    }

    #[test]
    fn test_gc_pinned_flag() {
        let ptr = gc_malloc(32, GC_TYPE_STRING);
        unsafe {
            let header = header_from_user_ptr(ptr);

            // Pin it
            (*header).gc_flags |= GC_FLAG_PINNED;

            // Run GC - pinned objects should survive
            gc_collect_inner();

            // Verify still tracked (rebuild lazy set first)
            let tracked = MALLOC_STATE.with(|s| {
                let mut s = s.borrow_mut();
                ensure_set_built(&mut s);
                s.set.contains(&(header as usize))
            });
            assert!(tracked, "pinned object should survive GC");

            // Unpin
            (*header).gc_flags &= !GC_FLAG_PINNED;
        }
    }

    #[test]
    fn test_build_valid_pointer_set() {
        // Allocate some objects
        let ptr1 = gc_malloc(32, GC_TYPE_STRING);
        let ptr2 = gc_malloc(64, GC_TYPE_CLOSURE);

        let valid_set = build_valid_pointer_set();

        // Our malloc objects should be in the valid set
        assert!(
            valid_set.contains(&(ptr1 as usize)),
            "ptr1 should be in valid set"
        );
        assert!(
            valid_set.contains(&(ptr2 as usize)),
            "ptr2 should be in valid set"
        );
    }

    /// Helper: reset the shadow stack to a known-empty state
    /// between tests. Needed because Rust's thread-local state
    /// persists across tests in the same thread.
    fn reset_shadow_stack() {
        SHADOW.with(|cell| unsafe {
            let s = &mut *cell.get();
            s.stack.clear();
            s.frame_top = usize::MAX;
        });
    }

    #[test]
    fn test_shadow_stack_push_pop_single_frame() {
        reset_shadow_stack();
        assert_eq!(shadow_stack_depth(), 0);
        let h = js_shadow_frame_push(3);
        assert_eq!(shadow_stack_depth(), 1);
        // Slots initialized to 0.
        for i in 0..3 {
            assert_eq!(js_shadow_slot_get(i), 0, "slot {} not zero", i);
        }
        js_shadow_frame_pop(h);
        assert_eq!(shadow_stack_depth(), 0);
        // After pop, reads return 0 (no active frame).
        assert_eq!(js_shadow_slot_get(0), 0);
    }

    #[test]
    fn test_shadow_stack_slot_store_load() {
        reset_shadow_stack();
        let h = js_shadow_frame_push(4);
        // Store some pointer bit patterns.
        js_shadow_slot_set(0, 0x7FFD_0000_1234_5678); // POINTER_TAG
        js_shadow_slot_set(1, 0x7FFF_0000_9ABC_DEF0); // STRING_TAG
        js_shadow_slot_set(2, 0); // hole
        js_shadow_slot_set(3, 0x7FF9_0200_0000_6B6F); // SSO "ok"
        assert_eq!(js_shadow_slot_get(0), 0x7FFD_0000_1234_5678);
        assert_eq!(js_shadow_slot_get(1), 0x7FFF_0000_9ABC_DEF0);
        assert_eq!(js_shadow_slot_get(2), 0);
        assert_eq!(js_shadow_slot_get(3), 0x7FF9_0200_0000_6B6F);
        // Out-of-range read returns 0 (clamp).
        assert_eq!(js_shadow_slot_get(4), 0);
        js_shadow_frame_pop(h);
    }

    #[test]
    fn test_shadow_stack_nested_frames() {
        reset_shadow_stack();
        let outer = js_shadow_frame_push(2);
        js_shadow_slot_set(0, 0x1111);
        js_shadow_slot_set(1, 0x2222);
        assert_eq!(shadow_stack_depth(), 1);

        let inner = js_shadow_frame_push(3);
        js_shadow_slot_set(0, 0xAAAA);
        js_shadow_slot_set(1, 0xBBBB);
        js_shadow_slot_set(2, 0xCCCC);
        assert_eq!(shadow_stack_depth(), 2);
        // Inner frame sees its own slots, not the outer's.
        assert_eq!(js_shadow_slot_get(0), 0xAAAA);
        assert_eq!(js_shadow_slot_get(1), 0xBBBB);
        assert_eq!(js_shadow_slot_get(2), 0xCCCC);

        js_shadow_frame_pop(inner);
        assert_eq!(shadow_stack_depth(), 1);
        // Outer slots preserved across the inner push+pop — this is
        // the load-bearing invariant for codegen: a called function
        // can freely mutate its own frame without corrupting the
        // caller's.
        assert_eq!(js_shadow_slot_get(0), 0x1111);
        assert_eq!(js_shadow_slot_get(1), 0x2222);

        js_shadow_frame_pop(outer);
        assert_eq!(shadow_stack_depth(), 0);
    }

    #[test]
    fn test_shadow_stack_frame_with_zero_slots() {
        reset_shadow_stack();
        let h = js_shadow_frame_push(0);
        assert_eq!(shadow_stack_depth(), 1);
        // No slots to read; get returns 0 anyway (out-of-range path).
        assert_eq!(js_shadow_slot_get(0), 0);
        js_shadow_frame_pop(h);
        assert_eq!(shadow_stack_depth(), 0);
    }

    #[test]
    fn test_shadow_stack_deep_nesting() {
        reset_shadow_stack();
        let mut handles = Vec::new();
        for i in 0..16 {
            let h = js_shadow_frame_push(2);
            js_shadow_slot_set(0, i as u64);
            js_shadow_slot_set(1, (i * 2) as u64);
            handles.push(h);
        }
        assert_eq!(shadow_stack_depth(), 16);
        // Pop back down; slots restore on each pop.
        for i in (0..16).rev() {
            assert_eq!(js_shadow_slot_get(0), i as u64);
            assert_eq!(js_shadow_slot_get(1), (i * 2) as u64);
            js_shadow_frame_pop(handles.pop().unwrap());
        }
        assert_eq!(shadow_stack_depth(), 0);
    }

    #[test]
    fn test_shadow_stack_root_scanner_empty() {
        reset_shadow_stack();
        let mut count = 0;
        shadow_stack_root_scanner(&mut |_| count += 1);
        assert_eq!(count, 0, "empty shadow stack yields no roots");
    }

    #[test]
    fn test_shadow_stack_root_scanner_single_frame() {
        reset_shadow_stack();
        let h = js_shadow_frame_push(4);
        // Mix of set / unset slots.
        js_shadow_slot_set(0, 0x7FFD_0000_1234_5678);
        // slot 1 left zero — must NOT be emitted
        js_shadow_slot_set(2, 0x7FFF_0000_9ABC_DEF0);
        js_shadow_slot_set(3, 0x7FFA_0000_DEAD_BEEF);
        let mut emitted: Vec<u64> = Vec::new();
        shadow_stack_root_scanner(&mut |v| emitted.push(v.to_bits()));
        assert_eq!(emitted.len(), 3, "only non-zero slots should be emitted");
        assert!(emitted.contains(&0x7FFD_0000_1234_5678));
        assert!(emitted.contains(&0x7FFF_0000_9ABC_DEF0));
        assert!(emitted.contains(&0x7FFA_0000_DEAD_BEEF));
        js_shadow_frame_pop(h);
    }

    #[test]
    fn test_shadow_stack_root_scanner_nested_frames() {
        reset_shadow_stack();
        let outer = js_shadow_frame_push(2);
        js_shadow_slot_set(0, 0xAAAA);
        js_shadow_slot_set(1, 0xBBBB);
        let inner = js_shadow_frame_push(3);
        js_shadow_slot_set(0, 0xCCCC);
        js_shadow_slot_set(1, 0xDDDD);
        js_shadow_slot_set(2, 0xEEEE);

        let mut emitted: Vec<u64> = Vec::new();
        shadow_stack_root_scanner(&mut |v| emitted.push(v.to_bits()));

        // Scanner should hit BOTH frames — outer frame's slots
        // must also be reported, not just the innermost. This is
        // the load-bearing invariant for Phase B+ where the GC
        // collects while deep in a call chain.
        assert_eq!(emitted.len(), 5);
        assert!(emitted.contains(&0xAAAA));
        assert!(emitted.contains(&0xBBBB));
        assert!(emitted.contains(&0xCCCC));
        assert!(emitted.contains(&0xDDDD));
        assert!(emitted.contains(&0xEEEE));

        js_shadow_frame_pop(inner);
        js_shadow_frame_pop(outer);
    }

    #[test]
    fn test_shadow_stack_root_scanner_zero_slot_frames() {
        reset_shadow_stack();
        // Zero-slot frame (function with no pointer-typed locals)
        // contributes nothing. Nested non-zero frame still works.
        let a = js_shadow_frame_push(0);
        let b = js_shadow_frame_push(2);
        js_shadow_slot_set(0, 0x1234);
        js_shadow_slot_set(1, 0x5678);
        let c = js_shadow_frame_push(0);

        let mut emitted: Vec<u64> = Vec::new();
        shadow_stack_root_scanner(&mut |v| emitted.push(v.to_bits()));
        assert_eq!(emitted.len(), 2);

        js_shadow_frame_pop(c);
        js_shadow_frame_pop(b);
        js_shadow_frame_pop(a);
    }

    /// Helper for write-barrier tests: clear the remembered set
    /// to a known-empty state.
    fn reset_remembered_set() {
        REMEMBERED_SET.with(|s| s.borrow_mut().clear());
    }

    #[test]
    fn test_write_barrier_old_to_young_records() {
        reset_remembered_set();
        let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        let old = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
        let parent_nanbox = POINTER_TAG | (old as u64);
        let child_nanbox = POINTER_TAG | (young as u64);
        assert_eq!(remembered_set_size(), 0);
        js_write_barrier(parent_nanbox, child_nanbox);
        assert_eq!(
            remembered_set_size(),
            1,
            "old→young write must record parent"
        );
        // Same write again should NOT double-count (HashSet dedups).
        js_write_barrier(parent_nanbox, child_nanbox);
        assert_eq!(
            remembered_set_size(),
            1,
            "duplicate barrier call must dedup"
        );
    }

    #[test]
    fn test_write_barrier_young_to_young_skipped() {
        reset_remembered_set();
        let parent = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        let child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        js_write_barrier(POINTER_TAG | (parent as u64), POINTER_TAG | (child as u64));
        assert_eq!(
            remembered_set_size(),
            0,
            "young→young write must not enter remembered set"
        );
    }

    #[test]
    fn test_write_barrier_old_to_old_skipped() {
        reset_remembered_set();
        let parent = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
        let child = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
        js_write_barrier(POINTER_TAG | (parent as u64), POINTER_TAG | (child as u64));
        assert_eq!(
            remembered_set_size(),
            0,
            "old→old write must not enter remembered set (no inter-gen edge)"
        );
    }

    #[test]
    fn test_write_barrier_old_to_young_string_tag() {
        reset_remembered_set();
        let young_str = crate::arena::arena_alloc_gc(32, 8, GC_TYPE_STRING) as usize;
        let old = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
        // STRING_TAG should also fire the barrier — strings can be young.
        js_write_barrier(POINTER_TAG | (old as u64), STRING_TAG | (young_str as u64));
        assert_eq!(remembered_set_size(), 1);
    }

    #[test]
    fn test_write_barrier_non_pointer_child_skipped() {
        reset_remembered_set();
        let old = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
        // INT32_TAG in child position.
        let int32_val = 0x7FFE_0000_0000_002A_u64;
        js_write_barrier(POINTER_TAG | (old as u64), int32_val);
        assert_eq!(
            remembered_set_size(),
            0,
            "non-pointer child must not enter remembered set"
        );
        // SHORT_STRING_TAG (SSO inline) — also not a heap pointer.
        let sso = 0x7FF9_0500_0000_0000_u64;
        js_write_barrier(POINTER_TAG | (old as u64), sso);
        assert_eq!(
            remembered_set_size(),
            0,
            "SSO child is inline data, not a heap pointer"
        );
        // Plain double in child position.
        js_write_barrier(POINTER_TAG | (old as u64), 3.14_f64.to_bits());
        assert_eq!(
            remembered_set_size(),
            0,
            "number child must not enter remembered set"
        );
    }

    #[test]
    fn test_write_barrier_remembered_set_clear() {
        reset_remembered_set();
        let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        let old = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
        js_write_barrier(POINTER_TAG | (old as u64), POINTER_TAG | (young as u64));
        assert_eq!(remembered_set_size(), 1);
        remembered_set_clear();
        assert_eq!(remembered_set_size(), 0);
    }

    #[test]
    fn test_gc_collect_minor_clears_rs() {
        reset_remembered_set();
        let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        let old = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
        js_write_barrier(POINTER_TAG | (old as u64), POINTER_TAG | (young as u64));
        assert_eq!(remembered_set_size(), 1);
        let _freed = gc_collect_minor();
        assert_eq!(
            remembered_set_size(),
            0,
            "minor GC must clear RS just like full GC does"
        );
    }

    #[test]
    fn test_minor_gc_promotes_after_two_survivals() {
        reset_remembered_set();
        // Allocate an arena object and pin it so it survives every GC.
        let user_ptr = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
        unsafe {
            let header = header_from_user_ptr(user_ptr);
            (*header).gc_flags |= GC_FLAG_PINNED;
            // Initial state: not yet survived, not tenured.
            assert_eq!((*header).gc_flags & GC_FLAG_HAS_SURVIVED, 0);
            assert_eq!((*header).gc_flags & GC_FLAG_TENURED, 0);
        }
        // First minor GC: object survives, gets HAS_SURVIVED bit.
        let _ = gc_collect_minor();
        unsafe {
            let header = header_from_user_ptr(user_ptr);
            assert_ne!(
                (*header).gc_flags & GC_FLAG_HAS_SURVIVED,
                0,
                "first survival should set HAS_SURVIVED"
            );
            assert_eq!(
                (*header).gc_flags & GC_FLAG_TENURED,
                0,
                "first survival should not yet tenure"
            );
        }
        // Second minor GC: HAS_SURVIVED + survives → TENURED, clear HAS_SURVIVED.
        let _ = gc_collect_minor();
        unsafe {
            let header = header_from_user_ptr(user_ptr);
            assert_ne!(
                (*header).gc_flags & GC_FLAG_TENURED,
                0,
                "second survival should tenure"
            );
            assert_eq!(
                (*header).gc_flags & GC_FLAG_HAS_SURVIVED,
                0,
                "tenuring should clear HAS_SURVIVED"
            );
        }
        // Third minor GC: stays tenured idempotently.
        let _ = gc_collect_minor();
        unsafe {
            let header = header_from_user_ptr(user_ptr);
            assert_ne!(
                (*header).gc_flags & GC_FLAG_TENURED,
                0,
                "tenured stays tenured across subsequent collections"
            );
        }
    }

    #[test]
    fn test_forwarding_pointer_roundtrip() {
        // Allocate a nursery object, simulate evacuation by copying
        // its bytes into an old-gen alloc, install the forwarding
        // address in the nursery header. Read back via
        // forwarding_address to confirm round-trip.
        let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
        let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
        unsafe {
            // Pre-condition: not forwarded yet.
            let nursery_hdr = header_from_user_ptr(nursery_user);
            assert_eq!((*nursery_hdr).gc_flags & GC_FLAG_FORWARDED, 0);
            // Install forwarding pointer.
            set_forwarding_address(nursery_hdr as *mut GcHeader, old_user);
            // Post-condition: flag set, address readable.
            assert_ne!((*nursery_hdr).gc_flags & GC_FLAG_FORWARDED, 0);
            assert_eq!(forwarding_address(nursery_hdr), old_user);
        }
    }

    #[test]
    fn test_forwarding_does_not_disturb_other_flags() {
        // Setting FORWARDED must preserve every other gc_flags bit.
        let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
        let old = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
        unsafe {
            let hdr = header_from_user_ptr(user) as *mut GcHeader;
            // Set a few unrelated flags.
            (*hdr).gc_flags |= GC_FLAG_MARKED | GC_FLAG_TENURED | GC_FLAG_HAS_SURVIVED;
            let before = (*hdr).gc_flags;
            set_forwarding_address(hdr, old);
            let after = (*hdr).gc_flags;
            assert_eq!(after & GC_FLAG_FORWARDED, GC_FLAG_FORWARDED);
            // Every bit that was set before stays set.
            assert_eq!(
                after & before,
                before,
                "forwarding installation cleared an existing flag"
            );
        }
    }

    #[test]
    fn test_forwarding_pointer_value_is_8_bytes_at_user_offset_zero() {
        // The forwarding pointer is stored in the first 8 bytes of
        // the user payload. This invariant is load-bearing for any
        // future walker that wants to skip over forwarded objects
        // by reading the new address inline. Verify by direct
        // pointer arithmetic.
        let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
        let target = 0x12345678_9ABCDEF0_u64 as *mut u8;
        unsafe {
            let hdr = header_from_user_ptr(nursery_user) as *mut GcHeader;
            set_forwarding_address(hdr, target);
            // Read directly: user_ptr cast to *const *mut u8.
            let raw = nursery_user as *const *mut u8;
            assert_eq!(*raw, target);
        }
    }

    #[test]
    fn test_cons_pinned_cleared_after_minor_gc() {
        // Allocate something to give the GC sweep work to do.
        let _ = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
        // Pre-populate CONS_PINNED to simulate a prior GC's leftover.
        CONS_PINNED.with(|s| {
            s.borrow_mut().insert(0xDEAD_BEEF);
        });
        assert!(cons_pinned_count() >= 1);
        let _ = gc_collect_minor();
        assert_eq!(
            cons_pinned_count(),
            0,
            "minor GC must clear CONS_PINNED after collection"
        );
    }

    #[test]
    fn test_pin_currently_marked_captures_marked_objects() {
        // Manually mark an arena object, then run the pinning
        // scan. The pinned set should contain the marked header.
        CONS_PINNED.with(|s| s.borrow_mut().clear());
        let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
        let header = unsafe { header_from_user_ptr(user) as *mut GcHeader };
        unsafe {
            (*header).gc_flags |= GC_FLAG_MARKED;
        }
        pin_currently_marked_as_conservative();
        assert!(
            is_conservatively_pinned(header),
            "marked header should land in CONS_PINNED"
        );
        // Cleanup for test isolation.
        unsafe {
            (*header).gc_flags &= !GC_FLAG_MARKED;
        }
        CONS_PINNED.with(|s| s.borrow_mut().clear());
    }

    #[test]
    fn test_pin_currently_marked_skips_unmarked() {
        CONS_PINNED.with(|s| s.borrow_mut().clear());
        let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
        let header = unsafe { header_from_user_ptr(user) as *const GcHeader };
        // Ensure unmarked.
        unsafe {
            assert_eq!((*(header as *mut GcHeader)).gc_flags & GC_FLAG_MARKED, 0);
        }
        pin_currently_marked_as_conservative();
        assert!(
            !is_conservatively_pinned(header),
            "unmarked header should NOT land in CONS_PINNED"
        );
    }

    #[test]
    fn test_evacuate_tenured_skips_pinned() {
        // An object that's MARKED + TENURED + CONS_PINNED must
        // NOT be evacuated.
        CONS_PINNED.with(|s| s.borrow_mut().clear());
        let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
        let header = unsafe { header_from_user_ptr(user) as *mut GcHeader };
        unsafe {
            (*header).gc_flags |= GC_FLAG_MARKED | GC_FLAG_TENURED;
        }
        // Pin it.
        CONS_PINNED.with(|s| s.borrow_mut().insert(header as usize));
        let n = evacuate_tenured_nursery_objects();
        assert_eq!(n, 0, "pinned tenured object must not be evacuated");
        unsafe {
            assert_eq!(
                (*header).gc_flags & GC_FLAG_FORWARDED,
                0,
                "FORWARDED flag must not be set on pinned object"
            );
        }
        // Cleanup
        unsafe {
            (*header).gc_flags &= !(GC_FLAG_MARKED | GC_FLAG_TENURED);
        }
        CONS_PINNED.with(|s| s.borrow_mut().clear());
    }

    #[test]
    fn test_evacuate_tenured_skips_unmarked() {
        // TENURED but not MARKED → dead this cycle, sweep handles it.
        // Evacuation must skip.
        CONS_PINNED.with(|s| s.borrow_mut().clear());
        let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
        let header = unsafe { header_from_user_ptr(user) as *mut GcHeader };
        unsafe {
            (*header).gc_flags |= GC_FLAG_TENURED; // no MARK
        }
        let _n = evacuate_tenured_nursery_objects();
        unsafe {
            assert_eq!(
                (*header).gc_flags & GC_FLAG_FORWARDED,
                0,
                "unmarked object must not be evacuated"
            );
        }
        unsafe {
            (*header).gc_flags &= !GC_FLAG_TENURED;
        }
    }

    #[test]
    fn test_evacuate_tenured_marks_forwarded_and_copies_payload() {
        // The happy path: marked + tenured + not pinned → evacuated.
        // Verify (a) GC_FLAG_FORWARDED set on nursery header,
        // (b) forwarding_address points into OLD_ARENA,
        // (c) payload bytes copied.
        CONS_PINNED.with(|s| s.borrow_mut().clear());
        let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
        let header = unsafe { header_from_user_ptr(user) as *mut GcHeader };
        // Write a sentinel pattern into the user payload so we can
        // confirm it survives the copy.
        unsafe {
            let p = user as *mut u64;
            *p = 0xCAFE_BABE_DEAD_BEEF;
            *p.add(1) = 0x1234_5678_9ABC_DEF0;
            (*header).gc_flags |= GC_FLAG_MARKED | GC_FLAG_TENURED;
        }
        let n = evacuate_tenured_nursery_objects();
        assert_eq!(n, 1, "tenured non-pinned marked object must evacuate");
        unsafe {
            assert_ne!((*header).gc_flags & GC_FLAG_FORWARDED, 0);
            let new_user = forwarding_address(header);
            // Verify old_user points into nursery, new_user points into OLD.
            assert!(
                crate::arena::pointer_in_old_gen(new_user as usize),
                "forwarding address should point into OLD_ARENA"
            );
            assert!(
                !crate::arena::pointer_in_old_gen(user as usize),
                "old (nursery) location should NOT be in OLD_ARENA"
            );
            // Verify payload was copied.
            let new_p = new_user as *const u64;
            // Note: payload starts at user_ptr offset 0, but the
            // forwarding write at the OLD slot overwrites the first 8
            // bytes with the new address. So the payload at the OLD
            // location is partially clobbered now — we can only
            // verify the NEW location's payload.
            assert_eq!(*new_p, 0xCAFE_BABE_DEAD_BEEF);
            assert_eq!(*new_p.add(1), 0x1234_5678_9ABC_DEF0);
        }
        unsafe {
            (*header).gc_flags &= !(GC_FLAG_MARKED | GC_FLAG_TENURED);
        }
    }

    #[test]
    fn test_gc_collect_minor_runs_without_panic() {
        // Smoke test: minor GC over an arena with a mix of nursery
        // and old-gen objects must complete without panic. Real
        // correctness is checked by the broader regression suite
        // (test_json_*.ts under PERRY_GEN_GC=1).
        let _y1 = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
        let _y2 = crate::arena::arena_alloc_gc(32, 8, GC_TYPE_STRING);
        let _o1 = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
        let _o2 = crate::arena::arena_alloc_gc_old(48, 8, GC_TYPE_ARRAY);
        let _ = gc_collect_minor();
        // Following collection runs interleave nicely (cleared marks).
        let _ = gc_collect_minor();
        let _ = gc_collect_minor();
    }

    #[test]
    fn test_remembered_set_cleared_after_full_gc() {
        reset_remembered_set();
        // Set up an old→young edge to populate the RS.
        let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        let old = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
        js_write_barrier(POINTER_TAG | (old as u64), POINTER_TAG | (young as u64));
        assert_eq!(remembered_set_size(), 1);
        // Run a full collection.
        let _freed = gc_collect_inner();
        // RS must be empty after collection — coherence invariant.
        assert_eq!(
            remembered_set_size(),
            0,
            "remembered set must be cleared after gc_collect_inner"
        );
    }

    #[test]
    fn test_clear_marks_resets_all() {
        // Allocate and mark some objects
        let ptr1 = gc_malloc(32, GC_TYPE_STRING);
        let ptr2 = gc_malloc(64, GC_TYPE_CLOSURE);

        unsafe {
            (*header_from_user_ptr(ptr1)).gc_flags |= GC_FLAG_MARKED;
            (*header_from_user_ptr(ptr2)).gc_flags |= GC_FLAG_MARKED;
        }

        clear_marks();

        unsafe {
            assert_eq!(
                (*header_from_user_ptr(ptr1)).gc_flags & GC_FLAG_MARKED,
                0,
                "mark should be cleared on ptr1"
            );
            assert_eq!(
                (*header_from_user_ptr(ptr2)).gc_flags & GC_FLAG_MARKED,
                0,
                "mark should be cleared on ptr2"
            );
        }
    }
}
