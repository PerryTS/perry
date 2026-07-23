use super::blob::string_from_value;
use super::stream_promises::{
    abort_error_value, get_object_property, object_ptr_from_value, undefined_value,
};
use super::*;
use std::cell::RefCell;

const FS_PROMISES_EXPORTS: &[&str] = &[
    "access",
    "appendFile",
    "chmod",
    "chown",
    "copyFile",
    "cp",
    "glob",
    "lchmod",
    "lchown",
    "link",
    "lstat",
    "lutimes",
    "mkdir",
    "mkdtemp",
    "mkdtempDisposable",
    "open",
    "opendir",
    "readFile",
    "readdir",
    "readlink",
    "realpath",
    "rename",
    "rm",
    "rmdir",
    "stat",
    "statfs",
    "symlink",
    "truncate",
    "unlink",
    "utimes",
    "watch",
    "writeFile",
];

#[test]
fn known_submodules_have_at_least_one_export() {
    for s in super::ALL_SUBMODULE_SPECS {
        assert!(
            !s.exports.is_empty(),
            "submodule {} has zero exports",
            s.key
        );
    }
}

#[test]
fn find_submodule_for_known_keys() {
    for key in [
        "timers_promises",
        "vm",
        "readline_promises",
        "fs_promises",
        "stream_promises",
        "stream_consumers",
        "stream_web",
        "sys",
        "diagnostics_channel",
        "trace_events",
        "test_reporters",
    ] {
        assert!(
            find_submodule(key).is_some(),
            "submodule {} missing from SUBMODULES table",
            key
        );
    }
}

#[test]
fn find_submodule_for_unknown_key_returns_none() {
    assert!(find_submodule("not_a_real_submodule").is_none());
}

/// #906 follow-up — pino reads `tracingChannel('pino_asJson').hasSubscribers`
/// before deciding whether to enter the tracing branch. The stub MUST
/// expose `tracingChannel` as a callable thunk in the SUBMODULES table
/// so the namespace singleton's field is a function (not TAG_TRUE).
#[test]
fn diagnostics_channel_exposes_tracingChannel_export() {
    let submod =
        find_submodule("diagnostics_channel").expect("diagnostics_channel must be in SUBMODULES");
    let names: Vec<&str> = submod.exports.iter().map(|e| e.name).collect();
    for required in ["tracingChannel", "channel", "subscribe", "unsubscribe"] {
        assert!(
            names.contains(&required),
            "diagnostics_channel must export `{}` for pino's `require('node:diagnostics_channel')` to keep working",
            required
        );
    }
}

#[test]
fn http_outgoing_message_export_is_callable() {
    let value = unsafe {
        crate::object::js_native_module_property_by_name(
            b"http".as_ptr(),
            "http".len(),
            b"OutgoingMessage".as_ptr(),
            "OutgoingMessage".len(),
        )
    };
    assert_ne!(value.to_bits(), crate::value::TAG_UNDEFINED);
    assert_eq!(
        unsafe { crate::object::bound_native_callable_value_arity(value) },
        Some(1)
    );

    let namespace =
        crate::object::js_create_native_module_namespace(b"http".as_ptr(), "http".len());
    let keys = crate::object::js_object_keys_value(namespace);
    let mut found = false;
    for i in 0..crate::array::js_array_length(keys) {
        let key = crate::array::js_array_get(keys, i);
        if string_from_value(f64::from_bits(key.bits())).as_deref() == Some("OutgoingMessage") {
            found = true;
            break;
        }
    }
    assert!(found, "http namespace keys should include OutgoingMessage");
}

#[test]
fn util_is_array_export_records_its_node_arity() {
    // Namespace reads (`util.isArray`) and named-import materialization
    // (`import { isArray }`) intentionally converge on this same native-module
    // property helper. The node-suite fixture exercises both source forms
    // end-to-end; this unit test pins the shared closure metadata they consume.
    let value = unsafe {
        crate::object::js_native_module_property_by_name(
            b"util".as_ptr(),
            "util".len(),
            b"isArray".as_ptr(),
            "isArray".len(),
        )
    };
    let closure = crate::value::JSValue::from_bits(value.to_bits())
        .as_pointer::<crate::closure::ClosureHeader>();

    assert_eq!(
        crate::object::builtin_closure_length(closure as usize),
        Some(1)
    );
}

