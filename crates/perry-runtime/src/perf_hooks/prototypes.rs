use super::*;

const PERF_CONSTRUCTOR_NAMES: &[&str] = &[
    "Performance",
    "PerformanceEntry",
    "PerformanceMark",
    "PerformanceMeasure",
    "PerformanceObserver",
    "PerformanceObserverEntryList",
    "PerformanceResourceTiming",
];

pub(super) fn is_perf_constructor_name(class_name: &str) -> bool {
    PERF_CONSTRUCTOR_NAMES.contains(&class_name)
}

fn invalid_perf_receiver(class_name: &str) -> ! {
    throw_type_error_with_code(
        &format!("The \"this\" argument must be an instance of {class_name}"),
        "ERR_INVALID_THIS",
    )
}

extern "C" fn perf_entry_field_getter_thunk(closure: *const crate::closure::ClosureHeader) -> f64 {
    unsafe {
        let this = crate::object::js_implicit_this_get();
        let Some(obj) = as_object_ptr(this) else {
            invalid_perf_receiver("PerformanceEntry");
        };
        if !is_perf_entry_object(obj) {
            invalid_perf_receiver("PerformanceEntry");
        }
        let name_ptr = crate::closure::js_closure_get_capture_ptr(closure, 0) as *const u8;
        let name_len = crate::closure::js_closure_get_capture_ptr(closure, 1) as usize;
        if name_ptr.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        let name = std::slice::from_raw_parts(name_ptr, name_len);
        let scope = crate::gc::RuntimeHandleScope::new();
        let receiver = scope.root_nanbox_f64(this);
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let obj = JSValue::from_bits(receiver.get_nanbox_u64())
            .as_pointer::<crate::object::ObjectHeader>();
        f64::from_bits(js_object_get_field_by_name(obj, key).bits())
    }
}

extern "C" fn perf_entry_to_json_thunk(_closure: *const crate::closure::ClosureHeader) -> f64 {
    unsafe {
        let this = crate::object::js_implicit_this_get();
        let Some(obj) = as_object_ptr(this) else {
            invalid_perf_receiver("PerformanceEntry");
        };
        if !is_perf_entry_object(obj) {
            invalid_perf_receiver("PerformanceEntry");
        }
        perf_entry_to_json(this)
    }
}

extern "C" fn perf_time_origin_getter_thunk(_closure: *const crate::closure::ClosureHeader) -> f64 {
    time_origin_ms()
}

extern "C" fn perf_supported_entry_types_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    perf_supported_entry_types_value()
}

pub(crate) fn perf_supported_entry_types_value() -> f64 {
    let _no_move = crate::gc::GcSuppressScope::new();
    let value = js_perf_supported_entry_types();
    crate::object::js_object_freeze(value)
}

extern "C" fn perf_observer_observe_thunk(
    _closure: *const crate::closure::ClosureHeader,
    options: f64,
) -> f64 {
    let this = crate::object::js_implicit_this_get();
    if !is_perf_observer_value(this) {
        invalid_perf_receiver("PerformanceObserver");
    }
    js_perf_observer_observe(this, options)
}

extern "C" fn perf_observer_disconnect_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    let this = crate::object::js_implicit_this_get();
    if !is_perf_observer_value(this) {
        invalid_perf_receiver("PerformanceObserver");
    }
    js_perf_observer_disconnect(this)
}

extern "C" fn perf_observer_take_records_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    let this = crate::object::js_implicit_this_get();
    if !is_perf_observer_value(this) {
        invalid_perf_receiver("PerformanceObserver");
    }
    js_perf_observer_take_records(this)
}

extern "C" fn perf_list_get_entries_thunk(_closure: *const crate::closure::ClosureHeader) -> f64 {
    let this = crate::object::js_implicit_this_get();
    if !is_perf_observer_list_value(this) {
        invalid_perf_receiver("PerformanceObserverEntryList");
    }
    unsafe { current_list_get_entries() }
}

extern "C" fn perf_list_get_by_type_thunk(
    _closure: *const crate::closure::ClosureHeader,
    entry_type: f64,
) -> f64 {
    let this = crate::object::js_implicit_this_get();
    if !is_perf_observer_list_value(this) {
        invalid_perf_receiver("PerformanceObserverEntryList");
    }
    unsafe { current_list_get_by_type(entry_type) }
}

extern "C" fn perf_list_get_by_name_thunk(
    _closure: *const crate::closure::ClosureHeader,
    name: f64,
) -> f64 {
    let this = crate::object::js_implicit_this_get();
    if !is_perf_observer_list_value(this) {
        invalid_perf_receiver("PerformanceObserverEntryList");
    }
    unsafe { current_list_get_by_name(name) }
}

