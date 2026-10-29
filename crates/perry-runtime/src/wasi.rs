//! `node:wasi` lifecycle and Preview 1 hosting through the optional host archive.

mod host;
pub(crate) use host::{call_import, is_exit_marker};

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

const WASI_IMPORT_NAMES: &[&str] = perry_abi::wasi::NAMES;

fn ptr_value(ptr: *mut ObjectHeader) -> f64 {
    f64::from_bits(JSValue::pointer(ptr as *const u8).bits())
}

fn array_value(ptr: *mut crate::array::ArrayHeader) -> f64 {
    crate::value::js_nanbox_pointer(ptr as i64)
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
    if ptr.is_null() {
        return None;
    }
    let header = unsafe { crate::value::addr_class::try_read_gc_header(ptr as usize)? };
    if header.obj_type == crate::gc::GC_TYPE_OBJECT {
        Some(ptr as *mut ObjectHeader)
    } else {
        None
    }
}

fn is_array_value(value: f64) -> bool {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_pointer() {
        return false;
    }
    let ptr = jsval.as_pointer::<u8>();
    if ptr.is_null() {
        return false;
    }
    unsafe { crate::value::addr_class::try_read_gc_header(ptr as usize) }.is_some_and(|header| {
        matches!(
            header.obj_type,
            crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_LAZY_ARRAY
        )
    })
}

fn is_object_value(value: f64) -> bool {
    heap_object_ptr(value).is_some()
}

fn object_field(object: f64, name: &[u8]) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_nanbox_f64(object);
    let key = scope.root_string_ptr(named_key(name));
    let Some(obj) = heap_object_ptr(object.get_nanbox_f64()) else {
        return undefined();
    };
    crate::object::js_object_get_field_by_name_f64(obj, key.get_raw_const_ptr::<StringHeader>())
}

fn set_object_field(object: f64, name: &[u8], value: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_nanbox_f64(object);
    let value = scope.root_nanbox_f64(value);
    let key = scope.root_string_ptr(named_key(name));
    if let Some(obj) = heap_object_ptr(object.get_nanbox_f64()) {
        crate::object::js_object_set_field_by_name(
            obj,
            key.get_raw_const_ptr::<StringHeader>(),
            value.get_nanbox_f64(),
        );
    }
}

