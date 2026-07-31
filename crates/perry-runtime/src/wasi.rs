//! Minimal `node:wasi` surface for constructor/import-object and lifecycle parity.
//!
//! This intentionally validates lifecycle state and WASI instance export shape
//! without attempting full WASI syscall fidelity.

use crate::closure::ClosureHeader;
use crate::object::ObjectHeader;
use crate::string::StringHeader;
use crate::value::{JSValue, TAG_UNDEFINED};

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

thread_local! {
    static WASI_EXIT_CODE: Cell<Option<i32>> = const { Cell::new(None) };
}

pub const CLASS_ID_WASI: u32 = 0xFFFF_00B2;
const CLASS_ID_WASI_IMPORT_PREVIEW1: u32 = 0xFFFF_00B3;
const CLASS_ID_WASI_IMPORT_UNSTABLE: u32 = 0xFFFF_00B4;
const FIELD_WASI_IMPORT: &str = "wasiImport";
const FIELD_WASI_STARTED: &str = "__wasiStarted";
const FIELD_WASI_MEMORY: &str = "__wasiMemory";
const FIELD_WASI_ARGS: &str = "__wasiArgs";
const FIELD_WASI_ENV: &str = "__wasiEnv";
const FIELD_WASI_RETURN_ON_EXIT: &str = "__wasiReturnOnExit";
const FIELD_WASI_BINDING: &str = "__wasiBinding";

static WASI_PROTOTYPE_INITIALIZED: AtomicBool = AtomicBool::new(false);
static WASI_WARNING_EMITTED: AtomicBool = AtomicBool::new(false);

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

