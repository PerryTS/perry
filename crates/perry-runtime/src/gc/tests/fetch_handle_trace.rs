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
