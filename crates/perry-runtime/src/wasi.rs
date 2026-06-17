//! Minimal `node:wasi` surface for constructor/import-object and lifecycle parity.
//!
//! This intentionally validates lifecycle state and WASI instance export shape
//! without attempting full WASI syscall fidelity.

use crate::closure::ClosureHeader;
use crate::object::ObjectHeader;
use crate::string::StringHeader;
use crate::value::{JSValue, TAG_UNDEFINED};

use std::sync::atomic::{AtomicBool, Ordering};

pub const CLASS_ID_WASI: u32 = 0xFFFF_00B2;
const CLASS_ID_WASI_IMPORT_PREVIEW1: u32 = 0xFFFF_00B3;
const CLASS_ID_WASI_IMPORT_UNSTABLE: u32 = 0xFFFF_00B4;
const FIELD_WASI_IMPORT: &str = "wasiImport";
const FIELD_WASI_STARTED: &str = "__wasiStarted";

static WASI_PROTOTYPE_INITIALIZED: AtomicBool = AtomicBool::new(false);

const WASI_IMPORT_NAMES: &[&str] = &[
    "args_get",
    "args_sizes_get",
    "clock_res_get",
    "clock_time_get",
    "environ_get",
    "environ_sizes_get",
    "fd_advise",
    "fd_allocate",
    "fd_close",
    "fd_datasync",
    "fd_fdstat_get",
    "fd_fdstat_set_flags",
    "fd_fdstat_set_rights",
    "fd_filestat_get",
    "fd_filestat_set_size",
    "fd_filestat_set_times",
    "fd_pread",
    "fd_prestat_get",
    "fd_prestat_dir_name",
    "fd_pwrite",
    "fd_read",
    "fd_readdir",
    "fd_renumber",
    "fd_seek",
    "fd_sync",
    "fd_tell",
    "fd_write",
    "path_create_directory",
    "path_filestat_get",
    "path_filestat_set_times",
    "path_link",
    "path_open",
    "path_readlink",
    "path_remove_directory",
    "path_rename",
    "path_symlink",
    "path_unlink_file",
    "poll_oneoff",
    "proc_exit",
    "proc_raise",
    "random_get",
    "sched_yield",
    "sock_accept",
    "sock_recv",
    "sock_send",
    "sock_shutdown",
];

fn ptr_value(ptr: *mut ObjectHeader) -> f64 {
    f64::from_bits(JSValue::pointer(ptr as *const u8).bits())
}

fn undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn bool_value(value: bool) -> f64 {
    f64::from_bits(JSValue::bool(value).bits())
}

fn is_undefined(value: f64) -> bool {
    JSValue::from_bits(value.to_bits()).is_undefined()
}

fn named_key(name: &[u8]) -> *mut StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn heap_object_ptr(value: f64) -> Option<*mut ObjectHeader> {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_pointer() {
        return None;
    }
    let ptr = jsval.as_pointer::<u8>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    unsafe {
        let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type == crate::gc::GC_TYPE_OBJECT {
            Some(ptr as *mut ObjectHeader)
        } else {
            None
        }
    }
}

fn is_array_value(value: f64) -> bool {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_pointer() {
        return false;
    }
    let ptr = jsval.as_pointer::<u8>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return false;
    }
    unsafe {
        let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        matches!(
            (*gc_header).obj_type,
            crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_LAZY_ARRAY
        )
    }
}

fn is_object_value(value: f64) -> bool {
    heap_object_ptr(value).is_some()
}

fn option_field(options: Option<*mut ObjectHeader>, name: &[u8]) -> f64 {
    options
        .map(|obj| crate::object::js_object_get_field_by_name_f64(obj, named_key(name)))
        .unwrap_or_else(undefined)
}

fn type_error_with_code(message: &str, code: &'static str) -> ! {
    crate::fs::validate::throw_type_error_with_code(message, code)
}

fn invalid_arg_type(message: &str) -> ! {
    type_error_with_code(message, "ERR_INVALID_ARG_TYPE")
}

fn invalid_type(property: &str, expected: &str, value: f64) -> ! {
    let message = format!(
        "The \"{property}\" property must be of type {expected}. Received {}",
        crate::fs::validate::describe_received(value)
    );
    invalid_arg_type(&message)
}

fn invalid_undefined_property(property: &str, value: f64) -> ! {
    let message = format!(
        "The \"{property}\" property must be undefined. Received {}",
        crate::fs::validate::describe_received(value)
    );
    invalid_arg_type(&message)
}