fn string_value(ptr: *mut StringHeader) -> f64 {
    f64::from_bits(JSValue::string_ptr(ptr).bits())
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

fn validate_optional_fd(options: Option<*mut ObjectHeader>, name: &[u8], label: &str) {
    let value = option_field(options, name);
    if JSValue::from_bits(value.to_bits()).is_undefined() {
        return;
    }
    crate::fs::validate::validate_int32(value, label, 0, i32::MAX as i64);
}

struct ValidatedOptions {
    binding_name: &'static str,
    import_class_id: u32,
    args: *mut crate::array::ArrayHeader,
    env: *mut crate::array::ArrayHeader,
    return_on_exit: bool,
}

fn option_string(value: f64) -> Vec<u8> {
    let coerced = crate::builtins::js_string_coerce(value);
    let text = crate::builtins::jsvalue_string_content(string_value(coerced)).unwrap_or_default();
    text.into_bytes()
        .into_iter()
        .take_while(|byte| *byte != 0)
        .collect()
}

fn snapshot_args(value: f64) -> *mut crate::array::ArrayHeader {
    if is_undefined(value) {
        return crate::array::js_array_alloc(0);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let raw = JSValue::from_bits(value.get_nanbox_f64().to_bits())
        .as_pointer::<crate::array::ArrayHeader>();
    let len = crate::array::js_array_length(raw);
    let out = scope.root_nanbox_f64(ptr_value(
        crate::array::js_array_alloc(len) as *mut ObjectHeader
    ));
    for index in 0..len {
        let raw = JSValue::from_bits(value.get_nanbox_f64().to_bits())
            .as_pointer::<crate::array::ArrayHeader>();
        let item = scope.root_nanbox_f64(crate::array::js_array_get_f64(raw, index));
        let bytes = option_string(item.get_nanbox_f64());
        let string = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        let out_ptr = JSValue::from_bits(out.get_nanbox_f64().to_bits())
            .as_pointer::<crate::array::ArrayHeader>()
            as *mut crate::array::ArrayHeader;
        crate::array::js_array_push_f64(out_ptr, string_value(string));
    }
    JSValue::from_bits(out.get_nanbox_f64().to_bits()).as_pointer::<crate::array::ArrayHeader>()
        as *mut crate::array::ArrayHeader
}

fn snapshot_env(value: f64) -> *mut crate::array::ArrayHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let out = scope.root_nanbox_f64(ptr_value(
        crate::array::js_array_alloc(0) as *mut ObjectHeader
    ));
    if is_undefined(value) {
        return JSValue::from_bits(out.get_nanbox_f64().to_bits())
            .as_pointer::<crate::array::ArrayHeader>()
            as *mut crate::array::ArrayHeader;
    }
    let value = scope.root_nanbox_f64(value);
    let keys = scope.root_nanbox_f64(ptr_value(crate::object::js_object_keys_value(
        value.get_nanbox_f64(),
    ) as *mut ObjectHeader));
    for index in 0..crate::array::js_array_length(
        JSValue::from_bits(keys.get_nanbox_f64().to_bits())
            .as_pointer::<crate::array::ArrayHeader>(),
    ) {
        let keys_ptr = JSValue::from_bits(keys.get_nanbox_f64().to_bits())
            .as_pointer::<crate::array::ArrayHeader>();
        let key_value = crate::array::js_array_get_f64(keys_ptr, index);
        let Some(key) = crate::builtins::jsvalue_string_content(key_value) else {
            continue;
        };
        let Some(obj) = heap_object_ptr(value.get_nanbox_f64()) else {
            continue;
        };
        let field = crate::object::js_object_get_field_by_name_f64(obj, named_key(key.as_bytes()));
        if is_undefined(field) {
            continue;
        }
        let field = scope.root_nanbox_f64(field);
        let mut bytes = key.into_bytes();
        bytes.push(b'=');
        bytes.extend(option_string(field.get_nanbox_f64()));
        let string = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        let out_ptr = JSValue::from_bits(out.get_nanbox_f64().to_bits())
            .as_pointer::<crate::array::ArrayHeader>()
            as *mut crate::array::ArrayHeader;
        crate::array::js_array_push_f64(out_ptr, string_value(string));
    }
    JSValue::from_bits(out.get_nanbox_f64().to_bits()).as_pointer::<crate::array::ArrayHeader>()
        as *mut crate::array::ArrayHeader
}

fn validate_options(options: f64) -> ValidatedOptions {
    let options_js = JSValue::from_bits(options.to_bits());
    let options_obj = if options_js.is_undefined() {
        None
    } else {
        heap_object_ptr(options).or_else(|| invalid_options(options))
    };

    // Node materializes each configurable option while creating its internal
    // snapshot; preserve that observable getter order rather than caching a
    // single property read.
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
    let _ = option_field(options_obj, b"version");

    let args_validate = option_field(options_obj, b"args");
    if !is_undefined(args_validate) && !is_array_value(args_validate) {
        invalid_type("options.args", "Array", args_validate);
    }
    let _ = option_field(options_obj, b"args");
    let args = snapshot_args(option_field(options_obj, b"args"));

    let env_validate = option_field(options_obj, b"env");
    if !is_undefined(env_validate) && !is_object_value(env_validate) {
        invalid_type("options.env", "object", env_validate);
    }
    let _ = option_field(options_obj, b"env");
    let env = snapshot_env(option_field(options_obj, b"env"));

    let preopens_validate = option_field(options_obj, b"preopens");
    if !is_undefined(preopens_validate) && !is_object_value(preopens_validate) {
        invalid_type("options.preopens", "object", preopens_validate);
    }
    let _ = option_field(options_obj, b"preopens");
    let _ = option_field(options_obj, b"preopens");

    validate_optional_fd(options_obj, b"stdin", "options.stdin");
    validate_optional_fd(options_obj, b"stdout", "options.stdout");
    validate_optional_fd(options_obj, b"stderr", "options.stderr");

    let return_validate = option_field(options_obj, b"returnOnExit");
    if !is_undefined(return_validate) && !JSValue::from_bits(return_validate.to_bits()).is_bool() {
        invalid_type("options.returnOnExit", "boolean", return_validate);
    }
    let _ = option_field(options_obj, b"returnOnExit");
    let return_value = option_field(options_obj, b"returnOnExit");
    let return_on_exit = if is_undefined(return_value) {
        true
    } else {
        JSValue::from_bits(return_value.to_bits()).as_bool()
    };

    ValidatedOptions {
        binding_name,
        import_class_id,
        args,
        env,
        return_on_exit,
    }
}

fn closure_value(func_ptr: *const u8, name: &str, arity: u32) -> f64 {
    crate::closure::js_register_closure_arity(func_ptr, arity);
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    crate::object::set_bound_native_closure_name(closure, name);
    crate::object::set_builtin_closure_length(closure as usize, arity);
    crate::value::js_nanbox_pointer(closure as i64)
}

fn closure_rest_value(func_ptr: *const u8, name: &str, arity: u32) -> f64 {
    crate::closure::js_register_closure_rest(func_ptr, arity);
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    crate::object::set_bound_native_closure_name(closure, name);
    crate::object::set_builtin_closure_length(closure as usize, arity);
    crate::value::js_nanbox_pointer(closure as i64)
}

fn create_import_function(import_obj: *mut ObjectHeader, name: &str) -> f64 {
    let arity = match name {
        "args_get" | "args_sizes_get" | "environ_get" | "environ_sizes_get" | "clock_res_get"
        | "random_get" => 2,
        "clock_time_get" => 3,
        "proc_exit" => 1,
        _ => 4,
    };
    // The native thunk has four ABI arguments; keep dispatch from invoking it
    // through a shorter function pointer while exposing Node's public length.
    crate::closure::js_register_closure_arity(js_wasi_import_stub as *const u8, 4);
    let closure = crate::closure::js_closure_alloc(js_wasi_import_stub as *const u8, 2);
    crate::closure::js_closure_set_capture_ptr(closure, 0, import_obj as i64);
    crate::closure::js_closure_set_capture_f64(
        closure,
        1,
        WASI_IMPORT_NAMES
            .iter()
            .position(|candidate| *candidate == name)
            .unwrap_or_default() as f64,
    );
    let display_name = if name == "proc_exit" {
        "bound wasiReturnOnProcExit".to_string()
    } else {
        format!("bound {name}")
    };
    crate::object::set_bound_native_closure_name(closure, &display_name);
    crate::object::set_builtin_closure_length(closure as usize, arity);
    crate::object::set_builtin_closure_non_constructable(closure as usize);
    crate::value::js_nanbox_pointer(closure as i64)
}

fn create_import_object(class_id: u32) -> *mut ObjectHeader {
    let obj = crate::object::js_object_alloc(class_id, 0);
    for name in WASI_IMPORT_NAMES {
        crate::object::js_object_set_field_by_name(
            obj,
            named_key(name.as_bytes()),
            create_import_function(obj, name),
        );
    }
    obj
}

fn import_binding_name(import_obj: *mut ObjectHeader) -> &'static str {
    let binding = import_field(import_obj, FIELD_WASI_BINDING.as_bytes());
    if crate::builtins::jsvalue_string_content(binding).as_deref() == Some("wasi_unstable") {
        "wasi_unstable"
    } else {
        "wasi_snapshot_preview1"
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
            closure_rest_value(
                js_wasi_finalize_bindings as *const u8,
                "finalizeBindings",
                1,
            )
            .to_bits(),
        ),
    );
    for name in [
        "constructor",
        "getImportObject",
        "start",
        "initialize",
        "finalizeBindings",
    ] {
        crate::object::set_builtin_property_attrs(
            proto as usize,
            name.to_string(),
            crate::object::PropertyAttrs::new(true, false, true),
        );
    }
    crate::object::class_prototype_object_root_store(CLASS_ID_WASI, proto);
}

