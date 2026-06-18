//! Minimal `node:module.createRequire` / CommonJS `require` bridge.
//!
//! This intentionally covers Perry's deterministic native-builtin path and the
//! public function shape. Full CommonJS file/package resolution remains in the
//! compiler-side CJS wrapper and future `Module._*` work.

use crate::closure::{js_closure_alloc, js_register_closure_arity, ClosureHeader};
use crate::object::{js_object_alloc, js_object_set_field_by_name};
use crate::string::js_string_from_bytes;
use crate::value::{js_nanbox_pointer, JSValue, TAG_NULL, TAG_UNDEFINED};

fn undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn null() -> f64 {
    f64::from_bits(TAG_NULL)
}

fn string_value(value: &str) -> f64 {
    let ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn object_value(obj: *mut crate::object::ObjectHeader) -> f64 {
    f64::from_bits(JSValue::object_ptr(obj as *mut u8).bits())
}

fn set_field(obj: *mut crate::object::ObjectHeader, name: &str, value: f64) {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(obj, key, value);
}

fn set_closure_prop(closure: *mut ClosureHeader, name: &str, value: f64) {
    crate::closure::closure_set_dynamic_prop(closure as usize, name, value);
}

fn named_closure(
    func: *const u8,
    arity: u32,
    length: u32,
    name: &str,
) -> (*mut ClosureHeader, f64) {
    js_register_closure_arity(func, arity);
    crate::closure::js_register_closure_length(func, length);
    let closure = js_closure_alloc(func, 0);
    crate::object::set_bound_native_closure_name(closure, name);
    crate::object::set_builtin_closure_length(closure as usize, length);
    (closure, js_nanbox_pointer(closure as i64))
}

fn value_to_string(value: f64, arg_name: &str) -> String {
    let jv = JSValue::from_bits(value.to_bits());
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(bytes) = (unsafe { crate::string::js_string_key_bytes(jv, &mut sso) }) else {
        let message = format!(
            "The \"{}\" argument must be of type string. Received {}",
            arg_name,
            crate::fs::validate::describe_received(value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    };
    String::from_utf8_lossy(bytes).into_owned()
}

fn throw_invalid_value(arg_name: &str, value: f64) -> ! {
    let message = format!(
        "The argument '{}' is invalid. Received {}",
        arg_name,
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE")
}

fn validate_create_require_base(filename_or_url: f64) {
    let jv = JSValue::from_bits(filename_or_url.to_bits());
    if jv.is_any_string() {
        let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let Some(bytes) = (unsafe { crate::string::js_string_key_bytes(jv, &mut sso) }) else {
            throw_invalid_value("filename", filename_or_url);
        };
        let s = String::from_utf8_lossy(bytes);
        if s.starts_with("file:") || std::path::Path::new(s.as_ref()).is_absolute() {
            return;
        }
        throw_invalid_value("filename", filename_or_url);
    }
    if crate::url::node_compat::module_base_to_path(filename_or_url).is_some() {
        return;
    }
    throw_invalid_value("filename", filename_or_url);
}

fn supported_require_builtin(specifier: &str) -> Option<&str> {
    let name = specifier.strip_prefix("node:").unwrap_or(specifier);
    match name {
        "assert" | "assert/strict" | "async_hooks" | "buffer" | "child_process" | "cluster"
        | "console" | "constants" | "crypto" | "dns" | "dns/promises" | "events" | "fs"
        | "http" | "http2" | "https" | "module" | "net" | "os" | "path" | "path/posix"
        | "path/win32" | "perf_hooks" | "process" | "punycode" | "querystring" | "readline"
        | "readline/promises" | "stream" | "stream/promises" | "string_decoder" | "sys"
        | "test" | "test/reporters" | "timers" | "timers/promises" | "tty" | "url" | "util"
        | "util/types" | "worker_threads" | "zlib" => Some(name),
        _ => None,
    }
}

fn resolve_builtin(specifier: &str) -> Option<&str> {
    supported_require_builtin(specifier).map(|_| specifier)
}

fn require_builtin_value(module_name: &str) -> f64 {
    if module_name == "timers/promises" {
        return unsafe {
            crate::node_submodules::js_node_submodule_namespace(
                b"timers_promises".as_ptr(),
                "timers_promises".len() as u32,
            )
        };
    }
    crate::object::native_module_get_builtin_module_value(module_name)
}

fn throw_module_not_found(specifier: &str) -> ! {
    let message = format!("Cannot find module '{}'", specifier);
    crate::fs::validate::throw_error_with_code(&message, "MODULE_NOT_FOUND")
}

fn throw_unsupported_package_require(specifier: &str) -> ! {
    let message = format!(
        "Perry createRequire() currently supports built-in modules only; package/file require('{}') is not supported under perry compile. Use ESM import syntax and perry.compilePackages instead.",
        specifier
    );
    crate::fs::validate::throw_error_with_code(&message, "ERR_PERRY_UNSUPPORTED_CREATE_REQUIRE")
}

extern "C" fn require_thunk(_closure: *const ClosureHeader, id: f64) -> f64 {
    let specifier = value_to_string(id, "id");
    if specifier.is_empty() {
        let message = "The argument 'id' must be a non-empty string";
        crate::fs::validate::throw_type_error_with_code(message, "ERR_INVALID_ARG_VALUE");
    }
    let Some(module_name) = supported_require_builtin(&specifier) else {
        throw_unsupported_package_require(&specifier);
    };
    require_builtin_value(module_name)
}

extern "C" fn resolve_thunk(_closure: *const ClosureHeader, request: f64, _options: f64) -> f64 {
    let specifier = value_to_string(request, "request");
    if let Some(resolved) = resolve_builtin(&specifier) {
        return string_value(resolved);
    }
    throw_module_not_found(&specifier)
}

extern "C" fn resolve_paths_thunk(_closure: *const ClosureHeader, request: f64) -> f64 {
    let specifier = value_to_string(request, "request");
    if supported_require_builtin(&specifier).is_some() {
        return null();
    }
    null()
}

extern "C" fn extension_noop_thunk(
    _closure: *const ClosureHeader,
    _module: f64,
    _filename: f64,
) -> f64 {
    undefined()
}

fn extensions_object() -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = js_object_alloc(0, 3);
    let obj_handle = scope.root_raw_mut_ptr(obj);
    for name in [".js", ".json", ".node"] {
        let (_, value) = named_closure(extension_noop_thunk as *const u8, 2, 2, name);
        let value_handle = scope.root_nanbox_f64(value);
        set_field(
            obj_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>(),
            name,
            value_handle.get_nanbox_f64(),
        );
    }
    object_value(obj_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>())
}

fn make_require(main_value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let (_, paths_value) = named_closure(resolve_paths_thunk as *const u8, 1, 1, "paths");
    let paths_handle = scope.root_nanbox_f64(paths_value);
    let (resolve_closure, resolve_value) =
        named_closure(resolve_thunk as *const u8, 2, 2, "resolve");
    let resolve_handle = scope.root_nanbox_f64(resolve_value);
    set_closure_prop(resolve_closure, "paths", paths_handle.get_nanbox_f64());

    let cache_handle = scope.root_nanbox_f64(object_value(js_object_alloc(0, 0)));
    let extensions_handle = scope.root_nanbox_f64(extensions_object());

    let (require_closure, require_value) =
        named_closure(require_thunk as *const u8, 1, 1, "require");
    let require_handle = scope.root_nanbox_f64(require_value);
    set_closure_prop(require_closure, "resolve", resolve_handle.get_nanbox_f64());
    set_closure_prop(require_closure, "cache", cache_handle.get_nanbox_f64());
    set_closure_prop(
        require_closure,
        "extensions",
        extensions_handle.get_nanbox_f64(),
    );
    set_closure_prop(require_closure, "main", main_value);
    require_handle.get_nanbox_f64()
}

#[no_mangle]
pub extern "C" fn js_module_create_require(filename_or_url: f64) -> f64 {
    validate_create_require_base(filename_or_url);
    make_require(undefined())
}

/// Ambient `require` for compiled external / `compilePackages` modules (Tier 1 of
/// #5389, fixes #5373). These modules carry no CJS ambient `require` binding, so a
/// bare or computed `require(expr)` would otherwise lower to
/// `js_global_get_or_throw_unresolved` and throw `ReferenceError: require is not
/// defined`. This returns the same `createRequire`-backed closure as
/// `js_module_create_require`, but takes no base argument (it is produced where a
/// bare `require` identifier appears, not from an explicit `createRequire(base)`).
/// Builtins resolve by string today; package/file specifiers throw the descriptive
/// `ERR_PERRY_UNSUPPORTED_CREATE_REQUIRE` until Tier 2 lands static package
/// resolution.
#[no_mangle]
pub extern "C" fn js_module_ambient_require() -> f64 {
    make_require(undefined())
}

/// Keepalive anchor for the auto-optimize whole-program build (generated-code-only
/// callee; see project_auto_optimize_keepalive_3320).
#[used]
static KEEP_JS_MODULE_AMBIENT_REQUIRE: extern "C" fn() -> f64 = js_module_ambient_require;
