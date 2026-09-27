use super::super::*;
use super::support::*;
use std::cell::Cell;
use std::ffi::c_void;

thread_local! {
    static EDGE: Cell<u64> = const { Cell::new(0) };
    static OBSERVED: Cell<bool> = const { Cell::new(false) };
}

extern "C" fn phase(_: u32) {}
extern "C" fn observe(bits: u64, mark: extern "C" fn(u64, *mut c_void), ctx: *mut c_void) -> bool {
    if bits != ptr_bits(crate::value::addr_class::FETCH_HANDLE_BAND_START) {
        return false;
    }
    if !OBSERVED.with(|seen| seen.replace(true)) {
        mark(EDGE.with(Cell::get), ctx);
    }
    true
}

#[test]
fn publishing_a_fetch_root_during_incremental_marking_traces_its_edges() {
    let _guard = GcTestIsolationGuard::new();
    clear_marks();
    clear_mark_seeds();
    let ptr = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid = build_valid_pointer_set();
    EDGE.with(|edge| edge.set(ptr_bits(ptr as usize)));
    OBSERVED.with(|seen| seen.set(false));
    perry_ffi_gc_register_fetch_trace(phase, observe);
    begin_full_trace();
    let active = IncrementalMarkBarrierTestGuard::new(&valid);
    let scope = RuntimeHandleScope::new();
    let _root = scope.root_nanbox_u64(ptr_bits(crate::value::addr_class::FETCH_HANDLE_BAND_START));
    assert!(
        OBSERVED.with(Cell::get),
        "the provider must see a newly published handle"
    );
    assert_marked_user_ptr(ptr as usize, "the handle's heap edge must be shaded");
    drop(active);
    abort_full_trace();
    assert!(
        !full_trace_active(),
        "cancelled cycles must release the trace scope"
    );
}

thread_local! {
    static CALLS: Cell<usize> = const { Cell::new(0) };
}

extern "C" fn counting_observe(
    bits: u64,
    mark: extern "C" fn(u64, *mut c_void),
    ctx: *mut c_void,
) -> bool {
    CALLS.with(|calls| calls.set(calls.get() + 1));
    observe(bits, mark, ctx)
}

/// SUSPECTED in #11165's review: a handle stored mid-mark from an unscanned
/// slot into an already-scanned object must still reach the provider. The
/// store barrier's child prologue decodes any POINTER_TAG payload, so a
/// fetch-band handle is shaded like a heap pointer, not filtered as small.
#[test]
fn a_handle_stored_during_incremental_marking_reaches_the_provider() {
    let _guard = GcTestIsolationGuard::new();
    clear_marks();
    clear_mark_seeds();
    let parent = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let edge = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid = build_valid_pointer_set();
    EDGE.with(|slot| slot.set(ptr_bits(edge as usize)));
    OBSERVED.with(|seen| seen.set(false));
    perry_ffi_gc_register_fetch_trace(phase, observe);
    begin_full_trace();
    let active = IncrementalMarkBarrierTestGuard::new(&valid);
    crate::gc::js_write_barrier(
        ptr_bits(parent as usize),
        ptr_bits(crate::value::addr_class::FETCH_HANDLE_BAND_START),
    );
    assert!(
        OBSERVED.with(Cell::get),
        "the store barrier must offer a stored handle to the provider"
    );
    assert_marked_user_ptr(
        edge as usize,
        "the stored handle's heap edge must be shaded",
    );
    drop(active);
    abort_full_trace();
}

/// Ordinary heap words are filtered by arithmetic before the provider's
/// thread-local lookup and indirect call.
#[test]
fn only_fetch_band_words_reach_the_provider() {
    let _guard = GcTestIsolationGuard::new();
    let valid = build_valid_pointer_set();
    perry_ffi_gc_register_fetch_trace(phase, counting_observe);
    begin_full_trace();
    CALLS.with(|calls| calls.set(0));
    let heap = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    for bits in [
        ptr_bits(heap as usize),
        heap as u64,
        42.5f64.to_bits(),
        crate::value::TAG_UNDEFINED,
    ] {
        assert!(!observe_handle(bits, &valid));
    }
    assert_eq!(
        CALLS.with(Cell::get),
        0,
        "non-handle words reached the provider"
    );
    let band = crate::value::addr_class::FETCH_HANDLE_BAND_START + 1;
    observe_handle(ptr_bits(band), &valid);
    observe_handle(band as u64, &valid);
    assert_eq!(
        CALLS.with(Cell::get),
        2,
        "boxed and raw handle ids must reach it"
    );
    abort_full_trace();
    assert!(!handle_trace_active() || crate::proxy::gc_full_trace_active());
}

