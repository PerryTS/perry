//! Each fixture proves the owner and its only payload edge existed during a
//! copying collection, then reads the rewritten payload after that collection.
use super::*;

const TEXT: &[u8] = b"layout-moving-child";
fn text() -> usize {
    crate::string::js_string_from_bytes(TEXT.as_ptr(), TEXT.len() as u32) as usize
}

fn move_root(owner: usize, minimum_copies: usize) -> usize {
    js_shadow_slot_set(0, ptr_bits(owner));
    let before = gc_collection_count();
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(
        gc_collection_count() > before,
        "the collection must have run"
    );
    assert!(
        trace.copying_nursery.copied_objects >= minimum_copies,
        "the live graph must be copied"
    );
    assert!(whole_heap_kinds::assert_whole_heap_kinds() > 0);
    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(moved, owner, "the live owner must move");
    moved
}

fn moved_text(bits: u64, before: usize) {
    let after = (bits & POINTER_MASK) as usize;
    assert_ne!(after, before, "the sole payload edge must be rewritten");
    unsafe { assert_string_bytes(after as *const crate::StringHeader, TEXT) };
}

fn array_case(kind: &str) {
    let _gc = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = text();
    let mut arr = if kind == "push" {
        crate::array::js_array_alloc(8)
    } else {
        crate::array::js_array_alloc_with_length(8)
    };
    let index = if kind == "sparse" { 6 } else { 1 };
    if kind == "push" {
        arr = crate::array::js_array_push_f64(arr, 1.5);
        arr = crate::array::js_array_push_f64(arr, f64::from_bits(string_bits(child)));
    } else {
        crate::array::js_array_set_f64(arr, index, f64::from_bits(string_bits(child)));
    }
    if kind == "growth" {
        let stub = arr;
        arr = crate::array::js_array_grow(arr, 128);
        assert_ne!(arr, stub);
        unsafe {
            assert_ne!(
                (*header_from_user_ptr(stub.cast())).gc_flags & GC_FLAG_FORWARDED,
                0
            );
        }
        // Root the forwarding stub, so the collector must follow the hop too.
        let moved = move_root(stub as usize, 2) as *mut crate::array::ArrayHeader;
        moved_text(
            crate::array::js_array_get_f64(moved, index).to_bits(),
            child,
        );
    } else {
        let moved = move_root(arr as usize, 2) as *mut crate::array::ArrayHeader;
        moved_text(
            crate::array::js_array_get_f64(moved, index).to_bits(),
            child,
        );
        if kind == "sparse" {
            unsafe {
                assert_eq!(
                    *crate::array::array_elements_ptr(moved),
                    crate::value::TAG_HOLE
                );
            }
        }
    }
}

#[test]
fn moving_push_built_mixed_array() {
    array_case("push");
}
#[test]
fn moving_holey_sparse_array() {
    array_case("sparse");
}
#[test]
fn moving_growth_forwarding_stub() {
    array_case("growth");
}

#[test]
fn moving_json_mixed_record() {
    let _gc = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let bytes = br#"{"n":1.5,"child":"layout-moving-child"}"#;
    let parsed = unsafe {
        crate::json::js_json_parse(crate::string::js_string_from_bytes(
            bytes.as_ptr(),
            bytes.len() as u32,
        ))
    };
    let obj = (parsed.bits() & POINTER_MASK) as *mut crate::object::ObjectHeader;
    let key = crate::string::js_string_from_bytes(b"child".as_ptr(), 5);
    let child =
        (crate::object::js_object_get_field_by_name(obj, key).bits() & POINTER_MASK) as usize;
    let moved = move_root(obj as usize, 2) as *mut crate::object::ObjectHeader;
    moved_text(
        crate::object::js_object_get_field_by_name(
            moved,
            crate::string::js_string_from_bytes(b"child".as_ptr(), 5),
        )
        .bits(),
        child,
    );
}

#[test]
fn moving_twelve_key_null_proto_spill() {
    let _gc = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = text();
    let obj = crate::object::js_object_alloc_null_proto(0, 0);
    for i in 0..12 {
        let name = format!("k{i}");
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let value = if i == 11 {
            f64::from_bits(string_bits(child))
        } else {
            i as f64
        };
        crate::object::js_object_set_field_by_name(obj, key, value);
    }
    assert_ne!(
        crate::object::test_spill_buffer_addr(obj as usize),
        0,
        "fixture needs a spill array"
    );
    let moved = move_root(obj as usize, 4) as *mut crate::object::ObjectHeader;
    moved_text(
        crate::object::js_object_get_field_by_name(
            moved,
            crate::string::js_string_from_bytes(b"k11".as_ptr(), 3),
        )
        .bits(),
        child,
    );
}

#[test]
fn moving_arguments_object() {
    let _gc = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = text();
    let values = [1.5, f64::from_bits(string_bits(child))];
    let bundle = crate::array::js_array_from_f64(values.as_ptr(), 2);
    let args = crate::object::js_arguments_object_alloc(
        crate::value::js_nanbox_pointer(bundle as i64),
        f64::from_bits(crate::value::TAG_UNDEFINED),
        0,
    );
    let moved = move_root(args as usize, 2) as *mut crate::object::ObjectHeader;
    moved_text(
        crate::object::js_object_get_field_by_name(
            moved,
            crate::string::js_string_from_bytes(b"1".as_ptr(), 1),
        )
        .bits(),
        child,
    );
}

