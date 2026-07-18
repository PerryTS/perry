//! Native-module namespace machinery: allocator (`js_create_native_module_namespace`),
//! property/method bindings (`js_native_module_property_by_name`,
//! `js_native_module_bind_method`, `js_class_method_bind`), and the
//! per-module constant/sub-namespace tables consumed from
//! `dispatch_native_module_method` and `js_object_get_field_by_name`.
//!
//! Split out of `object/mod.rs` (issue #1103). Pure relocation — no
//! logic changes.

use super::*;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, Ordering};

mod callable_export_check;
mod callable_exports;
mod constants;
mod module_keys;
mod namespace_builders;
mod web_locks;

pub(crate) use callable_export_check::is_native_module_callable_export;
pub(crate) use callable_exports::{
    bound_native_callable_export_value, bound_native_callable_module_and_method,
    bound_native_callable_value_arity, buffer_constructor_value,
    builtin_closure_is_non_constructable, builtin_closure_is_non_constructable_value,
    builtin_closure_length, fs_namespace_descriptor_getter_value,
    fs_namespace_descriptor_setter_value, is_buffer_constructor_value, is_cluster_emitter_method,
    module_cjs_cache_value, module_cjs_extensions_value, module_cjs_global_paths_value,
    module_cjs_path_cache_value, native_string_value, set_bound_native_closure_name,
    set_builtin_closure_length, set_builtin_closure_non_constructable,
    sqlite_session_constructor_value, sqlite_statement_sync_constructor_value,
    timers_promises_parent_namespace, util_debuglog_logger_value,
    util_inspect_default_options_value, zlib_codes_object,
};
pub(crate) use constants::get_native_module_constant;
pub(crate) use module_keys::{native_module_enumerable_keys, native_module_has_enumerable_key};
pub(crate) use namespace_builders::{
    create_cached_sub_namespace, create_fs_constants_object, create_sub_namespace,
    http_global_agent_object, http_methods_array, http_status_codes_object,
    https_global_agent_object, native_namespace_or_create,
};
pub(crate) use web_locks::{worker_threads_locks_value, WebLocksState};

thread_local! {
    pub(crate) static NATIVE_CALLABLE_EXPORTS: RefCell<HashMap<String, u64>> =
        RefCell::new(HashMap::new());
    pub(crate) static NATIVE_MODULE_ACCESSOR_EXPORTS: RefCell<HashMap<String, u64>> =
        RefCell::new(HashMap::new());
    static HANDLE_PROPERTY_BIND_REENTRY: Cell<bool> = const { Cell::new(false) };
    pub(crate) static BUFFER_CONSTRUCTOR_VALUE: Cell<u64> = const { Cell::new(0) };
    pub(crate) static SQLITE_STATEMENT_SYNC_CONSTRUCTOR_VALUE: Cell<u64> = const { Cell::new(0) };
    pub(crate) static SQLITE_SESSION_CONSTRUCTOR_VALUE: Cell<u64> = const { Cell::new(0) };
    pub(crate) static UTIL_INSPECT_DEFAULT_OPTIONS: Cell<u64> = const { Cell::new(0) };
    pub(crate) static UTIL_INSPECT_STYLES: Cell<u64> = const { Cell::new(0) };
    pub(crate) static UTIL_INSPECT_COLORS: Cell<u64> = const { Cell::new(0) };
    pub(crate) static TIMERS_PROMISES_PARENT_NAMESPACE: Cell<u64> = const { Cell::new(0) };
    pub(crate) static ZLIB_CODES_OBJECT: Cell<u64> = const { Cell::new(0) };
    pub(crate) static WORKER_THREADS_LOCKS_VALUE: Cell<u64> = const { Cell::new(0) };
    pub(crate) static WORKER_THREADS_WEB_LOCKS: RefCell<WebLocksState> =
        RefCell::new(WebLocksState::default());
    pub(crate) static MODULE_CJS_CACHE_VALUE: Cell<u64> = const { Cell::new(0) };
    pub(crate) static MODULE_CJS_EXTENSIONS_VALUE: Cell<u64> = const { Cell::new(0) };
    pub(crate) static MODULE_CJS_PATH_CACHE_VALUE: Cell<u64> = const { Cell::new(0) };
    pub(crate) static MODULE_CJS_GLOBAL_PATHS_VALUE: Cell<u64> = const { Cell::new(0) };
    pub(crate) static NATIVE_MODULE_NAMESPACES: RefCell<HashMap<String, u64>> =
        RefCell::new(HashMap::new());
    /// User overrides of native-module namespace properties, keyed
    /// `"{module}\0{prop}"`. CommonJS module exports are MUTABLE in Node —
    /// monkey-patching like Next.js's
    /// `require('node:timers').setImmediate = patched` must store and win
    /// subsequent property reads instead of throwing read-only.
    static NATIVE_NAMESPACE_PROP_OVERRIDES: RefCell<HashMap<String, u64>> =
        RefCell::new(HashMap::new());
}

/// Store a user override for a native-module namespace property
/// (`require('node:timers').setImmediate = fn`). Wins subsequent reads via
/// `vt_get_own_field`.
pub(crate) fn native_namespace_prop_override_store(module: &str, prop: &str, value: f64) {
    NATIVE_NAMESPACE_PROP_OVERRIDES.with(|m| {
        m.borrow_mut()
            .insert(format!("{module}\0{prop}"), value.to_bits());
    });
}

/// Read back a stored native-namespace property override, if any.
pub(crate) fn native_namespace_prop_override_get(module: &str, prop: &str) -> Option<f64> {
    NATIVE_NAMESPACE_PROP_OVERRIDES.with(|m| {
        m.borrow()
            .get(&format!("{module}\0{prop}"))
            .map(|bits| f64::from_bits(*bits))
    })
}

fn bound_native_method_length(name: &str) -> Option<u32> {
    match name {
        "keepSocketAlive" => Some(1),
        "reuseSocket" => Some(2),
        "getName" | "destroy" | "close" => Some(0),
        _ => None,
    }
}

#[no_mangle]
pub extern "C" fn js_vm_create_context(sandbox: f64) -> f64 {
    crate::node_vm::create_context(sandbox)
}