pub(crate) fn ensure_wasi_prototype_for_subclass() {
    ensure_wasi_prototype();
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
    crate::object::set_builtin_property_attrs(
        (constructor_value.to_bits() & crate::value::POINTER_MASK) as usize,
        "prototype".to_string(),
        crate::object::PropertyAttrs::new(false, false, false),
    );
}

fn emit_wasi_warning() {
    let message = b"WASI is an experimental feature and might change at any time";
    let kind = b"ExperimentalWarning";
    let message = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let kind = crate::string::js_string_from_bytes(kind.as_ptr(), kind.len() as u32);
    crate::process::js_process_emit_warning(
        f64::from_bits(JSValue::string_ptr(message).bits()),
        f64::from_bits(JSValue::string_ptr(kind).bits()),
        undefined(),
    );
}

pub(crate) fn emit_wasi_static_warning() {
    if WASI_WARNING_EMITTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        eprintln!(
            "(node:{}) ExperimentalWarning: WASI is an experimental feature and might change at any time",
            std::process::id()
        );
        eprintln!("(Use `node --trace-warnings ...` to show where the warning was created)");
    }
}

#[no_mangle]
pub extern "C" fn js_wasi_emit_warning() -> f64 {
    if WASI_WARNING_EMITTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        emit_wasi_warning();
    }
    undefined()
}

