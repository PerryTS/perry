//! #7246 — a `Symbol`'s description used to be a `*mut StringHeader` inside
//! `SymbolHeader`, and **the collector never traced or rewrote it**.
//!
//! `alloc_symbol` gc_malloc's the header as `GC_TYPE_STRING`, whose type info
//! is `pointer_free: true` / `GcRewriteDescriptorKind::Leaf` /
//! `GcLayoutSlotKind::None`, so nothing walks into the payload. That is right
//! for a *string*, whose payload is bytes, and wrong for a *symbol*, whose
//! payload's third word was a heap pointer — and symbols and strings share one
//! GC type, so no descriptor could distinguish them. A symbol that was itself
//! perfectly rooted could have its description reaped or relocated out from
//! under it, and `String(sym)` / `sym.description` then read recycled memory.
//!
//! `SYMBOL_POINTERS` did not close it either:
//! `scan_symbol_pointer_metadata_roots_mut` visits the set with
//! `visit_metadata_usize_slot`, which rewrites a recorded address **without
//! marking**, and never looks at `(*ptr).description` at all.
//!
//! Fresh symbols now carry GC_TYPE_SYMBOL with a real description edge.
//! The edge moves and dies through ordinary tracing, without an id-keyed map.

use super::*;
use crate::symbol::SymbolHeader;

fn symbol_ptr_bits(sym: *mut SymbolHeader) -> u64 {
    ptr_bits(sym as usize)
}

/// The type descriptor, rather than a pointer-free String leaf, owns the edge.
#[test]
fn a_fresh_symbol_owns_a_traced_description_edge() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    unsafe {
        let desc = crate::string::js_string_from_bytes(b"k".as_ptr(), 1);
        let sym = crate::symbol::alloc_symbol(desc, false);
        assert_eq!((*sym).description, desc);
        assert!(!gc_type_is_pointer_free(GC_TYPE_SYMBOL));
        assert_eq!(
            gc_type_rewrite_descriptor_kind(GC_TYPE_SYMBOL),
            GcRewriteDescriptorKind::Symbol
        );
    }
}

/// BEHAVIOURAL. The symbol is the only owner of its description string.
/// A collection must retain and repair that edge while the symbol survives.
///
/// The recycling loop after the collection is load-bearing. Without it a stale
/// read can still find the old bytes intact and the test passes for the wrong
/// reason — the classic vacuous GC probe.
#[test]
fn a_symbols_description_survives_reclamation_of_the_string_it_came_from() {
    // These assertions are about the BUDGETED, non-moving assist —
    // `assert_automatic_minor_gc_progressed` requires a bounded assist to finish
    // or a budgeted cycle to be left ACTIVE. The default moving-loop pacing
    // deliberately routes nursery pressure AWAY from the budgeted stepper and
    // into the safepoint deferral, which in a Rust unit test has no loop
    // back-edge poll to drain it, so the assist is never entered. Pin the pacing
    // whose collection point this assertion describes, exactly as the
    // `debt_pacer` tests do. The moving default's rooting coverage for these
    // helpers is the gap suite's `test_gap_gc_*_rooting.ts` cases plus the
    // rate-1 schedule + from-space-protect runs, not this vehicle.
    let _legacy_pacing = crate::gc::policy::force_legacy_gc_pacing();
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    let sym = unsafe {
        let desc = crate::string::js_string_from_bytes(b"k".as_ptr(), 1);
        crate::symbol::alloc_symbol(desc, false)
    };
    let desc_addr_before = unsafe { (*sym).description as usize };
    js_shadow_slot_set(0, symbol_ptr_bits(sym));

    force_next_general_arena_alloc_slow();
    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();
    let _drive = crate::string::js_string_from_bytes(b"drive".as_ptr(), 5);
    drain_scheduled_minor_gc(before, "description string reclamation");

    // Recycle the retired bytes: 512 fresh strings of a distinctive byte, so a
    // stale read finds 'Z's rather than the original 'k'.
    for _ in 0..512 {
        let filler = [b'Z'; 32];
        let _ = crate::string::js_string_from_bytes(filler.as_ptr(), filler.len() as u32);
    }

    let sym_after = (js_shadow_slot_get(0) & POINTER_MASK) as *mut SymbolHeader;
    unsafe {
        let rendered =
            crate::symbol::js_symbol_to_string(f64::from_bits(symbol_ptr_bits(sym_after)));
        assert_string_bytes(rendered as *const crate::StringHeader, b"Symbol(k)");
    }
    assert_ne!(
        desc_addr_before, 0,
        "the fixture must use the owned description edge"
    );
}