pub fn scan_native_callable_export_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    NATIVE_CALLABLE_EXPORTS.with(|cache| {
        let mut cache = cache.borrow_mut();
        for value_bits in cache.values_mut() {
            visitor.visit_nanbox_u64_slot(value_bits);
        }
    });
    NATIVE_NAMESPACE_PROP_OVERRIDES.with(|cache| {
        let mut cache = cache.borrow_mut();
        for value_bits in cache.values_mut() {
            visitor.visit_nanbox_u64_slot(value_bits);
        }
    });
    NATIVE_MODULE_ACCESSOR_EXPORTS.with(|cache| {
        let mut cache = cache.borrow_mut();
        for value_bits in cache.values_mut() {
            visitor.visit_nanbox_u64_slot(value_bits);
        }
    });
    BUFFER_CONSTRUCTOR_VALUE.with(|slot| {
        let mut value_bits = slot.get();
        if value_bits != 0 {
            visitor.visit_nanbox_u64_slot(&mut value_bits);
            slot.set(value_bits);
        }
    });
    SQLITE_STATEMENT_SYNC_CONSTRUCTOR_VALUE.with(|slot| {
        let mut value_bits = slot.get();
        if value_bits != 0 {
            visitor.visit_nanbox_u64_slot(&mut value_bits);
            slot.set(value_bits);
        }
    });
    SQLITE_SESSION_CONSTRUCTOR_VALUE.with(|slot| {
        let mut value_bits = slot.get();
        if value_bits != 0 {
            visitor.visit_nanbox_u64_slot(&mut value_bits);
            slot.set(value_bits);
        }
    });
    UTIL_INSPECT_DEFAULT_OPTIONS.with(|slot| {
        let mut value_bits = slot.get();
        if value_bits != 0 {
            visitor.visit_nanbox_u64_slot(&mut value_bits);
            slot.set(value_bits);
        }
    });
    UTIL_INSPECT_STYLES.with(|slot| {
        let mut value_bits = slot.get();
        if value_bits != 0 {
            visitor.visit_nanbox_u64_slot(&mut value_bits);
            slot.set(value_bits);
        }
    });
    UTIL_INSPECT_COLORS.with(|slot| {
        let mut value_bits = slot.get();
        if value_bits != 0 {
            visitor.visit_nanbox_u64_slot(&mut value_bits);
            slot.set(value_bits);
        }
    });
    TIMERS_PROMISES_PARENT_NAMESPACE.with(|slot| {
        let mut value_bits = slot.get();
        if value_bits != 0 {
            visitor.visit_nanbox_u64_slot(&mut value_bits);
            slot.set(value_bits);
        }
    });
    ZLIB_CODES_OBJECT.with(|slot| {
        let mut value_bits = slot.get();
        if value_bits != 0 {
            visitor.visit_nanbox_u64_slot(&mut value_bits);
            slot.set(value_bits);
        }
    });
    WORKER_THREADS_LOCKS_VALUE.with(|slot| {
        let mut value_bits = slot.get();
        if value_bits != 0 {
            visitor.visit_nanbox_u64_slot(&mut value_bits);
            slot.set(value_bits);
        }
    });
    MODULE_CJS_CACHE_VALUE.with(|slot| {
        let mut value_bits = slot.get();
        if value_bits != 0 {
            visitor.visit_nanbox_u64_slot(&mut value_bits);
            slot.set(value_bits);
        }
    });
    MODULE_CJS_EXTENSIONS_VALUE.with(|slot| {
        let mut value_bits = slot.get();
        if value_bits != 0 {
            visitor.visit_nanbox_u64_slot(&mut value_bits);
            slot.set(value_bits);
        }
    });
    MODULE_CJS_PATH_CACHE_VALUE.with(|slot| {
        let mut value_bits = slot.get();
        if value_bits != 0 {
            visitor.visit_nanbox_u64_slot(&mut value_bits);
            slot.set(value_bits);
        }
    });
    MODULE_CJS_GLOBAL_PATHS_VALUE.with(|slot| {
        let mut value_bits = slot.get();
        if value_bits != 0 {
            visitor.visit_nanbox_u64_slot(&mut value_bits);
            slot.set(value_bits);
        }
    });
    WORKER_THREADS_WEB_LOCKS.with(|state| {
        let mut state = state.borrow_mut();
        for held in &mut state.held {
            visitor.visit_raw_mut_ptr_slot(&mut held.source_promise);
            visitor.visit_raw_mut_ptr_slot(&mut held.output_promise);
        }
        for pending in &mut state.pending {
            visitor.visit_nanbox_u64_slot(&mut pending.callback_bits);
            visitor.visit_raw_mut_ptr_slot(&mut pending.output_promise);
        }
    });
    NATIVE_MODULE_NAMESPACES.with(|cache| {
        let mut cache = cache.borrow_mut();
        for value_bits in cache.values_mut() {
            visitor.visit_nanbox_u64_slot(value_bits);
        }
    });
    // #6468: only present when the program imports `node:http2`; when the gate
    // is off the `sensitiveHeaders` symbol slot doesn't exist, so there's no
    // root to scan.
    #[cfg(feature = "mod-http2-constants")]
    crate::node_http2_constants::scan_roots_mut(visitor);
    scan_stream_event_emitter_prototype_roots_mut(visitor);
}

/// Special class ID for native module namespace objects
/// This is used to identify objects that represent native module namespaces
pub const NATIVE_MODULE_CLASS_ID: u32 = 0xFFFFFFFE;
pub(crate) const WORKER_THREADS_LOCK_MANAGER_CLASS_ID: u32 = 0xFFFF_00B1;
pub(crate) const WORKER_THREADS_LOCK_CLASS_ID: u32 = 0xFFFF_00B2;

static BUFFER_POOL_SIZE_BITS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(8192f64.to_bits());

type WorkerThreadsValueGetter = extern "C" fn() -> f64;

pub(crate) static WORKER_THREADS_WORKER_DATA_GETTER: AtomicPtr<()> = AtomicPtr::new(null_mut());
pub(crate) static WORKER_THREADS_IS_MAIN_THREAD_GETTER: AtomicPtr<()> = AtomicPtr::new(null_mut());
pub(crate) static WORKER_THREADS_PARENT_PORT_GETTER: AtomicPtr<()> = AtomicPtr::new(null_mut());
pub(crate) static WORKER_THREADS_THREAD_NAME_GETTER: AtomicPtr<()> = AtomicPtr::new(null_mut());
pub(crate) static WORKER_THREADS_RESOURCE_LIMITS_GETTER: AtomicPtr<()> = AtomicPtr::new(null_mut());

#[no_mangle]
pub extern "C" fn js_register_worker_threads_namespace_getters(
    worker_data: WorkerThreadsValueGetter,
    is_main_thread: WorkerThreadsValueGetter,
    parent_port: WorkerThreadsValueGetter,
    thread_name: WorkerThreadsValueGetter,
    resource_limits: WorkerThreadsValueGetter,
) {
    WORKER_THREADS_WORKER_DATA_GETTER.store(worker_data as *mut (), Ordering::Release);
    WORKER_THREADS_IS_MAIN_THREAD_GETTER.store(is_main_thread as *mut (), Ordering::Release);
    WORKER_THREADS_PARENT_PORT_GETTER.store(parent_port as *mut (), Ordering::Release);
    WORKER_THREADS_THREAD_NAME_GETTER.store(thread_name as *mut (), Ordering::Release);
    WORKER_THREADS_RESOURCE_LIMITS_GETTER.store(resource_limits as *mut (), Ordering::Release);
}

pub(crate) fn call_worker_threads_getter(
    slot: &AtomicPtr<()>,
    fallback: impl FnOnce() -> f64,
) -> f64 {
    let ptr = slot.load(Ordering::Acquire);
    if ptr.is_null() {
        return fallback();
    }
    let getter: WorkerThreadsValueGetter = unsafe { std::mem::transmute(ptr) };
    getter()
}

pub(crate) fn buffer_pool_size() -> f64 {
    f64::from_bits(BUFFER_POOL_SIZE_BITS.load(std::sync::atomic::Ordering::Relaxed))
}

