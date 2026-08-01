//! #7154 — publishing a pointer into a GC-traced payload slot must RECORD it.
//!
//! Invariant under test:
//!
//! > Any store that publishes a pointer-bearing value into a slot the collector
//! > traces (an object field, an array element, a closure capture) must go
//! > through the barriered store helper, so the store both records the layout
//! > bit (`layout_note_slot`) and dirties the remembered-set page
//! > (`runtime_write_barrier_slot`). A raw `*slot = bits` breaks it silently.
//!
//! Why a raw store is fatal rather than merely conservative: every heap object
//! is allocated in `GC_LAYOUT_POINTER_FREE` and only leaves that state when a
//! store *records* a pointer. `heap_payload_slot_selection` short-circuits on
//! `POINTER_FREE` and skips the WHOLE payload without consulting any mask, so a
//! raw store leaves the child untraced: the evacuating young-generation minor
//! neither keeps it alive nor rewrites the slot when it moves. The next read of
//! that slot returns a pointer into reclaimed nursery memory — `TypeError:
//! value is not a function` at a call site with no relation to the store, or a
//! SIGSEGV inside the collector on a later cycle.
//!
//! Both cases below were live offenders found with `PERRY_GC_FROMSPACE_SCAN=1`
//! on a Perry-compiled zod workload (294 dangling closure-capture edges per
//! cycle, deterministic SIGSEGV; clean under `PERRY_GC_MOVING_LOOP_POLLS=0`).
//!
//! The first assertion in each test is the *deterministic* one: it interrogates
//! the collector's own child-slot enumerator directly, so it is red on the raw
//! store regardless of GC timing or conservative-stack pinning. The survival
//! assertions that follow are the end-to-end check and are gated on the cycle
//! having actually copied something.

use super::*;

extern "C" fn test_bound_method_body(_closure: *const crate::closure::ClosureHeader) -> f64 {
    0.0
}

/// Addresses of the slots the collector says it will visit inside `user_ptr`'s
/// payload. A `POINTER_FREE` payload yields an empty list — that is exactly the
/// failure mode this file guards.
unsafe fn enumerated_child_slots(user_ptr: usize) -> Vec<usize> {
    test_heap_child_slots_for_user(user_ptr as *mut u8)
        .into_iter()
        .filter_map(|slot| match slot {
            HeapChildSlot::Child(p, _) => Some(p as usize),
            HeapChildSlot::PointerFreeRange(_) => None,
        })
        .collect()
}