fn invalid_wasm_memory() -> ! {
    invalid_arg_type("\"instance.exports.memory\" property must be a WebAssembly.Memory object")
}

fn invalid_options(value: f64) -> ! {
    let message = format!(
        "The \"options\" argument must be of type object. Received {}",
        crate::fs::validate::describe_received(value)
    );
    type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn validate_optional_array(options: Option<*mut ObjectHeader>, name: &[u8], label: &str) {
    let value = option_field(options, name);
    if JSValue::from_bits(value.to_bits()).is_undefined() {
        return;
    }
    if !is_array_value(value) {
        invalid_type(label, "Array", value);
    }
}

fn validate_optional_object(options: Option<*mut ObjectHeader>, name: &[u8], label: &str) {
    let value = option_field(options, name);
    if JSValue::from_bits(value.to_bits()).is_undefined() {
        return;
    }
    if !is_object_value(value) {
        invalid_type(label, "object", value);
    }
}

fn validate_optional_bool(options: Option<*mut ObjectHeader>, name: &[u8], label: &str) {
    let value = option_field(options, name);
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_undefined() {
        return;
    }
    if !jsval.is_bool() {
        invalid_type(label, "boolean", value);
    }
}

fn validate_optional_fd(options: Option<*mut ObjectHeader>, name: &[u8], label: &str) {
    let value = option_field(options, name);
    if JSValue::from_bits(value.to_bits()).is_undefined() {
        return;
    }
    crate::fs::validate::validate_int32(value, label, 0, i32::MAX as i64);
}

fn validate_options(options: f64) -> (&'static str, u32) {
    let options_js = JSValue::from_bits(options.to_bits());
    let options_obj = if options_js.is_undefined() {
        None
    } else {
        heap_object_ptr(options).or_else(|| {
            invalid_options(options);
        })
    };

    let version_value = option_field(options_obj, b"version");
    let Some(version) = crate::builtins::jsvalue_string_content(version_value) else {
        invalid_type("options.version", "string", version_value);
    };
    let (binding_name, import_class_id) = match version.as_str() {
        "preview1" => ("wasi_snapshot_preview1", CLASS_ID_WASI_IMPORT_PREVIEW1),
        "unstable" => ("wasi_unstable", CLASS_ID_WASI_IMPORT_UNSTABLE),
        _ => {
            let message = format!(
                "The property 'options.version' unsupported WASI version. Received '{}'",
                version
            );
            type_error_with_code(&message, "ERR_INVALID_ARG_VALUE");
        }
    };

    validate_optional_array(options_obj, b"args", "options.args");
    validate_optional_object(options_obj, b"env", "options.env");
    validate_optional_object(options_obj, b"preopens", "options.preopens");
    validate_optional_bool(options_obj, b"returnOnExit", "options.returnOnExit");
    validate_optional_fd(options_obj, b"stdin", "options.stdin");
    validate_optional_fd(options_obj, b"stdout", "options.stdout");
    validate_optional_fd(options_obj, b"stderr", "options.stderr");

    (binding_name, import_class_id)
}

fn closure_value(func_ptr: *const u8, name: &str, arity: u32) -> f64 {
    crate::closure::js_register_closure_arity(func_ptr, arity);
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    crate::object::set_bound_native_closure_name(closure, name);
    crate::object::set_builtin_closure_length(closure as usize, arity);
    crate::value::js_nanbox_pointer(closure as i64)
}

fn create_import_function(name: &str) -> f64 {
    closure_value(js_wasi_import_stub as *const u8, name, 4)
}

fn create_import_object(class_id: u32) -> *mut ObjectHeader {
    let obj = crate::object::js_object_alloc(class_id, 0);
    for name in WASI_IMPORT_NAMES {
        crate::object::js_object_set_field_by_name(
            obj,
            named_key(name.as_bytes()),
            create_import_function(name),
        );
    }
    obj
}

fn import_binding_name(import_obj: *mut ObjectHeader) -> &'static str {
    if import_obj.is_null() {
        return "wasi_snapshot_preview1";
    }
    unsafe {
        if (*import_obj).class_id == CLASS_ID_WASI_IMPORT_UNSTABLE {
            "wasi_unstable"
        } else {
            "wasi_snapshot_preview1"
        }
    }
}