// #11453: the shared native-handle pool's weak-owner provider.
thread_local! {
    static POOL_CALLS: Cell<usize> = const { Cell::new(0) };
    static POOL_ARM: Cell<bool> = const { Cell::new(true) };
    static POOL_PHASES: Cell<u32> = const { Cell::new(0) };
}

extern "C" fn pool_phase(phase: u32) -> bool {
    POOL_PHASES.with(|seen| seen.set(seen.get() | (1 << phase)));
    phase != 0 || POOL_ARM.with(Cell::get)
}
extern "C" fn pool_observe(
    bits: u64,
    _mark: extern "C" fn(u64, *mut c_void),
    _ctx: *mut c_void,
) -> bool {
    POOL_CALLS.with(|calls| calls.set(calls.get() + 1));
    bits & crate::value::POINTER_MASK == 7 || bits == 7
}

#[test]
fn only_pool_band_words_reach_the_pool_provider() {
    let _guard = GcTestIsolationGuard::new();
    let valid = build_valid_pointer_set();
    POOL_ARM.with(|arm| arm.set(true));
    perry_ffi_gc_register_pool_handle_trace(pool_phase, pool_observe);
    begin_full_trace();
    assert!(
        handle_trace_active(),
        "an armed pool provider activates observation"
    );
    POOL_CALLS.with(|calls| calls.set(0));
    let heap = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    for bits in [
        ptr_bits(heap as usize),
        heap as u64,
        42.5f64.to_bits(),
        crate::value::TAG_UNDEFINED,
        ptr_bits(crate::value::addr_class::COMMON_HANDLE_BAND_END),
        ptr_bits(0),
    ] {
        observe_handle(bits, &valid);
    }
    assert_eq!(
        POOL_CALLS.with(Cell::get),
        0,
        "non-pool words reached the provider"
    );
    assert!(
        observe_handle(ptr_bits(7), &valid),
        "a boxed parked id is claimed"
    );
    assert!(observe_handle(7, &valid), "a raw parked id is claimed");
    assert!(!observe_handle(ptr_bits(8), &valid));
    assert_eq!(POOL_CALLS.with(Cell::get), 3);
    abort_full_trace();
    assert!(
        POOL_PHASES.with(Cell::get) & 0b101 == 0b101,
        "begin and abort were reported"
    );
    perry_ffi_gc_register_pool_handle_trace(pool_phase_unregistered, pool_observe);
}

extern "C" fn pool_phase_unregistered(_: u32) -> bool {
    false
}

#[test]
fn a_pool_provider_with_nothing_parked_costs_no_observation() {
    let _guard = GcTestIsolationGuard::new();
    let valid = build_valid_pointer_set();
    POOL_ARM.with(|arm| arm.set(false));
    perry_ffi_gc_register_pool_handle_trace(pool_phase, pool_observe);
    begin_full_trace();
    POOL_CALLS.with(|calls| calls.set(0));
    observe_handle(ptr_bits(7), &valid);
    assert_eq!(
        POOL_CALLS.with(Cell::get),
        0,
        "an unarmed provider must not be called"
    );
    POOL_PHASES.with(|seen| seen.set(0));
    abort_full_trace();
    assert_eq!(
        POOL_PHASES.with(Cell::get) & 0b100,
        0,
        "an unarmed provider is not told about the abort"
    );
    POOL_ARM.with(|arm| arm.set(true));
    perry_ffi_gc_register_pool_handle_trace(pool_phase_unregistered, pool_observe);
}

/// A parked id stored into an already-scanned object during incremental
/// marking must still reach the provider, or the id would be released while
/// the heap holds it.
#[test]
fn a_pool_id_stored_during_incremental_marking_reaches_the_provider() {
    let _guard = GcTestIsolationGuard::new();
    clear_marks();
    clear_mark_seeds();
    let parent = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid = build_valid_pointer_set();
    POOL_ARM.with(|arm| arm.set(true));
    perry_ffi_gc_register_pool_handle_trace(pool_phase, pool_observe);
    begin_full_trace();
    let active = IncrementalMarkBarrierTestGuard::new(&valid);
    POOL_CALLS.with(|calls| calls.set(0));
    crate::gc::js_write_barrier(ptr_bits(parent as usize), ptr_bits(7));
    assert!(
        POOL_CALLS.with(Cell::get) >= 1,
        "the store barrier must offer a stored pool id to the provider"
    );
    drop(active);
    abort_full_trace();
    perry_ffi_gc_register_pool_handle_trace(pool_phase_unregistered, pool_observe);
}
