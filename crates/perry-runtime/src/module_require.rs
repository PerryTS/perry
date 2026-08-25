//! Minimal `node:module.createRequire` / CommonJS `require` bridge.
//!
//! This intentionally covers Perry's deterministic native-builtin path and the
//! public function shape. Full CommonJS file/package resolution remains in the
//! compiler-side CJS wrapper and future `Module._*` work.

use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_set_capture_f64,
    js_register_closure_arity, ClosureHeader,
};
use crate::object::{js_object_alloc, js_object_get_field_by_name, js_object_set_field_by_name};
use crate::string::js_string_from_bytes;
use crate::value::{js_nanbox_pointer, JSValue, TAG_FALSE, TAG_NULL, TAG_TRUE, TAG_UNDEFINED};

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
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let value_handle = scope.root_nanbox_f64(value);
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    obj_handle.with_mut_ptr(|obj: *mut crate::object::ObjectHeader| {
        js_object_set_field_by_name(obj, key, value_handle.get_nanbox_f64());
    });
}

/// `set_field` for an object still named only by its handle. The raw address
/// never gets bound at the call site, so the rooting ratchet has nothing to
/// count and no copy can outlive a collection; `set_field` re-roots the
/// pointer it is handed before allocating the key.
fn set_field_rooted(handle: &crate::gc::RuntimeHandle<'_>, name: &str, value: f64) {
    handle.with_mut_ptr(|obj: *mut crate::object::ObjectHeader| set_field(obj, name, value));
}

fn set_closure_prop(closure: *mut ClosureHeader, name: &str, value: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let closure_handle = scope.root_raw_mut_ptr(closure);
    let value_handle = scope.root_nanbox_f64(value);
    closure_handle.with_mut_ptr(|closure: *mut ClosureHeader| {
        crate::closure::closure_set_dynamic_prop(
            closure as usize,
            name,
            value_handle.get_nanbox_f64(),
        );
    });
}

fn named_closure(
    func: *const u8,
    arity: u32,
    length: u32,
    name: &str,
) -> (*mut ClosureHeader, f64) {
    js_register_closure_arity(func, arity);
    crate::closure::js_register_closure_length(func, length);
    let closure = js_closure_alloc(func, 1);
    let scope = crate::gc::RuntimeHandleScope::new();
    let closure_handle = scope.root_raw_mut_ptr(closure);
    closure_handle.with_mut_ptr(|closure: *mut ClosureHeader| {
        crate::object::set_bound_native_closure_name(closure, name);
        crate::object::set_builtin_closure_length(closure as usize, length);
    });
    closure_handle
        .with_mut_ptr(|closure: *mut ClosureHeader| (closure, js_nanbox_pointer(closure as i64)))
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

/// #6651 (pi wall #5, same family as #6644's wall #3): this used to be a
/// hand-copied allowlist that drifted from `process.getBuiltinModule`'s and
/// from the static-import tables — `v8` (and `sea`, `fs/promises`,
/// `stream/consumers`, `stream/web`, `trace_events`, `test/reporters`) were
/// implemented and statically importable but rejected here as "package/file".
/// Both resolvers now share one source of truth (`MODULE_BUILTIN_MODULES`,
/// i.e. `module.builtinModules`), including the `node:` normalization and the
/// scheme-only / `_`-internal carve-outs.
fn supported_require_builtin(specifier: &str) -> Option<&str> {
    crate::process::supported_builtin_module_name(specifier)
}

fn resolve_builtin(specifier: &str) -> Option<&str> {
    supported_require_builtin(specifier).map(|_| specifier)
}

fn require_builtin_value(module_name: &str) -> f64 {
    // #6651: shared routing with `process.getBuiltinModule` — submodule-spec
    // modules (diagnostics_channel, timers/promises, fs/promises, …) resolve
    // through the node_submodules registry, the rest through the native-module
    // namespace.
    crate::process::builtin_module_value(module_name)
}

fn throw_module_not_found(specifier: &str) -> ! {
    let message = format!("Cannot find module '{}'", specifier);
    crate::fs::validate::throw_error_with_code(&message, "MODULE_NOT_FOUND")
}

fn throw_require_module_not_found(specifier: &str, base: &std::path::Path) -> ! {
    let message = format!("Cannot find module '{specifier}'");
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, "MODULE_NOT_FOUND");
    let error = crate::error::js_error_new_with_message(msg);
    let scope = crate::gc::RuntimeHandleScope::new();
    let error_handle = scope.root_raw_mut_ptr(error);
    let stack = crate::array::js_array_alloc_with_length(1);
    let stack_handle = scope.root_raw_mut_ptr(stack);
    let base_value = string_value(&base.to_string_lossy());
    stack_handle.with_mut_ptr(|stack: *mut crate::array::ArrayHeader| {
        crate::array::js_array_set_f64(stack, 0, base_value);
    });
    let stack_value = stack_handle.with_mut_ptr(|stack: *mut crate::array::ArrayHeader| {
        f64::from_bits(JSValue::array_ptr(stack).bits())
    });
    let error_value = error_handle.with_mut_ptr(|error: *mut crate::object::ObjectHeader| {
        let error_value = js_nanbox_pointer(error as i64);
        unsafe {
            crate::object::exotic_expando::exotic_set_property(
                error as usize,
                crate::object::exotic_expando::ExoticKind::Error,
                "requireStack",
                stack_value,
                error_value,
            );
        }
        error_value
    });
    crate::exception::js_throw(error_value)
}

fn throw_package_path_not_exported(specifier: &str) -> ! {
    let message = format!("Package subpath '{specifier}' is not defined by \"exports\"");
    crate::fs::validate::throw_error_with_code(&message, "ERR_PACKAGE_PATH_NOT_EXPORTED")
}

#[derive(Clone, Copy)]
enum ResolveError {
    NotFound,
    NotExported,
}

fn require_base_dir(closure: *const ClosureHeader) -> std::path::PathBuf {
    let path = std::path::PathBuf::from(require_base_filename(closure));
    path.parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf()
}

fn require_base_filename(closure: *const ClosureHeader) -> String {
    if closure.is_null() {
        return std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join("__perry_ambient.cjs")
            .to_string_lossy()
            .into_owned();
    }
    value_to_string(js_closure_get_capture_f64(closure, 0), "filename")
}