fn option_field(options: f64, name: &[u8]) -> f64 {
    if is_undefined(options) {
        undefined()
    } else {
        object_field(options, name)
    }
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

fn validate_optional_fd(options: f64, name: &[u8], label: &str) -> Option<i32> {
    let value = option_field(options, name);
    if JSValue::from_bits(value.to_bits()).is_undefined() {
        return None;
    }
    crate::fs::validate::validate_int32(value, label, 0, i32::MAX as i64);
    Some(value as i32)
}

struct ValidatedOptions {
    binding_name: &'static str,
    import_class_id: u32,
    args: f64,
    env: f64,
    return_on_exit: bool,
    preopens: Vec<(String, String)>,
    stdio: [Option<i32>; 3],
}

fn option_string(value: f64) -> Vec<u8> {
    let coerced = crate::builtins::js_string_coerce(value);
    let text = crate::builtins::jsvalue_string_content(string_value(coerced)).unwrap_or_default();
    text.into_bytes()
        .into_iter()
        .take_while(|byte| *byte != 0)
        .collect()
}

fn snapshot_args(value: f64) -> f64 {
    if is_undefined(value) {
        return array_value(crate::array::js_array_alloc(0));
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let raw = JSValue::from_bits(value.get_nanbox_f64().to_bits())
        .as_pointer::<crate::array::ArrayHeader>();
    let len = crate::array::js_array_length(raw);
    let out = scope.root_nanbox_f64(array_value(crate::array::js_array_alloc(len)));
    for index in 0..len {
        let raw = JSValue::from_bits(value.get_nanbox_f64().to_bits())
            .as_pointer::<crate::array::ArrayHeader>();
        let item = scope.root_nanbox_f64(crate::array::js_array_get_f64(raw, index));
        let bytes = option_string(item.get_nanbox_f64());
        let string = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        let out_ptr = JSValue::from_bits(out.get_nanbox_f64().to_bits())
            .as_pointer::<crate::array::ArrayHeader>()
            as *mut crate::array::ArrayHeader;
        let out_ptr = crate::array::js_array_push_f64(out_ptr, string_value(string));
        out.set_nanbox_f64(array_value(out_ptr));
    }
    out.get_nanbox_f64()
}

fn snapshot_env(value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let out = scope.root_nanbox_f64(array_value(crate::array::js_array_alloc(0)));
    if is_undefined(value) {
        return out.get_nanbox_f64();
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
        let field = object_field(value.get_nanbox_f64(), key.as_bytes());
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
        let out_ptr = crate::array::js_array_push_f64(out_ptr, string_value(string));
        out.set_nanbox_f64(array_value(out_ptr));
    }
    out.get_nanbox_f64()
}

fn validate_options(options: f64) -> ValidatedOptions {
    let scope = crate::gc::RuntimeHandleScope::new();
    let options = scope.root_nanbox_f64(options);
    let options_js = JSValue::from_bits(options.get_nanbox_f64().to_bits());
    if !options_js.is_undefined() && heap_object_ptr(options.get_nanbox_f64()).is_none() {
        invalid_options(options.get_nanbox_f64());
    }

    // Node materializes each configurable option while creating its internal
    // snapshot; preserve that observable getter order rather than caching a
    // single property read.
    let version_value = option_field(options.get_nanbox_f64(), b"version");
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
    let _ = option_field(options.get_nanbox_f64(), b"version");

    let args_validate = option_field(options.get_nanbox_f64(), b"args");
    if !is_undefined(args_validate) && !is_array_value(args_validate) {
        invalid_type("options.args", "Array", args_validate);
    }
    let _ = option_field(options.get_nanbox_f64(), b"args");
    let args = scope.root_nanbox_f64(snapshot_args(option_field(
        options.get_nanbox_f64(),
        b"args",
    )));

    let env_validate = option_field(options.get_nanbox_f64(), b"env");
    if !is_undefined(env_validate) && !is_object_value(env_validate) {
        invalid_type("options.env", "object", env_validate);
    }
    let _ = option_field(options.get_nanbox_f64(), b"env");
    let env = scope.root_nanbox_f64(snapshot_env(option_field(options.get_nanbox_f64(), b"env")));

    let preopens_validate = option_field(options.get_nanbox_f64(), b"preopens");
    if !is_undefined(preopens_validate) && !is_object_value(preopens_validate) {
        invalid_type("options.preopens", "object", preopens_validate);
    }
    let _ = option_field(options.get_nanbox_f64(), b"preopens");
    let preopens_value = scope.root_nanbox_f64(option_field(options.get_nanbox_f64(), b"preopens"));
    let mut preopens = Vec::new();
    if !is_undefined(preopens_value.get_nanbox_f64()) {
        let keys = scope.root_nanbox_f64(array_value(crate::object::js_object_keys_value(
            preopens_value.get_nanbox_f64(),
        )));
        let count = crate::array::js_array_length(
            JSValue::from_bits(keys.get_nanbox_f64().to_bits())
                .as_pointer::<crate::array::ArrayHeader>(),
        );
        for index in 0..count {
            let array = JSValue::from_bits(keys.get_nanbox_f64().to_bits())
                .as_pointer::<crate::array::ArrayHeader>();
            let guest = crate::builtins::jsvalue_string_content(crate::array::js_array_get_f64(
                array, index,
            ))
            .unwrap_or_default();
            let path = object_field(preopens_value.get_nanbox_f64(), guest.as_bytes());
            preopens.push((
                guest,
                String::from_utf8_lossy(&option_string(path)).into_owned(),
            ));
        }
    }

    let stdio = [
        validate_optional_fd(options.get_nanbox_f64(), b"stdin", "options.stdin"),
        validate_optional_fd(options.get_nanbox_f64(), b"stdout", "options.stdout"),
        validate_optional_fd(options.get_nanbox_f64(), b"stderr", "options.stderr"),
    ];

    let return_validate = option_field(options.get_nanbox_f64(), b"returnOnExit");
    if !is_undefined(return_validate) && !JSValue::from_bits(return_validate.to_bits()).is_bool() {
        invalid_type("options.returnOnExit", "boolean", return_validate);
    }
    let _ = option_field(options.get_nanbox_f64(), b"returnOnExit");
    let return_value = option_field(options.get_nanbox_f64(), b"returnOnExit");
    let return_on_exit = if is_undefined(return_value) {
        true
    } else {
        JSValue::from_bits(return_value.to_bits()).as_bool()
    };

    ValidatedOptions {
        binding_name,
        import_class_id,
        args: args.get_nanbox_f64(),
        env: env.get_nanbox_f64(),
        return_on_exit,
        preopens,
        stdio,
    }
}

fn closure_value(info: *const crate::closure::JsFunctionInfo, name: &str, arity: u32) -> f64 {
    let closure = crate::closure::js_closure_alloc(info, 0);
    crate::object::set_bound_native_closure_name(closure, name);
    crate::object::set_builtin_closure_length(closure as usize, arity);
    crate::value::js_nanbox_pointer(closure as i64)
}

fn closure_rest_value(info: *const crate::closure::JsFunctionInfo, name: &str, arity: u32) -> f64 {
    let closure = crate::closure::js_closure_alloc(info, 0);
    crate::object::set_bound_native_closure_name(closure, name);
    crate::object::set_builtin_closure_length(closure as usize, arity);
    crate::value::js_nanbox_pointer(closure as i64)
}

fn create_import_function(import_value: f64, name: &str) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let import = scope.root_nanbox_f64(import_value);
    let index = WASI_IMPORT_NAMES
        .iter()
        .position(|candidate| *candidate == name)
        .unwrap();
    let arity = perry_abi::wasi::PARAMS[index].len() as u32;
    let closure = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(js_wasi_import_call, 1; with_rest(0)),
        2,
    ));
    let closure_ptr = closure.get_raw_mut_ptr::<ClosureHeader>();
    let import_ptr = heap_object_ptr(import.get_nanbox_f64()).unwrap_or(std::ptr::null_mut());
    crate::closure::js_closure_set_capture_ptr(closure_ptr, 0, import_ptr as i64);
    crate::closure::js_closure_set_capture_f64(
        closure_ptr,
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
    crate::object::set_bound_native_closure_name(closure_ptr, &display_name);
    crate::object::set_builtin_closure_length(closure_ptr as usize, arity);
    crate::object::set_builtin_closure_non_constructable(closure_ptr as usize);
    crate::value::js_nanbox_pointer(closure_ptr as i64)
}