fn perf_method_value(func_ptr: *const u8, name: &str, arity: u32) -> f64 {
    crate::closure::js_register_closure_arity(func_ptr, arity);
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    crate::object::set_bound_native_closure_name(closure, name);
    crate::object::set_builtin_closure_length(closure as usize, arity);
    crate::value::js_nanbox_pointer(closure as i64)
}

unsafe fn install_perf_method(
    proto: *mut crate::object::ObjectHeader,
    name: &str,
    value: f64,
    enumerable: bool,
) {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(proto, key, value);
    crate::object::set_builtin_property_attrs(
        proto as usize,
        name.to_string(),
        crate::object::PropertyAttrs::new(true, enumerable, true),
    );
}

unsafe fn install_perf_getter(
    proto: *mut crate::object::ObjectHeader,
    name: &str,
    getter: f64,
    enumerable: bool,
) {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(proto, key, f64::from_bits(crate::value::TAG_UNDEFINED));
    crate::object::set_builtin_accessor_descriptor(
        proto as usize,
        name.to_string(),
        crate::object::AccessorDescriptor {
            get: getter.to_bits(),
            set: 0,
        },
        crate::object::PropertyAttrs::new(true, enumerable, true),
    );
}

unsafe fn install_perf_to_string_tag(proto: *mut crate::object::ObjectHeader, tag: &str) {
    let symbol = crate::symbol::well_known_symbol("toStringTag");
    if symbol.is_null() {
        return;
    }
    let value = str_value(tag);
    crate::symbol::js_object_set_symbol_property(
        crate::value::js_nanbox_pointer(proto as i64),
        crate::value::js_nanbox_pointer(symbol as i64),
        f64::from_bits(value.bits()),
    );
    crate::symbol::set_symbol_property_attrs(
        proto as usize,
        symbol as usize,
        crate::object::PropertyAttrs::new(false, false, true),
    );
}

unsafe fn perf_field_getter(name: &str) -> f64 {
    let leaked: &'static [u8] = name.as_bytes().to_vec().leak();
    let func_ptr = perf_entry_field_getter_thunk as *const u8;
    crate::closure::js_register_closure_arity(func_ptr, 0);
    let closure = crate::closure::js_closure_alloc(func_ptr, 2);
    crate::closure::js_closure_set_capture_ptr(closure, 0, leaked.as_ptr() as i64);
    crate::closure::js_closure_set_capture_ptr(closure, 1, leaked.len() as i64);
    crate::object::set_bound_native_closure_name(closure, &format!("get {name}"));
    crate::object::set_builtin_closure_length(closure as usize, 0);
    crate::value::js_nanbox_pointer(closure as i64)
}

fn perf_constructor_prototype(class_name: &str) -> f64 {
    let ctor = crate::object::bound_native_callable_export_value("perf_hooks", class_name);
    let ptr = crate::value::js_nanbox_get_pointer(ctor) as usize;
    crate::closure::closure_get_dynamic_prop(ptr, "prototype")
}

/// Link runtime-created perf objects through their built-in class hierarchy.
///
/// This is class-default wiring, not a user `Object.setPrototypeOf` override.
/// Before #9251, the loud setter's divergence bit was also interpreted as a
/// user-origin signal; method dispatch then treated the chain as user-replaced
/// and re-dispatched an already bound prototype method through
/// `js_native_call_method`,
/// recursing until the process exhausts its stack (#9281).
fn link_perf_class_default_prototype(obj: usize, proto_bits: u64) {
    crate::object::prototype_chain::object_link_class_default_prototype(obj, proto_bits);
}

pub(super) fn link_perf_prototype(value: f64, class_name: &str) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let prototype = scope.root_nanbox_f64(perf_constructor_prototype(class_name));
    let obj = crate::value::js_nanbox_get_pointer(value.get_nanbox_f64()) as usize;
    link_perf_class_default_prototype(obj, prototype.get_nanbox_f64().to_bits());
    value.get_nanbox_f64()
}