pub(crate) fn set_buffer_pool_size(value: f64) {
    BUFFER_POOL_SIZE_BITS.store(value.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// Linker-strippability vtable for every native-module behavior reachable
/// from the always-linked generic object paths (method dispatch, own-field
/// reads, Object.keys, has/in checks). All of these bottom out in large
/// static (module, method) tables that reference every module's runtime
/// implementation; a direct call from a generic path pins all of it in
/// every binary, `-dead_strip` notwithstanding. Namespace-class objects
/// (NATIVE_MODULE_CLASS_ID) are only created by
/// `js_create_native_module_namespace` and a handful of in-crate
/// allocators (node_v8 serializer, perf_hooks observer), all of which
/// install this vtable first — so a program that never creates one lets
/// the linker drop the tables wholesale. Relaxed ordering is sufficient:
/// the store happens-before any namespace object can reach a call site on
/// the creating thread, and cross-thread publication of the object
/// pointer itself already synchronizes.
pub(crate) struct NativeModuleVtable {
    pub dispatch: unsafe fn(*const ObjectHeader, &str, *const f64, usize) -> f64,
    pub get_own_field:
        unsafe fn(*const ObjectHeader, *const crate::StringHeader) -> Option<JSValue>,
    pub own_keys_array: unsafe fn(*const ObjectHeader) -> Option<*mut crate::array::ArrayHeader>,
    pub has_enumerable_key: fn(&str, &str) -> bool,
}

static NATIVE_MODULE_VTABLE_IMPL: NativeModuleVtable = NativeModuleVtable {
    dispatch: dispatch_native_module_method,
    get_own_field: vt_get_own_field,
    own_keys_array: vt_own_keys_array,
    has_enumerable_key: native_module_has_enumerable_key,
};

static NATIVE_MODULE_VTABLE_PTR: AtomicPtr<NativeModuleVtable> =
    AtomicPtr::new(std::ptr::null_mut());

/// Make the native-module vtable reachable. Must be called by every code
/// path that creates a NATIVE_MODULE_CLASS_ID object — this is the only
/// static reference to the dispatch/table machinery in the crate.
pub(crate) fn install_native_module_vtable() {
    NATIVE_MODULE_VTABLE_PTR.store(
        &NATIVE_MODULE_VTABLE_IMPL as *const NativeModuleVtable as *mut NativeModuleVtable,
        Ordering::Relaxed,
    );
}

/// `None` until the first namespace object exists; generic paths treat
/// that as "no native module can be involved" and fall through to their
/// default behavior.
#[inline]
pub(crate) fn native_module_vtable() -> Option<&'static NativeModuleVtable> {
    let p = NATIVE_MODULE_VTABLE_PTR.load(Ordering::Relaxed);
    if p.is_null() {
        None
    } else {
        Some(unsafe { &*(p as *const NativeModuleVtable) })
    }
}

/// Route a NATIVE_MODULE_CLASS_ID method call through the vtable. A null
/// vtable means no namespace object was ever created, so no such object
/// can exist to dispatch on — unreachable in practice.
#[inline]
pub(crate) unsafe fn call_native_module_dispatch_hook(
    obj: *const ObjectHeader,
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    match native_module_vtable() {
        Some(vt) => (vt.dispatch)(obj, method_name, args_ptr, args_len),
        None => {
            debug_assert!(
                false,
                "native-module method call before any namespace was created"
            );
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
    }
}

/// Create a native module namespace object/// Create a native module namespace object
/// This is used for `import * as X from 'module'` patterns
/// The returned object identifies itself as an object (typeof returns "object")
/// and stores the module name for debugging purposes
///
/// module_name_ptr: pointer to the module name string bytes
/// module_name_len: length of the module name
/// Returns the object as a NaN-boxed f64
#[no_mangle]
pub extern "C" fn js_create_native_module_namespace(
    module_name_ptr: *const u8,
    module_name_len: usize,
) -> f64 {
    // Install the vtable the moment the first namespace exists — the only
    // static reference to the dispatch/table machinery in the crate.
    install_native_module_vtable();
    let module_name = unsafe {
        std::str::from_utf8(std::slice::from_raw_parts(module_name_ptr, module_name_len))
            .unwrap_or("")
    };
    let module_name = normalize_native_module_alias(module_name);
    if should_cache_native_module_namespace(module_name) {
        if let Some(bits) =
            NATIVE_MODULE_NAMESPACES.with(|cache| cache.borrow().get(module_name).copied())
        {
            return f64::from_bits(bits);
        }
    }

    // Create an object with one field to store the module name
    let obj = js_object_alloc(NATIVE_MODULE_CLASS_ID, 1);

    // Create a string from the module name
    let module_name_header =
        crate::string::js_string_from_bytes(module_name.as_ptr(), module_name.len() as u32);

    // Store the module name in the first field
    js_object_set_field(obj, 0, JSValue::string_ptr(module_name_header));

    // Create a keys array with one key: "__module__"
    let keys_array = crate::array::js_array_alloc(1);
    let key_bytes = b"__module__";
    let key_str = crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
    crate::array::js_array_push(keys_array, JSValue::string_ptr(key_str));
    js_object_set_keys(obj, keys_array);

    // Return as NaN-boxed pointer
    let value = crate::value::js_nanbox_pointer(obj as i64);
    if should_cache_native_module_namespace(module_name) {
        NATIVE_MODULE_NAMESPACES.with(|cache| {
            cache
                .borrow_mut()
                .insert(module_name.to_string(), value.to_bits());
        });
    }
    value
}

pub(crate) fn normalize_native_module_alias(module_name: &str) -> &str {
    let module_name = module_name.strip_prefix("node:").unwrap_or(module_name);
    match module_name {
        "sys" => {
            crate::node_submodules::emit_sys_deprecation_warning_once();
            "util"
        }
        "path/posix" => "path.posix",
        "path/win32" => "path.win32",
        // #6563: `@lydell/node-pty` is an API-identical fork of node-pty
        // (opencode's import); both names resolve to the one runtime pty.
        "@lydell/node-pty" => "node-pty",
        _ => module_name,
    }
}

pub(crate) fn webcrypto_namespace() -> f64 {
    js_create_native_module_namespace(b"crypto.webcrypto".as_ptr(), "crypto.webcrypto".len())
}

pub(crate) fn install_global_webcrypto(singleton: *mut ObjectHeader) {
    let key = crate::string::js_string_from_bytes(b"crypto".as_ptr(), "crypto".len() as u32);
    js_object_set_field_by_name(singleton, key, webcrypto_namespace());
}

pub(crate) fn install_webcrypto_constructor_proto(proto_obj: *mut ObjectHeader, ctor_value: f64) {
    let constructor = "constructor";
    let key = crate::string::js_string_from_bytes(constructor.as_ptr(), constructor.len() as u32);
    js_object_set_field_by_name(proto_obj, key, ctor_value);
    super::set_builtin_property_attrs(
        proto_obj as usize,
        constructor.to_string(),
        super::PropertyAttrs::new(true, false, true),
    );
}

pub(crate) fn subtle_crypto_namespace() -> f64 {
    js_create_native_module_namespace(b"crypto.subtle".as_ptr(), "crypto.subtle".len())
}

pub(crate) fn cjs_default_base_module(module_name: &str) -> Option<&'static str> {
    match module_name {
        "async_hooks.default" => Some("async_hooks"),
        "child_process.default" => Some("child_process"),
        "cluster.default" => Some("cluster"),
        "constants.default" => Some("constants"),
        "dns.default" => Some("dns"),
        "dns/promises.default" => Some("dns/promises"),
        "inspector.default" => Some("inspector"),
        "inspector/promises.default" => Some("inspector/promises"),
        "module.default" => Some("module"),
        "node-pty.default" => Some("node-pty"),
        "os.default" => Some("os"),
        "path.default" => Some("path"),
        "path.posix.default" => Some("path.posix"),
        "path.win32.default" => Some("path.win32"),
        "process.default" => Some("process"),
        "punycode.default" => Some("punycode"),
        "querystring.default" => Some("querystring"),
        "repl.default" => Some("repl"),
        "sea.default" => Some("sea"),
        "url.default" => Some("url"),
        "util.default" => Some("util"),
        _ => None,
    }
}

fn cjs_default_namespace_name(module_name: &str) -> Option<&'static str> {
    match module_name {
        "async_hooks" => Some("async_hooks.default"),
        "child_process" => Some("child_process.default"),
        "cluster" => Some("cluster.default"),
        "constants" => Some("constants.default"),
        "dns" => Some("dns.default"),
        "dns/promises" => Some("dns/promises.default"),
        "inspector" => Some("inspector.default"),
        "inspector/promises" => Some("inspector/promises.default"),
        "module" => Some("module.default"),
        "node-pty" => Some("node-pty.default"),
        "os" => Some("os.default"),
        "path" => Some("path.default"),
        "path.posix" => Some("path.posix.default"),
        "path.win32" => Some("path.win32.default"),
        "process" => Some("process.default"),
        "punycode" => Some("punycode.default"),
        "querystring" => Some("querystring.default"),
        "repl" => Some("repl.default"),
        "sea" => Some("sea.default"),
        "url" => Some("url.default"),
        "util" => Some("util.default"),
        _ => None,
    }
}

fn create_cjs_default_namespace(module_name: &str) -> Option<f64> {
    let name = cjs_default_namespace_name(module_name)?;
    Some(js_create_native_module_namespace(name.as_ptr(), name.len()))
}

pub(crate) fn cjs_default_export_value(module_name: &str) -> Option<f64> {
    match module_name {
        "events" => Some(bound_native_callable_export_value("events", "EventEmitter")),
        // #3687: `node:cluster` default import is a distinct EventEmitter-shaped
        // `cluster.default` namespace (its `on`/`emit`/… reads diverge from the
        // bare `import * as` namespace).
        "cluster" => create_cjs_default_namespace("cluster"),
        // #3693: `node:dgram` default === the module namespace (CJS
        // `module.exports`); a cached singleton makes `dgram === ns.default`.
        "dgram" => Some(js_create_native_module_namespace(
            b"dgram".as_ptr(),
            "dgram".len(),
        )),
        "module" => Some(bound_native_callable_export_value("module", "Module")),
        "process" => Some(js_create_native_module_namespace(
            b"process".as_ptr(),
            "process".len(),
        )),
        "module" => Some(bound_native_callable_export_value("module", "Module")),
        "async_hooks" | "child_process" | "constants" | "dns" | "dns/promises" | "node-pty"
        | "os" | "path" | "path.posix" | "path.win32" | "punycode" | "querystring" | "repl"
        | "sea" | "url" | "util" | "inspector" | "inspector/promises" => {
            create_cjs_default_namespace(module_name)
        }
        _ => None,
    }
}

