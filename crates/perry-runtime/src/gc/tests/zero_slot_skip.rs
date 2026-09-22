//! The zero-slot skip (#10362): an object that provably yields no child slot is
//! marked and moved but never queued for a walk that would find nothing.
//!
//! Skipping a walk is skipping every edge that walk would have produced, so the
//! witnesses here are organised by EDGE SOURCE, not by fixture. Each term of
//! `gc_object_yields_no_child_slots` gets a case that fails without it, and the
//! full mark's extra `!proxy_trace_active` term — the one no ordinary GC
//! fixture can see — gets a real collection and a sabotaged twin.

use super::super::trace::zero_slot_skip_sabotage;
use super::super::*;
use super::support::*;

fn full_collect() {
    let trigger = GcTriggerSnapshot {
        kind: GcTriggerKind::Manual,
        steps_before: Some(GcStepSnapshot::current()),
    };
    let _ = GcCycleState::new_full(trigger).run_to_completion();
}

fn alloc_proxy_endpoint() -> (*mut u8, f64) {
    let ptr = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(ptr);
    }
    (ptr, f64::from_bits(ptr_bits(ptr as usize)))
}

/// A plain pointer-free array: the population the skip exists for.
unsafe fn pointer_free_array(length: u32) -> (*mut crate::array::ArrayHeader, *mut u64) {
    let (arr, elements) = alloc_old_test_array(length);
    layout_init_pointer_free(arr as *mut u8);
    (arr, elements)
}

unsafe fn header_of(user: usize) -> *mut GcHeader {
    header_from_user_ptr(user as *const u8) as *mut GcHeader
}

// ------------------------------------------------------------ the predicate --

/// The predicate keys on `GC_TYPE_ARRAY` for speed, which is only sound while
/// that type is the one whose rewrite arm has no uncovered sibling and whose
/// layout kind has no prefix or meta edge. Both are table facts, so both are
/// pinned here rather than argued in a comment.
#[test]
fn the_array_type_still_pairs_with_the_prefix_free_layout_kind() {
    assert_eq!(
        gc_type_rewrite_descriptor_kind(GC_TYPE_ARRAY),
        GcRewriteDescriptorKind::Array,
        "the skip assumes GC_TYPE_ARRAY takes the Array rewrite arm, whose only \
         siblings are named props and the residual prototype"
    );
    assert_eq!(
        gc_type_layout_slot_kind(GC_TYPE_ARRAY),
        GcLayoutSlotKind::ArrayElements,
        "the skip assumes GC_TYPE_ARRAY's layout kind yields no prefix or meta \
         child edge, which is what gc_child_slots builds for ArrayElements"
    );
}

#[test]
fn a_plain_pointer_free_array_is_admitted() {
    let _guard = GcTestIsolationGuard::new();
    unsafe {
        let (arr, _) = pointer_free_array(4);
        assert!(
            gc_object_yields_no_child_slots(header_of(arr as usize)),
            "a pointer-free array with no named props and no residual prototype \
             is exactly the population this skip is for"
        );
    }
}

#[test]
fn an_array_that_still_holds_pointers_is_refused() {
    let _guard = GcTestIsolationGuard::new();
    unsafe {
        let (arr, _) = alloc_old_test_array(4);
        assert!(
            !gc_object_yields_no_child_slots(header_of(arr as usize)),
            "without GC_LAYOUT_POINTER_FREE the payload may hold anything"
        );
    }
}

#[test]
fn named_properties_refuse_the_skip() {
    let _guard = GcTestIsolationGuard::new();
    unsafe {
        let (arr, _) = pointer_free_array(4);
        let header = header_of(arr as usize);
        assert!(gc_object_yields_no_child_slots(header), "premise");
        (*header)._reserved |= crate::gc::GC_ARRAY_NAMED_PROPS;
        assert!(
            !gc_object_yields_no_child_slots(header),
            "named-property reserve slots sit in front of element 0, outside \
             every layout range, so POINTER_FREE says nothing about them"
        );
    }
}

#[test]
fn a_residual_prototype_owner_refuses_the_skip() {
    let _guard = GcTestIsolationGuard::new();
    unsafe {
        let (arr, _) = pointer_free_array(4);
        let header = header_of(arr as usize);
        assert!(gc_object_yields_no_child_slots(header), "premise");
        (*header)._reserved |= crate::gc::GC_RESIDUAL_PROTO_OWNER;
        assert!(
            !gc_object_yields_no_child_slots(header),
            "an explicit Object.setPrototypeOf value is a child edge of its \
             owner whatever the payload holds (#10493)"
        );
    }
}