fn resolve_file(path: &std::path::Path) -> Option<std::path::PathBuf> {
    if path.is_file() {
        return Some(std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()));
    }
    for ext in ["js", "json", "node"] {
        let mut candidate = path.as_os_str().to_os_string();
        candidate.push(".");
        candidate.push(ext);
        let candidate = std::path::PathBuf::from(candidate);
        if candidate.is_file() {
            return Some(std::fs::canonicalize(&candidate).unwrap_or(candidate));
        }
    }
    if path.is_dir() {
        if let Ok(text) = std::fs::read_to_string(path.join("package.json")) {
            if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(main) = manifest.get("main").and_then(|v| v.as_str()) {
                    if let Some(found) = resolve_file(&path.join(main)) {
                        return Some(found);
                    }
                }
            }
        }
        for ext in ["js", "json", "node", "cjs"] {
            let candidate = path.join(format!("index.{ext}"));
            if candidate.is_file() {
                return Some(std::fs::canonicalize(&candidate).unwrap_or(candidate));
            }
        }
    }
    None
}

fn package_parts(specifier: &str) -> (&str, Option<&str>) {
    if specifier.starts_with('@') {
        let mut parts = specifier.splitn(3, '/');
        let scope = parts.next().unwrap_or(specifier);
        let name = parts.next().unwrap_or("");
        let package_len = scope.len() + 1 + name.len();
        (&specifier[..package_len.min(specifier.len())], parts.next())
    } else {
        specifier
            .split_once('/')
            .map_or((specifier, None), |(p, s)| (p, Some(s)))
    }
}