fn create_import_object(class_id: u32) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(class_id, 0));
    for name in WASI_IMPORT_NAMES {
        let function = scope.root_nanbox_f64(create_import_function(
            ptr_value(obj.get_raw_mut_ptr::<ObjectHeader>()),
            name,
        ));
        let key = scope.root_string_ptr(named_key(name.as_bytes()));
        crate::object::js_object_set_field_by_name(
            obj.get_raw_mut_ptr::<ObjectHeader>(),
            key.get_raw_const_ptr::<StringHeader>(),
            function.get_nanbox_f64(),
        );
    }
    ptr_value(obj.get_raw_mut_ptr::<ObjectHeader>())
}

fn import_binding_name(import_value: f64) -> &'static str {
    let binding = import_field(import_value, FIELD_WASI_BINDING.as_bytes());
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
        crate::realm_decl! { birth crate::object::js_object_alloc_with_shape(0x7FFF_FF41, 5, keys.as_ptr(), keys.len() as u32); realm { "node:wasi.WASI.prototype", members = { constructor; getImportObject; start; initialize; finalizeBindings; }; } };
    // Publish the root before allocating any of the method closures. The
    // class-prototype root is rewritten if those allocations evacuate it.
    crate::object::class_prototype_object_root_store(CLASS_ID_WASI, proto);
    let method_scope = crate::gc::RuntimeHandleScope::new();
    let method = method_scope.root_nanbox_f64(closure_value(
        crate::fn_info!(#[realm("node:wasi.WASI.prototype.getImportObject", constructible = true)] export crate::wasi::js_wasi_get_import_object, 0; with_declared(0)),
        "getImportObject",
        0,
    ));
    crate::object::js_object_set_field(
        crate::object::class_prototype_object(CLASS_ID_WASI),
        1,
        JSValue::from_bits(method.get_nanbox_f64().to_bits()),
    );
    method.set_nanbox_f64(closure_value(
        crate::fn_info!(#[realm("node:wasi.WASI.prototype.start", constructible = true)] export crate::wasi::js_wasi_start, 1; with_declared(1)),
        "start",
        1,
    ));
    crate::object::js_object_set_field(
        crate::object::class_prototype_object(CLASS_ID_WASI),
        2,
        JSValue::from_bits(method.get_nanbox_f64().to_bits()),
    );
    method.set_nanbox_f64(closure_value(
        crate::fn_info!(#[realm("node:wasi.WASI.prototype.initialize", constructible = true)] export crate::wasi::js_wasi_initialize, 1; with_declared(1)),
        "initialize",
        1,
    ));
    crate::object::js_object_set_field(
        crate::object::class_prototype_object(CLASS_ID_WASI),
        3,
        JSValue::from_bits(method.get_nanbox_f64().to_bits()),
    );
    method.set_nanbox_f64(closure_rest_value(
        crate::fn_info!(#[realm("node:wasi.WASI.prototype.finalizeBindings", constructible = true)] export crate::wasi::js_wasi_finalize_bindings, 2; with_rest(1)),
        "finalizeBindings",
        1,
    ));
    crate::object::js_object_set_field(
        crate::object::class_prototype_object(CLASS_ID_WASI),
        4,
        JSValue::from_bits(method.get_nanbox_f64().to_bits()),
    );
    for name in [
        "constructor",
        "getImportObject",
        "start",
        "initialize",
        "finalizeBindings",
    ] {
        crate::object::set_builtin_property_attrs(
            crate::object::class_prototype_object(CLASS_ID_WASI) as usize,
            name.to_string(),
            crate::object::PropertyAttrs::new(true, false, true),
        );
    }
}

pub(crate) fn ensure_wasi_prototype_for_subclass() {
    ensure_wasi_prototype();
}

pub(crate) fn attach_wasi_constructor_prototype(constructor_value: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let constructor = scope.root_nanbox_f64(constructor_value);
    ensure_wasi_prototype();
    let proto = crate::object::class_prototype_object(CLASS_ID_WASI);
    if proto.is_null() {
        return;
    }
    crate::object::js_object_set_field(
        proto,
        0,
        JSValue::from_bits(constructor.get_nanbox_f64().to_bits()),
    );
    let constructor_addr =
        (constructor.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK) as usize;
    crate::closure::closure_set_dynamic_prop(
        constructor_addr,
        "prototype",
        crate::value::js_nanbox_pointer(proto as i64),
    );
    crate::object::set_builtin_property_attrs(
        constructor_addr,
        "prototype".to_string(),
        crate::object::PropertyAttrs::new(false, false, false),
    );
}

fn emit_wasi_warning() {
    let message = b"WASI is an experimental feature and might change at any time";
    let kind = b"ExperimentalWarning";
    let scope = crate::gc::RuntimeHandleScope::new();
    let message = scope.root_string_ptr(crate::string::js_string_from_bytes(
        message.as_ptr(),
        message.len() as u32,
    ));
    let kind = scope.root_string_ptr(crate::string::js_string_from_bytes(
        kind.as_ptr(),
        kind.len() as u32,
    ));
    crate::process::js_process_emit_warning(
        f64::from_bits(JSValue::string_ptr(message.get_raw_mut_ptr()).bits()),
        f64::from_bits(JSValue::string_ptr(kind.get_raw_mut_ptr()).bits()),
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
            crate::os::current_process_id()
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
    let mut class_id = unsafe { (*obj).class_id };
    for _ in 0..=64 {
        if class_id == CLASS_ID_WASI {
            return true;
        }
        let Some(parent) = crate::object::get_parent_class_id(class_id) else {
            return false;
        };
        if parent == 0 || parent == class_id {
            return false;
        }
        class_id = parent;
    }
    false
}

pub(crate) fn is_wasi_import_object(obj: *const ObjectHeader) -> bool {
    if obj.is_null() {
        return false;
    }
    let Some(header) = (unsafe { crate::value::addr_class::try_read_gc_header(obj as usize) })
    else {
        return false;
    };
    if header.obj_type != crate::gc::GC_TYPE_OBJECT {
        return false;
    }
    unsafe {
        matches!(
            (*obj).class_id,
            CLASS_ID_WASI_IMPORT_PREVIEW1 | CLASS_ID_WASI_IMPORT_UNSTABLE
        )
    }
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let args = scope.root_nanbox_f64(options.args);
    let env = scope.root_nanbox_f64(options.env);
    let context = scope.root_nanbox_f64(host::new(&options));
    ensure_wasi_prototype();
    let import = scope.root_nanbox_f64(create_import_object(options.import_class_id));
    host::attach(import.get_nanbox_f64(), context.get_nanbox_f64());
    set_import_field(
        import.get_nanbox_f64(),
        FIELD_WASI_ARGS.as_bytes(),
        args.get_nanbox_f64(),
    );
    set_import_field(
        import.get_nanbox_f64(),
        FIELD_WASI_ENV.as_bytes(),
        env.get_nanbox_f64(),
    );
    set_import_field(
        import.get_nanbox_f64(),
        FIELD_WASI_RETURN_ON_EXIT.as_bytes(),
        bool_value(options.return_on_exit),
    );
    set_import_field(
        import.get_nanbox_f64(),
        FIELD_WASI_STARTED.as_bytes(),
        bool_value(false),
    );
    let binding = scope.root_string_ptr(crate::string::js_string_from_bytes(
        options.binding_name.as_ptr(),
        options.binding_name.len() as u32,
    ));
    set_import_field(
        import.get_nanbox_f64(),
        FIELD_WASI_BINDING.as_bytes(),
        string_value(binding.get_raw_mut_ptr()),
    );
    let keys = b"wasiImport\0";
    let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc_class_with_keys(
        CLASS_ID_WASI,
        0,
        1,
        keys.as_ptr(),
        keys.len() as u32,
    ));
    crate::object::js_object_set_field(
        obj.get_raw_mut_ptr::<ObjectHeader>(),
        0,
        JSValue::from_bits(import.get_nanbox_f64().to_bits()),
    );
    set_object_field(
        ptr_value(obj.get_raw_mut_ptr::<ObjectHeader>()),
        FIELD_WASI_BINDING.as_bytes(),
        string_value(binding.get_raw_mut_ptr()),
    );
    let new_target = crate::object::js_new_target_value();
    let new_target_js = JSValue::from_bits(new_target.to_bits());
    if new_target_js.is_pointer() {
        let target = new_target_js.as_pointer::<u8>() as usize;
        if crate::closure::is_closure_ptr(target) {
            let prototype = crate::closure::closure_get_dynamic_prop(target, "prototype");
            if heap_object_ptr(prototype).is_some() {
                crate::object::prototype_chain::object_set_static_prototype(
                    obj.get_raw_mut_ptr::<ObjectHeader>() as usize,
                    prototype.to_bits(),
                );
            }
        }
    }
    ptr_value(obj.get_raw_mut_ptr::<ObjectHeader>())
}

pub(crate) unsafe fn js_wasi_init_subclass(this_box: f64, options: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this_box);
    if heap_object_ptr(this.get_nanbox_f64()).is_none() {
        return;
    }
    let wasi = scope.root_nanbox_f64(js_wasi_new(options));
    if heap_object_ptr(wasi.get_nanbox_f64()).is_none() {
        return;
    }
    let import = scope.root_nanbox_f64(object_field(
        wasi.get_nanbox_f64(),
        FIELD_WASI_IMPORT.as_bytes(),
    ));
    set_object_field(
        this.get_nanbox_f64(),
        FIELD_WASI_IMPORT.as_bytes(),
        import.get_nanbox_f64(),
    );
}

#[no_mangle]
pub extern "C" fn js_wasi_get_import_object(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this.as_f64());
    if heap_object_ptr(this.get_nanbox_f64()).is_none() || !is_wasi_instance(this.get_nanbox_f64())
    {
        let wrapper = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
        set_object_field(
            ptr_value(wrapper.get_raw_mut_ptr::<ObjectHeader>()),
            b"undefined",
            undefined(),
        );
        return ptr_value(wrapper.get_raw_mut_ptr::<ObjectHeader>());
    }
    let import = scope.root_nanbox_f64(object_field(
        this.get_nanbox_f64(),
        FIELD_WASI_IMPORT.as_bytes(),
    ));
    if heap_object_ptr(import.get_nanbox_f64()).is_none() {
        return undefined();
    }
    let binding = object_field(this.get_nanbox_f64(), FIELD_WASI_BINDING.as_bytes());
    let binding_name =
        if crate::builtins::jsvalue_string_content(binding).as_deref() == Some("wasi_unstable") {
            "wasi_unstable"
        } else {
            import_binding_name(import.get_nanbox_f64())
        };
    let wrapper = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    set_object_field(
        ptr_value(wrapper.get_raw_mut_ptr::<ObjectHeader>()),
        binding_name.as_bytes(),
        import.get_nanbox_f64(),
    );
    ptr_value(wrapper.get_raw_mut_ptr::<ObjectHeader>())
}

#[no_mangle]
pub extern "C" fn js_wasi_start(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    instance: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let instance = scope.root_nanbox_f64(instance);
    let wasi = scope.root_nanbox_f64(wasi_receiver_or_throw(this));
    ensure_wasi_not_started(wasi.get_nanbox_f64());
    let import = scope.root_nanbox_f64(wasi_import_or_throw(wasi.get_nanbox_f64()));
    let memory = scope.root_nanbox_f64(instance_export(instance.get_nanbox_f64(), b"memory"));
    validate_memory_value(memory.get_nanbox_f64());
    mark_wasi_started(wasi.get_nanbox_f64());
    bind_memory(import.get_nanbox_f64(), memory.get_nanbox_f64());
    let start = scope.root_nanbox_f64(instance_export(instance.get_nanbox_f64(), b"_start"));
    if !is_callable_value(start.get_nanbox_f64()) {
        invalid_type(
            "instance.exports._start",
            "function",
            start.get_nanbox_f64(),
        )
    }
    let initialize = instance_export(instance.get_nanbox_f64(), b"_initialize");
    if !is_undefined(initialize) {
        invalid_undefined_property("instance.exports._initialize", initialize)
    }
    let previous_exit = WASI_EXIT_CODE.with(|slot| slot.replace(None));
    let outcome = crate::exception::catch_js_throw(|| unsafe {
        crate::closure::js_native_call_value(
            start.get_nanbox_f64(),
            crate::closure::plain_call_receiver(),
            std::ptr::null(),
            0,
        )
    });
    let exit = WASI_EXIT_CODE.with(|slot| slot.replace(previous_exit));
    if let Err(error) = outcome {
        if host::is_exit_marker(error) {
            if let Some(code) = exit {
                return code as u32 as f64;
            }
        }
        crate::exception::js_throw(error);
    }
    if let Some(code) = exit {
        return code as u32 as f64;
    }
    // The WASM bridge records its preview1 proc_exit outcome on the instance;
    // ordinary JS `_start` return values are deliberately ignored by Node.
    let host_exit = object_field(
        validate_instance_arg(instance.get_nanbox_f64()),
        b"__wasiProcExitCode",
    );
    if host_exit.is_finite() && host_exit.fract() == 0.0 {
        return host_exit;
    }
    0.0
}

#[no_mangle]
pub extern "C" fn js_wasi_initialize(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    instance: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let instance = scope.root_nanbox_f64(instance);
    let wasi = scope.root_nanbox_f64(wasi_receiver_or_throw(this));
    ensure_wasi_not_started(wasi.get_nanbox_f64());
    let import = scope.root_nanbox_f64(wasi_import_or_throw(wasi.get_nanbox_f64()));
    let memory = scope.root_nanbox_f64(instance_export(instance.get_nanbox_f64(), b"memory"));
    validate_memory_value(memory.get_nanbox_f64());
    mark_wasi_started(wasi.get_nanbox_f64());
    bind_memory(import.get_nanbox_f64(), memory.get_nanbox_f64());
    let start = instance_export(instance.get_nanbox_f64(), b"_start");
    if !is_undefined(start) {
        invalid_undefined_property("instance.exports._start", start)
    }
    let initialize =
        scope.root_nanbox_f64(instance_export(instance.get_nanbox_f64(), b"_initialize"));
    if !is_undefined(initialize.get_nanbox_f64()) && !is_callable_value(initialize.get_nanbox_f64())
    {
        invalid_type(
            "instance.exports._initialize",
            "function",
            initialize.get_nanbox_f64(),
        )
    }
    if !is_undefined(initialize.get_nanbox_f64()) {
        unsafe {
            crate::closure::js_native_call_value(
                initialize.get_nanbox_f64(),
                crate::closure::plain_call_receiver(),
                std::ptr::null(),
                0,
            )
        };
    }
    undefined()
}

#[no_mangle]
pub extern "C" fn js_wasi_finalize_bindings(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    instance: f64,
    rest: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let instance = scope.root_nanbox_f64(instance);
    // Node reads `options.memory` before the receiver/state and instance
    // checks. The rest closure preserves the public arity of one while still
    // accepting the optional second argument.
    let options = rest_argument(rest, 0);
    let override_memory = scope.root_nanbox_f64(finalize_memory_option(options));
    let wasi = scope.root_nanbox_f64(wasi_receiver_or_throw_with_code(
        "ERR_INVALID_ARG_TYPE",
        this,
    ));
    ensure_wasi_not_started(wasi.get_nanbox_f64());
    let import = scope.root_nanbox_f64(wasi_import_or_throw(wasi.get_nanbox_f64()));
    let exported_memory =
        scope.root_nanbox_f64(instance_export(instance.get_nanbox_f64(), b"memory"));
    let memory = if is_undefined(override_memory.get_nanbox_f64()) {
        exported_memory.get_nanbox_f64()
    } else {
        override_memory.get_nanbox_f64()
    };
    let memory = scope.root_nanbox_f64(memory);
    validate_memory_value(memory.get_nanbox_f64());
    mark_wasi_started(wasi.get_nanbox_f64());
    bind_memory(import.get_nanbox_f64(), memory.get_nanbox_f64());
    undefined()
}

fn wasi_receiver_or_throw(this: crate::closure::JsThis) -> f64 {
    wasi_receiver_or_throw_with_code("", this)
}

fn wasi_receiver_or_throw_with_code(code: &'static str, this: crate::closure::JsThis) -> f64 {
    let this = this.as_f64();
    if heap_object_ptr(this).is_none() {
        type_error_with_code("Value of \"this\" must be of type WASI", code);
    }
    if !is_wasi_instance(this) {
        type_error_with_code("Value of \"this\" must be of type WASI", code);
    }
    this
}

fn wasi_import_or_throw(wasi: f64) -> f64 {
    let import = object_field(wasi, FIELD_WASI_IMPORT.as_bytes());
    if heap_object_ptr(import).is_none() {
        type_error_with_code("Value of \"this\" must be of type WASI", "ERR_INVALID_THIS")
    }
    import
}

fn wasi_started(wasi: f64) -> bool {
    let import = wasi_import_or_throw(wasi);
    import_started(import)
}

fn import_started(import: f64) -> bool {
    let value = import_field(import, FIELD_WASI_STARTED.as_bytes());
    JSValue::from_bits(value.to_bits()).is_bool() && JSValue::from_bits(value.to_bits()).as_bool()
}

fn ensure_import_started(import: f64) {
    if !import_started(import) {
        crate::fs::validate::throw_error_with_code(
            "WASI instance has not been started",
            "ERR_WASI_NOT_STARTED",
        );
    }
}

fn ensure_wasi_not_started(obj: f64) {
    if wasi_started(obj) {
        crate::fs::validate::throw_error_with_code(
            "WASI instance has already started",
            "ERR_WASI_ALREADY_STARTED",
        );
    }
}

fn mark_wasi_started(wasi: f64) {
    let import = wasi_import_or_throw(wasi);
    set_import_field(import, FIELD_WASI_STARTED.as_bytes(), bool_value(true));
}

fn instance_export(instance: f64, name: &[u8]) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let instance = scope.root_nanbox_f64(validate_instance_arg(instance));
    let exports = scope.root_nanbox_f64(object_field(instance.get_nanbox_f64(), b"exports"));
    if heap_object_ptr(exports.get_nanbox_f64()).is_none() {
        let message = format!(
            "The \"instance.exports\" property must be of type object. Received {}",
            crate::fs::validate::describe_received(exports.get_nanbox_f64())
        );
        invalid_arg_type(&message)
    }
    object_field(exports.get_nanbox_f64(), name)
}

