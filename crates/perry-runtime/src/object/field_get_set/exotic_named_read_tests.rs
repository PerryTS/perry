//! Pin both sides of the shape gate, including the work an ordinary miss skips.
use super::*;

fn key(name: &[u8]) -> *const crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn ptr(receiver: &crate::gc::RuntimeHandle<'_>) -> *mut ObjectHeader {
    crate::value::js_nanbox_get_pointer(receiver.get_nanbox_f64()) as *mut ObjectHeader
}

fn read(receiver: &crate::gc::RuntimeHandle<'_>, name: &[u8]) -> JSValue {
    let scope = crate::gc::RuntimeHandleScope::new();
    let name = scope.root_nanbox_f64(crate::value::nanbox_string_key(key(name)));
    let obj = crate::value::js_nanbox_get_pointer(receiver.get_nanbox_f64());
    let name = crate::value::js_get_string_pointer_unified(name.get_nanbox_f64());
    js_object_get_field_by_name(
        obj as *const ObjectHeader,
        name as *const crate::StringHeader,
    )
}

fn slow_read(receiver: &crate::gc::RuntimeHandle<'_>, name: &[u8]) -> JSValue {
    let scope = crate::gc::RuntimeHandleScope::new();
    let name = scope.root_raw_const_ptr(key(name));
    super::get_field_by_name::test_get_past_data_probe(ptr(receiver), name.get_raw_const_ptr())
}

#[test]
fn ordinary_named_misses_skip_all_exotic_probes() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let plain = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::object_alloc_plain(0) as i64,
    ));
    let instance = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(12345, 0) as i64,
    ));
    for receiver in [&plain, &instance] {
        // Give the receiver a keys view so an absent lookup reaches the
        // full shared tail.
        let own = scope.root_raw_const_ptr(key(b"own"));
        js_object_set_field_by_name(ptr(receiver), own.get_raw_const_ptr(), 21.0);
        let descriptor = unsafe {
            super::super::shapes::object_shape_descriptor(ptr(receiver))
                .expect("test premise: the receiver has a shape")
        };
        assert_eq!(
            descriptor.object_kind,
            super::super::shapes::ShapeObjectKind::Ordinary
        );
        // Warm lazy prototype state, then measure actual slow misses. A
        // native-get hit could not satisfy the positive ungated control.
        assert!(slow_read(receiver, b"probeGateMissing").is_undefined());
        let before = super::exotic_named_read::ordinary_probe_counts();
        for _ in 0..4 {
            assert!(slow_read(receiver, b"probeGateMissing").is_undefined());
        }
        assert_eq!(
            super::exotic_named_read::ordinary_probe_counts(),
            before,
            "ordinary misses must enter none of the exotic probe blocks"
        );
    }
}

#[test]
fn exotic_named_reads_keep_native_properties() {
    crate::gc::initialize_runtime_fixture();
    let scope = crate::gc::RuntimeHandleScope::new();
    let date = scope.root_nanbox_f64(crate::date_hook_slots::js_date_new_from_timestamp(0.0));
    let params = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::url_hooks::url_search_params_new_empty() as i64,
    ));
    let typed = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::typedarray::js_typed_array_new_empty(0, 3) as i64,
    ));
    let before = super::exotic_named_read::probe_counts();
    assert_eq!(slow_read(&typed, b"length").to_number(), 3.0);
    assert!(
        super::exotic_named_read::probe_counts()[0] > before[0],
        "typed arrays must still reach their native metadata probe"
    );
    let before = super::exotic_named_read::probe_counts();
    assert!(read(&date, b"getTime").is_pointer());
    let after = super::exotic_named_read::probe_counts();
    assert!(after[1] > before[1], "Date must still reach its cell probe");
    // #12697: URLSearchParams is born on its declared layout, an ordinary
    // shape; `size` and the methods are its prototype's, found through the
    // layout's prototype family like any inherited property.
    assert_eq!(read(&params, b"size").to_number(), 0.0);
    let append = read(&params, b"append");
    assert!(append.is_pointer());
    let proto = crate::object::js_object_get_prototype_of(params.get_nanbox_f64());
    let proto = scope.root_nanbox_f64(proto);
    assert_eq!(
        read(&proto, b"append").bits(),
        append.bits(),
        "the method is URLSearchParams.prototype's own"
    );
    assert!(read(&params, b"probeGateMissing").is_undefined());
}