pub(crate) fn is_wasi_instance(value: f64) -> bool {
    let Some(obj) = heap_object_ptr(value) else {
        return false;
    };
    let class_id = unsafe { (*obj).class_id };
    class_id == CLASS_ID_WASI
        || crate::object::get_parent_class_id(class_id)
            .is_some_and(|parent| parent == CLASS_ID_WASI)
}

pub(crate) unsafe fn is_wasi_import_object(obj: *const ObjectHeader) -> bool {
    !obj.is_null()
        && matches!(
            (*obj).class_id,
            CLASS_ID_WASI_IMPORT_PREVIEW1 | CLASS_ID_WASI_IMPORT_UNSTABLE
        )
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
    let options = validate_options(options);
    ensure_wasi_prototype();
    let import_obj = create_import_object(options.import_class_id);
    set_import_field(
        import_obj,
        FIELD_WASI_ARGS.as_bytes(),
        ptr_value(options.args as *mut ObjectHeader),
    );
    set_import_field(
        import_obj,
        FIELD_WASI_ENV.as_bytes(),
        ptr_value(options.env as *mut ObjectHeader),
    );
    set_import_field(
        import_obj,
        FIELD_WASI_RETURN_ON_EXIT.as_bytes(),
        bool_value(options.return_on_exit),
    );
    set_import_field(import_obj, FIELD_WASI_STARTED.as_bytes(), bool_value(false));
    let binding = crate::string::js_string_from_bytes(
        options.binding_name.as_ptr(),
        options.binding_name.len() as u32,
    );
    set_import_field(
        import_obj,
        FIELD_WASI_BINDING.as_bytes(),
        string_value(binding),
    );
    let keys = b"wasiImport\0";
    let obj = crate::object::js_object_alloc_class_with_keys(
        CLASS_ID_WASI,
        0,
        1,
        keys.as_ptr(),
        keys.len() as u32,
    );
    crate::object::js_object_set_field(obj, 0, JSValue::from_bits(ptr_value(import_obj).to_bits()));
    crate::object::js_object_set_field_by_name(
        obj,
        named_key(FIELD_WASI_BINDING.as_bytes()),
        string_value(binding),
    );
    let new_target = crate::object::js_new_target_value();
    let new_target_js = JSValue::from_bits(new_target.to_bits());
    if new_target_js.is_pointer() {
        let target = new_target_js.as_pointer::<u8>() as usize;
        if crate::closure::is_closure_ptr(target) {
            let prototype = crate::closure::closure_get_dynamic_prop(target, "prototype");
            if heap_object_ptr(prototype).is_some() {
                crate::object::prototype_chain::object_set_static_prototype(
                    obj as usize,
                    prototype.to_bits(),
                );
            }
        }
    }
    ptr_value(obj)
}

pub(crate) unsafe fn js_wasi_init_subclass(this_box: f64, options: f64) {
    let Some(this) = heap_object_ptr(this_box) else {
        return;
    };
    let wasi = js_wasi_new(options);
    let Some(wasi) = heap_object_ptr(wasi) else {
        return;
    };
    let import = crate::object::js_object_get_field_by_name_f64(
        wasi,
        named_key(FIELD_WASI_IMPORT.as_bytes()),
    );
    crate::object::js_object_set_field_by_name(
        this,
        named_key(FIELD_WASI_IMPORT.as_bytes()),
        import,
    );
}

