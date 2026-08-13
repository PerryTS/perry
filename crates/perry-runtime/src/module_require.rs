//! Minimal `node:module.createRequire` / CommonJS `require` bridge.
//!
//! This intentionally covers Perry's deterministic native-builtin path and the
//! public function shape. Full CommonJS file/package resolution remains in the
//! compiler-side CJS wrapper and future `Module._*` work.

use crate::closure::{js_closure_alloc, js_register_closure_arity, ClosureHeader};
use crate::object::{js_object_alloc, js_object_set_field_by_name};
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

/// State of one AOT-compiled module that can be loaded by runtime path.
///
/// `Initializing` carries the owner thread so a CommonJS cycle on that same
/// thread can observe the wrapper's partial `exports` object. Other callers
/// wait for the owner to publish the final value (or failure), which prevents
/// concurrent first requests from racing the generated module init guard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PathModuleStatus {
    Registered,
    Initializing(std::thread::ThreadId),
    Initialized,
    /// Initializers are not retried. Every later caller receives the exact
    /// same thrown JS value until process teardown.
    Failed(u64),
}

#[derive(Debug)]
struct PathModuleEntry {
    init_addr: Option<usize>,
    /// `Option` is the presence bit: `Some(TAG_UNDEFINED)` is a real,
    /// initialized CommonJS export and must not be confused with a miss.
    exports: Option<u64>,
    status: PathModuleStatus,
}

#[derive(Default)]
struct PathModuleState {
    entries: std::collections::HashMap<String, PathModuleEntry>,
}

/// Provider-visible path-module registry. App-only dylibs call these runtime
/// symbols through their undefined ABI references, so the state lives in the
/// separately loaded runtime provider rather than being duplicated per app.
/// One mutex protects init ownership and export publication atomically; it is
/// always released before generated code runs.
struct PathModuleRegistry {
    state: std::sync::Mutex<PathModuleState>,
    ready: std::sync::Condvar,
}

impl Default for PathModuleRegistry {
    fn default() -> Self {
        Self {
            state: std::sync::Mutex::new(PathModuleState::default()),
            ready: std::sync::Condvar::new(),
        }
    }
}

impl PathModuleRegistry {
    fn lock(&self) -> std::sync::MutexGuard<'_, PathModuleState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Register one canonical path -> initializer mapping. The same mapping is
    /// idempotent. A second address for the same canonical file is rejected so
    /// an alias can never create a second logical module initialization.
    fn register_init(&self, key: String, init_addr: usize) -> bool {
        let mut state = self.lock();
        let entry = state.entries.entry(key).or_insert_with(|| PathModuleEntry {
            init_addr: None,
            exports: None,
            status: PathModuleStatus::Registered,
        });
        if let Some(existing) = entry.init_addr {
            return existing == init_addr;
        }
        entry.init_addr = Some(init_addr);
        true
    }

    /// Publish the initial CommonJS `exports` object before the wrapper body.
    /// Only same-thread recursive loads may observe it; unrelated waiters stay
    /// parked while the status is `Initializing`.
    fn register_partial_exports(&self, key: String, exports: u64) {
        let mut state = self.lock();
        let entry = state.entries.entry(key).or_insert_with(|| PathModuleEntry {
            init_addr: None,
            exports: None,
            status: PathModuleStatus::Registered,
        });
        if matches!(entry.status, PathModuleStatus::Failed(_)) {
            return;
        }
        entry.exports = Some(exports);
        if entry.status == PathModuleStatus::Registered {
            entry.status = PathModuleStatus::Initializing(std::thread::current().id());
        }
    }

    /// Store the wrapper's final `module.exports`. Lazy modules remain owned by
    /// `require_with` until the generated init function returns (namespace
    /// population can still follow the CJS body). Eager modules have no
    /// registered init address, so this call is their completion boundary.
    fn register_final_exports(&self, key: String, exports: u64) {
        let mut state = self.lock();
        let entry = state.entries.entry(key).or_insert_with(|| PathModuleEntry {
            init_addr: None,
            exports: None,
            status: PathModuleStatus::Registered,
        });
        if matches!(entry.status, PathModuleStatus::Failed(_)) {
            return;
        }
        entry.exports = Some(exports);
        if entry.init_addr.is_none() {
            entry.status = PathModuleStatus::Initialized;
            self.ready.notify_all();
        }
    }