fn resolve_exports(value: &serde_json::Value, key: &str) -> Option<String> {
    match value {
        serde_json::Value::String(target) => Some(target.clone()),
        serde_json::Value::Array(items) => items.iter().find_map(|v| resolve_exports(v, key)),
        serde_json::Value::Object(map) => {
            if let Some(target) = map.get(key) {
                return resolve_exports(target, key);
            }
            for (condition, target) in map {
                if matches!(condition.as_str(), "node" | "require" | "default") {
                    if let Some(found) = resolve_exports(target, key) {
                        return Some(found);
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn resolve_request(
    base: &std::path::Path,
    specifier: &str,
) -> Result<std::path::PathBuf, ResolveError> {
    if specifier.starts_with('/') {
        return resolve_file(std::path::Path::new(specifier)).ok_or(ResolveError::NotFound);
    }
    if specifier.starts_with("./") || specifier.starts_with("../") {
        return resolve_file(&base.join(specifier)).ok_or(ResolveError::NotFound);
    }
    let (package, subpath) = package_parts(specifier);
    for ancestor in base.ancestors() {
        let package_dir = ancestor.join("node_modules").join(package);
        if !package_dir.is_dir() {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(package_dir.join("package.json")) {
            if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(exports) = manifest.get("exports") {
                    let key = subpath
                        .map(|s| format!("./{s}"))
                        .unwrap_or_else(|| ".".into());
                    let target = resolve_exports(exports, &key).ok_or(ResolveError::NotExported)?;
                    return resolve_file(&package_dir.join(target)).ok_or(ResolveError::NotFound);
                }
                if let Some(subpath) = subpath {
                    return resolve_file(&package_dir.join(subpath)).ok_or(ResolveError::NotFound);
                }
                if let Some(main) = manifest.get("main").and_then(|v| v.as_str()) {
                    if let Some(found) = resolve_file(&package_dir.join(main)) {
                        return Ok(found);
                    }
                }
            }
        }
        return resolve_file(&package_dir).ok_or(ResolveError::NotFound);
    }
    Err(ResolveError::NotFound)
}

fn object_ptr(value: f64) -> *mut crate::object::ObjectHeader {
    crate::value::js_nanbox_get_pointer(value) as *mut crate::object::ObjectHeader
}

fn cached_record(cache: f64, filename: &str) -> Option<(f64, f64)> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let cache_handle = scope.root_nanbox_f64(cache);
    let key = js_string_from_bytes(filename.as_ptr(), filename.len() as u32);
    let record = js_object_get_field_by_name(object_ptr(cache_handle.get_nanbox_f64()), key);
    if record.is_undefined() {
        return None;
    }
    let record_handle = scope.root_nanbox_f64(f64::from_bits(record.bits()));
    let exports_key = js_string_from_bytes(b"exports".as_ptr(), 7);
    let exports = f64::from_bits(
        js_object_get_field_by_name(object_ptr(record_handle.get_nanbox_f64()), exports_key).bits(),
    );
    Some((record_handle.get_nanbox_f64(), exports))
}

fn cache_exports(cache: f64, filename: &str, exports: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let cache_handle = scope.root_nanbox_f64(cache);
    let exports_handle = scope.root_nanbox_f64(exports);
    let record = js_object_alloc(0, 5);
    let record_handle = scope.root_raw_mut_ptr(record);
    let id = string_value(filename);
    set_field_rooted(&record_handle, "id", id);
    let filename_value = string_value(filename);
    set_field_rooted(&record_handle, "filename", filename_value);
    set_field_rooted(&record_handle, "exports", exports_handle.get_nanbox_f64());
    set_field_rooted(
        &record_handle,
        "loaded",
        f64::from_bits(crate::value::TAG_TRUE),
    );
    let children = crate::array::js_array_alloc_with_length(0);
    let children_handle = scope.root_raw_mut_ptr(children);
    let children_value =
        children_handle.with_mut_ptr(|children: *mut crate::array::ArrayHeader| {
            f64::from_bits(JSValue::array_ptr(children).bits())
        });
    set_field_rooted(&record_handle, "children", children_value);
    let key = js_string_from_bytes(filename.as_ptr(), filename.len() as u32);
    let record_value =
        record_handle.with_mut_ptr(|record: *mut crate::object::ObjectHeader| object_value(record));
    js_object_set_field_by_name(object_ptr(cache_handle.get_nanbox_f64()), key, record_value);
    record_value
}

fn cjs_record_exports(value: f64) -> Option<f64> {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let record_handle = scope.root_nanbox_f64(value);
    let marker_key = js_string_from_bytes(b"__perry_cjs_record".as_ptr(), 18);
    let record = object_ptr(record_handle.get_nanbox_f64());
    let marker = js_object_get_field_by_name(record, marker_key);
    if !marker.is_bool() || !marker.as_bool() {
        return None;
    }
    let exports_key = js_string_from_bytes(b"exports".as_ptr(), 7);
    let exports = f64::from_bits(
        js_object_get_field_by_name(object_ptr(record_handle.get_nanbox_f64()), exports_key).bits(),
    );
    Some(exports)
}

fn cjs_record_field(value: f64, name: &str) -> Option<f64> {
    if cjs_record_exports(value).is_none() {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let record_handle = scope.root_nanbox_f64(value);
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    Some(f64::from_bits(
        js_object_get_field_by_name(object_ptr(record_handle.get_nanbox_f64()), key).bits(),
    ))
}

/// The value a completed path module published, WITHOUT triggering a deferred
/// initializer. `js_require_path_module` is the initializing read; this is the
/// "did it already run?" read that `require()` needs before it decides to run
/// one.
fn registered_path_module_value(path: &str) -> Option<f64> {
    let key = canonicalize_module_path(path);
    MODULE_PATH_REGISTRY
        .with(|registry| registry.published_exports(&key))
        .map(f64::from_bits)
}

/// The registry holds whatever the CommonJS wrapper published: the module
/// RECORD once the wrapper reaches its tail, or bare partial exports while a
/// cycle is still initializing. Generated `require` sites want the exports in
/// both cases.
fn path_module_exports(bits: u64) -> f64 {
    let value = f64::from_bits(bits);
    cjs_record_exports(value).unwrap_or(value)
}

fn link_parent(cache: f64, record: f64, parent_filename: &str) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let cache_handle = scope.root_nanbox_f64(cache);
    let record_handle = scope.root_nanbox_f64(record);
    let parent_key = js_string_from_bytes(b"parent".as_ptr(), 6);
    let existing =
        js_object_get_field_by_name(object_ptr(record_handle.get_nanbox_f64()), parent_key);
    if !existing.is_undefined() {
        return;
    }
    let cache_key = js_string_from_bytes(parent_filename.as_ptr(), parent_filename.len() as u32);
    let cached_parent =
        js_object_get_field_by_name(object_ptr(cache_handle.get_nanbox_f64()), cache_key);
    let parent = scope.root_nanbox_f64(if cached_parent.is_undefined() {
        let parent = js_object_alloc(0, 2);
        let parent_handle = scope.root_raw_mut_ptr(parent);
        let id = string_value(parent_filename);
        set_field_rooted(&parent_handle, "id", id);
        let children = scope.root_nanbox_f64(f64::from_bits(
            JSValue::array_ptr(crate::array::js_array_alloc_with_length(0)).bits(),
        ));
        set_field_rooted(&parent_handle, "children", children.get_nanbox_f64());
        parent_handle.with_mut_ptr(|parent: *mut crate::object::ObjectHeader| object_value(parent))
    } else {
        f64::from_bits(cached_parent.bits())
    });
    set_field(
        object_ptr(record_handle.get_nanbox_f64()),
        "parent",
        parent.get_nanbox_f64(),
    );
    let children_key = js_string_from_bytes(b"children".as_ptr(), 8);
    let children = js_object_get_field_by_name(object_ptr(parent.get_nanbox_f64()), children_key);
    let mut children_ptr = if children.is_pointer() {
        let ptr = children.as_pointer::<u8>();
        if unsafe { crate::value::addr_class::try_read_gc_header(ptr as usize) }
            .is_some_and(|header| header.obj_type == crate::gc::GC_TYPE_ARRAY)
        {
            ptr as *mut crate::array::ArrayHeader
        } else {
            std::ptr::null_mut()
        }
    } else {
        std::ptr::null_mut()
    };
    if children_ptr.is_null() {
        let children = scope.root_nanbox_f64(f64::from_bits(
            JSValue::array_ptr(crate::array::js_array_alloc_with_length(0)).bits(),
        ));
        js_object_set_field_by_name(
            object_ptr(parent.get_nanbox_f64()),
            children_key,
            children.get_nanbox_f64(),
        );
        children_ptr = crate::value::js_nanbox_get_pointer(children.get_nanbox_f64())
            as *mut crate::array::ArrayHeader;
    }
    if crate::array::js_array_includes_f64(children_ptr, record_handle.get_nanbox_f64()) == 0 {
        let children_ptr =
            crate::array::js_array_push_f64(children_ptr, record_handle.get_nanbox_f64());
        js_object_set_field_by_name(
            object_ptr(parent.get_nanbox_f64()),
            children_key,
            f64::from_bits(JSValue::array_ptr(children_ptr).bits()),
        );
    }
}

struct PendingRequireParentGuard;

impl Drop for PendingRequireParentGuard {
    fn drop(&mut self) {
        PENDING_REQUIRE_PARENT.with(|pending| {
            pending.borrow_mut().take();
        });
    }
}

fn require_path(cache: f64, path: &std::path::Path, parent_filename: &str) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let cache_handle = scope.root_nanbox_f64(cache);
    let filename = path.to_string_lossy();
    if let Some((record, exports)) = cached_record(cache_handle.get_nanbox_f64(), &filename) {
        link_parent(cache_handle.get_nanbox_f64(), record, parent_filename);
        return exports;
    }
    let mut registered = registered_path_module_value(&filename).unwrap_or_else(|| {
        PENDING_REQUIRE_PARENT.with(|pending| {
            *pending.borrow_mut() = Some(parent_filename.to_string());
        });
        let _pending_parent_guard = PendingRequireParentGuard;
        let result = js_require_path_module(string_value(&filename));
        registered_path_module_value(&filename).unwrap_or(result)
    });
    if let Some((record, exports)) = cached_record(cache_handle.get_nanbox_f64(), &filename) {
        link_parent(cache_handle.get_nanbox_f64(), record, parent_filename);
        run_custom_extension(path, record, &filename);
        return exports;
    }
    if cjs_record_field(registered, "loaded").is_some_and(|loaded| {
        JSValue::from_bits(loaded.to_bits()).is_bool()
            && JSValue::from_bits(loaded.to_bits()).as_bool()
    }) {
        if let Some(factory) = cjs_record_field(registered, "__perry_cjs_factory") {
            let factory_value = JSValue::from_bits(factory.to_bits());
            if factory_value.is_pointer()
                && crate::closure::is_closure_ptr(
                    crate::value::js_nanbox_get_pointer(factory) as usize
                )
            {
                let factory_handle = scope.root_nanbox_f64(factory);
                let exports = crate::closure::js_closure_call0(
                    crate::value::js_nanbox_get_pointer(factory_handle.get_nanbox_f64())
                        as *const ClosureHeader,
                );
                if let Some((record, cached)) =
                    cached_record(cache_handle.get_nanbox_f64(), &filename)
                {
                    link_parent(cache_handle.get_nanbox_f64(), record, parent_filename);
                    run_custom_extension(path, record, &filename);
                    return cached;
                }
                let exports_handle = scope.root_nanbox_f64(exports);
                let (record, value) =
                    if let Some(value) = cjs_record_exports(exports_handle.get_nanbox_f64()) {
                        let key = js_string_from_bytes(filename.as_ptr(), filename.len() as u32);
                        js_object_set_field_by_name(
                            object_ptr(cache_handle.get_nanbox_f64()),
                            key,
                            exports_handle.get_nanbox_f64(),
                        );
                        (exports_handle.get_nanbox_f64(), value)
                    } else {
                        (
                            cache_exports(
                                cache_handle.get_nanbox_f64(),
                                &filename,
                                exports_handle.get_nanbox_f64(),
                            ),
                            exports_handle.get_nanbox_f64(),
                        )
                    };
                let record_handle = scope.root_nanbox_f64(record);
                link_parent(
                    cache_handle.get_nanbox_f64(),
                    record_handle.get_nanbox_f64(),
                    parent_filename,
                );
                run_custom_extension(path, record_handle.get_nanbox_f64(), &filename);
                return value;
            }
        }
        registered = registered_path_module_value(&filename).unwrap_or(registered);
    }
    let registered_handle = scope.root_nanbox_f64(registered);
    let exports = if let Some(exports) = cjs_record_exports(registered_handle.get_nanbox_f64()) {
        exports
    } else if !JSValue::from_bits(registered_handle.get_nanbox_f64().to_bits()).is_undefined() {
        registered_handle.get_nanbox_f64()
    } else if path.extension().and_then(|e| e.to_str()) == Some("json") {
        js_require_json_disk(string_value(&filename))
    } else {
        if JSValue::from_bits(registered_handle.get_nanbox_f64().to_bits()).is_undefined() {
            throw_module_not_found(&filename);
        }
        registered_handle.get_nanbox_f64()
    };
    let exports_handle = scope.root_nanbox_f64(exports);
    let record = if cjs_record_exports(registered_handle.get_nanbox_f64()).is_some() {
        let key = js_string_from_bytes(filename.as_ptr(), filename.len() as u32);
        js_object_set_field_by_name(
            object_ptr(cache_handle.get_nanbox_f64()),
            key,
            registered_handle.get_nanbox_f64(),
        );
        registered_handle.get_nanbox_f64()
    } else {
        cache_exports(
            cache_handle.get_nanbox_f64(),
            &filename,
            exports_handle.get_nanbox_f64(),
        )
    };
    let record_handle = scope.root_nanbox_f64(record);
    if cjs_record_exports(record_handle.get_nanbox_f64()).is_some() {
        set_field(
            object_ptr(record_handle.get_nanbox_f64()),
            "loaded",
            f64::from_bits(crate::value::TAG_TRUE),
        );
    }
    link_parent(
        cache_handle.get_nanbox_f64(),
        record_handle.get_nanbox_f64(),
        parent_filename,
    );
    run_custom_extension(path, record_handle.get_nanbox_f64(), &filename);
    exports_handle.get_nanbox_f64()
}

fn run_custom_extension(path: &std::path::Path, record: f64, filename: &str) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let record_handle = scope.root_nanbox_f64(record);
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        let extension = format!(".{extension}");
        if !matches!(extension.as_str(), ".js" | ".json" | ".node" | ".cjs") {
            let extensions_handle =
                scope.root_nanbox_f64(crate::object::module_cjs_extensions_value());
            let key = js_string_from_bytes(extension.as_ptr(), extension.len() as u32);
            let handler =
                js_object_get_field_by_name(object_ptr(extensions_handle.get_nanbox_f64()), key);
            if !handler.is_undefined() && !handler.is_null() {
                let handler_handle = scope.root_nanbox_f64(f64::from_bits(handler.bits()));
                let handler_ptr =
                    crate::value::js_nanbox_get_pointer(handler_handle.get_nanbox_f64()) as usize;
                if !handler.is_pointer() || !crate::closure::is_closure_ptr(handler_ptr) {
                    crate::process::module_throw_plain_type_error(
                        "Module._extensions[extension] is not a function",
                    );
                }
                let filename_value = string_value(&filename);
                crate::closure::js_closure_call2(
                    handler_ptr as *mut ClosureHeader,
                    record_handle.get_nanbox_f64(),
                    filename_value,
                );
            }
        }
    }
}

extern "C" fn require_thunk(closure: *const ClosureHeader, id: f64) -> f64 {
    let specifier = value_to_string(id, "id");
    if specifier.is_empty() {
        let message = "The argument 'id' must be a non-empty string";
        crate::fs::validate::throw_type_error_with_code(message, "ERR_INVALID_ARG_VALUE");
    }
    if let Some(module_name) = supported_require_builtin(&specifier) {
        return require_builtin_value(module_name);
    }
    let base = require_base_dir(closure);
    let parent_filename = require_base_filename(closure);
    match resolve_request(&base, &specifier) {
        Ok(path) => require_path(
            crate::object::module_cjs_cache_value(),
            &path,
            &parent_filename,
        ),
        Err(ResolveError::NotExported) => throw_package_path_not_exported(&specifier),
        Err(ResolveError::NotFound) => throw_require_module_not_found(&specifier, &base),
    }
}

extern "C" fn resolve_thunk(closure: *const ClosureHeader, request: f64, _options: f64) -> f64 {
    let specifier = value_to_string(request, "request");
    if let Some(resolved) = resolve_builtin(&specifier) {
        return string_value(resolved);
    }
    let base = require_base_dir(closure);
    match resolve_request(&base, &specifier) {
        Ok(path) => string_value(&path.to_string_lossy()),
        Err(ResolveError::NotExported) => throw_package_path_not_exported(&specifier),
        Err(ResolveError::NotFound) => throw_require_module_not_found(&specifier, &base),
    }
}

extern "C" fn resolve_paths_thunk(closure: *const ClosureHeader, request: f64) -> f64 {
    let specifier = value_to_string(request, "request");
    if supported_require_builtin(&specifier).is_some() {
        return null();
    }
    let base = require_base_dir(closure);
    let paths: Vec<_> = if specifier.starts_with("./") || specifier.starts_with("../") {
        vec![base.to_string_lossy().into_owned()]
    } else {
        base.ancestors()
            .map(|dir| dir.join("node_modules").to_string_lossy().into_owned())
            .collect()
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr = crate::array::js_array_alloc_with_length(paths.len() as u32);
    let arr_handle = scope.root_raw_mut_ptr(arr);
    for (index, path) in paths.iter().enumerate() {
        let path_value = string_value(path);
        arr_handle.with_mut_ptr(|arr: *mut crate::array::ArrayHeader| {
            crate::array::js_array_set_f64(arr, index as u32, path_value);
        });
    }
    arr_handle.with_mut_ptr(|arr: *mut crate::array::ArrayHeader| {
        f64::from_bits(JSValue::array_ptr(arr).bits())
    })
}

fn make_require(base: f64, main_value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let base_handle = scope.root_nanbox_f64(base);
    let main_handle = scope.root_nanbox_f64(main_value);
    let (_, paths_value) = named_closure(resolve_paths_thunk as *const u8, 1, 1, "paths");
    let paths_handle = scope.root_nanbox_f64(paths_value);
    js_closure_set_capture_f64(
        object_ptr(paths_handle.get_nanbox_f64()) as *mut ClosureHeader,
        0,
        base_handle.get_nanbox_f64(),
    );
    let (_, resolve_value) = named_closure(resolve_thunk as *const u8, 2, 2, "resolve");
    let resolve_handle = scope.root_nanbox_f64(resolve_value);
    js_closure_set_capture_f64(
        object_ptr(resolve_handle.get_nanbox_f64()) as *mut ClosureHeader,
        0,
        base_handle.get_nanbox_f64(),
    );
    set_closure_prop(
        object_ptr(resolve_handle.get_nanbox_f64()) as *mut ClosureHeader,
        "paths",
        paths_handle.get_nanbox_f64(),
    );
    let resolve_prototype = scope.root_nanbox_f64(object_value(js_object_alloc(0, 0)));
    set_closure_prop(
        object_ptr(resolve_handle.get_nanbox_f64()) as *mut ClosureHeader,
        "prototype",
        resolve_prototype.get_nanbox_f64(),
    );

    let cache_handle = scope.root_nanbox_f64(crate::object::module_cjs_cache_value());
    let extensions_handle = scope.root_nanbox_f64(crate::object::module_cjs_extensions_value());

    let (_, require_value) = named_closure(require_thunk as *const u8, 1, 1, "require");
    let require_handle = scope.root_nanbox_f64(require_value);
    js_closure_set_capture_f64(
        object_ptr(require_handle.get_nanbox_f64()) as *mut ClosureHeader,
        0,
        base_handle.get_nanbox_f64(),
    );
    let require_ptr = || object_ptr(require_handle.get_nanbox_f64()) as *mut ClosureHeader;
    set_closure_prop(require_ptr(), "resolve", resolve_handle.get_nanbox_f64());
    set_closure_prop(require_ptr(), "cache", cache_handle.get_nanbox_f64());
    set_closure_prop(
        require_ptr(),
        "extensions",
        extensions_handle.get_nanbox_f64(),
    );
    set_closure_prop(require_ptr(), "main", main_handle.get_nanbox_f64());
    let require_prototype = scope.root_nanbox_f64(object_value(js_object_alloc(0, 0)));
    set_closure_prop(
        require_ptr(),
        "prototype",
        require_prototype.get_nanbox_f64(),
    );
    require_handle.get_nanbox_f64()
}

#[no_mangle]
pub extern "C" fn js_module_create_require(filename_or_url: f64) -> f64 {
    validate_create_require_base(filename_or_url);
    let base = crate::url::node_compat::module_base_to_path(filename_or_url)
        .unwrap_or_else(|| value_to_string(filename_or_url, "filename"));
    make_require(string_value(&base), undefined())
}

/// Devirt codegen entry for `module.createRequire(...)` (#6644). The require
/// closure it returns resolves builtins from a RUNTIME string, so — exactly like
/// `js_process_get_builtin_module_devirt` — codegen could not emit the precise
/// per-module dispatch installs. Arm both install-all hooks so a dynamically
/// required module's methods (`require('node:diagnostics_channel').channel(...)`,
/// `require('tls').connect(...)`) can dispatch. Codegen targets THIS symbol, so
/// the all-buckets `js_nm_install_all` / `js_node_submod_install_all` are
/// referenced only by programs whose source actually calls `createRequire`; the
/// plain `js_module_create_require` (reachable from the always-pinned ambient
/// require keepalives via the module dispatch bucket) stays free of that
/// reference, preserving per-module stripping.
#[no_mangle]
pub extern "C" fn js_module_create_require_devirt(filename_or_url: f64) -> f64 {
    crate::object::js_nm_enable_install_all();
    crate::node_submodules::js_node_submod_enable_install_all();
    js_module_create_require(filename_or_url)
}

mod path_registry;

use path_registry::{PathModuleRequireError, MODULE_PATH_REGISTRY};

crate::perry_thread_local! {
    static PENDING_REQUIRE_PARENT: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

fn canonicalize_module_path(path: &str) -> String {
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string())
}

/// Codegen FFI: record that `<prefix>__init` (address `init_addr`) initializes
/// the module whose absolute source path is `path_value`. Emitted once per
/// Deferred `.next/server/**` module at the top of the executable or app-dylib
/// entry point. The registry records the address without executing it.
/// # Safety
/// `path_ptr`/`path_len` describe a valid UTF-8 byte range (a codegen string
/// constant). `init_addr` is the address of an `extern "C" fn()` module
/// initializer (from `ptrtoint` of the symbol).
#[no_mangle]
pub unsafe extern "C" fn js_register_path_init(path_ptr: *const u8, path_len: i64, init_addr: i64) {
    let slice = std::slice::from_raw_parts(path_ptr, path_len as usize);
    let path = String::from_utf8_lossy(slice).into_owned();
    let key = canonicalize_module_path(&path);
    if !MODULE_PATH_REGISTRY.with(|r| r.register_init(key.clone(), init_addr as usize)) {
        eprintln!("perry: rejected duplicate path-module initializer for canonical path {key}");
    }
}

/// Codegen FFI: publish a CommonJS module's initial `exports` object before
/// executing its body. This is visible only to recursive loads by the owning
/// thread; concurrent callers wait for [`js_register_path_module`] and the
/// generated initializer to complete.
#[no_mangle]
pub extern "C" fn js_register_path_module_partial(path_value: f64, exports: f64) {
    let path = value_to_string(path_value, "path");
    let key = canonicalize_module_path(&path);
    if !MODULE_PATH_REGISTRY.with(|r| r.register_partial_exports(key, exports.to_bits())) {
        crate::fs::validate::throw_error_with_code(
            "Perry rejected path-module partial exports from a non-owner initializer",
            "ERR_PERRY_PATH_MODULE_OWNER",
        );
    }
}

/// Codegen FFI: link `module.parent` / `parent.children` for a CommonJS
/// wrapper that a runtime `require()` triggered. Emitted in the PREAMBLE,
/// before the body, because Node has `module.parent` populated while the body
/// evaluates (`exports.parentId = module.parent && module.parent.id` is the
/// shape the node-suite pins).
///
/// `link_parent` resolves the parent through the shared `require.cache`, so
/// `child.parent === require.cache[parentPath]` holds by IDENTITY; minting a
/// fresh `{ id }` object here would satisfy `.id` and fail `===`. This does
/// NOT touch the path-module registry — publication stays with the
/// partial/final pair so the cycle state machine is untouched.
#[no_mangle]
pub extern "C" fn js_link_path_module_parent(record: f64) {
    let Some(parent_filename) = PENDING_REQUIRE_PARENT.with(|pending| pending.borrow_mut().take())
    else {
        return;
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let record = scope.root_nanbox_f64(record);
    if cjs_record_exports(record.get_nanbox_f64()).is_none() {
        return;
    }
    link_parent(
        crate::object::module_cjs_cache_value(),
        record.get_nanbox_f64(),
        &parent_filename,
    );
}

/// Codegen FFI: register an AOT-compiled module's exports under its absolute
/// source path (emitted at the tail of each CJS wrapper). See
/// [`MODULE_PATH_REGISTRY`].
#[no_mangle]
pub extern "C" fn js_register_path_module(path_value: f64, exports: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let exports = scope.root_nanbox_f64(exports);
    let path = value_to_string(path_value, "path");
    let key = canonicalize_module_path(&path);
    if !MODULE_PATH_REGISTRY
        .with(|r| r.register_final_exports(key, exports.get_nanbox_f64().to_bits()))
    {
        crate::fs::validate::throw_error_with_code(
            "Perry rejected path-module final exports from a non-owner initializer",
            "ERR_PERRY_PATH_MODULE_OWNER",
        );
    }
}

/// Execute a generated module-init body behind a native exception boundary.
/// If a CommonJS wrapper published partial exports and then threw, cache the
/// exact value and wake waiters before propagating it. The boundary lives here
/// rather than in generated JavaScript so top-level lexical declarations keep
/// their original module/function scope.
///
/// # Safety
/// `init_addr` is codegen's `ptrtoint` of an `extern "C" fn()` module body.
#[no_mangle]
pub unsafe extern "C" fn js_run_module_init_catching(init_addr: i64) {
    let init_fn: extern "C" fn() = std::mem::transmute::<usize, _>(init_addr as usize);
    let boundary = MODULE_PATH_REGISTRY.with(|r| r.begin_module_boundary());
    let outcome = crate::exception::js_call_catching(|| {
        init_fn();
        undefined()
    });
    match outcome {
        Ok(_) => {
            MODULE_PATH_REGISTRY.with(|r| r.finish_module_boundary(boundary, None));
        }
        Err(error) => {
            MODULE_PATH_REGISTRY
                .with(|r| r.finish_module_boundary(boundary, Some(error.to_bits())));
            crate::exception::js_throw(error)
        }
    }
}

fn directory_module_candidates(key: &str) -> Vec<String> {
    let dir = std::path::Path::new(&key);
    if !dir.is_dir() {
        return Vec::new();
    }
    let mut candidates = Vec::new();
    if let Ok(manifest) = std::fs::read_to_string(dir.join("package.json")) {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&manifest) {
            if let Some(main) = parsed.get("main").and_then(|m| m.as_str()) {
                let main_path = dir.join(main);
                candidates.push(main_path.to_string_lossy().into_owned());
                if main_path.extension().is_none() {
                    candidates.push(format!("{}.js", main_path.to_string_lossy()));
                }
            }
        }
    }
    candidates.push(dir.join("index.js").to_string_lossy().into_owned());
    candidates
}

fn run_path_initializer(addr: usize) -> Result<(), u64> {
    // SAFETY: `addr` came from codegen's `ptrtoint` of an `extern "C" fn()`
    // module initializer and was accepted once for this canonical path.
    let init_fn: extern "C" fn() = unsafe { std::mem::transmute::<usize, _>(addr) };
    crate::exception::js_call_catching(|| {
        init_fn();
        undefined()
    })
    .map(|_| ())
    .map_err(f64::to_bits)
}

fn require_path_key(key: &str) -> Result<Option<u64>, PathModuleRequireError> {
    MODULE_PATH_REGISTRY.with(|r| r.require_with(key, &run_path_initializer))
}

/// Codegen FFI: resolve a runtime `require(absolutePath.js)` to an AOT module.
/// Initialization is once-only and waitable; recursive CommonJS loads receive
/// partial exports, while unrelated waiters receive only the final namespace.
#[no_mangle]
pub extern "C" fn js_require_path_module(path_value: f64) -> f64 {
    let path = value_to_string(path_value, "id");
    let key = canonicalize_module_path(&path);
    match require_path_key(&key) {
        Ok(Some(bits)) => return path_module_exports(bits),
        Err(PathModuleRequireError::Initializer(error)) => {
            crate::exception::js_throw(f64::from_bits(error))
        }
        Err(PathModuleRequireError::OwnershipConflict) => {
            crate::fs::validate::throw_error_with_code(
                "Perry rejected a cross-owner path-module initialization cycle",
                "ERR_PERRY_PATH_MODULE_OWNER",
            )
        }
        Ok(None) => {}
    }
    for candidate in directory_module_candidates(&key) {
        let candidate = canonicalize_module_path(&candidate);
        match require_path_key(&candidate) {
            Ok(Some(bits)) => return path_module_exports(bits),
            Err(PathModuleRequireError::Initializer(error)) => {
                crate::exception::js_throw(f64::from_bits(error))
            }
            Err(PathModuleRequireError::OwnershipConflict) => {
                crate::fs::validate::throw_error_with_code(
                    "Perry rejected a cross-owner path-module initialization cycle",
                    "ERR_PERRY_PATH_MODULE_OWNER",
                )
            }
            Ok(None) => {}
        }
    }
    undefined()
}

/// Presence bit paired with [`js_require_path_module`]. A real module may
/// export JavaScript `undefined`, so the CJS wrapper calls this only when the
/// returned value is undefined to distinguish that value from a registry miss.
#[no_mangle]
pub extern "C" fn js_has_path_module(path_value: f64) -> f64 {
    let path = value_to_string(path_value, "id");
    let key = canonicalize_module_path(&path);
    let found = MODULE_PATH_REGISTRY.with(|registry| {
        registry.has_exports(&key)
            || directory_module_candidates(&key)
                .into_iter()
                .map(|candidate| canonicalize_module_path(&candidate))
                .any(|candidate| registry.has_exports(&candidate))
    });
    f64::from_bits(if found { TAG_TRUE } else { TAG_FALSE })
}

/// Keep path-registry exports and cached exception values alive and rewrite
/// them when a copying collection moves their referents.
pub fn scan_module_path_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    // No owner gate: the table is per-heap, so the collector running this
    // scanner is by construction the one that owns every pointer in it. A
    // gate here would leave every heap but the first one unscanned.
    //
    // `try_with`, not `with`: a Coop app thread tears down its heap when the
    // deployment stops, and a scanner that ran during that teardown would
    // panic on the destroyed thread-local. A destroyed table holds no live
    // pointer worth rewriting, so reporting is the whole handling.
    let _ = MODULE_PATH_REGISTRY.try_with(|registry| registry.scan_roots(visitor));
}

#[cfg(test)]
pub(crate) fn test_store_path_module_root(key: &str, value_bits: u64) {
    assert!(MODULE_PATH_REGISTRY.with(|r| r.register_final_exports(key.to_string(), value_bits)));
}

#[cfg(test)]
pub(crate) fn test_remove_path_module_root(key: &str) {
    MODULE_PATH_REGISTRY.with(|r| r.remove_for_test(key));
}

/// Node-style `require.resolve` fallback for package-subpath specifiers that
/// were never statically required (e.g. Next's require-hook probing
/// `resolve('styled-jsx/package.json')`, unguarded before Next 16.2). Walks
/// `node_modules` directories upward from `from_dir`, trying the exact file,
/// then `.js`, `.json`, and `/index.js` — returning the absolute path string
/// or `undefined` for the caller's MODULE_NOT_FOUND path.
#[no_mangle]
pub extern "C" fn js_require_resolve_node_modules(from_dir: f64, specifier: f64) -> f64 {
    let from = value_to_string(from_dir, "from");
    let spec = value_to_string(specifier, "specifier");
    if spec.is_empty() || spec.starts_with('.') {
        return f64::from_bits(TAG_UNDEFINED);
    }
    // Absolute specifier: `require.resolve('<abs>')` returns the resolved FILE
    // (a directory resolves through package.json `main`, then `index.js`) —
    // Next's require-hook re-resolves its alias map values, which are package
    // DIRECTORIES by construction.
    if spec.starts_with('/') {
        let base = std::path::PathBuf::from(&spec);
        let mut candidates: Vec<std::path::PathBuf> = Vec::new();
        if base.is_file() {
            candidates.push(base.clone());
        } else if base.is_dir() {
            if let Ok(manifest) = std::fs::read_to_string(base.join("package.json")) {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&manifest) {
                    if let Some(main) = parsed.get("main").and_then(|m| m.as_str()) {
                        let main_path = base.join(main);
                        candidates.push(main_path.clone());
                        if main_path.extension().is_none() {
                            candidates.push(std::path::PathBuf::from(format!(
                                "{}.js",
                                main_path.to_string_lossy()
                            )));
                        }
                    }
                }
            }
            candidates.push(base.join("index.js"));
        } else {
            candidates.push(std::path::PathBuf::from(format!("{spec}.js")));
            candidates.push(std::path::PathBuf::from(format!("{spec}.json")));
        }
        for cand in candidates {
            if cand.is_file() {
                let text = cand.to_string_lossy();
                let ptr = js_string_from_bytes(text.as_ptr(), text.len() as u32);
                return crate::value::js_nanbox_string(ptr as i64);
            }
        }
        return f64::from_bits(TAG_UNDEFINED);
    }
    let mut dir = std::path::Path::new(&from);
    loop {
        let base = dir.join("node_modules").join(&spec);
        for cand in [
            base.clone(),
            base.with_extension("js"),
            base.with_extension("json"),
            base.join("index.js"),
        ] {
            if cand.is_file() {
                let text = cand.to_string_lossy();
                let ptr = js_string_from_bytes(text.as_ptr(), text.len() as u32);
                return crate::value::js_nanbox_string(ptr as i64);
            }
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => return f64::from_bits(TAG_UNDEFINED),
        }
    }
}

/// Next.js wall 53: runtime `require(absolutePath)` of a `.json` file.
///
/// Emitted only by the CJS wrapper's `require` fallback (cjs_wrap/wrap.rs) for a
/// specifier computed at runtime (e.g. Next.js `require(this.middlewareManifestPath)`)
/// — the statically-resolved relative cases can't cover it. Node's `require`
/// reads + `JSON.parse`s `.json` files; `.json` is pure data so this needs no
/// code evaluation. Reads the file from disk and parses it, throwing
/// `MODULE_NOT_FOUND` (matching Node's require) when the path doesn't exist.
#[no_mangle]
pub extern "C" fn js_require_json_disk(specifier: f64) -> f64 {
    let path = value_to_string(specifier, "id");
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => throw_module_not_found(&path),
    };
    let text_ptr = js_string_from_bytes(content.as_ptr(), content.len() as u32);
    let parsed = unsafe { crate::json::js_json_parse(text_ptr) };
    f64::from_bits(parsed.bits())
}
/// Ambient `require` for compiled external / `compilePackages` modules (Tier 1 of
/// #5389, fixes #5373). These modules carry no CJS ambient `require` binding, so a
/// bare or computed `require(expr)` would otherwise lower to
/// `js_global_get_or_throw_unresolved` and throw `ReferenceError: require is not
/// defined`. This returns the same `createRequire`-backed closure as
/// `js_module_create_require`, but takes no base argument (it is produced where a
/// bare `require` identifier appears, not from an explicit `createRequire(base)`).
/// Builtins resolve by string; unresolved package/file specifiers throw Node's
/// `MODULE_NOT_FOUND` error code.
#[no_mangle]
pub extern "C" fn js_module_ambient_require() -> f64 {
    let base = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("__perry_ambient.cjs")
        .to_string_lossy()
        .into_owned();
    make_require(string_value(&base), undefined())
}