/// `js_object_set_method_by_name` (the ordered object-literal lowering used when
/// a `...spread` precedes a `this`-reading method) patches the closure's
/// reserved `this` capture slot with the receiver. That store publishes a
/// pointer into a traced payload slot.
#[test]
fn test_bound_this_capture_is_traced_after_method_bind_7154() {
    let _guard = CopyingNurseryTestGuard::new(1);

    let closure = crate::closure::js_closure_alloc(
        test_bound_method_body as *const u8,
        crate::closure::CAPTURES_THIS_FLAG | 1,
    );
    let receiver = crate::object::js_object_alloc(0, 1);
    let key = crate::string::js_string_from_bytes(b"m".as_ptr(), 1);

    unsafe {
        crate::symbol::js_object_set_method_by_name(
            f64::from_bits(ptr_bits(receiver as usize)),
            f64::from_bits(string_bits(key as usize)),
            f64::from_bits(ptr_bits(closure as usize)),
        );
    }

    // The `this` slot now holds the receiver. The collector must say so.
    let capture_slot =
        unsafe { crate::closure::closure_capture_slots_mut(closure) as usize };
    let enumerated = unsafe { enumerated_child_slots(closure as usize) };
    assert!(
        enumerated.contains(&capture_slot),
        "#7154: the bound `this` capture slot must be enumerated as a child \
         edge after `js_object_set_method_by_name`; the collector reported \
         {enumerated:?} (a raw slot store leaves the closure POINTER_FREE, so \
         the whole capture payload is skipped)"
    );

    // End to end: the closure is the ONLY root. The receiver has to survive a
    // copying minor through the capture edge, and the slot has to be rewritten
    // to the receiver's new address.
    js_shadow_slot_set(0, ptr_bits(closure as usize));
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(
        trace.copying_nursery.copied_objects > 0,
        "#7154 regression test requires a COPYING minor; a non-moving \
         collection cannot expose a missing rewrite (copied_objects=0)"
    );

    let moved_closure = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::closure::ClosureHeader;
    let capture_bits = crate::closure::js_closure_get_capture_bits(moved_closure, 0);
    let recovered = (capture_bits & POINTER_MASK) as usize;
    assert!(
        crate::arena::classify_heap_generation(recovered)
            != crate::arena::HeapGeneration::Unknown,
        "#7154: the bound receiver must still be a live heap object after a \
         copying minor (capture bits {capture_bits:#x})"
    );
    unsafe {
        let header = header_from_user_ptr(recovered as *const u8);
        assert_eq!(
            (*header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "#7154: the `this` capture slot still points at a FORWARDED \
             from-space copy — the slot was not rewritten"
        );
        assert_eq!(
            (*header).obj_type,
            GC_TYPE_OBJECT,
            "#7154: the `this` capture no longer names an object"
        );
    }
}

/// `js_weakmap_set` overwriting an EXISTING key publishes the new value into
/// field 1 (+40 from the user pointer — the offset #7154's diagnostic scan
/// reports) of an already-reachable entry object.
#[test]
fn test_weakmap_overwrite_value_is_traced_7154() {
    let _guard = CopyingNurseryTestGuard::new(2);

    let map = crate::weakref::js_weakmap_new();
    js_shadow_slot_set(0, ptr_bits(map as usize));
    let key = crate::object::js_object_alloc(0, 0);
    js_shadow_slot_set(1, ptr_bits(key as usize));

    let map_val = f64::from_bits(ptr_bits(map as usize));
    let key_val = f64::from_bits(ptr_bits(key as usize));

    // Insert, then OVERWRITE with a fresh young value: the second call takes the
    // existing-entry path, which is the one that used a raw store.
    crate::weakref::js_weakmap_set(
        map_val,
        key_val,
        f64::from_bits(ptr_bits(crate::object::js_object_alloc(0, 0) as usize)),
    );
    let map_val = f64::from_bits(ptr_bits((js_shadow_slot_get(0) & POINTER_MASK) as usize));
    let key_val = f64::from_bits(ptr_bits((js_shadow_slot_get(1) & POINTER_MASK) as usize));
    let second = crate::object::js_object_alloc(0, 1);
    crate::weakref::js_weakmap_set(map_val, key_val, f64::from_bits(ptr_bits(second as usize)));

    // Find the entry that now holds `second` and assert its value slot is a
    // child edge the collector will follow.
    let stored = crate::weakref::js_weakmap_get(map_val, key_val);
    assert_eq!(
        stored.to_bits() & POINTER_MASK,
        second as u64 & POINTER_MASK,
        "weakmap overwrite must be observable through `get`"
    );

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(
        trace.copying_nursery.copied_objects > 0,
        "#7154 regression test requires a COPYING minor (copied_objects=0)"
    );

    let map_val = f64::from_bits(ptr_bits((js_shadow_slot_get(0) & POINTER_MASK) as usize));
    let key_val = f64::from_bits(ptr_bits((js_shadow_slot_get(1) & POINTER_MASK) as usize));
    let after = crate::weakref::js_weakmap_get(map_val, key_val);
    let recovered = (after.to_bits() & POINTER_MASK) as usize;
    assert!(
        recovered != 0
            && crate::arena::classify_heap_generation(recovered)
                != crate::arena::HeapGeneration::Unknown,
        "#7154: the overwritten WeakMap value must survive a copying minor \
         (got {:#x})",
        after.to_bits()
    );
    unsafe {
        let header = header_from_user_ptr(recovered as *const u8);
        assert_eq!(
            (*header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "#7154: WeakMap value slot still points at a FORWARDED from-space \
             copy — the slot was not rewritten"
        );
    }
}

/// A freshly allocated closure's capture slots must read as a non-pointer
/// sentinel, not raw recycled arena bytes: anything that decodes payload words
/// (the conservative scan, `PERRY_GC_FROMSPACE_SCAN`, a later whole-range layout
/// rebuild) would otherwise follow garbage as a reference. Mirrors #7138's
/// HOLE-initialisation of unused array capacity.
#[test]
fn test_fresh_closure_capture_slots_are_initialized_7154() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let closure = crate::closure::js_closure_alloc(test_bound_method_body as *const u8, 3);
    unsafe {
        let slots = crate::closure::closure_capture_slots_mut(closure);
        for i in 0..3 {
            assert_eq!(
                *slots.add(i),
                crate::value::TAG_UNDEFINED,
                "#7154: fresh closure capture slot {i} must be undefined-initialized"
            );
        }
    }
}
