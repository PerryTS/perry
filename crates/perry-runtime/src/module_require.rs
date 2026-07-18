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
        | "test" | "test/reporters" | "timers" | "timers/promises" | "tls" | "tty" | "url"
        | "util" | "util/types" | "vm" | "wasi" | "worker_threads" | "zlib"
        // Implemented native modules that were missing from the createRequire
        // allowlist (they have runtime registry buckets + dispatch, but
        // `require('tls')` etc. via createRequire was rejected as "package/file").
        | "dgram" | "domain" | "inspector" | "inspector/promises" | "repl"
        | "sqlite"
        // #6644: implemented as a node_submodules spec (real pub/sub channel
        // registry in node_submodules/diagnostics.rs) but missing here, so
        // `require('node:diagnostics_channel')` through createRequire (the
        // esbuild banner shim in any ESM bundle of CJS deps — lru-cache's node
        // build in the pi bundle) was rejected as "package/file".
        | "diagnostics_channel" => Some(name),
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
    // #6644: diagnostics_channel lives in the node_submodules registry (not a
    // native-module dispatch bucket); route it there like timers/promises so
    // `require('diagnostics_channel')` / `require('node:diagnostics_channel')`
    // return the real channel/subscribe/tracingChannel exports instead of an
    // empty native-module namespace.
    if module_name == "diagnostics_channel" {
        return unsafe {
            crate::node_submodules::js_node_submodule_namespace(
                b"diagnostics_channel".as_ptr(),
                "diagnostics_channel".len() as u32,
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

/// Next.js wall 54: registry mapping an AOT-compiled CJS module's absolute
/// source path to its evaluated `module.exports`, so a RUNTIME
/// `require(absolutePath.js)` (Next.js / turbopack load page + chunk modules by
/// a path computed at request time, not a static specifier) resolves to the
/// module Perry already compiled instead of throwing `MODULE_NOT_FOUND`. Keyed
/// by canonicalized path so `/a/../b` and symlinks normalize to one entry. Each
/// compiled module self-registers at the end of its CJS wrapper init.
static MODULE_PATH_REGISTRY: std::sync::RwLock<Option<std::collections::HashMap<String, u64>>> =
    std::sync::RwLock::new(None);

/// Next.js wall 54 (part 2): registry mapping an AOT-compiled module's absolute
/// source path to the ADDRESS of its `<prefix>__init` function, so a runtime
/// `require(absolutePath.js)` can LAZILY trigger init of a module that was NOT
/// run at startup (Deferred). The `.next/server/**` page/route/chunk modules are
/// loaded by a path computed at request time; eager-initing them at startup runs
/// React-SSR code before the server is ready (and turbopack chunks must init in
/// the order the page loader's `R.c()` calls demand), so they are Deferred and
/// init on first `require`. Populated once at program start by codegen-emitted
/// `js_register_path_init` calls (no init runs there — only the address is
/// recorded). The init function is idempotent (guarded), self-registers its
/// exports into [`MODULE_PATH_REGISTRY`], and may recursively `require` its own
/// dependencies (chunk loaders) — all safe because each module inits once.
static MODULE_PATH_INIT_REGISTRY: std::sync::RwLock<
    Option<std::collections::HashMap<String, usize>>,
> = std::sync::RwLock::new(None);

fn canonicalize_module_path(path: &str) -> String {
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string())
}

/// Codegen FFI: record that `<prefix>__init` (address `init_addr`) initializes
/// the module whose absolute source path is `path_value`. Emitted once per
/// Deferred `.next/server/**` module at the top of `main`. See
/// [`MODULE_PATH_INIT_REGISTRY`].
/// # Safety
/// `path_ptr`/`path_len` describe a valid UTF-8 byte range (a codegen string
/// constant). `init_addr` is the address of an `extern "C" fn()` module
/// initializer (from `ptrtoint` of the symbol).
#[no_mangle]
pub unsafe extern "C" fn js_register_path_init(path_ptr: *const u8, path_len: i64, init_addr: i64) {
    let slice = std::slice::from_raw_parts(path_ptr, path_len as usize);
    let path = String::from_utf8_lossy(slice).into_owned();
    let key = canonicalize_module_path(&path);
    let mut guard = MODULE_PATH_INIT_REGISTRY.write().unwrap();
    guard
        .get_or_insert_with(std::collections::HashMap::new)
        .insert(key, init_addr as usize);
}

/// Codegen FFI: register an AOT-compiled module's exports under its absolute
/// source path (emitted at the tail of each CJS wrapper). See
/// [`MODULE_PATH_REGISTRY`].
#[no_mangle]
pub extern "C" fn js_register_path_module(path_value: f64, exports: f64) {
    let path = value_to_string(path_value, "path");
    let key = canonicalize_module_path(&path);
    let mut guard = MODULE_PATH_REGISTRY.write().unwrap();
    guard
        .get_or_insert_with(std::collections::HashMap::new)
        .insert(key, exports.to_bits());
}

/// Codegen FFI: resolve a runtime `require(absolutePath.js)` to a registered
/// AOT-compiled module's exports, or `undefined` when no module is registered
/// for that path (caller then falls back to the `.json` disk read / throws
/// `MODULE_NOT_FOUND`). Module exports are always objects, so `undefined`
/// unambiguously signals "miss".
#[no_mangle]
pub extern "C" fn js_require_path_module(path_value: f64) -> f64 {
    let path = value_to_string(path_value, "id");
    let key = canonicalize_module_path(&path);
    // Fast path: the module already ran (eager, or a prior require) and
    // self-registered its exports.
    {
        let guard = MODULE_PATH_REGISTRY.read().unwrap();
        if let Some(map) = guard.as_ref() {
            if let Some(bits) = map.get(&key) {
                return f64::from_bits(*bits);
            }
        }
    }
    // Wall 54 (part 2): no exports yet — if a Deferred module is registered for
    // this path, trigger its init now. The init self-registers its exports (and
    // may recursively require its chunk dependencies). Crucially, all registry
    // locks are RELEASED before calling init, since init re-enters
    // `js_register_path_module` / `js_require_path_module`.
    let init_addr = {
        let guard = MODULE_PATH_INIT_REGISTRY.read().unwrap();
        guard.as_ref().and_then(|m| m.get(&key).copied())
    };
    if let Some(addr) = init_addr {
        // SAFETY: `addr` is the address of a codegen-emitted `extern "C" fn()`
        // module initializer, recorded by `js_register_path_init` from a value
        // produced by `ptrtoint` of the same function symbol. The initializer
        // is idempotent (guarded by `@__perry_init_done_<prefix>`).
        let init_fn: extern "C" fn() = unsafe { std::mem::transmute::<usize, _>(addr) };
        init_fn();
        let guard = MODULE_PATH_REGISTRY.read().unwrap();
        if let Some(map) = guard.as_ref() {
            if let Some(bits) = map.get(&key) {
                return f64::from_bits(*bits);
            }
        }
    }
    // Node directory resolution: `require('<abs dir>')` loads the package's
    // `main` (else `index.js`). Next's require-hook aliases `styled-jsx` to
    // the RESOLVED PACKAGE DIRECTORY, so the eventual require arrives here
    // with a directory path. Map it to the registered file module.
    let dir = std::path::Path::new(&key);
    if dir.is_dir() {
        let mut candidates: Vec<String> = Vec::new();
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
        for cand in candidates {
            let cand_key = canonicalize_module_path(&cand);
            let resolved = {
                let guard = MODULE_PATH_REGISTRY.read().unwrap();
                guard.as_ref().and_then(|m| m.get(&cand_key).copied())
            };
            if let Some(bits) = resolved {
                return f64::from_bits(bits);
            }
            // Deferred module: trigger its init, then re-check.
            let cand_init = {
                let guard = MODULE_PATH_INIT_REGISTRY.read().unwrap();
                guard.as_ref().and_then(|m| m.get(&cand_key).copied())
            };
            if let Some(addr) = cand_init {
                // SAFETY: same contract as the direct-path init above.
                let init_fn: extern "C" fn() = unsafe { std::mem::transmute::<usize, _>(addr) };
                init_fn();
                let guard = MODULE_PATH_REGISTRY.read().unwrap();
                if let Some(bits) = guard.as_ref().and_then(|m| m.get(&cand_key).copied()) {
                    return f64::from_bits(bits);
                }
            }
        }
    }
    undefined()
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

/// Synchronous ambient `require(spec)` resolution for the #5389 Tier 2 codegen
/// fallthrough. When a computed `require(expr)` in a compiled external module did
/// not const-fold to a compiled-module target, the dynamic-require dispatch calls
/// this with the runtime specifier value: it resolves exactly like a
/// createRequire-backed `require(spec)` — builtins (`node:os`, …) by string,
/// unknown package/file specifiers throw the descriptive
/// `ERR_PERRY_UNSUPPORTED_CREATE_REQUIRE`. Returns the required value directly
/// (no Promise).
#[no_mangle]
pub extern "C" fn js_module_ambient_require_apply(spec: f64) -> f64 {
    require_thunk(std::ptr::null(), spec)
}

/// Keepalive anchor for the auto-optimize whole-program build (generated-code-only
/// callee; see project_auto_optimize_keepalive_3320).
#[used]
static KEEP_JS_MODULE_AMBIENT_REQUIRE_APPLY: extern "C" fn(f64) -> f64 =
    js_module_ambient_require_apply;