#[no_mangle]
pub extern "C" fn js_wasi_get_import_object(_closure: *const ClosureHeader) -> f64 {
    let this = crate::object::js_implicit_this_get();
    let Some(obj) = heap_object_ptr(this) else {
        let wrapper = crate::object::js_object_alloc(0, 0);
        crate::object::js_object_set_field_by_name(wrapper, named_key(b"undefined"), undefined());
        return ptr_value(wrapper);
    };
    unsafe {
        if !is_wasi_instance(this) {
            let wrapper = crate::object::js_object_alloc(0, 0);
            crate::object::js_object_set_field_by_name(
                wrapper,
                named_key(b"undefined"),
                undefined(),
            );
            return ptr_value(wrapper);
        }
    }
    let import_value = crate::object::js_object_get_field_by_name_f64(
        obj,
        named_key(FIELD_WASI_IMPORT.as_bytes()),
    );
    let Some(import_obj) = heap_object_ptr(import_value) else {
        return undefined();
    };
    let binding = crate::object::js_object_get_field_by_name_f64(
        obj,
        named_key(FIELD_WASI_BINDING.as_bytes()),
    );
    let binding_name =
        if crate::builtins::jsvalue_string_content(binding).as_deref() == Some("wasi_unstable") {
            "wasi_unstable"
        } else {
            import_binding_name(import_obj)
        };
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
    let import = wasi_import_or_throw(wasi);
    let memory = instance_export(instance, b"memory");
    validate_memory_value(memory);
    mark_wasi_started(wasi);
    bind_memory(import, memory);
    let start = instance_export(instance, b"_start");
    if !is_callable_value(start) {
        invalid_type("instance.exports._start", "function", start)
    }
    let initialize = instance_export(instance, b"_initialize");
    if !is_undefined(initialize) {
        invalid_undefined_property("instance.exports._initialize", initialize)
    }
    WASI_EXIT_CODE.with(|slot| slot.set(None));
    unsafe { crate::closure::js_native_call_value(start, std::ptr::null(), 0) };
    if let Some(code) = WASI_EXIT_CODE.with(|slot| slot.take()) {
        return code as f64;
    }
    // The WASM bridge records its preview1 proc_exit outcome on the instance;
    // ordinary JS `_start` return values are deliberately ignored by Node.
    let host_exit = crate::object::js_object_get_field_by_name_f64(
        validate_instance_arg(instance),
        named_key(b"__wasiProcExitCode"),
    );
    if host_exit.is_finite() && host_exit.fract() == 0.0 {
        return host_exit;
    }
    0.0
}

#[no_mangle]
pub extern "C" fn js_wasi_initialize(_closure: *const ClosureHeader, instance: f64) -> f64 {
    let wasi = wasi_receiver_or_throw();
    ensure_wasi_not_started(wasi);
    let import = wasi_import_or_throw(wasi);
    let memory = instance_export(instance, b"memory");
    validate_memory_value(memory);
    mark_wasi_started(wasi);
    bind_memory(import, memory);
    let start = instance_export(instance, b"_start");
    if !is_undefined(start) {
        invalid_undefined_property("instance.exports._start", start)
    }
    let initialize = instance_export(instance, b"_initialize");
    if !is_undefined(initialize) && !is_callable_value(initialize) {
        invalid_type("instance.exports._initialize", "function", initialize)
    }
    if !is_undefined(initialize) {
        unsafe { crate::closure::js_native_call_value(initialize, std::ptr::null(), 0) };
    }
    undefined()
}

#[no_mangle]
pub extern "C" fn js_wasi_finalize_bindings(
    _closure: *const ClosureHeader,
    instance: f64,
    rest: f64,
) -> f64 {
    // Node reads `options.memory` before the receiver/state and instance
    // checks. The rest closure preserves the public arity of one while still
    // accepting the optional second argument.
    let options = rest_argument(rest, 0);
    let override_memory = finalize_memory_option(options);
    let wasi = wasi_receiver_or_throw_with_code("ERR_INVALID_ARG_TYPE");
    ensure_wasi_not_started(wasi);
    let import = wasi_import_or_throw(wasi);
    let exported_memory = instance_export(instance, b"memory");
    let memory = if is_undefined(override_memory) {
        exported_memory
    } else {
        override_memory
    };
    validate_memory_value(memory);
    mark_wasi_started(wasi);
    bind_memory(import, memory);
    undefined()
}

fn wasi_receiver_or_throw() -> *mut ObjectHeader {
    wasi_receiver_or_throw_with_code("")
}

fn wasi_receiver_or_throw_with_code(code: &'static str) -> *mut ObjectHeader {
    let this = crate::object::js_implicit_this_get();
    let Some(obj) = heap_object_ptr(this) else {
        type_error_with_code("Value of \"this\" must be of type WASI", code);
    };
    unsafe {
        if !is_wasi_instance(this) {
            type_error_with_code("Value of \"this\" must be of type WASI", code);
        }
    }
    obj
}

fn wasi_import_or_throw(wasi: *mut ObjectHeader) -> *mut ObjectHeader {
    let import = crate::object::js_object_get_field_by_name_f64(
        wasi,
        named_key(FIELD_WASI_IMPORT.as_bytes()),
    );
    let Some(import) = heap_object_ptr(import) else {
        type_error_with_code("Value of \"this\" must be of type WASI", "ERR_INVALID_THIS")
    };
    import
}

fn wasi_started(wasi: *mut ObjectHeader) -> bool {
    let import = wasi_import_or_throw(wasi);
    import_started(import)
}

