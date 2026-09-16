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
    crate::object::js_object_set_field(obj, 0, crate::value::JSValue::number(1.5));
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
/// #6893 shares the canonical descriptor by shape in `SHAPE_LAYOUTS`
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
            crate::object::object_keys_array(first),
            crate::object::object_keys_array(second),
            "same-shape objects must share one canonical keys array"
        );
    }

    let raw_mask = [0b01u64];
    for object in [first, second] {
        crate::object::js_object_set_field(object, 0, crate::value::JSValue::number(1.5));
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
    crate::object::js_object_set_field(src, 0, crate::value::JSValue::number(1.5));
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
         SHAPE_LAYOUTS descriptor is keyed by the immutable ShapeId, which the copy carries"
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
    crate::object::js_object_set_field(sibling, 0, crate::value::JSValue::number(7.5));
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
    crate::object::js_object_set_field(obj, 0, crate::value::JSValue::number(10.5));
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

/// #10362: a relocation carries `GC_OBJ_TYPED_LAYOUT_INTACT` in the `_reserved`
/// copy and no longer re-derives it. This pins the exact state that the old
/// re-derivation used to clear — an INTACT receiver whose shape's SHARED
/// descriptor has since been poisoned to `None` — across a real copying minor,
/// together with the address-keyed half the funnel must still move.
///
/// The two receivers are one fixture because they are one cycle:
///
/// * `poisoned` installs the shared descriptor first and keeps its bit. An
///   unmoved sibling in this state keeps its bit too (`shape_install_shared`
///   leaves "any still-INTACT siblings" to fall back), so clearing it on the
///   copy alone was never a correctness rule. What the collector owes the
///   object is that its pointer field survive: with no descriptor resolvable
///   the trace falls back to scanning every slot.
/// * `per_object` is the receiver whose different layout POISONED the shape,
///   so its canonical descriptor is a per-object record — the address-keyed
///   half `transfer_address_keyed_records` still has to move. Its pointer
///   mask must answer the same after the move; a funnel that skips the record
///   move fails exactly that assertion.
#[test]
fn test_poisoned_shape_intact_and_per_object_record_survive_a_copying_minor() {
    // Two rooted receivers, so two shadow slots: a slot index outside the
    // pushed frame is bounds-checked into a silent no-op (#7184), which would
    // leave the second receiver unrooted and every verdict below vacuous.
    let _guard = CopyingNurseryTestGuard::new(2);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let packed = b"x\0y\0";
    let keys = crate::object::js_build_class_keys_array(
        0x1036_20,
        2,
        packed.as_ptr(),
        packed.len() as u32,
    );

    // Slot 0 raw-f64, slot 1 a declared pointer: the shape's first descriptor.
    let poisoned = crate::object::js_object_alloc_class_inline_keys(0x1036_20, 0, 2, keys);
    let poisoned_child = crate::string::js_string_from_bytes(b"poisoned-child".as_ptr(), 14);
    crate::object::js_object_set_field(poisoned, 0, crate::value::JSValue::number(1.5));
    crate::object::js_object_set_field(
        poisoned,
        1,
        crate::value::JSValue::string_ptr(poisoned_child),
    );
    let raw_mask = [0b01u64];
    let pointer_mask = [0b10u64];
    js_gc_init_typed_shape_layout(
        poisoned as u64,
        2,
        raw_mask.as_ptr(),
        raw_mask.len() as u32,
        pointer_mask.as_ptr(),
        pointer_mask.len() as u32,
    );
    assert!(layout_typed_intact_for_user(poisoned as usize));
    assert!(layout_typed_raw_f64_slot_for_user(poisoned as usize, 0));

    // Same keys, DIFFERENT layout (slot 0 carries no raw-f64 proof): the
    // install poisons the shared entry and falls back to a per-object record.
    let per_object = crate::object::js_object_alloc_class_inline_keys(0x1036_20, 0, 2, keys);
    let per_object_child = crate::string::js_string_from_bytes(b"per-object-child".as_ptr(), 16);
    crate::object::js_object_set_field(per_object, 0, crate::value::JSValue::number(2.5));
    crate::object::js_object_set_field(
        per_object,
        1,
        crate::value::JSValue::string_ptr(per_object_child),
    );
    js_gc_init_typed_shape_layout(
        per_object as u64,
        2,
        std::ptr::null(),
        0,
        pointer_mask.as_ptr(),
        pointer_mask.len() as u32,
    );

    // The fixture must START in the state under test, or every verdict below
    // is vacuous: the shared descriptor is gone for the first receiver while
    // its intact bit stands, and the second receiver answers from a record.
    assert!(
        layout_typed_intact_for_user(poisoned as usize),
        "the poisoning install must not clear a sibling's intact bit"
    );
    assert!(
        !layout_typed_raw_f64_slot_for_user(poisoned as usize, 0),
        "fixture precondition: the shared descriptor must be poisoned, so no \
         descriptor is resolvable for the first receiver"
    );
    assert_eq!(
        test_layout_pointer_slot_count(poisoned as usize, 2),
        None,
        "fixture precondition: an unresolvable descriptor means the conservative scan"
    );
    assert_eq!(
        test_layout_pointer_slot_count(per_object as usize, 2),
        Some(1),
        "fixture precondition: the second receiver's layout is a per-object record"
    );

    js_shadow_slot_set(0, ptr_bits(poisoned as usize));
    js_shadow_slot_set(1, ptr_bits(per_object as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);

    let poisoned_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let per_object_after = (js_shadow_slot_get(1) & POINTER_MASK) as usize;
    assert_ne!(
        poisoned_after, poisoned as usize,
        "the minor must actually relocate the first receiver — an inert arm proves nothing"
    );
    assert_ne!(
        per_object_after, per_object as usize,
        "the minor must actually relocate the second receiver"
    );

    // The bit rides `_reserved`. Before #10362 the funnel re-probed
    // `SHAPE_LAYOUTS` here, found the poisoned `None`, and cleared it on the
    // copy — a downgrade no unmoved sibling ever received.
    assert!(
        layout_typed_intact_for_user(poisoned_after),
        "the relocated receiver keeps the intact bit its `_reserved` copy carried"
    );
    assert!(
        !layout_typed_raw_f64_slot_for_user(poisoned_after, 0),
        "and still resolves no descriptor, exactly as before the move"
    );
    assert_eq!(
        test_layout_pointer_slot_count(poisoned_after, 2),
        None,
        "so the trace still falls back to scanning every slot"
    );

    // What the collector owes it: the field behind the unresolvable descriptor
    // is marked and rewritten.
    let moved_child =
        crate::object::js_object_get_field(poisoned_after as *const crate::object::ObjectHeader, 1);
    assert!(moved_child.is_string());
    let moved_child_ptr = moved_child.as_string_ptr();
    assert_ne!(
        moved_child_ptr as usize, poisoned_child as usize,
        "the child moved too, so the slot proves the rewrite, not just the mark"
    );
    unsafe {
        assert_string_bytes(moved_child_ptr, b"poisoned-child");
    }

    // The address-keyed half: the per-object record followed the move.
    assert_eq!(
        test_layout_pointer_slot_count(per_object_after, 2),
        Some(1),
        "the per-object layout record must be keyed by the post-move address"
    );
    assert!(layout_typed_intact_for_user(per_object_after));
    let moved_per_object_child = crate::object::js_object_get_field(
        per_object_after as *const crate::object::ObjectHeader,
        1,
    );
    assert!(moved_per_object_child.is_string());
    unsafe {
        assert_string_bytes(moved_per_object_child.as_string_ptr(), b"per-object-child");
    }
}

/// #10362: the relocation contract is asserted in test and debug builds, so a
/// future move path that allocates a destination without copying `_reserved`
/// fails here instead of silently losing a layout state, an `ALL_POINTERS` bit
/// or an element-shape proof at the first collection.
#[test]
#[should_panic(expected = "the caller must copy `_reserved`")]
fn test_layout_transfer_requires_the_relocation_header_copy() {
    let src = crate::array::js_array_alloc_pointer_elements(2);
    let dst = crate::array::js_array_alloc(2);
    unsafe {
        layout_transfer(src as *mut u8, dst as *mut u8);
    }
}

#[test]
fn test_typed_shape_raw_numeric_slots_accept_pointer_like_f64_bits() {
    clear_marks();
    clear_mark_seeds();

    let obj = crate::object::js_object_alloc(0, 2);
    let pointer_like_number = f64::from_bits(0x1000);
    crate::object::js_object_set_field(obj, 0, crate::value::JSValue::number(pointer_like_number));
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
    crate::object::js_object_set_field(
        obj,
        0,
        crate::value::JSValue::number(next_pointer_like_number),
    );
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
    crate::object::js_object_set_field(obj, 0, crate::value::JSValue::number(1.5));
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
    crate::object::js_object_set_field(handle_obj, 0, crate::value::JSValue::number(2.5));
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
        assert_eq!(crate::object::object_live_slot_count(obj), 2);
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
        model_relocation_header_copy(src as usize, dst as usize);
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
        // `GC_LAYOUT_ALL_POINTERS` rides `_reserved`, so since #10362 the
        // header copy is what carries it and the funnel must leave it alone.
        model_relocation_header_copy(src as usize, dst as usize);
        layout_transfer(src as *mut u8, dst as *mut u8);
    }

    assert_eq!(test_layout_pointer_slot_count(dst as usize, 2), Some(2));

    clear_marks();
    clear_mark_seeds();
}