pub(crate) fn native_module_get_builtin_module_value(module_name: &str) -> f64 {
    // Devirt: this is the runtime-dynamic builtin resolver (`require(spec)`,
    // `process.getBuiltinModule(spec)`) — `module_name` is only known at runtime,
    // so codegen could not emit the per-module dispatch install. Run the
    // install-all hook so a dynamically-resolved namespace can dispatch methods.
    // The hook is an INDIRECT pointer (null unless codegen emitted
    // `js_nm_enable_install_all()` because the program actually uses dynamic
    // require/getBuiltinModule) — so this resolver, which is linked into every
    // program via the always-present `process.getBuiltinModule` method table,
    // does NOT statically reference `js_nm_install_all` and therefore does not
    // pin every bucket. Static imports keep their precise per-module installs.
    super::native_module_registry::nm_run_install_all_hook();
    cjs_default_export_value(module_name).unwrap_or_else(|| {
        js_create_native_module_namespace(module_name.as_ptr(), module_name.len())
    })
}

pub(crate) fn canonical_native_callable_property<'a>(
    module_name: &str,
    property_name: &'a str,
) -> &'a str {
    match (module_name, property_name) {
        ("fs", "FileReadStream") => "ReadStream",
        ("fs", "FileWriteStream") => "WriteStream",
        ("path" | "path.posix" | "path.win32", "_makeLong") => "toNamespacedPath",
        ("querystring", "decode") => "parse",
        ("querystring", "encode") => "stringify",
        _ => property_name,
    }
}

pub(crate) fn assert_instance_base_module(module_name: &str) -> Option<&'static str> {
    match module_name {
        "assert.instance" | "assert.instance.skip" => Some("assert"),
        "assert/strict.instance" | "assert/strict.instance.skip" => Some("assert/strict"),
        _ => None,
    }
}

fn should_cache_native_module_namespace(module_name: &str) -> bool {
    matches!(
        module_name,
        "assert/strict"
            | "async_hooks"
            | "async_hooks.default"
            | "constants"
            | "constants.default"
            // #5263: cache the top-level namespace objects whose dynamic
            // member access is now allowed by default. A stable (cached)
            // namespace object means a user-set symbol property
            // (`fs[Symbol.for('graceful-fs.queue')] = queue`, keyed by object
            // pointer in `SYMBOL_PROPERTIES`) round-trips on reads — otherwise
            // each `NativeModuleRef` mints a fresh object and the write is lost.
            // String-keyed writes already persist via the module-keyed
            // `NATIVE_NAMESPACE_PROP_OVERRIDES` side-table. These are pure
            // tag+name holders (all real dispatch keys off the module name, not
            // object state), so caching only affects object identity.
            | "fs"
            | "dns.default"
            | "dns/promises.default"
            | "child_process.default"
            | "cluster"
            | "cluster.default"
            | "dgram"
            | "events"
            | "fs.constants"
            | "inspector"
            | "inspector.default"
            | "inspector.Network"
            | "inspector/promises"
            | "inspector/promises.default"
            | "module"
            | "node-pty"
            | "node-pty.default"
            | "os"
            | "os.default"
            | "path"
            | "path.default"
            | "path.posix.default"
            | "path.win32.default"
            | "punycode"
            | "punycode.default"
            | "punycode.ucs2"
            | "querystring"
            | "querystring.default"
            | "repl"
            | "repl.default"
            | "sea"
            | "sea.default"
            | "process"
            | "process.namespace"
            | "process.default"
            | "url"
            | "url.default"
            | "util"
            | "util.default"
            | "util.types"
            | "path.posix"
            | "path.win32"
            | "readline/promises"
            | "timers/promises"
            | "vm"
            | "vm.constants"
            | "crypto.webcrypto"
            | "crypto.subtle"
    )
}