fn import_started(import: *mut ObjectHeader) -> bool {
    let value = import_field(import, FIELD_WASI_STARTED.as_bytes());
    JSValue::from_bits(value.to_bits()).is_bool() && JSValue::from_bits(value.to_bits()).as_bool()
}

fn ensure_import_started(import: *mut ObjectHeader) {
    if !import_started(import) {
        crate::fs::validate::throw_error_with_code(
            "WASI instance has not been started",
            "ERR_WASI_NOT_STARTED",
        );
    }
}

fn ensure_wasi_not_started(obj: *mut ObjectHeader) {
    if wasi_started(obj) {
        crate::fs::validate::throw_error_with_code(
            "WASI instance has already started",
            "ERR_WASI_ALREADY_STARTED",
        );
    }
}

fn mark_wasi_started(wasi: *mut ObjectHeader) {
    let import = wasi_import_or_throw(wasi);
    set_import_field(import, FIELD_WASI_STARTED.as_bytes(), bool_value(true));
}

fn instance_export(instance: f64, name: &[u8]) -> f64 {
    let instance = validate_instance_arg(instance);
    let exports = crate::object::js_object_get_field_by_name_f64(instance, named_key(b"exports"));
    let Some(exports) = heap_object_ptr(exports) else {
        let message = format!(
            "The \"instance.exports\" property must be of type object. Received {}",
            crate::fs::validate::describe_received(exports)
        );
        invalid_arg_type(&message)
    };
    crate::object::js_object_get_field_by_name_f64(exports, named_key(name))
}

fn validate_instance_arg(instance: f64) -> *mut ObjectHeader {
    let Some(obj) = heap_object_ptr(instance) else {
        let message = format!(
            "The \"instance\" argument must be of type object. Received {}",
            crate::fs::validate::describe_received(instance)
        );
        invalid_arg_type(&message)
    };
    obj
}

fn is_callable_value(value: f64) -> bool {
    let jsval = JSValue::from_bits(value.to_bits());
    jsval.is_pointer() && crate::closure::is_closure_ptr(jsval.as_pointer::<u8>() as usize)
}

fn memory_buffer(memory: f64) -> Option<*mut crate::buffer::BufferHeader> {
    let object = heap_object_ptr(memory)?;
    let buffer = crate::object::js_object_get_field_by_name_f64(object, named_key(b"buffer"));
    let ptr = JSValue::from_bits(buffer.to_bits()).as_pointer::<u8>() as usize;
    crate::buffer::is_array_buffer(ptr).then_some(ptr as *mut crate::buffer::BufferHeader)
}

fn validate_memory_value(memory: f64) {
    if memory_buffer(memory).is_none() {
        invalid_wasm_memory()
    }
}

fn bind_memory(import: *mut ObjectHeader, memory: f64) {
    set_import_field(import, FIELD_WASI_MEMORY.as_bytes(), memory);
}

fn rest_argument(rest: f64, index: u32) -> f64 {
    let ptr = JSValue::from_bits(rest.to_bits()).as_pointer::<crate::array::ArrayHeader>();
    if ptr.is_null() || crate::array::js_array_length(ptr) <= index {
        undefined()
    } else {
        crate::array::js_array_get_f64(ptr, index)
    }
}

fn finalize_memory_option(options: f64) -> f64 {
    if is_undefined(options) {
        return undefined();
    }
    let Some(options) = heap_object_ptr(options) else {
        // Node's null options throw a TypeError without an ERR_* code.
        type_error_with_code("Cannot read properties of null (reading 'memory')", "")
    };
    crate::object::js_object_get_field_by_name_f64(options, named_key(b"memory"))
}

fn import_from_closure(closure: *const ClosureHeader) -> *mut ObjectHeader {
    let this = crate::object::js_implicit_this_get();
    if let Some(obj) = heap_object_ptr(this) {
        if unsafe { is_wasi_import_object(obj) } {
            return obj;
        }
    }
    crate::closure::js_closure_get_capture_ptr(closure, 0) as *mut ObjectHeader
}

fn import_field(import: *mut ObjectHeader, name: &[u8]) -> f64 {
    crate::object::js_object_get_field_by_name_f64(import, named_key(name))
}

fn set_import_field(import: *mut ObjectHeader, name: &[u8], value: f64) {
    crate::object::js_object_set_field_by_name(import, named_key(name), value);
}

