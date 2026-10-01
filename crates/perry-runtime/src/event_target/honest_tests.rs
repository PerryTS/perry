use super::*;

pub(crate) fn constructors() -> [fn() -> f64; 6] {
    [
        || boxed_ptr(js_event_target_new()),
        || boxed_ptr(js_event_new(string_value(b"x"), undefined_value(), 1)),
        || {
            boxed_ptr(js_custom_event_new(
                string_value(b"x"),
                undefined_value(),
                1,
            ))
        },
        || boxed_ptr(crate::url::js_abort_controller_new()),
        || boxed_ptr(crate::url::js_abort_signal_abort(undefined_value())),
        || boxed_ptr(js_dom_exception_new(undefined_value(), undefined_value())),
    ]
}

pub(crate) fn assert_surface(value: f64) {
    let _gc = crate::gc::GcSuppressScope::new();
    let ptr = crate::value::js_nanbox_get_pointer(value) as usize;
    let names = crate::object::js_object_get_own_property_names(value);
    let names = crate::value::js_nanbox_get_pointer(names) as *const ArrayHeader;
    let expected = usize::from(is_dom_exception_error(
        ptr as *const crate::error::ErrorHeader,
    ));
    assert_eq!(
        crate::array::js_array_length(names) as usize,
        expected,
        "internal event state must not be an own string property"
    );
    if expected == 1 {
        let name = crate::array::js_array_get_f64(names, 0);
        let name = crate::builtins::js_string_coerce(name);
        unsafe {
            let bytes = std::slice::from_raw_parts(
                (name as *const u8).add(std::mem::size_of::<StringHeader>()),
                (*name).byte_len as usize,
            );
            assert_eq!(bytes, b"stack");
        }
    }
    let keys = crate::object::js_object_keys_value(value);
    assert_eq!(crate::array::js_array_length(keys), 0);
}

#[test]
fn producers_return_headered_distinct_cells_with_only_node_own_names() {
    let _gc = crate::gc::GcSuppressScope::new();
    for make in constructors() {
        let first = make();
        let second = make();
        let ptr = crate::value::js_nanbox_get_pointer(first) as usize;
        assert!(
            !crate::value::addr_class::is_handle_band(ptr),
            "gate B: producer returned a handle id"
        );
        let header = unsafe { crate::value::addr_class::try_read_tracked_gc_header(ptr) }
            .expect("gate B: producer must own its GC header");
        assert!(matches!(
            unsafe { header.as_ref().obj_type },
            crate::gc::GC_TYPE_OBJECT | crate::gc::GC_TYPE_ERROR
        ));
        assert_ne!(first.to_bits(), second.to_bits());
        assert_surface(first);
    }
}

#[test]
fn event_resources_are_refused_by_name_at_the_worker_boundary() {
    let _gc = crate::gc::GcSuppressScope::new();
    let names = [
        "EventTarget",
        "Event",
        "CustomEvent",
        "AbortController",
        "AbortSignal",
        "DOMException",
    ];
    for (make, name) in constructors().into_iter().zip(names) {
        let value = make();
        let result = unsafe { crate::thread::serialize_nanbox_for_thread(value.to_bits()) };
        assert!(
            matches!(result, crate::thread::SerializedValue::Unsupported(actual) if actual == name),
            "{name} must not serialize as a plain object: {result:?}"
        );
    }
}

#[test]
fn event_target_methods_are_shared_inherited_values() {
    let _gc = crate::gc::GcSuppressScope::new();
    let first = js_event_target_new();
    let second = js_event_target_new();
    let name = key(b"dispatchEvent");
    let method = js_object_get_field_by_name_f64(first, name);
    assert_eq!(
        method.to_bits(),
        js_object_get_field_by_name_f64(second, name).to_bits()
    );
    assert!(crate::closure::is_closure_ptr(
        crate::value::js_nanbox_get_pointer(method) as usize
    ));
    let proto = crate::object::builtin_prototype_value("EventTarget");
    let proto = crate::value::js_nanbox_get_pointer(proto) as *const ObjectHeader;
    assert_eq!(
        method.to_bits(),
        js_object_get_field_by_name_f64(proto, name).to_bits()
    );
}

#[test]
fn class_hosts_do_not_acquire_instance_state() {
    let _gc = crate::gc::GcSuppressScope::new();
    for class in [
        crate::native_class_ids::EVENT_TARGET,
        crate::native_class_ids::EVENT,
        crate::native_class_ids::CUSTOM_EVENT,
        crate::native_class_ids::ABORT_CONTROLLER,
        crate::native_class_ids::ABORT_SIGNAL,
        crate::native_class_ids::DOM_EXCEPTION,
    ] {
        let host = crate::object::js_object_alloc(class, 1);
        assert!(
            unsafe { crate::object::shaped_symbols::entries(host as usize, true).is_empty() },
            "a class host must not gain an instance brand"
        );
        let meta = unsafe { crate::object::object_meta_ensure(host) };
        assert_eq!(unsafe { (*meta).native_state }, 0);
    }
}