#[test]
fn moving_array_subclass_elements() {
    let _gc = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _representation =
        crate::array::subclass_elements::ArraySubclassRepresentationGuard::elements();
    let child = text();
    let obj = crate::object::js_object_alloc(99123, 0);
    unsafe {
        crate::array::subclass_elements::install_elements(obj, 8);
        let elements = crate::array::subclass_elements::elements_of(obj);
        crate::array::js_array_set_f64(elements, 1, f64::from_bits(string_bits(child)));
        let moved = move_root(obj as usize, 4) as *mut crate::object::ObjectHeader;
        let elements = crate::array::subclass_elements::elements_of(moved);
        moved_text(crate::array::js_array_get_f64(elements, 1).to_bits(), child);
    }
}

extern "C" fn callback(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    1.5
}

#[test]
fn moving_closure_box_capture() {
    let _gc = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = text();
    let cell = crate::r#box::js_box_alloc(f64::from_bits(string_bits(child)));
    let closure = crate::closure::js_closure_alloc(crate::fn_info!(callback, 0), 8);
    crate::closure::js_closure_set_capture_f64(closure, 0, 1.5);
    crate::closure::js_closure_set_capture_ptr(closure, 1, cell as i64);
    let moved = move_root(closure as usize, 3) as *mut crate::closure::ClosureHeader;
    let after = crate::closure::js_closure_get_capture_ptr(moved, 1) as *const crate::r#box::Box;
    assert_ne!(after, cell as *const crate::r#box::Box);
    moved_text(unsafe { (*after).value }, child);
}

#[test]
fn moving_native_promise_closure_raw_captures() {
    let _gc = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = text();
    let callback = crate::closure::js_closure_alloc(crate::fn_info!(callback, 0), 1);
    crate::closure::js_closure_set_capture_f64(callback, 0, f64::from_bits(string_bits(child)));
    let promise = crate::promise::js_promise_new();
    let _next = crate::promise::js_promise_finally(promise, callback);
    let wrapper = unsafe { (*promise).on_fulfilled };
    assert!(!wrapper.is_null(), "finally must install a native wrapper");
    let moved = move_root(wrapper as usize, 3) as *mut crate::closure::ClosureHeader;
    let after =
        crate::closure::js_closure_get_capture_ptr(moved, 0) as *mut crate::closure::ClosureHeader;
    assert_ne!(after, callback, "raw callback capture must be rewritten");
    moved_text(
        crate::closure::js_closure_get_capture_f64(after, 0).to_bits(),
        child,
    );
}

#[test]
fn moving_closure_f64_captures() {
    let _gc = CopyingNurseryTestGuard::new(1);
    let closure = crate::closure::js_closure_alloc(crate::fn_info!(callback, 0), 8);
    for i in 0..8 {
        crate::closure::js_closure_set_capture_f64(closure, i, i as f64 + 0.25);
    }
    let moved = move_root(closure as usize, 1) as *mut crate::closure::ClosureHeader;
    assert_eq!(
        test_heap_child_slot_count(moved.cast()),
        0,
        "NUMBERS captures must be skipped"
    );
    for i in 0..8 {
        assert_eq!(
            crate::closure::js_closure_get_capture_f64(moved, i),
            i as f64 + 0.25
        );
    }
}

#[test]
fn double_bits_equal_to_live_address_are_never_rewritten() {
    let _gc = CopyingNurseryTestGuard::new(3);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let (child, _) = unsafe { alloc_nursery_test_object(0) };
    let bits = child as u64;
    let values = [f64::from_bits(bits), 1.5];
    let array = crate::array::js_array_from_f64(values.as_ptr(), 2);
    assert_eq!(crate::array::js_array_mark_numeric_f64_layout(array), 1);
    let obj = crate::object::js_object_alloc(0, 1);
    unsafe {
        let fields = obj
            .cast::<u8>()
            .add(std::mem::size_of::<crate::object::ObjectHeader>())
            .cast::<u64>();
        *fields = bits;
        restamp_with_rep(obj, f64_lanes([0]));
    }
    js_shadow_slot_set(0, ptr_bits(child as usize));
    js_shadow_slot_set(1, ptr_bits(array as usize));
    js_shadow_slot_set(2, ptr_bits(obj as usize));
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(trace.copying_nursery.copied_objects >= 3);
    assert_ne!(
        (js_shadow_slot_get(0) & POINTER_MASK) as usize,
        child as usize
    );
    let array_after = (js_shadow_slot_get(1) & POINTER_MASK) as *mut crate::array::ArrayHeader;
    let obj_after = (js_shadow_slot_get(2) & POINTER_MASK) as *mut crate::object::ObjectHeader;
    assert_eq!(
        crate::array::js_array_get_f64(array_after, 0).to_bits(),
        bits
    );
    unsafe {
        let fields = obj_after
            .cast::<u8>()
            .add(std::mem::size_of::<crate::object::ObjectHeader>())
            .cast::<u64>();
        assert_eq!(*fields, bits);
    }
}
