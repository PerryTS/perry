//! Mark-sweep garbage collector for Perry
//!
//! Design:
//! - 8-byte GcHeader prepended to every heap allocation (invisible to callers)
//! - Arena objects (arrays/objects): discovered by walking arena blocks linearly (zero per-alloc tracking cost)
//! - Malloc objects (strings/closures/promises/bigints/errors): tracked in MALLOC_STATE
//! - Mark phase: precise thread-local roots + optional conservative stack scan + type-specific tracing
//! - Sweep phase: free malloc objects; arena objects added to free list for reuse
//! - Trigger: only checked on new arena block allocation or explicit gc() call

use std::alloc::{alloc, dealloc, realloc, Layout};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex, MutexGuard, OnceLock,
};
use std::time::{Duration, Instant};

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
/// Object's user payload begins with a forwarding address. The new
/// address is stored in the **user-payload's first 8 bytes**
/// (immediately after the GcHeader). Walkers that encounter a
/// FORWARDED header read the forwarding address and follow it;
/// ref-rewrite passes update every NaN-boxed pointer they observe to
/// the forwarded address.
///
/// Two runtime paths use the same bit and payload layout:
/// - GC evacuation/copying stubs are short-lived. Evacuation keeps an
///   explicit list of original nursery headers and clears this bit
///   after owned references have been rewritten/verified, so sweep can
///   reclaim the original slot. Copying nursery stubs disappear when
///   from-space is reset.
/// - Array-growth stubs are intentionally retained. `clean_arr_ptr`
///   follows those chains for stale array references that the runtime
///   cannot rewrite.
///
/// Conservative-stack scans STILL get the old (now-stale) address;
/// objects that might be conservatively referenced are pinned out of
/// the evacuation set via `GC_FLAG_PINNED` to avoid corrupting reads
/// from those words.
///
/// This is the last bit in the u8 gc_flags. Adding more flags
/// requires extending GcHeader (currently 8 bytes total — extending
/// breaks ABI everywhere; deferred until/unless a future phase
/// genuinely needs more bits).
pub const GC_FLAG_FORWARDED: u8 = 0x80;

/// Read the forwarding address embedded in a forwarded object's user
/// payload. Caller must verify `gc_flags & GC_FLAG_FORWARDED` is set;
/// reading otherwise returns garbage. The forwarded address is the
/// **user pointer** of the new location — i.e. what the allocating
/// path returned for the new copy. Callers that need the new GcHeader
/// subtract `GC_HEADER_SIZE` themselves.
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

/// Install a forwarding address in an object's user payload and set
/// `GC_FLAG_FORWARDED` on its header. The first 8 bytes of the user
/// payload become the forwarding pointer (the new user address).
/// Subsequent reads via `forwarding_address` recover the new location.
///
/// GC evacuation must later clear this bit only for the originals it
/// just moved. Array growth uses the same representation but leaves the
/// stub retained so stale array references can continue to resolve via
/// `clean_arr_ptr`.
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

// Pointer-slot layout state stored in the high bits of GcHeader._reserved.
// Low bits remain object freeze/seal/preventExtensions flags.
pub const GC_LAYOUT_STATE_MASK: u16 = 0xC000;
const GC_LAYOUT_UNKNOWN: u16 = 0x0000;
pub const GC_LAYOUT_POINTER_FREE: u16 = 0x4000;
const GC_LAYOUT_SIDE_MASK: u16 = 0x8000;
// Side masks are a win for larger layouts, but a memory tax for the tiny
// mixed objects that dominate JSON churn. Keep small pointer-bearing layouts
// in UNKNOWN state so tracing falls back to the legacy full-slot walk.
const GC_LAYOUT_SIDE_MASK_MIN_SLOTS: usize = 16;

#[derive(Clone)]
enum LayoutSlotMask {
    Inline(u64),
    Heap(Vec<u64>),
}

impl LayoutSlotMask {
    #[inline]
    fn set_slot(&mut self, slot_index: usize) {
        match self {
            LayoutSlotMask::Inline(bits) if slot_index < 64 => {
                *bits |= 1u64 << slot_index;
            }
            LayoutSlotMask::Inline(bits) => {
                let mut words = vec![0; slot_index / 64 + 1];
                words[0] = *bits;
                words[slot_index / 64] |= 1u64 << (slot_index % 64);
                *self = LayoutSlotMask::Heap(words);
            }
            LayoutSlotMask::Heap(words) => {
                let word = slot_index / 64;
                if words.len() <= word {
                    words.resize(word + 1, 0);
                }
                words[word] |= 1u64 << (slot_index % 64);
            }
        }
    }

    #[inline]
    fn clear_slot(&mut self, slot_index: usize) {
        match self {
            LayoutSlotMask::Inline(bits) if slot_index < 64 => {
                *bits &= !(1u64 << slot_index);
            }
            LayoutSlotMask::Inline(_) => {}
            LayoutSlotMask::Heap(words) => {
                let word = slot_index / 64;
                if word < words.len() {
                    words[word] &= !(1u64 << (slot_index % 64));
                    while words.last().copied() == Some(0) {
                        words.pop();
                    }
                    if words.len() == 1 {
                        *self = LayoutSlotMask::Inline(words[0]);
                    }
                }
            }
        }
    }

    #[inline]
    fn is_empty(&self) -> bool {
        match self {
            LayoutSlotMask::Inline(bits) => *bits == 0,
            LayoutSlotMask::Heap(words) => words.iter().all(|&w| w == 0),
        }
    }

    fn visit_slots<F: FnMut(usize)>(&self, slot_count: usize, mut visit: F) {
        match self {
            LayoutSlotMask::Inline(bits) => {
                let limit = slot_count.min(64);
                let mask = if limit == 64 {
                    u64::MAX
                } else if limit == 0 {
                    0
                } else {
                    (1u64 << limit) - 1
                };
                let mut word = *bits & mask;
                while word != 0 {
                    let bit = word.trailing_zeros() as usize;
                    visit(bit);
                    word &= word - 1;
                }
            }
            LayoutSlotMask::Heap(words) => {
                let word_count = slot_count.div_ceil(64);
                for (word_index, &raw_word) in words.iter().take(word_count).enumerate() {
                    let remaining = slot_count.saturating_sub(word_index * 64);
                    let limit = remaining.min(64);
                    let mask = if limit == 64 {
                        u64::MAX
                    } else if limit == 0 {
                        0
                    } else {
                        (1u64 << limit) - 1
                    };
                    let mut word = raw_word & mask;
                    while word != 0 {
                        let bit = word.trailing_zeros() as usize;
                        visit(word_index * 64 + bit);
                        word &= word - 1;
                    }
                }
            }
        }
    }
}

// NaN-boxing tag constants (duplicated from value.rs to avoid circular deps)
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const BIGINT_TAG: u64 = 0x7FFA_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;

pub type MutableRootScanner = for<'a> fn(&mut RuntimeRootVisitor<'a>);

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
    registry_state: MallocRegistryState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MallocRegistryState {
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
        registry_state: MallocRegistryState::Inactive,
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

    /// Legacy Rust root scanners that expose copied f64 values only.
    /// Runtime-owned scanners should use MUTABLE_ROOT_SCANNERS instead.
    static ROOT_SCANNERS: RefCell<Vec<fn(&mut dyn FnMut(f64))>> = RefCell::new(Vec::new());

    /// Registered runtime-owned root slot scanners. These expose mutable
    /// storage so evacuation can rewrite forwarded references in place.
    static MUTABLE_ROOT_SCANNERS: RefCell<Vec<MutableRootScanner>> = RefCell::new(Vec::new());

    /// Registered root scanner functions from perry-ffi/native packages.
    static FFI_ROOT_SCANNERS: RefCell<Vec<PerryFfiRootScanner>> = RefCell::new(Vec::new());

    /// Module-level global variable addresses (registered by codegen)
    static GLOBAL_ROOTS: RefCell<Vec<*mut u64>> = const { RefCell::new(Vec::new()) };

    /// Pointer-slot masks for arrays, object fields/overflow fields, and
    /// closure captures. Keyed by user pointer (payload address after GcHeader).
    static LAYOUT_SLOT_MASKS: RefCell<crate::fast_hash::PtrHashMap<usize, LayoutSlotMask>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());

    #[cfg(test)]
    static TRACE_SLOT_READS: Cell<usize> = const { Cell::new(0) };

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

#[inline]
fn gc_trace_enabled() -> bool {
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        matches!(
            std::env::var("PERRY_GC_TRACE").as_deref(),
            Ok("1") | Ok("on") | Ok("true")
        )
    })
}

#[derive(Clone, Copy)]
enum GcCollectionKind {
    Minor,
    Full,
}

impl GcCollectionKind {
    #[inline]
    fn as_str(self) -> &'static str {
        match self {
            GcCollectionKind::Minor => "minor",
            GcCollectionKind::Full => "full",
        }
    }
}

#[derive(Clone, Copy)]
enum GcTriggerKind {
    ArenaBytes,
    MallocCount,
    Manual,
    Direct,
}

impl GcTriggerKind {
    #[inline]
    fn as_str(self) -> &'static str {
        match self {
            GcTriggerKind::ArenaBytes => "arena_bytes",
            GcTriggerKind::MallocCount => "malloc_count",
            GcTriggerKind::Manual => "manual",
            GcTriggerKind::Direct => "direct",
        }
    }
}

#[derive(Clone, Copy)]
enum DeferredGcRequest {
    None,
    CheckTrigger,
    DirectMinor,
    Collect(GcTriggerKind),
}

impl DeferredGcRequest {
    #[inline]
    fn merge(self, next: DeferredGcRequest) -> DeferredGcRequest {
        use DeferredGcRequest::*;
        match (self, next) {
            (None, request) => request,
            (request, None) => request,
            (Collect(GcTriggerKind::Manual), _) | (_, Collect(GcTriggerKind::Manual)) => {
                Collect(GcTriggerKind::Manual)
            }
            (Collect(kind), _) => Collect(kind),
            (_, Collect(kind)) => Collect(kind),
            (DirectMinor, _) | (_, DirectMinor) => DirectMinor,
            (CheckTrigger, CheckTrigger) => CheckTrigger,
        }
    }
}

#[derive(Clone, Copy)]
struct GcStepSnapshot {
    arena_step_bytes: usize,
    next_arena_trigger_bytes: usize,
    malloc_step: usize,
    next_malloc_trigger: usize,
    trigger_bumped: bool,
}

impl GcStepSnapshot {
    #[inline]
    fn current() -> Self {
        Self {
            arena_step_bytes: GC_STEP_BYTES.with(|c| c.get()),
            next_arena_trigger_bytes: GC_NEXT_TRIGGER_BYTES.with(|c| c.get()),
            malloc_step: GC_MALLOC_COUNT_STEP.with(|c| c.get()),
            next_malloc_trigger: GC_NEXT_MALLOC_TRIGGER.with(|c| c.get()),
            trigger_bumped: GC_TRIGGER_BUMPED.with(|c| c.get()),
        }
    }
}

#[derive(Clone, Copy)]
struct GcTriggerSnapshot {
    kind: GcTriggerKind,
    steps_before: Option<GcStepSnapshot>,
}

impl GcTriggerSnapshot {
    #[inline]
    fn capture(kind: GcTriggerKind) -> Self {
        Self {
            kind,
            steps_before: gc_trace_enabled().then(GcStepSnapshot::current),
        }
    }
}

thread_local! {
    static GC_ROOT_LOCK_DEPTH: Cell<usize> = const { Cell::new(0) };
    static GC_DEFERRED_REQUEST: Cell<DeferredGcRequest> =
        const { Cell::new(DeferredGcRequest::None) };
}

/// Guard returned by `lock_gc_root_registry`.
///
/// The mutex is released before any deferred GC request is flushed. That
/// drop order is what lets scanner-owned registries use ordinary blocking
/// locks in their root scanners: a GC request made while the same mutex is
/// held records pending work, returns immediately, and the final guard drop
/// runs the collection only after the scanner can reacquire the mutex.
pub(crate) struct GcRootRegistryGuard<'a, T> {
    guard: Option<MutexGuard<'a, T>>,
}

impl<T> std::ops::Deref for GcRootRegistryGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.guard
            .as_deref()
            .expect("GC root registry guard missing")
    }
}

impl<T> std::ops::DerefMut for GcRootRegistryGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard
            .as_deref_mut()
            .expect("GC root registry guard missing")
    }
}

impl<T> Drop for GcRootRegistryGuard<'_, T> {
    fn drop(&mut self) {
        drop(self.guard.take());
        exit_gc_root_lock();
    }
}

pub(crate) fn lock_gc_root_registry<T>(mutex: &Mutex<T>) -> GcRootRegistryGuard<'_, T> {
    let guard = mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    enter_gc_root_lock();
    GcRootRegistryGuard { guard: Some(guard) }
}

#[inline]
fn enter_gc_root_lock() {
    GC_ROOT_LOCK_DEPTH.with(|depth| depth.set(depth.get() + 1));
}

fn exit_gc_root_lock() {
    let should_flush = GC_ROOT_LOCK_DEPTH.with(|depth| {
        let current = depth.get();
        debug_assert!(current > 0, "GC root lock depth underflow");
        if current == 0 {
            return false;
        }
        depth.set(current - 1);
        current == 1
    });
    if should_flush {
        flush_deferred_gc_request();
    }
}

#[inline]
fn defer_gc_request(request: DeferredGcRequest) -> bool {
    let locked = GC_ROOT_LOCK_DEPTH.with(|depth| depth.get() != 0);
    if locked {
        GC_DEFERRED_REQUEST.with(|pending| {
            pending.set(pending.get().merge(request));
        });
    }
    locked
}

fn take_deferred_gc_request() -> DeferredGcRequest {
    GC_DEFERRED_REQUEST.with(|pending| {
        let request = pending.get();
        pending.set(DeferredGcRequest::None);
        request
    })
}

fn flush_deferred_gc_request() {
    if std::thread::panicking() {
        let _ = take_deferred_gc_request();
        return;
    }
    match take_deferred_gc_request() {
        DeferredGcRequest::None => {}
        DeferredGcRequest::CheckTrigger => gc_check_trigger(),
        DeferredGcRequest::DirectMinor => {
            gc_collect_minor_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct))
                .emit_after_current();
        }
        DeferredGcRequest::Collect(GcTriggerKind::Manual) => {
            if manual_gc_blocked_by_unsafe_zone() {
                return;
            }
            gc_collect_inner_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Manual))
                .emit_after_current();
        }
        DeferredGcRequest::Collect(kind) => {
            gc_collect_inner_with_trigger(GcTriggerSnapshot::capture(kind)).emit_after_current();
        }
    }
}

#[derive(Clone, Copy, Default)]
struct RememberedSetTraceStats {
    entries_scanned: usize,
    valid_roots: usize,
    newly_marked: usize,
    dirty_pages_before: usize,
    dirty_pages_after: usize,
    dirty_pages_scanned: usize,
    old_objects_considered: usize,
    dirty_objects_scanned: usize,
    dirty_slot_pages_considered: usize,
    dirty_slot_ranges_scanned: usize,
    dirty_slots_scanned: usize,
}

#[derive(Clone, Copy, Default)]
struct BlockPersistTraceStats {
    iterations: usize,
    candidate_blocks: usize,
    live_blocks: usize,
    marked_objects: usize,
}

#[derive(Clone, Copy, Default)]
struct EvacuationTraceStats {
    // Compatibility fields: historically these were the moved counts.
    objects: usize,
    bytes: usize,
    moved_objects: usize,
    moved_bytes: usize,
    released_original_objects: usize,
    released_original_bytes: usize,
    retained_forwarded_stub_objects: usize,
    retained_forwarded_stub_bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CopiedMinorFallbackReason {
    None,
    NotAttempted,
    BarriersInactive,
    ConservativeStack,
    CopyOnlyRoots,
    MallocRegistryUnavailable,
    PinnedYoungRoot,
    PinnedYoungDirtySlot,
    PinnedYoungTransitive,
}

impl CopiedMinorFallbackReason {
    #[inline]
    const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::NotAttempted => "not_attempted",
            Self::BarriersInactive => "barriers_inactive",
            Self::ConservativeStack => "conservative_stack",
            Self::CopyOnlyRoots => "copy_only_roots",
            Self::MallocRegistryUnavailable => "malloc_registry_unavailable",
            Self::PinnedYoungRoot => "pinned_young_root",
            Self::PinnedYoungDirtySlot => "pinned_young_dirty_slot",
            Self::PinnedYoungTransitive => "pinned_young_transitive",
        }
    }
}

impl Default for CopiedMinorFallbackReason {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Clone, Copy, Default)]
struct CopyingNurseryTraceStats {
    eligible: bool,
    copied_objects: usize,
    copied_bytes: usize,
    promoted_objects: usize,
    promoted_bytes: usize,
    reset_blocks: usize,
    malloc_validation_lookups: usize,
    malloc_registry_rebuilds: u64,
    malloc_sweep_due: bool,
    fallback_reason: CopiedMinorFallbackReason,
}

#[derive(Clone, Copy, Default)]
struct LegacyRootTraceStats {
    pinned_roots: usize,
    pinned_bytes: usize,
}

#[derive(Clone, Copy, Default)]
struct ConservativeRootTraceStats {
    root_count: usize,
}

#[derive(Clone, Copy, Default)]
struct ConservativePinTraceStats {
    pinned_roots: usize,
    pinned_bytes: usize,
}

#[derive(Clone, Copy, Default)]
struct ShadowRootTraceStats {
    slots_scanned: usize,
    nonzero_slots: usize,
    pointer_roots: usize,
    rewritten_slots: usize,
}

impl ShadowRootTraceStats {
    fn record_scan(&mut self, bits: u64) {
        self.slots_scanned = self.slots_scanned.saturating_add(1);
        if bits == 0 {
            return;
        }
        self.nonzero_slots = self.nonzero_slots.saturating_add(1);
        if shadow_slot_pointer_root(bits) {
            self.pointer_roots = self.pointer_roots.saturating_add(1);
        }
    }

    fn record_rewrite(&mut self) {
        self.rewritten_slots = self.rewritten_slots.saturating_add(1);
    }
}

const MIN_TENURED_NURSERY_BYTES: usize = 16 * 1024 * 1024;
const MIN_CANDIDATE_BYTES: usize = 8 * 1024 * 1024;
const MIN_CANDIDATE_RATIO_PCT: u64 = 25;
const RSS_PRESSURE_BYTES: u64 = 192 * 1024 * 1024;
const RSS_HARD_PRESSURE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_PREVIOUS_PAUSE_US: u64 = 20_000;

#[derive(Clone, Copy, Default)]
struct EvacuationPolicySnapshot {
    tenured_still_in_nursery_bytes: usize,
    candidate_bytes: usize,
    candidate_objects: usize,
    reclaimable_candidate_bytes: usize,
    reclaimable_candidate_objects: usize,
    retained_forwarded_stub_bytes: usize,
    retained_forwarded_stub_objects: usize,
    conservative_pinned_bytes: usize,
    rss_bytes: u64,
    previous_pause_us: u64,
    pre_evac_pause_us: u64,
}

impl EvacuationPolicySnapshot {
    #[inline]
    fn candidate_ratio_pct(self) -> u64 {
        if self.tenured_still_in_nursery_bytes == 0 {
            return 0;
        }
        ((self.candidate_bytes as u128 * 100) / self.tenured_still_in_nursery_bytes as u128) as u64
    }

    #[inline]
    fn reclaimable_candidate_ratio_pct(self) -> u64 {
        if self.tenured_still_in_nursery_bytes == 0 {
            return 0;
        }
        ((self.reclaimable_candidate_bytes as u128 * 100)
            / self.tenured_still_in_nursery_bytes as u128) as u64
    }
}

#[derive(Clone, Copy)]
struct EvacuationPolicyDecision {
    allowed: bool,
    considered: bool,
    force: bool,
    enabled: bool,
    reason: &'static str,
    snapshot: EvacuationPolicySnapshot,
}

impl Default for EvacuationPolicyDecision {
    fn default() -> Self {
        Self {
            allowed: true,
            considered: false,
            force: false,
            enabled: false,
            reason: "not_evaluated",
            snapshot: EvacuationPolicySnapshot::default(),
        }
    }
}

#[derive(Clone, Copy, Default)]
struct SweepTraceStats {
    freed_bytes: u64,
    reset_blocks: usize,
    deallocated_blocks: usize,
    deallocated_bytes: usize,
    retained_forwarded_stub_objects: usize,
    retained_forwarded_stub_bytes: usize,
}

#[derive(Clone, Copy, Default)]
struct BarrierTraceCounters {
    calls: u64,
    non_pointer_parent_skips: u64,
    non_pointer_child_skips: u64,
    parent_not_old_skips: u64,
    child_not_young_skips: u64,
    remembered_set_insert_attempts: u64,
    new_inserts: u64,
    dirty_page_mark_attempts: u64,
    new_dirty_pages: u64,
    conservative_parent_span_marks: u64,
}

impl BarrierTraceCounters {
    const fn zero() -> Self {
        Self {
            calls: 0,
            non_pointer_parent_skips: 0,
            non_pointer_child_skips: 0,
            parent_not_old_skips: 0,
            child_not_young_skips: 0,
            remembered_set_insert_attempts: 0,
            new_inserts: 0,
            dirty_page_mark_attempts: 0,
            new_dirty_pages: 0,
            conservative_parent_span_marks: 0,
        }
    }
}

#[derive(Clone, Copy)]
enum BarrierTraceCounter {
    Calls,
    NonPointerParentSkips,
    NonPointerChildSkips,
    ParentNotOldSkips,
    ChildNotYoungSkips,
    RememberedSetInsertAttempts,
    NewInserts,
    DirtyPageMarkAttempts,
    NewDirtyPages,
    ConservativeParentSpanMarks,
}

struct GcCycleTrace {
    collection_kind: GcCollectionKind,
    trigger_kind: GcTriggerKind,
    steps_before: GcStepSnapshot,
    pause_us: u64,
    phase_us: BTreeMap<&'static str, u64>,
    arena_before: crate::arena::ArenaTelemetrySnapshot,
    malloc_before: usize,
    remembered_set_before: usize,
    remembered_set: RememberedSetTraceStats,
    conservative_root_count: usize,
    conservative_pinned: usize,
    conservative_pinned_bytes: usize,
    legacy_copy_only_scanner_pinned: LegacyRootTraceStats,
    shadow_roots: ShadowRootTraceStats,
    evacuation_policy: EvacuationPolicyDecision,
    evacuation: EvacuationTraceStats,
    copying_nursery: CopyingNurseryTraceStats,
    block_persist: BlockPersistTraceStats,
    sweep: SweepTraceStats,
    write_barrier: BarrierTraceCounters,
}

impl GcCycleTrace {
    fn new(collection_kind: GcCollectionKind, trigger: GcTriggerSnapshot) -> Option<Self> {
        let steps_before = trigger.steps_before?;
        let mut phase_us = BTreeMap::new();
        for name in [
            "build_valid_pointer_set",
            "root_marking",
            "remembered_set_marking",
            "trace_worklist",
            "block_persistence",
            "evacuation",
            "copying_nursery",
            "reference_rewrite",
            "sweep",
            "remembered_set_clear",
            "conservative_pin_clear",
            "malloc_trim",
        ] {
            phase_us.insert(name, 0);
        }
        Some(Self {
            collection_kind,
            trigger_kind: trigger.kind,
            steps_before,
            pause_us: 0,
            phase_us,
            arena_before: crate::arena::arena_telemetry_snapshot(),
            malloc_before: malloc_object_count(),
            remembered_set_before: remembered_set_size(),
            remembered_set: RememberedSetTraceStats::default(),
            conservative_root_count: 0,
            conservative_pinned: 0,
            conservative_pinned_bytes: 0,
            legacy_copy_only_scanner_pinned: LegacyRootTraceStats::default(),
            shadow_roots: ShadowRootTraceStats::default(),
            evacuation_policy: EvacuationPolicyDecision::default(),
            evacuation: EvacuationTraceStats::default(),
            copying_nursery: CopyingNurseryTraceStats {
                fallback_reason: CopiedMinorFallbackReason::NotAttempted,
                ..CopyingNurseryTraceStats::default()
            },
            block_persist: BlockPersistTraceStats::default(),
            sweep: SweepTraceStats::default(),
            write_barrier: take_write_barrier_trace_counters(),
        })
    }

    #[inline]
    fn record_phase(&mut self, name: &'static str, elapsed: Duration) {
        *self.phase_us.entry(name).or_insert(0) += elapsed.as_micros() as u64;
    }

    fn emit(self, steps_after: GcStepSnapshot) {
        let arena_after = crate::arena::arena_telemetry_snapshot();
        let malloc_after = malloc_object_count();
        let remembered_set_after = remembered_set_size();
        let event = serde_json::json!({
            "event": "gc_cycle",
            "collection_kind": self.collection_kind.as_str(),
            "pause_us": self.pause_us,
            "phase_us": self.phase_us,
            "arena_bytes": {
                "before": arena_snapshot_json(self.arena_before),
                "after": arena_snapshot_json(arena_after),
            },
            "malloc_objects": {
                "before": self.malloc_before,
                "after": malloc_after,
            },
            "remembered_set": {
                "before": self.remembered_set_before,
                "after": remembered_set_after,
                "entries_scanned": self.remembered_set.entries_scanned,
                "valid_roots": self.remembered_set.valid_roots,
                "newly_marked": self.remembered_set.newly_marked,
                "dirty_pages_before": self.remembered_set.dirty_pages_before,
                "dirty_pages_after": remembered_dirty_page_count(),
                "dirty_pages_scanned": self.remembered_set.dirty_pages_scanned,
                "old_objects_considered": self.remembered_set.old_objects_considered,
                "dirty_objects_scanned": self.remembered_set.dirty_objects_scanned,
                "dirty_slot_pages_considered": self.remembered_set.dirty_slot_pages_considered,
                "dirty_slot_ranges_scanned": self.remembered_set.dirty_slot_ranges_scanned,
                "dirty_slots_scanned": self.remembered_set.dirty_slots_scanned,
            },
            "conservative_root_count": self.conservative_root_count,
            "conservative_pinned": self.conservative_pinned,
            "conservative_pinned_bytes": self.conservative_pinned_bytes,
            "legacy_copy_only_scanner_pinned": {
                "roots": self.legacy_copy_only_scanner_pinned.pinned_roots,
                "bytes": self.legacy_copy_only_scanner_pinned.pinned_bytes,
            },
            "shadow_roots": {
                "slots_scanned": self.shadow_roots.slots_scanned,
                "nonzero_slots": self.shadow_roots.nonzero_slots,
                "pointer_roots": self.shadow_roots.pointer_roots,
                "rewritten_slots": self.shadow_roots.rewritten_slots,
            },
            "evacuation": {
                "objects": self.evacuation.objects,
                "bytes": self.evacuation.bytes,
                "moved_objects": self.evacuation.moved_objects,
                "moved_bytes": self.evacuation.moved_bytes,
                "released_original_objects": self.evacuation.released_original_objects,
                "released_original_bytes": self.evacuation.released_original_bytes,
                "retained_forwarded_stub_objects": self.evacuation.retained_forwarded_stub_objects,
                "retained_forwarded_stub_bytes": self.evacuation.retained_forwarded_stub_bytes,
            },
            "copying_nursery": {
                "eligible": self.copying_nursery.eligible,
                "copied_objects": self.copying_nursery.copied_objects,
                "copied_bytes": self.copying_nursery.copied_bytes,
                "promoted_objects": self.copying_nursery.promoted_objects,
                "promoted_bytes": self.copying_nursery.promoted_bytes,
                "reset_blocks": self.copying_nursery.reset_blocks,
                "malloc_validation_lookups": self.copying_nursery.malloc_validation_lookups,
                "malloc_registry_rebuilds": self.copying_nursery.malloc_registry_rebuilds,
                "malloc_sweep_due": self.copying_nursery.malloc_sweep_due,
                "fallback_reason": self.copying_nursery.fallback_reason.as_str(),
            },
            "evacuation_policy": {
                "allowed": self.evacuation_policy.allowed,
                "considered": self.evacuation_policy.considered,
                "force": self.evacuation_policy.force,
                "enabled": self.evacuation_policy.enabled,
                "reason": self.evacuation_policy.reason,
                "tenured_still_in_nursery_bytes": self.evacuation_policy.snapshot.tenured_still_in_nursery_bytes,
                "candidate_bytes": self.evacuation_policy.snapshot.candidate_bytes,
                "candidate_objects": self.evacuation_policy.snapshot.candidate_objects,
                "candidate_ratio_pct": self.evacuation_policy.snapshot.candidate_ratio_pct(),
                "reclaimable_candidate_bytes": self.evacuation_policy.snapshot.reclaimable_candidate_bytes,
                "reclaimable_candidate_objects": self.evacuation_policy.snapshot.reclaimable_candidate_objects,
                "reclaimable_candidate_ratio_pct": self.evacuation_policy.snapshot.reclaimable_candidate_ratio_pct(),
                "retained_forwarded_stub_bytes": self.evacuation_policy.snapshot.retained_forwarded_stub_bytes,
                "retained_forwarded_stub_objects": self.evacuation_policy.snapshot.retained_forwarded_stub_objects,
                "conservative_pinned_bytes": self.evacuation_policy.snapshot.conservative_pinned_bytes,
                "rss_bytes": self.evacuation_policy.snapshot.rss_bytes,
                "previous_pause_us": self.evacuation_policy.snapshot.previous_pause_us,
                "pre_evac_pause_us": self.evacuation_policy.snapshot.pre_evac_pause_us,
            },
            "block_persist": {
                "iterations": self.block_persist.iterations,
                "candidate_blocks": self.block_persist.candidate_blocks,
                "live_blocks": self.block_persist.live_blocks,
                "marked_objects": self.block_persist.marked_objects,
            },
            "sweep": {
                "freed_bytes": self.sweep.freed_bytes,
                "reset_blocks": self.sweep.reset_blocks,
                "deallocated_blocks": self.sweep.deallocated_blocks,
                "deallocated_bytes": self.sweep.deallocated_bytes,
                "retained_forwarded_stub_objects": self.sweep.retained_forwarded_stub_objects,
                "retained_forwarded_stub_bytes": self.sweep.retained_forwarded_stub_bytes,
            },
            "write_barrier": {
                "calls": self.write_barrier.calls,
                "non_pointer_parent_skips": self.write_barrier.non_pointer_parent_skips,
                "non_pointer_child_skips": self.write_barrier.non_pointer_child_skips,
                "parent_not_old_skips": self.write_barrier.parent_not_old_skips,
                "child_not_young_skips": self.write_barrier.child_not_young_skips,
                "remembered_set_insert_attempts": self.write_barrier.remembered_set_insert_attempts,
                "new_inserts": self.write_barrier.new_inserts,
                "dirty_page_mark_attempts": self.write_barrier.dirty_page_mark_attempts,
                "new_dirty_pages": self.write_barrier.new_dirty_pages,
                "conservative_parent_span_marks": self.write_barrier.conservative_parent_span_marks,
            },
            "trigger": {
                "kind": self.trigger_kind.as_str(),
            },
            "steps": steps_json(self.steps_before, steps_after),
        });
        if let Ok(line) = serde_json::to_string(&event) {
            eprintln!("{line}");
        }
    }
}

struct GcCollectOutcome {
    freed_bytes: u64,
    malloc_swept: bool,
    trace: Option<GcCycleTrace>,
}

struct CopiedMinorFastPathOutcome {
    freed_bytes: u64,
    malloc_swept: bool,
}

fn gc_last_pause_us() -> u64 {
    GC_STATS.with(|stats| stats.borrow().last_pause_us)
}

fn evacuation_policy_initial_decision(
    tenured_still_in_nursery_bytes: usize,
    rss_bytes: u64,
    previous_pause_us: u64,
    pre_evac_pause_us: u64,
    allowed: bool,
    force: bool,
    old_to_young_tracking_complete: bool,
) -> EvacuationPolicyDecision {
    let snapshot = EvacuationPolicySnapshot {
        tenured_still_in_nursery_bytes,
        rss_bytes,
        previous_pause_us,
        pre_evac_pause_us,
        ..EvacuationPolicySnapshot::default()
    };
    if !allowed {
        return EvacuationPolicyDecision {
            allowed,
            force,
            reason: "disabled",
            snapshot,
            ..EvacuationPolicyDecision::default()
        };
    }
    if !old_to_young_tracking_complete {
        return EvacuationPolicyDecision {
            allowed,
            force,
            reason: "barriers_inactive",
            snapshot,
            ..EvacuationPolicyDecision::default()
        };
    }
    if force {
        return EvacuationPolicyDecision {
            allowed,
            considered: true,
            force,
            reason: "force_considered",
            snapshot,
            ..EvacuationPolicyDecision::default()
        };
    }
    if tenured_still_in_nursery_bytes >= MIN_TENURED_NURSERY_BYTES {
        return EvacuationPolicyDecision {
            allowed,
            considered: true,
            force,
            reason: "nursery_pressure",
            snapshot,
            ..EvacuationPolicyDecision::default()
        };
    }
    if rss_bytes >= RSS_PRESSURE_BYTES {
        return EvacuationPolicyDecision {
            allowed,
            considered: true,
            force,
            reason: "rss_pressure",
            snapshot,
            ..EvacuationPolicyDecision::default()
        };
    }
    EvacuationPolicyDecision {
        allowed,
        force,
        reason: "low_pressure",
        snapshot,
        ..EvacuationPolicyDecision::default()
    }
}

fn evacuation_policy_snapshot_after_mark(
    mut snapshot: EvacuationPolicySnapshot,
    force: bool,
    pre_evac_pause_us: u64,
) -> EvacuationPolicySnapshot {
    #[derive(Clone, Copy, Default)]
    struct BlockCandidateState {
        candidate_bytes: usize,
        candidate_objects: usize,
        retained_live: bool,
    }

    snapshot.tenured_still_in_nursery_bytes = 0;
    snapshot.candidate_bytes = 0;
    snapshot.candidate_objects = 0;
    snapshot.reclaimable_candidate_bytes = 0;
    snapshot.reclaimable_candidate_objects = 0;
    snapshot.retained_forwarded_stub_bytes = 0;
    snapshot.retained_forwarded_stub_objects = 0;
    snapshot.conservative_pinned_bytes = 0;
    snapshot.pre_evac_pause_us = pre_evac_pause_us;

    let n_blocks = crate::arena::arena_block_count();
    let general_n = crate::arena::general_block_count();
    let mut blocks = vec![BlockCandidateState::default(); n_blocks];

    crate::arena::arena_walk_objects_with_block_index(|header_ptr, block_idx| {
        let header = header_ptr as *mut GcHeader;
        unsafe {
            let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
            if !crate::arena::pointer_in_nursery(user_ptr as usize) {
                return;
            }
            let flags = (*header).gc_flags;
            let total = (*header).size as usize;
            if flags & GC_FLAG_FORWARDED != 0 {
                if block_idx < general_n {
                    snapshot.retained_forwarded_stub_objects += 1;
                    snapshot.retained_forwarded_stub_bytes += total;
                }
                if let Some(block) = blocks.get_mut(block_idx) {
                    block.retained_live = true;
                }
                return;
            }
            let is_tenured = flags & GC_FLAG_TENURED != 0;
            if is_tenured {
                snapshot.tenured_still_in_nursery_bytes += total;
            }
            if flags & GC_FLAG_MARKED == 0 {
                if flags & GC_FLAG_PINNED != 0 {
                    if let Some(block) = blocks.get_mut(block_idx) {
                        block.retained_live = true;
                    }
                }
                return;
            }
            if flags & GC_FLAG_PINNED != 0 {
                if let Some(block) = blocks.get_mut(block_idx) {
                    block.retained_live = true;
                }
                return;
            }
            if is_conservatively_pinned(header) {
                snapshot.conservative_pinned_bytes += total;
                if let Some(block) = blocks.get_mut(block_idx) {
                    block.retained_live = true;
                }
                return;
            }
            if !force && !is_tenured {
                if let Some(block) = blocks.get_mut(block_idx) {
                    block.retained_live = true;
                }
                return;
            }
            snapshot.candidate_objects += 1;
            snapshot.candidate_bytes += total;
            if let Some(block) = blocks.get_mut(block_idx) {
                block.candidate_objects += 1;
                block.candidate_bytes += total;
            }
        }
    });

    for block in blocks.iter().take(general_n) {
        if block.candidate_bytes > 0 && !block.retained_live {
            snapshot.reclaimable_candidate_objects += block.candidate_objects;
            snapshot.reclaimable_candidate_bytes += block.candidate_bytes;
        }
    }
    snapshot
}

fn evacuation_policy_final_decision(
    mut decision: EvacuationPolicyDecision,
    snapshot: EvacuationPolicySnapshot,
) -> EvacuationPolicyDecision {
    decision.snapshot = snapshot;
    decision.enabled = false;
    if !decision.allowed {
        decision.reason = "disabled";
        return decision;
    }
    if !decision.considered {
        decision.reason = "low_pressure";
        return decision;
    }
    if snapshot.candidate_bytes == 0 {
        decision.reason = "zero_candidates";
        return decision;
    }
    if decision.force {
        decision.enabled = true;
        decision.reason = "force";
        return decision;
    }
    if snapshot.reclaimable_candidate_bytes == 0 {
        decision.reason = "zero_reclaimable_candidates";
        return decision;
    }
    if snapshot.reclaimable_candidate_bytes < MIN_CANDIDATE_BYTES {
        decision.reason = "reclaimable_candidate_bytes_below_threshold";
        return decision;
    }
    if snapshot.reclaimable_candidate_ratio_pct() < MIN_CANDIDATE_RATIO_PCT {
        decision.reason = "reclaimable_candidate_ratio_below_threshold";
        return decision;
    }
    let hard_rss_pressure = snapshot.rss_bytes >= RSS_HARD_PRESSURE_BYTES;
    let pause_budget_exceeded = snapshot.previous_pause_us > MAX_PREVIOUS_PAUSE_US
        || snapshot.pre_evac_pause_us > MAX_PREVIOUS_PAUSE_US;
    if pause_budget_exceeded && !hard_rss_pressure {
        decision.reason = "pause_budget_exceeded";
        return decision;
    }
    decision.enabled = true;
    decision.reason = if hard_rss_pressure {
        "rss_hard_pressure"
    } else if snapshot.rss_bytes >= RSS_PRESSURE_BYTES {
        "rss_pressure"
    } else {
        "nursery_pressure"
    };
    decision
}

fn maybe_print_evacuation_policy_diag(
    decision: EvacuationPolicyDecision,
    evacuation: EvacuationTraceStats,
) {
    if std::env::var_os("PERRY_GC_DIAG").is_none() {
        return;
    }
    if !decision.considered && decision.reason != "barriers_inactive" {
        return;
    }
    let snapshot = decision.snapshot;
    eprintln!(
        "[gc-evac-policy] enabled={} reason={} tenured={} candidate_bytes={} candidate_objects={} candidate_ratio_pct={} reclaimable_candidate_bytes={} reclaimable_candidate_objects={} reclaimable_candidate_ratio_pct={} policy_retained_forwarded_stub_bytes={} policy_retained_forwarded_stub_objects={} cons_pinned={} rss={} prev_pause_us={} pre_evac_pause_us={} moved_bytes={} moved_objects={} released_original_bytes={} released_original_objects={} sweep_retained_forwarded_stub_bytes={} sweep_retained_forwarded_stub_objects={}",
        decision.enabled,
        decision.reason,
        snapshot.tenured_still_in_nursery_bytes,
        snapshot.candidate_bytes,
        snapshot.candidate_objects,
        snapshot.candidate_ratio_pct(),
        snapshot.reclaimable_candidate_bytes,
        snapshot.reclaimable_candidate_objects,
        snapshot.reclaimable_candidate_ratio_pct(),
        snapshot.retained_forwarded_stub_bytes,
        snapshot.retained_forwarded_stub_objects,
        snapshot.conservative_pinned_bytes,
        snapshot.rss_bytes,
        snapshot.previous_pause_us,
        snapshot.pre_evac_pause_us,
        evacuation.moved_bytes,
        evacuation.moved_objects,
        evacuation.released_original_bytes,
        evacuation.released_original_objects,
        evacuation.retained_forwarded_stub_bytes,
        evacuation.retained_forwarded_stub_objects,
    );
}

impl GcCollectOutcome {
    #[inline]
    fn emit_after_current(self) -> u64 {
        let Self {
            freed_bytes, trace, ..
        } = self;
        if let Some(trace) = trace {
            trace.emit(GcStepSnapshot::current());
        }
        freed_bytes
    }
}

#[inline]
fn trace_phase_start(trace: &Option<GcCycleTrace>) -> Option<Instant> {
    trace.as_ref().map(|_| Instant::now())
}

#[inline]
fn trace_phase_record(
    trace: &mut Option<GcCycleTrace>,
    name: &'static str,
    start: Option<Instant>,
) {
    if let (Some(trace), Some(start)) = (trace.as_mut(), start) {
        trace.record_phase(name, start.elapsed());
    }
}

#[inline]
fn malloc_object_count() -> usize {
    MALLOC_STATE.with(|s| s.borrow().objects.len())
}

fn arena_region_json(region: crate::arena::ArenaRegionTelemetry) -> serde_json::Value {
    serde_json::json!({
        "in_use_bytes": region.in_use_bytes,
        "reserved_bytes": region.reserved_bytes,
        "block_count": region.block_count,
    })
}

fn arena_snapshot_json(snapshot: crate::arena::ArenaTelemetrySnapshot) -> serde_json::Value {
    serde_json::json!({
        "arena": arena_region_json(snapshot.arena),
        "survivor0": arena_region_json(snapshot.survivor0),
        "survivor1": arena_region_json(snapshot.survivor1),
        "longlived": arena_region_json(snapshot.longlived),
        "old": arena_region_json(snapshot.old),
        "total_in_use_bytes": snapshot.total_in_use_bytes,
        "total_reserved_bytes": snapshot.total_reserved_bytes,
        "total_block_count": snapshot.total_block_count,
    })
}

fn steps_json(before: GcStepSnapshot, after: GcStepSnapshot) -> serde_json::Value {
    serde_json::json!({
        "arena_step_bytes": {
            "before": before.arena_step_bytes,
            "after": after.arena_step_bytes,
        },
        "next_arena_trigger_bytes": {
            "before": before.next_arena_trigger_bytes,
            "after": after.next_arena_trigger_bytes,
        },
        "malloc_step": {
            "before": before.malloc_step,
            "after": after.malloc_step,
        },
        "next_malloc_trigger": {
            "before": before.next_malloc_trigger,
            "after": after.next_malloc_trigger,
        },
        "trigger_bumped": {
            "before": before.trigger_bumped,
            "after": after.trigger_bumped,
        },
    })
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
/// the return value of the matching `js_shadow_frame_push`. Restores
/// the prior `SHADOW.frame_top`.
///
/// Robustness: the bounds check below was previously a `debug_assert!`,
/// which is **compiled out in release builds**. A corrupted / out-of-range
/// `frame_handle` therefore reached `s.stack[base]` unchecked and aborted
/// the entire process with an out-of-bounds panic. This was observed on
/// Windows release builds, where codegen could thread a NaN-boxed value
/// (e.g. boxed `undefined`, `0x7FFC_0000_0000_0001`) into this `extern "C"`
/// argument instead of the small index `js_shadow_frame_push` returned —
/// `js_shadow_frame_pop(9222246136947933185)` → `s.stack[huge]` →
/// hard crash a few seconds into startup. The shadow stack is Phase A
/// (built but not yet consumed by the GC tracer), so skipping a malformed
/// pop is memory-safe and GC-correctness-neutral; aborting the host
/// program is not. Promote the check to a real release-safe guard and
/// bail out — mirrors the bounds checks `js_shadow_slot_set` /
/// `js_shadow_slot_get` already perform on every access.
#[no_mangle]
pub extern "C" fn js_shadow_frame_pop(frame_handle: u64) {
    SHADOW.with(|cell| unsafe {
        let s = &mut *cell.get();
        let base = frame_handle as usize;
        if base + SHADOW_STACK_HEADER_SLOTS > s.stack.len() {
            debug_assert!(false, "shadow-stack pop past end (corrupted frame handle)");
            return;
        }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConservativeStackScanMode {
    Auto,
    Disabled,
    Full,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConservativeStackScanDecision {
    Scan,
    SkipDisabled,
    SkipShadowStackActive,
}

fn conservative_stack_scan_mode_from_value(value: Option<&str>) -> ConservativeStackScanMode {
    let Some(value) = value else {
        return ConservativeStackScanMode::Auto;
    };
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "auto" => ConservativeStackScanMode::Auto,
        "0" | "off" | "false" => ConservativeStackScanMode::Disabled,
        "1" | "on" | "true" | "full" | "debug" => ConservativeStackScanMode::Full,
        _ => ConservativeStackScanMode::Auto,
    }
}

fn conservative_stack_scan_mode() -> ConservativeStackScanMode {
    match std::env::var("PERRY_CONSERVATIVE_STACK_SCAN") {
        Ok(value) => conservative_stack_scan_mode_from_value(Some(&value)),
        Err(_) => ConservativeStackScanMode::Auto,
    }
}

#[inline]
fn shadow_stack_has_active_frame() -> bool {
    SHADOW.with(|cell| unsafe { (*cell.get()).frame_top != usize::MAX })
}

#[inline]
fn conservative_stack_scan_decision_for(
    mode: ConservativeStackScanMode,
    shadow_frame_active: bool,
) -> ConservativeStackScanDecision {
    match mode {
        ConservativeStackScanMode::Disabled => ConservativeStackScanDecision::SkipDisabled,
        ConservativeStackScanMode::Full => ConservativeStackScanDecision::Scan,
        ConservativeStackScanMode::Auto if shadow_frame_active => {
            ConservativeStackScanDecision::SkipShadowStackActive
        }
        ConservativeStackScanMode::Auto => ConservativeStackScanDecision::Scan,
    }
}

fn conservative_stack_scan_decision() -> ConservativeStackScanDecision {
    conservative_stack_scan_decision_for(
        conservative_stack_scan_mode(),
        shadow_stack_has_active_frame(),
    )
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
    fn malloc_registry_available(&self) -> bool {
        self.registry_state == MallocRegistryState::ActiveConsistent
    }
}

thread_local! {
    static MALLOC_REGISTRY_REBUILD_COUNT: Cell<u64> = const { Cell::new(0) };
}

/// Lazily activate `MallocState.set` from `MallocState.objects`.
///
/// Once activated, the registry stays exact: `gc_malloc`,
/// `gc_malloc_batch`, `gc_realloc`, and `sweep_malloc_objects` update it
/// inline. This preserves the malloc hot path for workloads that never need
/// exact validation, while keeping copied-minor from rebuilding the registry
/// during nursery collection.
#[inline]
fn ensure_set_built(s: &mut MallocState) {
    if s.malloc_registry_available() {
        return;
    }
    s.set.clear();
    s.set.extend(s.objects.iter().map(|&h| h as usize));
    s.registry_state = MallocRegistryState::ActiveConsistent;
    MALLOC_REGISTRY_REBUILD_COUNT.with(|c| c.set(c.get().saturating_add(1)));
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
/// This legacy API exposes copied values only. When evacuation is
/// enabled, every discovered target is treated as pinned because the GC
/// has no mutable slot it can rewrite after forwarding.
pub fn gc_register_root_scanner(scanner: fn(&mut dyn FnMut(f64))) {
    ROOT_SCANNERS.with(|scanners| {
        scanners.borrow_mut().push(scanner);
    });
}

/// Register a runtime-owned root scanner that exposes mutable slots.
/// These scanners are marked like ordinary roots, but their storage is
/// revisited after evacuation so forwarded references can be rewritten.
pub fn gc_register_mutable_root_scanner(scanner: MutableRootScanner) {
    MUTABLE_ROOT_SCANNERS.with(|scanners| {
        scanners.borrow_mut().push(scanner);
    });
}

type PerryFfiRootMarker = extern "C" fn(value: f64, ctx: *mut c_void);
type PerryFfiRootScanner = extern "C" fn(mark: PerryFfiRootMarker, ctx: *mut c_void);

/// Register a native-package root scanner through a stable C ABI.
///
/// `perry-ffi` adapts its Rust-facing `fn(&mut dyn FnMut(f64))`
/// convenience API to this callback shape so native wrapper archives
/// can stay runtime-free. Like the Rust legacy scanner API, this is
/// copy-only storage from the GC's perspective; evacuation pins those
/// roots instead of attempting to rewrite native-owned slots.
#[no_mangle]
pub extern "C" fn perry_ffi_gc_register_root_scanner(scanner: PerryFfiRootScanner) {
    FFI_ROOT_SCANNERS.with(|scanners| {
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
    if defer_gc_request(DeferredGcRequest::CheckTrigger) {
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
        let outcome =
            gc_collect_inner_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::ArenaBytes));
        let sweep_freed_bytes = outcome.freed_bytes;
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
        // Rebaseline the malloc-count trigger only if this collection
        // actually swept malloc objects. Copied-minor arena collections
        // may skip the malloc sweep while count pressure is still below
        // its trigger; moving the trigger in that case would postpone
        // reclamation of already-tracked dead malloc churn.
        if outcome.malloc_swept {
            let survivors = MALLOC_STATE.with(|s| s.borrow().objects.len());
            let mstep = GC_MALLOC_COUNT_STEP.with(|c| c.get());
            GC_NEXT_MALLOC_TRIGGER.with(|c| c.set(survivors + mstep));
        }
        outcome.emit_after_current();
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
        let outcome =
            gc_collect_inner_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::MallocCount));
        debug_assert!(
            outcome.malloc_swept,
            "malloc-count trigger must sweep malloc objects"
        );
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
        if outcome.malloc_swept {
            GC_NEXT_MALLOC_TRIGGER.with(|c| c.set(survivors + mstep));
        }
        outcome.emit_after_current();
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
    if manual_gc_blocked_by_unsafe_zone() {
        return;
    }
    if defer_gc_request(DeferredGcRequest::Collect(GcTriggerKind::Manual)) {
        return;
    }
    gc_collect_inner_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Manual))
        .emit_after_current();
}

fn manual_gc_blocked_by_unsafe_zone() -> bool {
    if GC_UNSAFE_ZONES.load(std::sync::atomic::Ordering::Acquire) <= 0 {
        return false;
    }
    unsafe_zone_manual_gc_warning();
    true
}

fn unsafe_zone_manual_gc_warning() {
    if !GC_UNSAFE_WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        // One-shot warning — user likely has `setInterval(() => gc(), N)`
        // in a server; we don't want to print every 30s.
        eprintln!(
            "perry: gc() skipped — a tokio-based server (WebSocket/HTTP) is active \
             and may hold JSValue refs on worker threads that the main-thread GC \
             can't see. Manual gc() is a no-op for the rest of this process."
        );
    }
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
/// - Precise mutable roots (shadow stack, globals, runtime scanners)
///   keep compiled-frame values live. The conservative C-stack scan
///   is a fallback for non-shadow-stack runtime frames and debug
///   bisects (`PERRY_CONSERVATIVE_STACK_SCAN=full`).
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
/// Enabled by default via `gen_gc_enabled()`. Set `PERRY_GEN_GC=0`,
/// `=false`, or `=off` to route collection through the full mark-sweep
/// path for GC bisection.
pub fn gc_collect_minor() -> u64 {
    if defer_gc_request(DeferredGcRequest::DirectMinor) {
        return 0;
    }
    gc_collect_minor_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct))
        .emit_after_current()
}

fn gc_collect_minor_with_trigger(trigger: GcTriggerSnapshot) -> GcCollectOutcome {
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
    let mut trace = GcCycleTrace::new(GcCollectionKind::Minor, trigger);
    let start = Instant::now();
    let previous_pause_us = gc_last_pause_us();
    let current_rss_bytes = crate::process::get_rss_bytes();
    let evacuation_policy_allowed = gen_gc_evacuate_enabled();
    let force_evacuation = gc_force_evacuate_enabled();
    // MARK_SEEDS persists across GC cycles. Clear before any try_mark
    // call so trace sees only this cycle's freshly-marked headers.
    clear_mark_seeds();
    if let Some(fast_path) = gc_collect_minor_copying_fast_path(&mut trace, start, trigger.kind) {
        let freed_bytes = fast_path.freed_bytes;
        let elapsed_us = start.elapsed().as_micros() as u64;
        GC_STATS.with(|stats| {
            let mut stats = stats.borrow_mut();
            stats.collection_count += 1;
            stats.total_freed_bytes += freed_bytes;
            stats.last_pause_us = elapsed_us;
        });
        GC_FLAGS.with(|f| {
            let cur = f.get();
            if prev_in_alloc != 0 {
                f.set(cur | GC_FLAG_IN_ALLOC);
            } else {
                f.set(cur & !GC_FLAG_IN_ALLOC);
            }
        });
        if let Some(trace) = trace.as_mut() {
            trace.pause_us = elapsed_us;
        }
        return GcCollectOutcome {
            freed_bytes,
            malloc_swept: fast_path.malloc_swept,
            trace,
        };
    }
    clear_mark_seeds();
    let phase_start = trace_phase_start(&trace);
    let valid_ptrs = build_valid_pointer_set();
    trace_phase_record(&mut trace, "build_valid_pointer_set", phase_start);
    let mut evacuation_policy = evacuation_policy_initial_decision(
        valid_ptrs.tenured_nursery_bytes(),
        current_rss_bytes,
        previous_pause_us,
        start.elapsed().as_micros() as u64,
        evacuation_policy_allowed,
        force_evacuation,
        old_to_young_tracking_complete(),
    );
    if let Some(trace) = trace.as_mut() {
        trace.evacuation_policy = evacuation_policy;
    }

    // === MARK PHASE (minor) ===
    // Order matters for the C4b pinning policy:
    //
    //   1. Optional conservative C-stack/register scan first. Those
    //      words cannot be rewritten, so when evacuation is enabled
    //      we pin objects discovered by this phase before any
    //      rewriteable root source can add marks. Default `auto`
    //      mode skips this scan while a precise shadow-stack frame is
    //      active; `PERRY_CONSERVATIVE_STACK_SCAN=full` restores the
    //      legacy always-scan fallback.
    //   2. Mutable root slots (shadow stack + registered globals).
    //      These are real slots we can rewrite after forwarding, so
    //      they stay out of CONS_PINNED.
    //   3. Mutable registered scanners. These expose runtime-owned
    //      slots and are revisited by the forwarding rewrite pass, so
    //      they also stay out of CONS_PINNED.
    //   4. Legacy Rust/FFI scanners. Their API exposes copied f64
    //      values only; when evacuation is enabled the scanner
    //      callbacks pin each discovery directly.
    //
    // Pinning only root-direct discoveries keeps heap-field reachability
    // movable: heap fields are handled later by the reference-rewrite
    // pass.
    let phase_start = trace_phase_start(&trace);
    let conservative_root_stats = mark_stack_roots(&valid_ptrs);
    // CONS_PINNED is only consumed by `evacuate_tenured_nursery_objects`.
    // Stage 1 keeps the low-pressure path from doing the pinning walk.
    let consider_evacuation = evacuation_policy.considered;
    let conservative_pin_stats = if consider_evacuation {
        pin_currently_marked_as_conservative()
    } else {
        ConservativePinTraceStats::default()
    };
    mark_mutable_root_slots(
        &valid_ptrs,
        trace.as_mut().map(|trace| &mut trace.shadow_roots),
    );
    mark_mutable_registered_roots(&valid_ptrs);
    let legacy_root_stats = mark_registered_roots(&valid_ptrs, consider_evacuation);
    if let Some(trace) = trace.as_mut() {
        trace.conservative_root_count = conservative_root_stats.root_count;
        trace.conservative_pinned = conservative_pin_stats.pinned_roots;
        trace.conservative_pinned_bytes = conservative_pin_stats.pinned_bytes;
        trace.legacy_copy_only_scanner_pinned = legacy_root_stats;
    }
    trace_phase_record(&mut trace, "root_marking", phase_start);
    let phase_start = trace_phase_start(&trace);
    let remembered_set = mark_remembered_set_roots(&valid_ptrs);
    trace_phase_record(&mut trace, "remembered_set_marking", phase_start);
    if let Some(trace) = trace.as_mut() {
        trace.remembered_set = remembered_set;
    }
    let phase_start = trace_phase_start(&trace);
    trace_marked_objects_minor(&valid_ptrs);
    trace_phase_record(&mut trace, "trace_worklist", phase_start);
    let phase_start = trace_phase_start(&trace);
    let block_persist = mark_block_persisting_arena_objects(&valid_ptrs);
    trace_phase_record(&mut trace, "block_persistence", phase_start);
    if let Some(trace) = trace.as_mut() {
        trace.block_persist = block_persist;
    }
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

    // === EVACUATION PASS (Phase C4b-β + C4b-γ-2, auto-policy) ===
    // Copy productive sets of non-pinned tenured nursery objects into
    // OLD_ARENA and install short-lived forwarding pointers in the
    // original nursery slots. After owned references are rewritten and
    // optionally verified, those original stubs release FORWARDED so sweep
    // can reclaim them. Stage 2 runs after mark/trace/block-persist so
    // the policy uses measured movable bytes, block-reclaimable candidate
    // bytes, retained forwarded stubs, pinned bytes, RSS, and pause
    // telemetry instead of a simple env-var opt-in.
    if evacuation_policy.considered {
        let snapshot = evacuation_policy_snapshot_after_mark(
            evacuation_policy.snapshot,
            evacuation_policy.force,
            start.elapsed().as_micros() as u64,
        );
        evacuation_policy = evacuation_policy_final_decision(evacuation_policy, snapshot);
    } else {
        evacuation_policy.snapshot.pre_evac_pause_us = start.elapsed().as_micros() as u64;
    }
    if let Some(trace) = trace.as_mut() {
        trace.evacuation_policy = evacuation_policy;
    }
    let mut evacuation = EvacuationTraceStats::default();
    let mut evacuation_sticky = StickyRememberedSet::default();
    if evacuation_policy.enabled {
        let phase_start = trace_phase_start(&trace);
        let mut evacuated_new_headers = Vec::new();
        let mut evacuated_original_headers = Vec::new();
        evacuation = evacuate_tenured_nursery_objects_collecting(
            evacuation_policy.force,
            &mut evacuated_new_headers,
            &mut evacuated_original_headers,
        );
        trace_phase_record(&mut trace, "evacuation", phase_start);
        if evacuation.objects > 0 {
            let phase_start = trace_phase_start(&trace);
            rewrite_forwarded_references(
                &valid_ptrs,
                trace.as_mut().map(|trace| &mut trace.shadow_roots),
            );
            evacuation_sticky =
                rebuild_evacuated_old_to_young_remembered_set(&evacuated_new_headers);
            trace_phase_record(&mut trace, "reference_rewrite", phase_start);
            if gc_verify_evacuation_enabled() {
                let phase_start = trace_phase_start(&trace);
                verify_evacuated_no_stale_forwarded_refs(&valid_ptrs);
                trace_phase_record(&mut trace, "evacuation_verify", phase_start);
            }
            let released = release_evacuated_original_forwarding_stubs(&evacuated_original_headers);
            evacuation.released_original_objects = released.released_original_objects;
            evacuation.released_original_bytes = released.released_original_bytes;
        }
    }

    // === SWEEP PHASE ===
    // `do_age_bump = true` folds the per-object HAS_SURVIVED / TENURED
    // update into this same walk (see comment block above the removed
    // dedicated age-bump pass).
    let phase_start = trace_phase_start(&trace);
    let sweep = sweep_with_age_bump(true);
    trace_phase_record(&mut trace, "sweep", phase_start);
    let freed_bytes = sweep.freed_bytes;
    evacuation.retained_forwarded_stub_objects = sweep.retained_forwarded_stub_objects;
    evacuation.retained_forwarded_stub_bytes = sweep.retained_forwarded_stub_bytes;
    maybe_print_evacuation_policy_diag(evacuation_policy, evacuation);
    if let Some(trace) = trace.as_mut() {
        trace.evacuation = evacuation;
        trace.sweep = sweep;
    }

    // RS clear — see gc_collect_inner for the rationale.
    let phase_start = trace_phase_start(&trace);
    remembered_set_clear();
    evacuation_sticky.restore();
    trace_phase_record(&mut trace, "remembered_set_clear", phase_start);
    // Conservative-pinning is per-cycle; clear so next cycle
    // re-discovers fresh.
    let phase_start = trace_phase_start(&trace);
    CONS_PINNED.with(|s| s.borrow_mut().clear());
    trace_phase_record(&mut trace, "conservative_pin_clear", phase_start);

    #[cfg(target_env = "gnu")]
    {
        let phase_start = trace_phase_start(&trace);
        unsafe {
            libc::malloc_trim(0);
        }
        trace_phase_record(&mut trace, "malloc_trim", phase_start);
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
    if let Some(trace) = trace.as_mut() {
        trace.pause_us = elapsed_us;
    }
    GcCollectOutcome {
        freed_bytes,
        malloc_swept: true,
        trace,
    }
}

#[inline]
fn copied_minor_malloc_sweep_due(trigger_kind: GcTriggerKind) -> bool {
    matches!(trigger_kind, GcTriggerKind::MallocCount)
        || malloc_object_count() >= GC_NEXT_MALLOC_TRIGGER.with(|c| c.get())
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
/// pointer infrastructure, conservative-pinning safety, policy-gated
/// evacuation, reference rewriting,
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

/// Gen-GC Phase C4b: evacuation is policy-driven by default.
/// `PERRY_GEN_GC_EVACUATE=0`, `=false`, or `=off` disables the
/// policy. `=1`, `=true`, and `=on` are accepted for compatibility
/// but mean "allow the auto-policy", not unconditional evacuation.
pub fn gen_gc_evacuate_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_GEN_GC_EVACUATE").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

fn gc_force_evacuate_enabled() -> bool {
    gen_gc_evacuate_enabled()
        && matches!(
            std::env::var("PERRY_GC_FORCE_EVACUATE").as_deref(),
            Ok("1") | Ok("on") | Ok("true")
        )
}

fn gc_verify_evacuation_enabled() -> bool {
    matches!(
        std::env::var("PERRY_GC_VERIFY_EVACUATION").as_deref(),
        Ok("1") | Ok("on") | Ok("true")
    )
}

#[cfg(test)]
fn gc_collect_inner() -> u64 {
    if defer_gc_request(DeferredGcRequest::Collect(GcTriggerKind::Direct)) {
        return 0;
    }
    gc_collect_inner_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct))
        .emit_after_current()
}

fn gc_collect_inner_with_trigger(trigger: GcTriggerSnapshot) -> GcCollectOutcome {
    // Issue #745: clear the per-cycle bytes-bump flag so the next
    // gc-suppressed parse can rebaseline the trigger again. Done at
    // the top so all entry points — full GC, minor GC, manual
    // `gc()`, the malloc-count trigger path — keep the flag in sync.
    GC_TRIGGER_BUMPED.with(|c| c.set(false));
    if gen_gc_enabled() {
        return gc_collect_minor_with_trigger(trigger);
    }
    let mut trace = GcCycleTrace::new(GcCollectionKind::Full, trigger);
    let start = Instant::now();

    // MARK_SEEDS persists across GC cycles. Clear before any try_mark
    // call so trace sees only this cycle's freshly-marked headers.
    clear_mark_seeds();
    // Build set of valid heap pointers for conservative stack scan validation
    let phase_start = trace_phase_start(&trace);
    let valid_ptrs = build_valid_pointer_set();
    trace_phase_record(&mut trace, "build_valid_pointer_set", phase_start);

    // === MARK PHASE ===

    // 1. Optional conservative stack scan. Default `auto` mode skips
    // this while a precise shadow-stack frame is active; the fallback
    // remains available with `PERRY_CONSERVATIVE_STACK_SCAN=full`.
    let phase_start = trace_phase_start(&trace);
    let conservative_root_stats = mark_stack_roots(&valid_ptrs);

    // 2. Scan mutable roots (shadow stack + registered globals)
    mark_mutable_root_slots(
        &valid_ptrs,
        trace.as_mut().map(|trace| &mut trace.shadow_roots),
    );

    // 3. Run runtime-owned mutable scanners, then legacy copy-only scanners.
    mark_mutable_registered_roots(&valid_ptrs);
    let legacy_root_stats = mark_registered_roots(&valid_ptrs, false);
    if let Some(trace) = trace.as_mut() {
        trace.conservative_root_count = conservative_root_stats.root_count;
        trace.legacy_copy_only_scanner_pinned = legacy_root_stats;
    }
    trace_phase_record(&mut trace, "root_marking", phase_start);

    // 3b. Gen-GC Phase C3: scan remembered set as additional roots.
    //     Old-gen objects that wrote young-gen pointers since the
    //     last collection are recorded here by the write barrier
    //     (gen-gc-plan.md §C). For full GC this is redundant with
    //     the conservative+precise scan that already covered them,
    //     but it's cheap and keeps the dispatch path uniform with
    //     the eventual minor-GC entry. RS is cleared at the end of
    //     collection so the next cycle starts coherent.
    let phase_start = trace_phase_start(&trace);
    let remembered_set = mark_remembered_set_roots(&valid_ptrs);
    trace_phase_record(&mut trace, "remembered_set_marking", phase_start);
    if let Some(trace) = trace.as_mut() {
        trace.remembered_set = remembered_set;
    }

    // 4. Trace from marked roots (iterative worklist)
    let phase_start = trace_phase_start(&trace);
    trace_marked_objects(&valid_ptrs);
    trace_phase_record(&mut trace, "trace_worklist", phase_start);

    // 5. Block-persistence pass: arena blocks survive whole or not at all, so
    //    arena objects sharing a block with a root-reachable object persist
    //    even when not themselves reachable. Their malloc children must stay
    //    alive too (issues #43 / #44).
    let phase_start = trace_phase_start(&trace);
    let block_persist = mark_block_persisting_arena_objects(&valid_ptrs);
    trace_phase_record(&mut trace, "block_persistence", phase_start);
    if let Some(trace) = trace.as_mut() {
        trace.block_persist = block_persist;
    }

    // === SWEEP PHASE ===
    // The sweep walk clears mark bits on surviving objects inline,
    // eliminating 2 redundant heap walks (arena + malloc).
    let phase_start = trace_phase_start(&trace);
    let sweep = sweep_with_age_bump(false);
    trace_phase_record(&mut trace, "sweep", phase_start);
    let freed_bytes = sweep.freed_bytes;
    if let Some(trace) = trace.as_mut() {
        trace.sweep = sweep;
    }

    // Gen-GC Phase C3: clear the remembered set after sweep. The
    // RS records old→young writes since the previous collection;
    // after a full collection, every young object referenced by
    // an old-gen parent has either been kept alive (via the
    // mark_remembered_set_roots scan above) or is dead and gets
    // swept. Either way the parent's RS entry is no longer
    // load-bearing — the next allocation cycle's barrier emissions
    // will repopulate it as needed.
    let phase_start = trace_phase_start(&trace);
    remembered_set_clear();
    trace_phase_record(&mut trace, "remembered_set_clear", phase_start);

    // Return released glibc heap pages to the kernel. Without this, glibc
    // keeps freed memory in its arena for reuse but never shrinks RSS, so
    // long-running services show unbounded RSS growth from transient
    // allocations (HTTP buffers, JSON parsers, etc.) even though the
    // Perry GC successfully frees the underlying objects.
    // No-op on non-glibc platforms (macOS, musl).
    #[cfg(target_env = "gnu")]
    {
        let phase_start = trace_phase_start(&trace);
        unsafe {
            libc::malloc_trim(0);
        }
        trace_phase_record(&mut trace, "malloc_trim", phase_start);
    }

    let elapsed_us = start.elapsed().as_micros() as u64;

    GC_STATS.with(|stats| {
        let mut stats = stats.borrow_mut();
        stats.collection_count += 1;
        stats.total_freed_bytes += freed_bytes;
        stats.last_pause_us = elapsed_us;
    });
    if let Some(trace) = trace.as_mut() {
        trace.pause_us = elapsed_us;
    }
    GcCollectOutcome {
        freed_bytes,
        malloc_swept: true,
        trace,
    }
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
    /// Bytes of logically tenured objects that are still physically
    /// resident in nursery blocks at collection entry. Populated while
    /// building the pointer set so evacuation policy Stage 1 doesn't
    /// need a second full arena walk on low-pressure cycles.
    tenured_nursery_bytes: usize,
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
            tenured_nursery_bytes: 0,
        }
    }
    /// Caller must guarantee that pushes happen in ascending address
    /// order — `build_valid_pointer_set` does so via
    /// `arena_walk_objects_addr_sorted`.
    fn push_arena(&mut self, ptr: usize) {
        self.arena_sorted.push(ptr);
    }
    fn record_tenured_nursery_bytes(&mut self, bytes: usize) {
        self.tenured_nursery_bytes += bytes;
    }
    fn tenured_nursery_bytes(&self) -> usize {
        self.tenured_nursery_bytes
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
        unsafe {
            let header = header_ptr as *const GcHeader;
            let flags = (*header).gc_flags;
            if flags & GC_FLAG_TENURED != 0
                && flags & GC_FLAG_FORWARDED == 0
                && crate::arena::pointer_in_nursery(user_ptr as usize)
            {
                set.record_tenured_nursery_bytes((*header).size as usize);
            }
        }
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

#[inline]
unsafe fn set_layout_state(header: *mut GcHeader, state: u16) {
    (*header)._reserved =
        ((*header)._reserved & !GC_LAYOUT_STATE_MASK) | (state & GC_LAYOUT_STATE_MASK);
}

#[inline]
fn strip_nanbox_user_ptr(bits: u64) -> usize {
    if (bits >> 48) >= 0x7FF8 {
        (bits & POINTER_MASK) as usize
    } else {
        bits as usize
    }
}

#[inline]
fn layout_pointer_bearing_bits(bits: u64) -> bool {
    let tag = bits & TAG_MASK;
    if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
        return bits & POINTER_MASK != 0;
    }
    if tag >= 0x7FF8_0000_0000_0000 {
        return false;
    }
    (0x1000..=POINTER_MASK).contains(&bits) && (bits & 0x7) == 0
}

#[inline]
unsafe fn layout_header_for_user(user_ptr: usize) -> Option<*mut GcHeader> {
    if user_ptr < GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let header = header_from_user_ptr(user_ptr as *const u8);
    let obj_type = (*header).obj_type;
    matches!(obj_type, GC_TYPE_ARRAY | GC_TYPE_OBJECT | GC_TYPE_CLOSURE).then_some(header)
}

unsafe fn layout_slot_capacity_for_user(header: *const GcHeader, user_ptr: usize) -> usize {
    match (*header).obj_type {
        GC_TYPE_ARRAY => (*(user_ptr as *const crate::array::ArrayHeader)).length as usize,
        GC_TYPE_OBJECT => (*(user_ptr as *const crate::object::ObjectHeader)).field_count as usize,
        GC_TYPE_CLOSURE => crate::closure::real_capture_count(
            (*(user_ptr as *const crate::closure::ClosureHeader)).capture_count,
        ) as usize,
        _ => 0,
    }
}

#[inline]
unsafe fn layout_side_mask_worth_tracking(
    header: *const GcHeader,
    user_ptr: usize,
    slot_index: usize,
) -> bool {
    slot_index >= GC_LAYOUT_SIDE_MASK_MIN_SLOTS
        || layout_slot_capacity_for_user(header, user_ptr) >= GC_LAYOUT_SIDE_MASK_MIN_SLOTS
}

pub(crate) unsafe fn layout_init_pointer_free(user_ptr: *mut u8) {
    let Some(header) = layout_header_for_user(user_ptr as usize) else {
        return;
    };
    set_layout_state(header, GC_LAYOUT_POINTER_FREE);
    LAYOUT_SLOT_MASKS.with(|m| {
        m.borrow_mut().remove(&(user_ptr as usize));
    });
}

pub(crate) unsafe fn layout_mark_unknown(user_ptr: *mut u8) {
    let Some(header) = layout_header_for_user(user_ptr as usize) else {
        return;
    };
    set_layout_state(header, GC_LAYOUT_UNKNOWN);
    LAYOUT_SLOT_MASKS.with(|m| {
        m.borrow_mut().remove(&(user_ptr as usize));
    });
}

pub(crate) fn layout_clear_for_ptr(user_ptr: usize) {
    if user_ptr == 0 {
        return;
    }
    LAYOUT_SLOT_MASKS.with(|m| {
        m.borrow_mut().remove(&user_ptr);
    });
}

pub(crate) fn layout_note_slot(parent_user: usize, slot_index: usize, value_bits: u64) {
    if slot_index > 16_000_000 {
        return;
    }
    unsafe {
        let Some(header) = layout_header_for_user(parent_user) else {
            return;
        };
        if (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
            let new_user = forwarding_address(header) as usize;
            if new_user != 0 && new_user != parent_user {
                layout_note_slot(new_user, slot_index, value_bits);
            }
            return;
        }
        if (*header)._reserved & GC_LAYOUT_STATE_MASK == GC_LAYOUT_UNKNOWN {
            return;
        }
        let pointer = layout_pointer_bearing_bits(value_bits);
        if !pointer && (*header)._reserved & GC_LAYOUT_STATE_MASK == GC_LAYOUT_POINTER_FREE {
            return;
        }
        LAYOUT_SLOT_MASKS.with(|m| {
            let mut masks = m.borrow_mut();
            if pointer {
                if let Some(mask) = masks.get_mut(&parent_user) {
                    mask.set_slot(slot_index);
                } else if (*header)._reserved & GC_LAYOUT_STATE_MASK == GC_LAYOUT_POINTER_FREE
                    && layout_side_mask_worth_tracking(header, parent_user, slot_index)
                {
                    let mut mask = LayoutSlotMask::Inline(0);
                    mask.set_slot(slot_index);
                    masks.insert(parent_user, mask);
                    set_layout_state(header, GC_LAYOUT_SIDE_MASK);
                } else {
                    set_layout_state(header, GC_LAYOUT_UNKNOWN);
                }
            } else if let Some(mask) = masks.get_mut(&parent_user) {
                mask.clear_slot(slot_index);
                if mask.is_empty() {
                    masks.remove(&parent_user);
                    set_layout_state(header, GC_LAYOUT_POINTER_FREE);
                }
            }
        });
    }
}

#[no_mangle]
pub extern "C" fn js_gc_note_slot_layout(parent: u64, slot_index: u32, value_bits: u64) {
    let parent_user = strip_nanbox_user_ptr(parent);
    layout_note_slot(parent_user, slot_index as usize, value_bits);
}

pub(crate) unsafe fn layout_rebuild_from_slots(
    user_ptr: *mut u8,
    slots: *const u64,
    slot_count: usize,
) {
    let Some(header) = layout_header_for_user(user_ptr as usize) else {
        return;
    };
    if slots.is_null() || slot_count == 0 {
        set_layout_state(header, GC_LAYOUT_POINTER_FREE);
        LAYOUT_SLOT_MASKS.with(|m| {
            m.borrow_mut().remove(&(user_ptr as usize));
        });
        return;
    }

    let mut mask = if slot_count <= 64 {
        LayoutSlotMask::Inline(0)
    } else {
        LayoutSlotMask::Heap(vec![0; slot_count.div_ceil(64)])
    };
    for i in 0..slot_count {
        if layout_pointer_bearing_bits(*slots.add(i)) {
            mask.set_slot(i);
        }
    }

    if mask.is_empty() {
        set_layout_state(header, GC_LAYOUT_POINTER_FREE);
        LAYOUT_SLOT_MASKS.with(|m| {
            m.borrow_mut().remove(&(user_ptr as usize));
        });
    } else if slot_count < GC_LAYOUT_SIDE_MASK_MIN_SLOTS {
        set_layout_state(header, GC_LAYOUT_UNKNOWN);
        LAYOUT_SLOT_MASKS.with(|m| {
            m.borrow_mut().remove(&(user_ptr as usize));
        });
    } else {
        set_layout_state(header, GC_LAYOUT_SIDE_MASK);
        LAYOUT_SLOT_MASKS.with(|m| {
            m.borrow_mut().insert(user_ptr as usize, mask);
        });
    }
}

pub(crate) unsafe fn layout_transfer(old_user: *mut u8, new_user: *mut u8) {
    if old_user.is_null() || new_user.is_null() || old_user == new_user {
        return;
    }
    let Some(old_header) = layout_header_for_user(old_user as usize) else {
        return;
    };
    let Some(new_header) = layout_header_for_user(new_user as usize) else {
        return;
    };
    let state = (*old_header)._reserved & GC_LAYOUT_STATE_MASK;
    set_layout_state(new_header, state);
    LAYOUT_SLOT_MASKS.with(|m| {
        let mut masks = m.borrow_mut();
        masks.remove(&(new_user as usize));
        if let Some(mask) = masks.remove(&(old_user as usize)) {
            masks.insert(new_user as usize, mask);
        }
    });
}

fn layout_visit_pointer_slots<F: FnMut(usize)>(
    user_ptr: usize,
    slot_count: usize,
    mut visit: F,
) -> bool {
    unsafe {
        let Some(header) = layout_header_for_user(user_ptr) else {
            return false;
        };
        match (*header)._reserved & GC_LAYOUT_STATE_MASK {
            GC_LAYOUT_POINTER_FREE => true,
            GC_LAYOUT_SIDE_MASK => {
                let mask = LAYOUT_SLOT_MASKS.with(|m| m.borrow().get(&user_ptr).cloned());
                let Some(mask) = mask else {
                    return false;
                };
                mask.visit_slots(slot_count, &mut visit);
                true
            }
            _ => false,
        }
    }
}

pub(crate) fn layout_visit_pointer_slots_for_user<F: FnMut(usize)>(
    user_ptr: usize,
    slot_count: usize,
    visit: F,
) -> bool {
    layout_visit_pointer_slots(user_ptr, slot_count, visit)
}

#[cfg(test)]
pub(crate) fn test_layout_pointer_slot_count(user_ptr: usize, slot_count: usize) -> Option<usize> {
    let mut count = 0usize;
    if layout_visit_pointer_slots(user_ptr, slot_count, |_| count += 1) {
        Some(count)
    } else {
        None
    }
}

#[inline(always)]
fn record_trace_slot_read() {
    #[cfg(test)]
    TRACE_SLOT_READS.with(|c| c.set(c.get() + 1));
}

#[cfg(test)]
fn test_reset_trace_slot_reads() {
    TRACE_SLOT_READS.with(|c| c.set(0));
}

#[cfg(test)]
fn test_trace_slot_reads() -> usize {
    TRACE_SLOT_READS.with(|c| c.get())
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

#[inline]
fn try_mark_raw_root_addr(addr: usize, valid_ptrs: &ValidPointerSet) -> bool {
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
fn mark_stack_roots(valid_ptrs: &ValidPointerSet) -> ConservativeRootTraceStats {
    match conservative_stack_scan_decision() {
        ConservativeStackScanDecision::Scan => mark_stack_roots_unchecked(valid_ptrs),
        ConservativeStackScanDecision::SkipDisabled
        | ConservativeStackScanDecision::SkipShadowStackActive => {
            ConservativeRootTraceStats::default()
        }
    }
}

/// Conservative stack scan: scan the current thread's stack for heap pointers.
/// Handles BOTH NaN-boxed pointers (POINTER_TAG/STRING_TAG/BIGINT_TAG) AND raw I64 pointers.
/// Raw I64 pointers arise from Perry's `is_array`/`is_string`/`is_pointer`/`is_closure` local
/// variables — codegen stores these as raw I64 words (not NaN-boxed) in registers and on stack.
fn mark_stack_roots_unchecked(valid_ptrs: &ValidPointerSet) -> ConservativeRootTraceStats {
    let mut stats = ConservativeRootTraceStats::default();
    // Capture callee-saved registers into a buffer via setjmp.
    //
    // On Apple platforms the C `setjmp(3)` saves the signal mask via a
    // `sigprocmask` system call, which dominates GC cost (~25 μs per
    // call on arm64). We only need register capture, not signal-state
    // save — switch to `_setjmp(3)` (linker symbol `__setjmp`) on
    // Apple targets. See the matching switch in
    // `promise.rs::js_promise_run_microtasks` for the full rationale.
    //
    // The `setjmp` extern lives in `crate::ffi::setjmp` so this and
    // `promise.rs` share one libc-matching declaration (issue #856).
    // We view the buffer as `u64` slots here because the goal of this
    // path is to scan register-sized words for potential NaN-boxed /
    // raw pointers; the cast to `*mut c_int` at the FFI boundary is
    // the inverse of the cast `promise.rs` does from its `*mut i32`
    // buffer.
    //
    // Size check: 32 * 8 = 256 bytes, which exceeds the darwin arm64
    // `jmp_buf` (48 * 4 = 192 bytes) and every other platform we
    // currently support — see `crate::ffi::setjmp::JMP_BUF_MIN_BYTES`.
    let mut jmp_buf = [0u64; 32]; // oversized for safety
    unsafe {
        crate::ffi::setjmp::setjmp(jmp_buf.as_mut_ptr() as *mut std::os::raw::c_int);
    }

    // Scan the register buffer (covers callee-saved regs: x19-x28 on AArch64, rbx/rbp/r12-r15 on x86_64)
    for &word in &jmp_buf {
        if try_mark_value_or_raw(word, valid_ptrs) {
            stats.root_count += 1;
        }
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
            if try_mark_value_or_raw(word, valid_ptrs) {
                stats.root_count += 1;
            }
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
        return stats;
    }

    let stack_bottom = get_stack_bottom();
    if stack_bottom == 0 {
        return stats; // Can't determine stack bounds
    }

    // Walk the stack from current SP to stack bottom.
    // Each 8-byte word may be: NaN-boxed pointer, raw I64 heap pointer, return addr, or plain value.
    let mut addr = stack_top;
    while addr < stack_bottom {
        let word = unsafe { *(addr as *const u64) };
        if try_mark_value_or_raw(word, valid_ptrs) {
            stats.root_count += 1;
        }
        addr += 8;
    }
    stats
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

enum RuntimeRootVisitMode<'a> {
    Mark {
        valid_ptrs: &'a ValidPointerSet,
    },
    CopyingCheck {
        checker: &'a mut CopyingNurseryPreflight,
    },
    CopyingMark {
        collector: &'a mut CopyingNurseryCollector,
    },
    CopyingRewrite {
        collector: &'a CopyingNurseryCollector,
    },
    Rewrite {
        valid_ptrs: &'a ValidPointerSet,
    },
    Verify {
        valid_ptrs: &'a ValidPointerSet,
        surface: &'static str,
    },
    Copy {
        mark: &'a mut dyn FnMut(f64),
    },
}

/// Mutable runtime-root visitor used by GC-owned scanner families.
///
/// A scanner calls the slot method that matches its storage. During mark,
/// root slots mark their current referent. During evacuation rewrite, the
/// same scanner is revisited and any forwarded referent is written back to
/// the runtime-owned slot. Compatibility copy mode powers the legacy
/// `scan_*_roots(mark)` wrappers.
pub struct RuntimeRootVisitor<'a> {
    mode: RuntimeRootVisitMode<'a>,
}

impl<'a> RuntimeRootVisitor<'a> {
    fn for_mark(valid_ptrs: &'a ValidPointerSet) -> Self {
        Self {
            mode: RuntimeRootVisitMode::Mark { valid_ptrs },
        }
    }

    fn for_rewrite(valid_ptrs: &'a ValidPointerSet) -> Self {
        Self {
            mode: RuntimeRootVisitMode::Rewrite { valid_ptrs },
        }
    }

    fn for_copying_check(checker: &'a mut CopyingNurseryPreflight) -> Self {
        Self {
            mode: RuntimeRootVisitMode::CopyingCheck { checker },
        }
    }

    fn for_copying_mark(collector: &'a mut CopyingNurseryCollector) -> Self {
        Self {
            mode: RuntimeRootVisitMode::CopyingMark { collector },
        }
    }

    fn for_copying_rewrite(collector: &'a CopyingNurseryCollector) -> Self {
        Self {
            mode: RuntimeRootVisitMode::CopyingRewrite { collector },
        }
    }

    fn for_verify(valid_ptrs: &'a ValidPointerSet, surface: &'static str) -> Self {
        Self {
            mode: RuntimeRootVisitMode::Verify {
                valid_ptrs,
                surface,
            },
        }
    }

    pub fn for_copy(mark: &'a mut dyn FnMut(f64)) -> Self {
        Self {
            mode: RuntimeRootVisitMode::Copy { mark },
        }
    }

    #[inline]
    fn visit_nanbox_bits(&mut self, bits: u64) -> Option<u64> {
        match &mut self.mode {
            RuntimeRootVisitMode::Mark { valid_ptrs } => {
                try_mark_value(bits, valid_ptrs);
                None
            }
            RuntimeRootVisitMode::CopyingCheck { checker } => {
                checker.check_bits(bits);
                None
            }
            RuntimeRootVisitMode::CopyingMark { collector } => collector.visit_value_bits(bits),
            RuntimeRootVisitMode::CopyingRewrite { collector } => {
                collector.rewrite_value_bits(bits)
            }
            RuntimeRootVisitMode::Rewrite { valid_ptrs } => {
                try_rewrite_nanboxed_value(bits, valid_ptrs)
            }
            RuntimeRootVisitMode::Verify {
                valid_ptrs,
                surface,
            } => {
                if let Some(new_bits) = try_rewrite_nanboxed_value(bits, valid_ptrs) {
                    panic_stale_forwarded_reference(surface, 0, bits, new_bits);
                }
                None
            }
            RuntimeRootVisitMode::Copy { mark } => {
                (*mark)(f64::from_bits(bits));
                None
            }
        }
    }

    #[inline]
    fn visit_heap_word_bits(&mut self, bits: u64) -> Option<u64> {
        match &mut self.mode {
            RuntimeRootVisitMode::Mark { valid_ptrs } => {
                try_mark_value_or_raw(bits, valid_ptrs);
                None
            }
            RuntimeRootVisitMode::CopyingCheck { checker } => {
                checker.check_bits(bits);
                None
            }
            RuntimeRootVisitMode::CopyingMark { collector } => collector.visit_value_bits(bits),
            RuntimeRootVisitMode::CopyingRewrite { collector } => {
                collector.rewrite_value_bits(bits)
            }
            RuntimeRootVisitMode::Rewrite { valid_ptrs } => try_rewrite_value(bits, valid_ptrs),
            RuntimeRootVisitMode::Verify {
                valid_ptrs,
                surface,
            } => {
                if let Some(new_bits) = try_rewrite_value(bits, valid_ptrs) {
                    panic_stale_forwarded_reference(surface, 0, bits, new_bits);
                }
                None
            }
            RuntimeRootVisitMode::Copy { mark } => {
                let tag = bits & TAG_MASK;
                if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
                    (*mark)(f64::from_bits(bits));
                } else if tag < 0x7FF8_0000_0000_0000
                    && (0x1000..=0x0000_FFFF_FFFF_FFFF).contains(&bits)
                {
                    (*mark)(f64::from_bits(POINTER_TAG | (bits & POINTER_MASK)));
                }
                None
            }
        }
    }

    #[inline]
    fn visit_tagged_raw_addr(&mut self, addr: usize, copy_tag: u64) -> Option<usize> {
        if addr == 0 {
            return None;
        }
        match &mut self.mode {
            RuntimeRootVisitMode::Mark { valid_ptrs } => {
                try_mark_raw_root_addr(addr, valid_ptrs);
                None
            }
            RuntimeRootVisitMode::CopyingCheck { checker } => {
                checker.check_addr(addr);
                None
            }
            RuntimeRootVisitMode::CopyingMark { collector } => collector.visit_raw_addr(addr),
            RuntimeRootVisitMode::CopyingRewrite { collector } => collector.rewrite_raw_addr(addr),
            RuntimeRootVisitMode::Rewrite { valid_ptrs } => try_rewrite_raw_addr(addr, valid_ptrs),
            RuntimeRootVisitMode::Verify {
                valid_ptrs,
                surface,
            } => {
                if let Some(new_addr) = try_rewrite_raw_addr(addr, valid_ptrs) {
                    panic_stale_forwarded_reference(
                        surface,
                        0,
                        copy_tag | (addr as u64 & POINTER_MASK),
                        copy_tag | (new_addr as u64 & POINTER_MASK),
                    );
                }
                None
            }
            RuntimeRootVisitMode::Copy { mark } => {
                (*mark)(f64::from_bits(copy_tag | (addr as u64 & POINTER_MASK)));
                None
            }
        }
    }

    #[inline]
    fn visit_metadata_raw_addr(&mut self, addr: usize) -> Option<usize> {
        if addr == 0 {
            return None;
        }
        match &mut self.mode {
            RuntimeRootVisitMode::Rewrite { valid_ptrs } => try_rewrite_raw_addr(addr, valid_ptrs),
            RuntimeRootVisitMode::CopyingCheck { .. } => None,
            RuntimeRootVisitMode::CopyingMark { .. } => None,
            RuntimeRootVisitMode::CopyingRewrite { collector } => collector.rewrite_raw_addr(addr),
            RuntimeRootVisitMode::Verify {
                valid_ptrs,
                surface,
            } => {
                if let Some(new_addr) = try_rewrite_raw_addr(addr, valid_ptrs) {
                    panic_stale_forwarded_reference(surface, 0, addr as u64, new_addr as u64);
                }
                None
            }
            RuntimeRootVisitMode::Mark { .. } | RuntimeRootVisitMode::Copy { .. } => None,
        }
    }

    /// Visit a mutable NaN-boxed JSValue stored as `f64`.
    /// Returns true when rewrite mode changed the slot.
    pub fn visit_nanbox_f64_slot(&mut self, slot: &mut f64) -> bool {
        let bits = slot.to_bits();
        if let Some(new_bits) = self.visit_nanbox_bits(bits) {
            *slot = f64::from_bits(new_bits);
            true
        } else {
            false
        }
    }

    /// Visit a mutable NaN-boxed JSValue stored as `u64` bits.
    /// Returns true when rewrite mode changed the slot.
    pub fn visit_nanbox_u64_slot(&mut self, slot: &mut u64) -> bool {
        if let Some(new_bits) = self.visit_nanbox_bits(*slot) {
            *slot = new_bits;
            true
        } else {
            false
        }
    }

    /// Visit a mutable heap word that may store either a NaN-boxed JSValue
    /// pointer or a raw heap pointer.
    ///
    /// This matches heap-field rewrite semantics for runtime-owned caches
    /// whose keys are bit copies of closure captures or object fields.
    pub fn visit_heap_word_u64_slot(&mut self, slot: &mut u64) -> bool {
        if let Some(new_bits) = self.visit_heap_word_bits(*slot) {
            *slot = new_bits;
            true
        } else {
            false
        }
    }

    /// Visit a raw `f64` slot address when the owner cannot hand out a
    /// Rust `&mut f64` (for example `static mut` storage).
    ///
    /// # Safety
    /// `slot` must be valid for a read and, in rewrite mode, a write.
    pub unsafe fn visit_nanbox_f64_raw_slot(&mut self, slot: *mut f64) -> bool {
        if slot.is_null() {
            return false;
        }
        let bits = (*slot).to_bits();
        if let Some(new_bits) = self.visit_nanbox_bits(bits) {
            *slot = f64::from_bits(new_bits);
            true
        } else {
            false
        }
    }

    /// Visit a raw `u64` slot address when the owner cannot hand out a
    /// Rust `&mut u64`.
    ///
    /// # Safety
    /// `slot` must be valid for a read and, in rewrite mode, a write.
    pub unsafe fn visit_nanbox_u64_raw_slot(&mut self, slot: *mut u64) -> bool {
        if slot.is_null() {
            return false;
        }
        if let Some(new_bits) = self.visit_nanbox_bits(*slot) {
            *slot = new_bits;
            true
        } else {
            false
        }
    }

    /// Visit a `Cell<f64>` that stores a NaN-boxed JSValue.
    pub fn visit_cell_f64_slot(&mut self, slot: &Cell<f64>) -> bool {
        let bits = slot.get().to_bits();
        if let Some(new_bits) = self.visit_nanbox_bits(bits) {
            slot.set(f64::from_bits(new_bits));
            true
        } else {
            false
        }
    }

    /// Visit a root slot that stores a raw mutable heap pointer.
    pub fn visit_raw_mut_ptr_slot<T>(&mut self, slot: &mut *mut T) -> bool {
        if let Some(new_addr) = self.visit_tagged_raw_addr(*slot as usize, POINTER_TAG) {
            *slot = new_addr as *mut T;
            true
        } else {
            false
        }
    }

    /// Visit a root slot that stores a raw const heap pointer.
    pub fn visit_raw_const_ptr_slot<T>(&mut self, slot: &mut *const T) -> bool {
        if let Some(new_addr) = self.visit_tagged_raw_addr(*slot as usize, POINTER_TAG) {
            *slot = new_addr as *const T;
            true
        } else {
            false
        }
    }

    /// Visit a raw const heap pointer slot, using a specific NaN-box tag
    /// when the visitor is running in compatibility copy mode.
    pub fn visit_tagged_raw_const_ptr_slot<T>(&mut self, slot: &mut *const T, tag: u64) -> bool {
        if let Some(new_addr) = self.visit_tagged_raw_addr(*slot as usize, tag) {
            *slot = new_addr as *const T;
            true
        } else {
            false
        }
    }

    /// Visit a root slot that stores a raw heap pointer as `usize`.
    pub fn visit_usize_slot(&mut self, slot: &mut usize) -> bool {
        if let Some(new_addr) = self.visit_tagged_raw_addr(*slot, POINTER_TAG) {
            *slot = new_addr;
            true
        } else {
            false
        }
    }

    /// Visit a raw heap pointer stored as `usize`, using a specific
    /// NaN-box tag when the visitor is running in compatibility copy mode.
    pub fn visit_tagged_usize_slot(&mut self, slot: &mut usize, tag: u64) -> bool {
        if let Some(new_addr) = self.visit_tagged_raw_addr(*slot, tag) {
            *slot = new_addr;
            true
        } else {
            false
        }
    }

    /// Visit a root slot that stores a raw heap pointer as `i64`.
    pub fn visit_i64_slot(&mut self, slot: &mut i64) -> bool {
        if *slot <= 0 {
            return false;
        }
        if let Some(new_addr) = self.visit_tagged_raw_addr(*slot as usize, POINTER_TAG) {
            *slot = new_addr as i64;
            true
        } else {
            false
        }
    }

    /// Visit a raw `usize` slot address.
    ///
    /// # Safety
    /// `slot` must be valid for a read and, in rewrite mode, a write.
    pub unsafe fn visit_usize_raw_slot(&mut self, slot: *mut usize) -> bool {
        if slot.is_null() {
            return false;
        }
        if let Some(new_addr) = self.visit_tagged_raw_addr(*slot, POINTER_TAG) {
            *slot = new_addr;
            true
        } else {
            false
        }
    }

    /// Visit an atomic raw pointer root slot.
    pub fn visit_atomic_raw_mut_ptr_slot<T>(
        &mut self,
        slot: &std::sync::atomic::AtomicPtr<T>,
        load_ordering: std::sync::atomic::Ordering,
        store_ordering: std::sync::atomic::Ordering,
    ) -> bool {
        let current = slot.load(load_ordering);
        if let Some(new_addr) = self.visit_tagged_raw_addr(current as usize, POINTER_TAG) {
            slot.store(new_addr as *mut T, atomic_store_ordering(store_ordering));
            true
        } else {
            false
        }
    }

    /// Visit an atomic `i64` root slot containing a raw heap pointer.
    pub fn visit_atomic_i64_slot(
        &mut self,
        slot: &std::sync::atomic::AtomicI64,
        load_ordering: std::sync::atomic::Ordering,
        store_ordering: std::sync::atomic::Ordering,
    ) -> bool {
        let current = slot.load(load_ordering);
        if current <= 0 {
            return false;
        }
        if let Some(new_addr) = self.visit_tagged_raw_addr(current as usize, POINTER_TAG) {
            slot.store(new_addr as i64, atomic_store_ordering(store_ordering));
            true
        } else {
            false
        }
    }

    /// Visit a metadata-only raw heap pointer key. The value is rewritten
    /// if forwarded, but it is not marked as a root. Mark/copy modes emit
    /// nothing; post-copy rewrite only follows forwarding pointers that
    /// already exist.
    pub fn visit_metadata_usize_slot(&mut self, slot: &mut usize) -> bool {
        if let Some(new_addr) = self.visit_metadata_raw_addr(*slot) {
            *slot = new_addr;
            true
        } else {
            false
        }
    }

    /// Visit a metadata-only raw heap pointer key stored as `i64`.
    pub fn visit_metadata_i64_slot(&mut self, slot: &mut i64) -> bool {
        if *slot <= 0 {
            return false;
        }
        if let Some(new_addr) = self.visit_metadata_raw_addr(*slot as usize) {
            *slot = new_addr as i64;
            true
        } else {
            false
        }
    }

    /// Visit a raw metadata-only `usize` slot address.
    ///
    /// # Safety
    /// `slot` must be valid for a read and, in rewrite mode, a write.
    pub unsafe fn visit_metadata_usize_raw_slot(&mut self, slot: *mut usize) -> bool {
        if slot.is_null() {
            return false;
        }
        if let Some(new_addr) = self.visit_metadata_raw_addr(*slot) {
            *slot = new_addr;
            true
        } else {
            false
        }
    }
}

#[inline]
fn atomic_store_ordering(ordering: std::sync::atomic::Ordering) -> std::sync::atomic::Ordering {
    match ordering {
        std::sync::atomic::Ordering::Relaxed => std::sync::atomic::Ordering::Relaxed,
        std::sync::atomic::Ordering::Acquire | std::sync::atomic::Ordering::Release => {
            std::sync::atomic::Ordering::Release
        }
        std::sync::atomic::Ordering::AcqRel => std::sync::atomic::Ordering::Release,
        std::sync::atomic::Ordering::SeqCst => std::sync::atomic::Ordering::SeqCst,
        _ => std::sync::atomic::Ordering::SeqCst,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MutableRootSlotKind {
    ShadowStack,
    GlobalRoot,
}

#[derive(Clone, Copy)]
struct MutableRootSlot {
    kind: MutableRootSlotKind,
    ptr: *mut u64,
}

impl MutableRootSlot {
    #[inline]
    unsafe fn read(self) -> u64 {
        *self.ptr
    }

    #[inline]
    unsafe fn write(self, bits: u64) {
        *self.ptr = bits;
    }
}

/// Visit every live shadow-stack slot. The visitor receives real
/// mutable slot addresses so the same walk can support mark-only
/// scanning and post-forwarding rewrites.
fn visit_shadow_stack_root_slots(mut visit: impl FnMut(MutableRootSlot)) {
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
            let base = s.stack.as_mut_ptr().add(top);
            for i in 0..slot_count {
                visit(MutableRootSlot {
                    kind: MutableRootSlotKind::ShadowStack,
                    ptr: base.add(i),
                });
            }
            top = s.stack[header_base] as usize;
        }
    });
}

/// Visit every registered module-global root slot.
fn visit_global_root_slots(mut visit: impl FnMut(MutableRootSlot)) {
    GLOBAL_ROOTS.with(|roots| {
        let roots = roots.borrow();
        for &root_ptr in roots.iter() {
            if root_ptr.is_null() {
                continue;
            }
            visit(MutableRootSlot {
                kind: MutableRootSlotKind::GlobalRoot,
                ptr: root_ptr,
            });
        }
    });
}

/// Visit the root slots whose storage is owned by this runtime and can
/// therefore be rewritten after evacuation.
fn visit_mutable_root_slots(mut visit: impl FnMut(MutableRootSlot)) {
    visit_shadow_stack_root_slots(&mut visit);
    visit_global_root_slots(&mut visit);
}

#[inline]
fn shadow_slot_pointer_root(bits: u64) -> bool {
    let tag = bits & TAG_MASK;
    let addr = bits & POINTER_MASK;
    addr != 0 && (tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG)
}

#[inline]
fn mark_global_root_bits(bits: u64, valid_ptrs: &ValidPointerSet) {
    // First try NaN-boxed interpretation (exported globals, closures, etc.).
    if try_mark_value(bits, valid_ptrs) {
        return;
    }
    // Module variable globals store raw I64 pointers (not NaN-boxed).
    // Preserve the historical direct-object-start behavior: validate
    // against valid_ptrs and mark the target, without the conservative
    // interior-pointer fallback used by stack scanning.
    let raw_ptr = bits as usize;
    try_mark_raw_root_addr(raw_ptr, valid_ptrs);
}

/// Mark mutable roots (shadow-stack slots and registered globals).
fn mark_mutable_root_slots(
    valid_ptrs: &ValidPointerSet,
    mut shadow_stats: Option<&mut ShadowRootTraceStats>,
) {
    visit_mutable_root_slots(|slot| unsafe {
        let bits = slot.read();
        if matches!(slot.kind, MutableRootSlotKind::ShadowStack) {
            if let Some(stats) = shadow_stats.as_mut() {
                stats.record_scan(bits);
            }
        }
        if bits == 0 {
            return;
        }
        match slot.kind {
            MutableRootSlotKind::ShadowStack => {
                try_mark_value(bits, valid_ptrs);
            }
            MutableRootSlotKind::GlobalRoot => mark_global_root_bits(bits, valid_ptrs),
        }
    });
}

#[inline]
fn nanboxed_root_header(value_bits: u64, valid_ptrs: &ValidPointerSet) -> Option<*mut GcHeader> {
    let tag = value_bits & TAG_MASK;
    if tag != POINTER_TAG && tag != STRING_TAG && tag != BIGINT_TAG {
        return None;
    }
    let ptr_val = (value_bits & POINTER_MASK) as usize;
    if ptr_val == 0 || !valid_ptrs.maybe_contains(ptr_val) || !valid_ptrs.contains(&ptr_val) {
        return None;
    }
    Some(unsafe { header_from_user_ptr(ptr_val as *const u8) })
}

#[inline]
fn pin_conservative_root_header(header: *mut GcHeader) -> bool {
    CONS_PINNED.with(|s| {
        let mut pinned = s.borrow_mut();
        pinned.insert(header as usize)
    })
}

#[inline]
fn mark_copy_only_scanner_bits(
    bits: u64,
    valid_ptrs: &ValidPointerSet,
    pin_discoveries: bool,
) -> Option<usize> {
    let Some(header) = nanboxed_root_header(bits, valid_ptrs) else {
        return None;
    };
    unsafe {
        let flags = (*header).gc_flags;
        if flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) == 0 {
            (*header).gc_flags = flags | GC_FLAG_MARKED;
            push_mark_seed(header);
        }
    }
    if pin_discoveries {
        if pin_conservative_root_header(header) {
            return Some(unsafe { (*header).size as usize });
        }
    }
    None
}

struct RegisteredRootMarkContext {
    valid_ptrs: *const ValidPointerSet,
    pin_discoveries: bool,
    legacy_stats: *mut LegacyRootTraceStats,
}

/// Run registered runtime-owned scanners that expose mutable slots.
fn mark_mutable_registered_roots(valid_ptrs: &ValidPointerSet) {
    let scanners: Vec<MutableRootScanner> = MUTABLE_ROOT_SCANNERS.with(|s| s.borrow().clone());
    let mut visitor = RuntimeRootVisitor::for_mark(valid_ptrs);
    for scanner in scanners {
        scanner(&mut visitor);
    }
}

/// Run legacy copy-only root scanners. When evacuation is enabled,
/// every discovered root is pinned because the scanner API gives us no
/// slot to rewrite after forwarding.
fn mark_registered_roots(
    valid_ptrs: &ValidPointerSet,
    pin_discoveries: bool,
) -> LegacyRootTraceStats {
    let mut legacy_stats = LegacyRootTraceStats::default();
    // Collect scanners first to avoid borrow conflicts
    let scanners: Vec<fn(&mut dyn FnMut(f64))> = ROOT_SCANNERS.with(|s| s.borrow().clone());

    for scanner in scanners {
        scanner(&mut |value: f64| {
            if let Some(bytes) =
                mark_copy_only_scanner_bits(value.to_bits(), valid_ptrs, pin_discoveries)
            {
                legacy_stats.pinned_roots += 1;
                legacy_stats.pinned_bytes += bytes;
            }
        });
    }

    let ffi_scanners: Vec<PerryFfiRootScanner> = FFI_ROOT_SCANNERS.with(|s| s.borrow().clone());
    let mut ctx = RegisteredRootMarkContext {
        valid_ptrs: valid_ptrs as *const ValidPointerSet,
        pin_discoveries,
        legacy_stats: &mut legacy_stats as *mut LegacyRootTraceStats,
    };
    let ctx = &mut ctx as *mut RegisteredRootMarkContext as *mut c_void;
    for scanner in ffi_scanners {
        scanner(perry_ffi_mark_root, ctx);
    }
    legacy_stats
}

extern "C" fn perry_ffi_mark_root(value: f64, ctx: *mut c_void) {
    if ctx.is_null() {
        return;
    }
    let ctx = unsafe { &*(ctx as *const RegisteredRootMarkContext) };
    if ctx.valid_ptrs.is_null() {
        return;
    }
    let valid_ptrs = unsafe { &*ctx.valid_ptrs };
    if let Some(bytes) =
        mark_copy_only_scanner_bits(value.to_bits(), valid_ptrs, ctx.pin_discoveries)
    {
        if !ctx.legacy_stats.is_null() {
            unsafe {
                (*ctx.legacy_stats).pinned_roots += 1;
                (*ctx.legacy_stats).pinned_bytes += bytes;
            }
        }
    }
}

/// Snapshot the remembered dirty ranges before the collection clears them.
struct RememberedDirtySnapshot {
    dirty_old_pages: crate::fast_hash::PtrHashSet<usize>,
    external_dirty_entries: Vec<(usize, usize)>,
    dirty_pages: crate::fast_hash::PtrHashSet<usize>,
    fallback_headers: Vec<usize>,
}

fn remembered_dirty_snapshot() -> RememberedDirtySnapshot {
    let dirty_old_pages: crate::fast_hash::PtrHashSet<usize> =
        DIRTY_OLD_PAGES.with(|s| s.borrow().iter().copied().collect());
    let external_dirty_entries: Vec<(usize, usize)> = EXTERNAL_DIRTY_SLOT_PAGES.with(|s| {
        s.borrow()
            .iter()
            .flat_map(|(&page, headers)| headers.iter().copied().map(move |header| (page, header)))
            .collect()
    });
    let mut dirty_pages = dirty_old_pages.clone();
    for (page, _) in &external_dirty_entries {
        dirty_pages.insert(*page);
    }
    let fallback_headers = REMEMBERED_SET.with(|s| s.borrow().iter().copied().collect());

    RememberedDirtySnapshot {
        dirty_old_pages,
        external_dirty_entries,
        dirty_pages,
        fallback_headers,
    }
}

/// Gen-GC Phase C3: mark the remembered set as roots. Old-gen
/// dirty pages may hold pointers to young-gen objects that would
/// otherwise be missed by a minor GC. This is Perry's compact
/// equivalent of MMTk's modbuf / ProcessModBuf: barriers log old
/// pages, this phase scans those bounded regions, and the clear at
/// collection end gives the log consumed semantics.
fn mark_remembered_set_roots(valid_ptrs: &ValidPointerSet) -> RememberedSetTraceStats {
    let snapshot = remembered_dirty_snapshot();
    let mut stats = RememberedSetTraceStats {
        entries_scanned: snapshot.dirty_old_pages.len()
            + snapshot.external_dirty_entries.len()
            + snapshot.fallback_headers.len(),
        dirty_pages_before: snapshot.dirty_pages.len(),
        dirty_pages_scanned: snapshot.dirty_pages.len(),
        ..RememberedSetTraceStats::default()
    };

    let mut mark_slot = |slot: *mut u64, stats: &mut RememberedSetTraceStats| unsafe {
        if try_mark_young_value_as_seed(*slot, valid_ptrs) {
            stats.newly_marked += 1;
        }
    };
    scan_remembered_dirty_slot_ranges(&snapshot, valid_ptrs, &mut stats, &mut mark_slot);

    // Test-only fallback path. Production barriers no longer insert
    // object headers here, but keeping the scan lets tests compare the
    // old object-set behavior against the dirty-page path.
    for header_addr in snapshot.fallback_headers {
        // Header sits at GcHeader; user pointer is +GC_HEADER_SIZE.
        let user_ptr = header_addr + GC_HEADER_SIZE;
        if !valid_ptrs.contains(&user_ptr) {
            continue;
        }
        stats.valid_roots += 1;
        let nanbox = POINTER_TAG | (user_ptr as u64);
        if try_mark_value(nanbox, valid_ptrs) {
            stats.newly_marked += 1;
        }
    }
    stats.dirty_pages_after = remembered_dirty_page_count();
    stats
}

fn scan_remembered_dirty_slot_ranges(
    snapshot: &RememberedDirtySnapshot,
    valid_ptrs: &ValidPointerSet,
    stats: &mut RememberedSetTraceStats,
    visit_slot: &mut dyn FnMut(*mut u64, &mut RememberedSetTraceStats),
) {
    if snapshot.dirty_old_pages.is_empty() && snapshot.external_dirty_entries.is_empty() {
        return;
    }

    let mut seen_headers = crate::fast_hash::new_ptr_hash_set();
    if !snapshot.dirty_old_pages.is_empty() {
        crate::arena::old_arena_walk_objects_on_pages(
            &snapshot.dirty_old_pages,
            |header_ptr| unsafe {
                let header = header_ptr as *mut GcHeader;
                if !seen_headers.insert(header as usize) {
                    return;
                }
                scan_dirty_header_once(
                    header,
                    &snapshot.dirty_pages,
                    valid_ptrs,
                    stats,
                    visit_slot,
                );
            },
        );
    }
    for &(_, header_addr) in &snapshot.external_dirty_entries {
        if !seen_headers.insert(header_addr) {
            continue;
        }
        unsafe {
            scan_dirty_header_once(
                header_addr as *mut GcHeader,
                &snapshot.dirty_pages,
                valid_ptrs,
                stats,
                visit_slot,
            );
        }
    }
}

unsafe fn scan_dirty_header_once(
    header: *mut GcHeader,
    dirty_pages: &crate::fast_hash::PtrHashSet<usize>,
    valid_ptrs: &ValidPointerSet,
    stats: &mut RememberedSetTraceStats,
    visit_slot: &mut dyn FnMut(*mut u64, &mut RememberedSetTraceStats),
) {
    let total_size = (*header).size as usize;
    if total_size == 0 {
        return;
    }
    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
    if !valid_ptrs.contains(&(user_ptr as usize)) {
        return;
    }
    stats.old_objects_considered += 1;
    stats.valid_roots += 1;
    stats.dirty_objects_scanned += 1;
    scan_dirty_object_slots(header, dirty_pages, stats, visit_slot);
}

#[inline]
fn dirty_pages_contains_addr(
    dirty_pages: &crate::fast_hash::PtrHashSet<usize>,
    addr: usize,
) -> bool {
    dirty_pages.contains(&crate::arena::generation_page_for_addr(addr))
}

unsafe fn scan_dirty_slot(
    slot: *mut u64,
    dirty_pages: &crate::fast_hash::PtrHashSet<usize>,
    stats: &mut RememberedSetTraceStats,
    visit_slot: &mut dyn FnMut(*mut u64, &mut RememberedSetTraceStats),
) {
    if !dirty_pages_contains_addr(dirty_pages, slot as usize) {
        return;
    }
    stats.dirty_slots_scanned += 1;
    visit_slot(slot, stats);
}

unsafe fn scan_dirty_raw_ptr_slot<T>(
    slot: *const *mut T,
    dirty_pages: &crate::fast_hash::PtrHashSet<usize>,
    stats: &mut RememberedSetTraceStats,
    visit_slot: &mut dyn FnMut(*mut u64, &mut RememberedSetTraceStats),
) {
    scan_dirty_raw_ptr_value_slot(slot as *mut u64, dirty_pages, stats, visit_slot);
}

unsafe fn scan_dirty_const_raw_ptr_slot<T>(
    slot: *const *const T,
    dirty_pages: &crate::fast_hash::PtrHashSet<usize>,
    stats: &mut RememberedSetTraceStats,
    visit_slot: &mut dyn FnMut(*mut u64, &mut RememberedSetTraceStats),
) {
    scan_dirty_raw_ptr_value_slot(slot as *mut u64, dirty_pages, stats, visit_slot);
}

fn scan_dirty_raw_ptr_value_slot(
    slot: *mut u64,
    dirty_pages: &crate::fast_hash::PtrHashSet<usize>,
    stats: &mut RememberedSetTraceStats,
    visit_slot: &mut dyn FnMut(*mut u64, &mut RememberedSetTraceStats),
) {
    if !dirty_pages_contains_addr(dirty_pages, slot as usize) {
        return;
    }
    stats.dirty_slots_scanned += 1;
    visit_slot(slot, stats);
}

unsafe fn scan_dirty_slot_range(
    slots: *mut u64,
    slot_count: usize,
    dirty_pages: &crate::fast_hash::PtrHashSet<usize>,
    stats: &mut RememberedSetTraceStats,
    visit_slot: &mut dyn FnMut(*mut u64, &mut RememberedSetTraceStats),
) {
    if slots.is_null() || slot_count == 0 || dirty_pages.is_empty() {
        return;
    }
    const PAGE_SHIFT: usize = 12;
    const PAGE_SIZE: usize = 1 << PAGE_SHIFT;

    let slots_start = slots as usize;
    let Some(slots_bytes) = slot_count.checked_mul(std::mem::size_of::<u64>()) else {
        return;
    };
    let Some(slots_end) = slots_start.checked_add(slots_bytes) else {
        return;
    };
    let mut ranges = Vec::<(usize, usize)>::new();

    for &page in dirty_pages {
        let page_start = page << PAGE_SHIFT;
        let page_end = page_start + PAGE_SIZE;
        if page_end <= slots_start || page_start >= slots_end {
            continue;
        }
        stats.dirty_slot_pages_considered += 1;
        let start_addr = page_start.max(slots_start);
        let end_addr = page_end.min(slots_end);
        let start_idx = (start_addr - slots_start + 7) / 8;
        let end_idx = (end_addr - slots_start + 7) / 8;
        if start_idx < end_idx && start_idx < slot_count {
            ranges.push((start_idx, end_idx.min(slot_count)));
        }
    }

    if ranges.is_empty() {
        return;
    }
    ranges.sort_unstable();
    let mut merged = Vec::<(usize, usize)>::with_capacity(ranges.len());
    for (start, end) in ranges {
        if let Some((_, last_end)) = merged.last_mut() {
            if start <= *last_end {
                *last_end = (*last_end).max(end);
                continue;
            }
        }
        merged.push((start, end));
    }

    for (start, end) in merged {
        stats.dirty_slot_ranges_scanned += 1;
        for i in start..end {
            stats.dirty_slots_scanned += 1;
            visit_slot(slots.add(i), stats);
        }
    }
}

unsafe fn scan_dirty_object_slots(
    header: *mut GcHeader,
    dirty_pages: &crate::fast_hash::PtrHashSet<usize>,
    stats: &mut RememberedSetTraceStats,
    visit_slot: &mut dyn FnMut(*mut u64, &mut RememberedSetTraceStats),
) {
    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
    match (*header).obj_type {
        GC_TYPE_ARRAY => scan_dirty_array_slots(user_ptr, dirty_pages, stats, visit_slot),
        GC_TYPE_OBJECT => scan_dirty_object_field_slots(user_ptr, dirty_pages, stats, visit_slot),
        GC_TYPE_CLOSURE => scan_dirty_closure_slots(user_ptr, dirty_pages, stats, visit_slot),
        GC_TYPE_PROMISE => scan_dirty_promise_slots(user_ptr, dirty_pages, stats, visit_slot),
        GC_TYPE_ERROR => scan_dirty_error_slots(user_ptr, dirty_pages, stats, visit_slot),
        GC_TYPE_MAP => scan_dirty_map_slots(user_ptr, dirty_pages, stats, visit_slot),
        GC_TYPE_LAZY_ARRAY => scan_dirty_lazy_array_slots(user_ptr, dirty_pages, stats, visit_slot),
        GC_TYPE_STRING | GC_TYPE_BIGINT => {}
        _ => {}
    }
}

unsafe fn scan_dirty_array_slots(
    user_ptr: *mut u8,
    dirty_pages: &crate::fast_hash::PtrHashSet<usize>,
    stats: &mut RememberedSetTraceStats,
    visit_slot: &mut dyn FnMut(*mut u64, &mut RememberedSetTraceStats),
) {
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
    if layout_visit_pointer_slots(user_ptr as usize, length as usize, |i| unsafe {
        scan_dirty_slot(elements.add(i), dirty_pages, stats, visit_slot);
    }) {
        return;
    }
    scan_dirty_slot_range(elements, length as usize, dirty_pages, stats, visit_slot);
}

unsafe fn scan_dirty_object_field_slots(
    user_ptr: *mut u8,
    dirty_pages: &crate::fast_hash::PtrHashSet<usize>,
    stats: &mut RememberedSetTraceStats,
    visit_slot: &mut dyn FnMut(*mut u64, &mut RememberedSetTraceStats),
) {
    let obj = user_ptr as *const crate::object::ObjectHeader;
    let field_count = (*obj).field_count;
    if field_count > 1_000_000 {
        return;
    }
    scan_dirty_raw_ptr_slot(&(*obj).keys_array, dirty_pages, stats, visit_slot);
    let fields =
        (user_ptr as *const u8).add(std::mem::size_of::<crate::object::ObjectHeader>()) as *mut u64;
    if layout_visit_pointer_slots(user_ptr as usize, field_count as usize, |i| unsafe {
        scan_dirty_slot(fields.add(i), dirty_pages, stats, visit_slot);
    }) {
        return;
    }
    scan_dirty_slot_range(fields, field_count as usize, dirty_pages, stats, visit_slot);
}

unsafe fn scan_dirty_closure_slots(
    user_ptr: *mut u8,
    dirty_pages: &crate::fast_hash::PtrHashSet<usize>,
    stats: &mut RememberedSetTraceStats,
    visit_slot: &mut dyn FnMut(*mut u64, &mut RememberedSetTraceStats),
) {
    let closure = user_ptr as *const crate::closure::ClosureHeader;
    let capture_count = crate::closure::real_capture_count((*closure).capture_count);
    let captures =
        (user_ptr as *mut u8).add(std::mem::size_of::<crate::closure::ClosureHeader>()) as *mut u64;
    if layout_visit_pointer_slots(user_ptr as usize, capture_count as usize, |i| unsafe {
        scan_dirty_slot(captures.add(i), dirty_pages, stats, visit_slot);
    }) {
        return;
    }
    scan_dirty_slot_range(
        captures,
        capture_count as usize,
        dirty_pages,
        stats,
        visit_slot,
    );
}

unsafe fn scan_dirty_promise_slots(
    user_ptr: *mut u8,
    dirty_pages: &crate::fast_hash::PtrHashSet<usize>,
    stats: &mut RememberedSetTraceStats,
    visit_slot: &mut dyn FnMut(*mut u64, &mut RememberedSetTraceStats),
) {
    let promise = user_ptr as *const crate::promise::Promise;
    scan_dirty_slot(
        &(*promise).value as *const f64 as *mut u64,
        dirty_pages,
        stats,
        visit_slot,
    );
    scan_dirty_slot(
        &(*promise).reason as *const f64 as *mut u64,
        dirty_pages,
        stats,
        visit_slot,
    );
    scan_dirty_const_raw_ptr_slot(&(*promise).on_fulfilled, dirty_pages, stats, visit_slot);
    scan_dirty_const_raw_ptr_slot(&(*promise).on_rejected, dirty_pages, stats, visit_slot);
    scan_dirty_raw_ptr_slot(&(*promise).next, dirty_pages, stats, visit_slot);
}

unsafe fn scan_dirty_error_slots(
    user_ptr: *mut u8,
    dirty_pages: &crate::fast_hash::PtrHashSet<usize>,
    stats: &mut RememberedSetTraceStats,
    visit_slot: &mut dyn FnMut(*mut u64, &mut RememberedSetTraceStats),
) {
    let error = user_ptr as *const crate::error::ErrorHeader;
    scan_dirty_raw_ptr_slot(&(*error).message, dirty_pages, stats, visit_slot);
    scan_dirty_raw_ptr_slot(&(*error).name, dirty_pages, stats, visit_slot);
    scan_dirty_raw_ptr_slot(&(*error).stack, dirty_pages, stats, visit_slot);
    scan_dirty_slot(
        &(*error).cause as *const f64 as *mut u64,
        dirty_pages,
        stats,
        visit_slot,
    );
    scan_dirty_raw_ptr_slot(&(*error).errors, dirty_pages, stats, visit_slot);
}

unsafe fn scan_dirty_map_slots(
    user_ptr: *mut u8,
    dirty_pages: &crate::fast_hash::PtrHashSet<usize>,
    stats: &mut RememberedSetTraceStats,
    visit_slot: &mut dyn FnMut(*mut u64, &mut RememberedSetTraceStats),
) {
    let map = user_ptr as *const crate::map::MapHeader;
    let size = (*map).size;
    let capacity = (*map).capacity;
    if size > capacity || size > 100_000 || (*map).entries.is_null() {
        return;
    }
    let entries = (*map).entries as *const u64;
    scan_dirty_slot_range(
        entries as *mut u64,
        size as usize * 2,
        dirty_pages,
        stats,
        visit_slot,
    );
}

unsafe fn scan_dirty_lazy_array_slots(
    user_ptr: *mut u8,
    dirty_pages: &crate::fast_hash::PtrHashSet<usize>,
    stats: &mut RememberedSetTraceStats,
    visit_slot: &mut dyn FnMut(*mut u64, &mut RememberedSetTraceStats),
) {
    let lazy = user_ptr as *const crate::json_tape::LazyArrayHeader;
    if (*lazy).magic != crate::json_tape::LAZY_ARRAY_MAGIC {
        return;
    }
    scan_dirty_const_raw_ptr_slot(&(*lazy).blob_str, dirty_pages, stats, visit_slot);
    scan_dirty_raw_ptr_slot(&(*lazy).materialized, dirty_pages, stats, visit_slot);
    scan_dirty_raw_ptr_slot(
        &(*lazy).materialized_elements,
        dirty_pages,
        stats,
        visit_slot,
    );
    scan_dirty_raw_ptr_slot(&(*lazy).materialized_bitmap, dirty_pages, stats, visit_slot);

    let cached_length = (*lazy).cached_length as usize;
    let cache = (*lazy).materialized_elements;
    let bitmap = (*lazy).materialized_bitmap;
    if cache.is_null() || bitmap.is_null() || cached_length == 0 {
        return;
    }
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
            scan_dirty_slot(cache.add(i) as *mut u64, dirty_pages, stats, visit_slot);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CopyingPointerKind {
    Eden,
    FromSurvivor,
    ToSurvivor,
    Longlived,
    Old,
    Malloc,
}

#[derive(Clone, Copy)]
struct CopyingPointer {
    header: *mut GcHeader,
    kind: CopyingPointerKind,
}

struct CopyingPointerSet {
    malloc_registry_available: bool,
    malloc_validation_lookups: Cell<usize>,
    malloc_registry_rebuild_count_start: u64,
}

impl CopyingPointerSet {
    fn new() -> Self {
        let malloc_registry_available =
            MALLOC_STATE.with(|s| s.borrow().malloc_registry_available());
        let malloc_registry_rebuild_count_start = MALLOC_REGISTRY_REBUILD_COUNT.with(|c| c.get());
        Self {
            malloc_registry_available,
            malloc_validation_lookups: Cell::new(0),
            malloc_registry_rebuild_count_start,
        }
    }

    #[inline]
    fn classify(&self, addr: usize) -> Option<CopyingPointer> {
        self.classify_arena(addr)
            .or_else(|| self.classify_malloc(addr))
    }

    #[inline]
    fn classify_for_preflight(
        &self,
        addr: usize,
        possible_malloc: bool,
    ) -> Result<Option<CopyingPointer>, CopiedMinorFallbackReason> {
        if let Some(ptr) = self.classify_arena(addr) {
            return Ok(Some(ptr));
        }
        if possible_malloc && !self.malloc_registry_available {
            return Err(CopiedMinorFallbackReason::MallocRegistryUnavailable);
        }
        Ok(self.classify_malloc(addr))
    }

    #[inline]
    fn classify_arena(&self, addr: usize) -> Option<CopyingPointer> {
        if addr < GC_HEADER_SIZE {
            return None;
        }
        let space = crate::arena::classify_heap_space(addr);
        if matches!(space, crate::arena::HeapSpace::Unknown) {
            return None;
        }
        let header_addr = addr - GC_HEADER_SIZE;
        if !matches!(
            crate::arena::classify_heap_space(header_addr),
            crate::arena::HeapSpace::NurseryEden
                | crate::arena::HeapSpace::Survivor0
                | crate::arena::HeapSpace::Survivor1
                | crate::arena::HeapSpace::Longlived
                | crate::arena::HeapSpace::Old
        ) {
            return None;
        }
        let header = header_addr as *mut GcHeader;
        if unsafe { !plausible_gc_header(header, true) } {
            return None;
        }
        let active_survivor = crate::arena::active_survivor_space();
        let inactive_survivor = crate::arena::inactive_survivor_space();
        let kind = match space {
            crate::arena::HeapSpace::NurseryEden => CopyingPointerKind::Eden,
            s if s == active_survivor => CopyingPointerKind::FromSurvivor,
            s if s == inactive_survivor => CopyingPointerKind::ToSurvivor,
            crate::arena::HeapSpace::Longlived => CopyingPointerKind::Longlived,
            crate::arena::HeapSpace::Old => CopyingPointerKind::Old,
            _ => return None,
        };
        Some(CopyingPointer { header, kind })
    }

    #[inline]
    fn classify_malloc(&self, addr: usize) -> Option<CopyingPointer> {
        if addr < GC_HEADER_SIZE || !self.malloc_registry_available {
            return None;
        }
        let header = unsafe { header_from_user_ptr(addr as *const u8) };
        self.malloc_validation_lookups
            .set(self.malloc_validation_lookups.get().saturating_add(1));
        let tracked = MALLOC_STATE.with(|s| s.borrow().set.contains(&(header as usize)));
        if !tracked {
            return None;
        }
        unsafe { plausible_gc_header(header, false) }.then_some(CopyingPointer {
            header,
            kind: CopyingPointerKind::Malloc,
        })
    }

    #[inline]
    fn raw_pointer_candidate(bits: u64) -> bool {
        (0x1000..=POINTER_MASK).contains(&bits) && bits & 0x7 == 0
    }

    #[inline]
    fn decode_bits(&self, bits: u64) -> Option<(usize, bool, u64)> {
        let tag = bits & TAG_MASK;
        if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
            let addr = (bits & POINTER_MASK) as usize;
            return (addr != 0).then_some((addr, true, tag));
        }
        if tag >= 0x7FF8_0000_0000_0000 {
            return None;
        }
        if !Self::raw_pointer_candidate(bits) {
            return None;
        }
        let addr = bits as usize;
        self.classify(addr).map(|_| (addr, false, 0))
    }

    #[inline]
    fn decode_bits_for_preflight(
        &self,
        bits: u64,
    ) -> Result<Option<(usize, CopyingPointer)>, CopiedMinorFallbackReason> {
        let tag = bits & TAG_MASK;
        if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
            let addr = (bits & POINTER_MASK) as usize;
            if addr == 0 {
                return Ok(None);
            }
            return self
                .classify_for_preflight(addr, true)
                .map(|ptr| ptr.map(|ptr| (addr, ptr)));
        }
        if tag >= 0x7FF8_0000_0000_0000 || !Self::raw_pointer_candidate(bits) {
            return Ok(None);
        }
        let addr = bits as usize;
        self.classify_for_preflight(addr, true)
            .map(|ptr| ptr.map(|ptr| (addr, ptr)))
    }

    #[inline]
    fn malloc_validation_lookups(&self) -> usize {
        self.malloc_validation_lookups.get()
    }

    #[inline]
    fn malloc_registry_rebuilds(&self) -> u64 {
        MALLOC_REGISTRY_REBUILD_COUNT.with(|c| {
            c.get()
                .saturating_sub(self.malloc_registry_rebuild_count_start)
        })
    }
}

unsafe fn plausible_gc_header(header: *mut GcHeader, arena: bool) -> bool {
    if header.is_null() {
        return false;
    }
    let obj_type = (*header).obj_type;
    if !(1..=GC_TYPE_LAZY_ARRAY).contains(&obj_type) {
        return false;
    }
    let size = (*header).size as usize;
    if size < GC_HEADER_SIZE || size > (1usize << 34) {
        return false;
    }
    let is_arena = (*header).gc_flags & GC_FLAG_ARENA != 0;
    is_arena == arena
}

struct CopyingNurseryPreflight {
    ptrs: *const CopyingPointerSet,
    fallback_reason: Option<CopiedMinorFallbackReason>,
    pinned_reason: CopiedMinorFallbackReason,
    worklist: Vec<*mut GcHeader>,
    seen: crate::fast_hash::PtrHashSet<usize>,
}

impl CopyingNurseryPreflight {
    fn new(ptrs: &CopyingPointerSet, pinned_reason: CopiedMinorFallbackReason) -> Self {
        Self {
            ptrs,
            fallback_reason: None,
            pinned_reason,
            worklist: Vec::new(),
            seen: crate::fast_hash::new_ptr_hash_set(),
        }
    }

    fn ptrs(&self) -> &CopyingPointerSet {
        unsafe { &*self.ptrs }
    }

    fn check_bits(&mut self, bits: u64) {
        self.check_bits_with_reason(bits, self.pinned_reason);
    }

    fn check_bits_with_reason(&mut self, bits: u64, pinned_reason: CopiedMinorFallbackReason) {
        if self.fallback_reason.is_some() {
            return;
        }
        match self.ptrs().decode_bits_for_preflight(bits) {
            Ok(Some((_addr, ptr))) => self.check_ptr_with_reason(ptr, pinned_reason),
            Ok(None) => {}
            Err(reason) => self.fallback_reason = Some(reason),
        }
    }

    fn check_addr(&mut self, addr: usize) {
        self.check_addr_with_reason(addr, self.pinned_reason);
    }

    fn check_addr_with_reason(&mut self, addr: usize, pinned_reason: CopiedMinorFallbackReason) {
        if self.fallback_reason.is_some() {
            return;
        }
        let ptr = match self.ptrs().classify_for_preflight(addr, true) {
            Ok(Some(ptr)) => ptr,
            Ok(None) => return,
            Err(reason) => {
                self.fallback_reason = Some(reason);
                return;
            }
        };
        self.check_ptr_with_reason(ptr, pinned_reason);
    }

    fn check_ptr_with_reason(
        &mut self,
        ptr: CopyingPointer,
        pinned_reason: CopiedMinorFallbackReason,
    ) {
        unsafe {
            if matches!(
                ptr.kind,
                CopyingPointerKind::Eden | CopyingPointerKind::FromSurvivor
            ) && (*ptr.header).gc_flags & GC_FLAG_PINNED != 0
            {
                self.fallback_reason = Some(pinned_reason);
                return;
            }
        }
        if matches!(
            ptr.kind,
            CopyingPointerKind::Eden
                | CopyingPointerKind::FromSurvivor
                | CopyingPointerKind::Longlived
                | CopyingPointerKind::Malloc
        ) && self.seen.insert(ptr.header as usize)
        {
            self.worklist.push(ptr.header);
        }
    }

    unsafe fn drain(&mut self) {
        let mut i = 0usize;
        while i < self.worklist.len() && self.fallback_reason.is_none() {
            let header = self.worklist[i];
            i += 1;
            if (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
                continue;
            }
            self.scan_object_fields(header);
        }
    }

    unsafe fn scan_object_fields(&mut self, header: *mut GcHeader) {
        let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
        match (*header).obj_type {
            GC_TYPE_ARRAY => self.scan_array_fields(user_ptr),
            GC_TYPE_OBJECT => self.scan_object_field_slots(user_ptr),
            GC_TYPE_CLOSURE => self.scan_closure_fields(user_ptr),
            GC_TYPE_PROMISE => self.scan_promise_fields(user_ptr),
            GC_TYPE_ERROR => self.scan_error_fields(user_ptr),
            GC_TYPE_MAP => self.scan_map_fields(user_ptr),
            GC_TYPE_LAZY_ARRAY => self.scan_lazy_array_fields(user_ptr),
            GC_TYPE_STRING | GC_TYPE_BIGINT => {}
            _ => {}
        }
    }

    unsafe fn scan_slot(&mut self, slot: *const u64) {
        if slot.is_null() {
            return;
        }
        self.check_bits_with_reason(*slot, CopiedMinorFallbackReason::PinnedYoungTransitive);
    }

    unsafe fn scan_array_fields(&mut self, user_ptr: *mut u8) {
        let arr = user_ptr as *const crate::array::ArrayHeader;
        let length = (*arr).length;
        let capacity = (*arr).capacity;
        if length > capacity || length > 16_000_000 {
            return;
        }
        let elements = user_ptr.add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const u64;
        if layout_visit_pointer_slots(user_ptr as usize, length as usize, |i| unsafe {
            self.scan_slot(elements.add(i));
        }) {
            return;
        }
        for i in 0..length as usize {
            self.scan_slot(elements.add(i));
        }
    }

    unsafe fn scan_object_field_slots(&mut self, user_ptr: *mut u8) {
        let obj = user_ptr as *const crate::object::ObjectHeader;
        let field_count = (*obj).field_count;
        if field_count > 1_000_000 {
            return;
        }
        self.scan_slot(&(*obj).keys_array as *const _ as *const u64);
        let fields = user_ptr.add(std::mem::size_of::<crate::object::ObjectHeader>()) as *const u64;
        if layout_visit_pointer_slots(user_ptr as usize, field_count as usize, |i| unsafe {
            self.scan_slot(fields.add(i));
        }) {
            return;
        }
        for i in 0..field_count as usize {
            self.scan_slot(fields.add(i));
        }
    }

    unsafe fn scan_closure_fields(&mut self, user_ptr: *mut u8) {
        let closure = user_ptr as *const crate::closure::ClosureHeader;
        let capture_count = crate::closure::real_capture_count((*closure).capture_count);
        let captures =
            user_ptr.add(std::mem::size_of::<crate::closure::ClosureHeader>()) as *const u64;
        if layout_visit_pointer_slots(user_ptr as usize, capture_count as usize, |i| unsafe {
            self.scan_slot(captures.add(i));
        }) {
            return;
        }
        for i in 0..capture_count as usize {
            self.scan_slot(captures.add(i));
        }
    }

    unsafe fn scan_promise_fields(&mut self, user_ptr: *mut u8) {
        let promise = user_ptr as *const crate::promise::Promise;
        self.scan_slot(&(*promise).value as *const f64 as *const u64);
        self.scan_slot(&(*promise).reason as *const f64 as *const u64);
        self.scan_slot(&(*promise).on_fulfilled as *const _ as *const u64);
        self.scan_slot(&(*promise).on_rejected as *const _ as *const u64);
        self.scan_slot(&(*promise).next as *const _ as *const u64);
    }

    unsafe fn scan_error_fields(&mut self, user_ptr: *mut u8) {
        let error = user_ptr as *const crate::error::ErrorHeader;
        self.scan_slot(&(*error).message as *const _ as *const u64);
        self.scan_slot(&(*error).name as *const _ as *const u64);
        self.scan_slot(&(*error).stack as *const _ as *const u64);
        self.scan_slot(&(*error).cause as *const f64 as *const u64);
        self.scan_slot(&(*error).errors as *const _ as *const u64);
    }

    unsafe fn scan_map_fields(&mut self, user_ptr: *mut u8) {
        let map = user_ptr as *const crate::map::MapHeader;
        let size = (*map).size;
        let capacity = (*map).capacity;
        if size > capacity || size > 100_000 || (*map).entries.is_null() {
            return;
        }
        let entries = (*map).entries as *const u64;
        for i in 0..(size as usize) {
            self.scan_slot(entries.add(i * 2));
            self.scan_slot(entries.add(i * 2 + 1));
        }
    }

    unsafe fn scan_lazy_array_fields(&mut self, user_ptr: *mut u8) {
        let lazy = user_ptr as *const crate::json_tape::LazyArrayHeader;
        if (*lazy).magic != crate::json_tape::LAZY_ARRAY_MAGIC {
            return;
        }
        self.scan_slot(&(*lazy).blob_str as *const _ as *const u64);
        self.scan_slot(&(*lazy).materialized as *const _ as *const u64);
        self.scan_slot(&(*lazy).materialized_elements as *const _ as *const u64);
        self.scan_slot(&(*lazy).materialized_bitmap as *const _ as *const u64);

        let cached_length = (*lazy).cached_length as usize;
        let cache = (*lazy).materialized_elements;
        let bitmap = (*lazy).materialized_bitmap;
        if cache.is_null() || bitmap.is_null() || cached_length == 0 {
            return;
        }
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
                self.scan_slot(cache.add(i) as *const u64);
            }
        }
    }
}

#[derive(Default)]
struct StickyRememberedSet {
    old_pages: crate::fast_hash::PtrHashSet<usize>,
    external_pages: Vec<(usize, usize)>,
}

impl StickyRememberedSet {
    fn remember_slot(&mut self, parent_header: *mut GcHeader, slot: *mut u64, external: bool) {
        if parent_header.is_null() || slot.is_null() {
            return;
        }
        let page = crate::arena::generation_page_for_addr(slot as usize);
        if external {
            self.external_pages.push((parent_header as usize, page));
        } else {
            self.old_pages.insert(page);
        }
    }

    fn restore(&self) {
        for &page in &self.old_pages {
            mark_dirty_old_page(page);
        }
        for &(header, page) in &self.external_pages {
            mark_dirty_external_slot_page(header, page);
        }
    }
}

struct CopyingNurseryCollector {
    ptrs: CopyingPointerSet,
    worklist: Vec<*mut GcHeader>,
    marked_headers: Vec<*mut GcHeader>,
    moved_headers: Vec<*mut GcHeader>,
    sticky: StickyRememberedSet,
    stats: CopyingNurseryTraceStats,
    live_from_bytes: usize,
}

impl CopyingNurseryCollector {
    fn new(ptrs: CopyingPointerSet) -> Self {
        Self {
            ptrs,
            worklist: Vec::new(),
            marked_headers: Vec::new(),
            moved_headers: Vec::new(),
            sticky: StickyRememberedSet::default(),
            stats: CopyingNurseryTraceStats {
                eligible: true,
                fallback_reason: CopiedMinorFallbackReason::None,
                ..CopyingNurseryTraceStats::default()
            },
            live_from_bytes: 0,
        }
    }

    fn visit_value_bits(&mut self, bits: u64) -> Option<u64> {
        let (addr, is_nanbox, tag) = self.ptrs.decode_bits(bits)?;
        let new_addr = self.mark_addr(addr)?;
        if new_addr == addr {
            return None;
        }
        Some(if is_nanbox {
            tag | (new_addr as u64 & POINTER_MASK)
        } else {
            new_addr as u64
        })
    }

    fn visit_raw_addr(&mut self, addr: usize) -> Option<usize> {
        let new_addr = self.mark_addr(addr)?;
        (new_addr != addr).then_some(new_addr)
    }

    fn rewrite_value_bits(&self, bits: u64) -> Option<u64> {
        let (addr, is_nanbox, tag) = self.ptrs.decode_bits(bits)?;
        let new_addr = self.rewrite_raw_addr(addr)?;
        Some(if is_nanbox {
            tag | (new_addr as u64 & POINTER_MASK)
        } else {
            new_addr as u64
        })
    }

    fn rewrite_raw_addr(&self, addr: usize) -> Option<usize> {
        let ptr = self.ptrs.classify(addr)?;
        unsafe {
            if (*ptr.header).gc_flags & GC_FLAG_FORWARDED == 0 {
                return None;
            }
            Some(forwarding_address(ptr.header) as usize)
        }
    }

    fn mark_addr(&mut self, addr: usize) -> Option<usize> {
        let ptr = self.ptrs.classify(addr)?;
        match ptr.kind {
            CopyingPointerKind::Eden | CopyingPointerKind::FromSurvivor => {
                Some(unsafe { self.move_young(ptr) })
            }
            CopyingPointerKind::ToSurvivor => Some(addr),
            CopyingPointerKind::Longlived | CopyingPointerKind::Malloc => {
                unsafe {
                    let flags = (*ptr.header).gc_flags;
                    if flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) == 0 {
                        (*ptr.header).gc_flags = flags | GC_FLAG_MARKED;
                        self.worklist.push(ptr.header);
                        self.marked_headers.push(ptr.header);
                    }
                }
                Some(addr)
            }
            CopyingPointerKind::Old => Some(addr),
        }
    }

    unsafe fn move_young(&mut self, ptr: CopyingPointer) -> usize {
        let header = ptr.header;
        let old_user = (header as *mut u8).add(GC_HEADER_SIZE);
        let flags = (*header).gc_flags;
        if flags & GC_FLAG_FORWARDED != 0 {
            return forwarding_address(header) as usize;
        }

        let total = (*header).size as usize;
        let payload = total - GC_HEADER_SIZE;
        let promote = matches!(ptr.kind, CopyingPointerKind::FromSurvivor)
            || flags & (GC_FLAG_HAS_SURVIVED | GC_FLAG_TENURED) != 0;
        let new_user = if promote {
            crate::arena::arena_alloc_gc_old(payload, 8, (*header).obj_type)
        } else {
            crate::arena::arena_alloc_gc_survivor(payload, 8, (*header).obj_type)
        };
        std::ptr::copy_nonoverlapping(old_user, new_user, payload);

        let new_header = header_from_user_ptr(new_user);
        (*new_header)._reserved = (*header)._reserved;
        layout_transfer(old_user, new_user);
        let preserved = flags & (GC_FLAG_SHAPE_SHARED | GC_FLAG_INTERNED | GC_FLAG_PINNED);
        (*new_header).gc_flags = GC_FLAG_ARENA
            | GC_FLAG_MARKED
            | preserved
            | if promote {
                GC_FLAG_TENURED
            } else {
                GC_FLAG_HAS_SURVIVED
            };

        set_forwarding_address(header, new_user);
        (*header).gc_flags &= !GC_FLAG_MARKED;

        self.worklist.push(new_header);
        self.moved_headers.push(new_header);
        self.live_from_bytes += total;
        if promote {
            self.stats.promoted_objects += 1;
            self.stats.promoted_bytes += total;
        } else {
            self.stats.copied_objects += 1;
            self.stats.copied_bytes += total;
        }
        new_user as usize
    }

    unsafe fn visit_slot_with_parent(
        &mut self,
        slot: *mut u64,
        parent_header: *mut GcHeader,
        external: bool,
    ) {
        if slot.is_null() {
            return;
        }
        let bits = *slot;
        if let Some(new_bits) = self.visit_value_bits(bits) {
            *slot = new_bits;
        }
        if !parent_header.is_null() {
            let parent_user = (parent_header as *mut u8).add(GC_HEADER_SIZE) as usize;
            if matches!(
                crate::arena::classify_heap_generation(parent_user),
                crate::arena::HeapGeneration::Old
            ) {
                if let Some((child_addr, _, _)) = self.ptrs.decode_bits(*slot) {
                    if crate::arena::pointer_in_nursery(child_addr) {
                        self.sticky.remember_slot(parent_header, slot, external);
                    }
                }
            }
        }
    }

    unsafe fn drain(&mut self) {
        let mut i = 0usize;
        while i < self.worklist.len() {
            let header = self.worklist[i];
            i += 1;
            if (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
                continue;
            }
            self.scan_object_fields(header);
        }
    }

    unsafe fn scan_object_fields(&mut self, header: *mut GcHeader) {
        let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
        match (*header).obj_type {
            GC_TYPE_ARRAY => self.scan_array_fields(header, user_ptr),
            GC_TYPE_OBJECT => self.scan_object_field_slots(header, user_ptr),
            GC_TYPE_CLOSURE => self.scan_closure_fields(header, user_ptr),
            GC_TYPE_PROMISE => self.scan_promise_fields(header, user_ptr),
            GC_TYPE_ERROR => self.scan_error_fields(header, user_ptr),
            GC_TYPE_MAP => self.scan_map_fields(header, user_ptr),
            GC_TYPE_LAZY_ARRAY => self.scan_lazy_array_fields(header, user_ptr),
            GC_TYPE_STRING | GC_TYPE_BIGINT => {}
            _ => {}
        }
    }

    unsafe fn scan_array_fields(&mut self, header: *mut GcHeader, user_ptr: *mut u8) {
        let arr = user_ptr as *const crate::array::ArrayHeader;
        let length = (*arr).length;
        let capacity = (*arr).capacity;
        if length > capacity || length > 16_000_000 {
            return;
        }
        let elements = user_ptr.add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64;
        if layout_visit_pointer_slots(user_ptr as usize, length as usize, |i| unsafe {
            self.visit_slot_with_parent(elements.add(i), header, false);
        }) {
            return;
        }
        for i in 0..length as usize {
            self.visit_slot_with_parent(elements.add(i), header, false);
        }
    }

    unsafe fn scan_object_field_slots(&mut self, header: *mut GcHeader, user_ptr: *mut u8) {
        let obj = user_ptr as *const crate::object::ObjectHeader;
        let field_count = (*obj).field_count;
        if field_count > 1_000_000 {
            return;
        }
        self.visit_slot_with_parent(&(*obj).keys_array as *const _ as *mut u64, header, false);
        let fields = user_ptr.add(std::mem::size_of::<crate::object::ObjectHeader>()) as *mut u64;
        if layout_visit_pointer_slots(user_ptr as usize, field_count as usize, |i| unsafe {
            self.visit_slot_with_parent(fields.add(i), header, false);
        }) {
            return;
        }
        for i in 0..field_count as usize {
            self.visit_slot_with_parent(fields.add(i), header, false);
        }
    }

    unsafe fn scan_closure_fields(&mut self, header: *mut GcHeader, user_ptr: *mut u8) {
        let closure = user_ptr as *const crate::closure::ClosureHeader;
        let capture_count = crate::closure::real_capture_count((*closure).capture_count);
        let captures =
            user_ptr.add(std::mem::size_of::<crate::closure::ClosureHeader>()) as *mut u64;
        if layout_visit_pointer_slots(user_ptr as usize, capture_count as usize, |i| unsafe {
            self.visit_slot_with_parent(captures.add(i), header, false);
        }) {
            return;
        }
        for i in 0..capture_count as usize {
            self.visit_slot_with_parent(captures.add(i), header, false);
        }
    }

    unsafe fn scan_promise_fields(&mut self, header: *mut GcHeader, user_ptr: *mut u8) {
        let promise = user_ptr as *mut crate::promise::Promise;
        self.visit_slot_with_parent(&(*promise).value as *const f64 as *mut u64, header, false);
        self.visit_slot_with_parent(&(*promise).reason as *const f64 as *mut u64, header, false);
        self.visit_slot_with_parent(
            &(*promise).on_fulfilled as *const _ as *mut u64,
            header,
            false,
        );
        self.visit_slot_with_parent(
            &(*promise).on_rejected as *const _ as *mut u64,
            header,
            false,
        );
        self.visit_slot_with_parent(&(*promise).next as *const _ as *mut u64, header, false);
    }

    unsafe fn scan_error_fields(&mut self, header: *mut GcHeader, user_ptr: *mut u8) {
        let error = user_ptr as *mut crate::error::ErrorHeader;
        self.visit_slot_with_parent(&(*error).message as *const _ as *mut u64, header, false);
        self.visit_slot_with_parent(&(*error).name as *const _ as *mut u64, header, false);
        self.visit_slot_with_parent(&(*error).stack as *const _ as *mut u64, header, false);
        self.visit_slot_with_parent(&(*error).cause as *const f64 as *mut u64, header, false);
        self.visit_slot_with_parent(&(*error).errors as *const _ as *mut u64, header, false);
    }

    unsafe fn scan_map_fields(&mut self, header: *mut GcHeader, user_ptr: *mut u8) {
        let map = user_ptr as *const crate::map::MapHeader;
        let size = (*map).size;
        let capacity = (*map).capacity;
        if size > capacity || size > 100_000 || (*map).entries.is_null() {
            return;
        }
        let entries = (*map).entries as *mut u64;
        for i in 0..(size as usize) {
            self.visit_slot_with_parent(entries.add(i * 2), header, true);
            self.visit_slot_with_parent(entries.add(i * 2 + 1), header, true);
        }
    }

    unsafe fn scan_lazy_array_fields(&mut self, header: *mut GcHeader, user_ptr: *mut u8) {
        let lazy = user_ptr as *mut crate::json_tape::LazyArrayHeader;
        if (*lazy).magic != crate::json_tape::LAZY_ARRAY_MAGIC {
            return;
        }
        self.visit_slot_with_parent(&(*lazy).blob_str as *const _ as *mut u64, header, false);
        self.visit_slot_with_parent(&(*lazy).materialized as *const _ as *mut u64, header, false);
        self.visit_slot_with_parent(
            &(*lazy).materialized_elements as *const _ as *mut u64,
            header,
            false,
        );
        self.visit_slot_with_parent(
            &(*lazy).materialized_bitmap as *const _ as *mut u64,
            header,
            false,
        );

        let cached_length = (*lazy).cached_length as usize;
        let cache = (*lazy).materialized_elements;
        let bitmap = (*lazy).materialized_bitmap;
        if cache.is_null() || bitmap.is_null() || cached_length == 0 {
            return;
        }
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
                self.visit_slot_with_parent(cache.add(i) as *mut u64, header, false);
            }
        }
    }

    unsafe fn clear_marks(&mut self) {
        for &header in &self.marked_headers {
            (*header).gc_flags &= !GC_FLAG_MARKED;
        }
        for &header in &self.moved_headers {
            (*header).gc_flags &= !GC_FLAG_MARKED;
        }
    }
}

fn copying_legacy_root_scanners_present() -> bool {
    ROOT_SCANNERS.with(|s| !s.borrow().is_empty())
        || FFI_ROOT_SCANNERS.with(|s| !s.borrow().is_empty())
}

fn scan_remembered_dirty_slots_copying(
    snapshot: &RememberedDirtySnapshot,
    mut visit: impl FnMut(*mut u64, *mut GcHeader, bool, &mut RememberedSetTraceStats),
) -> RememberedSetTraceStats {
    let mut stats = RememberedSetTraceStats {
        entries_scanned: snapshot.dirty_old_pages.len()
            + snapshot.external_dirty_entries.len()
            + snapshot.fallback_headers.len(),
        dirty_pages_before: snapshot.dirty_pages.len(),
        dirty_pages_scanned: snapshot.dirty_pages.len(),
        ..RememberedSetTraceStats::default()
    };
    let mut seen_headers = crate::fast_hash::new_ptr_hash_set();

    let mut scan_header = |header: *mut GcHeader, stats: &mut RememberedSetTraceStats| unsafe {
        if header.is_null() || !seen_headers.insert(header as usize) {
            return;
        }
        if !plausible_gc_header(header, true) {
            return;
        }
        let user = (header as *mut u8).add(GC_HEADER_SIZE) as usize;
        if !matches!(
            crate::arena::classify_heap_generation(user),
            crate::arena::HeapGeneration::Old
        ) {
            return;
        }
        stats.old_objects_considered += 1;
        stats.valid_roots += 1;
        stats.dirty_objects_scanned += 1;
        let mut visit_slot = |slot: *mut u64, stats: &mut RememberedSetTraceStats| {
            let external = !matches!(
                crate::arena::classify_heap_generation(slot as usize),
                crate::arena::HeapGeneration::Old
            );
            visit(slot, header, external, stats);
        };
        scan_dirty_object_slots(header, &snapshot.dirty_pages, stats, &mut visit_slot);
    };

    if !snapshot.dirty_old_pages.is_empty() {
        crate::arena::old_arena_walk_objects_on_pages(&snapshot.dirty_old_pages, |header| {
            scan_header(header as *mut GcHeader, &mut stats);
        });
    }
    for &(_, header_addr) in &snapshot.external_dirty_entries {
        scan_header(header_addr as *mut GcHeader, &mut stats);
    }
    for header_addr in snapshot.fallback_headers.iter().copied() {
        scan_header(header_addr as *mut GcHeader, &mut stats);
    }

    stats.dirty_pages_after = remembered_dirty_page_count();
    stats
}

struct CopiedMinorEligibility {
    eligible: bool,
    fallback_reason: CopiedMinorFallbackReason,
    malloc_sweep_due: bool,
    malloc_validation_lookups: usize,
    malloc_registry_rebuilds: u64,
    ptrs: Option<CopyingPointerSet>,
}

impl CopiedMinorEligibility {
    fn evaluate(trigger_kind: GcTriggerKind) -> Self {
        let malloc_sweep_due = copied_minor_malloc_sweep_due(trigger_kind);
        if !old_to_young_tracking_complete() {
            return Self::fallback(
                CopiedMinorFallbackReason::BarriersInactive,
                malloc_sweep_due,
            );
        }
        if matches!(
            conservative_stack_scan_decision(),
            ConservativeStackScanDecision::Scan
        ) {
            return Self::fallback(
                CopiedMinorFallbackReason::ConservativeStack,
                malloc_sweep_due,
            );
        }
        if copying_legacy_root_scanners_present() {
            return Self::fallback(CopiedMinorFallbackReason::CopyOnlyRoots, malloc_sweep_due);
        }

        let ptrs = CopyingPointerSet::new();
        if let Some(reason) = Self::mutable_root_preflight_reason(&ptrs) {
            return Self::fallback_with_ptrs(reason, malloc_sweep_due, ptrs);
        }
        if let Some(reason) = Self::dirty_slot_preflight_reason(&ptrs) {
            return Self::fallback_with_ptrs(reason, malloc_sweep_due, ptrs);
        }

        Self {
            eligible: true,
            fallback_reason: CopiedMinorFallbackReason::None,
            malloc_sweep_due,
            malloc_validation_lookups: ptrs.malloc_validation_lookups(),
            malloc_registry_rebuilds: ptrs.malloc_registry_rebuilds(),
            ptrs: Some(ptrs),
        }
    }

    fn fallback(reason: CopiedMinorFallbackReason, malloc_sweep_due: bool) -> Self {
        Self {
            eligible: false,
            fallback_reason: reason,
            malloc_sweep_due,
            malloc_validation_lookups: 0,
            malloc_registry_rebuilds: 0,
            ptrs: None,
        }
    }

    fn fallback_with_ptrs(
        reason: CopiedMinorFallbackReason,
        malloc_sweep_due: bool,
        ptrs: CopyingPointerSet,
    ) -> Self {
        Self {
            eligible: false,
            fallback_reason: reason,
            malloc_sweep_due,
            malloc_validation_lookups: ptrs.malloc_validation_lookups(),
            malloc_registry_rebuilds: ptrs.malloc_registry_rebuilds(),
            ptrs: Some(ptrs),
        }
    }

    fn trace_stats(&self) -> CopyingNurseryTraceStats {
        CopyingNurseryTraceStats {
            eligible: self.eligible,
            fallback_reason: self.fallback_reason,
            malloc_sweep_due: self.malloc_sweep_due,
            malloc_validation_lookups: self.malloc_validation_lookups,
            malloc_registry_rebuilds: self.malloc_registry_rebuilds,
            ..CopyingNurseryTraceStats::default()
        }
    }

    fn mutable_root_preflight_reason(
        ptrs: &CopyingPointerSet,
    ) -> Option<CopiedMinorFallbackReason> {
        let mut checker =
            CopyingNurseryPreflight::new(ptrs, CopiedMinorFallbackReason::PinnedYoungRoot);
        visit_mutable_root_slots(|slot| unsafe {
            checker.check_bits(slot.read());
        });
        let scanners: Vec<MutableRootScanner> = MUTABLE_ROOT_SCANNERS.with(|s| s.borrow().clone());
        {
            let mut visitor = RuntimeRootVisitor::for_copying_check(&mut checker);
            for scanner in scanners {
                scanner(&mut visitor);
            }
        }
        unsafe {
            checker.drain();
        }
        checker.fallback_reason
    }

    fn dirty_slot_preflight_reason(ptrs: &CopyingPointerSet) -> Option<CopiedMinorFallbackReason> {
        let snapshot = remembered_dirty_snapshot();
        let mut dirty_checker =
            CopyingNurseryPreflight::new(ptrs, CopiedMinorFallbackReason::PinnedYoungDirtySlot);
        scan_remembered_dirty_slots_copying(&snapshot, |slot, _header, _external, _stats| unsafe {
            dirty_checker.check_bits(*slot);
        });
        unsafe {
            dirty_checker.drain();
        }
        dirty_checker.fallback_reason
    }
}

fn gc_collect_minor_copying_fast_path(
    trace: &mut Option<GcCycleTrace>,
    start: Instant,
    trigger_kind: GcTriggerKind,
) -> Option<CopiedMinorFastPathOutcome> {
    let eligibility = CopiedMinorEligibility::evaluate(trigger_kind);
    if let Some(trace) = trace.as_mut() {
        trace.copying_nursery = eligibility.trace_stats();
    }
    if !eligibility.eligible {
        return None;
    }
    let malloc_sweep_due = eligibility.malloc_sweep_due;
    let ptrs = eligibility
        .ptrs
        .expect("eligible copied-minor decision must carry pointer classifier");

    let phase_start = trace_phase_start(trace);
    let from_space_bytes = crate::arena::copying_from_space_in_use_bytes();
    let mut collector = CopyingNurseryCollector::new(ptrs);
    collector.stats.eligible = true;
    collector.stats.fallback_reason = CopiedMinorFallbackReason::None;
    collector.stats.malloc_sweep_due = malloc_sweep_due;
    collector.stats.reset_blocks += crate::arena::copying_prepare_to_space();

    visit_mutable_root_slots(|slot| unsafe {
        let bits = slot.read();
        if matches!(slot.kind, MutableRootSlotKind::ShadowStack) {
            if let Some(trace) = trace.as_mut() {
                trace.shadow_roots.record_scan(bits);
            }
        }
        if bits == 0 {
            return;
        }
        if let Some(new_bits) = collector.visit_value_bits(bits) {
            slot.write(new_bits);
            if matches!(slot.kind, MutableRootSlotKind::ShadowStack) {
                if let Some(trace) = trace.as_mut() {
                    trace.shadow_roots.record_rewrite();
                }
            }
        }
    });

    let scanners: Vec<MutableRootScanner> = MUTABLE_ROOT_SCANNERS.with(|s| s.borrow().clone());
    {
        let mut visitor = RuntimeRootVisitor::for_copying_mark(&mut collector);
        for scanner in scanners {
            scanner(&mut visitor);
        }
    }

    let snapshot = remembered_dirty_snapshot();
    let remembered_stats =
        scan_remembered_dirty_slots_copying(&snapshot, |slot, header, external, stats| unsafe {
            let before = *slot;
            collector.visit_slot_with_parent(slot, header, external);
            if *slot != before {
                stats.newly_marked += 1;
            }
        });
    if let Some(trace) = trace.as_mut() {
        trace.remembered_set = remembered_stats;
    }

    unsafe {
        collector.drain();
    }
    {
        let scanners: Vec<MutableRootScanner> = MUTABLE_ROOT_SCANNERS.with(|s| s.borrow().clone());
        let mut visitor = RuntimeRootVisitor::for_copying_rewrite(&collector);
        for scanner in scanners {
            scanner(&mut visitor);
        }
    }
    trace_phase_record(trace, "copying_nursery", phase_start);

    if gc_verify_evacuation_enabled() {
        let phase_start = trace_phase_start(trace);
        let valid_ptrs = build_valid_pointer_set();
        verify_evacuated_no_stale_forwarded_refs(&valid_ptrs);
        trace_phase_record(trace, "evacuation_verify", phase_start);
    }

    let reset = crate::arena::copying_reset_from_spaces_and_flip();
    collector.stats.reset_blocks += reset.reset_blocks;
    remembered_set_clear();
    collector.sticky.restore();
    let malloc_freed_bytes = if malloc_sweep_due {
        let phase_start = trace_phase_start(trace);
        let freed = sweep_malloc_objects();
        trace_phase_record(trace, "malloc_sweep", phase_start);
        freed
    } else {
        0
    };
    unsafe {
        collector.clear_marks();
    }

    CONS_PINNED.with(|s| s.borrow_mut().clear());
    let nursery_freed_bytes = from_space_bytes.saturating_sub(collector.live_from_bytes) as u64;
    let freed_bytes = nursery_freed_bytes.saturating_add(malloc_freed_bytes);
    collector.stats.malloc_validation_lookups = collector.ptrs.malloc_validation_lookups();
    collector.stats.malloc_registry_rebuilds = collector.ptrs.malloc_registry_rebuilds();
    if let Some(trace) = trace.as_mut() {
        trace.copying_nursery = collector.stats;
        trace.sweep = SweepTraceStats {
            freed_bytes,
            reset_blocks: reset.reset_blocks,
            deallocated_blocks: 0,
            deallocated_bytes: 0,
            retained_forwarded_stub_objects: 0,
            retained_forwarded_stub_bytes: 0,
        };
        trace.pause_us = start.elapsed().as_micros() as u64;
    }
    Some(CopiedMinorFastPathOutcome {
        freed_bytes,
        malloc_swept: malloc_sweep_due,
    })
}

fn try_mark_young_value_as_seed(value_bits: u64, valid_ptrs: &ValidPointerSet) -> bool {
    let ptr = decode_heap_addr(value_bits);
    try_mark_young_user_ptr_as_seed(ptr, valid_ptrs)
}

fn try_mark_young_user_ptr_as_seed(ptr_val: usize, valid_ptrs: &ValidPointerSet) -> bool {
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
                // objects (until the evacuation policy moves them) still
                // hold pointers to young-gen children, and
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

fn mark_block_persisting_arena_objects(valid_ptrs: &ValidPointerSet) -> BlockPersistTraceStats {
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
    if layout_visit_pointer_slots(user_ptr as usize, length as usize, |i| {
        record_trace_slot_read();
        let val_bits = unsafe { *elements.add(i) };
        mark_field_into_worklist(val_bits, valid_ptrs, worklist);
    }) {
        return;
    }
    for i in 0..length as usize {
        record_trace_slot_read();
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
    if !layout_visit_pointer_slots(user_ptr as usize, field_count as usize, |i| {
        record_trace_slot_read();
        let val_bits = unsafe { *fields.add(i) };
        mark_field_into_worklist(val_bits, valid_ptrs, worklist);
    }) {
        for i in 0..field_count as usize {
            record_trace_slot_read();
            let val_bits = *fields.add(i);
            mark_field_into_worklist(val_bits, valid_ptrs, worklist);
        }
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
    if layout_visit_pointer_slots(user_ptr as usize, capture_count as usize, |i| {
        record_trace_slot_read();
        let val_bits = unsafe { *captures.add(i) };
        mark_field_into_worklist(val_bits, valid_ptrs, worklist);
    }) {
        return;
    }
    for i in 0..capture_count as usize {
        record_trace_slot_read();
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
#[cfg(test)]
fn sweep() -> u64 {
    sweep_with_age_bump(false).freed_bytes
}

fn sweep_malloc_objects() -> u64 {
    let mut freed_bytes: u64 = 0;

    // The malloc header registry is maintained only after activation. When
    // inactive, sweep remains a pure `objects` compaction. Once active, remove
    // freed headers inline so copied-minor can use the registry later without
    // rebuilding it.
    MALLOC_STATE.with(|s| {
        let mut s = s.borrow_mut();
        let mut i = 0;
        let registry_available = s.malloc_registry_available();
        while i < s.objects.len() {
            let header = s.objects[i];
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
                    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
                    freed_bytes += total_size as u64;
                    layout_clear_for_ptr(user_ptr as usize);

                    // For Maps, also free the separately-allocated entries array
                    // and drop the lookup side-table entry so the next allocation
                    // at this GC slot doesn't inherit stale key→index pairs.
                    if (*header).obj_type == GC_TYPE_MAP {
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
                        let promise = user_ptr as *mut crate::promise::Promise;
                        crate::async_hooks::enqueue_gc_destroy((*promise).async_id);
                        crate::promise::clear_promise_context_for_gc(promise);
                    }

                    let layout = Layout::from_size_align(total_size, 8).unwrap();
                    dealloc(header as *mut u8, layout);
                    s.objects.swap_remove(i);
                    if registry_available {
                        s.set.remove(&(header as usize));
                    }
                    // Don't increment i — swap_remove moved last element here
                } else {
                    // Surviving object — clear mark bit inline to avoid separate heap walk
                    (*header).gc_flags &= !GC_FLAG_MARKED;
                    i += 1;
                }
            }
        }
    });

    freed_bytes
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
fn sweep_with_age_bump(do_age_bump: bool) -> SweepTraceStats {
    let mut freed_bytes = sweep_malloc_objects();
    let mut retained_forwarded_stub_objects: usize = 0;
    let mut retained_forwarded_stub_bytes: usize = 0;

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
    let resettable_general_n = crate::arena::general_block_count();

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
            let age_bump_this = do_age_bump && block_idx < resettable_general_n;
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
                let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
                freed_bytes += total_size as u64;
                layout_clear_for_ptr(user_ptr as usize);
                if overflow_active && (*header).obj_type == GC_TYPE_OBJECT {
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
            // Retained FORWARDED objects must keep their containing block alive.
            // `trace_array` short-circuits on FORWARDED (it pushes the
            // forwarding TARGET onto the worklist instead of marking the
            // stub itself), so array-growth stubs reach sweep as
            // MARKED == 0 even though their first 8 bytes hold a
            // load-bearing forwarding pointer. GC-evacuation originals
            // are different: they are tracked explicitly and have
            // FORWARDED cleared after reference rewrite/verification, so
            // they fall through to the dead-object path below. If a
            // retained array-growth stub's block ends up with zero MARKED objects,
            // `arena_reset_empty_blocks` wipes it to offset=0, the
            // forwarding chain breaks, and `clean_arr_ptr` on any stale
            // old-array reference returns null. ECS demo-simple's `pipeline`
            // forEach hits this when `archetypesByComponent`'s value
            // array was reached via a forwarded chain — the next query
            // call's Map.get pointed at wiped memory and forEach
            // iterated zero entities. Treat FORWARDED as live for the
            // block-keep gate; the old payload is just an 8-byte
            // forwarding pointer, harmless to retain. Count only
            // reset-eligible general-nursery stubs in diagnostics because
            // those are the stubs that can keep a nursery block resident.
            if flags & GC_FLAG_FORWARDED != 0 {
                if block_idx < resettable_general_n {
                    retained_forwarded_stub_objects += 1;
                    retained_forwarded_stub_bytes += (*header).size as usize;
                }
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
                layout_clear_for_ptr(user_ptr as usize);

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
        let live_general = (0..resettable_general_n)
            .filter(|&i| block_has_live[i])
            .count();
        let live_ll = (resettable_general_n..n_blocks)
            .filter(|&i| block_has_live[i])
            .count();
        eprintln!(
            "[gc] blocks: general={} ({} live), longlived={} ({} live), freed_bytes={} retained_forwarded_stub_bytes={} retained_forwarded_stub_objects={}",
            resettable_general_n,
            live_general,
            n_blocks - resettable_general_n,
            live_ll,
            freed_bytes,
            retained_forwarded_stub_bytes,
            retained_forwarded_stub_objects,
        );
    }
    let reset = crate::arena::arena_reset_empty_blocks(&block_has_live);

    SweepTraceStats {
        freed_bytes,
        reset_blocks: reset.reset_blocks,
        deallocated_blocks: reset.deallocated_blocks,
        deallocated_bytes: reset.deallocated_bytes,
        retained_forwarded_stub_objects,
        retained_forwarded_stub_bytes,
    }
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

pub fn promise_mutable_root_scanner(visitor: &mut RuntimeRootVisitor<'_>) {
    crate::promise::scan_promise_roots_mut(visitor);
}

/// Root scanner for timer callbacks
pub fn timer_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::timer::scan_timer_roots(mark);
}

pub fn timer_mutable_root_scanner(visitor: &mut RuntimeRootVisitor<'_>) {
    crate::timer::scan_timer_roots_mut(visitor);
}

/// Root scanner for current exception
pub fn exception_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::exception::scan_exception_roots(mark);
}

pub fn exception_mutable_root_scanner(visitor: &mut RuntimeRootVisitor<'_>) {
    crate::exception::scan_exception_roots_mut(visitor);
}

/// Root scanner for active AsyncLocalStorage context.
pub fn async_context_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::async_context::scan_active_context_roots(mark);
    crate::builtins::scan_queued_microtask_roots(mark);
}

pub fn async_context_mutable_root_scanner(visitor: &mut RuntimeRootVisitor<'_>) {
    crate::async_context::scan_active_context_roots_mut(visitor);
    crate::builtins::scan_queued_microtask_roots_mut(visitor);
}

/// Root scanner for async_hooks hook callbacks and user resource references.
pub fn async_hooks_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::async_hooks::scan_async_hooks_roots(mark);
}

pub fn async_hooks_mutable_root_scanner(visitor: &mut RuntimeRootVisitor<'_>) {
    crate::async_hooks::scan_async_hooks_roots_mut(visitor);
}

/// Root scanner for object shape cache (keys arrays shared across objects with same shape)
pub fn shape_cache_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::object::scan_shape_cache_roots(mark);
}

pub fn shape_cache_mutable_root_scanner(visitor: &mut RuntimeRootVisitor<'_>) {
    crate::object::scan_shape_cache_roots_mut(visitor);
}

/// Root scanner for the shape-transition cache used by the dynamic-key
/// write path (`obj[name] = value`). Same role as `shape_cache_root_scanner`
/// — without it, GC would free cached target keys_arrays that no live
/// object currently references directly.
pub fn transition_cache_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::object::scan_transition_cache_roots(mark);
}

pub fn transition_cache_mutable_root_scanner(visitor: &mut RuntimeRootVisitor<'_>) {
    crate::object::scan_transition_cache_roots_mut(visitor);
}

/// Root scanner for OVERFLOW_FIELDS (per-object extra properties beyond inline slots)
pub fn overflow_fields_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::object::scan_overflow_fields_roots(mark);
}

pub fn overflow_fields_mutable_root_scanner(visitor: &mut RuntimeRootVisitor<'_>) {
    crate::object::scan_overflow_fields_roots_mut(visitor);
}

/// Root scanner for in-progress JSON.parse frames (issue #46).
/// Without this, GC triggered mid-parse would sweep in-progress arrays/objects
/// and the fresh string/object values about to be pushed into them.
pub fn json_parse_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::json::scan_parse_roots(mark);
}

pub fn json_parse_mutable_root_scanner(visitor: &mut RuntimeRootVisitor<'_>) {
    crate::json::scan_parse_roots_mut(visitor);
}

// ---------------------------------------------------------------------------
// Phase C — write barrier + remembered set
// (docs/generational-gc-plan.md §Phase C)
// ---------------------------------------------------------------------------
//
// Generational GC needs to know which old-gen regions hold
// references to young-gen objects, so a minor GC can scan just
// those dirty pages instead of the entire old-gen.
//
// The write barrier fires on every heap store. Semantics:
//   if parent is OLD and child points to YOUNG, dirty the page
//   containing the written slot.
//
// Bounded false-positive policy: dirty pages are allowed to scan
// extra slots on the same 4 KiB page; false negatives would skip a
// live young-gen object and break correctness. `REMEMBERED_SET` is
// retained only as a test fallback for the previous object-level
// HashSet behavior.

thread_local! {
    /// Dirty old-generation pages that have received a YOUNG-gen
    /// pointer since the last collection. This is Perry's compact
    /// modbuf: barriers log bounded page regions, and minor GC scans
    /// old objects intersecting those pages.
    pub(crate) static DIRTY_OLD_PAGES: std::cell::RefCell<crate::fast_hash::PtrHashSet<usize>> =
        std::cell::RefCell::new(crate::fast_hash::new_ptr_hash_set());

    /// Dirty non-arena slot pages owned by old-generation parents.
    /// `Map.entries` lives in a malloc buffer behind an old MapHeader,
    /// so its slot page cannot be discovered from the old-arena page
    /// index. Key by external page and retain the owning old headers.
    pub(crate) static EXTERNAL_DIRTY_SLOT_PAGES: std::cell::RefCell<crate::fast_hash::PtrHashMap<usize, Vec<usize>>> =
        std::cell::RefCell::new(crate::fast_hash::new_ptr_hash_map());

    /// Test-only object-level fallback remembered set. Production
    /// barriers use `DIRTY_OLD_PAGES`; tests keep this path available
    /// for parity checks and rollback coverage without a user-facing
    /// runtime mode.
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

    static WRITE_BARRIER_TRACE_COUNTERS: Cell<BarrierTraceCounters> =
        const { Cell::new(BarrierTraceCounters::zero()) };
}

static GENERATED_WRITE_BARRIERS_EMITTED: AtomicUsize = AtomicUsize::new(0);

#[no_mangle]
pub extern "C" fn js_gc_write_barriers_emitted(active: u32) {
    if active != 0 {
        GENERATED_WRITE_BARRIERS_EMITTED.fetch_add(1, Ordering::AcqRel);
    } else {
        let _ = GENERATED_WRITE_BARRIERS_EMITTED.fetch_update(
            Ordering::AcqRel,
            Ordering::Acquire,
            |count| count.checked_sub(1),
        );
    }
}

#[inline]
fn generated_write_barriers_emitted() -> bool {
    GENERATED_WRITE_BARRIERS_EMITTED.load(Ordering::Acquire) > 0
}

pub(crate) fn write_barriers_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_WRITE_BARRIERS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

#[inline]
fn old_to_young_tracking_complete() -> bool {
    generated_write_barriers_emitted() && write_barriers_enabled()
}

#[inline]
fn bump_write_barrier_trace_counter(counter: BarrierTraceCounter) {
    if !gc_trace_enabled() {
        return;
    }
    WRITE_BARRIER_TRACE_COUNTERS.with(|cell| {
        let mut counters = cell.get();
        match counter {
            BarrierTraceCounter::Calls => counters.calls += 1,
            BarrierTraceCounter::NonPointerParentSkips => counters.non_pointer_parent_skips += 1,
            BarrierTraceCounter::NonPointerChildSkips => counters.non_pointer_child_skips += 1,
            BarrierTraceCounter::ParentNotOldSkips => counters.parent_not_old_skips += 1,
            BarrierTraceCounter::ChildNotYoungSkips => counters.child_not_young_skips += 1,
            BarrierTraceCounter::RememberedSetInsertAttempts => {
                counters.remembered_set_insert_attempts += 1;
            }
            BarrierTraceCounter::NewInserts => counters.new_inserts += 1,
            BarrierTraceCounter::DirtyPageMarkAttempts => counters.dirty_page_mark_attempts += 1,
            BarrierTraceCounter::NewDirtyPages => counters.new_dirty_pages += 1,
            BarrierTraceCounter::ConservativeParentSpanMarks => {
                counters.conservative_parent_span_marks += 1;
            }
        }
        cell.set(counters);
    });
}

fn take_write_barrier_trace_counters() -> BarrierTraceCounters {
    WRITE_BARRIER_TRACE_COUNTERS.with(|cell| {
        let counters = cell.get();
        cell.set(BarrierTraceCounters::zero());
        counters
    })
}

/// Gen-GC Phase C4b: walk the current arena+malloc marked set and
/// record every header address as conservatively pinned. Returns the
/// count/bytes inserted by this stack-scan snapshot only; later
/// legacy copy-only scanner pins share CONS_PINNED for evacuation
/// safety but are reported separately in GC trace output. Called
/// after `mark_stack_roots` (the conservative scan) and before
/// mutable roots, registered scanners, and RS scan — so only the
/// conservative-scan results are captured. Subsequently-marked
/// objects from rewriteable precise sources stay out of CONS_PINNED,
/// and copy-only scanner roots are pinned directly by their callback
/// path when evacuation is enabled.
///
/// Called only from the minor-GC path. The full GC path
/// (`gc_collect_inner`) doesn't evacuate so doesn't need pinning.
fn pin_currently_marked_as_conservative() -> ConservativePinTraceStats {
    let mut stats = ConservativePinTraceStats::default();
    CONS_PINNED.with(|s| {
        let mut pinned = s.borrow_mut();
        crate::arena::arena_walk_objects(|header_ptr| {
            let header = header_ptr as *mut GcHeader;
            unsafe {
                if (*header).gc_flags & GC_FLAG_MARKED != 0 && pinned.insert(header as usize) {
                    stats.pinned_roots += 1;
                    stats.pinned_bytes += (*header).size as usize;
                }
            }
        });
        MALLOC_STATE.with(|m| {
            let m = m.borrow();
            for &header in m.objects.iter() {
                unsafe {
                    if (*header).gc_flags & GC_FLAG_MARKED != 0 && pinned.insert(header as usize) {
                        stats.pinned_roots += 1;
                        stats.pinned_bytes += (*header).size as usize;
                    }
                }
            }
        });
    });
    stats
}

/// Gen-GC Phase C4b-β: walk arena nursery objects and copy
/// non-pinned tenured ones into OLD_ARENA. Install a short-lived GC
/// forwarding pointer at the original nursery slot's user-payload
/// start. Returns evacuated object and byte counts (diagnostic only).
///
/// Candidate filter: the object must be
/// - in the nursery arena (not OLD, not LONGLIVED)
/// - MARKED (alive this cycle)
/// - TENURED (survived ≥2 minor GCs), unless
///   `PERRY_GC_FORCE_EVACUATE=1` is active for stress verification
/// - NOT in CONS_PINNED (no conservative root reaches it)
/// - NOT already FORWARDED (idempotent; duplicate evacuation is
///   safe-skipped)
///
/// Phase C4b-γ-2/3: this function is paired with
/// `rewrite_forwarded_references` and
/// `release_evacuated_original_forwarding_stubs` — every reference
/// site (heap fields, shadow stack, global roots) is rewalked AFTER
/// this function returns and any pointer to a forwarded object is
/// updated to the new address. The original's MARKED bit is cleared at
/// evac time, then its FORWARDED bit is cleared after rewrite/verify so
/// sweep treats the now-stale slot as dead and the nursery block can
/// reset; the new copy is marked MARKED so the rewrite walk picks up
/// its (copied) fields and so sweep keeps it alive.
fn evacuate_tenured_nursery_objects_collecting(
    force_evacuation: bool,
    evacuated_new_headers: &mut Vec<*mut GcHeader>,
    evacuated_original_headers: &mut Vec<*mut GcHeader>,
) -> EvacuationTraceStats {
    let mut evacuated = EvacuationTraceStats::default();
    crate::arena::arena_walk_objects(|header_ptr| {
        let header = header_ptr as *mut GcHeader;
        unsafe {
            let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
            // Skip if not in nursery (LONGLIVED + OLD have their own arenas).
            if !crate::arena::pointer_in_nursery(user_ptr as usize) {
                return;
            }
            let flags = (*header).gc_flags;
            // Already evacuated (shouldn't happen — caller's filter
            // should prevent — but defend against duplicate calls).
            if flags & GC_FLAG_FORWARDED != 0 {
                return;
            }
            // Must be alive and normally tenured. The force mode is
            // evacuation stress only and is active exclusively when the
            // outer evacuation gate is enabled.
            if flags & GC_FLAG_MARKED == 0 {
                return;
            }
            if !force_evacuation && flags & GC_FLAG_TENURED == 0 {
                return;
            }
            if flags & GC_FLAG_PINNED != 0 {
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
            // Install a GC-evacuation forwarding pointer at the original
            // nursery location. It is load-bearing only until the
            // rewrite/verify phase finishes.
            set_forwarding_address(header, new_user);
            // Clear MARKED on the original so, after the short-lived
            // FORWARDED bit is released, sweep frees its (now-stale)
            // nursery slot. The block can reset once every object in it
            // is either a released evacuation original or unmarked dead.
            (*header).gc_flags &= !GC_FLAG_MARKED;
            // Mark the new copy so (a) the rewrite walk visits
            // its fields and (b) sweep keeps it alive. The mark
            // bit is cleared inline by sweep on surviving objects.
            let new_header = (new_user as *mut u8).sub(GC_HEADER_SIZE) as *mut GcHeader;
            (*new_header)._reserved = (*header)._reserved;
            layout_transfer(user_ptr, new_user);
            (*new_header).gc_flags |= GC_FLAG_MARKED;
            // Carry TENURED forward — the new copy is logically
            // the same object, just relocated. Without this the
            // age-bump pass on the next cycle would treat it as
            // a fresh young object.
            (*new_header).gc_flags |= GC_FLAG_TENURED;
            evacuated_original_headers.push(header);
            evacuated_new_headers.push(new_header);
            evacuated.objects += 1;
            evacuated.bytes += total;
            evacuated.moved_objects += 1;
            evacuated.moved_bytes += total;
        }
    });
    evacuated
}

fn release_evacuated_original_forwarding_stubs(
    evacuated_original_headers: &[*mut GcHeader],
) -> EvacuationTraceStats {
    let mut released = EvacuationTraceStats::default();
    for &header in evacuated_original_headers {
        if header.is_null() {
            continue;
        }
        unsafe {
            let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
            if !crate::arena::pointer_in_nursery(user_ptr as usize) {
                continue;
            }
            let flags = (*header).gc_flags;
            if flags & GC_FLAG_FORWARDED == 0 {
                continue;
            }
            (*header).gc_flags = flags & !GC_FLAG_FORWARDED;
            released.released_original_objects += 1;
            released.released_original_bytes += (*header).size as usize;
        }
    }
    released
}

#[cfg(test)]
fn evacuate_tenured_nursery_objects_with_force(force_evacuation: bool) -> EvacuationTraceStats {
    let mut evacuated_new_headers = Vec::new();
    let mut evacuated_original_headers = Vec::new();
    evacuate_tenured_nursery_objects_collecting(
        force_evacuation,
        &mut evacuated_new_headers,
        &mut evacuated_original_headers,
    )
}

#[cfg(test)]
fn evacuate_tenured_nursery_objects() -> EvacuationTraceStats {
    evacuate_tenured_nursery_objects_with_force(gc_force_evacuate_enabled())
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

fn try_rewrite_nanboxed_value(bits: u64, valid_ptrs: &ValidPointerSet) -> Option<u64> {
    let tag = bits & TAG_MASK;
    if tag != POINTER_TAG && tag != STRING_TAG && tag != BIGINT_TAG {
        return None;
    }
    let ptr_addr = (bits & POINTER_MASK) as usize;
    let new_user = try_rewrite_raw_addr(ptr_addr, valid_ptrs)?;
    Some(tag | (new_user as u64 & POINTER_MASK))
}

fn try_rewrite_raw_addr(ptr_addr: usize, valid_ptrs: &ValidPointerSet) -> Option<usize> {
    if ptr_addr == 0 || !valid_ptrs.contains(&ptr_addr) {
        return None;
    }
    unsafe {
        let header = (ptr_addr as *const u8).sub(GC_HEADER_SIZE) as *const GcHeader;
        if (*header).gc_flags & GC_FLAG_FORWARDED == 0 {
            return None;
        }
        Some(forwarding_address(header) as usize)
    }
}

#[cold]
fn panic_stale_forwarded_reference(
    surface: &str,
    slot_addr: usize,
    old_bits: u64,
    new_bits: u64,
) -> ! {
    panic!(
        "gc evacuation verification failed: stale forwarded pointer in {surface}: slot=0x{slot_addr:x} old=0x{old_bits:x} forwarded_to=0x{new_bits:x}"
    );
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

#[inline]
unsafe fn verify_slot(slot: *const u64, valid_ptrs: &ValidPointerSet, surface: &str) {
    let bits = *slot;
    if let Some(new_bits) = try_rewrite_value(bits, valid_ptrs) {
        panic_stale_forwarded_reference(surface, slot as usize, bits, new_bits);
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
    if layout_visit_pointer_slots(user_ptr as usize, length as usize, |i| unsafe {
        rewrite_slot(elements.add(i), valid_ptrs);
    }) {
        return;
    }
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
    if !layout_visit_pointer_slots(user_ptr as usize, field_count as usize, |i| unsafe {
        rewrite_slot(fields.add(i), valid_ptrs);
    }) {
        for i in 0..field_count as usize {
            rewrite_slot(fields.add(i), valid_ptrs);
        }
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
    if layout_visit_pointer_slots(user_ptr as usize, capture_count as usize, |i| unsafe {
        rewrite_slot(captures.add(i), valid_ptrs);
    }) {
        return;
    }
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

unsafe fn rewrite_heap_object_fields(header: *mut GcHeader, valid_ptrs: &ValidPointerSet) {
    let flags = (*header).gc_flags;
    if flags & GC_FLAG_FORWARDED != 0 {
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
        GC_TYPE_STRING | GC_TYPE_BIGINT => {}
        _ => {}
    }
}

// Evacuation copies land in OLD_ARENA after the remembered-set scan
// for this cycle has already run. Rebuild only the pages for copied
// old objects that still hold nursery children so the next minor GC
// sees those old→young edges after the normal collection clear.
#[inline]
unsafe fn remember_evacuated_old_to_young_slot(
    sticky: &mut StickyRememberedSet,
    parent_header: *mut GcHeader,
    slot: *mut u64,
) {
    if slot.is_null() {
        return;
    }
    let child_addr = decode_heap_addr(*slot);
    if child_addr == 0 || !crate::arena::pointer_in_nursery(child_addr) {
        return;
    }
    let external = !matches!(
        crate::arena::classify_heap_generation(slot as usize),
        crate::arena::HeapGeneration::Old
    );
    sticky.remember_slot(parent_header, slot, external);
}

unsafe fn remember_evacuated_old_to_young_slot_range(
    sticky: &mut StickyRememberedSet,
    parent_header: *mut GcHeader,
    user_ptr: *mut u8,
    slots: *mut u64,
    slot_count: usize,
) {
    if slots.is_null() || slot_count == 0 {
        return;
    }
    if layout_visit_pointer_slots(user_ptr as usize, slot_count, |i| unsafe {
        remember_evacuated_old_to_young_slot(sticky, parent_header, slots.add(i));
    }) {
        return;
    }
    for i in 0..slot_count {
        remember_evacuated_old_to_young_slot(sticky, parent_header, slots.add(i));
    }
}

unsafe fn remember_evacuated_array_young_slots(
    sticky: &mut StickyRememberedSet,
    header: *mut GcHeader,
    user_ptr: *mut u8,
) {
    let arr = user_ptr as *const crate::array::ArrayHeader;
    let length = (*arr).length;
    let capacity = (*arr).capacity;
    if length > capacity || length > 16_000_000 {
        return;
    }
    let elements = user_ptr.add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64;
    remember_evacuated_old_to_young_slot_range(sticky, header, user_ptr, elements, length as usize);
}

unsafe fn remember_evacuated_object_young_slots(
    sticky: &mut StickyRememberedSet,
    header: *mut GcHeader,
    user_ptr: *mut u8,
) {
    let obj = user_ptr as *const crate::object::ObjectHeader;
    let field_count = (*obj).field_count;
    if field_count > 1_000_000 {
        return;
    }
    remember_evacuated_old_to_young_slot(
        sticky,
        header,
        &(*obj).keys_array as *const _ as *mut u64,
    );
    let fields = user_ptr.add(std::mem::size_of::<crate::object::ObjectHeader>()) as *mut u64;
    remember_evacuated_old_to_young_slot_range(
        sticky,
        header,
        user_ptr,
        fields,
        field_count as usize,
    );
}

unsafe fn remember_evacuated_closure_young_slots(
    sticky: &mut StickyRememberedSet,
    header: *mut GcHeader,
    user_ptr: *mut u8,
) {
    let closure = user_ptr as *const crate::closure::ClosureHeader;
    let capture_count = crate::closure::real_capture_count((*closure).capture_count);
    let captures = user_ptr.add(std::mem::size_of::<crate::closure::ClosureHeader>()) as *mut u64;
    remember_evacuated_old_to_young_slot_range(
        sticky,
        header,
        user_ptr,
        captures,
        capture_count as usize,
    );
}

unsafe fn remember_evacuated_promise_young_slots(
    sticky: &mut StickyRememberedSet,
    header: *mut GcHeader,
    user_ptr: *mut u8,
) {
    let promise = user_ptr as *mut crate::promise::Promise;
    remember_evacuated_old_to_young_slot(
        sticky,
        header,
        &(*promise).value as *const f64 as *mut u64,
    );
    remember_evacuated_old_to_young_slot(
        sticky,
        header,
        &(*promise).reason as *const f64 as *mut u64,
    );
    remember_evacuated_old_to_young_slot(
        sticky,
        header,
        &(*promise).on_fulfilled as *const _ as *mut u64,
    );
    remember_evacuated_old_to_young_slot(
        sticky,
        header,
        &(*promise).on_rejected as *const _ as *mut u64,
    );
    remember_evacuated_old_to_young_slot(sticky, header, &(*promise).next as *const _ as *mut u64);
}

unsafe fn remember_evacuated_error_young_slots(
    sticky: &mut StickyRememberedSet,
    header: *mut GcHeader,
    user_ptr: *mut u8,
) {
    let error = user_ptr as *mut crate::error::ErrorHeader;
    remember_evacuated_old_to_young_slot(sticky, header, &(*error).message as *const _ as *mut u64);
    remember_evacuated_old_to_young_slot(sticky, header, &(*error).name as *const _ as *mut u64);
    remember_evacuated_old_to_young_slot(sticky, header, &(*error).stack as *const _ as *mut u64);
    remember_evacuated_old_to_young_slot(sticky, header, &(*error).cause as *const f64 as *mut u64);
    remember_evacuated_old_to_young_slot(sticky, header, &(*error).errors as *const _ as *mut u64);
}

unsafe fn remember_evacuated_map_young_slots(
    sticky: &mut StickyRememberedSet,
    header: *mut GcHeader,
    user_ptr: *mut u8,
) {
    let map = user_ptr as *const crate::map::MapHeader;
    let size = (*map).size;
    let capacity = (*map).capacity;
    if size > capacity || size > 100_000 || (*map).entries.is_null() {
        return;
    }
    let entries = (*map).entries as *mut u64;
    for i in 0..(size as usize) {
        remember_evacuated_old_to_young_slot(sticky, header, entries.add(i * 2));
        remember_evacuated_old_to_young_slot(sticky, header, entries.add(i * 2 + 1));
    }
}

unsafe fn remember_evacuated_lazy_array_young_slots(
    sticky: &mut StickyRememberedSet,
    header: *mut GcHeader,
    user_ptr: *mut u8,
) {
    let lazy = user_ptr as *mut crate::json_tape::LazyArrayHeader;
    if (*lazy).magic != crate::json_tape::LAZY_ARRAY_MAGIC {
        return;
    }
    remember_evacuated_old_to_young_slot(sticky, header, &(*lazy).blob_str as *const _ as *mut u64);
    remember_evacuated_old_to_young_slot(
        sticky,
        header,
        &(*lazy).materialized as *const _ as *mut u64,
    );
    remember_evacuated_old_to_young_slot(
        sticky,
        header,
        &(*lazy).materialized_elements as *const _ as *mut u64,
    );
    remember_evacuated_old_to_young_slot(
        sticky,
        header,
        &(*lazy).materialized_bitmap as *const _ as *mut u64,
    );

    let cached_length = (*lazy).cached_length as usize;
    let cache = (*lazy).materialized_elements;
    let bitmap = (*lazy).materialized_bitmap;
    if cache.is_null() || bitmap.is_null() || cached_length == 0 {
        return;
    }
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
            remember_evacuated_old_to_young_slot(sticky, header, cache.add(i) as *mut u64);
        }
    }
}

unsafe fn remember_evacuated_old_copy_young_slots(
    sticky: &mut StickyRememberedSet,
    header: *mut GcHeader,
) {
    if header.is_null() {
        return;
    }
    let flags = (*header).gc_flags;
    if flags & GC_FLAG_FORWARDED != 0 || flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) == 0 {
        return;
    }
    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
    if !crate::arena::pointer_in_old_gen(user_ptr as usize) {
        return;
    }
    match (*header).obj_type {
        GC_TYPE_ARRAY => remember_evacuated_array_young_slots(sticky, header, user_ptr),
        GC_TYPE_OBJECT => remember_evacuated_object_young_slots(sticky, header, user_ptr),
        GC_TYPE_CLOSURE => remember_evacuated_closure_young_slots(sticky, header, user_ptr),
        GC_TYPE_PROMISE => remember_evacuated_promise_young_slots(sticky, header, user_ptr),
        GC_TYPE_ERROR => remember_evacuated_error_young_slots(sticky, header, user_ptr),
        GC_TYPE_MAP => remember_evacuated_map_young_slots(sticky, header, user_ptr),
        GC_TYPE_LAZY_ARRAY => remember_evacuated_lazy_array_young_slots(sticky, header, user_ptr),
        GC_TYPE_STRING | GC_TYPE_BIGINT => {}
        _ => {}
    }
}

fn rebuild_evacuated_old_to_young_remembered_set(
    evacuated_headers: &[*mut GcHeader],
) -> StickyRememberedSet {
    let mut sticky = StickyRememberedSet::default();
    for &header in evacuated_headers {
        unsafe {
            remember_evacuated_old_copy_young_slots(&mut sticky, header);
        }
    }
    sticky
}

unsafe fn verify_array_fields(user_ptr: *mut u8, valid_ptrs: &ValidPointerSet, surface: &str) {
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
        (user_ptr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const u64;
    if layout_visit_pointer_slots(user_ptr as usize, length as usize, |i| unsafe {
        verify_slot(elements.add(i), valid_ptrs, surface);
    }) {
        return;
    }
    for i in 0..length as usize {
        verify_slot(elements.add(i), valid_ptrs, surface);
    }
}

unsafe fn verify_object_fields(user_ptr: *mut u8, valid_ptrs: &ValidPointerSet, surface: &str) {
    let obj = user_ptr as *const crate::object::ObjectHeader;
    let field_count = (*obj).field_count;
    if field_count > 1_000_000 {
        return;
    }
    let fields = (user_ptr as *const u8).add(std::mem::size_of::<crate::object::ObjectHeader>())
        as *const u64;
    if !layout_visit_pointer_slots(user_ptr as usize, field_count as usize, |i| unsafe {
        verify_slot(fields.add(i), valid_ptrs, surface);
    }) {
        for i in 0..field_count as usize {
            verify_slot(fields.add(i), valid_ptrs, surface);
        }
    }
    verify_slot(
        &(*obj).keys_array as *const _ as *const u64,
        valid_ptrs,
        surface,
    );
}

unsafe fn verify_map_fields(user_ptr: *mut u8, valid_ptrs: &ValidPointerSet, surface: &str) {
    let map = user_ptr as *const crate::map::MapHeader;
    let size = (*map).size;
    let capacity = (*map).capacity;
    if size > capacity || size > 100_000 || (*map).entries.is_null() {
        return;
    }
    let entries = (*map).entries as *const u64;
    for i in 0..(size as usize) {
        verify_slot(entries.add(i * 2), valid_ptrs, surface);
        verify_slot(entries.add(i * 2 + 1), valid_ptrs, surface);
    }
}

unsafe fn verify_closure_fields(user_ptr: *mut u8, valid_ptrs: &ValidPointerSet, surface: &str) {
    let closure = user_ptr as *const crate::closure::ClosureHeader;
    let capture_count = crate::closure::real_capture_count((*closure).capture_count);
    let captures = (user_ptr as *const u8).add(std::mem::size_of::<crate::closure::ClosureHeader>())
        as *const u64;
    if layout_visit_pointer_slots(user_ptr as usize, capture_count as usize, |i| unsafe {
        verify_slot(captures.add(i), valid_ptrs, surface);
    }) {
        return;
    }
    for i in 0..capture_count as usize {
        verify_slot(captures.add(i), valid_ptrs, surface);
    }
}

unsafe fn verify_promise_fields(user_ptr: *mut u8, valid_ptrs: &ValidPointerSet, surface: &str) {
    let promise = user_ptr as *const crate::promise::Promise;
    verify_slot(
        &(*promise).value as *const f64 as *const u64,
        valid_ptrs,
        surface,
    );
    verify_slot(
        &(*promise).reason as *const f64 as *const u64,
        valid_ptrs,
        surface,
    );
    verify_slot(
        &(*promise).on_fulfilled as *const _ as *const u64,
        valid_ptrs,
        surface,
    );
    verify_slot(
        &(*promise).on_rejected as *const _ as *const u64,
        valid_ptrs,
        surface,
    );
    verify_slot(
        &(*promise).next as *const _ as *const u64,
        valid_ptrs,
        surface,
    );
}

unsafe fn verify_error_fields(user_ptr: *mut u8, valid_ptrs: &ValidPointerSet, surface: &str) {
    let error = user_ptr as *const crate::error::ErrorHeader;
    verify_slot(
        &(*error).message as *const _ as *const u64,
        valid_ptrs,
        surface,
    );
    verify_slot(
        &(*error).name as *const _ as *const u64,
        valid_ptrs,
        surface,
    );
    verify_slot(
        &(*error).stack as *const _ as *const u64,
        valid_ptrs,
        surface,
    );
    verify_slot(
        &(*error).cause as *const f64 as *const u64,
        valid_ptrs,
        surface,
    );
    verify_slot(
        &(*error).errors as *const _ as *const u64,
        valid_ptrs,
        surface,
    );
}

unsafe fn verify_lazy_array_fields(user_ptr: *mut u8, valid_ptrs: &ValidPointerSet, surface: &str) {
    let lazy = user_ptr as *const crate::json_tape::LazyArrayHeader;
    if (*lazy).magic != crate::json_tape::LAZY_ARRAY_MAGIC {
        return;
    }
    verify_slot(
        &(*lazy).blob_str as *const _ as *const u64,
        valid_ptrs,
        surface,
    );
    verify_slot(
        &(*lazy).materialized as *const _ as *const u64,
        valid_ptrs,
        surface,
    );
    verify_slot(
        &(*lazy).materialized_elements as *const _ as *const u64,
        valid_ptrs,
        surface,
    );
    verify_slot(
        &(*lazy).materialized_bitmap as *const _ as *const u64,
        valid_ptrs,
        surface,
    );

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
                verify_slot(cache.add(i) as *const u64, valid_ptrs, surface);
            }
        }
    }
}

unsafe fn verify_heap_object_fields(
    header: *mut GcHeader,
    valid_ptrs: &ValidPointerSet,
    surface: &'static str,
) {
    let flags = (*header).gc_flags;
    if flags & GC_FLAG_FORWARDED != 0 {
        return;
    }
    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
    match (*header).obj_type {
        GC_TYPE_ARRAY => verify_array_fields(user_ptr, valid_ptrs, surface),
        GC_TYPE_OBJECT => verify_object_fields(user_ptr, valid_ptrs, surface),
        GC_TYPE_CLOSURE => verify_closure_fields(user_ptr, valid_ptrs, surface),
        GC_TYPE_PROMISE => verify_promise_fields(user_ptr, valid_ptrs, surface),
        GC_TYPE_ERROR => verify_error_fields(user_ptr, valid_ptrs, surface),
        GC_TYPE_MAP => verify_map_fields(user_ptr, valid_ptrs, surface),
        GC_TYPE_LAZY_ARRAY => verify_lazy_array_fields(user_ptr, valid_ptrs, surface),
        GC_TYPE_STRING | GC_TYPE_BIGINT => {}
        _ => {}
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
            rewrite_heap_object_fields(header, valid_ptrs);
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

fn rewrite_remembered_dirty_ranges(valid_ptrs: &ValidPointerSet) {
    let snapshot = remembered_dirty_snapshot();
    let mut stats = RememberedSetTraceStats::default();
    let mut rewrite_dirty_slot = |slot: *mut u64, _stats: &mut RememberedSetTraceStats| unsafe {
        rewrite_slot(slot, valid_ptrs);
    };
    scan_remembered_dirty_slot_ranges(&snapshot, valid_ptrs, &mut stats, &mut rewrite_dirty_slot);

    for header_addr in snapshot.fallback_headers {
        let user_ptr = header_addr + GC_HEADER_SIZE;
        if !valid_ptrs.contains(&user_ptr) {
            continue;
        }
        unsafe {
            rewrite_heap_object_fields(header_addr as *mut GcHeader, valid_ptrs);
        }
    }
}

/// Walk every mutable root slot and rewrite forwarded pointers.
/// Shadow slots are NaN-boxed JSValues; globals can be NaN-boxed or
/// raw object-start pointers. `try_rewrite_value` handles both forms.
fn rewrite_mutable_root_slots(
    valid_ptrs: &ValidPointerSet,
    mut shadow_stats: Option<&mut ShadowRootTraceStats>,
) {
    visit_mutable_root_slots(|slot| unsafe {
        let bits = slot.read();
        if bits == 0 {
            return;
        }
        if let Some(new_bits) = try_rewrite_value(bits, valid_ptrs) {
            slot.write(new_bits);
            if matches!(slot.kind, MutableRootSlotKind::ShadowStack) {
                if let Some(stats) = shadow_stats.as_mut() {
                    stats.record_rewrite();
                }
            }
        }
    });
}

fn rewrite_mutable_registered_roots(valid_ptrs: &ValidPointerSet) {
    let scanners: Vec<MutableRootScanner> = MUTABLE_ROOT_SCANNERS.with(|s| s.borrow().clone());
    let mut visitor = RuntimeRootVisitor::for_rewrite(valid_ptrs);
    for scanner in scanners {
        scanner(&mut visitor);
    }
}

fn verify_mutable_root_slots(valid_ptrs: &ValidPointerSet) {
    visit_mutable_root_slots(|slot| unsafe {
        let bits = slot.read();
        if bits == 0 {
            return;
        }
        if let Some(new_bits) = try_rewrite_value(bits, valid_ptrs) {
            let surface = match slot.kind {
                MutableRootSlotKind::ShadowStack => "shadow stack roots",
                MutableRootSlotKind::GlobalRoot => "global roots",
            };
            panic_stale_forwarded_reference(surface, slot.ptr as usize, bits, new_bits);
        }
    });
}

fn verify_mutable_registered_roots(valid_ptrs: &ValidPointerSet) {
    let scanners: Vec<MutableRootScanner> = MUTABLE_ROOT_SCANNERS.with(|s| s.borrow().clone());
    let mut visitor = RuntimeRootVisitor::for_verify(valid_ptrs, "runtime mutable root scanner");
    for scanner in scanners {
        scanner(&mut visitor);
    }
}

fn verify_copy_only_scanner_bits(bits: u64, valid_ptrs: &ValidPointerSet, surface: &'static str) {
    if let Some(new_bits) = try_rewrite_nanboxed_value(bits, valid_ptrs) {
        panic_stale_forwarded_reference(surface, 0, bits, new_bits);
    }
}

struct RegisteredRootVerifyContext {
    valid_ptrs: *const ValidPointerSet,
}

extern "C" fn perry_ffi_verify_root(value: f64, ctx: *mut c_void) {
    if ctx.is_null() {
        return;
    }
    let ctx = unsafe { &*(ctx as *const RegisteredRootVerifyContext) };
    if ctx.valid_ptrs.is_null() {
        return;
    }
    let valid_ptrs = unsafe { &*ctx.valid_ptrs };
    verify_copy_only_scanner_bits(value.to_bits(), valid_ptrs, "ffi copy-only root scanner");
}

fn verify_copy_only_registered_roots(valid_ptrs: &ValidPointerSet) {
    let scanners: Vec<fn(&mut dyn FnMut(f64))> = ROOT_SCANNERS.with(|s| s.borrow().clone());
    for scanner in scanners {
        scanner(&mut |value: f64| {
            verify_copy_only_scanner_bits(value.to_bits(), valid_ptrs, "copy-only root scanner");
        });
    }

    let ffi_scanners: Vec<PerryFfiRootScanner> = FFI_ROOT_SCANNERS.with(|s| s.borrow().clone());
    let mut ctx = RegisteredRootVerifyContext {
        valid_ptrs: valid_ptrs as *const ValidPointerSet,
    };
    let ctx = &mut ctx as *mut RegisteredRootVerifyContext as *mut c_void;
    for scanner in ffi_scanners {
        scanner(perry_ffi_verify_root, ctx);
    }
}

fn verify_remembered_dirty_ranges(valid_ptrs: &ValidPointerSet) {
    let snapshot = remembered_dirty_snapshot();
    let mut stats = RememberedSetTraceStats::default();
    let mut verify_dirty_slot = |slot: *mut u64, _stats: &mut RememberedSetTraceStats| unsafe {
        verify_slot(slot as *const u64, valid_ptrs, "remembered dirty ranges");
    };
    scan_remembered_dirty_slot_ranges(&snapshot, valid_ptrs, &mut stats, &mut verify_dirty_slot);

    for header_addr in snapshot.fallback_headers {
        let user_ptr = header_addr + GC_HEADER_SIZE;
        if !valid_ptrs.contains(&user_ptr) {
            continue;
        }
        unsafe {
            verify_heap_object_fields(
                header_addr as *mut GcHeader,
                valid_ptrs,
                "remembered fallback headers",
            );
        }
    }
}

fn verify_heap_objects(valid_ptrs: &ValidPointerSet) {
    let verify_one = |header: *mut GcHeader| unsafe {
        let flags = (*header).gc_flags;
        if flags & GC_FLAG_FORWARDED != 0 {
            return;
        }
        if flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) == 0 {
            return;
        }
        verify_heap_object_fields(header, valid_ptrs, "heap fields");
    };
    crate::arena::arena_walk_objects(|hp| verify_one(hp as *mut GcHeader));
    MALLOC_STATE.with(|s| {
        let s = s.borrow();
        for &h in s.objects.iter() {
            verify_one(h);
        }
    });
}

fn verify_evacuated_no_stale_forwarded_refs(valid_ptrs: &ValidPointerSet) {
    verify_mutable_root_slots(valid_ptrs);
    verify_mutable_registered_roots(valid_ptrs);
    verify_copy_only_registered_roots(valid_ptrs);
    verify_remembered_dirty_ranges(valid_ptrs);
    verify_heap_objects(valid_ptrs);
}

/// Top-level Phase C4b-γ-2 entry: rewrite every reference site we
/// own. Skipped: conservatively-discovered C-stack words (we can't
/// safely overwrite arbitrary stack memory; pinning of conservative-
/// root targets in `gc_collect_minor` keeps those references valid
/// without rewriting). Legacy copy-only scanners still pin their own
/// discoveries directly during root marking.
fn rewrite_forwarded_references(
    valid_ptrs: &ValidPointerSet,
    shadow_stats: Option<&mut ShadowRootTraceStats>,
) {
    rewrite_mutable_root_slots(valid_ptrs, shadow_stats);
    rewrite_mutable_registered_roots(valid_ptrs);
    rewrite_remembered_dirty_ranges(valid_ptrs);
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

/// Gen-GC Phase C1: compatibility write barrier. Test callers and
/// older bitcode still call this two-argument form; it conservatively
/// dirties the parent object's occupied pages.
#[no_mangle]
pub extern "C" fn js_write_barrier(parent: u64, child: u64) {
    js_write_barrier_slot(parent, 0, child);
}

/// Gen-GC Phase C1: slot-aware write barrier. Called by
/// codegen-emitted store sites unless `PERRY_WRITE_BARRIERS=0`/
/// `off`/`false` disabled barrier emission at compile time.
///
/// Decode the parent + child as raw addresses. If parent's
/// GcHeader sits in the old-gen arena AND child's NaN-boxed
/// pointer (any of POINTER / STRING / BIGINT / SHORT_STRING)
/// resolves to a heap address inside the nursery, dirty the page
/// containing the written slot. A zero slot address falls back to
/// dirtying every occupied page in the parent object.
///
/// Hot-path constraints: this fires on EVERY heap store in
/// compiled code by default. Must be cheap:
/// generation checks use arena page side metadata rather than
/// scanning every arena block.
#[no_mangle]
pub extern "C" fn js_write_barrier_slot(parent: u64, slot_addr: u64, child: u64) {
    write_barrier_slot_inner(parent, slot_addr as usize, child, false);
}

fn write_barrier_slot_inner(parent: u64, slot_addr: usize, child: u64, external_slot: bool) {
    bump_write_barrier_trace_counter(BarrierTraceCounter::Calls);

    // Decode child first: primitive stores are the most common skip.
    let child_addr = decode_heap_addr(child);
    if child_addr == 0 {
        bump_write_barrier_trace_counter(BarrierTraceCounter::NonPointerChildSkips);
        return;
    }
    // Decode the parent — must be a NaN-boxed heap pointer.
    let parent_addr = decode_heap_addr(parent);
    if parent_addr == 0 {
        bump_write_barrier_trace_counter(BarrierTraceCounter::NonPointerParentSkips);
        return;
    }
    // Old → young check.
    if !matches!(
        crate::arena::classify_heap_generation(parent_addr),
        crate::arena::HeapGeneration::Old
    ) {
        bump_write_barrier_trace_counter(BarrierTraceCounter::ParentNotOldSkips);
        return;
    }
    if !matches!(
        crate::arena::classify_heap_generation(child_addr),
        crate::arena::HeapGeneration::Nursery
    ) {
        bump_write_barrier_trace_counter(BarrierTraceCounter::ChildNotYoungSkips);
        return;
    }

    bump_write_barrier_trace_counter(BarrierTraceCounter::RememberedSetInsertAttempts);
    let inserted = if external_slot {
        remember_old_to_young_external_slot(parent_addr, slot_addr)
    } else {
        remember_old_to_young_slot(parent_addr, slot_addr)
    };
    if inserted {
        bump_write_barrier_trace_counter(BarrierTraceCounter::NewInserts);
    }
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
        // Possible raw pointer. Accept only if the arena side metadata
        // recognizes it as a heap address; ordinary f64 payload bits
        // miss the metadata table and remain non-pointers.
        let addr = bits as usize;
        if matches!(
            crate::arena::classify_heap_generation(addr),
            crate::arena::HeapGeneration::Unknown
        ) {
            0
        } else {
            addr
        }
    } else {
        // SHORT_STRING_TAG (0x7FF9), INT32_TAG (0x7FFE),
        // primitive (0x7FFC), JS_HANDLE (0x7FFB) — none are
        // young-gen pointers.
        0
    }
}

fn remember_old_to_young_slot(parent_addr: usize, slot_addr: usize) -> bool {
    if slot_addr != 0
        && matches!(
            crate::arena::classify_heap_generation(slot_addr),
            crate::arena::HeapGeneration::Old
        )
    {
        return mark_dirty_old_page(crate::arena::generation_page_for_addr(slot_addr));
    }
    bump_write_barrier_trace_counter(BarrierTraceCounter::ConservativeParentSpanMarks);
    mark_dirty_parent_span(parent_addr)
}

fn mark_dirty_parent_span(parent_addr: usize) -> bool {
    if parent_addr < GC_HEADER_SIZE {
        return false;
    }
    let header_addr = parent_addr - GC_HEADER_SIZE;
    let header = header_addr as *const GcHeader;
    let total_size = unsafe { (*header).size as usize };
    if total_size == 0 {
        return false;
    }
    let first_page = crate::arena::generation_page_for_addr(header_addr);
    let last_page = crate::arena::generation_page_for_addr(header_addr + total_size - 1);
    let mut inserted_any = false;
    for page in first_page..=last_page {
        inserted_any |= mark_dirty_old_page(page);
    }
    inserted_any
}

fn remember_old_to_young_external_slot(parent_addr: usize, slot_addr: usize) -> bool {
    if slot_addr == 0 || parent_addr < GC_HEADER_SIZE {
        return false;
    }
    let header_addr = parent_addr - GC_HEADER_SIZE;
    mark_dirty_external_slot_page(
        header_addr,
        crate::arena::generation_page_for_addr(slot_addr),
    )
}

fn mark_dirty_old_page(page: usize) -> bool {
    bump_write_barrier_trace_counter(BarrierTraceCounter::DirtyPageMarkAttempts);
    DIRTY_OLD_PAGES.with(|s| {
        let inserted = s.borrow_mut().insert(page);
        if inserted {
            bump_write_barrier_trace_counter(BarrierTraceCounter::NewDirtyPages);
        }
        inserted
    })
}

fn mark_dirty_external_slot_page(header_addr: usize, page: usize) -> bool {
    bump_write_barrier_trace_counter(BarrierTraceCounter::DirtyPageMarkAttempts);
    EXTERNAL_DIRTY_SLOT_PAGES.with(|s| {
        let mut pages = s.borrow_mut();
        let page_was_new = !pages.contains_key(&page);
        let headers = pages.entry(page).or_insert_with(Vec::new);
        let header_was_new = if headers.contains(&header_addr) {
            false
        } else {
            headers.push(header_addr);
            true
        };
        if page_was_new {
            bump_write_barrier_trace_counter(BarrierTraceCounter::NewDirtyPages);
        }
        header_was_new
    })
}

pub(crate) fn runtime_write_barrier_slot(parent_addr: usize, slot_addr: usize, child_bits: u64) {
    if !write_barriers_enabled() {
        return;
    }
    js_write_barrier_slot(parent_addr as u64, slot_addr as u64, child_bits);
}

pub(crate) fn runtime_write_barrier_external_slot(
    parent_addr: usize,
    slot_addr: usize,
    child_bits: u64,
) {
    if !write_barriers_enabled() {
        return;
    }
    write_barrier_slot_inner(parent_addr as u64, slot_addr, child_bits, true);
}

pub(crate) fn runtime_dirty_external_slot_span(
    parent_addr: usize,
    first_slot_addr: usize,
    slot_count: usize,
) {
    if !write_barriers_enabled() {
        return;
    }
    dirty_external_slot_span(parent_addr, first_slot_addr, slot_count);
}

fn dirty_external_slot_span(parent_addr: usize, first_slot_addr: usize, slot_count: usize) {
    if parent_addr < GC_HEADER_SIZE || first_slot_addr == 0 || slot_count == 0 {
        return;
    }
    if !matches!(
        crate::arena::classify_heap_generation(parent_addr),
        crate::arena::HeapGeneration::Old
    ) {
        return;
    }
    let Some(bytes) = slot_count.checked_mul(std::mem::size_of::<u64>()) else {
        return;
    };
    let Some(last_byte) = first_slot_addr.checked_add(bytes.saturating_sub(1)) else {
        return;
    };
    bump_write_barrier_trace_counter(BarrierTraceCounter::ConservativeParentSpanMarks);
    let header_addr = parent_addr - GC_HEADER_SIZE;
    let first_page = crate::arena::generation_page_for_addr(first_slot_addr);
    let last_page = crate::arena::generation_page_for_addr(last_byte);
    for page in first_page..=last_page {
        mark_dirty_external_slot_page(header_addr, page);
    }
}

fn remembered_dirty_page_count() -> usize {
    DIRTY_OLD_PAGES.with(|old| {
        let old = old.borrow();
        EXTERNAL_DIRTY_SLOT_PAGES.with(|external| {
            let external = external.borrow();
            if external.is_empty() {
                return old.len();
            }
            let mut pages = crate::fast_hash::new_ptr_hash_set();
            for &page in old.iter() {
                pages.insert(page);
            }
            for &page in external.keys() {
                pages.insert(page);
            }
            pages.len()
        })
    })
}

/// Gen-GC Phase C: read the current remembered set size — used
/// by tests and `PERRY_GC_DIAG=1` output to confirm barrier
/// activity. Returns 0 in Phase C1 since no codegen-emitted
/// barrier has fired yet.
pub fn remembered_set_size() -> usize {
    remembered_dirty_page_count() + REMEMBERED_SET.with(|s| s.borrow().len())
}

/// Gen-GC Phase C: clear the remembered set. Will be called by
/// minor GC after the rs-scan completes (Phase C3). Test-only
/// for now to enable test isolation.
pub fn remembered_set_clear() {
    DIRTY_OLD_PAGES.with(|s| s.borrow_mut().clear());
    EXTERNAL_DIRTY_SLOT_PAGES.with(|s| s.borrow_mut().clear());
    REMEMBERED_SET.with(|s| s.borrow_mut().clear());
}

/// Compatibility scanner for the shadow stack.
/// Walks every live slot in every pushed frame and invokes `mark`
/// with the slot's NaN-boxed f64 value. The mark callback's
/// `try_mark_value` pipeline already knows how to distinguish
/// plain numbers / undefined / null / booleans (skipped) from
/// POINTER_TAG / STRING_TAG / BIGINT_TAG / SHORT_STRING_TAG
/// values that refer to heap objects.
///
/// GC marking now walks shadow slots through `mark_mutable_root_slots`
/// so they can share the same slot visitor used by forwarding rewrite.
/// This public function keeps the previous scanner shape available to
/// tests and any internal caller that still wants mark-only iteration.
///
/// Zero-slot frames (functions where no local is pointer-typed)
/// contribute nothing — the inner loop's `slots_count == 0` exits
/// immediately. Empty shadow stack (no function call currently
/// active, or PERRY_SHADOW_STACK=0 at compile time so push/pop
/// never emitted) also contributes nothing.
pub fn shadow_stack_root_scanner(mark: &mut dyn FnMut(f64)) {
    visit_shadow_stack_root_slots(|slot| unsafe {
        let bits = slot.read();
        if bits != 0 {
            mark(f64::from_bits(bits));
        }
    });
}

/// Initialize GC root scanners. Called once at runtime startup.
pub fn gc_init() {
    gc_register_mutable_root_scanner(promise_mutable_root_scanner);
    gc_register_mutable_root_scanner(timer_mutable_root_scanner);
    gc_register_mutable_root_scanner(exception_mutable_root_scanner);
    gc_register_mutable_root_scanner(async_context_mutable_root_scanner);
    gc_register_mutable_root_scanner(async_hooks_mutable_root_scanner);
    gc_register_mutable_root_scanner(shape_cache_mutable_root_scanner);
    gc_register_mutable_root_scanner(crate::regex::scan_last_exec_groups_root_mut);
    gc_register_mutable_root_scanner(crate::array::scan_template_raw_roots_mut);
    gc_register_mutable_root_scanner(transition_cache_mutable_root_scanner);
    gc_register_mutable_root_scanner(overflow_fields_mutable_root_scanner);
    gc_register_mutable_root_scanner(json_parse_mutable_root_scanner);
    gc_register_mutable_root_scanner(intern_table_mutable_root_scanner);
    gc_register_mutable_root_scanner(crate::builtins::scan_console_log_singleton_roots_mut);
    // Issue #841: GC roots for the per-(submodule, export) function
    // singletons + per-submodule namespace stub objects allocated by
    // `node_submodules.rs`. Without this scanner the next GC cycle
    // after first import-binding use would reclaim the singletons
    // (nothing else holds them — they live for the program's lifetime
    // via codegen `getter` calls, not via a user-visible JSValue root).
    gc_register_mutable_root_scanner(
        crate::node_submodules::scan_node_submodule_singleton_roots_mut,
    );
    // Box-capture root scanner (mutable closure captures, esp. the
    // generator state-machine's `__iter` and `__step` boxes that hold
    // the iter object + step closure across awaits).
    gc_register_mutable_root_scanner(crate::r#box::scan_box_roots_mut);
    // Iter-result scratch slot — the async-step fast path stows the
    // generator's most recent yield value here; it stays live until
    // the step driver reads it back.
    gc_register_mutable_root_scanner(crate::promise::scan_iter_result_root_mut);
    // Async-step thunk single-slot cache (build_async_step_thunks).
    gc_register_mutable_root_scanner(crate::promise::scan_async_step_thunk_cache_mut);
    // Closure singleton caches. Captured-closure cache keys mirror closure
    // capture heap words, so copied-minor must rewrite them after moving
    // captured young values or future cache hits miss on stale addresses.
    gc_register_mutable_root_scanner(crate::closure::scan_singleton_closure_roots_mut);
    // perry/tui hook + state slot pools — they store raw NaN-boxed
    // value bits but the GC has no other way to know which slots hold
    // heap pointers (arrays/objects/strings stashed via setState /
    // useState / useRef). #679 follow-up: pre-fix, an Enter-press in
    // the perry-code demo stored a freshly-concat'd messages array,
    // the next allocation triggered minor GC, and the array was
    // reclaimed because nothing else held it — `messages.map(…)` on
    // the stale pointer produced an empty render.
    gc_register_mutable_root_scanner(crate::tui::hooks::scan_hook_slot_roots_mut);
    gc_register_mutable_root_scanner(crate::tui::state::scan_state_slot_roots_mut);
    #[cfg(feature = "ohos-napi")]
    gc_register_mutable_root_scanner(crate::arkts_callbacks::arkts_callbacks_root_scanner_mut);
}

/// Root scanner for the string intern table.
pub fn intern_table_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::string::scan_intern_table_roots(mark);
}

pub fn intern_table_mutable_root_scanner(visitor: &mut RuntimeRootVisitor<'_>) {
    crate::string::scan_intern_table_roots_mut(visitor);
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
            init_test_closure(closure_ptr);
            assert_eq!((*header_from_user_ptr(string_ptr)).obj_type, GC_TYPE_STRING);
            assert_eq!(
                (*header_from_user_ptr(closure_ptr)).obj_type,
                GC_TYPE_CLOSURE
            );
            assert_eq!((*header_from_user_ptr(bigint_ptr)).obj_type, GC_TYPE_BIGINT);
        }
    }

    #[test]
    fn test_sweep_removes_unmarked_malloc_object() {
        let ptr = gc_malloc(64, GC_TYPE_STRING);
        let header = unsafe { header_from_user_ptr(ptr) };
        let header_addr = header as usize;

        let tracked_before = MALLOC_STATE.with(|s| {
            s.borrow()
                .objects
                .iter()
                .any(|&tracked| tracked as usize == header_addr)
        });
        assert!(
            tracked_before,
            "new gc_malloc object should be tracked before sweep"
        );

        // Direct sweep is intentionally rootless for this regression. Keep
        // older test allocations marked so this assertion is about only the
        // object created above.
        MALLOC_STATE.with(|s| {
            for &tracked in s.borrow().objects.iter() {
                if tracked as usize != header_addr {
                    unsafe {
                        (*tracked).gc_flags |= GC_FLAG_MARKED;
                    }
                }
            }
        });
        crate::arena::arena_walk_objects(|arena_header| unsafe {
            (*(arena_header as *mut GcHeader)).gc_flags |= GC_FLAG_MARKED;
        });

        let freed = sweep();
        assert!(
            freed >= (GC_HEADER_SIZE + 64) as u64,
            "sweep should report at least the target malloc object as freed"
        );

        let tracked_after = MALLOC_STATE.with(|s| {
            s.borrow()
                .objects
                .iter()
                .any(|&tracked| tracked as usize == header_addr)
        });
        assert!(
            !tracked_after,
            "unmarked malloc object should be removed from MALLOC_STATE.objects"
        );

        clear_marks();
        clear_mark_seeds();
    }

    #[test]
    fn test_trace_array_marks_child() {
        clear_marks();
        clear_mark_seeds();

        let child = crate::string::js_string_from_bytes(b"child".as_ptr(), 5) as *mut u8;
        let child_header = unsafe { header_from_user_ptr(child) };
        unsafe {
            assert_eq!(
                (*child_header).gc_flags & GC_FLAG_MARKED,
                0,
                "child should start unmarked before array tracing"
            );
        }
        let parent = crate::array::js_array_alloc_with_length(1);
        crate::array::js_array_set_f64(
            parent,
            0,
            f64::from_bits(STRING_TAG | (child as u64 & POINTER_MASK)),
        );

        let valid_ptrs = build_valid_pointer_set();
        let parent_bits = POINTER_TAG | (parent as u64 & POINTER_MASK);
        assert!(
            try_mark_value(parent_bits, &valid_ptrs),
            "parent array should be marked as a root"
        );

        trace_marked_objects(&valid_ptrs);

        unsafe {
            assert_ne!(
                (*child_header).gc_flags & GC_FLAG_MARKED,
                0,
                "tracing the marked array should mark its child element"
            );
        }

        clear_marks();
        clear_mark_seeds();
    }

    #[test]
    fn test_layout_mask_pointer_free_array_scans_zero_slots() {
        clear_marks();
        clear_mark_seeds();

        let arr = crate::array::js_array_alloc_with_length(4);
        for i in 0..4 {
            crate::array::js_array_set_f64(arr, i, (i + 1) as f64);
        }

        let valid_ptrs = build_valid_pointer_set();
        let mut worklist = Vec::new();
        test_reset_trace_slot_reads();
        unsafe {
            trace_array(arr as *mut u8, &valid_ptrs, &mut worklist);
        }

        assert_eq!(test_layout_pointer_slot_count(arr as usize, 4), Some(0));
        assert_eq!(test_trace_slot_reads(), 0);

        clear_marks();
        clear_mark_seeds();
    }

    #[test]
    fn test_layout_mask_small_mixed_array_falls_back_to_full_scan() {
        clear_marks();
        clear_mark_seeds();

        let child = crate::string::js_string_from_bytes(b"array-child".as_ptr(), 11) as *mut u8;
        let child_header = unsafe { header_from_user_ptr(child) };
        let arr = crate::array::js_array_alloc_with_length(3);
        crate::array::js_array_set_f64(arr, 0, 1.0);
        crate::array::js_array_set_f64(
            arr,
            1,
            f64::from_bits(STRING_TAG | (child as u64 & POINTER_MASK)),
        );
        crate::array::js_array_set_f64(arr, 2, 3.0);

        assert_eq!(test_layout_pointer_slot_count(arr as usize, 3), None);

        let valid_ptrs = build_valid_pointer_set();
        let mut worklist = Vec::new();
        test_reset_trace_slot_reads();
        unsafe {
            trace_array(arr as *mut u8, &valid_ptrs, &mut worklist);
            assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
        }
        assert_eq!(test_trace_slot_reads(), 3);

        crate::array::js_array_set_f64(arr, 1, 2.0);
        assert_eq!(test_layout_pointer_slot_count(arr as usize, 3), None);

        clear_marks();
        clear_mark_seeds();
    }

    #[test]
    fn test_layout_mask_heap_conversion_keeps_sparse_words_zeroed() {
        clear_marks();
        clear_mark_seeds();

        let first_child =
            crate::string::js_string_from_bytes(b"first-child".as_ptr(), 11) as *mut u8;
        let later_child =
            crate::string::js_string_from_bytes(b"later-child".as_ptr(), 11) as *mut u8;
        let first_child_header = unsafe { header_from_user_ptr(first_child) };
        let later_child_header = unsafe { header_from_user_ptr(later_child) };
        let arr = crate::array::js_array_alloc_with_length(66);
        crate::array::js_array_set_f64(
            arr,
            0,
            f64::from_bits(STRING_TAG | (first_child as u64 & POINTER_MASK)),
        );
        crate::array::js_array_set_f64(arr, 64, 64.0);
        crate::array::js_array_set_f64(
            arr,
            65,
            f64::from_bits(STRING_TAG | (later_child as u64 & POINTER_MASK)),
        );

        assert_eq!(test_layout_pointer_slot_count(arr as usize, 66), Some(2));

        let valid_ptrs = build_valid_pointer_set();
        let mut worklist = Vec::new();
        test_reset_trace_slot_reads();
        unsafe {
            trace_array(arr as *mut u8, &valid_ptrs, &mut worklist);
            assert_ne!((*first_child_header).gc_flags & GC_FLAG_MARKED, 0);
            assert_ne!((*later_child_header).gc_flags & GC_FLAG_MARKED, 0);
        }
        assert_eq!(test_trace_slot_reads(), 2);

        clear_marks();
        clear_mark_seeds();
    }

    #[test]
    fn test_layout_mask_object_and_closure_slots() {
        clear_marks();
        clear_mark_seeds();

        let object_child =
            crate::string::js_string_from_bytes(b"object-child".as_ptr(), 12) as *mut u8;
        let object_child_header = unsafe { header_from_user_ptr(object_child) };
        let obj = crate::object::js_object_alloc(0, 3);
        crate::object::js_object_set_field(obj, 0, crate::value::JSValue::number(1.0));
        crate::object::js_object_set_field(
            obj,
            1,
            crate::value::JSValue::from_bits(STRING_TAG | (object_child as u64 & POINTER_MASK)),
        );
        crate::object::js_object_set_field(obj, 2, crate::value::JSValue::number(3.0));

        assert_eq!(test_layout_pointer_slot_count(obj as usize, 3), None);
        let valid_ptrs = build_valid_pointer_set();
        let mut worklist = Vec::new();
        test_reset_trace_slot_reads();
        unsafe {
            trace_object(obj as *mut u8, &valid_ptrs, &mut worklist);
            assert_ne!((*object_child_header).gc_flags & GC_FLAG_MARKED, 0);
        }
        assert_eq!(test_trace_slot_reads(), 3);

        let closure_child =
            crate::string::js_string_from_bytes(b"closure-child".as_ptr(), 13) as *mut u8;
        let closure_child_header = unsafe { header_from_user_ptr(closure_child) };
        let closure = crate::closure::js_closure_alloc(std::ptr::null(), 3);
        crate::closure::js_closure_set_capture_f64(closure, 0, 10.0);
        crate::closure::js_closure_set_capture_f64(
            closure,
            1,
            f64::from_bits(STRING_TAG | (closure_child as u64 & POINTER_MASK)),
        );
        crate::closure::js_closure_set_capture_f64(closure, 2, 30.0);

        assert_eq!(test_layout_pointer_slot_count(closure as usize, 3), None);
        let valid_ptrs = build_valid_pointer_set();
        let mut worklist = Vec::new();
        test_reset_trace_slot_reads();
        unsafe {
            trace_closure(closure as *mut u8, &valid_ptrs, &mut worklist);
            assert_ne!((*closure_child_header).gc_flags & GC_FLAG_MARKED, 0);
        }
        assert_eq!(test_trace_slot_reads(), 3);

        clear_marks();
        clear_mark_seeds();
    }

    #[test]
    fn test_layout_mask_overflow_fields_and_array_grow_transfer() {
        clear_marks();
        clear_mark_seeds();

        let child = crate::string::js_string_from_bytes(b"overflow-child".as_ptr(), 14) as *mut u8;
        let child_header = unsafe { header_from_user_ptr(child) };
        let obj = crate::object::js_object_alloc(0, 0);
        for i in 0..9 {
            let name = format!("k{i}");
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            let value = if i == 8 {
                f64::from_bits(STRING_TAG | (child as u64 & POINTER_MASK))
            } else {
                i as f64
            };
            crate::object::js_object_set_field_by_name(obj, key, value);
        }

        assert_eq!(test_layout_pointer_slot_count(obj as usize, 9), None);
        let valid_ptrs = build_valid_pointer_set();
        crate::object::scan_overflow_fields_roots(&mut |value| {
            try_mark_value(value.to_bits(), &valid_ptrs);
        });
        unsafe {
            assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
        }

        let arr = crate::array::js_array_alloc_with_length(1);
        crate::array::js_array_set_f64(
            arr,
            0,
            f64::from_bits(STRING_TAG | (child as u64 & POINTER_MASK)),
        );
        let grown = crate::array::js_array_grow(arr, 128);
        assert_eq!(test_layout_pointer_slot_count(grown as usize, 1), None);

        let moved = crate::array::js_array_alloc_with_length(1);
        unsafe {
            layout_transfer(grown as *mut u8, moved as *mut u8);
        }
        assert_eq!(test_layout_pointer_slot_count(moved as usize, 1), None);

        clear_marks();
        clear_mark_seeds();
    }

    #[test]
    fn test_trace_array_uses_pointer_layout_mask() {
        clear_marks();
        clear_mark_seeds();

        let numeric = crate::array::js_array_alloc_with_length(3);
        crate::array::js_array_set_f64(numeric, 0, 1.0);
        crate::array::js_array_set_f64(numeric, 1, 2.0);
        crate::array::js_array_set_f64(numeric, 2, 3.0);
        assert_eq!(test_layout_pointer_slot_count(numeric as usize, 3), Some(0));

        let valid_ptrs = build_valid_pointer_set();
        assert!(try_mark_value(
            POINTER_TAG | (numeric as u64 & POINTER_MASK),
            &valid_ptrs
        ));
        test_reset_trace_slot_reads();
        trace_marked_objects(&valid_ptrs);
        assert_eq!(test_trace_slot_reads(), 0);
        clear_marks();
        clear_mark_seeds();

        let child = crate::string::js_string_from_bytes(b"array-child".as_ptr(), 11) as *mut u8;
        let child_header = unsafe { header_from_user_ptr(child) };
        let mixed = crate::array::js_array_alloc_with_length(3);
        crate::array::js_array_set_f64(mixed, 0, 1.0);
        crate::array::js_array_set_f64(
            mixed,
            1,
            f64::from_bits(STRING_TAG | (child as u64 & POINTER_MASK)),
        );
        crate::array::js_array_set_f64(mixed, 2, 3.0);
        assert_eq!(test_layout_pointer_slot_count(mixed as usize, 3), None);

        let valid_ptrs = build_valid_pointer_set();
        assert!(try_mark_value(
            POINTER_TAG | (mixed as u64 & POINTER_MASK),
            &valid_ptrs
        ));
        test_reset_trace_slot_reads();
        trace_marked_objects(&valid_ptrs);
        assert_eq!(test_trace_slot_reads(), 3);
        unsafe {
            assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
        }

        clear_marks();
        clear_mark_seeds();
    }

    #[test]
    fn test_trace_object_uses_pointer_layout_mask() {
        clear_marks();
        clear_mark_seeds();

        let numeric = crate::object::js_object_alloc(0, 3);
        crate::object::js_object_set_field(numeric, 0, crate::value::JSValue::number(1.0));
        crate::object::js_object_set_field(numeric, 1, crate::value::JSValue::number(2.0));
        crate::object::js_object_set_field(numeric, 2, crate::value::JSValue::bool(false));
        assert_eq!(test_layout_pointer_slot_count(numeric as usize, 3), Some(0));

        let valid_ptrs = build_valid_pointer_set();
        assert!(try_mark_value(
            POINTER_TAG | (numeric as u64 & POINTER_MASK),
            &valid_ptrs
        ));
        test_reset_trace_slot_reads();
        trace_marked_objects(&valid_ptrs);
        assert_eq!(test_trace_slot_reads(), 0);
        clear_marks();
        clear_mark_seeds();

        let child = crate::string::js_string_from_bytes(b"object-child".as_ptr(), 12);
        let child_header = unsafe { header_from_user_ptr(child as *mut u8) };
        let mixed = crate::object::js_object_alloc(0, 3);
        crate::object::js_object_set_field(mixed, 0, crate::value::JSValue::number(1.0));
        crate::object::js_object_set_field(mixed, 1, crate::value::JSValue::string_ptr(child));
        crate::object::js_object_set_field(mixed, 2, crate::value::JSValue::number(3.0));
        assert_eq!(test_layout_pointer_slot_count(mixed as usize, 3), None);

        let valid_ptrs = build_valid_pointer_set();
        assert!(try_mark_value(
            POINTER_TAG | (mixed as u64 & POINTER_MASK),
            &valid_ptrs
        ));
        test_reset_trace_slot_reads();
        trace_marked_objects(&valid_ptrs);
        assert_eq!(test_trace_slot_reads(), 3);
        unsafe {
            assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
        }

        clear_marks();
        clear_mark_seeds();
    }

    extern "C" fn layout_mask_test_closure(_closure: *const crate::closure::ClosureHeader) -> f64 {
        0.0
    }

    #[test]
    fn test_trace_closure_uses_pointer_layout_mask() {
        clear_marks();
        clear_mark_seeds();

        let numeric = crate::closure::js_closure_alloc(layout_mask_test_closure as *const u8, 3);
        crate::closure::js_closure_set_capture_f64(numeric, 0, 1.0);
        crate::closure::js_closure_set_capture_f64(numeric, 1, 2.0);
        crate::closure::js_closure_set_capture_ptr(numeric, 2, 7);
        assert_eq!(test_layout_pointer_slot_count(numeric as usize, 3), Some(0));

        let valid_ptrs = build_valid_pointer_set();
        assert!(try_mark_value(
            POINTER_TAG | (numeric as u64 & POINTER_MASK),
            &valid_ptrs
        ));
        test_reset_trace_slot_reads();
        trace_marked_objects(&valid_ptrs);
        assert_eq!(test_trace_slot_reads(), 0);
        clear_marks();
        clear_mark_seeds();

        let child = crate::string::js_string_from_bytes(b"closure-child".as_ptr(), 13) as *mut u8;
        let child_header = unsafe { header_from_user_ptr(child) };
        let mixed = crate::closure::js_closure_alloc(layout_mask_test_closure as *const u8, 3);
        crate::closure::js_closure_set_capture_f64(mixed, 0, 1.0);
        crate::closure::js_closure_set_capture_f64(
            mixed,
            1,
            f64::from_bits(STRING_TAG | (child as u64 & POINTER_MASK)),
        );
        crate::closure::js_closure_set_capture_ptr(mixed, 2, 7);
        assert_eq!(test_layout_pointer_slot_count(mixed as usize, 3), None);

        let valid_ptrs = build_valid_pointer_set();
        assert!(try_mark_value(
            POINTER_TAG | (mixed as u64 & POINTER_MASK),
            &valid_ptrs
        ));
        test_reset_trace_slot_reads();
        trace_marked_objects(&valid_ptrs);
        assert_eq!(test_trace_slot_reads(), 3);
        unsafe {
            assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
        }

        clear_marks();
        clear_mark_seeds();
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
        unsafe {
            init_test_closure(ptr2);
        }

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

    fn reset_global_roots() {
        GLOBAL_ROOTS.with(|roots| roots.borrow_mut().clear());
    }

    struct ShadowAndGlobalRootResetGuard;

    impl Drop for ShadowAndGlobalRootResetGuard {
        fn drop(&mut self) {
            reset_shadow_stack();
            reset_global_roots();
        }
    }

    fn assert_panics_with(expected: &str, f: impl FnOnce()) {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        let Err(payload) = result else {
            panic!("expected panic containing {expected:?}");
        };
        let message = if let Some(s) = payload.downcast_ref::<&str>() {
            *s
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.as_str()
        } else {
            "<non-string panic>"
        };
        assert!(
            message.contains(expected),
            "panic message {message:?} did not contain {expected:?}"
        );
    }

    thread_local! {
        static LOCK_SAFE_RUNTIME_SCANNERS_REGISTERED: std::cell::Cell<bool> =
            const { std::cell::Cell::new(false) };
    }

    static LOCK_SAFE_RUNTIME_SCANNER_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn lock_safe_runtime_scanner_test_guard() -> std::sync::MutexGuard<'static, ()> {
        LOCK_SAFE_RUNTIME_SCANNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn ensure_lock_safe_runtime_scanners_registered() {
        LOCK_SAFE_RUNTIME_SCANNERS_REGISTERED.with(|registered| {
            if registered.get() {
                return;
            }
            gc_register_mutable_root_scanner(crate::tui::hooks::scan_hook_slot_roots_mut);
            gc_register_mutable_root_scanner(crate::tui::state::scan_state_slot_roots_mut);
            #[cfg(feature = "ohos-napi")]
            {
                gc_register_mutable_root_scanner(
                    crate::arkts_callbacks::arkts_callbacks_root_scanner_mut,
                );
                gc_register_mutable_root_scanner(
                    crate::media_playback::media_callbacks_root_scanner_mut,
                );
            }
            registered.set(true);
        });
    }

    struct ActiveShadowFrame(u64);

    impl ActiveShadowFrame {
        fn push_empty() -> Self {
            reset_shadow_stack();
            Self(js_shadow_frame_push(0))
        }
    }

    impl Drop for ActiveShadowFrame {
        fn drop(&mut self) {
            js_shadow_frame_pop(self.0);
        }
    }

    fn lock_safe_runtime_scanner_closure() -> (*mut u8, u64, f64) {
        let ptr = crate::closure::js_closure_alloc(test_no_capture_singleton_func as *const u8, 0)
            as *mut u8;
        let bits = POINTER_TAG | (ptr as u64 & POINTER_MASK);
        (ptr, bits, f64::from_bits(bits))
    }

    fn malloc_user_ptr_tracked(ptr: *mut u8) -> bool {
        let header = unsafe { header_from_user_ptr(ptr) };
        MALLOC_STATE.with(|s| s.borrow().objects.iter().any(|&tracked| tracked == header))
    }

    fn activate_malloc_registry_for_tests() {
        MALLOC_STATE.with(|s| {
            let mut s = s.borrow_mut();
            ensure_set_built(&mut s);
        });
    }

    fn deactivate_malloc_registry_for_tests() {
        MALLOC_STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.set.clear();
            s.registry_state = MallocRegistryState::Inactive;
        });
    }

    fn malloc_registry_active_for_tests() -> bool {
        MALLOC_STATE.with(|s| s.borrow().malloc_registry_available())
    }

    fn gc_collection_count() -> u64 {
        GC_STATS.with(|s| s.borrow().collection_count)
    }

    struct GcUnsafeZoneResetGuard;

    impl GcUnsafeZoneResetGuard {
        fn clear() -> Self {
            GC_UNSAFE_ZONES.store(0, std::sync::atomic::Ordering::Release);
            GC_UNSAFE_WARNED.store(false, std::sync::atomic::Ordering::Release);
            Self
        }

        fn enter() -> Self {
            let guard = Self::clear();
            GC_UNSAFE_ZONES.store(1, std::sync::atomic::Ordering::Release);
            guard
        }
    }

    impl Drop for GcUnsafeZoneResetGuard {
        fn drop(&mut self) {
            GC_UNSAFE_ZONES.store(0, std::sync::atomic::Ordering::Release);
            GC_UNSAFE_WARNED.store(false, std::sync::atomic::Ordering::Release);
        }
    }

    #[test]
    fn lock_safe_runtime_scanners_tui_state_defers_gc_check_trigger() {
        let _test_lock = lock_safe_runtime_scanner_test_guard();
        let _reset = ShadowAndGlobalRootResetGuard;
        ensure_lock_safe_runtime_scanners_registered();
        crate::tui::state::test_reset_state_slots();

        let (ptr, bits, value) = lock_safe_runtime_scanner_closure();
        let handle = crate::tui::state::js_perry_tui_state_alloc(value);
        GC_NEXT_MALLOC_TRIGGER.with(|trigger| {
            trigger.set(MALLOC_STATE.with(|s| s.borrow().objects.len()));
        });

        let before = gc_collection_count();
        let _shadow = ActiveShadowFrame::push_empty();
        crate::tui::state::test_with_state_slots_locked(|| {
            gc_check_trigger();
            assert_eq!(
                gc_collection_count(),
                before,
                "gc_check_trigger should defer while a state root lock is held"
            );
        });

        assert!(
            gc_collection_count() > before,
            "deferred trigger check should run after the state root lock is released"
        );
        assert!(
            malloc_user_ptr_tracked(ptr),
            "state slot root should survive the deferred collection"
        );
        assert_eq!(
            crate::tui::state::js_perry_tui_state_get(handle).to_bits(),
            bits
        );
        crate::tui::state::test_reset_state_slots();
    }

    #[test]
    fn lock_safe_runtime_scanners_tui_hooks_defers_direct_minor_gc() {
        let _test_lock = lock_safe_runtime_scanner_test_guard();
        let _reset = ShadowAndGlobalRootResetGuard;
        ensure_lock_safe_runtime_scanners_registered();

        let (ptr, bits, _value) = lock_safe_runtime_scanner_closure();
        crate::tui::hooks::test_seed_hook_slot_roots(bits);

        let before = gc_collection_count();
        let _shadow = ActiveShadowFrame::push_empty();
        crate::tui::hooks::test_with_hook_slots_locked(|| {
            let freed = gc_collect_minor();
            assert_eq!(freed, 0);
            assert_eq!(
                gc_collection_count(),
                before,
                "direct minor GC should defer while a hook root lock is held"
            );
        });

        assert!(
            gc_collection_count() > before,
            "deferred direct minor GC should run after the hook root lock is released"
        );
        assert!(
            malloc_user_ptr_tracked(ptr),
            "hook slot root should survive the deferred collection"
        );
        assert_eq!(
            crate::tui::hooks::test_hook_slot_roots(),
            (bits, bits, bits)
        );
    }

    #[test]
    fn lock_safe_runtime_scanners_tui_state_defers_manual_gc() {
        let _test_lock = lock_safe_runtime_scanner_test_guard();
        let _reset = ShadowAndGlobalRootResetGuard;
        ensure_lock_safe_runtime_scanners_registered();
        crate::tui::state::test_reset_state_slots();
        let _unsafe_zone = GcUnsafeZoneResetGuard::clear();

        let (ptr, bits, value) = lock_safe_runtime_scanner_closure();
        let handle = crate::tui::state::js_perry_tui_state_alloc(value);

        let before = gc_collection_count();
        let _shadow = ActiveShadowFrame::push_empty();
        crate::tui::state::test_with_state_slots_locked(|| {
            js_gc_collect();
            assert_eq!(
                gc_collection_count(),
                before,
                "manual GC should defer while a state root lock is held"
            );
        });

        assert!(
            gc_collection_count() > before,
            "deferred manual GC should run after the state root lock is released"
        );
        assert!(
            malloc_user_ptr_tracked(ptr),
            "state slot root should survive deferred manual GC"
        );
        assert_eq!(
            crate::tui::state::js_perry_tui_state_get(handle).to_bits(),
            bits
        );
        crate::tui::state::test_reset_state_slots();
    }

    #[test]
    fn lock_safe_runtime_scanners_manual_gc_unsafe_zone_stays_noop_after_unlock() {
        let _test_lock = lock_safe_runtime_scanner_test_guard();
        let _reset = ShadowAndGlobalRootResetGuard;
        ensure_lock_safe_runtime_scanners_registered();
        crate::tui::state::test_reset_state_slots();
        let _unsafe_zone = GcUnsafeZoneResetGuard::enter();

        let (_ptr, bits, value) = lock_safe_runtime_scanner_closure();
        let handle = crate::tui::state::js_perry_tui_state_alloc(value);

        let before = gc_collection_count();
        let _shadow = ActiveShadowFrame::push_empty();
        crate::tui::state::test_with_state_slots_locked(|| {
            js_gc_collect();
            assert_eq!(
                gc_collection_count(),
                before,
                "manual GC should no-op while unsafe zones are active"
            );
        });

        assert_eq!(
            gc_collection_count(),
            before,
            "manual GC skipped by an unsafe zone must not flush after the state root lock unlocks"
        );
        assert_eq!(
            crate::tui::state::js_perry_tui_state_get(handle).to_bits(),
            bits
        );
        crate::tui::state::test_reset_state_slots();
    }

    #[test]
    fn lock_safe_runtime_scanners_deferred_manual_gc_respects_unsafe_zone_at_flush() {
        let _test_lock = lock_safe_runtime_scanner_test_guard();
        let _reset = ShadowAndGlobalRootResetGuard;
        ensure_lock_safe_runtime_scanners_registered();
        crate::tui::state::test_reset_state_slots();
        let _unsafe_zone = GcUnsafeZoneResetGuard::clear();

        let (_ptr, bits, value) = lock_safe_runtime_scanner_closure();
        let handle = crate::tui::state::js_perry_tui_state_alloc(value);

        let before = gc_collection_count();
        let _shadow = ActiveShadowFrame::push_empty();
        crate::tui::state::test_with_state_slots_locked(|| {
            js_gc_collect();
            assert_eq!(
                gc_collection_count(),
                before,
                "manual GC should defer while a state root lock is held"
            );
            GC_UNSAFE_ZONES.store(1, std::sync::atomic::Ordering::Release);
        });

        assert_eq!(
            gc_collection_count(),
            before,
            "deferred manual GC should re-check unsafe zones before flushing after unlock"
        );
        assert_eq!(
            crate::tui::state::js_perry_tui_state_get(handle).to_bits(),
            bits
        );
        crate::tui::state::test_reset_state_slots();
    }

    #[test]
    fn lock_safe_runtime_scanners_tui_hooks_defers_direct_full_gc() {
        let _test_lock = lock_safe_runtime_scanner_test_guard();
        let _reset = ShadowAndGlobalRootResetGuard;
        ensure_lock_safe_runtime_scanners_registered();

        let (ptr, bits, _value) = lock_safe_runtime_scanner_closure();
        crate::tui::hooks::test_seed_hook_slot_roots(bits);

        let before = gc_collection_count();
        let _shadow = ActiveShadowFrame::push_empty();
        crate::tui::hooks::test_with_hook_slots_locked(|| {
            let freed = gc_collect_inner();
            assert_eq!(freed, 0);
            assert_eq!(
                gc_collection_count(),
                before,
                "direct full GC should defer while a hook root lock is held"
            );
        });

        assert!(
            gc_collection_count() > before,
            "deferred direct full GC should run after the hook root lock is released"
        );
        assert!(
            malloc_user_ptr_tracked(ptr),
            "hook slot root should survive deferred direct full GC"
        );
        assert_eq!(
            crate::tui::hooks::test_hook_slot_roots(),
            (bits, bits, bits)
        );
    }

    #[cfg(feature = "ohos-napi")]
    #[test]
    fn lock_safe_runtime_scanners_arkts_callbacks_defers_direct_minor_gc() {
        let _test_lock = lock_safe_runtime_scanner_test_guard();
        let _reset = ShadowAndGlobalRootResetGuard;
        ensure_lock_safe_runtime_scanners_registered();
        crate::arkts_callbacks::test_clear_arkts_callback_roots();

        let (ptr, bits, value) = lock_safe_runtime_scanner_closure();
        let callback_idx = 17;
        crate::arkts_callbacks::test_seed_arkts_callback_root(callback_idx, value);

        let before = gc_collection_count();
        let _shadow = ActiveShadowFrame::push_empty();
        crate::arkts_callbacks::test_with_arkts_callback_roots_locked(|| {
            let freed = gc_collect_minor();
            assert_eq!(freed, 0);
            assert_eq!(
                gc_collection_count(),
                before,
                "direct minor GC should defer while ArkTS callback roots are locked"
            );
        });

        assert!(
            gc_collection_count() > before,
            "deferred direct minor GC should run after ArkTS callback roots unlock"
        );
        assert!(
            malloc_user_ptr_tracked(ptr),
            "ArkTS callback root should survive deferred GC"
        );
        assert_eq!(
            crate::arkts_callbacks::test_arkts_callback_root(callback_idx),
            bits
        );
        crate::arkts_callbacks::test_clear_arkts_callback_roots();
    }

    #[cfg(feature = "ohos-napi")]
    #[test]
    fn lock_safe_runtime_scanners_media_callbacks_defers_direct_minor_gc() {
        let _test_lock = lock_safe_runtime_scanner_test_guard();
        let _reset = ShadowAndGlobalRootResetGuard;
        ensure_lock_safe_runtime_scanners_registered();

        let (ptr, bits, value) = lock_safe_runtime_scanner_closure();
        let handle = i64::MIN + 861;
        crate::media_playback::test_seed_media_callback_roots(handle, value, value);

        let before = gc_collection_count();
        let _shadow = ActiveShadowFrame::push_empty();
        crate::media_playback::test_with_media_callback_roots_locked(|| {
            let freed = gc_collect_minor();
            assert_eq!(freed, 0);
            assert_eq!(
                gc_collection_count(),
                before,
                "direct minor GC should defer while media callback roots are locked"
            );
        });

        assert!(
            gc_collection_count() > before,
            "deferred direct minor GC should run after media callback roots unlock"
        );
        assert!(
            malloc_user_ptr_tracked(ptr),
            "media callback root should survive deferred GC"
        );
        assert_eq!(
            crate::media_playback::test_media_callback_roots(handle),
            (bits, bits)
        );
    }

    #[test]
    fn test_conservative_stack_scan_auto_policy_skips_active_shadow_frame() {
        let _guard = ShadowAndGlobalRootResetGuard;
        reset_shadow_stack();
        assert_eq!(
            conservative_stack_scan_mode_from_value(None),
            ConservativeStackScanMode::Auto
        );
        assert_eq!(
            conservative_stack_scan_decision_for(ConservativeStackScanMode::Auto, false),
            ConservativeStackScanDecision::Scan
        );

        let h = js_shadow_frame_push(1);
        assert!(shadow_stack_has_active_frame());
        assert_eq!(
            conservative_stack_scan_decision_for(
                ConservativeStackScanMode::Auto,
                shadow_stack_has_active_frame()
            ),
            ConservativeStackScanDecision::SkipShadowStackActive
        );
        js_shadow_frame_pop(h);
    }

    #[test]
    fn test_conservative_stack_scan_env_off_disables_decision() {
        for value in ["0", "off", "false"] {
            let mode = conservative_stack_scan_mode_from_value(Some(value));
            assert_eq!(mode, ConservativeStackScanMode::Disabled);
            assert_eq!(
                conservative_stack_scan_decision_for(mode, false),
                ConservativeStackScanDecision::SkipDisabled
            );
            assert_eq!(
                conservative_stack_scan_decision_for(mode, true),
                ConservativeStackScanDecision::SkipDisabled
            );
        }
    }

    #[test]
    fn test_conservative_stack_scan_full_preserves_legacy_fallback_decision() {
        for value in ["1", "on", "true", "full", "debug"] {
            let mode = conservative_stack_scan_mode_from_value(Some(value));
            assert_eq!(mode, ConservativeStackScanMode::Full);
            assert_eq!(
                conservative_stack_scan_decision_for(mode, false),
                ConservativeStackScanDecision::Scan
            );
            assert_eq!(
                conservative_stack_scan_decision_for(mode, true),
                ConservativeStackScanDecision::Scan
            );
        }
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
        DIRTY_OLD_PAGES.with(|s| s.borrow_mut().clear());
        EXTERNAL_DIRTY_SLOT_PAGES.with(|s| s.borrow_mut().clear());
        REMEMBERED_SET.with(|s| s.borrow_mut().clear());
        crate::arena::old_arena_page_index_clear_for_tests();
    }

    static COPYING_NURSERY_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn copying_nursery_isolation_lock() -> std::sync::MutexGuard<'static, ()> {
        COPYING_NURSERY_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    struct CopyingNurseryTestGuard {
        frame: u64,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl CopyingNurseryTestGuard {
        fn new(slot_count: u32) -> Self {
            let lock = copying_nursery_isolation_lock();
            reset_shadow_stack();
            reset_global_roots();
            reset_remembered_set();
            js_gc_write_barriers_emitted(1);
            let frame = js_shadow_frame_push(slot_count);
            Self { frame, _lock: lock }
        }
    }

    impl Drop for CopyingNurseryTestGuard {
        fn drop(&mut self) {
            js_shadow_frame_pop(self.frame);
            reset_shadow_stack();
            reset_global_roots();
            reset_remembered_set();
            js_gc_write_barriers_emitted(0);
        }
    }

    struct GcTriggerThresholdTestGuard {
        next_arena_trigger: usize,
        next_malloc_trigger: usize,
        malloc_step: usize,
    }

    impl GcTriggerThresholdTestGuard {
        fn suppress_automatic_triggers() -> Self {
            let next_arena_trigger = GC_NEXT_TRIGGER_BYTES.with(|trigger| {
                let previous = trigger.get();
                trigger.set(usize::MAX);
                previous
            });
            let next_malloc_trigger = GC_NEXT_MALLOC_TRIGGER.with(|trigger| {
                let previous = trigger.get();
                trigger.set(usize::MAX);
                previous
            });
            let malloc_step = GC_MALLOC_COUNT_STEP.with(|step| step.get());
            Self {
                next_arena_trigger,
                next_malloc_trigger,
                malloc_step,
            }
        }

        fn make_malloc_sweep_due(&self) {
            let current = malloc_object_count();
            GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(current));
        }

        fn make_arena_trigger_due(&self) {
            GC_NEXT_TRIGGER_BYTES.with(|trigger| trigger.set(0));
        }
    }

    impl Drop for GcTriggerThresholdTestGuard {
        fn drop(&mut self) {
            GC_NEXT_TRIGGER_BYTES.with(|trigger| trigger.set(self.next_arena_trigger));
            GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(self.next_malloc_trigger));
            GC_MALLOC_COUNT_STEP.with(|step| step.set(self.malloc_step));
        }
    }

    fn collect_minor_trace(trigger_kind: GcTriggerKind) -> GcCycleTrace {
        gc_collect_minor_with_trigger(GcTriggerSnapshot {
            kind: trigger_kind,
            steps_before: Some(GcStepSnapshot::current()),
        })
        .trace
        .expect("test requested GC trace capture")
    }

    fn assert_copied_minor_trace(
        trace: &GcCycleTrace,
        eligible: bool,
        fallback_reason: CopiedMinorFallbackReason,
        malloc_sweep_due: bool,
    ) {
        assert_eq!(trace.copying_nursery.eligible, eligible);
        assert_eq!(trace.copying_nursery.fallback_reason, fallback_reason);
        assert_eq!(trace.copying_nursery.malloc_sweep_due, malloc_sweep_due);
    }

    static ENV_VAR_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &'static str) -> Self {
            let lock = ENV_VAR_TEST_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self {
                key,
                previous,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(previous) = self.previous.as_ref() {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    static GENERATED_BARRIER_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct GeneratedWriteBarrierTestGuard {
        previous: usize,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl GeneratedWriteBarrierTestGuard {
        fn active() -> Self {
            let lock = GENERATED_BARRIER_TEST_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let previous = GENERATED_WRITE_BARRIERS_EMITTED.swap(0, Ordering::AcqRel);
            js_gc_write_barriers_emitted(1);
            Self {
                previous,
                _lock: lock,
            }
        }

        fn inactive() -> Self {
            let lock = GENERATED_BARRIER_TEST_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let previous = GENERATED_WRITE_BARRIERS_EMITTED.swap(0, Ordering::AcqRel);
            Self {
                previous,
                _lock: lock,
            }
        }
    }

    impl Drop for GeneratedWriteBarrierTestGuard {
        fn drop(&mut self) {
            GENERATED_WRITE_BARRIERS_EMITTED.store(self.previous, Ordering::Release);
        }
    }

    fn noop_copy_only_root_scanner(_mark: &mut dyn FnMut(f64)) {}

    struct TemporaryCopyOnlyRootScanner {
        previous_len: usize,
    }

    impl TemporaryCopyOnlyRootScanner {
        fn new() -> Self {
            let previous_len = ROOT_SCANNERS.with(|scanners| {
                let mut scanners = scanners.borrow_mut();
                let previous_len = scanners.len();
                scanners.push(noop_copy_only_root_scanner);
                previous_len
            });
            Self { previous_len }
        }
    }

    impl Drop for TemporaryCopyOnlyRootScanner {
        fn drop(&mut self) {
            ROOT_SCANNERS.with(|scanners| {
                scanners.borrow_mut().truncate(self.previous_len);
            });
        }
    }

    fn young_leaf() -> usize {
        crate::arena::arena_alloc_gc(32, 8, GC_TYPE_STRING) as usize
    }

    fn ptr_bits(addr: usize) -> u64 {
        POINTER_TAG | (addr as u64 & POINTER_MASK)
    }

    fn arena_block_index_for_user(user: usize) -> Option<usize> {
        let mut found = None;
        crate::arena::arena_walk_objects_with_block_index(|header_ptr, block_idx| {
            let current_user = unsafe { (header_ptr as *mut u8).add(GC_HEADER_SIZE) as usize };
            if current_user == user {
                found = Some(block_idx);
            }
        });
        found
    }

    extern "C" fn test_no_capture_singleton_func(
        _closure: *const crate::closure::ClosureHeader,
    ) -> f64 {
        0.0
    }

    extern "C" fn test_captured_singleton_func(
        _closure: *const crate::closure::ClosureHeader,
    ) -> f64 {
        0.0
    }

    unsafe fn init_test_closure(ptr: *mut u8) {
        let closure = ptr as *mut crate::closure::ClosureHeader;
        (*closure).func_ptr = std::ptr::null();
        (*closure).capture_count = 0;
        (*closure).type_tag = crate::closure::CLOSURE_MAGIC;
    }

    unsafe fn init_test_closure_with_one_capture(ptr: *mut u8, capture_bits: u64) -> *mut u64 {
        let closure = ptr as *mut crate::closure::ClosureHeader;
        (*closure).func_ptr = std::ptr::null();
        (*closure).capture_count = 1;
        (*closure).type_tag = crate::closure::CLOSURE_MAGIC;
        let capture_slot =
            ptr.add(std::mem::size_of::<crate::closure::ClosureHeader>()) as *mut u64;
        *capture_slot = capture_bits;
        layout_note_slot(ptr as usize, 0, capture_bits);
        capture_slot
    }

    #[inline(never)]
    fn allocate_dead_malloc_churn_headers(per_type: usize) -> Vec<usize> {
        let mut headers = Vec::with_capacity(per_type * 3);
        for _ in 0..per_type {
            let ptr = gc_malloc(32, GC_TYPE_STRING);
            unsafe {
                std::ptr::write_bytes(ptr, 0xA5, 32);
                headers.push(header_from_user_ptr(ptr) as usize);
            }
        }
        for _ in 0..per_type {
            let ptr = gc_malloc(
                std::mem::size_of::<crate::closure::ClosureHeader>(),
                GC_TYPE_CLOSURE,
            );
            unsafe {
                init_test_closure(ptr);
                headers.push(header_from_user_ptr(ptr) as usize);
            }
        }
        for _ in 0..per_type {
            let ptr = gc_malloc(
                std::mem::size_of::<crate::promise::Promise>(),
                GC_TYPE_PROMISE,
            ) as *mut crate::promise::Promise;
            unsafe {
                std::ptr::write(
                    ptr,
                    crate::promise::Promise {
                        state: crate::promise::PromiseState::Pending,
                        value: 0.0,
                        reason: 0.0,
                        on_fulfilled: std::ptr::null(),
                        on_rejected: std::ptr::null(),
                        next: std::ptr::null_mut(),
                        async_id: 0,
                        trigger_async_id: 0,
                    },
                );
                headers.push(header_from_user_ptr(ptr as *const u8) as usize);
            }
        }
        headers
    }

    fn tracked_malloc_headers_matching(headers: &[usize]) -> usize {
        MALLOC_STATE.with(|state| {
            let state = state.borrow();
            headers
                .iter()
                .filter(|&&addr| state.objects.iter().any(|&header| header as usize == addr))
                .count()
        })
    }

    unsafe fn alloc_old_test_object(
        field_count: u32,
    ) -> (*mut crate::object::ObjectHeader, *mut u64) {
        let payload = std::mem::size_of::<crate::object::ObjectHeader>() + field_count as usize * 8;
        let obj = crate::arena::arena_alloc_gc_old(payload, 8, GC_TYPE_OBJECT)
            as *mut crate::object::ObjectHeader;
        (*obj).object_type = 1;
        (*obj).class_id = 0;
        (*obj).parent_class_id = 0;
        (*obj).field_count = field_count;
        (*obj).keys_array = std::ptr::null_mut();
        let fields =
            (obj as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>()) as *mut u64;
        for i in 0..field_count as usize {
            *fields.add(i) = 0;
        }
        (obj, fields)
    }

    unsafe fn alloc_nursery_test_object(
        field_count: u32,
    ) -> (*mut crate::object::ObjectHeader, *mut u64) {
        let payload = std::mem::size_of::<crate::object::ObjectHeader>() + field_count as usize * 8;
        let obj = crate::arena::arena_alloc_gc(payload, 8, GC_TYPE_OBJECT)
            as *mut crate::object::ObjectHeader;
        (*obj).object_type = 1;
        (*obj).class_id = 0;
        (*obj).parent_class_id = 0;
        (*obj).field_count = field_count;
        (*obj).keys_array = std::ptr::null_mut();
        let fields =
            (obj as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>()) as *mut u64;
        for i in 0..field_count as usize {
            *fields.add(i) = 0;
        }
        (obj, fields)
    }

    unsafe fn alloc_old_test_array(length: u32) -> (*mut crate::array::ArrayHeader, *mut u64) {
        let payload = std::mem::size_of::<crate::array::ArrayHeader>() + length as usize * 8;
        let arr = crate::arena::arena_alloc_gc_old(payload, 8, GC_TYPE_ARRAY)
            as *mut crate::array::ArrayHeader;
        (*arr).length = length;
        (*arr).capacity = length;
        let elements =
            (arr as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64;
        for i in 0..length as usize {
            *elements.add(i) = 0;
        }
        (arr, elements)
    }

    #[test]
    fn test_copied_minor_eligibility_falls_back_for_barriers_inactive() {
        let _guard = CopyingNurseryTestGuard::new(0);
        let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _barrier_guard = GeneratedWriteBarrierTestGuard::inactive();

        let trace = collect_minor_trace(GcTriggerKind::Direct);

        assert_copied_minor_trace(
            &trace,
            false,
            CopiedMinorFallbackReason::BarriersInactive,
            false,
        );
    }

    #[test]
    fn test_copied_minor_eligibility_falls_back_for_conservative_stack_scan() {
        let _isolation = copying_nursery_isolation_lock();
        let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _barrier_guard = GeneratedWriteBarrierTestGuard::active();
        reset_shadow_stack();
        reset_global_roots();
        reset_remembered_set();

        let trace = collect_minor_trace(GcTriggerKind::Direct);

        assert_copied_minor_trace(
            &trace,
            false,
            CopiedMinorFallbackReason::ConservativeStack,
            false,
        );
    }

    #[test]
    fn test_copied_minor_eligibility_falls_back_for_copy_only_roots() {
        let _guard = CopyingNurseryTestGuard::new(0);
        let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _copy_only_root_guard = TemporaryCopyOnlyRootScanner::new();

        let trace = collect_minor_trace(GcTriggerKind::Direct);

        assert_copied_minor_trace(
            &trace,
            false,
            CopiedMinorFallbackReason::CopyOnlyRoots,
            false,
        );
    }

    #[test]
    fn test_copying_minor_rewrites_shadow_and_global_roots() {
        let _guard = CopyingNurseryTestGuard::new(1);
        let shadow_child = young_leaf();
        let global_child = young_leaf();
        let mut global_slot = global_child as u64;
        js_shadow_slot_set(0, ptr_bits(shadow_child));
        js_gc_register_global_root(&mut global_slot as *mut u64 as i64);

        let _ = gc_collect_minor();
        let shadow_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
        let global_after = global_slot as usize;

        assert_ne!(shadow_after, shadow_child);
        assert_ne!(global_after, global_child);
        assert!(crate::arena::pointer_in_nursery(shadow_after));
        assert!(crate::arena::pointer_in_nursery(global_after));
        assert_eq!(
            crate::arena::classify_heap_space(shadow_after),
            crate::arena::active_survivor_space()
        );
    }

    #[test]
    fn test_copying_minor_ignores_cleared_dead_shadow_slot_but_preserves_live_slot() {
        let _guard = CopyingNurseryTestGuard::new(2);
        let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let dead = young_leaf();
        let live = young_leaf();
        js_shadow_slot_set(0, ptr_bits(dead));
        js_shadow_slot_set(0, 0);
        js_shadow_slot_set(1, ptr_bits(live));

        let trace = collect_minor_trace(GcTriggerKind::Direct);
        let live_after = (js_shadow_slot_get(1) & POINTER_MASK) as usize;

        assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
        assert_eq!(js_shadow_slot_get(0), 0);
        assert_ne!(live_after, live);
        assert!(crate::arena::pointer_in_nursery(live_after));
        assert_eq!(trace.copying_nursery.copied_objects, 1);
        assert_eq!(trace.copying_nursery.promoted_objects, 0);
        assert_eq!(trace.shadow_roots.slots_scanned, 2);
        assert_eq!(trace.shadow_roots.nonzero_slots, 1);
        assert_eq!(trace.shadow_roots.pointer_roots, 1);
        assert_eq!(trace.shadow_roots.rewritten_slots, 1);
    }

    #[test]
    fn test_copied_minor_verify_evacuation_env_remains_eligible() {
        let _env_guard = EnvVarGuard::set("PERRY_GC_VERIFY_EVACUATION", "1");
        let _guard = CopyingNurseryTestGuard::new(1);
        let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let child = young_leaf();
        js_shadow_slot_set(0, ptr_bits(child));

        let trace = collect_minor_trace(GcTriggerKind::Direct);
        let after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;

        assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
        assert!(
            trace.phase_us.contains_key("evacuation_verify"),
            "forced copied-minor verification should run before from-space reset"
        );
        assert_ne!(after, child);
        assert!(crate::arena::pointer_in_nursery(after));
    }

    #[test]
    fn test_copying_minor_rewrites_dirty_old_slot_and_keeps_sticky_page() {
        let _guard = CopyingNurseryTestGuard::new(0);
        let child = young_leaf();
        let (old_arr, elements) = unsafe { alloc_old_test_array(1) };
        unsafe {
            *elements = ptr_bits(child);
        }
        js_write_barrier_slot(ptr_bits(old_arr as usize), elements as u64, ptr_bits(child));
        assert!(remembered_set_size() > 0);

        let _ = gc_collect_minor();
        let rewritten = unsafe { (*elements & POINTER_MASK) as usize };

        assert_ne!(rewritten, child);
        assert!(crate::arena::pointer_in_nursery(rewritten));
        assert!(
            remembered_set_size() > 0,
            "old-to-survivor edge must stay dirty for the next minor"
        );
    }

    #[test]
    fn test_copying_minor_copies_transitive_young_graph() {
        let _guard = CopyingNurseryTestGuard::new(1);
        let arr = crate::array::js_array_alloc(1);
        let child = young_leaf();
        unsafe {
            (*arr).length = 1;
            let elements =
                (arr as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64;
            *elements = ptr_bits(child);
            layout_note_slot(arr as usize, 0, *elements);
        }
        js_shadow_slot_set(0, ptr_bits(arr as usize));

        let _ = gc_collect_minor();
        let arr_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
        let child_after = unsafe {
            let elements = (arr_after as *mut u8)
                .add(std::mem::size_of::<crate::array::ArrayHeader>())
                as *mut u64;
            (*elements & POINTER_MASK) as usize
        };

        assert_ne!(arr_after, arr as usize);
        assert_ne!(child_after, child);
        assert!(crate::arena::pointer_in_nursery(arr_after));
        assert!(crate::arena::pointer_in_nursery(child_after));
    }

    #[test]
    fn test_copying_minor_moves_layout_masked_transitive_object() {
        let _guard = CopyingNurseryTestGuard::new(1);
        let arr = crate::array::js_array_alloc(1);
        let (child, _child_fields) = unsafe { alloc_nursery_test_object(0) };
        unsafe {
            (*arr).length = 1;
            let elements =
                (arr as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64;
            *elements = ptr_bits(child as usize);
            layout_note_slot(arr as usize, 0, *elements);
        }
        js_shadow_slot_set(0, ptr_bits(arr as usize));

        let trace = collect_minor_trace(GcTriggerKind::Direct);
        let arr_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
        let child_after = unsafe {
            let elements = (arr_after as *mut u8)
                .add(std::mem::size_of::<crate::array::ArrayHeader>())
                as *mut u64;
            (*elements & POINTER_MASK) as usize
        };

        assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
        assert_ne!(arr_after, arr as usize);
        assert_ne!(child_after, child as usize);
        assert!(crate::arena::pointer_in_nursery(arr_after));
        assert!(crate::arena::pointer_in_nursery(child_after));
        assert!(
            trace.copying_nursery.copied_objects >= 2,
            "root array and transitive object should both move"
        );
    }

    #[test]
    fn test_copying_minor_rewrites_singleton_closure_caches() {
        struct SingletonClosureCacheGuard;

        impl Drop for SingletonClosureCacheGuard {
            fn drop(&mut self) {
                crate::closure::test_clear_singleton_closure_caches();
            }
        }

        let _guard = CopyingNurseryTestGuard::new(1);
        let _cache_guard = SingletonClosureCacheGuard;
        crate::closure::test_clear_singleton_closure_caches();
        gc_register_mutable_root_scanner(crate::closure::scan_singleton_closure_roots_mut);

        let no_capture_func = test_no_capture_singleton_func as *const u8;
        let no_capture = crate::closure::js_closure_alloc_singleton(no_capture_func);
        assert_eq!(
            crate::closure::test_singleton_closure_cache_entry(no_capture_func),
            Some(no_capture)
        );

        let captured_value = young_leaf();
        let capture_bits = ptr_bits(captured_value);
        js_shadow_slot_set(0, capture_bits);

        let captured_func = test_captured_singleton_func as *const u8;
        let captures = [capture_bits];
        let captured = crate::closure::js_closure_alloc_with_captures_singleton(
            captured_func,
            1,
            captures.as_ptr(),
        );
        assert_eq!(
            crate::closure::js_closure_alloc_with_captures_singleton(
                captured_func,
                1,
                captures.as_ptr(),
            ),
            captured,
            "captured singleton cache should hit before GC"
        );

        let before_entries =
            crate::closure::test_captured_singleton_closure_cache_entries(captured_func);
        assert_eq!(before_entries.len(), 1);
        assert_eq!(before_entries[0].0, vec![capture_bits]);
        assert_eq!(before_entries[0].1, captured);

        let capture_slot = unsafe {
            (captured as *mut u8).add(std::mem::size_of::<crate::closure::ClosureHeader>())
                as *mut u64
        };
        assert_eq!(unsafe { *capture_slot }, capture_bits);

        activate_malloc_registry_for_tests();
        js_shadow_slot_set(0, 0);
        let _ = gc_collect_minor();

        assert_eq!(
            crate::closure::js_closure_alloc_singleton(no_capture_func),
            no_capture,
            "no-capture singleton should remain a cache hit across copied-minor"
        );

        let capture_after_bits = unsafe { *capture_slot };
        let capture_after = (capture_after_bits & POINTER_MASK) as usize;
        assert_ne!(
            capture_after, captured_value,
            "captured young value should move out of eden"
        );
        assert_eq!(
            crate::arena::classify_heap_space(capture_after),
            crate::arena::active_survivor_space()
        );

        let after_entries =
            crate::closure::test_captured_singleton_closure_cache_entries(captured_func);
        assert_eq!(after_entries.len(), 1);
        assert_eq!(after_entries[0].1, captured);
        assert_eq!(
            after_entries[0].0,
            vec![capture_after_bits],
            "captured-cache key should be rewritten to the moved capture"
        );

        let rewritten_captures = [capture_after_bits];
        assert_eq!(
            crate::closure::js_closure_alloc_with_captures_singleton(
                captured_func,
                1,
                rewritten_captures.as_ptr(),
            ),
            captured,
            "future cache lookups should hit with the rewritten capture key"
        );
    }

    #[test]
    fn test_copying_minor_rewrites_overflow_owner_metadata_key() {
        struct OverflowFieldsRootGuard;

        impl Drop for OverflowFieldsRootGuard {
            fn drop(&mut self) {
                crate::object::test_clear_overflow_fields_root();
            }
        }

        let _guard = CopyingNurseryTestGuard::new(1);
        let _overflow_guard = OverflowFieldsRootGuard;
        crate::object::test_clear_overflow_fields_root();
        gc_register_mutable_root_scanner(overflow_fields_mutable_root_scanner);

        let owner = crate::object::js_object_alloc(0, 0) as usize;
        let overflow_value = young_leaf();
        crate::object::test_seed_overflow_fields_root(owner, ptr_bits(overflow_value));
        js_shadow_slot_set(0, ptr_bits(owner));

        let _ = gc_collect_minor();
        let owner_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
        let (mapped_owner, mapped_value_bits) = crate::object::test_overflow_fields_root();
        let mapped_value = (mapped_value_bits & POINTER_MASK) as usize;

        assert_ne!(owner_after, owner);
        assert_eq!(mapped_owner, owner_after);
        assert_ne!(mapped_value, overflow_value);
        assert!(crate::arena::pointer_in_nursery(owner_after));
        assert!(crate::arena::pointer_in_nursery(mapped_value));
    }

    #[test]
    fn test_copying_minor_promotes_survivor_on_second_survival() {
        let _guard = CopyingNurseryTestGuard::new(1);
        let child = young_leaf();
        js_shadow_slot_set(0, ptr_bits(child));

        let _ = gc_collect_minor();
        let survivor = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
        assert!(crate::arena::pointer_in_nursery(survivor));

        let _ = gc_collect_minor();
        let promoted = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
        assert_ne!(promoted, survivor);
        assert!(crate::arena::pointer_in_old_gen(promoted));
    }

    #[test]
    fn test_copying_minor_sticky_old_to_survivor_edge_promotes_next_cycle() {
        let _guard = CopyingNurseryTestGuard::new(0);
        let child = young_leaf();
        let (old_arr, elements) = unsafe { alloc_old_test_array(1) };
        unsafe {
            *elements = ptr_bits(child);
        }
        js_write_barrier_slot(ptr_bits(old_arr as usize), elements as u64, ptr_bits(child));

        let _ = gc_collect_minor();
        let survivor = unsafe { (*elements & POINTER_MASK) as usize };
        assert!(crate::arena::pointer_in_nursery(survivor));
        assert!(remembered_set_size() > 0);

        let _ = gc_collect_minor();
        let promoted = unsafe { (*elements & POINTER_MASK) as usize };
        assert!(crate::arena::pointer_in_old_gen(promoted));
    }

    #[test]
    fn test_copying_minor_resets_eden_wholesale() {
        let _guard = CopyingNurseryTestGuard::new(1);
        for _ in 0..128 {
            let _ = young_leaf();
        }
        let live = young_leaf();
        js_shadow_slot_set(0, ptr_bits(live));

        let _ = gc_collect_minor();
        let snapshot = crate::arena::arena_telemetry_snapshot();
        let live_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;

        assert_eq!(snapshot.arena.in_use_bytes, 0);
        assert!(crate::arena::pointer_in_nursery(live_after));
    }

    #[test]
    fn test_copying_minor_sweeps_malloc_when_due_on_arena_trigger() {
        let _guard = CopyingNurseryTestGuard::new(2);
        let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        assert!(copied_minor_malloc_sweep_due(GcTriggerKind::MallocCount));
        let live_young = young_leaf();
        js_shadow_slot_set(0, ptr_bits(live_young));
        let live_malloc = gc_malloc(
            std::mem::size_of::<crate::closure::ClosureHeader>(),
            GC_TYPE_CLOSURE,
        );
        unsafe {
            init_test_closure(live_malloc);
        }
        js_shadow_slot_set(1, ptr_bits(live_malloc as usize));
        activate_malloc_registry_for_tests();

        let churn_headers = allocate_dead_malloc_churn_headers(32);
        assert_eq!(
            tracked_malloc_headers_matching(&churn_headers),
            churn_headers.len(),
            "malloc churn should be tracked before the collection"
        );
        let tracked_before = malloc_object_count();
        trigger_guard.make_malloc_sweep_due();
        assert!(copied_minor_malloc_sweep_due(GcTriggerKind::ArenaBytes));

        let outcome = gc_collect_minor_with_trigger(GcTriggerSnapshot {
            kind: GcTriggerKind::ArenaBytes,
            steps_before: Some(GcStepSnapshot::current()),
        });
        let trace = outcome.trace.expect("test requested GC trace capture");

        assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, true);
        assert_eq!(
            tracked_malloc_headers_matching(&churn_headers),
            0,
            "copied-minor GC must sweep dead malloc churn when malloc pressure is due"
        );
        assert!(
            malloc_user_ptr_tracked(live_malloc),
            "live malloc root should survive copied-minor malloc sweep"
        );
        assert!(
            malloc_object_count() < tracked_before,
            "malloc sweep should reduce the tracked malloc object count"
        );
        assert!(
            outcome.freed_bytes > 0,
            "copied-minor path should report malloc reclaim"
        );
    }

    #[test]
    fn test_gc_check_trigger_copied_minor_malloc_sweep_rebaselines_trigger() {
        let _guard = CopyingNurseryTestGuard::new(1);
        let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let live_malloc = gc_malloc(
            std::mem::size_of::<crate::closure::ClosureHeader>(),
            GC_TYPE_CLOSURE,
        );
        unsafe {
            init_test_closure(live_malloc);
        }
        js_shadow_slot_set(0, ptr_bits(live_malloc as usize));
        activate_malloc_registry_for_tests();

        let churn_headers = allocate_dead_malloc_churn_headers(48);
        assert_eq!(
            tracked_malloc_headers_matching(&churn_headers),
            churn_headers.len(),
            "malloc churn should be tracked before gc_check_trigger"
        );
        let tracked_before = malloc_object_count();
        trigger_guard.make_malloc_sweep_due();
        let collections_before = gc_collection_count();

        gc_check_trigger();

        assert!(
            gc_collection_count() > collections_before,
            "gc_check_trigger should collect when malloc pressure is due"
        );
        assert_eq!(
            tracked_malloc_headers_matching(&churn_headers),
            0,
            "copied-minor collection should reclaim dead malloc churn"
        );
        assert!(
            malloc_user_ptr_tracked(live_malloc),
            "live malloc root should survive gc_check_trigger collection"
        );
        let survivors_after = malloc_object_count();
        assert!(
            survivors_after < tracked_before,
            "malloc sweep should reduce MALLOC_STATE.objects"
        );
        let malloc_step_after = GC_MALLOC_COUNT_STEP.with(|step| step.get());
        let next_malloc_trigger = GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.get());
        assert_eq!(
            next_malloc_trigger,
            survivors_after + malloc_step_after,
            "gc_check_trigger should rebaseline the next malloc trigger to survivors + step"
        );
    }

    #[test]
    fn test_gc_check_trigger_copied_minor_without_malloc_sweep_preserves_malloc_trigger() {
        let _guard = CopyingNurseryTestGuard::new(1);
        let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        deactivate_malloc_registry_for_tests();

        let live_young = young_leaf();
        js_shadow_slot_set(0, ptr_bits(live_young));
        let churn_headers = allocate_dead_malloc_churn_headers(48);
        let tracked_before = tracked_malloc_headers_matching(&churn_headers);
        assert_eq!(
            tracked_before,
            churn_headers.len(),
            "malloc churn should be tracked before gc_check_trigger"
        );

        let malloc_count_before = malloc_object_count();
        let next_malloc_trigger = malloc_count_before + 1;
        GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(next_malloc_trigger));
        trigger_guard.make_arena_trigger_due();
        assert!(
            !copied_minor_malloc_sweep_due(GcTriggerKind::ArenaBytes),
            "arena-triggered copied-minor should not sweep malloc while below malloc pressure"
        );

        let collections_before = gc_collection_count();
        gc_check_trigger();

        assert!(
            gc_collection_count() > collections_before,
            "gc_check_trigger should collect when arena pressure is due"
        );
        assert_eq!(
            tracked_malloc_headers_matching(&churn_headers),
            tracked_before,
            "malloc sweep was not due, so dead churn should remain tracked"
        );
        assert_eq!(
            malloc_object_count(),
            malloc_count_before,
            "copied-minor collection should not sweep malloc while below malloc pressure"
        );
        assert_eq!(
            GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.get()),
            next_malloc_trigger,
            "arena-triggered copied-minor without malloc sweep must preserve the existing malloc trigger"
        );
    }

    #[test]
    fn test_copied_minor_malloc_scaling_no_roots_skips_registry_walk() {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        deactivate_malloc_registry_for_tests();

        let churn_headers = allocate_dead_malloc_churn_headers(512);
        let tracked_before = tracked_malloc_headers_matching(&churn_headers);
        assert_eq!(tracked_before, churn_headers.len());
        let live_young = young_leaf();
        js_shadow_slot_set(0, ptr_bits(live_young));

        let outcome = gc_collect_minor_with_trigger(GcTriggerSnapshot {
            kind: GcTriggerKind::Direct,
            steps_before: Some(GcStepSnapshot::current()),
        });
        let trace = outcome.trace.expect("test requested GC trace capture");

        assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
        assert_eq!(
            trace.copying_nursery.malloc_validation_lookups, 0,
            "copied-minor should not probe malloc entries when no roots mention malloc"
        );
        assert_eq!(
            trace.copying_nursery.malloc_registry_rebuilds, 0,
            "copied-minor must not rebuild the malloc registry"
        );
        assert!(
            !malloc_registry_active_for_tests(),
            "copied-minor should leave an inactive malloc registry inactive"
        );
        assert_eq!(
            tracked_malloc_headers_matching(&churn_headers),
            tracked_before,
            "malloc sweep was not due, so dead churn should remain tracked without being walked"
        );
    }

    #[test]
    fn test_copied_minor_malloc_scaling_live_root_with_active_registry() {
        let _guard = CopyingNurseryTestGuard::new(1);
        let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let live_child = young_leaf();
        let live_malloc = gc_malloc(
            std::mem::size_of::<crate::closure::ClosureHeader>() + std::mem::size_of::<u64>(),
            GC_TYPE_CLOSURE,
        );
        let capture_slot =
            unsafe { init_test_closure_with_one_capture(live_malloc, ptr_bits(live_child)) };
        js_shadow_slot_set(0, ptr_bits(live_malloc as usize));
        activate_malloc_registry_for_tests();
        assert!(malloc_registry_active_for_tests());

        let churn_headers = allocate_dead_malloc_churn_headers(128);
        trigger_guard.make_malloc_sweep_due();
        let outcome = gc_collect_minor_with_trigger(GcTriggerSnapshot {
            kind: GcTriggerKind::ArenaBytes,
            steps_before: Some(GcStepSnapshot::current()),
        });
        let trace = outcome.trace.expect("test requested GC trace capture");

        assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, true);
        assert!(
            trace.copying_nursery.malloc_validation_lookups > 0,
            "active registry should validate the live malloc root"
        );
        assert!(
            trace.copying_nursery.malloc_validation_lookups < churn_headers.len(),
            "malloc validation should scale with reachable candidates, not dead churn"
        );
        assert_eq!(
            trace.copying_nursery.malloc_registry_rebuilds, 0,
            "copied-minor should use the active registry without rebuilding it"
        );
        assert_eq!(tracked_malloc_headers_matching(&churn_headers), 0);
        assert!(malloc_user_ptr_tracked(live_malloc));
        let capture_after = unsafe { (*capture_slot & POINTER_MASK) as usize };
        assert_ne!(capture_after, live_child);
        assert!(crate::arena::pointer_in_nursery(capture_after));
    }

    #[test]
    fn test_copied_minor_malloc_scaling_falls_back_when_registry_unavailable() {
        let _guard = CopyingNurseryTestGuard::new(0);
        let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let live_malloc = gc_malloc(
            std::mem::size_of::<crate::closure::ClosureHeader>(),
            GC_TYPE_CLOSURE,
        );
        unsafe {
            init_test_closure(live_malloc);
        }
        let mut raw_root = live_malloc as u64;
        js_gc_register_global_root(&mut raw_root as *mut u64 as i64);
        deactivate_malloc_registry_for_tests();

        let outcome = gc_collect_minor_with_trigger(GcTriggerSnapshot {
            kind: GcTriggerKind::Direct,
            steps_before: Some(GcStepSnapshot::current()),
        });
        let trace = outcome.trace.expect("test requested GC trace capture");

        assert_copied_minor_trace(
            &trace,
            false,
            CopiedMinorFallbackReason::MallocRegistryUnavailable,
            false,
        );
        assert_eq!(
            trace.copying_nursery.malloc_registry_rebuilds, 0,
            "copied-minor fallback must not rebuild the malloc registry"
        );
        assert!(malloc_user_ptr_tracked(live_malloc));
        assert_eq!(raw_root as usize, live_malloc as usize);
        assert!(
            !malloc_registry_active_for_tests(),
            "fallback mark-sweep should not activate the copied-minor malloc registry"
        );
    }

    #[test]
    fn test_copying_minor_falls_back_for_pinned_young_root() {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let child = young_leaf();
        unsafe {
            (*header_from_user_ptr(child as *const u8)).gc_flags |= GC_FLAG_PINNED;
        }
        js_shadow_slot_set(0, ptr_bits(child));

        let trace = collect_minor_trace(GcTriggerKind::Direct);
        let after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;

        assert_copied_minor_trace(
            &trace,
            false,
            CopiedMinorFallbackReason::PinnedYoungRoot,
            false,
        );
        assert_eq!(after, child);
        unsafe {
            (*header_from_user_ptr(child as *const u8)).gc_flags &= !GC_FLAG_PINNED;
        }
    }

    #[test]
    fn test_copying_minor_falls_back_for_pinned_young_dirty_slot() {
        let _guard = CopyingNurseryTestGuard::new(0);
        let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let child = young_leaf();
        let (old_arr, elements) = unsafe { alloc_old_test_array(1) };
        unsafe {
            *elements = ptr_bits(child);
            (*header_from_user_ptr(child as *const u8)).gc_flags |= GC_FLAG_PINNED;
        }
        js_write_barrier_slot(ptr_bits(old_arr as usize), elements as u64, ptr_bits(child));

        let trace = collect_minor_trace(GcTriggerKind::Direct);
        let child_after = unsafe { (*elements & POINTER_MASK) as usize };

        assert_copied_minor_trace(
            &trace,
            false,
            CopiedMinorFallbackReason::PinnedYoungDirtySlot,
            false,
        );
        assert_eq!(child_after, child);
        unsafe {
            (*header_from_user_ptr(child as *const u8)).gc_flags &= !GC_FLAG_PINNED;
        }
    }

    #[test]
    fn test_copying_minor_falls_back_for_transitive_pinned_young_child() {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let arr = crate::array::js_array_alloc(1);
        let child = young_leaf();
        let elements = unsafe {
            (*arr).length = 1;
            let elements =
                (arr as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64;
            *elements = ptr_bits(child);
            layout_note_slot(arr as usize, 0, *elements);
            (*header_from_user_ptr(child as *const u8)).gc_flags |= GC_FLAG_PINNED;
            elements
        };
        if gc_force_evacuate_enabled() {
            // This test is about copying-preflight fallback; forced
            // evacuation would otherwise move the parent after fallback.
            let arr_header = unsafe { header_from_user_ptr(arr as *const u8) };
            CONS_PINNED.with(|s| {
                s.borrow_mut().insert(arr_header as usize);
            });
        }
        js_shadow_slot_set(0, ptr_bits(arr as usize));

        let trace = collect_minor_trace(GcTriggerKind::Direct);
        let arr_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
        let child_after = unsafe { (*elements & POINTER_MASK) as usize };

        assert_copied_minor_trace(
            &trace,
            false,
            CopiedMinorFallbackReason::PinnedYoungTransitive,
            false,
        );
        assert_eq!(
            arr_after, arr as usize,
            "copying nursery must fall back before moving the young parent"
        );
        assert_eq!(
            child_after, child,
            "pinned transitive young child must keep its raw address"
        );
        unsafe {
            let child_header = header_from_user_ptr(child as *const u8);
            assert_eq!(
                (*child_header).gc_flags & GC_FLAG_FORWARDED,
                0,
                "pinned child must not receive a forwarding pointer"
            );
            (*child_header).gc_flags &= !GC_FLAG_PINNED;
        }
    }

    unsafe fn alloc_old_test_map(
        capacity: u32,
    ) -> (*mut crate::map::MapHeader, *mut u64, std::alloc::Layout) {
        let map = crate::arena::arena_alloc_gc_old(
            std::mem::size_of::<crate::map::MapHeader>(),
            8,
            GC_TYPE_MAP,
        ) as *mut crate::map::MapHeader;
        let layout = std::alloc::Layout::from_size_align((capacity as usize * 16).max(8), 8)
            .expect("valid map entries layout");
        let entries = std::alloc::alloc_zeroed(layout) as *mut u64;
        assert!(!entries.is_null());
        (*map).size = 0;
        (*map).capacity = capacity;
        (*map).entries = entries as *mut f64;
        (map, entries, layout)
    }

    unsafe fn retire_old_test_map(
        map: *mut crate::map::MapHeader,
        entries: *mut u64,
        layout: std::alloc::Layout,
    ) {
        (*map).size = 0;
        (*map).capacity = 0;
        (*map).entries = std::ptr::null_mut();
        std::alloc::dealloc(entries as *mut u8, layout);
    }

    unsafe fn field_index_not_on_last_page(fields: *mut u64, field_count: u32) -> usize {
        assert!(field_count > 1);
        let last_page =
            crate::arena::generation_page_for_addr(fields.add(field_count as usize - 1) as usize);
        for i in 0..field_count as usize {
            if crate::arena::generation_page_for_addr(fields.add(i) as usize) != last_page {
                return i;
            }
        }
        panic!("test object did not span multiple field pages");
    }

    unsafe fn field_indices_on_distinct_pages(
        fields: *mut u64,
        field_count: u32,
    ) -> (usize, usize) {
        assert!(field_count > 1);
        let first = field_index_not_on_last_page(fields, field_count);
        let first_page = crate::arena::generation_page_for_addr(fields.add(first) as usize);
        for i in 0..field_count as usize {
            if crate::arena::generation_page_for_addr(fields.add(i) as usize) != first_page {
                return (first, i);
            }
        }
        panic!("test object did not span two field pages");
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
            "old→young write must dirty the remembered page"
        );
        // Same write again should NOT double-count (dirty pages dedup).
        js_write_barrier(parent_nanbox, child_nanbox);
        assert_eq!(
            remembered_set_size(),
            1,
            "duplicate barrier call must dedup the dirty page"
        );
    }

    #[test]
    fn test_write_barrier_slot_marks_dirty_page_and_dedups() {
        reset_remembered_set();
        let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
        unsafe {
            *fields = POINTER_TAG | young as u64;
        }
        js_write_barrier_slot(
            POINTER_TAG | old_obj as u64,
            fields as u64,
            POINTER_TAG | young as u64,
        );
        assert_eq!(remembered_dirty_page_count(), 1);
        js_write_barrier_slot(
            POINTER_TAG | old_obj as u64,
            fields as u64,
            POINTER_TAG | young as u64,
        );
        assert_eq!(
            remembered_dirty_page_count(),
            1,
            "same dirty page should be logged once"
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
    fn test_write_barrier_non_pointer_parent_skipped() {
        reset_remembered_set();
        let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        js_write_barrier_slot(0x7FFE_0000_0000_002A_u64, 0, POINTER_TAG | young as u64);
        assert_eq!(
            remembered_set_size(),
            0,
            "non-pointer parent must not dirty remembered pages"
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
    fn test_write_barrier_slot_clear() {
        reset_remembered_set();
        let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
        js_write_barrier_slot(
            POINTER_TAG | old_obj as u64,
            fields as u64,
            POINTER_TAG | young as u64,
        );
        assert_eq!(remembered_dirty_page_count(), 1);
        remembered_set_clear();
        assert_eq!(remembered_dirty_page_count(), 0);
        assert_eq!(remembered_set_size(), 0);
    }

    #[test]
    fn test_gc_collect_minor_clears_rs() {
        reset_remembered_set();
        let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        let (old, fields) = unsafe { alloc_old_test_object(1) };
        unsafe {
            *fields = POINTER_TAG | young as u64;
        }
        js_write_barrier_slot(
            POINTER_TAG | old as u64,
            fields as u64,
            POINTER_TAG | young as u64,
        );
        assert_eq!(remembered_set_size(), 1);
        let _freed = gc_collect_minor();
        assert_eq!(
            remembered_set_size(),
            0,
            "minor GC must clear RS just like full GC does"
        );
    }

    #[test]
    fn test_dirty_page_scan_marks_young_child() {
        reset_remembered_set();
        clear_marks();
        let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
        unsafe {
            *fields = POINTER_TAG | young as u64;
        }
        js_write_barrier_slot(
            POINTER_TAG | old_obj as u64,
            fields as u64,
            POINTER_TAG | young as u64,
        );
        let valid_ptrs = build_valid_pointer_set();
        let stats = mark_remembered_set_roots(&valid_ptrs);
        assert_eq!(stats.dirty_pages_scanned, 1);
        assert_eq!(stats.old_objects_considered, 1);
        assert_eq!(stats.dirty_objects_scanned, 1);
        assert!(
            stats.dirty_slots_scanned >= 1,
            "dirty page should scan at least the written field slot"
        );
        assert_eq!(stats.newly_marked, 1);
        unsafe {
            let child_header = header_from_user_ptr(young as *const u8);
            assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
        }
        clear_marks();
        remembered_set_clear();
    }

    #[test]
    fn test_dirty_page_array_scan_is_slot_range_bounded() {
        reset_remembered_set();
        clear_marks();
        let dirty_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        let clean_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        let (old_arr, elements) = unsafe { alloc_old_test_array(2048) };
        let (dirty_idx, clean_idx) = unsafe { field_indices_on_distinct_pages(elements, 2048) };
        let dirty_slot = unsafe { elements.add(dirty_idx) };
        unsafe {
            *dirty_slot = POINTER_TAG | dirty_child as u64;
            *elements.add(clean_idx) = POINTER_TAG | clean_child as u64;
        }
        js_write_barrier_slot(
            POINTER_TAG | old_arr as u64,
            dirty_slot as u64,
            POINTER_TAG | dirty_child as u64,
        );

        let valid_ptrs = build_valid_pointer_set();
        let stats = mark_remembered_set_roots(&valid_ptrs);
        assert_eq!(stats.old_objects_considered, 1);
        assert_eq!(stats.dirty_objects_scanned, 1);
        assert_eq!(stats.dirty_slot_ranges_scanned, 1);
        assert!(
            stats.dirty_slots_scanned <= 512,
            "one dirty page should scan at most one 4 KiB page of u64 slots"
        );
        unsafe {
            let dirty_header = header_from_user_ptr(dirty_child as *const u8);
            let clean_header = header_from_user_ptr(clean_child as *const u8);
            assert_ne!((*dirty_header).gc_flags & GC_FLAG_MARKED, 0);
            assert_eq!((*clean_header).gc_flags & GC_FLAG_MARKED, 0);
        }
        clear_marks();
        remembered_set_clear();
    }

    #[test]
    fn test_dirty_page_scan_ignores_clean_old_pages() {
        reset_remembered_set();
        clear_marks();
        let dirty_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        let clean_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        let (dirty_obj, dirty_fields) = unsafe { alloc_old_test_object(2048) };
        let dirty_idx = unsafe { field_index_not_on_last_page(dirty_fields, 2048) };
        let dirty_slot = unsafe { dirty_fields.add(dirty_idx) };
        unsafe {
            *dirty_slot = POINTER_TAG | dirty_child as u64;
        }
        let (_clean_obj, clean_fields) = unsafe { alloc_old_test_object(2048) };
        let clean_idx = unsafe { field_index_not_on_last_page(clean_fields, 2048) };
        unsafe {
            *clean_fields.add(clean_idx) = POINTER_TAG | clean_child as u64;
        }

        js_write_barrier_slot(
            POINTER_TAG | dirty_obj as u64,
            dirty_slot as u64,
            POINTER_TAG | dirty_child as u64,
        );

        let valid_ptrs = build_valid_pointer_set();
        let stats = mark_remembered_set_roots(&valid_ptrs);
        assert_eq!(stats.dirty_pages_scanned, 1);
        assert_eq!(
            stats.old_objects_considered, 1,
            "clean old pages must not feed objects into the dirty scan"
        );
        assert_eq!(stats.dirty_objects_scanned, 1);
        assert_eq!(stats.dirty_slot_ranges_scanned, 1);
        assert!(
            stats.dirty_slots_scanned <= 512,
            "one dirty field page should not scan the whole old object"
        );
        unsafe {
            let dirty_header = header_from_user_ptr(dirty_child as *const u8);
            let clean_header = header_from_user_ptr(clean_child as *const u8);
            assert_ne!((*dirty_header).gc_flags & GC_FLAG_MARKED, 0);
            assert_eq!(
                (*clean_header).gc_flags & GC_FLAG_MARKED,
                0,
                "young child stored only on a clean old page should not be marked"
            );
        }
        clear_marks();
        remembered_set_clear();
    }

    #[test]
    fn test_dirty_page_scan_dedupes_object_spanning_dirty_pages() {
        reset_remembered_set();
        clear_marks();
        let young_a = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        let young_b = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        let (old_obj, fields) = unsafe { alloc_old_test_object(2048) };
        let (idx_a, idx_b) = unsafe { field_indices_on_distinct_pages(fields, 2048) };
        let slot_a = unsafe { fields.add(idx_a) };
        let slot_b = unsafe { fields.add(idx_b) };
        unsafe {
            *slot_a = POINTER_TAG | young_a as u64;
            *slot_b = POINTER_TAG | young_b as u64;
        }

        js_write_barrier_slot(
            POINTER_TAG | old_obj as u64,
            slot_a as u64,
            POINTER_TAG | young_a as u64,
        );
        js_write_barrier_slot(
            POINTER_TAG | old_obj as u64,
            slot_b as u64,
            POINTER_TAG | young_b as u64,
        );

        let valid_ptrs = build_valid_pointer_set();
        let stats = mark_remembered_set_roots(&valid_ptrs);
        assert_eq!(stats.dirty_pages_scanned, 2);
        assert_eq!(
            stats.old_objects_considered, 1,
            "one object spanning two dirty pages should be considered once"
        );
        assert_eq!(stats.dirty_objects_scanned, 1);
        assert_eq!(stats.dirty_slot_pages_considered, 2);
        assert!(stats.dirty_slot_ranges_scanned <= 2);
        assert!(
            stats.dirty_slots_scanned <= 1024,
            "two dirty field pages should bound scanning to two pages"
        );
        assert_eq!(stats.newly_marked, 2);
        unsafe {
            let header_a = header_from_user_ptr(young_a as *const u8);
            let header_b = header_from_user_ptr(young_b as *const u8);
            assert_ne!((*header_a).gc_flags & GC_FLAG_MARKED, 0);
            assert_ne!((*header_b).gc_flags & GC_FLAG_MARKED, 0);
        }
        clear_marks();
        remembered_set_clear();
    }

    #[test]
    fn test_dirty_page_map_entry_scan_is_external_range_bounded() {
        reset_remembered_set();
        clear_marks();
        let dirty_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        let clean_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        let (map, entries, layout) = unsafe { alloc_old_test_map(2048) };
        unsafe {
            (*map).size = 2048;
        }
        let (dirty_idx, clean_idx) = unsafe { field_indices_on_distinct_pages(entries, 4096) };
        let dirty_slot = unsafe { entries.add(dirty_idx) };
        unsafe {
            *dirty_slot = POINTER_TAG | dirty_child as u64;
            *entries.add(clean_idx) = POINTER_TAG | clean_child as u64;
        }
        write_barrier_slot_inner(
            POINTER_TAG | map as u64,
            dirty_slot as usize,
            POINTER_TAG | dirty_child as u64,
            true,
        );

        let valid_ptrs = build_valid_pointer_set();
        let stats = mark_remembered_set_roots(&valid_ptrs);
        assert_eq!(stats.dirty_pages_scanned, 1);
        assert_eq!(stats.old_objects_considered, 1);
        assert_eq!(stats.dirty_objects_scanned, 1);
        assert_eq!(stats.dirty_slot_ranges_scanned, 1);
        assert!(
            stats.dirty_slots_scanned <= 512,
            "one dirty map entries page should not scan the whole map"
        );
        unsafe {
            let dirty_header = header_from_user_ptr(dirty_child as *const u8);
            let clean_header = header_from_user_ptr(clean_child as *const u8);
            assert_ne!((*dirty_header).gc_flags & GC_FLAG_MARKED, 0);
            assert_eq!((*clean_header).gc_flags & GC_FLAG_MARKED, 0);
            retire_old_test_map(map, entries, layout);
        }
        clear_marks();
        remembered_set_clear();
    }

    #[test]
    fn test_dirty_lazy_array_external_cache_scan_marks_bitmap_selected_child() {
        reset_remembered_set();
        clear_marks();

        let cached_length = 4usize;
        let lazy = crate::arena::arena_alloc_gc_old(
            std::mem::size_of::<crate::json_tape::LazyArrayHeader>(),
            8,
            GC_TYPE_LAZY_ARRAY,
        ) as *mut crate::json_tape::LazyArrayHeader;
        let cache_bytes = cached_length * std::mem::size_of::<crate::value::JSValue>();
        let cache = crate::arena::arena_alloc_gc(cache_bytes, 8, GC_TYPE_STRING)
            as *mut crate::value::JSValue;
        let bitmap =
            crate::arena::arena_alloc_gc(std::mem::size_of::<u64>(), 8, GC_TYPE_STRING) as *mut u64;
        let selected_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        let unselected_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;

        unsafe {
            std::ptr::write_bytes(cache as *mut u8, 0, cache_bytes);
            *bitmap = 0;
            (*lazy).cached_length = cached_length as u32;
            (*lazy).magic = crate::json_tape::LAZY_ARRAY_MAGIC;
            (*lazy).root_idx = 0;
            (*lazy).tape_len = 0;
            (*lazy).blob_str = std::ptr::null();
            (*lazy).materialized = std::ptr::null_mut();
            (*lazy).materialized_elements = cache;
            (*lazy).materialized_bitmap = bitmap;
            (*lazy).walk_idx = u32::MAX;
            (*lazy).walk_tape_pos = 0;
            (*lazy).cumulative_walk_steps = 0;

            *(cache.add(1) as *mut u64) = ptr_bits(selected_child);
            *(cache.add(2) as *mut u64) = ptr_bits(unselected_child);
            *bitmap = 1u64 << 1;
        }

        let lazy_header = unsafe { header_from_user_ptr(lazy as *const u8) };
        let dirty_cache_page =
            crate::arena::generation_page_for_addr(unsafe { cache.add(1) } as usize);
        assert!(mark_dirty_external_slot_page(
            lazy_header as usize,
            dirty_cache_page
        ));

        let valid_ptrs = build_valid_pointer_set();
        let stats = mark_remembered_set_roots(&valid_ptrs);
        assert_eq!(stats.old_objects_considered, 1);
        assert_eq!(stats.dirty_objects_scanned, 1);
        assert_eq!(
            stats.newly_marked, 1,
            "external lazy-array cache page should mark bitmap-selected nursery values"
        );
        unsafe {
            let selected_header = header_from_user_ptr(selected_child as *const u8);
            let unselected_header = header_from_user_ptr(unselected_child as *const u8);
            assert_ne!((*selected_header).gc_flags & GC_FLAG_MARKED, 0);
            assert_eq!(
                (*unselected_header).gc_flags & GC_FLAG_MARKED,
                0,
                "unset cache bitmap entries must not be treated as live slots"
            );
        }

        clear_marks();
        remembered_set_clear();
    }

    #[test]
    fn test_dirty_page_map_external_dedupes_and_clears() {
        reset_remembered_set();
        let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        let (map, entries, layout) = unsafe { alloc_old_test_map(16) };
        unsafe {
            (*map).size = 16;
            *entries.add(1) = POINTER_TAG | young as u64;
        }
        let slot = unsafe { entries.add(1) };
        write_barrier_slot_inner(
            POINTER_TAG | map as u64,
            slot as usize,
            POINTER_TAG | young as u64,
            true,
        );
        write_barrier_slot_inner(
            POINTER_TAG | map as u64,
            slot as usize,
            POINTER_TAG | young as u64,
            true,
        );
        assert_eq!(remembered_set_size(), 1);
        remembered_set_clear();
        assert_eq!(remembered_set_size(), 0);
        unsafe {
            retire_old_test_map(map, entries, layout);
        }
    }

    #[test]
    fn test_dirty_page_map_realloc_span_marks_new_entries_pages() {
        reset_remembered_set();
        clear_marks();
        let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        let (map, entries, layout) = unsafe { alloc_old_test_map(1024) };
        unsafe {
            (*map).size = 1024;
            *entries.add(1023) = POINTER_TAG | young as u64;
        }
        let new_layout = std::alloc::Layout::from_size_align(2048 * 16, 8).unwrap();
        let new_entries = unsafe { std::alloc::alloc_zeroed(new_layout) as *mut u64 };
        assert!(!new_entries.is_null());
        unsafe {
            std::ptr::copy_nonoverlapping(entries, new_entries, 2048);
            (*map).entries = new_entries as *mut f64;
            (*map).capacity = 2048;
        }
        dirty_external_slot_span(map as usize, new_entries as usize, 2048);

        let valid_ptrs = build_valid_pointer_set();
        let stats = mark_remembered_set_roots(&valid_ptrs);
        assert!(stats.dirty_pages_scanned >= 1);
        assert_eq!(stats.old_objects_considered, 1);
        assert_eq!(stats.newly_marked, 1);
        unsafe {
            let header = header_from_user_ptr(young as *const u8);
            assert_ne!((*header).gc_flags & GC_FLAG_MARKED, 0);
            retire_old_test_map(map, new_entries, new_layout);
            std::alloc::dealloc(entries as *mut u8, layout);
        }
        clear_marks();
        remembered_set_clear();
    }

    #[test]
    fn test_rewrite_remembered_dirty_range_updates_unmarked_old_parent_slot() {
        reset_remembered_set();
        clear_marks();
        let child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
        unsafe {
            *fields = POINTER_TAG | child as u64;
        }
        js_write_barrier_slot(
            POINTER_TAG | old_obj as u64,
            fields as u64,
            POINTER_TAG | child as u64,
        );
        let valid_ptrs = build_valid_pointer_set();
        let new_child = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
        unsafe {
            set_forwarding_address(
                header_from_user_ptr(child as *const u8),
                new_child as *mut u8,
            );
            let old_header = header_from_user_ptr(old_obj as *const u8);
            assert_eq!(
                (*old_header).gc_flags & GC_FLAG_MARKED,
                0,
                "test must prove dirty rewrite does not depend on marked parent walk"
            );
        }

        rewrite_remembered_dirty_ranges(&valid_ptrs);

        unsafe {
            assert_eq!(
                *fields,
                POINTER_TAG | new_child as u64,
                "dirty old parent slot should be rewritten even when parent is unmarked"
            );
        }
        remembered_set_clear();
    }

    #[test]
    fn test_rewrite_remembered_dirty_range_updates_map_external_entry_span() {
        reset_remembered_set();
        clear_marks();
        let dirty_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        let clean_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        let (map, entries, layout) = unsafe { alloc_old_test_map(2048) };
        unsafe {
            (*map).size = 2048;
        }
        let (dirty_idx, clean_idx) = unsafe { field_indices_on_distinct_pages(entries, 4096) };
        let dirty_slot = unsafe { entries.add(dirty_idx) };
        unsafe {
            *dirty_slot = POINTER_TAG | dirty_child as u64;
            *entries.add(clean_idx) = POINTER_TAG | clean_child as u64;
        }
        write_barrier_slot_inner(
            POINTER_TAG | map as u64,
            dirty_slot as usize,
            POINTER_TAG | dirty_child as u64,
            true,
        );
        let valid_ptrs = build_valid_pointer_set();
        let new_dirty_child = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
        let new_clean_child = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
        unsafe {
            set_forwarding_address(
                header_from_user_ptr(dirty_child as *const u8),
                new_dirty_child as *mut u8,
            );
            set_forwarding_address(
                header_from_user_ptr(clean_child as *const u8),
                new_clean_child as *mut u8,
            );
        }

        rewrite_remembered_dirty_ranges(&valid_ptrs);

        unsafe {
            assert_eq!(*dirty_slot, POINTER_TAG | new_dirty_child as u64);
            assert_eq!(
                *entries.add(clean_idx),
                POINTER_TAG | clean_child as u64,
                "external dirty rewrite should stay bounded to the logged entry page"
            );
            retire_old_test_map(map, entries, layout);
        }
        remembered_set_clear();
    }

    #[test]
    fn test_rewrite_remembered_fallback_header_updates_fields() {
        reset_remembered_set();
        clear_marks();
        let child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
        unsafe {
            *fields = POINTER_TAG | child as u64;
        }
        REMEMBERED_SET.with(|s| {
            s.borrow_mut().insert(old_obj as usize - GC_HEADER_SIZE);
        });
        let valid_ptrs = build_valid_pointer_set();
        let new_child = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
        unsafe {
            set_forwarding_address(
                header_from_user_ptr(child as *const u8),
                new_child as *mut u8,
            );
        }

        rewrite_remembered_dirty_ranges(&valid_ptrs);

        unsafe {
            assert_eq!(*fields, POINTER_TAG | new_child as u64);
        }
        remembered_set_clear();
    }

    #[test]
    fn test_object_hashset_fallback_still_scans() {
        reset_remembered_set();
        clear_marks();
        let (old_obj, _fields) = unsafe { alloc_old_test_object(1) };
        let old_header = old_obj as usize - GC_HEADER_SIZE;
        REMEMBERED_SET.with(|s| {
            s.borrow_mut().insert(old_header);
        });
        let valid_ptrs = build_valid_pointer_set();
        let stats = mark_remembered_set_roots(&valid_ptrs);
        assert_eq!(stats.entries_scanned, 1);
        assert_eq!(stats.valid_roots, 1);
        assert_eq!(stats.newly_marked, 1);
        unsafe {
            let header = header_from_user_ptr(old_obj as *const u8);
            assert_ne!((*header).gc_flags & GC_FLAG_MARKED, 0);
        }
        clear_marks();
        remembered_set_clear();
    }

    #[test]
    fn test_gc_collect_minor_keeps_dirty_page_child_alive() {
        reset_remembered_set();
        let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
        unsafe {
            *fields = POINTER_TAG | young as u64;
        }
        js_write_barrier_slot(
            POINTER_TAG | old_obj as u64,
            fields as u64,
            POINTER_TAG | young as u64,
        );
        let _ = gc_collect_minor();
        unsafe {
            let child_header = header_from_user_ptr(young as *const u8);
            assert_ne!(
                (*child_header).gc_flags & GC_FLAG_HAS_SURVIVED,
                0,
                "dirty-page remembered scan should keep the young child alive through minor GC"
            );
        }
        remembered_set_clear();
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
    fn test_rewrite_mutable_root_slots_updates_shadow_and_global_roots() {
        let _guard = ShadowAndGlobalRootResetGuard;
        reset_shadow_stack();
        reset_global_roots();

        let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
        let valid_ptrs = build_valid_pointer_set();
        let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
        unsafe {
            let nursery_hdr = header_from_user_ptr(nursery_user) as *mut GcHeader;
            set_forwarding_address(nursery_hdr, old_user);
        }

        let shadow_bits = POINTER_TAG | ((nursery_user as u64) & POINTER_MASK);
        let expected_shadow_bits = POINTER_TAG | ((old_user as u64) & POINTER_MASK);
        let shadow = js_shadow_frame_push(1);
        js_shadow_slot_set(0, shadow_bits);

        let mut global_bits = nursery_user as u64;
        js_gc_register_global_root((&mut global_bits as *mut u64) as i64);

        rewrite_mutable_root_slots(&valid_ptrs, None);

        assert_eq!(
            js_shadow_slot_get(0),
            expected_shadow_bits,
            "shadow stack slot should be rewritten to the forwarding target"
        );
        assert_eq!(
            global_bits, old_user as u64,
            "registered global root slot should be rewritten in place"
        );

        js_shadow_frame_pop(shadow);
    }

    #[test]
    fn test_runtime_root_visitor_marks_and_rewrites_nanbox_slot() {
        let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
        let valid_ptrs = build_valid_pointer_set();
        let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
        let nursery_hdr = unsafe { header_from_user_ptr(nursery_user) as *mut GcHeader };
        unsafe {
            set_forwarding_address(nursery_hdr, old_user);
        }

        let mut slot = f64::from_bits(POINTER_TAG | (nursery_user as u64 & POINTER_MASK));
        RuntimeRootVisitor::for_mark(&valid_ptrs).visit_nanbox_f64_slot(&mut slot);
        unsafe {
            assert_ne!((*nursery_hdr).gc_flags & GC_FLAG_MARKED, 0);
        }

        RuntimeRootVisitor::for_rewrite(&valid_ptrs).visit_nanbox_f64_slot(&mut slot);
        assert_eq!(
            slot.to_bits(),
            POINTER_TAG | (old_user as u64 & POINTER_MASK)
        );
    }

    #[test]
    fn test_runtime_root_visitor_rewrites_raw_pointer_slots() {
        let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
        let valid_ptrs = build_valid_pointer_set();
        let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
        unsafe {
            set_forwarding_address(
                header_from_user_ptr(nursery_user) as *mut GcHeader,
                old_user,
            );
        }

        let mut mut_ptr = nursery_user;
        let mut const_ptr = nursery_user as *const u8;
        let mut usize_slot = nursery_user as usize;
        let mut i64_slot = nursery_user as i64;

        let mut visitor = RuntimeRootVisitor::for_rewrite(&valid_ptrs);
        visitor.visit_raw_mut_ptr_slot(&mut mut_ptr);
        visitor.visit_raw_const_ptr_slot(&mut const_ptr);
        visitor.visit_usize_slot(&mut usize_slot);
        visitor.visit_i64_slot(&mut i64_slot);

        assert_eq!(mut_ptr, old_user);
        assert_eq!(const_ptr, old_user as *const u8);
        assert_eq!(usize_slot, old_user as usize);
        assert_eq!(i64_slot, old_user as i64);
    }

    #[test]
    fn test_runtime_root_visitor_rewrites_cell_and_atomic_slots() {
        let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
        let valid_ptrs = build_valid_pointer_set();
        let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
        unsafe {
            set_forwarding_address(
                header_from_user_ptr(nursery_user) as *mut GcHeader,
                old_user,
            );
        }

        let cell = Cell::new(f64::from_bits(
            POINTER_TAG | (nursery_user as u64 & POINTER_MASK),
        ));
        let atomic = std::sync::atomic::AtomicPtr::new(nursery_user);
        let atomic_i64 = std::sync::atomic::AtomicI64::new(nursery_user as i64);

        let mut visitor = RuntimeRootVisitor::for_rewrite(&valid_ptrs);
        visitor.visit_cell_f64_slot(&cell);
        visitor.visit_atomic_raw_mut_ptr_slot(
            &atomic,
            std::sync::atomic::Ordering::Acquire,
            std::sync::atomic::Ordering::Release,
        );
        visitor.visit_atomic_i64_slot(
            &atomic_i64,
            std::sync::atomic::Ordering::Acquire,
            std::sync::atomic::Ordering::Release,
        );

        assert_eq!(
            cell.get().to_bits(),
            POINTER_TAG | (old_user as u64 & POINTER_MASK)
        );
        assert_eq!(atomic.load(std::sync::atomic::Ordering::Acquire), old_user);
        assert_eq!(
            atomic_i64.load(std::sync::atomic::Ordering::Acquire),
            old_user as i64
        );
    }

    #[test]
    fn test_runtime_root_visitor_rewrites_metadata_without_marking() {
        let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
        let valid_ptrs = build_valid_pointer_set();
        let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
        let nursery_hdr = unsafe { header_from_user_ptr(nursery_user) as *mut GcHeader };
        unsafe {
            set_forwarding_address(nursery_hdr, old_user);
        }

        let mut metadata = nursery_user as usize;
        RuntimeRootVisitor::for_mark(&valid_ptrs).visit_metadata_usize_slot(&mut metadata);
        unsafe {
            assert_eq!(
                (*nursery_hdr).gc_flags & GC_FLAG_MARKED,
                0,
                "metadata-only slots must not become roots"
            );
        }

        RuntimeRootVisitor::for_rewrite(&valid_ptrs).visit_metadata_usize_slot(&mut metadata);
        assert_eq!(metadata, old_user as usize);
    }

    #[test]
    fn test_promise_iter_result_mutable_scanner_rewrites_slot() {
        let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
        let valid_ptrs = build_valid_pointer_set();
        let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
        unsafe {
            set_forwarding_address(
                header_from_user_ptr(nursery_user) as *mut GcHeader,
                old_user,
            );
        }

        let initial = f64::from_bits(POINTER_TAG | (nursery_user as u64 & POINTER_MASK));
        crate::promise::js_iter_result_set(initial, 0);

        let mut visitor = RuntimeRootVisitor::for_rewrite(&valid_ptrs);
        crate::promise::scan_iter_result_root_mut(&mut visitor);

        assert_eq!(
            crate::promise::js_iter_result_get_value().to_bits(),
            POINTER_TAG | (old_user as u64 & POINTER_MASK)
        );
        crate::promise::js_iter_result_set(0.0, 0);
    }

    #[test]
    fn test_evacuation_verify_detects_stale_forwarded_root_slot() {
        let _guard = ShadowAndGlobalRootResetGuard;
        reset_shadow_stack();
        reset_global_roots();
        let fixture = ForwardedRootFixture::new();
        let shadow = js_shadow_frame_push(1);
        js_shadow_slot_set(0, fixture.nursery_bits);

        assert_panics_with("shadow stack roots", || {
            verify_mutable_root_slots(&fixture.valid_ptrs);
        });

        js_shadow_frame_pop(shadow);
    }

    #[test]
    fn test_evacuation_verify_detects_stale_forwarded_runtime_scanner_slot() {
        let fixture = ForwardedRootFixture::new();
        crate::promise::test_seed_promise_scanner_roots(
            fixture.nursery_user as *mut crate::promise::Promise,
            fixture.nursery_value(),
            fixture.nursery_value(),
            fixture.nursery_user as *const crate::closure::ClosureHeader,
            fixture.nursery_user as *mut crate::promise::Promise,
        );

        assert_panics_with("runtime mutable root scanner", || {
            let mut visitor =
                RuntimeRootVisitor::for_verify(&fixture.valid_ptrs, "runtime mutable root scanner");
            promise_mutable_root_scanner(&mut visitor);
        });

        crate::promise::test_clear_promise_scanner_roots();
    }

    #[test]
    fn test_evacuation_verify_detects_stale_forwarded_dirty_range_slot() {
        reset_remembered_set();
        clear_marks();
        let child = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
        let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
        let child_bits = POINTER_TAG | (child as u64 & POINTER_MASK);
        unsafe {
            *fields = child_bits;
        }
        js_write_barrier_slot(POINTER_TAG | old_obj as u64, fields as u64, child_bits);
        let valid_ptrs = build_valid_pointer_set();
        let old_child = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
        unsafe {
            set_forwarding_address(header_from_user_ptr(child), old_child);
        }

        assert_panics_with("remembered dirty ranges", || {
            verify_remembered_dirty_ranges(&valid_ptrs);
        });

        remembered_set_clear();
    }

    #[test]
    fn test_evacuation_verify_detects_stale_forwarded_heap_field() {
        clear_marks();
        let fixture = ForwardedRootFixture::new();
        let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
        unsafe {
            *fields = fixture.nursery_bits;
            let header = header_from_user_ptr(old_obj as *const u8);
            (*header).gc_flags |= GC_FLAG_MARKED;
            assert_panics_with("heap fields", || {
                verify_heap_object_fields(header, &fixture.valid_ptrs, "heap fields");
            });
            (*header).gc_flags &= !GC_FLAG_MARKED;
        }
    }

    #[test]
    fn test_evacuation_verify_copy_only_pinned_root_allows_non_forwarded_target() {
        let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
        let valid_ptrs = build_valid_pointer_set();
        unsafe {
            (*header_from_user_ptr(user)).gc_flags |= GC_FLAG_PINNED;
        }
        verify_copy_only_scanner_bits(
            POINTER_TAG | (user as u64 & POINTER_MASK),
            &valid_ptrs,
            "copy-only root scanner",
        );
        unsafe {
            (*header_from_user_ptr(user)).gc_flags &= !GC_FLAG_PINNED;
        }
    }

    #[test]
    fn test_evacuation_verify_copy_only_root_rejects_forwarded_target() {
        let fixture = ForwardedRootFixture::new();
        assert_panics_with("copy-only root scanner", || {
            verify_copy_only_scanner_bits(
                fixture.nursery_bits,
                &fixture.valid_ptrs,
                "copy-only root scanner",
            );
        });
    }

    struct ForwardedRootFixture {
        valid_ptrs: ValidPointerSet,
        nursery_user: *mut u8,
        old_user: *mut u8,
        nursery_bits: u64,
        old_bits: u64,
    }

    impl ForwardedRootFixture {
        fn new() -> Self {
            let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
            let valid_ptrs = build_valid_pointer_set();
            let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
            unsafe {
                set_forwarding_address(
                    header_from_user_ptr(nursery_user) as *mut GcHeader,
                    old_user,
                );
            }
            Self {
                valid_ptrs,
                nursery_user,
                old_user,
                nursery_bits: POINTER_TAG | (nursery_user as u64 & POINTER_MASK),
                old_bits: POINTER_TAG | (old_user as u64 & POINTER_MASK),
            }
        }

        fn nursery_value(&self) -> f64 {
            f64::from_bits(self.nursery_bits)
        }

        fn old_addr(&self) -> usize {
            self.old_user as usize
        }

        fn nursery_addr(&self) -> usize {
            self.nursery_user as usize
        }

        fn nursery_i64(&self) -> i64 {
            self.nursery_user as i64
        }
    }

    #[test]
    fn test_gc_init_mutable_scanner_families_rewrite_runtime_slots() {
        let fixture = ForwardedRootFixture::new();
        let active_context_handle = -724_331;
        let shape_id = 0x51A9_E001;
        let box_ptr = crate::r#box::js_box_alloc(fixture.nursery_value());

        crate::promise::test_seed_promise_scanner_roots(
            fixture.nursery_user as *mut crate::promise::Promise,
            fixture.nursery_value(),
            fixture.nursery_value(),
            fixture.nursery_user as *const crate::closure::ClosureHeader,
            fixture.nursery_user as *mut crate::promise::Promise,
        );
        crate::timer::test_seed_timer_scanner_roots(
            fixture.nursery_user as *mut crate::promise::Promise,
            fixture.nursery_value(),
            fixture.nursery_i64(),
            fixture.nursery_value(),
            fixture.nursery_value(),
        );
        crate::exception::test_set_exception(fixture.nursery_value());
        crate::async_context::clear_store(active_context_handle);
        crate::async_context::enter_with(active_context_handle, fixture.nursery_value());
        crate::builtins::test_seed_queued_microtask(fixture.nursery_i64(), fixture.nursery_value());
        crate::async_hooks::test_seed_async_hooks_scanner_roots(
            fixture.nursery_user as *const crate::closure::ClosureHeader,
            fixture.nursery_value(),
        );
        crate::object::test_seed_shape_cache_root(
            shape_id,
            fixture.nursery_user as *mut crate::array::ArrayHeader,
        );
        crate::regex::test_set_last_exec_groups(
            fixture.nursery_user as *mut crate::object::ObjectHeader,
        );
        crate::array::test_seed_template_raw_roots(
            fixture.nursery_user as *mut crate::array::ArrayHeader,
            fixture.nursery_user as *mut crate::array::ArrayHeader,
        );
        crate::object::test_seed_transition_cache_root(fixture.nursery_addr());
        crate::object::test_seed_overflow_fields_root(fixture.nursery_addr(), fixture.nursery_bits);
        crate::json::test_seed_parse_roots(
            fixture.nursery_value(),
            fixture.nursery_user as *const crate::string::StringHeader,
        );
        crate::string::test_seed_intern_table_root(fixture.nursery_addr());
        crate::builtins::test_set_console_log_singleton(fixture.nursery_i64());
        crate::node_submodules::test_seed_node_submodule_roots(
            fixture.nursery_user as *mut crate::closure::ClosureHeader,
            fixture.nursery_user as *mut crate::object::ObjectHeader,
            fixture.nursery_user as *mut crate::closure::ClosureHeader,
        );
        crate::promise::js_iter_result_set(fixture.nursery_value(), 0);
        crate::promise::test_seed_async_step_thunk_cache(
            fixture.nursery_addr(),
            fixture.nursery_user as *mut crate::closure::ClosureHeader,
            fixture.nursery_user as *mut crate::closure::ClosureHeader,
        );
        crate::closure::test_clear_singleton_closure_caches();
        crate::closure::test_seed_singleton_closure_cache(
            test_no_capture_singleton_func as *const u8,
            fixture.nursery_user as *mut crate::closure::ClosureHeader,
        );
        crate::closure::test_seed_captured_singleton_closure_cache(
            test_captured_singleton_func as *const u8,
            vec![fixture.nursery_bits],
            fixture.nursery_user as *mut crate::closure::ClosureHeader,
        );
        crate::tui::hooks::test_seed_hook_slot_roots(fixture.nursery_bits);
        crate::tui::state::test_reset_state_slots();
        let tui_state = crate::tui::state::js_perry_tui_state_alloc(fixture.nursery_value());

        let mut visitor = RuntimeRootVisitor::for_rewrite(&fixture.valid_ptrs);
        promise_mutable_root_scanner(&mut visitor);
        timer_mutable_root_scanner(&mut visitor);
        exception_mutable_root_scanner(&mut visitor);
        async_context_mutable_root_scanner(&mut visitor);
        async_hooks_mutable_root_scanner(&mut visitor);
        shape_cache_mutable_root_scanner(&mut visitor);
        crate::regex::scan_last_exec_groups_root_mut(&mut visitor);
        crate::array::scan_template_raw_roots_mut(&mut visitor);
        transition_cache_mutable_root_scanner(&mut visitor);
        overflow_fields_mutable_root_scanner(&mut visitor);
        json_parse_mutable_root_scanner(&mut visitor);
        intern_table_mutable_root_scanner(&mut visitor);
        crate::builtins::scan_console_log_singleton_roots_mut(&mut visitor);
        crate::node_submodules::scan_node_submodule_singleton_roots_mut(&mut visitor);
        crate::r#box::scan_box_roots_mut(&mut visitor);
        crate::promise::scan_iter_result_root_mut(&mut visitor);
        crate::promise::scan_async_step_thunk_cache_mut(&mut visitor);
        crate::closure::scan_singleton_closure_roots_mut(&mut visitor);
        crate::tui::hooks::scan_hook_slot_roots_mut(&mut visitor);
        crate::tui::state::scan_state_slot_roots_mut(&mut visitor);

        let promise = crate::promise::test_promise_scanner_snapshot();
        assert_eq!(promise.task_promise_ptr, fixture.old_addr());
        assert_eq!(promise.task_value_bits, fixture.old_bits);
        assert_eq!(promise.task_context_store_bits, fixture.old_bits);
        assert_eq!(promise.inline_callback_ptr, fixture.old_addr());
        assert_eq!(promise.inline_next_ptr, fixture.old_addr());
        assert_eq!(promise.inline_value_bits, fixture.old_bits);
        assert_eq!(promise.async_step_callback_ptr, fixture.old_addr());
        assert_eq!(promise.async_step_next_ptr, fixture.old_addr());
        assert_eq!(promise.async_step_value_bits, fixture.old_bits);
        assert_eq!(promise.promise_context_key, fixture.old_addr());
        assert_eq!(promise.promise_context_store_bits, fixture.old_bits);
        assert_eq!(promise.scheduled_promise_ptr, fixture.old_addr());
        assert_eq!(promise.scheduled_value_bits, fixture.old_bits);

        let timer = crate::timer::test_timer_scanner_snapshot();
        assert_eq!(timer.timeout_promise_ptr, fixture.old_addr());
        assert_eq!(timer.timeout_value_bits, fixture.old_bits);
        assert_eq!(timer.callback_ptr, fixture.old_addr());
        assert_eq!(timer.callback_arg_bits, fixture.old_bits);
        assert_eq!(timer.callback_context_store_bits, fixture.old_bits);
        assert_eq!(timer.interval_callback_ptr, fixture.old_addr());
        assert_eq!(timer.interval_context_store_bits, fixture.old_bits);

        assert_eq!(
            crate::exception::js_get_exception().to_bits(),
            fixture.old_bits
        );
        assert_eq!(
            crate::async_context::get_store(active_context_handle)
                .map(f64::to_bits)
                .unwrap_or(0),
            fixture.old_bits
        );
        assert_eq!(
            crate::builtins::test_queued_microtask_snapshot(),
            (fixture.old_addr(), fixture.old_bits)
        );
        assert_eq!(
            crate::async_hooks::test_async_hooks_scanner_snapshot(),
            (fixture.old_addr(), fixture.old_bits)
        );
        assert_eq!(
            crate::object::test_shape_cache_root(shape_id),
            (fixture.old_addr(), fixture.old_addr())
        );
        assert_eq!(crate::regex::test_last_exec_groups(), fixture.old_addr());
        assert_eq!(
            crate::array::test_template_raw_roots(),
            (fixture.old_addr(), fixture.old_addr())
        );
        assert_eq!(
            crate::object::test_transition_cache_root(),
            fixture.old_addr()
        );
        assert_eq!(
            crate::object::test_overflow_fields_root(),
            (fixture.old_addr(), fixture.old_bits)
        );
        assert_eq!(
            crate::json::test_parse_roots_snapshot(),
            (fixture.old_bits, fixture.old_addr())
        );
        assert_eq!(crate::string::test_intern_table_root(), fixture.old_addr());
        assert_eq!(
            crate::builtins::test_console_log_singleton() as usize,
            fixture.old_addr()
        );
        assert_eq!(
            crate::node_submodules::test_node_submodule_roots(),
            (fixture.old_addr(), fixture.old_addr(), fixture.old_addr())
        );
        assert_eq!(
            crate::r#box::js_box_get(box_ptr).to_bits(),
            fixture.old_bits
        );
        assert_eq!(
            crate::promise::js_iter_result_get_value().to_bits(),
            fixture.old_bits
        );
        assert_eq!(
            crate::promise::test_async_step_thunk_cache(),
            (fixture.old_addr(), fixture.old_addr(), fixture.old_addr())
        );
        assert_eq!(
            crate::closure::test_singleton_closure_cache_entry(
                test_no_capture_singleton_func as *const u8
            )
            .map(|ptr| ptr as usize),
            Some(fixture.old_addr())
        );
        assert_eq!(
            crate::closure::test_captured_singleton_closure_cache_entries(
                test_captured_singleton_func as *const u8
            ),
            vec![(
                vec![fixture.old_bits],
                fixture.old_user as *mut crate::closure::ClosureHeader
            )]
        );
        assert_eq!(
            crate::tui::hooks::test_hook_slot_roots(),
            (fixture.old_bits, fixture.old_bits, fixture.old_bits)
        );
        assert_eq!(
            crate::tui::state::js_perry_tui_state_get(tui_state).to_bits(),
            fixture.old_bits
        );

        crate::promise::test_clear_promise_scanner_roots();
        crate::timer::test_clear_timer_scanner_roots(fixture.nursery_addr(), fixture.old_addr());
        crate::exception::js_clear_exception();
        crate::async_context::clear_store(active_context_handle);
        crate::object::test_clear_transition_cache_root();
        crate::string::test_clear_intern_table_root();
        crate::builtins::test_set_console_log_singleton(0);
        crate::async_hooks::reset_for_tests();
        crate::promise::js_iter_result_set(0.0, 0);
        crate::closure::test_clear_singleton_closure_caches();
        crate::tui::state::test_reset_state_slots();
    }

    #[cfg(feature = "ohos-napi")]
    #[test]
    fn test_arkts_callbacks_mutable_scanner_rewrites_callback_slots() {
        let fixture = ForwardedRootFixture::new();
        let callback_idx = 3;
        crate::arkts_callbacks::test_clear_arkts_callback_roots();
        crate::arkts_callbacks::test_seed_arkts_callback_root(
            callback_idx,
            fixture.nursery_value(),
        );

        let mut visitor = RuntimeRootVisitor::for_rewrite(&fixture.valid_ptrs);
        crate::arkts_callbacks::arkts_callbacks_root_scanner_mut(&mut visitor);

        assert_eq!(
            crate::arkts_callbacks::test_arkts_callback_root(callback_idx),
            fixture.old_bits
        );
        crate::arkts_callbacks::test_clear_arkts_callback_roots();
    }

    #[cfg(feature = "ohos-napi")]
    #[test]
    fn test_lazy_media_mutable_scanner_rewrites_callback_slots() {
        let fixture = ForwardedRootFixture::new();
        let handle = i64::MIN + 377;
        crate::media_playback::test_seed_media_callback_roots(
            handle,
            fixture.nursery_value(),
            fixture.nursery_value(),
        );

        let mut visitor = RuntimeRootVisitor::for_rewrite(&fixture.valid_ptrs);
        crate::media_playback::media_callbacks_root_scanner_mut(&mut visitor);

        assert_eq!(
            crate::media_playback::test_media_callback_roots(handle),
            (fixture.old_bits, fixture.old_bits)
        );
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
        clear_marks();
        let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
        let header = unsafe { header_from_user_ptr(user) as *mut GcHeader };
        unsafe {
            (*header).gc_flags |= GC_FLAG_MARKED;
        }
        let stats = pin_currently_marked_as_conservative();
        assert!(
            is_conservatively_pinned(header),
            "marked header should land in CONS_PINNED"
        );
        assert_eq!(stats.pinned_roots, 1);
        assert_eq!(stats.pinned_bytes, unsafe { (*header).size as usize });
        // Cleanup for test isolation.
        unsafe {
            (*header).gc_flags &= !GC_FLAG_MARKED;
        }
        CONS_PINNED.with(|s| s.borrow_mut().clear());
    }

    #[test]
    fn test_pin_currently_marked_skips_unmarked() {
        CONS_PINNED.with(|s| s.borrow_mut().clear());
        clear_marks();
        let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
        let header = unsafe { header_from_user_ptr(user) as *const GcHeader };
        // Ensure unmarked.
        unsafe {
            assert_eq!((*(header as *mut GcHeader)).gc_flags & GC_FLAG_MARKED, 0);
        }
        let stats = pin_currently_marked_as_conservative();
        assert_eq!(stats.pinned_roots, 0);
        assert_eq!(stats.pinned_bytes, 0);
        assert!(
            !is_conservatively_pinned(header),
            "unmarked header should NOT land in CONS_PINNED"
        );
    }

    #[test]
    fn test_conservative_pin_stats_exclude_legacy_copy_only_scanner_pins() {
        CONS_PINNED.with(|s| s.borrow_mut().clear());
        clear_marks();
        let conservative_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
        let legacy_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
        let conservative_header =
            unsafe { header_from_user_ptr(conservative_user) as *mut GcHeader };
        let legacy_header = unsafe { header_from_user_ptr(legacy_user) as *mut GcHeader };
        unsafe {
            (*conservative_header).gc_flags |= GC_FLAG_MARKED;
        }

        let stats = pin_currently_marked_as_conservative();
        let conservative_bytes = unsafe { (*conservative_header).size as usize };
        assert_eq!(stats.pinned_roots, 1);
        assert_eq!(stats.pinned_bytes, conservative_bytes);

        let valid_ptrs = build_valid_pointer_set();
        let legacy_bits = POINTER_TAG | (legacy_user as u64 & POINTER_MASK);
        let legacy_bytes = mark_copy_only_scanner_bits(legacy_bits, &valid_ptrs, true);
        assert_eq!(
            legacy_bytes,
            Some(unsafe { (*legacy_header).size as usize })
        );
        assert_eq!(
            cons_pinned_count(),
            2,
            "evacuation set still contains both conservative and legacy pins"
        );
        assert_eq!(
            stats.pinned_roots, 1,
            "conservative pin stats must not absorb later legacy scanner pins"
        );
        assert_eq!(stats.pinned_bytes, conservative_bytes);

        clear_marks();
        CONS_PINNED.with(|s| s.borrow_mut().clear());
    }

    #[test]
    fn test_evacuation_policy() {
        fn snapshot(
            tenured: usize,
            candidate: usize,
            candidate_objects: usize,
            pinned: usize,
            rss: u64,
            previous_pause_us: u64,
            pre_evac_pause_us: u64,
        ) -> EvacuationPolicySnapshot {
            EvacuationPolicySnapshot {
                tenured_still_in_nursery_bytes: tenured,
                candidate_bytes: candidate,
                candidate_objects,
                reclaimable_candidate_bytes: candidate,
                reclaimable_candidate_objects: candidate_objects,
                retained_forwarded_stub_bytes: 0,
                retained_forwarded_stub_objects: 0,
                conservative_pinned_bytes: pinned,
                rss_bytes: rss,
                previous_pause_us,
                pre_evac_pause_us,
            }
        }

        fn decide(
            snapshot: EvacuationPolicySnapshot,
            considered: bool,
            force: bool,
        ) -> EvacuationPolicyDecision {
            evacuation_policy_final_decision(
                EvacuationPolicyDecision {
                    allowed: true,
                    considered,
                    force,
                    enabled: false,
                    reason: "test",
                    snapshot,
                },
                snapshot,
            )
        }

        let zero_candidates = decide(
            snapshot(MIN_TENURED_NURSERY_BYTES, 0, 0, 0, 0, 0, 0),
            true,
            false,
        );
        assert!(!zero_candidates.enabled);
        assert_eq!(zero_candidates.reason, "zero_candidates");

        let productive = decide(
            snapshot(
                MIN_TENURED_NURSERY_BYTES * 2,
                MIN_CANDIDATE_BYTES * 2,
                2,
                0,
                0,
                0,
                0,
            ),
            true,
            false,
        );
        assert!(productive.enabled);
        assert_eq!(productive.reason, "nursery_pressure");

        let rss_pressure = decide(
            snapshot(
                MIN_CANDIDATE_BYTES,
                MIN_CANDIDATE_BYTES,
                1,
                0,
                RSS_PRESSURE_BYTES,
                0,
                0,
            ),
            true,
            false,
        );
        assert!(rss_pressure.enabled);
        assert_eq!(rss_pressure.reason, "rss_pressure");

        let pinned_dominated = decide(
            snapshot(
                MIN_TENURED_NURSERY_BYTES * 4,
                MIN_CANDIDATE_BYTES,
                1,
                MIN_TENURED_NURSERY_BYTES * 3,
                0,
                0,
                0,
            ),
            true,
            false,
        );
        assert!(!pinned_dominated.enabled);
        assert_eq!(
            pinned_dominated.reason,
            "reclaimable_candidate_ratio_below_threshold"
        );

        let retained_stub_dominated = decide(
            EvacuationPolicySnapshot {
                tenured_still_in_nursery_bytes: MIN_TENURED_NURSERY_BYTES * 2,
                candidate_bytes: MIN_CANDIDATE_BYTES * 2,
                candidate_objects: 16,
                reclaimable_candidate_bytes: 0,
                reclaimable_candidate_objects: 0,
                retained_forwarded_stub_bytes: 64,
                retained_forwarded_stub_objects: 1,
                conservative_pinned_bytes: 0,
                rss_bytes: 0,
                previous_pause_us: 0,
                pre_evac_pause_us: 0,
            },
            true,
            false,
        );
        assert!(
            !retained_stub_dominated.enabled,
            "movable bytes alone must not enable evacuation when retained forwarded stubs keep the candidate blocks live"
        );
        assert_eq!(
            retained_stub_dominated.reason,
            "zero_reclaimable_candidates"
        );

        let pause_skip = decide(
            snapshot(
                MIN_TENURED_NURSERY_BYTES,
                MIN_CANDIDATE_BYTES,
                1,
                0,
                0,
                MAX_PREVIOUS_PAUSE_US + 1,
                0,
            ),
            true,
            false,
        );
        assert!(!pause_skip.enabled);
        assert_eq!(pause_skip.reason, "pause_budget_exceeded");

        let hard_rss_override = decide(
            snapshot(
                MIN_TENURED_NURSERY_BYTES,
                MIN_CANDIDATE_BYTES,
                1,
                0,
                RSS_HARD_PRESSURE_BYTES,
                MAX_PREVIOUS_PAUSE_US + 1,
                0,
            ),
            true,
            false,
        );
        assert!(hard_rss_override.enabled);
        assert_eq!(hard_rss_override.reason, "rss_hard_pressure");

        let force = decide(snapshot(0, 64, 1, 0, 0, 0, 0), true, true);
        assert!(force.enabled);
        assert_eq!(force.reason, "force");

        let low_pressure =
            evacuation_policy_initial_decision(0, RSS_PRESSURE_BYTES - 1, 0, 0, true, false, true);
        assert!(!low_pressure.considered);
        assert!(!low_pressure.enabled);
        assert_eq!(low_pressure.reason, "low_pressure");

        let pressure_barriers_inactive = evacuation_policy_initial_decision(
            MIN_TENURED_NURSERY_BYTES,
            RSS_HARD_PRESSURE_BYTES,
            0,
            0,
            true,
            false,
            false,
        );
        assert!(!pressure_barriers_inactive.considered);
        assert!(!pressure_barriers_inactive.enabled);
        assert_eq!(pressure_barriers_inactive.reason, "barriers_inactive");

        let force_barriers_inactive =
            evacuation_policy_initial_decision(0, 0, 0, 0, true, true, false);
        assert!(force_barriers_inactive.force);
        assert!(!force_barriers_inactive.considered);
        assert!(!force_barriers_inactive.enabled);
        assert_eq!(force_barriers_inactive.reason, "barriers_inactive");

        let disabled = evacuation_policy_initial_decision(
            MIN_TENURED_NURSERY_BYTES,
            RSS_HARD_PRESSURE_BYTES,
            0,
            0,
            false,
            true,
            false,
        );
        assert!(!disabled.considered);
        assert!(!disabled.enabled);
        assert_eq!(disabled.reason, "disabled");
    }

    #[test]
    fn test_evacuation_policy_snapshot_excludes_retained_forwarded_stub_blocks() {
        clear_marks();
        CONS_PINNED.with(|s| s.borrow_mut().clear());

        let mut pair = None;
        for _ in 0..64 {
            let candidate = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT) as usize;
            let stub = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_ARRAY) as usize;
            let candidate_block = arena_block_index_for_user(candidate);
            let stub_block = arena_block_index_for_user(stub);
            if candidate_block.is_some()
                && candidate_block == stub_block
                && candidate_block.unwrap() < crate::arena::general_block_count()
            {
                pair = Some((candidate, stub));
                break;
            }
        }
        let (candidate, stub) =
            pair.expect("test setup should find two nursery allocations in one general block");
        let candidate_header = unsafe { header_from_user_ptr(candidate as *const u8) };
        let stub_header = unsafe { header_from_user_ptr(stub as *const u8) };
        let stub_target = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_ARRAY);
        unsafe {
            (*candidate_header).gc_flags |= GC_FLAG_MARKED | GC_FLAG_TENURED;
            set_forwarding_address(stub_header, stub_target);
        }

        let snapshot =
            evacuation_policy_snapshot_after_mark(EvacuationPolicySnapshot::default(), false, 0);
        let candidate_size = unsafe { (*candidate_header).size as usize };
        let stub_size = unsafe { (*stub_header).size as usize };
        assert!(
            snapshot.candidate_bytes >= candidate_size,
            "marked tenured object should be a movable candidate"
        );
        assert_eq!(
            snapshot.reclaimable_candidate_bytes, 0,
            "candidate sharing a block with a retained forwarded stub is not block-reclaimable"
        );
        assert!(
            snapshot.retained_forwarded_stub_bytes >= stub_size,
            "policy snapshot should report retained forwarded stubs that keep blocks live"
        );

        unsafe {
            (*candidate_header).gc_flags &= !(GC_FLAG_MARKED | GC_FLAG_TENURED);
            (*stub_header).gc_flags &= !GC_FLAG_FORWARDED;
        }
        CONS_PINNED.with(|s| s.borrow_mut().clear());
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
        assert_eq!(n.objects, 0, "pinned tenured object must not be evacuated");
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
        assert_eq!(
            n.objects, 1,
            "tenured non-pinned marked object must evacuate"
        );
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
    fn test_release_evacuated_original_forwarding_stub_before_sweep() {
        CONS_PINNED.with(|s| s.borrow_mut().clear());
        clear_marks();
        let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
        let header = unsafe { header_from_user_ptr(user) as *mut GcHeader };
        unsafe {
            (*header).gc_flags |= GC_FLAG_MARKED | GC_FLAG_TENURED;
        }
        let total = unsafe { (*header).size as usize };
        let mut evacuated_new_headers = Vec::new();
        let mut evacuated_original_headers = Vec::new();
        let moved = evacuate_tenured_nursery_objects_collecting(
            false,
            &mut evacuated_new_headers,
            &mut evacuated_original_headers,
        );
        assert_eq!(moved.moved_objects, 1);
        assert_eq!(moved.moved_bytes, total);
        assert_eq!(evacuated_original_headers, vec![header]);
        unsafe {
            assert_ne!(
                (*header).gc_flags & GC_FLAG_FORWARDED,
                0,
                "evacuation must install a forwarding stub for rewrite"
            );
        }

        let released = release_evacuated_original_forwarding_stubs(&evacuated_original_headers);
        assert_eq!(released.released_original_objects, 1);
        assert_eq!(released.released_original_bytes, total);
        unsafe {
            assert_eq!(
                (*header).gc_flags & GC_FLAG_FORWARDED,
                0,
                "GC-evacuation originals should release FORWARDED before sweep"
            );
        }

        let sweep = sweep_with_age_bump(false);
        assert!(
            sweep.freed_bytes >= total as u64,
            "released evacuation original should contribute to sweep reclaimable bytes"
        );
        CONS_PINNED.with(|s| s.borrow_mut().clear());
    }

    #[test]
    fn test_sweep_reports_and_retains_non_evacuation_forwarded_stub() {
        clear_marks();
        let stub = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_ARRAY);
        let target = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_ARRAY);
        let stub_header = unsafe { header_from_user_ptr(stub) as *mut GcHeader };
        let total = unsafe { (*stub_header).size as usize };
        unsafe {
            set_forwarding_address(stub_header, target);
        }

        let sweep = sweep_with_age_bump(false);
        assert!(
            sweep.retained_forwarded_stub_objects >= 1,
            "sweep should count retained non-evacuation forwarding stubs"
        );
        assert!(
            sweep.retained_forwarded_stub_bytes >= total,
            "sweep should report bytes retained by non-evacuation forwarding stubs"
        );
        unsafe {
            assert_ne!(
                (*stub_header).gc_flags & GC_FLAG_FORWARDED,
                0,
                "sweep must not clear array-growth forwarding stubs"
            );
            (*stub_header).gc_flags &= !GC_FLAG_FORWARDED;
        }
    }

    #[test]
    fn test_forced_evacuation_barriers_inactive_does_not_forward_candidate() {
        struct ResetGcTestState;

        impl Drop for ResetGcTestState {
            fn drop(&mut self) {
                reset_shadow_stack();
                reset_global_roots();
                reset_remembered_set();
                clear_marks();
                clear_mark_seeds();
                CONS_PINNED.with(|s| s.borrow_mut().clear());
            }
        }

        let _reset = ResetGcTestState;
        let _isolation = copying_nursery_isolation_lock();
        let _barrier_guard = GeneratedWriteBarrierTestGuard::inactive();
        reset_shadow_stack();
        reset_global_roots();
        reset_remembered_set();
        clear_marks();
        clear_mark_seeds();
        CONS_PINNED.with(|s| s.borrow_mut().clear());
        if !gc_force_evacuate_enabled() {
            return;
        }
        assert!(
            !generated_write_barriers_emitted(),
            "this canary must verify the barriers-inactive evacuation gate"
        );

        let frame = js_shadow_frame_push(1);
        let (parent, _) = unsafe { alloc_nursery_test_object(0) };
        let parent_user = parent as usize;
        let parent_header = unsafe { header_from_user_ptr(parent as *const u8) };

        unsafe {
            (*parent_header).gc_flags |= GC_FLAG_TENURED;
        }
        js_shadow_slot_set(0, ptr_bits(parent_user));

        let _ = gc_collect_minor();

        let parent_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
        assert_eq!(
            parent_after, parent_user,
            "forced evacuation must not move candidates when generated barriers are inactive"
        );
        unsafe {
            assert_eq!(
                (*parent_header).gc_flags & GC_FLAG_FORWARDED,
                0,
                "barriers-inactive policy gate must leave the nursery candidate unforwarded"
            );
        }

        js_shadow_frame_pop(frame);
    }

    #[test]
    fn test_evacuated_old_parent_re_remembers_young_child_canary() {
        struct ResetGcTestState;

        impl Drop for ResetGcTestState {
            fn drop(&mut self) {
                reset_shadow_stack();
                reset_global_roots();
                reset_remembered_set();
                clear_marks();
                clear_mark_seeds();
                CONS_PINNED.with(|s| s.borrow_mut().clear());
            }
        }

        let _reset = ResetGcTestState;
        let _isolation = copying_nursery_isolation_lock();
        let _barrier_guard = GeneratedWriteBarrierTestGuard::active();
        let _copy_only_root_guard = TemporaryCopyOnlyRootScanner::new();
        reset_shadow_stack();
        reset_global_roots();
        reset_remembered_set();
        clear_marks();
        clear_mark_seeds();
        CONS_PINNED.with(|s| s.borrow_mut().clear());
        if !gc_force_evacuate_enabled() {
            return;
        }
        assert!(
            generated_write_barriers_emitted(),
            "this canary must exercise policy evacuation with generated barriers active"
        );

        let frame = js_shadow_frame_push(1);
        let (parent, fields) = unsafe { alloc_nursery_test_object(1) };
        let child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        let parent_user = parent as usize;
        let parent_header = unsafe { header_from_user_ptr(parent as *const u8) };
        let child_header = unsafe { header_from_user_ptr(child as *const u8) };

        unsafe {
            *fields = ptr_bits(child);
            (*parent_header).gc_flags |= GC_FLAG_TENURED;
        }
        js_shadow_slot_set(0, ptr_bits(parent_user));
        CONS_PINNED.with(|s| {
            s.borrow_mut().insert(child_header as usize);
        });

        let _ = gc_collect_minor();

        let parent_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
        assert_ne!(
            parent_after, parent_user,
            "rooted parent should be rewritten to its evacuated old-gen copy"
        );
        assert!(
            crate::arena::pointer_in_old_gen(parent_after),
            "evacuated parent should live in old-gen"
        );
        unsafe {
            assert_eq!(
                (*parent_header).gc_flags & GC_FLAG_FORWARDED,
                0,
                "original nursery parent should release its GC forwarding pointer after rewrite"
            );
        }

        let parent_after_fields = unsafe {
            (parent_after as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>())
                as *mut u64
        };
        let child_after = unsafe { (*parent_after_fields & POINTER_MASK) as usize };
        assert_eq!(
            child_after, child,
            "evacuated parent should still point at the pinned nursery child"
        );
        assert!(
            crate::arena::pointer_in_nursery(child_after),
            "child should remain young after parent evacuation"
        );

        assert!(
            remembered_set_size() > 0,
            "evacuated old parent retaining a nursery child must be re-remembered after the collection clear"
        );

        clear_marks();
        let valid_ptrs = build_valid_pointer_set();
        let stats = mark_remembered_set_roots(&valid_ptrs);
        assert!(
            stats.newly_marked > 0,
            "remembered scan should mark the nursery child reachable only from the evacuated old parent"
        );
        unsafe {
            assert_ne!(
                (*child_header).gc_flags & GC_FLAG_MARKED,
                0,
                "remembered scan should mark the pinned nursery child"
            );
        }

        clear_marks();
        CONS_PINNED.with(|s| {
            s.borrow_mut().insert(child_header as usize);
        });
        let _ = gc_collect_minor();

        let parent_after_second = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
        assert_eq!(
            parent_after_second, parent_after,
            "second minor GC should keep using the evacuated old parent"
        );
        let child_after_second = unsafe { (*parent_after_fields & POINTER_MASK) as usize };
        assert_eq!(
            child_after_second, child,
            "second minor GC should keep the nursery child alive through the rebuilt remembered entry"
        );
        unsafe {
            assert_ne!(
                (*child_header).gc_flags & GC_FLAG_TENURED,
                0,
                "second minor GC should mark and age the nursery child"
            );
        }

        js_shadow_frame_pop(frame);
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
        let (old, fields) = unsafe { alloc_old_test_object(1) };
        unsafe {
            *fields = POINTER_TAG | young as u64;
        }
        js_write_barrier_slot(
            POINTER_TAG | old as u64,
            fields as u64,
            POINTER_TAG | young as u64,
        );
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
            init_test_closure(ptr2);
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

    /// Issue #856 regression: `mark_stack_roots` performs a `setjmp`
    /// into a `u64` register-snapshot buffer, and `promise.rs` does a
    /// `setjmp` into an `i32` trap buffer. Both used to declare their
    /// own conflicting `extern "C" fn setjmp(...)` — the Rust compiler
    /// emitted `clashing_extern_declarations`, and on platforms where
    /// the ABI didn't happen to round-trip the bits the behaviour was
    /// UB. The fix routes both through `crate::ffi::setjmp::setjmp`
    /// with a libc-matching `*mut c_int` signature; this test exists
    /// to make sure the GC stack-scan path keeps running without
    /// crashing now that the extern is shared.
    ///
    /// `gc_collect_inner` invokes `mark_stack_roots`, which is the
    /// real production setjmp call site. The matching promise.rs
    /// trap path is exercised by `crate::ffi::setjmp::tests` and by
    /// any test that drains microtasks; the regression here is
    /// specifically the GC half of the pair.
    #[test]
    fn test_issue_856_setjmp_stack_scan_does_not_crash() {
        // A few allocations so `mark_stack_roots` actually has
        // pointers to consider; the test is about the setjmp not
        // crashing, not about a specific mark outcome.
        let _ptr1 = gc_malloc(32, GC_TYPE_STRING);
        let ptr2 = gc_malloc(48, GC_TYPE_CLOSURE);
        let _ptr3 = gc_malloc(16, GC_TYPE_BIGINT);
        unsafe {
            init_test_closure(ptr2);
        }

        // Should complete cleanly. If the shared `_setjmp` extern is
        // mis-sized, libc will scribble past the 256-byte buffer in
        // `mark_stack_roots` and corrupt this frame's stack — the
        // test would crash long before reaching the assert.
        gc_collect_inner();

        // Sanity: GC ran (count advanced). We don't assert anything
        // about WHICH allocations survived — that's covered by other
        // tests.
        let count = GC_STATS.with(|s| s.borrow().collection_count);
        assert!(count > 0, "gc_collect_inner should bump collection_count");
    }
}