fn validate_instance_arg(instance: f64) -> f64 {
    if heap_object_ptr(instance).is_none() {
        let message = format!(
            "The \"instance\" argument must be of type object. Received {}",
            crate::fs::validate::describe_received(instance)
        );
        invalid_arg_type(&message)
    }
    instance
}

fn is_callable_value(value: f64) -> bool {
    let jsval = JSValue::from_bits(value.to_bits());
    jsval.is_pointer() && crate::closure::is_closure_ptr(jsval.as_pointer::<u8>() as usize)
}

fn memory_buffer(memory: f64) -> Option<*mut crate::buffer::BufferHeader> {
    heap_object_ptr(memory)?;
    let buffer = object_field(memory, b"buffer");
    let ptr = JSValue::from_bits(buffer.to_bits()).as_pointer::<u8>() as usize;
    crate::buffer::is_array_buffer(ptr).then_some(ptr as *mut crate::buffer::BufferHeader)
}

fn validate_memory_value(memory: f64) {
    if memory_buffer(memory).is_none() {
        invalid_wasm_memory()
    }
}

fn bind_memory(import: f64, memory: f64) {
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
    if heap_object_ptr(options).is_none() {
        // Node's null options throw a TypeError without an ERR_* code.
        type_error_with_code("Cannot read properties of null (reading 'memory')", "")
    }
    object_field(options, b"memory")
}