    /// Return the value for `key`, initializing it once when necessary.
    ///
    /// The callback is invoked with every registry lock released. Its error is
    /// cached without retry and replayed to all waiters. `Ok(None)` is a miss;
    /// `Ok(Some(TAG_UNDEFINED))` is an initialized module exporting undefined.
    fn require_with(
        &self,
        key: &str,
        initialize: &dyn Fn(usize) -> Result<(), u64>,
    ) -> Result<Option<u64>, u64> {
        let current = std::thread::current().id();
        let init_addr = loop {
            let mut state = self.lock();
            let Some(entry) = state.entries.get_mut(key) else {
                return Ok(None);
            };
            match entry.status {
                PathModuleStatus::Initialized => return Ok(entry.exports),
                PathModuleStatus::Failed(error) => return Err(error),
                PathModuleStatus::Initializing(owner) if owner == current => {
                    // CommonJS cycle: the owner sees its own partial exports.
                    return Ok(entry.exports);
                }
                PathModuleStatus::Initializing(_) => {
                    drop(
                        self.ready
                            .wait(state)
                            .unwrap_or_else(std::sync::PoisonError::into_inner),
                    );
                }
                PathModuleStatus::Registered => {
                    let Some(addr) = entry.init_addr else {
                        return Ok(entry.exports);
                    };
                    entry.status = PathModuleStatus::Initializing(current);
                    break addr;
                }
            }
        };

        // Never hold the registry lock across generated module code. That code
        // self-registers exports and may recursively require another path.
        let outcome = initialize(init_addr);
        let mut state = self.lock();
        let entry = state
            .entries
            .get_mut(key)
            .expect("path initializer entry disappeared while it was running");
        let result = match outcome {
            Ok(()) => {
                entry.status = PathModuleStatus::Initialized;
                Ok(entry.exports)
            }
            Err(error) => {
                entry.exports = None;
                entry.status = PathModuleStatus::Failed(error);
                Err(error)
            }
        };
        self.ready.notify_all();
        result
    }

    fn has_exports(&self, key: &str) -> bool {
        let state = self.lock();
        state.entries.get(key).is_some_and(|entry| {
            entry.exports.is_some() && !matches!(entry.status, PathModuleStatus::Failed(_))
        })
    }

    fn scan_roots(&self, visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
        let mut state = crate::gc::lock_gc_root_registry(&self.state);
        for entry in state.entries.values_mut() {
            if let Some(exports) = entry.exports.as_mut() {
                visitor.visit_nanbox_u64_slot(exports);
            }
            if let PathModuleStatus::Failed(error) = &mut entry.status {
                visitor.visit_nanbox_u64_slot(error);
            }
        }
    }
}

static MODULE_PATH_REGISTRY: std::sync::LazyLock<PathModuleRegistry> =
    std::sync::LazyLock::new(PathModuleRegistry::default);

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
    if !MODULE_PATH_REGISTRY.register_init(key.clone(), init_addr as usize) {
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
    MODULE_PATH_REGISTRY.register_partial_exports(key, exports.to_bits());
}