pub(crate) unsafe fn attach_perf_hooks_constructor(
    class_name: &str,
    constructor_value: f64,
    closure_addr: usize,
) {
    if !PERF_CONSTRUCTOR_NAMES.contains(&class_name) {
        return;
    }
    let _no_move = crate::gc::GcSuppressScope::new();
    let proto = crate::object::js_object_alloc(0, 0);
    if proto.is_null() {
        return;
    }

    let constructor_key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
    js_object_set_field_by_name(proto, constructor_key, constructor_value);
    crate::object::set_builtin_property_attrs(
        proto as usize,
        "constructor".to_string(),
        crate::object::PropertyAttrs::new(true, false, true),
    );

    match class_name {
        "Performance" => {
            for method in [
                "clearMarks",
                "clearMeasures",
                "clearResourceTimings",
                "getEntries",
                "getEntriesByName",
                "getEntriesByType",
                "mark",
                "measure",
                "now",
                "setResourceTimingBufferSize",
                "toJSON",
            ] {
                let value = crate::object::bound_native_callable_export_value("perf_hooks", method);
                install_perf_method(proto, method, value, true);
            }
            for method in ["eventLoopUtilization", "markResourceTiming", "timerify"] {
                let value = crate::object::bound_native_callable_export_value("perf_hooks", method);
                install_perf_method(proto, method, value, false);
            }
            let getter = perf_method_value(
                perf_time_origin_getter_thunk as *const u8,
                "get timeOrigin",
                0,
            );
            install_perf_getter(proto, "timeOrigin", getter, true);
            install_perf_to_string_tag(proto, "Performance");
        }
        "PerformanceEntry" => {
            for field in ["name", "entryType", "startTime", "duration"] {
                let getter = perf_field_getter(field);
                install_perf_getter(proto, field, getter, true);
            }
            let to_json = perf_method_value(perf_entry_to_json_thunk as *const u8, "toJSON", 0);
            install_perf_method(proto, "toJSON", to_json, true);
        }
        "PerformanceMark" | "PerformanceMeasure" => {
            let base = perf_constructor_prototype("PerformanceEntry");
            link_perf_class_default_prototype(proto as usize, base.to_bits());
            let getter = perf_field_getter("detail");
            install_perf_getter(proto, "detail", getter, true);
            let to_json = perf_method_value(perf_entry_to_json_thunk as *const u8, "toJSON", 0);
            install_perf_method(proto, "toJSON", to_json, false);
            install_perf_to_string_tag(proto, class_name);
        }
        "PerformanceObserver" => {
            install_perf_method(
                proto,
                "observe",
                perf_method_value(perf_observer_observe_thunk as *const u8, "observe", 1),
                true,
            );
            install_perf_method(
                proto,
                "disconnect",
                perf_method_value(perf_observer_disconnect_thunk as *const u8, "disconnect", 0),
                true,
            );
            install_perf_method(
                proto,
                "takeRecords",
                perf_method_value(
                    perf_observer_take_records_thunk as *const u8,
                    "takeRecords",
                    0,
                ),
                true,
            );
            install_perf_to_string_tag(proto, class_name);
        }
        "PerformanceObserverEntryList" => {
            install_perf_method(
                proto,
                "getEntries",
                perf_method_value(perf_list_get_entries_thunk as *const u8, "getEntries", 0),
                true,
            );
            install_perf_method(
                proto,
                "getEntriesByType",
                perf_method_value(
                    perf_list_get_by_type_thunk as *const u8,
                    "getEntriesByType",
                    1,
                ),
                true,
            );
            install_perf_method(
                proto,
                "getEntriesByName",
                perf_method_value(
                    perf_list_get_by_name_thunk as *const u8,
                    "getEntriesByName",
                    1,
                ),
                true,
            );
            install_perf_to_string_tag(proto, class_name);
        }
        "PerformanceResourceTiming" => {
            let base = perf_constructor_prototype("PerformanceEntry");
            link_perf_class_default_prototype(proto as usize, base.to_bits());
            for field in [
                "initiatorType",
                "workerStart",
                "redirectStart",
                "redirectEnd",
                "fetchStart",
                "domainLookupStart",
                "domainLookupEnd",
                "connectStart",
                "connectEnd",
                "secureConnectionStart",
                "nextHopProtocol",
                "requestStart",
                "responseStart",
                "responseEnd",
                "encodedBodySize",
                "decodedBodySize",
                "transferSize",
                "deliveryType",
                "responseStatus",
            ] {
                let getter = perf_field_getter(field);
                install_perf_getter(proto, field, getter, true);
            }
            let to_json = perf_method_value(perf_entry_to_json_thunk as *const u8, "toJSON", 0);
            install_perf_method(proto, "toJSON", to_json, true);
            install_perf_to_string_tag(proto, class_name);
        }
        _ => {}
    }

    let proto_value = crate::value::js_nanbox_pointer(proto as i64);
    crate::closure::closure_set_dynamic_prop(closure_addr, "prototype", proto_value);
    crate::object::set_builtin_property_attrs(
        closure_addr,
        "prototype".to_string(),
        crate::object::PropertyAttrs::new(false, false, false),
    );

    if class_name == "PerformanceObserver" {
        let getter = perf_method_value(
            perf_supported_entry_types_getter_thunk as *const u8,
            "get supportedEntryTypes",
            0,
        );
        crate::closure::closure_set_dynamic_prop(
            closure_addr,
            "supportedEntryTypes",
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
        crate::object::set_builtin_accessor_descriptor(
            closure_addr,
            "supportedEntryTypes".to_string(),
            crate::object::AccessorDescriptor {
                get: getter.to_bits(),
                set: 0,
            },
            crate::object::PropertyAttrs::new(true, false, true),
        );
    }
}
