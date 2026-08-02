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
    // Slot 0 closure, slot 1 receiver, slot 2 key. `js_object_set_method_by_name`
    // allocates (it interns the key, may clone the keys array and may grow the
    // object), so it is a collection point: nothing may be carried across it in a
    // raw Rust local. Rust stack locals are not roots and do not pin, so every
    // address is parked in a shadow slot and re-read afterwards — otherwise a
    // minor landing inside the call would fail this test for a reason unrelated
    // to the fix. Same discipline as the WeakMap test below.
    let _guard = CopyingNurseryTestGuard::new(3);

    js_shadow_slot_set(
        0,
        ptr_bits(crate::closure::js_closure_alloc(
            test_bound_method_body as *const u8,
            crate::closure::CAPTURES_THIS_FLAG | 1,
        ) as usize),
    );
    js_shadow_slot_set(1, ptr_bits(crate::object::js_object_alloc(0, 1) as usize));
    js_shadow_slot_set(
        2,
        string_bits(crate::string::js_string_from_bytes(b"m".as_ptr(), 1) as usize),
    );

    unsafe {
        // Every argument is read from its slot at the call, so the last
        // allocation before the call cannot leave a sibling argument stale.
        crate::symbol::js_object_set_method_by_name(
            f64::from_bits(ptr_bits((js_shadow_slot_get(1) & POINTER_MASK) as usize)),
            f64::from_bits(string_bits((js_shadow_slot_get(2) & POINTER_MASK) as usize)),
            f64::from_bits(ptr_bits((js_shadow_slot_get(0) & POINTER_MASK) as usize)),
        );
    }

    // The `this` slot now holds the receiver. The collector must say so.
    // Re-derive the closure — the call above may have moved it.
    let closure = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::closure::ClosureHeader;
    let capture_slot = unsafe { crate::closure::closure_capture_slots_mut(closure) as usize };
    let enumerated = unsafe { enumerated_child_slots(closure as usize) };
    assert!(
        enumerated.contains(&capture_slot),
        "#7154: the bound `this` capture slot must be enumerated as a child \
         edge after `js_object_set_method_by_name`; the collector reported \
         {enumerated:?} (a raw slot store leaves the closure POINTER_FREE, so \
         the whole capture payload is skipped)"
    );

    // End to end: the closure (slot 0) becomes the ONLY root — drop the direct
    // roots on the receiver and the key so the receiver is reachable exclusively
    // through the capture edge under test. It has to survive a copying minor
    // through that edge, and the slot has to be rewritten to its new address.
    js_shadow_slot_set(1, crate::value::TAG_UNDEFINED);
    js_shadow_slot_set(2, crate::value::TAG_UNDEFINED);
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(
        trace.copying_nursery.copied_objects > 0,
        "#7154 regression test requires a COPYING minor; a non-moving \
         collection cannot expose a missing rewrite (copied_objects=0)"
    );

    let moved_closure =
        (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::closure::ClosureHeader;
    let capture_bits = crate::closure::js_closure_get_capture_bits(moved_closure, 0);
    let recovered = (capture_bits & POINTER_MASK) as usize;
    assert!(
        crate::arena::classify_heap_generation(recovered) != crate::arena::HeapGeneration::Unknown,
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
///
/// The interesting case is the OLD entry: once the entry has been promoted, an
/// overwrite creates an old→young edge, and only the write barrier puts that
/// edge in the remembered set. A raw store leaves the minor blind to it.
#[test]
fn test_weakmap_overwrite_value_is_traced_7154() {
    let _guard = CopyingNurseryTestGuard::new(3);

    js_shadow_slot_set(0, ptr_bits(crate::weakref::js_weakmap_new() as usize));
    js_shadow_slot_set(1, ptr_bits(crate::object::js_object_alloc(0, 0) as usize));

    let live =
        |slot: u32| f64::from_bits(ptr_bits((js_shadow_slot_get(slot) & POINTER_MASK) as usize));

    // Insert, then age the map/key/entry graph until the entry is tenured.
    crate::weakref::js_weakmap_set(
        live(0),
        live(1),
        f64::from_bits(ptr_bits(crate::object::js_object_alloc(0, 0) as usize)),
    );
    for _ in 0..6 {
        let _ = gc_collect_minor();
    }
    let entry_addr = weak_entry_addr_for(live(0), live(1));
    assert!(
        crate::arena::pointer_in_old_gen(entry_addr),
        "#7154 test setup: the WeakMap entry must be promoted to old-gen so the \
         overwrite creates an old->young edge (entry at {entry_addr:#x})"
    );

    // OVERWRITE with a fresh young value — the existing-entry path, which is
    // the one that used a raw store. The value is allocated inline so no named
    // local holds its address across the collection below.
    crate::weakref::js_weakmap_set(
        live(0),
        live(1),
        f64::from_bits(ptr_bits(crate::object::js_object_alloc(0, 1) as usize)),
    );

    let before = crate::weakref::js_weakmap_get(live(0), live(1)).to_bits() & POINTER_MASK;
    assert!(
        before != 0,
        "weakmap overwrite must be observable through `get`"
    );
    assert!(
        crate::arena::pointer_in_nursery(before as usize),
        "#7154 test setup: the overwritten value must be YOUNG"
    );

    // Rooted young canary: keeps `copied_objects > 0` true independently of the
    // subject, so the liveness gate below cannot be satisfied (or dissatisfied)
    // by the very edge under test.
    js_shadow_slot_set(2, ptr_bits(crate::object::js_object_alloc(0, 0) as usize));
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(
        trace.copying_nursery.copied_objects > 0,
        "#7154 regression test requires a COPYING minor (copied_objects=0)"
    );

    let after = crate::weakref::js_weakmap_get(live(0), live(1));
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
        assert_eq!(
            (*header).obj_type,
            GC_TYPE_OBJECT,
            "#7154: the WeakMap value slot no longer names an object"
        );
    }
}

/// Address of the entry object a WeakMap holds for `key`, found by walking the
/// map's entries array the same way `js_weakmap_get` does.
fn weak_entry_addr_for(map: f64, key: f64) -> usize {
    let map_ptr = (map.to_bits() & POINTER_MASK) as *mut crate::ObjectHeader;
    unsafe {
        let entries = crate::object::js_object_get_field(map_ptr, 0);
        let entries_ptr = (entries.bits() & POINTER_MASK) as *mut crate::array::ArrayHeader;
        let len = crate::array::js_array_length(entries_ptr) as usize;
        for i in 0..len {
            let entry_val = crate::array::js_array_get(entries_ptr, i as u32);
            let entry = (entry_val.bits() & POINTER_MASK) as *mut crate::ObjectHeader;
            if entry.is_null() {
                continue;
            }
            let stored_key = crate::object::js_object_get_field(entry, 0);
            if stored_key.bits() == key.to_bits() {
                return entry as usize;
            }
        }
        0
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
