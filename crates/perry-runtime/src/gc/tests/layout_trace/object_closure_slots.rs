use super::*;

#[test]
fn test_trace_object_uses_pointer_layout_mask() {
    clear_marks();
    clear_mark_seeds();

    let numeric = crate::object::js_object_alloc(0, 3);
    crate::object::js_object_set_field(numeric, 0, crate::value::JSValue::number(1.0));
    crate::object::js_object_set_field(numeric, 1, crate::value::JSValue::number(2.0));
    crate::object::js_object_set_field(numeric, 2, crate::value::JSValue::bool(false));
    assert_eq!(test_layout_pointer_slot_count(numeric as usize, 3), Some(0));
    assert_eq!(test_heap_child_slot_count(numeric as *mut u8), 0);

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (numeric as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    test_reset_trace_slot_reads();
    trace_marked_objects(&valid_ptrs);
    assert_eq!(test_trace_slot_reads(), 0);
    clear_marks();
    clear_mark_seeds();

    let child = crate::string::js_string_from_bytes(b"object-child".as_ptr(), 12);
    let child_header = unsafe { header_from_user_ptr(child as *mut u8) };
    let mixed = crate::object::js_object_alloc(0, 3);
    crate::object::js_object_set_field(mixed, 0, crate::value::JSValue::number(1.0));
    crate::object::js_object_set_field(mixed, 1, crate::value::JSValue::string_ptr(child));
    crate::object::js_object_set_field(mixed, 2, crate::value::JSValue::number(3.0));
    assert_eq!(test_layout_pointer_slot_count(mixed as usize, 3), Some(1));

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (mixed as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    test_reset_trace_slot_reads();
    trace_marked_objects(&valid_ptrs);
    assert_eq!(test_trace_slot_reads(), 1);
    unsafe {
        assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
    }

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_typed_shape_descriptor_scans_only_declared_pointer_slots() {
    clear_marks();
    clear_mark_seeds();

    let child = crate::string::js_string_from_bytes(b"typed-child".as_ptr(), 11);
    let child_header = unsafe { header_from_user_ptr(child as *mut u8) };
    let obj = crate::object::js_object_alloc(0, 3);
    crate::object::js_object_set_field(obj, 0, crate::value::JSValue::number(1.0));
    crate::object::js_object_set_field(obj, 1, crate::value::JSValue::string_ptr(child));
    crate::object::js_object_set_field(obj, 2, crate::value::JSValue::number(3.0));

    let mask = [1u64 << 1];
    js_gc_init_typed_shape_layout(
        obj as u64,
        3,
        std::ptr::null(),
        0,
        mask.as_ptr(),
        mask.len() as u32,
    );

    assert_eq!(test_layout_pointer_slot_count(obj as usize, 3), Some(1));
    assert_eq!(test_heap_child_slot_count(obj as *mut u8), 1);

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (obj as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    test_reset_trace_slot_reads();
    trace_marked_objects(&valid_ptrs);
    assert_eq!(test_trace_slot_reads(), 1);
    unsafe {
        assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
    }

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_typed_shape_descriptor_dynamic_pointer_mutation_falls_back_to_unknown_layout() {
    clear_marks();
    clear_mark_seeds();

    let obj = crate::object::js_object_alloc(0, 2);
    crate::object::js_object_set_field(obj, 0, crate::value::JSValue::number(1.0));
    crate::object::js_object_set_field(obj, 1, crate::value::JSValue::number(2.0));
    js_gc_init_typed_shape_layout(obj as u64, 2, std::ptr::null(), 0, std::ptr::null(), 0);
    assert_eq!(test_layout_pointer_slot_count(obj as usize, 2), Some(0));

    let child = crate::string::js_string_from_bytes(b"fallback-child".as_ptr(), 14);
    let child_header = unsafe { header_from_user_ptr(child as *mut u8) };
    crate::object::js_object_set_field(obj, 0, crate::value::JSValue::string_ptr(child));

    assert_eq!(
        test_layout_pointer_slot_count(obj as usize, 2),
        None,
        "storing a pointer into a non-pointer typed slot must drop to safe full scanning"
    );

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (obj as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    test_reset_trace_slot_reads();
    trace_marked_objects(&valid_ptrs);
    assert_eq!(test_trace_slot_reads(), 2);
    unsafe {
        assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
    }

    clear_marks();
    clear_mark_seeds();
}

extern "C" fn layout_mask_test_closure(_closure: *const crate::closure::ClosureHeader) -> f64 {
    0.0
}

#[test]
fn test_trace_closure_uses_pointer_layout_mask() {
    clear_marks();
    clear_mark_seeds();

    let numeric = crate::closure::js_closure_alloc(layout_mask_test_closure as *const u8, 3);
    crate::closure::js_closure_set_capture_f64(numeric, 0, 1.0);
    crate::closure::js_closure_set_capture_f64(numeric, 1, 2.0);
    crate::closure::js_closure_set_capture_ptr(numeric, 2, 7);
    assert_eq!(test_layout_pointer_slot_count(numeric as usize, 3), Some(0));
    assert_eq!(test_heap_child_slot_count(numeric as *mut u8), 0);

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (numeric as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    test_reset_trace_slot_reads();
    trace_marked_objects(&valid_ptrs);
    assert_eq!(test_trace_slot_reads(), 0);
    clear_marks();
    clear_mark_seeds();

    let child = crate::string::js_string_from_bytes(b"closure-child".as_ptr(), 13) as *mut u8;
    let child_header = unsafe { header_from_user_ptr(child) };
    let mixed = crate::closure::js_closure_alloc(layout_mask_test_closure as *const u8, 3);
    crate::closure::js_closure_set_capture_f64(mixed, 0, 1.0);
    crate::closure::js_closure_set_capture_f64(
        mixed,
        1,
        f64::from_bits(STRING_TAG | (child as u64 & POINTER_MASK)),
    );
    crate::closure::js_closure_set_capture_ptr(mixed, 2, 7);
    assert_eq!(test_layout_pointer_slot_count(mixed as usize, 3), Some(1));

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (mixed as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    test_reset_trace_slot_reads();
    trace_marked_objects(&valid_ptrs);
    assert_eq!(test_trace_slot_reads(), 1);
    unsafe {
        assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
    }

    clear_marks();
    clear_mark_seeds();
}

// Repsel Phase 4b.2 — an INT32-boxed numeric value reaching a raw-f64-masked
// object slot through the runtime store choke point must be canonicalized to
// raw f64 instead of permanently poisoning the object's typed layout.
//
// `layout_note_slot` treats any non-raw-f64 bit pattern landing in a raw-f64
// slot as a representation change and calls `layout_set_typed_unknown`, which
// evicts the `TypedLayoutDescriptor` one-way, per object. INT32 boxes genuinely
// reach object fields from FFI / native modules (sqlite row columns, `v8`
// deserialization), so one FFI integer used to cost that object its typed fast
// path forever. Codegen's guarded class-field store already canonicalized
// inline behind its plain-finite check; `runtime_store_jsvalue_slot` wrote the
// bits verbatim.

/// Install a two-slot typed descriptor: slot 0 raw-f64, slot 1 pointer.
unsafe fn typed_two_slot_object() -> (*mut crate::object::ObjectHeader, *mut u64) {
    let (obj, fields) = alloc_old_test_object(2);
    *fields = 0.0f64.to_bits();
    *fields.add(1) = crate::value::TAG_UNDEFINED;
    let raw_mask = [0b01u64];
    let ptr_mask = [0b10u64];
    js_gc_init_typed_shape_layout(
        obj as u64,
        2,
        raw_mask.as_ptr(),
        raw_mask.len() as u32,
        ptr_mask.as_ptr(),
        ptr_mask.len() as u32,
    );
    assert!(
        layout_has_typed_descriptor(obj as usize),
        "test setup: the typed descriptor must install"
    );
    (obj, fields)
}

#[test]
fn test_int32_store_into_raw_f64_slot_keeps_typed_descriptor() {
    let _guard = GcTestIsolationGuard::new();
    let (obj, fields) = unsafe { typed_two_slot_object() };

    // An INT32-boxed 42, exactly as an FFI / native module hands one over.
    let int32_bits = crate::value::INT32_TAG | 42u64;
    runtime_store_jsvalue_slot(obj as usize, fields as usize, 0, int32_bits);

    assert!(
        layout_has_typed_descriptor(obj as usize),
        "an INT32-boxed integer stored into a raw-f64 slot must not evict the typed descriptor"
    );
    let stored = unsafe { std::ptr::read(fields as *const u64) };
    assert_eq!(
        stored,
        42.0f64.to_bits(),
        "the slot holds canonical raw f64 bits, not the INT32 box"
    );
    assert_eq!(
        f64::from_bits(stored),
        42.0,
        "and reads back byte-exact as the same number"
    );
}

#[test]
fn test_int32_store_into_pointer_slot_is_left_verbatim() {
    let _guard = GcTestIsolationGuard::new();
    let (obj, fields) = unsafe { typed_two_slot_object() };

    // Slot 1 is pointer-masked, not raw-f64-masked: there is no raw-f64
    // contract to uphold and the note is already a no-op there, so the stored
    // bits must survive untouched.
    let int32_bits = crate::value::INT32_TAG | 7u64;
    let slot1 = unsafe { fields.add(1) };
    runtime_store_jsvalue_slot(obj as usize, slot1 as usize, 1, int32_bits);

    assert!(
        layout_has_typed_descriptor(obj as usize),
        "a non-pointer value in a pointer-masked slot leaves the descriptor intact"
    );
    assert_eq!(
        unsafe { std::ptr::read(slot1 as *const u64) },
        int32_bits,
        "canonicalization is scoped to raw-f64-masked slots"
    );
}

#[test]
fn test_non_numeric_store_into_raw_f64_slot_still_evicts_descriptor() {
    let _guard = GcTestIsolationGuard::new();
    let (obj, fields) = unsafe { typed_two_slot_object() };

    // The negative control for the fix: a string IS a genuine representation
    // change for a raw-f64 slot and must still downgrade the object — the scan
    // skips raw-f64 slots, so a mask left claiming "number here" over a live
    // string pointer would strand it.
    let payload = crate::string::js_string_from_bytes(b"not-a-number".as_ptr(), 12);
    let payload_bits = STRING_TAG | (payload as u64 & POINTER_MASK);
    runtime_store_jsvalue_slot(obj as usize, fields as usize, 0, payload_bits);

    assert!(
        !layout_has_typed_descriptor(obj as usize),
        "a non-numeric store into a raw-f64 slot must still evict the typed descriptor"
    );
    assert_eq!(
        unsafe { std::ptr::read(fields as *const u64) },
        payload_bits,
        "and the stored value itself is untouched"
    );
}

#[test]
fn test_int32_store_without_typed_descriptor_is_left_verbatim() {
    let _guard = GcTestIsolationGuard::new();
    let (obj, fields) = unsafe { alloc_old_test_object(1) };
    unsafe {
        *fields = 0.0f64.to_bits();
    }
    assert!(
        !layout_has_typed_descriptor(obj as usize),
        "test setup: no descriptor installed"
    );

    let int32_bits = crate::value::INT32_TAG | 5u64;
    runtime_store_jsvalue_slot(obj as usize, fields as usize, 0, int32_bits);

    assert_eq!(
        unsafe { std::ptr::read(fields as *const u64) },
        int32_bits,
        "with no intact descriptor there is no raw-f64 contract to uphold — bits stay verbatim"
    );
}

/// #6921 — the `lower_new_impl` standalone-constructor exit returns a freshly
/// allocated class instance on which NO constructor has run, so the
/// `js_gc_init_typed_shape_layout` that exit now emits sees an object whose
/// every field is still `undefined`. That is the premise the fix rests on, so
/// pin it here rather than reasoning about it.
///
/// Three properties, in the order they matter:
///
/// 1. A fresh instance really is left at `GC_LAYOUT_POINTER_FREE` with no
///    descriptor — the one state in which the per-store `layout_note_slot`
///    call is load-bearing for GC correctness rather than a precision hint.
///    That is why an exit which skips the layout init blocks the note elision.
/// 2. A raw-f64 mask CANNOT be honoured over `undefined` fields, and
///    `init_typed_shape_layout` must downgrade to `GC_LAYOUT_UNKNOWN` — the
///    conservative state — never install a mask that fails to describe the
///    live words. This is what makes emitting the init on that exit SAFE.
/// 3. A pointer-only mask IS installed over `undefined` fields, so a later
///    pointer store into that slot is traced even though `layout_note_slot`
///    never ran. This is what makes emitting it USEFUL.
#[test]
fn typed_shape_layout_init_on_unconstructed_instance_is_conservative() {
    let _guard = GcTestIsolationGuard::new();
    // `GcTestIsolationGuard` takes the thread's mutable-root scanner registry,
    // which includes the runtime-handle scanner. Put it back BEFORE opening the
    // scope below, or the handles are decorative and every object here is an
    // unrooted raw pointer held across a GC-capable allocation — the same bug
    // class (#6655) this test exists to be free of.
    register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    clear_marks();
    clear_mark_seeds();

    // Every allocation below is a GC point, and evacuation MOVES arena objects,
    // so nothing may be held as a raw pointer across one. Each instance is
    // rooted in `scope` the moment it is created and re-read through
    // `handle_user` after any later allocation.
    fn handle_user(handle: &RuntimeHandle<'_>) -> usize {
        (handle.get_nanbox_u64() & POINTER_MASK) as usize
    }
    let layout_state = |user: usize| unsafe {
        (*header_from_user_ptr(user as *const u8))._reserved & GC_LAYOUT_STATE_MASK
    };
    let alloc_instance =
        || crate::object::js_object_alloc_class_inline_keys(0, 0, 2, std::ptr::null_mut()) as usize;

    // (1) The unconstructed instance, exactly as `js_object_alloc_class_*`
    // hands it to the standalone-ctor exit.
    let fresh = scope.root_nanbox_u64(ptr_bits(alloc_instance()));
    let fresh_user = handle_user(&fresh);
    assert_eq!(
        layout_state(fresh_user),
        GC_LAYOUT_POINTER_FREE,
        "a fresh class instance is POINTER_FREE — the collector scans zero \
         slots on it until something publishes a pointer bit"
    );
    assert!(
        !layout_has_typed_descriptor(fresh_user),
        "and carries no typed descriptor"
    );

    // (2) Raw-f64 mask over all-`undefined` fields must land in UNKNOWN.
    let raw_only = scope.root_nanbox_u64(ptr_bits(alloc_instance()));
    let raw_only_user = handle_user(&raw_only);
    let raw_mask = [0b01u64];
    js_gc_init_typed_shape_layout(
        raw_only_user as u64,
        2,
        raw_mask.as_ptr(),
        raw_mask.len() as u32,
        std::ptr::null(),
        0,
    );
    assert_eq!(
        layout_state(raw_only_user),
        GC_LAYOUT_UNKNOWN,
        "`undefined` is not raw-f64 bits, so the descriptor must be refused \
         and the object downgraded to the conservative state — never left \
         POINTER_FREE, never given a mask that misdescribes it"
    );
    assert!(
        !layout_has_typed_descriptor(raw_only_user),
        "a refused descriptor must not be installed"
    );

    // (3) Pointer-only mask over all-`undefined` fields IS installed, and the
    // slot is traced on a later store without any `layout_note_slot` call.
    let ptr_only = scope.root_nanbox_u64(ptr_bits(alloc_instance()));
    let ptr_only_user = handle_user(&ptr_only);
    let ptr_mask = [0b01u64];
    js_gc_init_typed_shape_layout(
        ptr_only_user as u64,
        2,
        std::ptr::null(),
        0,
        ptr_mask.as_ptr(),
        ptr_mask.len() as u32,
    );
    assert_eq!(
        layout_state(ptr_only_user),
        GC_LAYOUT_SIDE_MASK,
        "a pointer mask is compatible with `undefined` fields and is installed"
    );
    assert_eq!(
        test_layout_pointer_slot_count(ptr_only_user, 2),
        Some(1),
        "slot 0 is published as pointer-bearing"
    );

    // The child is reachable ONLY through that slot; write it with a raw store
    // so no `layout_note_slot` runs, then prove tracing still finds it.
    let child = scope
        .root_nanbox_u64(string_bits(
            crate::string::js_string_from_bytes(b"6921-child".as_ptr(), 10) as usize,
        ));
    // Allocating the child was a GC point: re-read the instance rather than
    // reusing `ptr_only_user`, which may now name from-space.
    let ptr_only_user = handle_user(&ptr_only);
    let fields = unsafe {
        (ptr_only_user as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>())
            as *mut u64
    };
    unsafe {
        std::ptr::write(fields, child.get_nanbox_u64());
    }

    // Force a real collection here, with the conservative native-stack scan
    // pinned OFF. That makes the `RuntimeHandleScope` above LOAD-BEARING rather
    // than decorative: the raw Rust locals are no longer a safety net, so the
    // scope is the only thing keeping these objects alive. Drop the scanner
    // registration at the top of this test and this collection reclaims them.
    {
        let _scan = ConservativeScanDisabledGuard::new();
        let _ = collect_minor_trace(GcTriggerKind::Direct);
    }
    // Re-read everything through the handles — a copied minor relocates
    // nursery objects and rewrites the rooted slots.
    let ptr_only_user = handle_user(&ptr_only);
    assert_eq!(
        test_layout_pointer_slot_count(ptr_only_user, 2),
        Some(1),
        "the typed descriptor must survive the collection (and any relocation) \
         — the note-elision premise depends on it staying intact, not just on \
         it being installed once"
    );

    // A collection may have left objects marked; start the hand-driven mark
    // from a known-clean state so the assertions below mean what they say.
    clear_marks();
    clear_mark_seeds();
    let valid_ptrs = build_valid_pointer_set();
    assert!(
        try_mark_value(ptr_bits(handle_user(&ptr_only)), &valid_ptrs),
        "test setup: the instance marks as a root"
    );
    trace_marked_objects(&valid_ptrs);
    unsafe {
        let child_header = header_from_user_ptr(handle_user(&child) as *const u8);
        assert_ne!(
            (*child_header).gc_flags & GC_FLAG_MARKED,
            0,
            "the typed descriptor alone must make the pointer slot traceable — \
             this is the liveness `layout_note_slot` would otherwise have had \
             to establish store-by-store"
        );
    }

    clear_marks();
    clear_mark_seeds();
}