fn boxed_ptr(ptr: *const u8) -> f64 {
    f64::from_bits(JSValue::pointer(ptr).bits())
}

fn promise_ptr(value: f64) -> *mut crate::promise::Promise {
    crate::value::js_nanbox_get_pointer(value) as *mut crate::promise::Promise
}

fn string_value(s: &str) -> f64 {
    let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn string_header_value(ptr: *mut crate::StringHeader) -> f64 {
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn promise_reason_error(value: f64) -> *mut crate::error::ErrorHeader {
    let promise = promise_ptr(value);
    assert_eq!(crate::promise::js_promise_state(promise), 2);
    crate::value::js_nanbox_get_pointer(crate::promise::js_promise_reason(promise))
        as *mut crate::error::ErrorHeader
}

fn assert_rejected_node_error(value: f64, code: &str, message: &str) {
    let err = promise_reason_error(value);
    let name = string_from_value(string_header_value(crate::error::js_error_get_name(err)))
        .expect("error name should be a string");
    let actual_message =
        string_from_value(string_header_value(crate::error::js_error_get_message(err)))
            .expect("error message should be a string");
    let actual_code =
        crate::node_submodules::error_code_for_message(crate::error::js_error_get_message(err))
            .expect("error should have a registered Node code");

    assert_eq!(name, "TypeError");
    assert_eq!(actual_code, code);
    assert_eq!(actual_message, message);
}

fn assert_fs_promises_namespace_exports(ns_value: f64) {
    for name in FS_PROMISES_EXPORTS {
        let property = get_object_property(ns_value, name.as_bytes())
            .unwrap_or_else(|| panic!("fs_promises namespace missing runtime export: {name}"));
        let direct = unsafe {
            js_node_submodule_export_as_function(
                b"fs_promises".as_ptr(),
                "fs_promises".len() as u32,
                name.as_ptr(),
                name.len() as u32,
            )
        };
        assert_eq!(
            property.to_bits(),
            direct.to_bits(),
            "fs_promises namespace export `{name}` should match direct named export"
        );
    }

    let constants = get_object_property(ns_value, b"constants")
        .expect("fs_promises namespace missing constants export");
    assert!(
        object_ptr_from_value(constants).is_some(),
        "fs_promises.constants should be an object"
    );
    let fs_constants = fs_constants_namespace_value();
    assert_eq!(
        constants.to_bits(),
        fs_constants.to_bits(),
        "fs_promises.constants should reuse fs.constants"
    );
    let direct_constants = unsafe {
        js_node_submodule_export_as_function(
            b"fs_promises".as_ptr(),
            "fs_promises".len() as u32,
            b"constants".as_ptr(),
            "constants".len() as u32,
        )
    };
    assert_eq!(
        constants.to_bits(),
        direct_constants.to_bits(),
        "direct fs_promises constants export should match namespace property"
    );

    for not_implemented in ["FileHandle", "Dir", "Dirent"] {
        assert!(
            get_object_property(ns_value, not_implemented.as_bytes()).is_none(),
            "fs_promises namespace should not expose unimplemented `{not_implemented}`"
        );
    }
}

#[test]
fn stream_parent_promises_property_exposes_namespace() {
    let value = unsafe {
        crate::object::js_native_module_property_by_name(
            b"stream".as_ptr(),
            "stream".len(),
            b"promises".as_ptr(),
            "promises".len(),
        )
    };
    let ns = object_ptr_from_value(value).expect("stream.promises should be an object");
    assert!(get_object_property(boxed_ptr(ns as *const u8), b"pipeline").is_some());
    assert!(get_object_property(boxed_ptr(ns as *const u8), b"finished").is_some());
}

/// #2133: `fs.promises` (the parent `node:fs` module's `.promises`
/// property) must resolve to the populated `fs_promises` submodule
/// singleton — not an empty namespace stub — so destructured exports
/// like `const { open } = fs.promises` and the indirect form
/// (`const p = fs.promises; p.open(...)`) both reach real callable
/// closures and a returned FileHandle dispatches its methods.
#[test]
fn fs_parent_promises_property_exposes_namespace() {
    let value = unsafe {
        crate::object::js_native_module_property_by_name(
            b"fs".as_ptr(),
            "fs".len(),
            b"promises".as_ptr(),
            "promises".len(),
        )
    };
    let ns = object_ptr_from_value(value).expect("fs.promises should be an object");
    let ns_value = boxed_ptr(ns as *const u8);
    assert_fs_promises_namespace_exports(ns_value);

    let direct =
        unsafe { js_node_submodule_namespace(b"fs_promises".as_ptr(), "fs_promises".len() as u32) };
    assert_eq!(
        value.to_bits(),
        direct.to_bits(),
        "fs.promises should reuse the direct fs_promises namespace singleton"
    );
}

#[test]
fn fs_promises_direct_namespace_exposes_runtime_exports() {
    let value =
        unsafe { js_node_submodule_namespace(b"fs_promises".as_ptr(), "fs_promises".len() as u32) };
    let ns = object_ptr_from_value(value).expect("fs_promises namespace should be an object");
    assert_fs_promises_namespace_exports(boxed_ptr(ns as *const u8));
}

#[test]
fn fs_promises_constants_reuses_fs_constants_namespace() {
    let fs_constants = unsafe {
        crate::object::js_native_module_property_by_name(
            b"fs".as_ptr(),
            "fs".len(),
            b"constants".as_ptr(),
            "constants".len(),
        )
    };
    let named_constants = unsafe {
        js_node_submodule_export_as_function(
            b"fs_promises".as_ptr(),
            "fs_promises".len() as u32,
            b"constants".as_ptr(),
            "constants".len() as u32,
        )
    };
    let namespace_constants = unsafe {
        js_node_submodule_namespace_member(
            b"fs_promises".as_ptr(),
            "fs_promises".len() as u32,
            b"constants".as_ptr(),
            "constants".len() as u32,
        )
    };
    let namespace =
        unsafe { js_node_submodule_namespace(b"fs_promises".as_ptr(), "fs_promises".len() as u32) };
    let ns = object_ptr_from_value(namespace).expect("fs/promises namespace should be object");
    let object_constants = get_object_property(boxed_ptr(ns as *const u8), b"constants").unwrap();

    assert_eq!(named_constants.to_bits(), fs_constants.to_bits());
    assert_eq!(namespace_constants.to_bits(), fs_constants.to_bits());
    assert_eq!(object_constants.to_bits(), fs_constants.to_bits());
}

#[test]
fn zlib_codes_export_resolves_to_return_code_map() {
    let codes = unsafe {
        crate::object::js_native_module_property_by_name(
            b"zlib".as_ptr(),
            "zlib".len(),
            b"codes".as_ptr(),
            "codes".len(),
        )
    };
    let namespace =
        crate::object::js_create_native_module_namespace(b"zlib".as_ptr(), "zlib".len());
    let namespace_codes =
        get_object_property(namespace, b"codes").expect("zlib namespace should expose codes");

    assert_eq!(
        namespace_codes.to_bits(),
        codes.to_bits(),
        "direct and namespace zlib.codes reads should share the same object"
    );
    assert_eq!(
        JSValue::from_bits(get_object_property(codes, b"Z_OK").unwrap().to_bits()).as_number(),
        0.0
    );
    assert_eq!(
        JSValue::from_bits(
            get_object_property(codes, b"Z_DATA_ERROR")
                .unwrap()
                .to_bits()
        )
        .as_number(),
        -3.0
    );
    assert_eq!(
        string_from_value(get_object_property(codes, b"0").unwrap()).as_deref(),
        Some("Z_OK")
    );
    assert_eq!(
        string_from_value(get_object_property(codes, b"-6").unwrap()).as_deref(),
        Some("Z_VERSION_ERROR")
    );
    assert_eq!(
        string_from_value(crate::value::js_dyn_index_get(codes, -1.0)).as_deref(),
        Some("Z_ERRNO")
    );
    assert_eq!(
        string_from_value(crate::value::js_dyn_index_get(codes, -6.0)).as_deref(),
        Some("Z_VERSION_ERROR")
    );
}

#[test]
fn stream_promises_default_export_exposes_namespace() {
    let value = unsafe {
        js_node_submodule_export_as_function(
            b"stream_promises".as_ptr(),
            "stream_promises".len() as u32,
            b"default".as_ptr(),
            "default".len() as u32,
        )
    };
    let ns = object_ptr_from_value(value).expect("default export should be an object");
    let ns_value = boxed_ptr(ns as *const u8);

    assert!(get_object_property(ns_value, b"pipeline").is_some());
    assert!(get_object_property(ns_value, b"finished").is_some());
    assert_eq!(
        get_object_property(ns_value, b"default").unwrap().to_bits(),
        ns_value.to_bits()
    );
}

#[test]
fn namespace_member_missing_export_returns_undefined() {
    let value = unsafe {
        js_node_submodule_namespace_member(
            b"diagnostics_channel".as_ptr(),
            "diagnostics_channel".len() as u32,
            b"notARealExport".as_ptr(),
            "notARealExport".len() as u32,
        )
    };

    assert_eq!(value.to_bits(), crate::value::TAG_UNDEFINED);
}

#[test]
fn direct_missing_export_keeps_legacy_true_sentinel() {
    let value = unsafe {
        js_node_submodule_export_as_function(
            b"diagnostics_channel".as_ptr(),
            "diagnostics_channel".len() as u32,
            b"notARealExport".as_ptr(),
            "notARealExport".len() as u32,
        )
    };

    assert_eq!(value.to_bits(), crate::value::TAG_TRUE);
}

#[test]
fn sys_format_export_reuses_util_callable() {
    let sys_format = unsafe {
        js_node_submodule_export_as_function(
            b"sys".as_ptr(),
            "sys".len() as u32,
            b"format".as_ptr(),
            "format".len() as u32,
        )
    };
    let util_format = unsafe {
        crate::object::js_native_module_property_by_name(
            b"util".as_ptr(),
            "util".len(),
            b"format".as_ptr(),
            "format".len(),
        )
    };

    assert_eq!(sys_format.to_bits(), util_format.to_bits());
}

#[test]
fn sys_namespace_reuses_util_callable() {
    let value = unsafe { js_node_submodule_namespace(b"sys".as_ptr(), "sys".len() as u32) };
    let ns = object_ptr_from_value(value).expect("sys namespace should be an object");
    let ns_value = boxed_ptr(ns as *const u8);
    let sys_inspect = get_object_property(ns_value, b"inspect").unwrap();
    let util_inspect = unsafe {
        crate::object::js_native_module_property_by_name(
            b"util".as_ptr(),
            "util".len(),
            b"inspect".as_ptr(),
            "inspect".len(),
        )
    };

    assert_eq!(sys_inspect.to_bits(), util_inspect.to_bits());
}

#[test]
fn sys_default_export_is_util_default_export() {
    // `sys` is a deprecated alias of `util`, so its CJS default export must
    // be pointer-identical to `util`'s default export. In Node:
    //   import sysDefault from "node:sys";  import utilDefault from "node:util";
    //   sysDefault === utilDefault           // true
    //   sysDefault === (util namespace)      // false
    // (#3741 made `util`'s default export a distinct synthetic namespace
    // object rather than the `util` namespace itself; `sys` must follow it.)
    let sys_default = unsafe {
        js_node_submodule_export_as_function(
            b"sys".as_ptr(),
            "sys".len() as u32,
            b"default".as_ptr(),
            "default".len() as u32,
        )
    };
    let util_default = unsafe {
        crate::object::js_native_module_property_by_name(
            b"util".as_ptr(),
            "util".len(),
            b"default".as_ptr(),
            "default".len(),
        )
    };

    assert_eq!(sys_default.to_bits(), util_default.to_bits());
}

#[test]
fn sys_namespace_types_member_reuses_util_types() {
    let sys_types = unsafe {
        js_node_submodule_namespace_member(
            b"sys".as_ptr(),
            "sys".len() as u32,
            b"types".as_ptr(),
            "types".len() as u32,
        )
    };
    let util_types = unsafe {
        crate::object::js_native_module_property_by_name(
            b"util".as_ptr(),
            "util".len(),
            b"types".as_ptr(),
            "types".len(),
        )
    };

    assert_eq!(sys_types.to_bits(), util_types.to_bits());
}

#[test]
fn stream_promises_finished_resolves_for_finished_writable_side_stub_stream() {
    let stream = crate::node_stream::js_node_stream_passthrough_new(undefined_value());
    let end = get_object_property(stream, b"end").expect("stream.end should exist");
    let prev_this = crate::object::js_implicit_this_set(stream);
    unsafe {
        let _ = crate::closure::js_native_call_value(end, std::ptr::null(), 0);
    }
    crate::object::js_implicit_this_set(prev_this);
    let _ = crate::promise::js_promise_run_microtasks();

    let opts = js_object_alloc(0, 1);
    js_object_set_field_by_name(
        opts,
        js_string_from_bytes(b"readable".as_ptr(), 8),
        f64::from_bits(crate::value::TAG_FALSE),
    );

    let promise_value =
        thunk_streamP_finished(std::ptr::null(), stream, boxed_ptr(opts as *const u8));
    let promise = promise_ptr(promise_value);

    assert_eq!(crate::promise::js_promise_state(promise), 1);
    assert_eq!(
        crate::promise::js_promise_value(promise).to_bits(),
        crate::value::TAG_UNDEFINED
    );
}

#[test]
fn stream_promises_finished_rejects_invalid_inputs() {
    let number_promise = thunk_streamP_finished(std::ptr::null(), 123.0, undefined_value());
    assert_rejected_node_error(
        number_promise,
        "ERR_INVALID_ARG_TYPE",
        "The \"stream\" argument must be an instance of ReadableStream, WritableStream, or Stream. Received type number (123)",
    );

    let string_promise =
        thunk_streamP_finished(std::ptr::null(), string_value("x"), undefined_value());
    assert_rejected_node_error(
        string_promise,
        "ERR_INVALID_ARG_TYPE",
        "The \"stream\" argument must be an instance of ReadableStream, WritableStream, or Stream. Received type string ('x')",
    );

    let object = js_object_alloc(0, 0);
    let object_promise = thunk_streamP_finished(
        std::ptr::null(),
        boxed_ptr(object as *const u8),
        undefined_value(),
    );
    assert_rejected_node_error(
        object_promise,
        "ERR_INVALID_ARG_TYPE",
        "The \"stream\" argument must be an instance of ReadableStream, WritableStream, or Stream. Received an instance of Object",
    );
}

#[test]
fn stream_promises_finished_rejects_hidden_stream_error() {
    let stream = crate::node_stream::js_node_stream_passthrough_new(undefined_value());
    let err = abort_error_value();
    crate::node_stream::test_set_hidden_error(stream, err);

    let promise_value = thunk_streamP_finished(std::ptr::null(), stream, undefined_value());
    let promise = promise_ptr(promise_value);

    assert_eq!(crate::promise::js_promise_state(promise), 2);
    assert_eq!(
        crate::promise::js_promise_reason(promise).to_bits(),
        err.to_bits()
    );
}

#[test]
fn stream_promises_finished_rejects_later_destroy_error() {
    let stream = crate::node_stream::js_node_stream_readable_new(undefined_value());
    crate::node_stream::test_install_manual_read(stream);
    let promise_value = thunk_streamP_finished(std::ptr::null(), stream, undefined_value());
    let promise = promise_ptr(promise_value);

    assert_eq!(crate::promise::js_promise_state(promise), 0);

    let err = string_value("later-error");
    let handle = object_ptr_from_value(stream).expect("stream object") as i64;
    let _ = crate::node_stream::js_node_stream_method_destroy(handle, err);
    let _ = crate::promise::js_promise_run_microtasks();

    assert_eq!(crate::promise::js_promise_state(promise), 2);
    assert_eq!(
        crate::promise::js_promise_reason(promise).to_bits(),
        err.to_bits()
    );
}

fn set_stream_bool_property(stream: f64, name: &[u8], value: bool) {
    let obj = object_ptr_from_value(stream).expect("stream object");
    js_object_set_field_by_name(
        obj,
        js_string_from_bytes(name.as_ptr(), name.len() as u32),
        f64::from_bits(if value {
            crate::value::TAG_TRUE
        } else {
            crate::value::TAG_FALSE
        }),
    );
}

fn emit_stream_lifecycle_event(stream: f64, name: &str) {
    let handle = object_ptr_from_value(stream).expect("stream object") as i64;
    let _ = crate::node_stream::js_node_stream_method_emit(
        handle,
        string_value(name),
        undefined_value(),
    );
}

fn finished_bool_option(name: &[u8], value: bool) -> f64 {
    let opts = js_object_alloc(0, 1);
    js_object_set_field_by_name(
        opts,
        js_string_from_bytes(name.as_ptr(), name.len() as u32),
        f64::from_bits(if value {
            crate::value::TAG_TRUE
        } else {
            crate::value::TAG_FALSE
        }),
    );
    boxed_ptr(opts as *const u8)
}

fn assert_promise_resolved_undefined(promise: *mut crate::promise::Promise) {
    assert_eq!(crate::promise::js_promise_state(promise), 1);
    assert_eq!(
        crate::promise::js_promise_value(promise).to_bits(),
        crate::value::TAG_UNDEFINED
    );
}

#[test]
fn stream_promises_finished_resolves_for_resumed_readable_from() {
    let mut arr = crate::array::js_array_alloc(1);
    arr = crate::array::js_array_push_f64(arr, string_value("x"));
    let stream = crate::node_stream::js_node_stream_readable_from(boxed_ptr(arr as *const u8));
    let handle = object_ptr_from_value(stream).expect("stream object") as i64;
    let _ = crate::node_stream::js_node_stream_method_resume(handle);

    let promise_value = thunk_streamP_finished(std::ptr::null(), stream, undefined_value());
    let promise = promise_ptr(promise_value);
    assert_eq!(crate::promise::js_promise_state(promise), 0);

    let _ = crate::promise::js_promise_run_microtasks();

    assert_promise_resolved_undefined(promise);
}

#[test]
fn stream_promises_finished_duplex_default_waits_after_writable_finish_only() {
    let stream = crate::node_stream::js_node_stream_duplex_new(undefined_value());
    let promise_value = thunk_streamP_finished(std::ptr::null(), stream, undefined_value());
    let promise = promise_ptr(promise_value);

    assert_eq!(crate::promise::js_promise_state(promise), 0);

    set_stream_bool_property(stream, b"writableFinished", true);
    emit_stream_lifecycle_event(stream, "finish");

    assert_eq!(crate::promise::js_promise_state(promise), 0);

    set_stream_bool_property(stream, b"readableEnded", true);
    emit_stream_lifecycle_event(stream, "end");

    assert_promise_resolved_undefined(promise);
}

#[test]
fn stream_promises_finished_duplex_default_waits_after_readable_end_only() {
    let stream = crate::node_stream::js_node_stream_duplex_new(undefined_value());
    let promise_value = thunk_streamP_finished(std::ptr::null(), stream, undefined_value());
    let promise = promise_ptr(promise_value);

    assert_eq!(crate::promise::js_promise_state(promise), 0);

    set_stream_bool_property(stream, b"readableEnded", true);
    emit_stream_lifecycle_event(stream, "end");

    assert_eq!(crate::promise::js_promise_state(promise), 0);

    set_stream_bool_property(stream, b"writableFinished", true);
    emit_stream_lifecycle_event(stream, "finish");

    assert_promise_resolved_undefined(promise);
}

#[test]
fn stream_promises_finished_duplex_readable_false_resolves_after_writable_finish() {
    let stream = crate::node_stream::js_node_stream_duplex_new(undefined_value());
    let options = finished_bool_option(b"readable", false);
    let promise_value = thunk_streamP_finished(std::ptr::null(), stream, options);
    let promise = promise_ptr(promise_value);

    assert_eq!(crate::promise::js_promise_state(promise), 0);

    set_stream_bool_property(stream, b"writableFinished", true);
    emit_stream_lifecycle_event(stream, "finish");

    assert_promise_resolved_undefined(promise);
}

#[test]
fn stream_promises_finished_duplex_writable_false_resolves_after_readable_end() {
    let stream = crate::node_stream::js_node_stream_duplex_new(undefined_value());
    let options = finished_bool_option(b"writable", false);
    let promise_value = thunk_streamP_finished(std::ptr::null(), stream, options);
    let promise = promise_ptr(promise_value);

    assert_eq!(crate::promise::js_promise_state(promise), 0);

    set_stream_bool_property(stream, b"readableEnded", true);
    emit_stream_lifecycle_event(stream, "end");

    assert_promise_resolved_undefined(promise);
}

thread_local! {
    static PIPELINE_CAPTURED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

extern "C" fn pipeline_write_capture(_closure: *const ClosureHeader, chunk: f64, _enc: f64) -> f64 {
    PIPELINE_CAPTURED.with(|captured| {
        captured
            .borrow_mut()
            .push(string_from_value(chunk).unwrap_or_default());
    });
    f64::from_bits(crate::value::TAG_TRUE)
}

extern "C" fn pipeline_end_capture(_closure: *const ClosureHeader, _arg: f64) -> f64 {
    undefined_value()
}

#[test]
fn stream_promises_pipeline_transfers_readable_from_chunks() {
    PIPELINE_CAPTURED.with(|captured| captured.borrow_mut().clear());
    crate::closure::js_register_closure_arity(pipeline_write_capture as *const u8, 2);
    crate::closure::js_register_closure_arity(pipeline_end_capture as *const u8, 1);

    let mut arr = crate::array::js_array_alloc(2);
    arr = crate::array::js_array_push_f64(arr, string_value("await-"));
    arr = crate::array::js_array_push_f64(arr, string_value("works"));
    let source = crate::node_stream::js_node_stream_readable_from(boxed_ptr(arr as *const u8));

    let sink = js_object_alloc(0, 2);
    let write = js_closure_alloc(pipeline_write_capture as *const u8, 0);
    let end = js_closure_alloc(pipeline_end_capture as *const u8, 0);
    js_object_set_field_by_name(
        sink,
        js_string_from_bytes(b"write".as_ptr(), 5),
        boxed_ptr(write as *const u8),
    );
    js_object_set_field_by_name(
        sink,
        js_string_from_bytes(b"end".as_ptr(), 3),
        boxed_ptr(end as *const u8),
    );

    let promise_value = thunk_streamP_pipeline(
        std::ptr::null(),
        source,
        boxed_ptr(sink as *const u8),
        undefined_value(),
    );
    let promise = promise_ptr(promise_value);

    assert_eq!(crate::promise::js_promise_state(promise), 1);
    PIPELINE_CAPTURED.with(|captured| {
        assert_eq!(captured.borrow().join(""), "await-works");
    });
}

#[test]
fn stream_promises_pipeline_rejects_missing_streams() {
    let direct_promise = thunk_streamP_pipeline(
        std::ptr::null(),
        undefined_value(),
        undefined_value(),
        undefined_value(),
    );
    assert_rejected_node_error(
        direct_promise,
        "ERR_MISSING_ARGS",
        "The \"streams\" argument must be specified",
    );

    let empty_rest = crate::array::js_array_alloc(0);
    let rest_promise = thunk_streamP_pipeline(
        std::ptr::null(),
        123.0,
        undefined_value(),
        boxed_ptr(empty_rest as *const u8),
    );
    assert_rejected_node_error(
        rest_promise,
        "ERR_MISSING_ARGS",
        "The \"streams\" argument must be specified",
    );
}

#[test]
fn stream_promises_pipeline_rejects_invalid_source_body() {
    let direct_promise = thunk_streamP_pipeline(std::ptr::null(), 123.0, 456.0, undefined_value());
    assert_rejected_node_error(
        direct_promise,
        "ERR_INVALID_ARG_TYPE",
        "The \"body\" argument must be of type function or an instance of Blob, ReadableStream, WritableStream, Stream, Iterable, AsyncIterable, or Promise or { readable, writable } pair. Received type number (123)",
    );

    let empty_rest = crate::array::js_array_alloc(0);
    let rest_promise = thunk_streamP_pipeline(
        std::ptr::null(),
        123.0,
        456.0,
        boxed_ptr(empty_rest as *const u8),
    );
    assert_rejected_node_error(
        rest_promise,
        "ERR_INVALID_ARG_TYPE",
        "The \"body\" argument must be of type function or an instance of Blob, ReadableStream, WritableStream, Stream, Iterable, AsyncIterable, or Promise or { readable, writable } pair. Received type number (123)",
    );
}

#[test]
fn stream_promises_finished_rejects_when_signal_aborts() {
    let controller = crate::url::js_abort_controller_new();
    let signal = crate::url::js_abort_controller_signal(controller);
    let opts = js_object_alloc(0, 1);
    js_object_set_field_by_name(
        opts,
        js_string_from_bytes(b"signal".as_ptr(), 6),
        boxed_ptr(signal as *const u8),
    );
    let stream = crate::node_stream::js_node_stream_passthrough_new(undefined_value());

    let promise_value =
        thunk_streamP_finished(std::ptr::null(), stream, boxed_ptr(opts as *const u8));
    let promise = promise_ptr(promise_value);
    assert_eq!(crate::promise::js_promise_state(promise), 0);

    crate::url::js_abort_controller_abort(controller);

    assert_eq!(crate::promise::js_promise_state(promise), 2);
}

#[test]
fn stream_promises_finished_with_signal_resolves_for_finished_writable_side_stub_stream() {
    let controller = crate::url::js_abort_controller_new();
    let signal = crate::url::js_abort_controller_signal(controller);
    let opts = js_object_alloc(0, 2);
    js_object_set_field_by_name(
        opts,
        js_string_from_bytes(b"signal".as_ptr(), 6),
        boxed_ptr(signal as *const u8),
    );
    js_object_set_field_by_name(
        opts,
        js_string_from_bytes(b"readable".as_ptr(), 8),
        f64::from_bits(crate::value::TAG_FALSE),
    );
    let stream = crate::node_stream::js_node_stream_passthrough_new(undefined_value());
    let end = get_object_property(stream, b"end").expect("stream.end should exist");
    let prev_this = crate::object::js_implicit_this_set(stream);
    unsafe {
        let _ = crate::closure::js_native_call_value(end, std::ptr::null(), 0);
    }
    crate::object::js_implicit_this_set(prev_this);
    let _ = crate::promise::js_promise_run_microtasks();

    let promise_value =
        thunk_streamP_finished(std::ptr::null(), stream, boxed_ptr(opts as *const u8));
    let promise = promise_ptr(promise_value);

    assert_eq!(crate::promise::js_promise_state(promise), 1);
    assert_eq!(
        crate::promise::js_promise_value(promise).to_bits(),
        crate::value::TAG_UNDEFINED
    );
}