fn bound_memory(import: *mut ObjectHeader) -> *mut crate::buffer::BufferHeader {
    let memory = import_field(import, FIELD_WASI_MEMORY.as_bytes());
    let Some(buffer) = memory_buffer(memory) else {
        crate::fs::validate::throw_error_with_code(
            "WASI instance has not been started",
            "ERR_WASI_NOT_STARTED",
        )
    };
    buffer
}

fn argument(value: f64) -> Option<usize> {
    (value.is_finite() && value >= 0.0 && value.fract() == 0.0).then_some(value as usize)
}

fn write_u32(buffer: *mut crate::buffer::BufferHeader, offset: f64, value: u32) -> bool {
    let Some(offset) = argument(offset) else {
        return false;
    };
    let len = unsafe { (*buffer).length } as usize;
    if offset.checked_add(4).is_none_or(|end| end > len) {
        return false;
    }
    unsafe {
        let target = crate::buffer::buffer_data_mut(buffer).add(offset);
        std::ptr::copy_nonoverlapping(value.to_le_bytes().as_ptr(), target, 4);
        crate::buffer::view::propagate_written_range_from_receiver(
            buffer as usize,
            offset as u32,
            target,
            4,
        );
    }
    true
}

fn write_u64(buffer: *mut crate::buffer::BufferHeader, offset: f64, value: u64) -> bool {
    let Some(offset) = argument(offset) else {
        return false;
    };
    let len = unsafe { (*buffer).length } as usize;
    if offset.checked_add(8).is_none_or(|end| end > len) {
        return false;
    }
    unsafe {
        let target = crate::buffer::buffer_data_mut(buffer).add(offset);
        std::ptr::copy_nonoverlapping(value.to_le_bytes().as_ptr(), target, 8);
        crate::buffer::view::propagate_written_range_from_receiver(
            buffer as usize,
            offset as u32,
            target,
            8,
        );
    }
    true
}

fn snapshot_values(import: *mut ObjectHeader, key: &[u8]) -> *mut crate::array::ArrayHeader {
    JSValue::from_bits(import_field(import, key).to_bits())
        .as_pointer::<crate::array::ArrayHeader>() as *mut crate::array::ArrayHeader
}

fn snapshot_size(values: *mut crate::array::ArrayHeader) -> usize {
    (0..crate::array::js_array_length(values))
        .map(|index| {
            crate::builtins::jsvalue_string_content(crate::array::js_array_get_f64(values, index))
                .map_or(1, |value| value.as_bytes().len() + 1)
        })
        .sum()
}

fn snapshot_sizes(import: *mut ObjectHeader, key: &[u8], count: f64, size: f64) -> f64 {
    let Some(buffer) = memory_buffer(import_field(import, FIELD_WASI_MEMORY.as_bytes())) else {
        return 28.0;
    };
    let values = snapshot_values(import, key);
    if write_u32(buffer, count, crate::array::js_array_length(values))
        && write_u32(buffer, size, snapshot_size(values) as u32)
    {
        0.0
    } else {
        28.0
    }
}

fn snapshot_get(import: *mut ObjectHeader, key: &[u8], pointers: f64, strings: f64) -> f64 {
    let memory = import_field(import, FIELD_WASI_MEMORY.as_bytes());
    let Some(buffer) = memory_buffer(memory) else {
        return 28.0;
    };
    let Some(mut pointers) = argument(pointers) else {
        return 28.0;
    };
    let Some(mut strings) = argument(strings) else {
        return 28.0;
    };
    let values = snapshot_values(import, key);
    let len = unsafe { (*buffer).length } as usize;
    for index in 0..crate::array::js_array_length(values) {
        let bytes =
            crate::builtins::jsvalue_string_content(crate::array::js_array_get_f64(values, index))
                .unwrap_or_default()
                .into_bytes();
        if pointers.checked_add(4).is_none_or(|end| end > len)
            || strings
                .checked_add(bytes.len() + 1)
                .is_none_or(|end| end > len)
        {
            return 28.0;
        }
        if !write_u32(buffer, pointers as f64, strings as u32) {
            return 28.0;
        }
        unsafe {
            let target = crate::buffer::buffer_data_mut(buffer).add(strings);
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), target, bytes.len());
            *target.add(bytes.len()) = 0;
            crate::buffer::view::propagate_written_range_from_receiver(
                buffer as usize,
                strings as u32,
                target,
                (bytes.len() + 1) as u32,
            );
        }
        pointers += 4;
        strings += bytes.len() + 1;
    }
    0.0
}

