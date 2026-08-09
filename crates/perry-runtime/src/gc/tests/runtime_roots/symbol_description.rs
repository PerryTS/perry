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
//! The fix removes the pointer instead of tracing it: the text is copied off
//! the GC heap at allocation time into an id-keyed thread-local map, and the
//! field is left null. So the strongest assertion here is structural — there
//! is no untraced pointer to get wrong — and it is the one a future change
//! would trip first.

use super::*;
use crate::symbol::SymbolHeader;

fn symbol_ptr_bits(sym: *mut SymbolHeader) -> u64 {
    ptr_bits(sym as usize)
}

/// STRUCTURAL. `SymbolHeader::description` must stay null: the whole point of
/// #7246's fix is that there is no longer a heap pointer in a payload the
/// collector treats as opaque bytes. A change that re-populates this field
/// re-opens the defect whether or not any behavioural test happens to catch it
/// on the day.
#[test]
fn a_fresh_symbol_stores_no_heap_pointer_in_its_payload() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::symbol::test_clear_fresh_symbol_descriptions();

    unsafe {
        let desc = crate::string::js_string_from_bytes(b"k".as_ptr(), 1);
        let sym = crate::symbol::alloc_symbol(desc, false);
        assert!(
            (*sym).description.is_null(),
            "SymbolHeader::description must stay null — GC_TYPE_STRING is a \
             pointer-free Leaf, so anything stored there is neither marked nor \
             rewritten (#7246)"
        );
        // …and the description is still readable, off-heap.
        let rendered = crate::symbol::js_symbol_to_string(f64::from_bits(symbol_ptr_bits(sym)));
        assert_string_bytes(rendered as *const crate::StringHeader, b"Symbol(k)");
    }

    crate::symbol::test_clear_fresh_symbol_descriptions();
}

/// BEHAVIOURAL. The description string is referenced by nothing once
/// `alloc_symbol` has copied its text, so a collection reclaims it and the
/// from-space bytes are recycled into later allocations. The symbol itself is
/// kept alive in a shadow slot — that is deliberately NOT the subject; the
/// subject is whether its description survives.
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
    // zeal + from-space-protect runs, not this vehicle.
    let _legacy_pacing = crate::gc::policy::force_legacy_gc_pacing();
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();
    crate::symbol::test_clear_fresh_symbol_descriptions();

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
    assert_eq!(
        desc_addr_before, 0,
        "the payload pointer must have been null all along (#7246); a non-null \
         value here means this test measured the OLD representation and its \
         green result says nothing"
    );

    crate::symbol::test_clear_fresh_symbol_descriptions();
}

/// The price of interning off-heap is retention, and the issue named it:
/// "a workload that makes millions of symbols would feel it". `prune_dead_symbol_pointers`
/// pays it down — the description map is keyed on the symbol id, so the same
/// liveness verdict that prunes `SYMBOL_POINTERS` prunes the descriptions.
///
/// Asserted rather than assumed: without the prune, this table is a leak with a
/// doc comment claiming otherwise.
#[test]
fn dead_symbols_descriptions_are_pruned_with_their_pointers() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::symbol::test_clear_symbol_side_table_roots();
    crate::symbol::test_clear_fresh_symbol_descriptions();

    // One symbol we keep, and several we abandon.
    let keeper = unsafe {
        let desc = crate::string::js_string_from_bytes(b"keeper".as_ptr(), 6);
        crate::symbol::alloc_symbol(desc, false)
    };
    js_shadow_slot_set(0, symbol_ptr_bits(keeper));
    for _ in 0..8 {
        unsafe {
            let desc = crate::string::js_string_from_bytes(b"doomed".as_ptr(), 6);
            let _ = crate::symbol::alloc_symbol(desc, false);
        }
    }
    let seeded = crate::symbol::test_fresh_symbol_description_count();
    assert_eq!(
        seeded, 9,
        "setup did not record one description per described symbol"
    );

    // Prune with a predicate that declares everything except the keeper dead —
    // the same shape `gc::dead_owner` hands `prune_dead_symbol_pointers`.
    let keeper_addr = keeper as usize;
    crate::symbol::prune_dead_symbol_pointers(&|ptr| ptr != keeper_addr);

    assert_eq!(
        crate::symbol::test_fresh_symbol_description_count(),
        1,
        "descriptions of dead symbols must be pruned alongside their pointers — \
         otherwise interning off-heap trades a use-after-free for an unbounded \
         leak (#7246)"
    );
    unsafe {
        let rendered = crate::symbol::js_symbol_to_string(f64::from_bits(symbol_ptr_bits(keeper)));
        assert_string_bytes(rendered as *const crate::StringHeader, b"Symbol(keeper)");
    }

    crate::symbol::test_clear_fresh_symbol_descriptions();
    crate::symbol::test_clear_symbol_side_table_roots();
}