/// #1479: read the module-name string stored in field 0 of a
/// native-module-namespace ObjectHeader. Returns `None` if the field
/// is missing, not a string, or the bytes aren't valid UTF-8. Caller
/// must have confirmed `class_id == NATIVE_MODULE_CLASS_ID` already.
///
/// # Safety
/// `obj_ptr` must point to a live `ObjectHeader` with
/// `class_id == NATIVE_MODULE_CLASS_ID` (i.e. one produced by
/// [`js_create_native_module_namespace`]).
pub(crate) unsafe fn read_native_module_name(
    obj_ptr: *const crate::object::ObjectHeader,
) -> Option<String> {
    let field = crate::object::js_object_get_field(obj_ptr, 0);
    // #1781: SSO-aware — a native-module name of ≤ 5 bytes (e.g. `"fs"`,
    // `"os"`, `"tty"`, `"net"`, `"path"`) is stored as a SHORT_STRING_TAG
    // value. Pre-fix `is_string()` (STRING_TAG-only) returned None and
    // the auto-optimize sweep couldn't determine the requested module.
    let mut sso_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let bytes = crate::string::js_string_key_bytes(field, &mut sso_buf)?;
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

/// Issue #649: codegen entry for `PropertyGet { NativeModuleRef(name),
/// property }`. `NativeModuleRef` lowers to a literal `0.0` at the codegen
/// level, so the generic PropertyGet path can't find the namespace
/// object. This helper short-circuits to the constants dispatcher; for
/// the chained case (`fs.constants.F_OK`) the inner call returns a
/// sub-namespace ObjectHeader and the outer PropertyGet goes through
/// `js_object_get_field_by_name`'s NATIVE_MODULE_CLASS_ID arm.
#[no_mangle]
pub unsafe extern "C" fn js_native_module_property_by_name(
    module_name_ptr: *const u8,
    module_name_len: usize,
    property_name_ptr: *const u8,
    property_name_len: usize,
) -> f64 {
    // Codegen NativeModuleRef fast path — can mint native-module-backed
    // values without a namespace object; the vtable must be live for the
    // generic paths that later touch them.
    install_native_module_vtable();
    let module_name =
        std::str::from_utf8(std::slice::from_raw_parts(module_name_ptr, module_name_len))
            .unwrap_or("");
    let module_name = normalize_native_module_alias(module_name);
    let property_name = std::str::from_utf8(std::slice::from_raw_parts(
        property_name_ptr,
        property_name_len,
    ))
    .unwrap_or("");
    // #5263 / monkey-patch parity: a user-stored override of a namespace
    // property (`fs[k] = v`, `require('node:timers').setImmediate = fn`) wins
    // all built-in resolution below — CJS exports are mutable in Node, and
    // dynamic stdlib member writes are allowed by default. This mirrors
    // `vt_get_own_field`, which the generic object-by-name read path uses; the
    // codegen `NativeModuleRef` fast-path landed here without consulting the
    // side-table, so writes via `PutValueSet` didn't round-trip on reads.
    if let Some(value) = native_namespace_prop_override_get(module_name, property_name) {
        return value;
    }
    if module_name == "process.namespace" && property_name == "default" {
        return cjs_default_export_value("process")
            .unwrap_or_else(|| js_create_native_module_namespace(b"process".as_ptr(), 7));
    }
    let module_name = if module_name == "process.namespace" {
        "process"
    } else {
        module_name
    };
    if matches!(module_name, "process" | "process.default") {
        if let Some(value) = crate::process::process_ipc_property(property_name) {
            return value;
        }
    }
    // node:perf_hooks — `performance` and `constants` are object-valued
    // exports. Resolve them to a `perf_hooks`-tagged namespace object so
    // `typeof performance === "object"`, `performance.timeOrigin` (a
    // constant), `performance.now` (a callable export), and
    // `constants.NODE_PERFORMANCE_GC_*` (constants) all dispatch coherently.
    if module_name == "perf_hooks" && property_name == "performance" {
        // Singleton so `require("perf_hooks").performance` and the global
        // `performance` are the same object (Node identity guarantee, #1327).
        return crate::perf_hooks::performance_namespace();
    }
    if module_name == "perf_hooks" && property_name == "constants" {
        return js_create_native_module_namespace(module_name.as_ptr(), module_name.len());
    }
    // #1533: node:stream exposes a `promises` namespace (`await pipeline(...)`
    // / `finished(...)`). Resolve `stream.promises` to a `stream/promises`-
    // tagged namespace object so `typeof stream.promises === "object"` and
    // `stream.promises.pipeline` / `.finished` read as callable exports
    // (same dispatch the `import ... from "node:stream/promises"` form uses).
    if module_name == "stream" && property_name == "promises" {
        let submodule = "stream/promises";
        return js_create_native_module_namespace(submodule.as_ptr(), submodule.len());
    }
    // #2133: same shape for `node:fs.promises`. Route to the populated
    // `fs_promises` singleton so destructured exports + FileHandle methods
    // dispatch correctly.
    if module_name == "fs" && property_name == "promises" {
        return unsafe {
            crate::node_submodules::js_node_submodule_namespace(
                b"fs_promises".as_ptr(),
                "fs_promises".len() as u32,
            )
        };
    }
    if module_name == "dns" && property_name == "promises" {
        crate::dns::dns_promises_init_servers_from_callback_if_unset();
        return cjs_default_export_value("dns/promises").unwrap_or_else(|| {
            let submodule = "dns/promises";
            js_create_native_module_namespace(submodule.as_ptr(), submodule.len())
        });
    }

    // #5731 — `perry.isStandaloneExecutable` value export (always `true` at
    // runtime). `embeddedFiles` / `readEmbedded` are callable exports dispatched
    // via the native call table, not value reads.
    if module_name == "perry" && property_name == "isStandaloneExecutable" {
        return crate::embedded::is_standalone_executable_value();
    }

    if module_name == "util" && property_name == "debug" {
        return bound_native_callable_export_value("util", "debuglog");
    }
    if module_name == "url" && property_name == "URL" {
        return js_get_global_this_builtin_value(b"URL".as_ptr(), "URL".len());
    }
    if module_name == "url" && property_name == "URLSearchParams" {
        return js_get_global_this_builtin_value(
            b"URLSearchParams".as_ptr(),
            "URLSearchParams".len(),
        );
    }
    if module_name == "url" && property_name == "URLPattern" {
        return js_get_global_this_builtin_value(b"URLPattern".as_ptr(), "URLPattern".len());
    }
    // #6560 — Bun globals shim pack: `Bun.stdin` / `Bun.stdout` / `Bun.stderr`
    // are object-valued reads (BunFile-like handles built by `bun_compat`).
    if module_name == "bun" {
        match property_name {
            "stdin" => return crate::bun_compat::js_bun_stdin(),
            "stdout" => return crate::bun_compat::js_bun_stdout(),
            "stderr" => return crate::bun_compat::js_bun_stderr(),
            _ => {}
        }
    }
    if module_name == "crypto.webcrypto" {
        if let Some(value) = super::global_this::webcrypto_method_value(property_name) {
            return value;
        }
    }
    if module_name == "crypto.subtle" {
        if let Some(value) = super::global_this::subtle_crypto_method_value(property_name) {
            return value;
        }
    }

    // #3687: `node:cluster` is a singleton EventEmitter. Its EventEmitter
    // method surface is exposed ONLY on the default import (the distinct
    // `cluster.default` namespace) — `import * as cluster` reads these as
    // `undefined` (they live on EventEmitter.prototype, not as named exports).
    // Resolve them to bound methods here, before the generic
    // `get_native_module_constant` path (where `cluster_property` would return
    // `undefined` for `on`/`addListener`).
    if module_name == "cluster.default" && is_cluster_emitter_method(property_name) {
        return bound_native_callable_export_value("cluster.default", property_name);
    }

    if let Some(val) = get_native_module_constant(module_name, property_name, 0.0) {
        return val;
    }
    // For native modules whose surface includes known callable methods or
    // class exports, return a bound-method closure so `typeof` and property
    // capture (`const f = tty.isatty`) match Node's "function" shape. The
    // closure routes back through js_native_call_method when invoked. Kept
    // narrow to specific (module, property) pairs so a typo'd access still
    // returns undefined.
    if is_native_module_callable_export(module_name, property_name) {
        return bound_native_callable_export_value(module_name, property_name);
    }
    // Try V8 JS runtime fallback for unknown properties (e.g., ethers.Contract)
    let js_val = crate::value::native_module_try_js_property(module_name, property_name);
    if js_val.to_bits() != crate::value::TAG_UNDEFINED {
        return js_val;
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Access a property on a native module namespace object.
/// For method references (e.g., `fs.existsSync`), creates a bound method closure.
/// For constant properties (e.g., `path.sep`, `fs.constants`), returns the value directly.
#[no_mangle]
pub extern "C" fn js_native_module_bind_method(
    _namespace_obj: f64,
    property_name_ptr: *const u8,
    property_name_len: usize,
) -> f64 {
    let property_name = unsafe {
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(
            property_name_ptr,
            property_name_len,
        ))
    };

    // Extract module name from the namespace object's first field
    let module_name = unsafe { get_module_name_from_namespace(_namespace_obj) };

    if module_name == "crypto.webcrypto" {
        if let Some(value) = super::global_this::webcrypto_method_value(property_name) {
            return value;
        }
    }
    if module_name == "crypto.subtle" {
        if let Some(value) = super::global_this::subtle_crypto_method_value(property_name) {
            return value;
        }
    }

    // Check for known constant properties first
    if let Some(val) =
        unsafe { get_native_module_constant(module_name, property_name, _namespace_obj) }
    {
        return val;
    }

    // Not a constant. Only synthesize callables for
    // exports that are actually callable on this platform; otherwise namespace
    // reads such as Linux `fs.lchmodSync` must stay `undefined`.
    if is_native_module_callable_export(module_name, property_name) {
        return bound_native_callable_export_value(module_name, property_name);
    }

    // Try V8 JS runtime fallback for unknown properties (e.g., ethers.Contract)
    let js_val = crate::value::native_module_try_js_property(module_name, property_name);
    if js_val.to_bits() != crate::value::TAG_UNDEFINED {
        return js_val;
    }

    // Not a constant or JS-backed property. Only synthesize callables for
    // exports that are actually callable on this platform; otherwise namespace
    // reads such as Linux `fs.lchmodSync` must stay `undefined`.
    if !is_native_module_callable_export(module_name, property_name) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }

    bound_native_callable_export_value(module_name, property_name)
}

/// Build a "bound method" closure for `obj.method` PropertyGet on a known class
/// instance. The captures (instance, method_name_ptr, method_name_len) drive
/// `dispatch_bound_method` (closure.rs), which calls `js_native_call_method`
/// — that resolves the method through `CLASS_VTABLE_REGISTRY` for any class
/// registered by `js_register_class_method` at module init.
///
/// Issue #446: previously a class method reference (`let f = obj.method`,
/// `typeof obj.method`, `arr.map(obj.method)`) silently lowered to the
/// generic property-bag lookup, which doesn't store prototype methods —
/// every such read returned `undefined`, so `typeof obj.method === "undefined"`
/// and a captured method ran no body when invoked.
///
/// Method-name pointer is expected to be stable for the closure's lifetime;
/// codegen emits it from the per-module `.str.N.bytes` rodata global.
#[no_mangle]
pub extern "C" fn js_class_method_bind(
    instance: f64,
    method_name_ptr: *const u8,
    method_name_len: usize,
) -> f64 {
    if !method_name_ptr.is_null() && method_name_len > 0 {
        if let Ok(name) = unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(method_name_ptr, method_name_len))
        } {
            if matches!(
                name,
                "append"
                    | "delete"
                    | "entries"
                    | "forEach"
                    | "get"
                    | "getSetCookie"
                    | "has"
                    | "keys"
                    | "set"
                    | "Symbol.iterator"
                    | "@@iterator"
                    | "values"
            ) {
                let bits = instance.to_bits();
                if (bits >> 48) == 0x7FFD {
                    let id = (bits & 0x0000_FFFF_FFFF_FFFF) as i64;
                    if crate::value::addr_class::is_small_handle(id as usize) {
                        if let Some(dispatch) = handle_property_dispatch() {
                            let value = HANDLE_PROPERTY_BIND_REENTRY.with(|guard| {
                                if guard.get() {
                                    None
                                } else {
                                    guard.set(true);
                                    let value =
                                        unsafe { dispatch(id, method_name_ptr, method_name_len) };
                                    guard.set(false);
                                    Some(value)
                                }
                            });
                            if let Some(value) = value {
                                if value.to_bits() != crate::value::TAG_UNDEFINED {
                                    return value;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Method IDENTITY (test262 class/elements): a class method is a single
    // shared function object, so `c.m`, `c2.m` and `C.prototype.m` must all be
    // the IDENTICAL value. Route every user-class method-as-value read through
    // the per-`(owner_class, name)` cached canonical built by
    // `class_prototype_method_value_for_name` instead of minting a fresh
    // per-receiver closure here. The canonical captures the OWNER class's
    // prototype-ref (capture 0); `dispatch_bound_method` recognises that marker
    // and supplies the call-site `this` (IMPLICIT_THIS) so invocations still see
    // the right receiver — e.g. the `this.m = this.m.bind(this)` idiom rebinds
    // correctly, and a bare `const f = c.m; f()` runs with the spec `this`.
    //
    // Guard against re-entry from `class_prototype_method_value_for_name`
    // itself: it builds the canonical by calling `build_bound_method_closure`
    // directly (NOT this function), so the cache is populated without looping.
    if !method_name_ptr.is_null() && method_name_len > 0 {
        if let Ok(name) = unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(method_name_ptr, method_name_len))
        } {
            if bound_native_method_length(name).is_none() {
                if let Some(class_id) = class_id_from_method_receiver(instance) {
                    if let Some(owner) =
                        super::class_registry::method_owner_class_id(class_id, name)
                    {
                        // [[Get]] order: an OWN data property of this name
                        // shadows the prototype method. The ubiquitous
                        // `this.m = this.m.bind(this)` idiom installs an own `m`
                        // (a bound function), so `obj.m` must read that own value
                        // back — not the shared prototype method. Skipping this
                        // both returned the wrong identity (`obj.m ===
                        // C.prototype.m` where Node says false) and looped when
                        // the canonical re-resolved `m` by name. A class
                        // prototype-ref receiver has no own-property bag, so this
                        // check is naturally a no-op there.
                        let recv_jsv = JSValue::from_bits(instance.to_bits());
                        if recv_jsv.is_pointer()
                            && !super::class_registry::is_registered_class_prototype_object(
                                crate::value::js_nanbox_get_pointer(instance) as usize,
                            )
                        {
                            let obj = recv_jsv.as_pointer::<ObjectHeader>();
                            if crate::value::addr_class::is_above_handle_band(obj as usize) {
                                let key = crate::string::js_string_from_bytes(
                                    method_name_ptr,
                                    method_name_len as u32,
                                );
                                if let Some(own) =
                                    unsafe { super::own_data_field_by_name(obj, key) }
                                {
                                    if own.bits() != crate::value::TAG_UNDEFINED {
                                        return f64::from_bits(own.bits());
                                    }
                                }
                            }
                        }
                        let canonical = class_prototype_method_value_for_name(owner, name);
                        if canonical.to_bits() != crate::value::TAG_UNDEFINED {
                            return canonical;
                        }
                    }
                }
            }
        }
    }

    build_bound_method_closure(instance, method_name_ptr, method_name_len)
}

/// By-ID sibling of `js_class_method_bind` for static-name lowering.
///
/// The current ID is the interned StringPool `StringHeader*` payload, but this
/// also accepts boxed heap/short-string ids so future lowering paths do not
/// reintroduce heap-only string assumptions.
#[no_mangle]
pub extern "C" fn js_class_method_bind_by_id(instance: f64, method_id: i64) -> f64 {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(name_ref) = crate::string::perry_string_ref_from_dispatch_id(method_id, &mut scratch)
    else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    if name_ref.heap.is_null() {
        let heap = crate::string::js_string_from_bytes(name_ref.ptr, name_ref.len as u32);
        let ptr = unsafe { (heap as *const u8).add(std::mem::size_of::<crate::StringHeader>()) };
        js_class_method_bind(instance, ptr, name_ref.len)
    } else {
        js_class_method_bind(instance, name_ref.ptr, name_ref.len)
    }
}

#[used]
static KEEP_CLASS_METHOD_BIND_BY_ID: extern "C" fn(f64, i64) -> f64 = js_class_method_bind_by_id;

/// Allocate a BOUND_METHOD closure binding `instance` as the receiver for the
/// named method, stamping its `.name`/`.length`. This is the raw builder used
/// by both `js_class_method_bind` (after its canonical-identity short-circuit)
/// and `class_prototype_method_value_for_name` (which caches one canonical per
/// `(class_id, name)`). Keeping it separate breaks the recursion that an
/// unconditional canonical lookup inside `js_class_method_bind` would create.
pub(crate) fn build_bound_method_closure(
    instance: f64,
    method_name_ptr: *const u8,
    method_name_len: usize,
) -> f64 {
    let closure = crate::closure::js_closure_alloc(crate::closure::BOUND_METHOD_FUNC_PTR, 3);
    crate::closure::js_closure_set_capture_f64(closure, 0, instance);
    crate::closure::js_closure_set_capture_ptr(closure, 1, method_name_ptr as i64);
    crate::closure::js_closure_set_capture_ptr(closure, 2, method_name_len as i64);
    if !method_name_ptr.is_null() && method_name_len > 0 {
        if let Ok(name) = unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(method_name_ptr, method_name_len))
        } {
            set_bound_native_closure_name(closure, name);
            if let Some(length) = bound_native_method_length(name) {
                set_builtin_closure_length(closure as usize, length);
            } else if let Some(class_id) = class_id_from_method_receiver(instance) {
                // User class method bound as a value (`C.prototype.m`, `c.m`):
                // stamp its spec `.length` from the registered param count so
                // `C.prototype.m.length` reflects the declared arity instead of
                // the closure's capture count (Test262 method `.length` tests).
                if let Some(length) =
                    super::class_registry::class_method_bind_length(class_id, name)
                {
                    set_builtin_closure_length(closure as usize, length);
                }
            }
        }
    }
    crate::value::js_nanbox_pointer(closure as i64)
}

/// #6173: sentinel "method name" installed in the name-capture slots (1, 2) of
/// a BOUND_METHOD closure whose target is a SYMBOL-keyed class method. A
/// symbol method has no string name to re-resolve at call time, so the
/// closure instead carries the already-resolved dispatch data in two extra
/// capture slots:
///
///   slot 0: receiver (NaN-boxed instance/prototype-ref, or the INT32 class
///           ref for a static method)
///   slot 1: `SYMBOL_BOUND_METHOD_NAME.as_ptr()` — the discriminant, compared
///           by ADDRESS in `dispatch_bound_method`, never by content
///   slot 2: `SYMBOL_BOUND_METHOD_NAME.len()`
///   slot 3: resolved method func_ptr
///   slot 4: packed meta — bits 0..32 param_count, bit 32 has_rest,
///           bit 33 is_static
///
/// Slots 1/2 deliberately remain a VALID `(ptr, len)` name pair pointing at
/// this static byte string: every reader that interprets a BOUND_METHOD's
/// captures as a method name (`bound_native_callable_module_and_method`, the
/// by-name dispatch fallbacks) stays memory-safe and merely sees a name that
/// resolves to nothing. Only pointer identity with THIS static means "symbol
/// bound"; even a pathological collision is harmless because reads of slots
/// 3/4 on a 3-capture name closure are bounds-checked to 0 → undefined.
pub(crate) static SYMBOL_BOUND_METHOD_NAME: &[u8] = b"@@__perry_symbol_bound_method__";

/// #6173: materialize a symbol-keyed class method (already resolved via
/// `lookup_class_symbol_method_in_chain`) as a callable bound-method value.
/// See [`SYMBOL_BOUND_METHOD_NAME`] for the capture layout. All captures are
/// populated immediately after allocation, BEFORE any allocating call — the
/// capture slots are GC-scanned roots (mirrors `build_bound_method_closure`).
pub(crate) fn build_symbol_bound_method_closure(
    receiver: f64,
    func_ptr: usize,
    param_count: u32,
    has_rest: bool,
    is_static: bool,
) -> f64 {
    let closure = crate::closure::js_closure_alloc(crate::closure::BOUND_METHOD_FUNC_PTR, 5);
    if closure.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    crate::closure::js_closure_set_capture_f64(closure, 0, receiver);
    crate::closure::js_closure_set_capture_ptr(
        closure,
        1,
        SYMBOL_BOUND_METHOD_NAME.as_ptr() as i64,
    );
    crate::closure::js_closure_set_capture_ptr(closure, 2, SYMBOL_BOUND_METHOD_NAME.len() as i64);
    crate::closure::js_closure_set_capture_ptr(closure, 3, func_ptr as i64);
    let meta: i64 = (param_count as i64) | ((has_rest as i64) << 32) | ((is_static as i64) << 33);
    crate::closure::js_closure_set_capture_ptr(closure, 4, meta);
    // Spec `.length` = declared params minus a trailing rest param.
    set_builtin_closure_length(
        closure as usize,
        if has_rest {
            param_count.saturating_sub(1)
        } else {
            param_count
        },
    );
    crate::gc::runtime_write_barrier_root_heap_word(closure as u64);
    crate::value::js_nanbox_pointer(closure as i64)
}

/// Resolve the owning class id for a `js_class_method_bind` receiver: a class
/// constructor/prototype ref (INT32-tagged) or a real class instance pointer.
/// Resolve the effective receiver for a BOUND_METHOD dispatch. When the
/// captured receiver is a canonical class-method marker (a class prototype-ref,
/// produced by `class_prototype_method_value_for_name`), substitute the
/// call-site `this` (IMPLICIT_THIS) provided it is itself a dispatchable class
/// receiver (an instance or class ref). Otherwise the captured value is the real
/// receiver and is returned unchanged. See `dispatch_bound_method`.
/// Is `value` a bound STATIC-method value — a BOUND_METHOD closure whose
/// captured receiver is a class constructor ref (`C.staticMethod` read as a
/// value)? Used by the Function.prototype call/apply arms to arm the one-shot
/// static-`this` override with the explicit thisArg, so the static method body
/// sees the receiver (`C.m.call({})` → `this === {}`) and static private brand
/// checks behave per spec.
pub(crate) fn is_static_bound_method_value(value: f64) -> bool {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let raw = (value.to_bits() & crate::value::POINTER_MASK) as usize;
    if !crate::closure::is_closure_ptr(raw) {
        return false;
    }
    let closure = raw as *const crate::closure::ClosureHeader;
    if !std::ptr::eq(
        unsafe { (*closure).func_ptr },
        crate::closure::BOUND_METHOD_FUNC_PTR,
    ) {
        return false;
    }
    let captured = crate::closure::js_closure_get_capture_f64(closure, 0);
    class_ref_id(captured).is_some() && class_prototype_ref_id(captured).is_none()
}

pub(crate) fn canonical_bound_method_receiver(captured: f64) -> f64 {
    if class_prototype_ref_id(captured).is_some() {
        let call_this = super::js_implicit_this_get();
        if class_id_from_method_receiver(call_this).is_some() {
            return call_this;
        }
        // #6475: a FUNCTION-object call-site `this` — effect's `TagClass`, a
        // plain function given the Tag class prototype via
        // `Object.setPrototypeOf(TagClass, Object.getPrototypeOf(tagInstance))` —
        // is a legitimate spec receiver for an inherited class method
        // (`TagClass.pipe(...)`: `pipe` lives on the Tag class prototype and
        // must run with `this === TagClass`). `class_id_from_method_receiver`
        // deliberately rejects closures (reading `class_id` off a
        // `ClosureHeader` is type confusion), but that guard protects
        // RESOLUTION — and `dispatch_bound_method` resolves the method from
        // the CAPTURED owner proto-ref, never from this substituted receiver.
        // Passing the closure through only changes the `this` the body
        // observes, which previously leaked the INT32 proto-ref marker
        // (`typeof this === "number"`): effect's Pipeable composed against
        // it, `HttpApiBuilder.group(...)` returned a curried function instead
        // of a Layer, and web.ts died with "Not a valid effect: undefined".
        let jv = JSValue::from_bits(call_this.to_bits());
        if jv.is_pointer() {
            let raw = (call_this.to_bits() & crate::value::POINTER_MASK) as usize;
            if crate::closure::is_closure_ptr(raw) {
                return call_this;
            }
        }
    }
    captured
}

fn class_id_from_method_receiver(instance: f64) -> Option<u32> {
    if let Some(cid) = class_ref_id(instance) {
        return Some(cid);
    }
    let jsv = JSValue::from_bits(instance.to_bits());
    if jsv.is_pointer() {
        let obj = jsv.as_pointer::<ObjectHeader>();
        if crate::value::addr_class::is_above_handle_band(obj as usize) {
            // A callable (closure / function object) is never a class-method
            // receiver for bound-method marker substitution. Its allocation is a
            // `ClosureHeader`, so reading `class_id` off it as an `ObjectHeader`
            // is a type confusion that can yield a stray non-zero id. Without
            // this guard, a free call to a `C.prototype.method` bound-method
            // value made from inside a function-object method body (e.g.
            // test262's `assert.throws(…, function(){ m(...) })`, where
            // `IMPLICIT_THIS` is the `assert` function) would mis-substitute the
            // function object as the receiver and dispatch `assert.method(...)`
            // instead of `C.prototype.method`, bypassing the generator wrapper's
            // param prologue. See `canonical_bound_method_receiver`.
            if crate::closure::is_closure_ptr(obj as usize) {
                return None;
            }
            let cid = unsafe { (*obj).class_id };
            if cid != 0 {
                return Some(cid);
            }
        }
    }
    None
}

pub(crate) const CLASS_PROTOTYPE_REF_FLAG: u64 = 1u64 << 32;

pub(crate) fn class_constructor_ref_value(class_id: u32) -> f64 {
    f64::from_bits(0x7FFE_0000_0000_0000u64 | (class_id as u64 & 0xFFFF_FFFF))
}

pub(crate) fn class_prototype_ref_value(class_id: u32) -> f64 {
    f64::from_bits(
        0x7FFE_0000_0000_0000u64 | CLASS_PROTOTYPE_REF_FLAG | (class_id as u64 & 0xFFFF_FFFF),
    )
}

pub(crate) fn class_prototype_ref_id(value: f64) -> Option<u32> {
    let bits = value.to_bits();
    if (bits >> 48) == 0x7FFE && (bits & CLASS_PROTOTYPE_REF_FLAG) != 0 {
        let class_id = (bits & 0xFFFF_FFFF) as u32;
        if class_id != 0 && is_class_id_registered(class_id) {
            return Some(class_id);
        }
    }
    None
}

pub(crate) fn class_ref_id(value: f64) -> Option<u32> {
    let bits = value.to_bits();
    if (bits >> 48) == 0x7FFE {
        let class_id = (bits & 0xFFFF_FFFF) as u32;
        if class_id != 0 && is_class_id_registered(class_id) {
            return Some(class_id);
        }
    }
    None
}

pub(crate) unsafe fn metadata_key_to_string(value: f64) -> Option<String> {
    let key_str = crate::builtins::js_string_coerce(value);
    if key_str.is_null() {
        return None;
    }
    let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let name_len = (*key_str).byte_len as usize;
    std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
        .ok()
        .map(|s| s.to_string())
}

pub(crate) fn class_has_own_method(class_id: u32, method_name: &str) -> bool {
    let registry = match CLASS_VTABLE_REGISTRY.read() {
        Ok(g) => g,
        Err(_) => return false,
    };
    registry
        .as_ref()
        .and_then(|reg| reg.get(&class_id))
        .map(|vtable| vtable.methods.contains_key(method_name))
        .unwrap_or(false)
}

/// Wall 10 — `name in instance` for a class instance: true when `name` is a
/// prototype METHOD, GETTER, or SETTER anywhere in the instance's class chain.
/// Class instance methods/accessors live in `CLASS_VTABLE_REGISTRY` (the
/// instance carries no recorded `[[Prototype]]` object with a `keys_array`), so
/// the ordinary own-key + recorded-prototype walk in `js_object_has_property`
/// misses them — making `'method' in instance` wrongly `false`. NestJS's app
/// Proxy gates routing on `'listen' in receiver`; the false result misrouted
/// `app.listen`, so the server never bound. Walk the class parent chain here.
pub(crate) fn class_instance_has_member(class_id: u32, name: &str) -> bool {
    if class_id == 0 {
        return false;
    }
    let registry = match CLASS_VTABLE_REGISTRY.read() {
        Ok(g) => g,
        Err(_) => return false,
    };
    let Some(reg) = registry.as_ref() else {
        return false;
    };
    let mut cid = class_id;
    let mut depth = 0u32;
    while cid != 0 && depth < 32 {
        if let Some(vtable) = reg.get(&cid) {
            // Honor `delete C.prototype.m`: a deleted key must report `false`
            // from `'m' in new C()`, matching the descriptor/static lookup paths.
            if !super::class_registry::class_is_key_deleted(cid, name)
                && (vtable.methods.contains_key(name)
                    || vtable.getters.contains_key(name)
                    || vtable.setters.contains_key(name))
            {
                return true;
            }
        }
        match super::class_registry::get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    false
}

pub fn class_prototype_method_value_for_name(class_id: u32, method_name: &str) -> f64 {
    if let Some(bits) = CLASS_PROTOTYPE_METHOD_VALUES.with(|cache| {
        let cache = cache.borrow();
        if let Some(bits) = cache.get(&(class_id, method_name.to_string())).copied() {
            return Some(bits);
        }
        None
    }) {
        return f64::from_bits(bits);
    }

    // Bounded leak: `js_class_method_bind` keeps the byte pointer for the
    // lifetime of the bound closure (it's stashed inside the closure's
    // capture frame). We leak one allocation per unique
    // `(class_id, method_name)` pair the program ever asks for, so the
    // total leak is bounded by the static set of decorated method
    // descriptors. The cache below short-circuits repeat queries.
    let leaked: &'static [u8] = method_name.as_bytes().to_vec().leak();
    let class_ref = class_prototype_ref_value(class_id);
    // Build the closure DIRECTLY (not via `js_class_method_bind`, whose
    // canonical short-circuit would call back into this function and recurse).
    // The captured receiver is the prototype-ref, which doubles as the
    // "canonical class method" marker that `dispatch_bound_method` keys on.
    let value = build_bound_method_closure(class_ref, leaked.as_ptr(), leaked.len());
    class_prototype_method_value_cache_root_store(
        class_id,
        method_name.to_string(),
        value.to_bits(),
    );
    value
}

#[no_mangle]
pub extern "C" fn js_class_prototype_method_value(class_ref: f64, method_key: f64) -> f64 {
    let Some(class_id) = class_ref_id(class_ref) else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    let method_name = unsafe { metadata_key_to_string(method_key) };
    let Some(method_name) = method_name else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    class_prototype_method_value_for_name(class_id, &method_name)
}

/// Extract the module name string from a native module namespace object.
pub(crate) unsafe fn get_module_name_from_namespace(namespace_obj: f64) -> &'static str {
    let jsval = JSValue::from_bits(namespace_obj.to_bits());
    if !jsval.is_pointer() {
        return "";
    }
    let obj = jsval.as_pointer::<ObjectHeader>();
    if crate::value::addr_class::is_handle_band(obj as usize) {
        return "";
    }
    let module_field = js_object_get_field(obj as *mut _, 0);
    if !module_field.is_any_string() {
        return "";
    }
    // #1781: SSO-aware — ≤5-byte module names (fs, os, …) arrive as
    // SHORT_STRING_TAG values; route through `js_get_string_pointer_unified`
    // so SSO materializes onto the GC-managed heap (where its bytes
    // share the lifetime story the STRING_TAG path already assumes
    // for the `&'static` lie this signature carries).
    let module_f64 = f64::from_bits(module_field.bits());
    let str_ptr =
        crate::value::js_get_string_pointer_unified(module_f64) as *const crate::StringHeader;
    if str_ptr.is_null() || (str_ptr as usize) < 0x1000 {
        return "";
    }
    let len = (*str_ptr).byte_len as usize;
    let data = (str_ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    std::str::from_utf8(std::slice::from_raw_parts(data, len)).unwrap_or("")
}

// ─── Vtable impls relocated from field_get_set.rs (EN size work) ───────
// Bodies moved verbatim so their table references are reachable only
// through the installed vtable. See `NativeModuleVtable`.

/// Own-field read on a namespace object (`fs.constants`, method values,
/// process IPC props, …). Returns `None` when the receiver carries no
/// module name — the caller falls through to the generic field scan.
unsafe fn vt_get_own_field(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let key_len = (*key).byte_len as usize;
    let nb_ptr = crate::value::js_nanbox_pointer(obj as i64);
    let module_name = get_module_name_from_namespace(nb_ptr);
    if module_name.is_empty() {
        return None;
    }
    let property_name =
        std::str::from_utf8(std::slice::from_raw_parts(key_ptr, key_len)).unwrap_or("");
    // A user override (`require('node:timers').setImmediate = patched`)
    // wins all built-in resolution below — CJS exports are mutable in Node.
    if let Some(value) = native_namespace_prop_override_get(module_name, property_name) {
        return Some(JSValue::from_bits(value.to_bits()));
    }
    if matches!(
        module_name,
        "process" | "process.namespace" | "process.default"
    ) {
        if let Some(value) = crate::process::process_ipc_property(property_name) {
            return Some(JSValue::from_bits(value.to_bits()));
        }
    }
    if let Some(value) = super::field_get_set::native_module_own_field_by_key(obj, key) {
        return Some(value);
    }
    // #3687: node:cluster default-import EventEmitter methods on the
    // distinct `cluster.default` namespace (see original comment at the
    // pre-relocation site in field_get_set.rs history).
    if module_name == "cluster.default" && super::is_cluster_emitter_method(property_name) {
        return Some(JSValue::from_bits(
            bound_native_callable_export_value(module_name, property_name).to_bits(),
        ));
    }
    if let Some(val) = get_native_module_constant(module_name, property_name, nb_ptr) {
        return Some(JSValue::from_bits(val.to_bits()));
    }
    if module_name == "crypto.webcrypto" {
        if let Some(value) = super::global_this::webcrypto_method_value(property_name) {
            return Some(JSValue::from_bits(value.to_bits()));
        }
    }
    if module_name == "crypto.subtle" {
        if let Some(value) = super::global_this::subtle_crypto_method_value(property_name) {
            return Some(JSValue::from_bits(value.to_bits()));
        }
    }
    // Issue #894: callable exports (`("events", "EventEmitter")` …) get a
    // bound-method closure for require-then-member-access parity.
    if is_native_module_callable_export(module_name, property_name) {
        return Some(JSValue::from_bits(
            bound_native_callable_export_value(module_name, property_name).to_bits(),
        ));
    }
    // Object-valued exports (e.g. `perf_hooks.performance` / `.constants`) are
    // resolved by the shared per-property dispatch but are not covered by the
    // override / constant / callable checks above. Without delegating, a DYNAMIC
    // namespace read (`createRequire(...)("perf_hooks").performance`,
    // `process.getBuiltinModule(...)`) returned undefined for them while the
    // static codegen path resolved them via `js_native_module_property_by_name`.
    // Defer to that authoritative resolver so dynamic namespaces match static.
    let resolved = js_native_module_property_by_name(
        module_name.as_ptr(),
        module_name.len(),
        key_ptr,
        key_len,
    );
    Some(JSValue::from_bits(resolved.to_bits()))
}

/// `Object.keys(namespace)` — fresh array of the module's enumerable
/// keys. `None` when the module is unknown; caller falls back to the
/// generic keys_array path. Also reused by `Object.getOwnPropertyNames`
/// (#5268): a native-module object must enumerate its export surface there
/// too, not the internal `__module__` sentinel.
pub(crate) unsafe fn vt_own_keys_array(
    obj: *const ObjectHeader,
) -> Option<*mut crate::array::ArrayHeader> {
    let module_name = read_native_module_name(obj)?;
    let keys = native_module_enumerable_keys(&module_name)?;
    let include_permission = matches!(
        module_name.as_str(),
        "process" | "process.namespace" | "process.default"
    ) && crate::process::process_permission_enabled();
    let out = crate::array::js_array_alloc(keys.len() as u32 + include_permission as u32);
    for key_bytes in keys {
        let key_str =
            crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
        crate::array::js_array_push(out, JSValue::string_ptr(key_str));
    }
    if include_permission {
        let key_str =
            crate::string::js_string_from_bytes(b"permission".as_ptr(), b"permission".len() as u32);
        crate::array::js_array_push(out, JSValue::string_ptr(key_str));
    }
    Some(out)
}