fn ensure_wasi_prototype() {
    if WASI_PROTOTYPE_INITIALIZED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    let keys = b"constructor\0getImportObject\0start\0initialize\0finalizeBindings\0";
    let proto =
        crate::object::js_object_alloc_with_shape(0x7FFF_FF41, 5, keys.as_ptr(), keys.len() as u32);
    crate::object::js_object_set_field(
        proto,
        1,
        JSValue::from_bits(
            closure_value(js_wasi_get_import_object as *const u8, "getImportObject", 0).to_bits(),
        ),
    );
    crate::object::js_object_set_field(
        proto,
        2,
        JSValue::from_bits(closure_value(js_wasi_start as *const u8, "start", 1).to_bits()),
    );
    crate::object::js_object_set_field(
        proto,
        3,
        JSValue::from_bits(
            closure_value(js_wasi_initialize as *const u8, "initialize", 1).to_bits(),
        ),
    );
    crate::object::js_object_set_field(
        proto,
        4,
        JSValue::from_bits(
            closure_value(
                js_wasi_finalize_bindings as *const u8,
                "finalizeBindings",
                1,
            )
            .to_bits(),
        ),
    );
    crate::object::class_prototype_object_root_store(CLASS_ID_WASI, proto);
}

pub(crate) fn attach_wasi_constructor_prototype(constructor_value: f64) {
    ensure_wasi_prototype();
    let proto = crate::object::class_prototype_object(CLASS_ID_WASI);
    if proto.is_null() {
        return;
    }
    crate::object::js_object_set_field(proto, 0, JSValue::from_bits(constructor_value.to_bits()));
    crate::closure::closure_set_dynamic_prop(
        (constructor_value.to_bits() & crate::value::POINTER_MASK) as usize,
        "prototype",
        crate::value::js_nanbox_pointer(proto as i64),
    );
}

pub(crate) fn is_wasi_instance(value: f64) -> bool {
    let Some(obj) = heap_object_ptr(value) else {
        return false;
    };
    unsafe { (*obj).class_id == CLASS_ID_WASI }
}