/// Keepalive anchor for the auto-optimize whole-program build (generated-code-only
/// callee; see project_auto_optimize_keepalive_3320).
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_MODULE_AMBIENT_REQUIRE: extern "C" fn() -> f64 = js_module_ambient_require;

/// Synchronous ambient `require(spec)` resolution for the #5389 Tier 2 codegen
/// fallthrough. When a computed `require(expr)` in a compiled external module did
/// not const-fold to a compiled-module target, the dynamic-require dispatch calls
/// this with the runtime specifier value: it resolves exactly like a
/// createRequire-backed `require(spec)` — builtins (`node:os`, …) by string,
/// unknown package/file specifiers throw Node-compatible `MODULE_NOT_FOUND`.
/// Returns the required value directly (no Promise).
#[no_mangle]
pub extern "C" fn js_module_ambient_require_apply(spec: f64) -> f64 {
    require_thunk(std::ptr::null(), spec)
}

/// Keepalive anchor for the auto-optimize whole-program build (generated-code-only
/// callee; see project_auto_optimize_keepalive_3320).
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_MODULE_AMBIENT_REQUIRE_APPLY: extern "C" fn(f64) -> f64 =
    js_module_ambient_require_apply;

/// #6660 (pi wall #8): shared runtime fallback for a dynamic `import(spec)`
/// whose specifier did not match a compiled-module target at the dispatch
/// site. The `import()` analog of `js_module_ambient_require_apply` (#5389
/// Tier 2): builtins (`node:fs/promises`, `os`, …) resolve by string to the
/// same namespace `require(spec)` / `process.getBuiltinModule(spec)` produce,
/// wrapped in a resolved promise; anything else becomes a promise rejected
/// with a descriptive `Error` (`code: 'ERR_MODULE_NOT_FOUND'`, Node's dynamic
/// import failure family) — never a rejection with literal `undefined`, which
/// is what the old codegen fallthrough arms produced and what surfaced as the
/// reasonless `Uncaught (in promise) undefined` one-shot wall.
///
/// `deferred_note` carries the compile-time deferral message for #5230 sites
/// (runtime-computed specifier, non-strict policy) so a genuinely unknown
/// module still reports the site's `file:line`.
fn dynamic_import_fallback_promise(spec: f64, deferred_note: Option<String>) -> f64 {
    // Arm the install-all hooks the way `getBuiltinModule`'s devirt entry does
    // (#6644): the namespace handed back below must dispatch methods even when
    // no static import of the module exists anywhere in the program. Codegen
    // references this symbol only from dynamic-import fallback sites, so
    // programs without them keep per-module stripping.
    crate::object::js_nm_enable_install_all();
    crate::node_submodules::js_node_submod_enable_install_all();
    // `import()` performs ToString on the specifier: a string resolves
    // directly, any other value participates via its string form.
    let jv = JSValue::from_bits(spec.to_bits());
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let spec_str = match unsafe { crate::string::js_string_key_bytes(jv, &mut sso) } {
        Some(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        None => unsafe {
            crate::exception::string_header_to_string(crate::value::js_jsvalue_to_string(spec))
        },
    };
    if let Some(module_name) = supported_require_builtin(&spec_str) {
        let scope = crate::gc::RuntimeHandleScope::new();
        let ns_handle = scope.root_nanbox_f64(require_builtin_value(module_name));
        let promise = crate::promise::js_promise_resolved(ns_handle.get_nanbox_f64());
        return js_nanbox_pointer(promise as i64);
    }
    let message = deferred_note.unwrap_or_else(|| format!("Cannot find module '{spec_str}'"));
    let msg_ptr = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg_ptr, "ERR_MODULE_NOT_FOUND");
    let err = crate::error::js_error_new_with_message(msg_ptr);
    let scope = crate::gc::RuntimeHandleScope::new();
    let err_handle = scope.root_nanbox_f64(js_nanbox_pointer(err as i64));
    let promise = crate::promise::js_promise_rejected(err_handle.get_nanbox_f64());
    js_nanbox_pointer(promise as i64)
}