/// Codegen FFI: register an AOT-compiled module's exports under its absolute
/// source path (emitted at the tail of each CJS wrapper). See
/// [`MODULE_PATH_REGISTRY`].
#[no_mangle]
pub extern "C" fn js_register_path_module(path_value: f64, exports: f64) {
    let path = value_to_string(path_value, "path");
    let key = canonicalize_module_path(&path);
    MODULE_PATH_REGISTRY.register_final_exports(key, exports.to_bits());
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

fn require_path_key(key: &str) -> Result<Option<u64>, u64> {
    MODULE_PATH_REGISTRY.require_with(key, &run_path_initializer)
}

/// Codegen FFI: resolve a runtime `require(absolutePath.js)` to an AOT module.
/// Initialization is once-only and waitable; recursive CommonJS loads receive
/// partial exports, while unrelated waiters receive only the final namespace.
#[no_mangle]
pub extern "C" fn js_require_path_module(path_value: f64) -> f64 {
    let path = value_to_string(path_value, "id");
    let key = canonicalize_module_path(&path);
    match require_path_key(&key) {
        Ok(Some(bits)) => return f64::from_bits(bits),
        Err(error) => crate::exception::js_throw(f64::from_bits(error)),
        Ok(None) => {}
    }
    for candidate in directory_module_candidates(&key) {
        let candidate = canonicalize_module_path(&candidate);
        match require_path_key(&candidate) {
            Ok(Some(bits)) => return f64::from_bits(bits),
            Err(error) => crate::exception::js_throw(f64::from_bits(error)),
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
    let found = MODULE_PATH_REGISTRY.has_exports(&key)
        || directory_module_candidates(&key)
            .into_iter()
            .map(|candidate| canonicalize_module_path(&candidate))
            .any(|candidate| MODULE_PATH_REGISTRY.has_exports(&candidate));
    f64::from_bits(if found { TAG_TRUE } else { TAG_FALSE })
}

/// Keep path-registry exports and cached exception values alive and rewrite
/// them when a copying collection moves their referents.
pub fn scan_module_path_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    MODULE_PATH_REGISTRY.scan_roots(visitor);
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
#[cfg(feature = "keepalive-anchors")]
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

#[cfg(test)]
mod path_module_registry_tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    };

    #[test]
    fn recursive_load_observes_partial_exports_without_reentering_init() {
        let registry = PathModuleRegistry::default();
        assert!(registry.register_init("route.js".into(), 7));
        let calls = AtomicUsize::new(0);

        let result = registry
            .require_with("route.js", &|addr| {
                assert_eq!(addr, 7);
                calls.fetch_add(1, Ordering::Relaxed);
                registry.register_partial_exports("route.js".into(), 0xA1);
                let recursive = registry.require_with("route.js", &|_| {
                    panic!("recursive load must not execute the initializer")
                })?;
                assert_eq!(recursive, Some(0xA1));
                registry.register_final_exports("route.js".into(), 0xA2);
                Ok(())
            })
            .unwrap();

        assert_eq!(result, Some(0xA2));
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn concurrent_first_load_runs_one_initializer_and_publishes_one_value() {
        const THREADS: usize = 20;
        let registry = Arc::new(PathModuleRegistry::default());
        assert!(registry.register_init("chunk.js".into(), 11));
        let starts = Arc::new(Barrier::new(THREADS + 1));
        let init_entered = Arc::new(Barrier::new(2));
        let release_init = Arc::new(Barrier::new(2));
        let calls = Arc::new(AtomicUsize::new(0));

        let mut workers = Vec::new();
        for _ in 0..THREADS {
            let registry = Arc::clone(&registry);
            let starts = Arc::clone(&starts);
            let init_entered = Arc::clone(&init_entered);
            let release_init = Arc::clone(&release_init);
            let calls = Arc::clone(&calls);
            workers.push(std::thread::spawn(move || {
                starts.wait();
                registry.require_with("chunk.js", &|addr| {
                    assert_eq!(addr, 11);
                    calls.fetch_add(1, Ordering::Relaxed);
                    registry.register_partial_exports("chunk.js".into(), 0xB1);
                    init_entered.wait();
                    release_init.wait();
                    registry.register_final_exports("chunk.js".into(), 0xB2);
                    Ok(())
                })
            }));
        }

        starts.wait();
        init_entered.wait();
        release_init.wait();
        for worker in workers {
            assert_eq!(worker.join().unwrap().unwrap(), Some(0xB2));
        }
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn undefined_export_is_present_and_distinct_from_a_miss() {
        let registry = PathModuleRegistry::default();
        assert!(registry.register_init("undefined.js".into(), 13));
        let value = registry
            .require_with("undefined.js", &|_| {
                registry.register_partial_exports("undefined.js".into(), TAG_UNDEFINED);
                registry.register_final_exports("undefined.js".into(), TAG_UNDEFINED);
                Ok(())
            })
            .unwrap();

        assert_eq!(value, Some(TAG_UNDEFINED));
        assert!(registry.has_exports("undefined.js"));
        assert_eq!(
            registry.require_with("missing.js", &|_| unreachable!()),
            Ok(None)
        );
    }

    #[test]
    fn concurrent_initialization_failure_is_shared_and_cached_without_retry() {
        const THREADS: usize = 20;
        let registry = Arc::new(PathModuleRegistry::default());
        assert!(registry.register_init("throws.js".into(), 17));
        let starts = Arc::new(Barrier::new(THREADS + 1));
        let init_entered = Arc::new(Barrier::new(2));
        let release_init = Arc::new(Barrier::new(2));
        let calls = Arc::new(AtomicUsize::new(0));
        let error = 0x7FFD_0000_0000_0042;

        let mut workers = Vec::new();
        for _ in 0..THREADS {
            let registry = Arc::clone(&registry);
            let starts = Arc::clone(&starts);
            let init_entered = Arc::clone(&init_entered);
            let release_init = Arc::clone(&release_init);
            let calls = Arc::clone(&calls);
            workers.push(std::thread::spawn(move || {
                starts.wait();
                registry.require_with("throws.js", &|addr| {
                    assert_eq!(addr, 17);
                    calls.fetch_add(1, Ordering::Relaxed);
                    init_entered.wait();
                    release_init.wait();
                    Err(error)
                })
            }));
        }

        starts.wait();
        init_entered.wait();
        release_init.wait();
        for worker in workers {
            assert_eq!(worker.join().unwrap(), Err(error));
        }
        assert_eq!(
            registry.require_with("throws.js", &|_| {
                panic!("failed path modules use the explicit no-retry policy")
            }),
            Err(error)
        );
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn canonical_alias_cannot_replace_the_first_initializer() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp = std::env::temp_dir().join(format!(
            "perry-path-module-alias-{}-{nonce}",
            std::process::id()
        ));
        let nested = temp.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        let module = temp.join("module.js");
        std::fs::write(&module, "module.exports = 1;").unwrap();
        let direct = canonicalize_module_path(&module.to_string_lossy());
        let aliased =
            canonicalize_module_path(&nested.join("..").join("module.js").to_string_lossy());
        assert_eq!(direct, aliased);

        let registry = PathModuleRegistry::default();
        assert!(registry.register_init(direct.clone(), 19));
        assert!(!registry.register_init(aliased, 23));
        let seen = AtomicUsize::new(0);
        assert_eq!(
            registry.require_with(&direct, &|addr| {
                seen.store(addr, Ordering::Relaxed);
                registry.register_final_exports(direct.clone(), 0xC1);
                Ok(())
            }),
            Ok(Some(0xC1))
        );
        assert_eq!(seen.load(Ordering::Relaxed), 19);
        std::fs::remove_dir_all(temp).unwrap();
    }
}