fn import_from_closure(this: crate::closure::JsThis, closure: *const ClosureHeader) -> f64 {
    let this = this.as_f64();
    if let Some(obj) = heap_object_ptr(this) {
        if is_wasi_import_object(obj) {
            return this;
        }
    }
    ptr_value(crate::closure::js_closure_get_capture_ptr(closure, 0) as *mut ObjectHeader)
}

fn import_field(import: f64, name: &[u8]) -> f64 {
    object_field(import, name)
}

fn set_import_field(import: f64, name: &[u8], value: f64) {
    set_object_field(import, name, value);
}

fn bound_memory(import: f64) -> *mut crate::buffer::BufferHeader {
    let memory = import_field(import, FIELD_WASI_MEMORY.as_bytes());
    let Some(buffer) = memory_buffer(memory) else {
        crate::fs::validate::throw_error_with_code(
            "WASI instance has not been started",
            "ERR_WASI_NOT_STARTED",
        )
    };
    buffer
}

#[no_mangle]
pub extern "C" fn js_wasi_import_call(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    rest: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let rest = scope.root_nanbox_f64(rest);
    let import = scope.root_nanbox_f64(import_from_closure(this, closure));
    let syscall = crate::closure::js_closure_get_capture_f64(closure, 1) as usize;
    let Some(params) = perry_abi::wasi::PARAMS.get(syscall) else {
        return 28.0;
    };
    let rest_array = JSValue::from_bits(rest.get_nanbox_f64().to_bits())
        .as_pointer::<crate::array::ArrayHeader>();
    if crate::array::js_array_length(rest_array) as usize != params.len() {
        return 28.0;
    }
    let mut args = Vec::with_capacity(params.len());
    for (i, kind) in params.iter().enumerate() {
        let value = rest_argument(rest.get_nanbox_f64(), i as u32);
        if *kind == perry_abi::wasm_host::WASM_VAL_KIND_I64 {
            let Some(bits) = crate::webassembly::js_to_wasm_i64_bits(value) else {
                return 28.0;
            };
            args.push(bits);
        } else {
            if !value.is_finite() || value < 0.0 || value > u32::MAX as f64 || value.fract() != 0.0
            {
                return 28.0;
            }
            args.push(value as u32 as u64);
        }
    }
    let result = host::call(import.get_nanbox_f64(), syscall, &args);
    if result == perry_abi::wasi::IMPORT_EXIT {
        drop(args);
        crate::exception::js_throw(host::exit_marker(import.get_nanbox_f64()))
    } else {
        result as f64
    }
}

pub(crate) fn pending_exit_code() -> Option<i32> {
    WASI_EXIT_CODE.with(|slot| slot.get())
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