/// Codegen entry for the unresolved / no-match dynamic-`import()` fallthrough
/// arms (#6660). Returns a NaN-boxed promise; never throws synchronously
/// (`import()` always rejects, per spec).
#[no_mangle]
pub extern "C" fn js_module_dynamic_import_fallback(spec: f64) -> f64 {
    dynamic_import_fallback_promise(spec, None)
}

/// Keepalive anchor (same pattern as the ambient-require anchors above).
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_MODULE_DYNAMIC_IMPORT_FALLBACK: extern "C" fn(f64) -> f64 =
    js_module_dynamic_import_fallback;

/// Codegen entry for #5230 *deferred* dynamic-import sites (runtime-computed
/// specifier under the default non-strict policy). Same builtin-or-reject
/// fallback, but a genuinely unknown module rejects with the compile-time
/// deferral message (which names the site's `file:line`) instead of the
/// generic `Cannot find module` text. `msg` is the NaN-boxed deferral string.
#[no_mangle]
pub extern "C" fn js_module_dynamic_import_deferred(spec: f64, msg: f64) -> f64 {
    let note = {
        let jv = JSValue::from_bits(msg.to_bits());
        let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        unsafe { crate::string::js_string_key_bytes(jv, &mut sso) }
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
    };
    dynamic_import_fallback_promise(spec, note)
}

/// Keepalive anchor (same pattern as the ambient-require anchors above).
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_MODULE_DYNAMIC_IMPORT_DEFERRED: extern "C" fn(f64, f64) -> f64 =
    js_module_dynamic_import_deferred;

/// #6651 family regression guard: createRequire's resolver must never drift
/// from `process.getBuiltinModule`'s again. Today they are the same function;
/// this pins the contract so a future re-split of the implementations still
/// has to keep the module sets identical across both spellings.
#[cfg(test)]
mod builtin_allowlist_parity_tests {
    use super::*;

    #[test]
    fn createrequire_allowlist_matches_get_builtin_module() {
        for &entry in crate::process::MODULE_BUILTIN_MODULES {
            let bare = entry.strip_prefix("node:").unwrap_or(entry);
            let prefixed = format!("node:{bare}");
            for specifier in [bare, prefixed.as_str()] {
                assert_eq!(
                    supported_require_builtin(specifier),
                    crate::process::supported_builtin_module_name(specifier),
                    "{specifier}"
                );
            }
        }
    }
}