#[no_mangle]
pub extern "C" fn js_wasi_constructor_call(_options: f64) -> f64 {
    let message = "Class constructor WASI cannot be invoked without 'new'";
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

#[no_mangle]
pub extern "C" fn js_wasi_new(options: f64) -> f64 {
    let (_, import_class_id) = validate_options(options);
    ensure_wasi_prototype();
    let import_obj = create_import_object(import_class_id);
    let keys = b"wasiImport\0__wasiStarted\0";
    let obj = crate::object::js_object_alloc_class_with_keys(
        CLASS_ID_WASI,
        0,
        2,
        keys.as_ptr(),
        keys.len() as u32,
    );
    crate::object::js_object_set_field(obj, 0, JSValue::from_bits(ptr_value(import_obj).to_bits()));
    crate::object::js_object_set_field(obj, 1, JSValue::from_bits(bool_value(false).to_bits()));
    crate::object::set_builtin_property_attrs(
        obj as usize,
        FIELD_WASI_STARTED.to_string(),
        crate::object::PropertyAttrs::new(true, false, true),
    );
    ptr_value(obj)
}

#[no_mangle]
pub extern "C" fn js_wasi_get_import_object(_closure: *const ClosureHeader) -> f64 {
    let this = crate::object::js_implicit_this_get();
    let Some(obj) = heap_object_ptr(this) else {
        invalid_options(this);
    };
    let import_value = crate::object::js_object_get_field_by_name_f64(
        obj,
        named_key(FIELD_WASI_IMPORT.as_bytes()),
    );
    let Some(import_obj) = heap_object_ptr(import_value) else {
        return undefined();
    };
    let binding_name = import_binding_name(import_obj);
    let wrapper = crate::object::js_object_alloc(0, 0);
    crate::object::js_object_set_field_by_name(
        wrapper,
        named_key(binding_name.as_bytes()),
        import_value,
    );
    ptr_value(wrapper)
}

#[no_mangle]
pub extern "C" fn js_wasi_start(_closure: *const ClosureHeader, instance: f64) -> f64 {
    let wasi = wasi_receiver_or_throw();
    ensure_wasi_not_started(wasi);
    let exports = validate_instance_exports(instance);
    validate_start_exports(exports);
    mark_wasi_started(wasi);
    0.0
}

#[no_mangle]
pub extern "C" fn js_wasi_initialize(_closure: *const ClosureHeader, instance: f64) -> f64 {
    let wasi = wasi_receiver_or_throw();
    ensure_wasi_not_started(wasi);
    let exports = validate_instance_exports(instance);
    validate_initialize_exports(exports);
    mark_wasi_started(wasi);
    undefined()
}

#[no_mangle]
pub extern "C" fn js_wasi_finalize_bindings(_closure: *const ClosureHeader, instance: f64) -> f64 {
    let wasi = wasi_receiver_or_throw();
    ensure_wasi_not_started(wasi);
    let exports = validate_instance_exports(instance);
    validate_memory_export(exports);
    mark_wasi_started(wasi);
    undefined()
}

fn wasi_receiver_or_throw() -> *mut ObjectHeader {
    let this = crate::object::js_implicit_this_get();
    let Some(obj) = heap_object_ptr(this) else {
        type_error_with_code("Value of \"this\" must be of type WASI", "ERR_INVALID_THIS");
    };
    unsafe {
        if (*obj).class_id != CLASS_ID_WASI {
            type_error_with_code("Value of \"this\" must be of type WASI", "ERR_INVALID_THIS");
        }
    }
    obj
}

fn wasi_started(obj: *mut ObjectHeader) -> bool {
    let value = crate::object::js_object_get_field_by_name_f64(
        obj,
        named_key(FIELD_WASI_STARTED.as_bytes()),
    );
    let jsval = JSValue::from_bits(value.to_bits());
    jsval.is_bool() && jsval.as_bool()
}

fn ensure_wasi_not_started(obj: *mut ObjectHeader) {
    if wasi_started(obj) {
        crate::fs::validate::throw_error_with_code(
            "WASI instance has already started",
            "ERR_WASI_ALREADY_STARTED",
        );
    }
}

fn mark_wasi_started(obj: *mut ObjectHeader) {
    crate::object::js_object_set_field_by_name(
        obj,
        named_key(FIELD_WASI_STARTED.as_bytes()),
        bool_value(true),
    );
    crate::object::set_builtin_property_attrs(
        obj as usize,
        FIELD_WASI_STARTED.to_string(),
        crate::object::PropertyAttrs::new(true, false, true),
    );
}

fn validate_instance_arg(instance: f64) -> *mut ObjectHeader {
    let Some(obj) = heap_object_ptr(instance) else {
        let message = format!(
            "The \"instance\" argument must be of type object. Received {}",
            crate::fs::validate::describe_received(instance)
        );
        invalid_arg_type(&message);
    };
    obj
}

fn validate_instance_exports(instance: f64) -> *mut ObjectHeader {
    let instance_obj = validate_instance_arg(instance);
    let exports =
        crate::object::js_object_get_field_by_name_f64(instance_obj, named_key(b"exports"));
    let Some(exports_obj) = heap_object_ptr(exports) else {
        let message = format!(
            "The \"instance.exports\" property must be of type object. Received {}",
            crate::fs::validate::describe_received(exports)
        );
        invalid_arg_type(&message);
    };
    exports_obj
}

fn export_value(exports: *mut ObjectHeader, name: &[u8]) -> f64 {
    crate::object::js_object_get_field_by_name_f64(exports, named_key(name))
}

fn is_callable_value(value: f64) -> bool {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_pointer() {
        return false;
    }
    let ptr = jsval.as_pointer::<u8>() as usize;
    crate::closure::is_closure_ptr(ptr)
}

fn validate_memory_export(exports: *mut ObjectHeader) {
    let memory = export_value(exports, b"memory");
    if !is_object_value(memory) {
        invalid_wasm_memory();
    }
}

fn validate_start_exports(exports: *mut ObjectHeader) {
    validate_memory_export(exports);
    let start = export_value(exports, b"_start");
    if !is_callable_value(start) {
        invalid_type("instance.exports._start", "function", start);
    }
    let initialize = export_value(exports, b"_initialize");
    if !is_undefined(initialize) {
        invalid_undefined_property("instance.exports._initialize", initialize);
    }
}

fn validate_initialize_exports(exports: *mut ObjectHeader) {
    validate_memory_export(exports);
    let start = export_value(exports, b"_start");
    if !is_undefined(start) {
        invalid_undefined_property("instance.exports._start", start);
    }
}

#[no_mangle]
pub extern "C" fn js_wasi_import_stub(
    _closure: *const ClosureHeader,
    _arg0: f64,
    _arg1: f64,
    _arg2: f64,
    _arg3: f64,
) -> f64 {
    28.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_name_count_matches_node_preview1_surface() {
        assert_eq!(WASI_IMPORT_NAMES.len(), 46);
        assert!(WASI_IMPORT_NAMES.contains(&"args_get"));
        assert!(WASI_IMPORT_NAMES.contains(&"fd_write"));
        assert!(WASI_IMPORT_NAMES.contains(&"random_get"));
        assert!(WASI_IMPORT_NAMES.contains(&"sock_shutdown"));
    }
}
