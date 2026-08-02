use super::*;

#[test]
fn test_typed_shape_descriptor_preserves_pointer_slots_after_non_pointer_overwrite() {
    clear_marks();
    clear_mark_seeds();

    let obj = crate::object::js_object_alloc(0, 2);
    let mask = [0b10u64];
    js_gc_init_typed_shape_layout(
        obj as u64,
        2,
        std::ptr::null(),
        0,
        mask.as_ptr(),
        mask.len() as u32,
    );

    assert_eq!(test_layout_pointer_slot_count(obj as usize, 2), Some(1));
    assert_eq!(test_heap_child_slot_count(obj as *mut u8), 1);

    crate::object::js_object_set_field(obj, 1, crate::value::JSValue::number(7.0));

    assert_eq!(test_layout_pointer_slot_count(obj as usize, 2), Some(1));
    assert_eq!(test_heap_child_slot_count(obj as *mut u8), 1);

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_typed_shape_descriptor_pointer_write_to_non_pointer_slot_falls_back() {
    clear_marks();
    clear_mark_seeds();

    let child = crate::string::js_string_from_bytes(b"typed-child".as_ptr(), 11);
    let child_header = unsafe { header_from_user_ptr(child as *mut u8) };
    let obj = crate::object::js_object_alloc(0, 2);
    let mask = [0b10u64];
    js_gc_init_typed_shape_layout(
        obj as u64,
        2,
        std::ptr::null(),
        0,
        mask.as_ptr(),
        mask.len() as u32,
    );

    crate::object::js_object_set_field(obj, 0, crate::value::JSValue::string_ptr(child));

    assert_eq!(test_layout_pointer_slot_count(obj as usize, 2), None);
    assert_eq!(test_heap_child_slot_count(obj as *mut u8), 2);

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (obj as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    trace_marked_objects(&valid_ptrs);
    unsafe {
        assert_ne!(
            (*child_header).gc_flags & GC_FLAG_MARKED,
            0,
            "fallback all-field tracing should mark a pointer written to a numeric slot"
        );
    }

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_typed_shape_descriptor_tracks_raw_numeric_slots() {
    clear_marks();
    clear_mark_seeds();

    let obj = crate::object::js_object_alloc(0, 2);
    crate::object::js_object_set_unboxed_f64_field(obj, 0, 1.5);
    crate::object::js_object_set_field(obj, 1, crate::value::JSValue::number(2.5));
    let raw_mask = [0b01u64];
    js_gc_init_typed_shape_layout(
        obj as u64,
        2,
        raw_mask.as_ptr(),
        raw_mask.len() as u32,
        std::ptr::null(),
        0,
    );

    assert!(layout_typed_raw_f64_slot_for_user(obj as usize, 0));
    assert!(!layout_typed_raw_f64_slot_for_user(obj as usize, 1));
    assert_eq!(test_layout_pointer_slot_count(obj as usize, 2), Some(0));

    let child = crate::string::js_string_from_bytes(b"raw-child".as_ptr(), 9);
    let child_header = unsafe { header_from_user_ptr(child as *mut u8) };
    crate::object::js_object_set_field(obj, 0, crate::value::JSValue::string_ptr(child));

    assert!(
        !layout_typed_raw_f64_slot_for_user(obj as usize, 0),
        "non-number writes must clear the exact raw-f64 descriptor"
    );
    assert_eq!(test_layout_pointer_slot_count(obj as usize, 2), None);

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (obj as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    trace_marked_objects(&valid_ptrs);
    unsafe {
        assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
    }

    clear_marks();
    clear_mark_seeds();
}

/// #6957 regression guard: the typed descriptor of a **shape-keyed** object must
/// be visible to the layout query helpers.
///
/// #6893 keys the canonical descriptor by the shared `keys_array` (`SHAPE_LAYOUTS`)
/// and deletes the per-object `TYPED_LAYOUTS` entry — so every class instance
/// (the only objects that carry a keys_array) moved to the shared map. Every
/// other test in this file allocates with `js_object_alloc` (class 0, no
/// keys_array), which still takes the per-object path; that is precisely why the
/// query helpers could go blind on real class instances with the whole layout
/// suite green.
#[test]
fn test_typed_shape_descriptor_visible_for_shape_keyed_objects() {
    clear_marks();
    clear_mark_seeds();

    let packed = b"x\0y\0";
    let keys = crate::object::js_build_class_keys_array(
        0x6957_01,
        2,
        packed.as_ptr(),
        packed.len() as u32,
    );
    let first = crate::object::js_object_alloc_class_inline_keys(0x6957_01, 0, 2, keys);
    let second = crate::object::js_object_alloc_class_inline_keys(0x6957_01, 0, 2, keys);
    unsafe {
        assert_eq!(
            (*first).keys_array,
            (*second).keys_array,
            "same-shape objects must share one canonical keys array"
        );
    }

    let raw_mask = [0b01u64];
    for object in [first, second] {
        crate::object::js_object_set_unboxed_f64_field(object, 0, 1.5);
        crate::object::js_object_set_field(object, 1, crate::value::JSValue::number(2.5));
        js_gc_init_typed_shape_layout(
            object as u64,
            2,
            raw_mask.as_ptr(),
            raw_mask.len() as u32,
            std::ptr::null(),
            0,
        );
    }

    for object in [first, second] {
        let user = object as usize;
        assert!(
            layout_typed_intact_for_user(user),
            "the shared shape install must set the intact bit"
        );
        assert!(
            layout_typed_raw_f64_slot_for_user(user, 0),
            "slot 0 is raw-f64 in the shape descriptor"
        );
        assert!(!layout_typed_raw_f64_slot_for_user(user, 1));
        assert!(
            layout_slot_is_raw_f64_typed(user, 0),
            "the store fast path must agree with layout_note_slot's own resolution"
        );
        assert!(
            layout_typed_accepts_finite_number_slot_for_user(user, 1),
            "an ordinary JSValue slot of an intact descriptor accepts finite numbers"
        );
    }

    // A contradicting store downgrades ONLY the object that made it. The shared
    // entry cannot be removed (it still describes every sibling), so the intact
    // bit is what separates the two — assert both halves.
    let payload = crate::string::js_string_from_bytes(b"boxed".as_ptr(), 5);
    crate::object::js_object_set_field(first, 0, crate::value::JSValue::string_ptr(payload));

    assert!(
        !layout_typed_raw_f64_slot_for_user(first as usize, 0),
        "a boxed store into a raw-f64 slot must evict this object's descriptor"
    );
    assert!(!layout_slot_is_raw_f64_typed(first as usize, 0));
    assert!(
        !layout_typed_accepts_finite_number_slot_for_user(first as usize, 0),
        "a downgraded object must not keep reading its shape's stale descriptor"
    );
    assert!(
        layout_typed_raw_f64_slot_for_user(second as usize, 0),
        "the sibling never diverged and must keep the shared shape descriptor"
    );
    assert!(layout_slot_is_raw_f64_typed(second as usize, 0));

    clear_marks();
    clear_mark_seeds();
}

/// #6964: `layout_transfer` resolved the moved object's typed descriptor only
/// through the per-object `TYPED_LAYOUTS` map. #6893 moved the canonical
/// descriptor of every object carrying a `keys_array` (i.e. every class
/// instance) into the shape-keyed `SHAPE_LAYOUTS` map and DELETED the per-object
/// entry, so that lookup missed and the relocated copy had a still-valid
/// `GC_OBJ_TYPED_LAYOUT_INTACT` bit cleared.
///
/// Deliberately a *shape-keyed* object: every pre-existing `layout_transfer`
/// test allocates with `js_object_alloc` (class 0, no keys_array), which keeps
/// its per-object entry and therefore takes the surviving path. That gap is why
/// #6893 merged green.
#[test]
fn test_shape_keyed_typed_layout_survives_layout_transfer() {
    clear_marks();
    clear_mark_seeds();

    let packed = b"x\0y\0";
    let keys = crate::object::js_build_class_keys_array(
        0x6964_01,
        2,
        packed.as_ptr(),
        packed.len() as u32,
    );
    let src = crate::object::js_object_alloc_class_inline_keys(0x6964_01, 0, 2, keys);
    crate::object::js_object_set_unboxed_f64_field(src, 0, 1.5);
    crate::object::js_object_set_field(src, 1, crate::value::JSValue::number(2.5));
    let raw_mask = [0b01u64];
    js_gc_init_typed_shape_layout(
        src as u64,
        2,
        raw_mask.as_ptr(),
        raw_mask.len() as u32,
        std::ptr::null(),
        0,
    );
    assert!(layout_typed_intact_for_user(src as usize));
    assert!(layout_typed_raw_f64_slot_for_user(src as usize, 0));

    // Model an evacuation copy the way every caller performs it: a destination
    // of the same shape, payload copied verbatim, `_reserved` propagated, then
    // `layout_transfer`.
    let dst = crate::object::js_object_alloc_class_inline_keys(0x6964_01, 0, 2, keys);
    unsafe {
        let header_size = std::mem::size_of::<crate::object::ObjectHeader>();
        std::ptr::copy_nonoverlapping(
            src as *const u8,
            dst as *mut u8,
            header_size + 2 * std::mem::size_of::<crate::value::JSValue>(),
        );
        let src_header = header_from_user_ptr(src as *const u8);
        let dst_header = header_from_user_ptr(dst as *const u8);
        (*(dst_header as *mut GcHeader))._reserved = (*src_header)._reserved;
        layout_transfer(src as *mut u8, dst as *mut u8);
    }

    assert!(
        layout_typed_intact_for_user(dst as usize),
        "a relocated shape-keyed object must keep GC_OBJ_TYPED_LAYOUT_INTACT — its \
         SHAPE_LAYOUTS descriptor is keyed by the shared keys_array, which the copy carries"
    );
    assert!(
        layout_typed_raw_f64_slot_for_user(dst as usize, 0),
        "slot 0 is still raw-f64 after relocation"
    );
    assert!(!layout_typed_raw_f64_slot_for_user(dst as usize, 1));
    assert!(
        layout_slot_is_raw_f64_typed(dst as usize, 0),
        "the store fast path must agree with the descriptor after relocation"
    );

    // The source is downgraded on transfer (it is dead / a forwarding stub), and
    // that must NOT take the shared entry with it: an untouched sibling still
    // reads the shape descriptor.
    let sibling = crate::object::js_object_alloc_class_inline_keys(0x6964_01, 0, 2, keys);
    crate::object::js_object_set_unboxed_f64_field(sibling, 0, 7.5);
    crate::object::js_object_set_field(sibling, 1, crate::value::JSValue::number(8.5));
    js_gc_init_typed_shape_layout(
        sibling as u64,
        2,
        raw_mask.as_ptr(),
        raw_mask.len() as u32,
        std::ptr::null(),
        0,
    );
    assert!(layout_typed_raw_f64_slot_for_user(sibling as usize, 0));

    clear_marks();
    clear_mark_seeds();
}

/// #6964, but driven through the real evacuation path (`gc/copying.rs`'s
/// `layout_transfer` call site) instead of calling the helper directly.
#[test]
fn test_shape_keyed_typed_layout_survives_copying_minor() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let packed = b"x\0y\0";
    let keys = crate::object::js_build_class_keys_array(
        0x6964_02,
        2,
        packed.as_ptr(),
        packed.len() as u32,
    );
    let obj = crate::object::js_object_alloc_class_inline_keys(0x6964_02, 0, 2, keys);
    crate::object::js_object_set_unboxed_f64_field(obj, 0, 10.5);
    crate::object::js_object_set_field(obj, 1, crate::value::JSValue::number(-3.25));
    let raw_mask = [0b01u64];
    js_gc_init_typed_shape_layout(
        obj as u64,
        2,
        raw_mask.as_ptr(),
        raw_mask.len() as u32,
        std::ptr::null(),
        0,
    );
    assert!(layout_typed_intact_for_user(obj as usize));
    assert!(layout_typed_raw_f64_slot_for_user(obj as usize, 0));
    js_shadow_slot_set(0, ptr_bits(obj as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);

    let after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(
        after, obj as usize,
        "the copying minor must actually relocate the instance — an inert arm proves nothing"
    );

    let fields = unsafe {
        (after as *const u8).add(std::mem::size_of::<crate::object::ObjectHeader>()) as *const u64
    };
    assert_eq!(f64::from_bits(unsafe { *fields.add(0) }), 10.5);

    assert!(
        layout_typed_intact_for_user(after),
        "#6964: the relocated class instance must keep its shape-keyed typed layout"
    );
    assert!(
        layout_typed_raw_f64_slot_for_user(after, 0),
        "#6964: the shape descriptor still describes slot 0 as raw-f64 after relocation"
    );
    assert!(layout_slot_is_raw_f64_typed(after, 0));
}

#[test]
fn test_typed_shape_raw_numeric_slots_accept_pointer_like_f64_bits() {
    clear_marks();
    clear_mark_seeds();

    let obj = crate::object::js_object_alloc(0, 2);
    let pointer_like_number = f64::from_bits(0x1000);
    crate::object::js_object_set_unboxed_f64_field(obj, 0, pointer_like_number);
    let child = crate::string::js_string_from_bytes(b"mixed-child".as_ptr(), 11);
    crate::object::js_object_set_field(obj, 1, crate::value::JSValue::string_ptr(child));

    let raw_mask = [0b01u64];
    let pointer_mask = [0b10u64];
    js_gc_init_typed_shape_layout(
        obj as u64,
        2,
        raw_mask.as_ptr(),
        raw_mask.len() as u32,
        pointer_mask.as_ptr(),
        pointer_mask.len() as u32,
    );

    assert!(layout_typed_raw_f64_slot_for_user(obj as usize, 0));
    assert_eq!(test_layout_pointer_slot_count(obj as usize, 2), Some(1));

    let next_pointer_like_number = f64::from_bits(0x2000);
    crate::object::js_object_set_unboxed_f64_field(obj, 0, next_pointer_like_number);
    assert!(
        layout_typed_raw_f64_slot_for_user(obj as usize, 0),
        "raw f64 slots must not be downgraded by numeric payload bits that resemble raw pointers"
    );
    assert_eq!(test_layout_pointer_slot_count(obj as usize, 2), Some(1));

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_typed_shape_descriptor_rejects_nanbox_non_number_tags() {
    clear_marks();
    clear_mark_seeds();

    let raw_mask = [0b1u64];
    let obj = crate::object::js_object_alloc(0, 1);
    crate::object::js_object_set_unboxed_f64_field(obj, 0, 1.5);
    js_gc_init_typed_shape_layout(
        obj as u64,
        1,
        raw_mask.as_ptr(),
        raw_mask.len() as u32,
        std::ptr::null(),
        0,
    );
    assert!(layout_typed_raw_f64_slot_for_user(obj as usize, 0));

    let short = crate::value::JSValue::try_short_string(b"abc").unwrap();
    crate::object::js_object_set_field(obj, 0, short);
    assert!(
        !layout_typed_raw_f64_slot_for_user(obj as usize, 0),
        "SSO string tags must downgrade raw-f64 descriptors"
    );
    assert_eq!(test_layout_pointer_slot_count(obj as usize, 1), None);

    let handle_obj = crate::object::js_object_alloc(0, 1);
    crate::object::js_object_set_unboxed_f64_field(handle_obj, 0, 2.5);
    js_gc_init_typed_shape_layout(
        handle_obj as u64,
        1,
        raw_mask.as_ptr(),
        raw_mask.len() as u32,
        std::ptr::null(),
        0,
    );
    assert!(layout_typed_raw_f64_slot_for_user(handle_obj as usize, 0));

    let handle = crate::value::JSValue::from_bits(crate::value::JS_HANDLE_TAG | 0x1234);
    crate::object::js_object_set_field(handle_obj, 0, handle);
    assert!(
        !layout_typed_raw_f64_slot_for_user(handle_obj as usize, 0),
        "JS handle tags must downgrade raw-f64 descriptors"
    );
    assert_eq!(test_layout_pointer_slot_count(handle_obj as usize, 1), None);

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_typed_shape_descriptor_growing_new_field_falls_back() {
    clear_marks();
    clear_mark_seeds();

    let packed_keys = b"stable\0";
    let keys = crate::object::js_build_class_keys_array(
        65_001,
        1,
        packed_keys.as_ptr(),
        packed_keys.len() as u32,
    );
    let obj = crate::object::js_object_alloc_class_inline_keys(65_001, 0, 1, keys);
    js_gc_init_typed_shape_layout(obj as u64, 1, std::ptr::null(), 0, std::ptr::null(), 0);

    let extra_key = crate::string::js_string_from_bytes(b"extra".as_ptr(), 5);
    crate::object::js_object_set_field_by_name(obj, extra_key, 42.0);

    unsafe {
        assert_eq!((*obj).field_count, 2);
    }
    assert_eq!(test_layout_pointer_slot_count(obj as usize, 2), None);

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_typed_shape_descriptor_transfers_on_object_move() {
    clear_marks();
    clear_mark_seeds();

    let src = crate::object::js_object_alloc(0, 2);
    let dst = crate::object::js_object_alloc(0, 2);
    let mask = [0b10u64];
    js_gc_init_typed_shape_layout(
        src as u64,
        2,
        std::ptr::null(),
        0,
        mask.as_ptr(),
        mask.len() as u32,
    );

    unsafe {
        layout_transfer(src as *mut u8, dst as *mut u8);
    }

    assert_eq!(test_layout_pointer_slot_count(dst as usize, 2), Some(1));
    crate::object::js_object_set_field(dst, 1, crate::value::JSValue::number(9.0));
    assert_eq!(test_layout_pointer_slot_count(dst as usize, 2), Some(1));

    let child = crate::string::js_string_from_bytes(b"moved-child".as_ptr(), 11);
    crate::object::js_object_set_field(dst, 0, crate::value::JSValue::string_ptr(child));
    assert_eq!(test_layout_pointer_slot_count(dst as usize, 2), None);

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_all_pointer_layout_transfers_on_array_move() {
    clear_marks();
    clear_mark_seeds();

    let src = crate::array::js_array_alloc_pointer_elements(2);
    let dst = crate::array::js_array_alloc(2);
    unsafe {
        layout_transfer(src as *mut u8, dst as *mut u8);
    }

    assert_eq!(test_layout_pointer_slot_count(dst as usize, 2), Some(2));

    clear_marks();
    clear_mark_seeds();
}