fn import_function_name(closure: *const ClosureHeader) -> &'static str {
    let index = crate::closure::js_closure_get_capture_f64(closure, 1) as usize;
    WASI_IMPORT_NAMES.get(index).copied().unwrap_or("")
}

#[no_mangle]
pub extern "C" fn js_wasi_import_stub(
    closure: *const ClosureHeader,
    arg0: f64,
    arg1: f64,
    arg2: f64,
    arg3: f64,
) -> f64 {
    let import = import_from_closure(closure);
    match import_function_name(closure).trim_start_matches("bound ") {
        "args_sizes_get" => {
            if !is_undefined(arg2) || argument(arg0).is_none() || argument(arg1).is_none() {
                28.0
            } else {
                ensure_import_started(import);
                snapshot_sizes(import, FIELD_WASI_ARGS.as_bytes(), arg0, arg1)
            }
        }
        "args_get" => {
            if argument(arg0).is_none() || argument(arg1).is_none() {
                28.0
            } else {
                ensure_import_started(import);
                snapshot_get(import, FIELD_WASI_ARGS.as_bytes(), arg0, arg1)
            }
        }
        "environ_sizes_get" => {
            if !is_undefined(arg2) || argument(arg0).is_none() || argument(arg1).is_none() {
                28.0
            } else {
                ensure_import_started(import);
                snapshot_sizes(import, FIELD_WASI_ENV.as_bytes(), arg0, arg1)
            }
        }
        "environ_get" => {
            if argument(arg0).is_none() || argument(arg1).is_none() {
                28.0
            } else {
                ensure_import_started(import);
                snapshot_get(import, FIELD_WASI_ENV.as_bytes(), arg0, arg1)
            }
        }
        "clock_res_get" => {
            if argument(arg0).is_none() || argument(arg1).is_none() {
                return 28.0;
            }
            ensure_import_started(import);
            let memory = import_field(import, FIELD_WASI_MEMORY.as_bytes());
            let Some(buffer) = memory_buffer(memory) else {
                return 28.0;
            };
            if write_u64(buffer, arg1, 1) {
                0.0
            } else {
                28.0
            }
        }
        "clock_time_get" => {
            if !JSValue::from_bits(arg1.to_bits()).is_bigint()
                || argument(arg0).is_none()
                || argument(arg2).is_none()
            {
                return 28.0;
            }
            ensure_import_started(import);
            let memory = import_field(import, FIELD_WASI_MEMORY.as_bytes());
            let Some(buffer) = memory_buffer(memory) else {
                return 28.0;
            };
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(1, |time| time.as_nanos().min(u64::MAX as u128) as u64);
            if write_u64(buffer, arg2, nanos) {
                0.0
            } else {
                28.0
            }
        }
        "random_get" => {
            let (Some(offset), Some(len)) = (argument(arg0), argument(arg1)) else {
                return 28.0;
            };
            ensure_import_started(import);
            let buffer = bound_memory(import);
            let buffer_len = unsafe { (*buffer).length } as usize;
            if offset.checked_add(len).is_none_or(|end| end > buffer_len) {
                return 28.0;
            }
            if len > 0 {
                let seed = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(1, |time| time.as_nanos() as u64);
                unsafe {
                    let target = crate::buffer::buffer_data_mut(buffer).add(offset);
                    for index in 0..len {
                        *target.add(index) = (seed >> ((index % 8) * 8)) as u8;
                    }
                    crate::buffer::view::propagate_written_range_from_receiver(
                        buffer as usize,
                        offset as u32,
                        target,
                        len as u32,
                    );
                }
            }
            0.0
        }
        "proc_exit" => {
            let code = argument(arg0).unwrap_or(0) as i32;
            let return_on_exit = JSValue::from_bits(
                import_field(import, FIELD_WASI_RETURN_ON_EXIT.as_bytes()).to_bits(),
            )
            .as_bool();
            if return_on_exit {
                WASI_EXIT_CODE.with(|slot| slot.set(Some(code)));
                0.0
            } else {
                std::process::exit(code)
            }
        }
        _ => {
            let _ = (arg0, arg1, arg2, arg3);
            28.0
        }
    }
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
