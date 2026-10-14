//! The zero-slot skip (#10362): an object that provably yields no child slot is
//! marked and moved but never queued for a walk that would find nothing.
//!
//! Skipping a walk is skipping every edge that walk would have produced, so the
//! witnesses here are organised by EDGE SOURCE, not by fixture. Each term of
//! `gc_object_yields_no_child_slots` gets a case that fails without it, and the
//! native-provider gate remains separate; proxy cells use ordinary heap edges.

use super::super::*;
use super::support::*;

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
        (*header)._reserved |= crate::gc::GC_ARRAY_KEY_ATTRS;
        assert!(
            !gc_object_yields_no_child_slots(header),
            "named-property reserve slots sit in front of element 0, outside \
             every layout range, so POINTER_FREE says nothing about them"
        );
    }
}

#[test]
fn an_owned_prototype_bag_refuses_the_skip() {
    let _guard = GcTestIsolationGuard::new();
    unsafe {
        let (arr, _) = pointer_free_array(4);
        let header = header_of(arr as usize);
        assert!(gc_object_yields_no_child_slots(header), "premise");
        let _no_move = GcSuppressScope::new();
        crate::object::prototype_chain::object_set_static_prototype(
            arr as usize,
            crate::value::TAG_NULL,
        );
        let arr = crate::array::clean_arr_ptr(arr);
        let header = header_of(arr as usize);
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

/// The no-child fast path is array-only. Ordinary objects leave the header
/// layout bits clear and may still carry shape keys, meta and overflow edges.
/// Closures can be pointer-free in their capture payload but own other edges.
#[test]
fn a_non_array_is_refused_whatever_its_payload_says() {
    let _guard = GcTestIsolationGuard::new();
    unsafe {
        let (obj, _) = alloc_old_test_object(1);
        layout_init_pointer_free(obj as *mut u8);
        let obj_header = header_of(obj as usize);
        assert_eq!(
            (*obj_header)._reserved & GC_LAYOUT_STATE_MASK,
            GC_LAYOUT_UNKNOWN,
            "object allocation leaves header layout bits clear"
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