#[test]
fn a_forwarded_array_refuses_the_skip() {
    let _guard = GcTestIsolationGuard::new();
    unsafe {
        let (arr, _) = pointer_free_array(4);
        let header = header_of(arr as usize);
        assert!(gc_object_yields_no_child_slots(header), "premise");
        (*header).gc_flags |= GC_FLAG_FORWARDED;
        assert!(
            !gc_object_yields_no_child_slots(header),
            "array growth installs PERMANENT forwarding stubs and walking the \
             stub is what propagates liveness across the hop (#6228)"
        );
    }
}

/// The kind term, and the reason it is a term at all: `GC_LAYOUT_POINTER_FREE`
/// is NOT an array-only bit. A closure is allocated pointer-free
/// (`symbol/properties.rs`, #7154) and a typed object with an empty pointer mask
/// acquires it. Both carry child edges outside the payload, so admitting them
/// on the payload bit alone would drop those edges silently.
#[test]
fn a_pointer_free_non_array_is_refused_whatever_its_payload_says() {
    let _guard = GcTestIsolationGuard::new();
    unsafe {
        let (obj, _) = alloc_old_test_object(1);
        layout_init_pointer_free(obj as *mut u8);
        let obj_header = header_of(obj as usize);
        assert_eq!(
            (*obj_header)._reserved & GC_LAYOUT_STATE_MASK,
            GC_LAYOUT_POINTER_FREE,
            "premise: the object really is marked pointer-free"
        );
        assert!(
            !gc_object_yields_no_child_slots(obj_header),
            "an object carries the meta record edge, the shape keys edge and \
             its overflow fields, none of which the payload bit describes"
        );

        let (closure_ptr, _) = alloc_proxy_endpoint();
        layout_init_pointer_free(closure_ptr);
        assert!(
            !gc_object_yields_no_child_slots(header_of(closure_ptr as usize)),
            "a closure is ALLOCATED pointer-free and still has dynamic property \
             values and a static prototype edge"
        );
    }
}

// --------------------------------------------- the full mark's proxy term ---

/// A live proxy reachable ONLY through a pointer-free array, which is itself
/// reached as a FIELD (so the mark takes `mark_field_into_worklist`, the skip
/// site, rather than the root path). Returns whether the proxy survived.
fn proxy_behind_a_pointer_free_array(sabotaged: bool) -> bool {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let (_target_ptr, target) = alloc_proxy_endpoint();
    let (_handler_ptr, handler) = alloc_proxy_endpoint();
    let proxy = crate::proxy::js_proxy_new(target, handler);

    let (arr, elements) = unsafe { pointer_free_array(1) };
    unsafe {
        *elements = proxy.to_bits();
        assert!(
            gc_object_yields_no_child_slots(header_of(arr as usize)),
            "premise: the carrier must be a skip candidate, or this proves nothing"
        );
    }
    // Reached as a FIELD, not as a root: the skip lives in
    // `mark_field_into_worklist`, and the root path does not go through it.
    let (holder, fields) = unsafe { alloc_old_test_array(1) };
    unsafe {
        *fields = ptr_bits(arr as usize);
        layout_init_all_pointer_slots(holder as *mut u8);
    }
    js_shadow_slot_set(0, ptr_bits(holder as usize));

    {
        let _sabotage = sabotaged.then(zero_slot_skip_sabotage::Guard::arm);
        full_collect();
    }
    let live = crate::proxy::test_proxy_slot_is_live(proxy);
    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
    live
}

#[test]
fn a_proxy_behind_a_pointer_free_array_survives_a_full_trace() {
    assert!(
        proxy_behind_a_pointer_free_array(false),
        "the full mark must still read every word of a pointer-free payload \
         while a proxy trace is active: a proxy id is a POINTER_TAG value in \
         the proxy-id band, not a heap pointer, which is why the layout mask \
         calls that payload pointer-free in the first place"
    );
}

#[test]
fn sabotaging_the_proxy_gate_strands_that_proxys_target() {
    assert!(
        !proxy_behind_a_pointer_free_array(true),
        "with the !proxy_trace_active term removed the array is skipped, the \
         registry entry is never observed, gc_finish_full_trace prunes it and \
         a LIVE proxy loses its target and handler. If this twin ever passes, \
         the term is unwitnessed."
    );
}
