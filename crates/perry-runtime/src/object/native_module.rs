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
use std::collections::HashMap;
use std::ptr::null_mut;
use std::sync::{
    atomic::{AtomicPtr, Ordering},
    OnceLock, RwLock,
};

mod async_hooks_exports;
mod callable_export_arity_table;
mod callable_export_check;
mod callable_export_table;
pub(crate) mod callable_exports;
mod perf_instance_bind;
pub(crate) use perf_instance_bind::{instance_bound_perf_method, performance_namespace_method};
pub(crate) mod constants;
mod constants_tables;
mod constructor_exports;
pub(crate) mod constructor_shapes;
mod module_keys;
mod name_tables;
mod namespace_builders;
mod namespace_prototype;
mod vtable_impls;
mod web_locks;
pub(crate) mod worker_environment;

pub(crate) use callable_export_check::is_native_module_callable_export;
pub use callable_exports::bound_native_callable_export_value;
#[cfg(test)]
pub(crate) use callable_exports::builtin_closure_is_non_constructable;
pub(crate) use callable_exports::minted_native_callable_export;
#[cfg(test)]
pub(crate) use callable_exports::test_collect_native_export_after_alloc;
pub(crate) use callable_exports::{
    bound_native_callable_module_and_method, bound_native_callable_value_arity,
    bound_native_parent, buffer_constructor_value, buffer_intrinsic_prototype_parent,
    buffer_intrinsic_prototype_value, buffer_original_prototype_value,
    builtin_closure_is_non_constructable_value, builtin_closure_length,
    cached_buffer_intrinsic_prototype_value, fs_namespace_descriptor_getter_value,
    fs_namespace_descriptor_setter_value, install_module_cjs_cache_value,
    is_buffer_constructor_value, is_cluster_emitter_method, module_builtin_modules_value,
    module_cjs_cache_value, module_cjs_extensions_value, module_cjs_global_paths_value,
    module_cjs_path_cache_value, module_cjs_prototype_for_instance, module_constants_value,
    native_string_value, scan_tls_derived_prototype_roots_mut, set_bound_native_closure_metadata,
    set_bound_native_closure_name, set_builtin_closure_length,
    set_builtin_closure_non_constructable, timers_promises_parent_namespace,
    tls_constructor_prototype_is_instance_of, util_inspect_default_options_value,
    zlib_codes_object, BoundNativeParent,
};
pub(crate) use constants::{get_native_module_constant, native_module_constant_is_live};
pub(crate) use constructor_exports::{
    bound_native_callable_is_constructor_value, is_native_module_constructor_export,
};
pub(crate) use module_keys::{
    native_module_enumerable_keys, native_module_has_enumerable_key,
    zlib_readonly_property_enumerability,
};
use name_tables::should_cache_native_module_namespace;
pub(crate) use name_tables::{assert_instance_base_module, canonical_native_callable_property};
#[cfg(test)]
pub(crate) use namespace_builders::create_fs_constants_object;
pub(crate) use namespace_builders::{
    create_cached_sub_namespace, create_sub_namespace, http_global_agent_object,
    http_methods_array, http_status_codes_object, https_global_agent_object,
    native_namespace_or_create,
};
pub(crate) use namespace_prototype::{
    native_module_namespace_default_prototype, native_module_namespace_prototype_bits,
};
use vtable_impls::vt_get_own_field;
pub(crate) use vtable_impls::vt_own_keys_array;
pub(crate) use web_locks::{worker_threads_locks_value, WebLocksState};

crate::perry_thread_local! {
    /// Every minted bound export by `"<module>\0<property>"`. Read on hot
    /// paths (`new EventEmitter()` resolves its prototype through it), so it
    /// hashes with aHash, which keeps a random key without SipHash's rounds.
    pub(crate) static NATIVE_CALLABLE_EXPORTS: RefCell<HashMap<String, u64, crate::fast_hash::AHashState>> =
        RefCell::new(HashMap::default());
    pub(crate) static NATIVE_MODULE_ACCESSOR_EXPORTS: RefCell<HashMap<String, u64>> =
        RefCell::new(HashMap::new());
    static HANDLE_PROPERTY_BIND_REENTRY: Cell<bool> = const { Cell::new(false) };
    pub(crate) static BUFFER_CONSTRUCTOR_VALUE: Cell<u64> = const { Cell::new(0) };
    /// `Buffer.poolSize` as JS value bits. Node 26 raised its initial value
    /// from 8 KiB to 64 KiB. It is a plain writable property of each realm's
    /// `Buffer`, so any value may be stored: per-thread (a worker's write never
    /// reaches the main thread, as in Node) and visited by
    /// `scan_native_callable_export_roots_mut`. #11471: it used to be a
    /// process-global atomic, which kept an object/string written by an
    /// exited thread as a dangling, unrooted address.
    static BUFFER_POOL_SIZE_BITS: Cell<u64> = const { Cell::new(65536f64.to_bits()) };
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
    pub(crate) static MODULE_CJS_PROTOTYPE_VALUE: Cell<u64> = const { Cell::new(0) };
    pub(crate) static MODULE_BUILTIN_MODULES_VALUE: Cell<u64> = const { Cell::new(0) };
    pub(crate) static MODULE_CONSTANTS_VALUE: Cell<u64> = const { Cell::new(0) };
    pub(crate) static NATIVE_MODULE_NAMESPACES: RefCell<HashMap<String, u64>> =
        RefCell::new(HashMap::new());
    /// User overrides of native-module namespace properties, keyed
    /// `"{module}\0{prop}"`. CommonJS module exports are MUTABLE in Node —
    /// monkey-patching like Next.js's
    /// `require('node:timers').setImmediate = patched` must store and win
    /// subsequent property reads instead of throwing read-only.
    static NATIVE_NAMESPACE_PROP_OVERRIDES: RefCell<HashMap<String, u64>> =
        RefCell::new(HashMap::new());
    /// EventEmitter-compatible listeners for the two built-in global Agent
    /// objects. They live here (rather than in ext-http) because the namespace
    /// objects and their prototype methods are runtime-owned GC objects.
    static GLOBAL_AGENT_LISTENERS: RefCell<HashMap<(bool, String), Vec<GlobalAgentListener>>> =
        RefCell::new(HashMap::new());
    static NATIVE_ESM_EXPORT_VALUES: RefCell<HashMap<String, u64>> =
        RefCell::new(HashMap::new());
}

#[derive(Clone, Copy)]
struct GlobalAgentListener {
    callback_bits: u64,
    once: bool,
}

/// Store a user override for a native-module namespace property
/// (`require('node:timers').setImmediate = fn`). Wins subsequent reads via
/// `vt_get_own_field`.
pub(crate) fn native_namespace_prop_override_store(module: &str, prop: &str, value: f64) {
    NATIVE_NAMESPACE_PROP_OVERRIDES.with(|m| {
        m.borrow_mut()
            .insert(format!("{module}\0{prop}"), value.to_bits());
    });
    // `node:tls` is a CommonJS builtin and its default import is the mutable
    // exports object. Codegen currently shares the snapshot-backed property
    // read used by native ESM imports for that default object, so keep the TLS
    // defaults in that cache coherent with writes to the default export. Do
    // not do this for ordinary builtin named exports: those intentionally stay
    // unchanged until `module.syncBuiltinESMExports()` is called.
    if module == "tls"
        && matches!(
            prop,
            "DEFAULT_CIPHERS" | "DEFAULT_MIN_VERSION" | "DEFAULT_MAX_VERSION"
        )
    {
        let key = format!("{module}\0{prop}");
        NATIVE_ESM_EXPORT_VALUES.with(|values| {
            if let Some(slot) = values.borrow_mut().get_mut(&key) {
                *slot = value.to_bits();
            }
        });
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
    }
}

/// Read back a stored native-namespace property override, if any.
pub(crate) fn native_namespace_prop_override_get(module: &str, prop: &str) -> Option<f64> {
    NATIVE_NAMESPACE_PROP_OVERRIDES.with(|m| {
        let m = m.borrow();
        // #10523: every native namespace member read lands here and almost no
        // program monkey-patches one, so skip building the probe key.
        if m.is_empty() {
            return None;
        }
        m.get(&format!("{module}\0{prop}"))
            .map(|bits| f64::from_bits(*bits))
    })
}

/// pi boot blocker: a user write to a builtin namespace member must win every
/// subsequent NAME-KEYED read, no matter which lowering performed the store.
/// Today the stores are split: computed writes (`process[k] = fn`) land in
/// `NATIVE_NAMESPACE_PROP_OVERRIDES` via `nm_field_set_override`, while static
/// writes (`process.chdir = fn`) reach the generic store path and land as an
/// OWN dynamic field on the canonical namespace object. The name-keyed read
/// entries (`js_native_module_property_by_name`,
/// `js_native_module_esm_export_value`) carry no object pointer, so they only
/// consulted the override table — a static write was invisible to them and
/// the read handed back the canonical BOUND_METHOD closure again. graceful-fs
/// then did `Object.setPrototypeOf(process.chdir, chdir)` with the SAME
/// closure on both sides and pi's boot died on the resulting (correct)
/// "Cyclic __proto__ value" self-set rejection. Consult BOTH stores. This
/// never CREATES a namespace: if none was ever built, no user store can have
/// landed on one.
pub(crate) fn native_namespace_user_value(module: &str, prop: &str) -> Option<f64> {
    if let Some(value) = native_namespace_prop_override_get(module, prop) {
        return Some(value);
    }
    // Build the probe key BEFORE reading the cached namespace bits: the
    // string allocation can run a moving collection, and the cache slot is
    // rewritten by `scan_native_callable_export_roots_mut`, so bits read afterwards
    // are current.
    let key = crate::string::js_string_from_bytes(prop.as_ptr(), prop.len() as u32);
    let ns_bits = NATIVE_MODULE_NAMESPACES.with(|cache| cache.borrow().get(module).copied())?;
    let obj = (ns_bits & crate::value::POINTER_MASK) as *const ObjectHeader;
    if obj.is_null() {
        return None;
    }
    // Accessor descriptors occupy the property's ordinary slot. Do not expose
    // that storage object as an override (or snapshot it as a named export).
    if let Some(accessor) = super::descriptor_state::get_accessor_descriptor(obj as usize, prop) {
        if accessor.get == 0 {
            return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
        }
        let scope = crate::gc::RuntimeHandleScope::new();
        let receiver = scope.root_nanbox_u64(ns_bits);
        let value = unsafe {
            super::field_get_set::invoke_accessor_getter(accessor.get, receiver.get_nanbox_f64())
        };
        return Some(f64::from_bits(value.bits()));
    }
    unsafe { super::field_get_set::native_module_own_field_by_key(obj, key) }
        .map(|v| f64::from_bits(v.bits()))
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
pub extern "C" fn js_vm_create_context(sandbox: f64, options: f64) -> f64 {
    crate::node_vm::create_context(sandbox, options)
}

#[no_mangle]
pub extern "C" fn js_vm_create_script_branded(code: f64, options: f64) -> f64 {
    crate::node_vm::dispatch_vm_method(
        "createScript",
        code,
        options,
        f64::from_bits(crate::value::TAG_UNDEFINED),
    )
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
    GLOBAL_AGENT_LISTENERS.with(|listeners| {
        for callbacks in listeners.borrow_mut().values_mut() {
            for listener in callbacks {
                visitor.visit_nanbox_u64_slot(&mut listener.callback_bits);
            }
        }
    });
    NATIVE_ESM_EXPORT_VALUES.with(|cache| {
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
    BUFFER_POOL_SIZE_BITS.with(|slot| {
        let mut value_bits = slot.get();
        visitor.visit_nanbox_u64_slot(&mut value_bits);
        slot.set(value_bits);
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
    MODULE_CJS_PROTOTYPE_VALUE.with(|slot| {
        let mut value_bits = slot.get();
        if value_bits != 0 {
            visitor.visit_nanbox_u64_slot(&mut value_bits);
            slot.set(value_bits);
        }
    });
    MODULE_BUILTIN_MODULES_VALUE.with(|slot| {
        let mut value_bits = slot.get();
        if value_bits != 0 {
            visitor.visit_nanbox_u64_slot(&mut value_bits);
            slot.set(value_bits);
        }
    });
    MODULE_CONSTANTS_VALUE.with(|slot| {
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
    // #6468: only present when the program imports `node:http2`; until the
    // install the `sensitiveHeaders` symbol slot doesn't exist, so there's no
    // root to scan.
    crate::module_hooks::http2_scan_roots(visitor);
    scan_tls_derived_prototype_roots_mut(visitor);
}

/// Special class ID for native module namespace objects
/// This is used to identify objects that represent native module namespaces
pub const NATIVE_MODULE_CLASS_ID: u32 = 0xFFFFFFFE;
pub(crate) const WORKER_THREADS_LOCK_MANAGER_CLASS_ID: u32 = 0xFFFF_00B1;
pub(crate) const WORKER_THREADS_LOCK_CLASS_ID: u32 = 0xFFFF_00B2;

type WorkerThreadsValueGetter = extern "C" fn() -> f64;

pub(crate) static WORKER_THREADS_WORKER_DATA_GETTER: AtomicPtr<()> = AtomicPtr::new(null_mut());
pub(crate) static WORKER_THREADS_IS_MAIN_THREAD_GETTER: AtomicPtr<()> = AtomicPtr::new(null_mut());
pub(crate) static WORKER_THREADS_PARENT_PORT_GETTER: AtomicPtr<()> = AtomicPtr::new(null_mut());
pub(crate) static WORKER_THREADS_THREAD_NAME_GETTER: AtomicPtr<()> = AtomicPtr::new(null_mut());
pub(crate) static WORKER_THREADS_RESOURCE_LIMITS_GETTER: AtomicPtr<()> = AtomicPtr::new(null_mut());
pub(crate) static WORKER_THREADS_THREAD_ID_GETTER: AtomicPtr<()> = AtomicPtr::new(null_mut());
static WORKER_THREADS_WORKER_CONSTRUCTOR: AtomicPtr<()> = AtomicPtr::new(null_mut());

#[no_mangle]
pub extern "C" fn js_register_worker_threads_namespace_getters(
    worker_data: WorkerThreadsValueGetter,
    is_main_thread: WorkerThreadsValueGetter,
    parent_port: WorkerThreadsValueGetter,
    thread_name: WorkerThreadsValueGetter,
    resource_limits: WorkerThreadsValueGetter,
    thread_id: WorkerThreadsValueGetter,
) {
    WORKER_THREADS_WORKER_DATA_GETTER.store(worker_data as *mut (), Ordering::Release);
    WORKER_THREADS_IS_MAIN_THREAD_GETTER.store(is_main_thread as *mut (), Ordering::Release);
    WORKER_THREADS_PARENT_PORT_GETTER.store(parent_port as *mut (), Ordering::Release);
    WORKER_THREADS_THREAD_NAME_GETTER.store(thread_name as *mut (), Ordering::Release);
    WORKER_THREADS_RESOURCE_LIMITS_GETTER.store(resource_limits as *mut (), Ordering::Release);
    WORKER_THREADS_THREAD_ID_GETTER.store(thread_id as *mut (), Ordering::Release);
}

type WorkerThreadsWorkerConstructor = extern "C" fn(f64, f64) -> f64;

/// perry-stdlib registers its `new Worker(filename, options)` here, so a
/// `Worker` reached through a namespace value (`getBuiltinModule`, a
/// `require` result, a stored reference) constructs a real Worker.
#[no_mangle]
pub extern "C" fn js_register_worker_threads_worker_constructor(
    construct: WorkerThreadsWorkerConstructor,
) {
    WORKER_THREADS_WORKER_CONSTRUCTOR.store(construct as *mut (), Ordering::Release);
}

/// `new ns.Worker(filename, options)` where `ns` is the worker_threads
/// namespace reached as a value (`process.getBuiltinModule`, `require`). The
/// compiler cannot see such a call site, so the stdlib looks the filename up
/// in the table of worker entries compiled into the binary.
///
/// # Safety
/// `args_ptr` must point to `args_len` values or be null.
pub(crate) unsafe fn worker_threads_construct(
    module: &str,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if module != "worker_threads" || method != "Worker" {
        return None;
    }
    let ptr = WORKER_THREADS_WORKER_CONSTRUCTOR.load(Ordering::Acquire);
    if ptr.is_null() {
        return None;
    }
    // NOT-A-JS-BODY: a native Rust helper registered by another crate.
    let construct: WorkerThreadsWorkerConstructor = std::mem::transmute(ptr);
    let arg = |n: usize| {
        if !args_ptr.is_null() && args_len > n {
            *args_ptr.add(n)
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
    };
    Some(construct(arg(0), arg(1)))
}

pub(crate) fn call_worker_threads_getter(
    slot: &AtomicPtr<()>,
    fallback: impl FnOnce() -> f64,
) -> f64 {
    let ptr = slot.load(Ordering::Acquire);
    if ptr.is_null() {
        return fallback();
    }
    // NOT-A-JS-BODY: a native Rust helper registered by another crate.
    let getter: WorkerThreadsValueGetter = unsafe { std::mem::transmute(ptr) };
    getter()
}

pub(crate) fn buffer_pool_size() -> f64 {
    let constructor = BUFFER_CONSTRUCTOR_VALUE.with(Cell::get);
    if constructor != 0 {
        // The ordinary function property is the authority once Buffer has
        // been materialized. Its bag also receives normal JS assignments.
        let ptr = (constructor & crate::value::POINTER_MASK) as usize;
        return crate::closure::closure_get_dynamic_prop(ptr, "poolSize");
    }
    f64::from_bits(BUFFER_POOL_SIZE_BITS.with(Cell::get))
}

pub(crate) fn set_buffer_pool_size(value: f64) {
    BUFFER_POOL_SIZE_BITS.with(|slot| slot.set(value.to_bits()));
    crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
    let constructor = BUFFER_CONSTRUCTOR_VALUE.with(Cell::get);
    if constructor != 0 {
        crate::closure::closure_set_dynamic_prop(
            (constructor & crate::value::POINTER_MASK) as usize,
            "poolSize",
            value,
        );
    }
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

/// Generic-object-path behaviors for namespace objects, referenced ONLY from
/// here (see `nm_namespace_hooks`): descriptors / dynamic stores / reflect
/// probes / key enumeration link into a binary exactly when a namespace
/// object can exist.
static NM_NAMESPACE_OPS_IMPL: super::NmNamespaceOps = super::NmNamespaceOps {
    get_own_descriptor: super::descriptors::nm_get_own_descriptor,
    field_set_override: super::field_set_by_name::nm_field_set_override,
    reflect_has_enumerable: super::reflect_support::nm_reflect_has_enumerable,
    own_keys_array: nm_own_keys_array_opt,
    bind_method: nm_bind_method_ops,
};

static NM_EE_OPS_IMPL: super::NmEeOps = super::NmEeOps {
    ee_prototype_install: super::class_registry::prototype_objects::nm_ee_prototype_install,
    ee_prototype_layout: super::class_registry::prototype_objects::nm_ee_prototype_layout,
    ee_dynamic_super: nm_ee_dynamic_super,
};

/// Arm the EventEmitter ops (see `NmEeOps`). Called by
/// `js_nm_install_events()` / `js_nm_install_stream()`.
pub(crate) fn install_nm_ee_ops() {
    super::arm_nm_ee_ops(&NM_EE_OPS_IMPL);
}

/// Dynamic-`super()` EventEmitter-subclass init (extracted from
/// `closure::dispatch::value_call`; see `NmEeOps::ee_dynamic_super`).
unsafe fn nm_ee_dynamic_super(
    func_value: f64,
    this: crate::closure::JsThis,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let (module, method) = bound_native_callable_module_and_method(func_value)?;
    let module = module.trim_start_matches("node:");
    // #10430: legacy `Stream` is `function Stream(opts) { EE.call(this, opts) }`.
    if (module == "events" && (method == "EventEmitter" || method == "EventEmitterAsyncResource"))
        || (module == "stream" && method == "Stream")
    {
        let this_val = this.as_f64();
        if crate::value::JSValue::from_bits(this_val.to_bits()).is_pointer() {
            if method == "EventEmitterAsyncResource" {
                let options = if !args_ptr.is_null() && args_len > 0 {
                    *args_ptr
                } else {
                    f64::from_bits(crate::value::TAG_UNDEFINED)
                };
                return Some(
                    crate::node_stream::js_event_emitter_async_resource_subclass_init(
                        this_val, options,
                    ),
                );
            }
            let options = if !args_ptr.is_null() && args_len > 0 {
                *args_ptr
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            return Some(crate::node_stream::js_event_emitter_subclass_init(
                this_val, options,
            ));
        }
    }
    None
}

unsafe fn nm_bind_method_ops(obj_value: f64, name_ptr: *const u8, name_len: usize) -> f64 {
    js_native_module_bind_method(obj_value, name_ptr, name_len)
}

unsafe fn nm_own_keys_array_opt(
    obj: *const super::ObjectHeader,
) -> Option<*mut crate::array::ArrayHeader> {
    vt_own_keys_array(obj)
}

/// Make the native-module vtable reachable. Must be called by every code
/// path that creates a NATIVE_MODULE_CLASS_ID object — this is the only
/// static reference to the dispatch/table machinery in the crate.
pub(crate) fn install_native_module_vtable() {
    super::arm_nm_namespace_ops(&NM_NAMESPACE_OPS_IMPL);
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

/// Create a native module namespace object
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
    if module_name == "wasi" {
        crate::wasi_hook_slots::emit_wasi_static_warning();
    }
    if should_cache_native_module_namespace(module_name) {
        if let Some(bits) =
            NATIVE_MODULE_NAMESPACES.with(|cache| cache.borrow().get(module_name).copied())
        {
            return f64::from_bits(bits);
        }
    }
    crate::realm_decl! { "%NativeNamespace:assert%", provider = native, births = [Object]; "%NativeNamespace:async_hooks%", provider = native, births = [Object]; "%NativeNamespace:buffer%", provider = native, births = [Object]; "%NativeNamespace:buffer.Buffer%", provider = native, births = [Object]; "%NativeNamespace:child_process%", provider = native, births = [Object]; "%NativeNamespace:cluster%", provider = native, births = [Object]; "%NativeNamespace:console%", provider = native, births = [Object]; "%NativeNamespace:crypto%", provider = native, births = [Object]; "%NativeNamespace:crypto.KeyObject%", provider = native, births = [Object]; "%NativeNamespace:dgram%", provider = native, births = [Object]; "%NativeNamespace:dns%", provider = native, births = [Object]; "%NativeNamespace:dns/promises%", provider = native, births = [Object]; "%NativeNamespace:domain%", provider = native, births = [Object]; "%NativeNamespace:events%", provider = native, births = [Object]; "%NativeNamespace:inspector%", provider = native, births = [Object]; "%NativeNamespace:inspector.DOMStorage%", provider = native, births = [Object]; "%NativeNamespace:inspector.NetworkResources%", provider = native, births = [Object]; "%NativeNamespace:inspector/promises%", provider = native, births = [Object]; "%NativeNamespace:module%", provider = native, births = [Object]; "%NativeNamespace:os%", provider = native, births = [Object]; "%NativeNamespace:path%", provider = native, births = [Object]; "%NativeNamespace:path.posix%", provider = native, births = [Object]; "%NativeNamespace:path.win32%", provider = native, births = [Object]; "%NativeNamespace:perf_hooks%", provider = native, births = [Object]; "%NativeNamespace:querystring%", provider = native, births = [Object]; "%NativeNamespace:readline%", provider = native, births = [Object]; "%NativeNamespace:repl%", provider = native, births = [Object]; "%NativeNamespace:sea%", provider = native, births = [Object]; "%NativeNamespace:stream%", provider = native, births = [Object]; "%NativeNamespace:string_decoder%", provider = native, births = [Object]; "%NativeNamespace:timers%", provider = native, births = [Object]; "%NativeNamespace:tls%", provider = native, births = [Object]; "%NativeNamespace:tty%", provider = native, births = [Object]; "%NativeNamespace:url%", provider = native, births = [Object]; "%NativeNamespace:util%", provider = native, births = [Object]; "%NativeNamespace:v8%", provider = native, births = [Object]; "%NativeNamespace:wasi%", provider = native, births = [Object]; "%NativeNamespace:worker_threads%", provider = native, births = [Object]; "%NativeNamespace:zlib%", provider = native, births = [Object]; "node:@lydell/node-pty", provider = native, births = [Array, Error, Function, Object]; "node:@parcel/watcher", provider = native, births = [Array, Error, Function, Object]; "node:@parcel/watcher-darwin-arm64", provider = native, births = [Array, Error, Function, Object]; "node:@parcel/watcher-darwin-x64", provider = native, births = [Array, Error, Function, Object]; "node:@parcel/watcher-linux-arm64-glibc", provider = native, births = [Array, Error, Function, Object]; "node:@parcel/watcher-linux-arm64-musl", provider = native, births = [Array, Error, Function, Object]; "node:@parcel/watcher-linux-x64-glibc", provider = native, births = [Array, Error, Function, Object]; "node:@parcel/watcher-linux-x64-musl", provider = native, births = [Array, Error, Function, Object]; "node:@parcel/watcher-win32-arm64", provider = native, births = [Array, Error, Function, Object]; "node:@parcel/watcher-win32-x64", provider = native, births = [Array, Error, Function, Object]; "node:@perryts/pdf", provider = native, births = [Array, Error, Function, Object]; "node:__disposable__", provider = native, births = [Array, Error, Function, Object]; "node:argon2", provider = native, births = [Array, Error, Function, Object]; "node:bcrypt", provider = native, births = [Array, Error, Function, Object]; "node:better-sqlite3", provider = native, births = [Array, Error, Function, Object]; "node:buffer.Buffer", provider = native, births = [Array, Error, Function, Object]; "node:bun", provider = native, births = [Array, Buffer, Error, Function, Object, Promise]; "node:bun-pty", provider = native, births = [Array, Error, Function, Object]; "node:bun:ffi", provider = native, births = [Array, Error, Function, Object]; "node:bun:jsc", provider = native, births = [Array, Error, Function, Object]; "node:bun:sqlite", provider = native, births = [Array, Error, Function, Object]; "node:cheerio", provider = native, births = [Array, Error, Function, Object]; "node:ethers", provider = native, births = [Array, Error, Function, Object]; "node:ffi", provider = native, births = [Array, Error, Function, Object]; "node:inspector.DOMStorage", provider = native, births = [Array, Error, Function, Object]; "node:inspector.NetworkResources", provider = native, births = [Array, Error, Function, Object]; "node:lodash", provider = native, births = [Array, Error, Function, Object]; "node:node-fetch", provider = native, births = [Array, Error, Function, Headers, Object, Promise, Request, Response, fetch]; "node:node-pty", provider = native, births = [Array, Error, Function, Object]; "node:nodemailer", provider = native, births = [Array, Error, Function, Object]; "node:perry", provider = native, births = [Array, Error, Function, Object]; "node:perry/ads", provider = native, births = [Array, Error, Function, Object]; "node:perry/audio", provider = native, births = [Array, Error, Function, Object]; "node:perry/background", provider = native, births = [Array, Error, Function, Object]; "node:perry/component", provider = native, births = [Array, Error, Function, Object]; "node:perry/compose", provider = native, births = [Array, Error, Function, Object]; "node:perry/container", provider = native, births = [Array, Error, Function, Object]; "node:perry/container-compose", provider = native, births = [Array, Error, Function, Object]; "node:perry/gc", provider = native, births = [Array, Error, Function, Object]; "node:perry/i18n", provider = native, births = [Array, Error, Function, Object]; "node:perry/ios", provider = native, births = [Array, Error, Function, Object]; "node:perry/media", provider = native, births = [Array, Error, Function, Object]; "node:perry/native", provider = native, births = [Array, Error, Function, Object]; "node:perry/plugin", provider = native, births = [Array, Error, Function, Object]; "node:perry/system", provider = native, births = [Array, Error, Function, Object]; "node:perry/thread", provider = native, births = [Array, Error, Function, Object]; "node:perry/tui", provider = native, births = [Array, Error, Function, Object]; "node:perry/ui", provider = native, births = [Array, Error, Function, Object]; "node:perry/updater", provider = native, births = [Array, Error, Function, Object]; "node:perry/widget", provider = native, births = [Array, Error, Function, Object]; "node:perry/workloads", provider = native, births = [Array, Error, Function, Object]; "node:perry/yoga", provider = native, births = [Array, Error, Function, Object]; "node:punycode", provider = native, births = [Array, Error, Function, Object]; "node:sharp", provider = native, births = [Array, Error, Function, Object]; "node:sqlite", provider = native, births = [Array, Error, Function, Object]; "node:typescript", provider = native, births = [Array, Error, Function, Object]; "node:undici", provider = native, births = [Array, ArrayBuffer, Error, Function, Headers, Object, Promise, Request, Response, fetch]; "node:util.types", provider = native, births = [Array, Error, Function, Object]; "node:ws", provider = native, births = [Array, Error, Function, Object]; }
    // Create an object with one field to store the module name
    let obj = crate::realm_decl! { birth js_object_alloc(NATIVE_MODULE_CLASS_ID, 1); realm { "node:async_hooks.default", members = { AsyncLocalStorage = ref("node:async_hooks.AsyncLocalStorage"); createHook = ref("node:async_hooks.createHook"); executionAsyncId = ref("node:async_hooks.executionAsyncId"); triggerAsyncId = ref("node:async_hooks.triggerAsyncId"); executionAsyncResource = ref("node:async_hooks.executionAsyncResource"); asyncWrapProviders = ref("node:async_hooks.asyncWrapProviders"); AsyncResource = ref("node:async_hooks.AsyncResource"); }; "node:buffer.constants", members = { MAX_LENGTH = 9007199254740991.0; MAX_STRING_LENGTH = 536870888.0; }; "node:buffer.default", members = { Buffer = ref(Buffer); transcode = undefined; isUtf8 = ref("node:buffer.isUtf8"); isAscii = ref("node:buffer.isAscii"); kMaxLength = 9007199254740991.0; kStringMaxLength = 536870888.0; btoa = ref("node:buffer.btoa"); atob = ref("node:buffer.atob"); constants; INSPECT_MAX_BYTES = 50.0; Blob = ref(Blob); resolveObjectURL = undefined; File = ref(File); }; "node:buffer.default.constants", members = { MAX_LENGTH = 9007199254740991.0; MAX_STRING_LENGTH = 536870888.0; }; "node:child_process.default", members = { ChildProcess = ref("node:child_process.ChildProcess"); _forkChild = ref("node:child_process._forkChild"); exec = ref("node:child_process.exec"); execFile = ref("node:child_process.execFile"); execFileSync = ref("node:child_process.execFileSync"); execSync = ref("node:child_process.execSync"); fork = ref("node:child_process.fork"); spawn = ref("node:child_process.spawn"); spawnSync = ref("node:child_process.spawnSync"); }; "node:cluster.default", extends = "node:events.EventEmitter.prototype", members = { _events; _eventsCount = 0.0; _maxListeners = undefined; isWorker = false; isMaster = true; isPrimary = true; Worker = ref("node:cluster.Worker"); workers = ref("node:cluster.workers"); settings = ref("node:cluster.settings"); SCHED_NONE = 1.0; SCHED_RR = 2.0; schedulingPolicy = 2.0; setupPrimary = ref("node:cluster.setupMaster"); setupMaster = ref("node:cluster.setupMaster"); fork = ref("node:cluster.fork"); disconnect = ref("node:cluster.disconnect"); }; "node:console.default", members = { log = ref(console.log); info = ref(console.info); debug = ref(console.debug); warn = ref(console.warn); error = ref(console.error); dir = ref(console.dir); time = ref(console.time); timeEnd = ref(console.timeEnd); timeLog = ref(console.timeLog); trace = ref(console.trace); assert = ref(console.assert); clear = ref(console.clear); count = ref(console.count); countReset = ref(console.countReset); group = ref(console.group); groupEnd = ref(console.groupEnd); table = ref(console.table); dirxml = ref(console.dirxml); groupCollapsed = ref(console.groupCollapsed); Console = ref(console.Console); profile = ref(console.profile); profileEnd = ref(console.profileEnd); timeStamp = ref(console.timeStamp); context = ref(console.context); createTask = ref(console.createTask); }; "node:constants.default", members = { F_OK = 0.0; R_OK = 4.0; W_OK = 2.0; X_OK = 1.0; O_RDONLY = 0.0; O_WRONLY = 1.0; O_RDWR = 2.0; O_NOFOLLOW = 131072.0; O_CREAT = 64.0; O_TRUNC = 512.0; O_APPEND = 1024.0; O_EXCL = 128.0; COPYFILE_EXCL = 1.0; COPYFILE_FICLONE = 2.0; COPYFILE_FICLONE_FORCE = 4.0; S_IRUSR = 256.0; S_IWUSR = 128.0; S_IXUSR = 64.0; S_IRGRP = 32.0; S_IWGRP = 16.0; S_IXGRP = 8.0; S_IROTH = 4.0; S_IWOTH = 2.0; S_IXOTH = 1.0; SIGHUP = 1.0; SIGINT = 2.0; SIGQUIT = 3.0; SIGILL = 4.0; SIGTRAP = 5.0; SIGABRT = 6.0; SIGIOT = 6.0; SIGBUS = 7.0; SIGFPE = 8.0; SIGKILL = 9.0; SIGUSR1 = 10.0; SIGSEGV = 11.0; SIGUSR2 = 12.0; SIGPIPE = 13.0; SIGALRM = 14.0; SIGTERM = 15.0; SIGCHLD = 17.0; SIGSTKFLT = 16.0; SIGCONT = 18.0; SIGSTOP = 19.0; SIGTSTP = 20.0; SIGTTIN = 21.0; SIGTTOU = 22.0; SIGURG = 23.0; SIGXCPU = 24.0; SIGXFSZ = 25.0; SIGVTALRM = 26.0; SIGPROF = 27.0; SIGWINCH = 28.0; SIGIO = 29.0; SIGPOLL = 29.0; SIGPWR = 30.0; SIGSYS = 31.0; E2BIG = 7.0; EACCES = 13.0; EADDRINUSE = 98.0; EADDRNOTAVAIL = 99.0; EAFNOSUPPORT = 97.0; EAGAIN = 11.0; EALREADY = 114.0; EBADF = 9.0; EBADMSG = 74.0; EBUSY = 16.0; ECANCELED = 125.0; ECHILD = 10.0; ECONNABORTED = 103.0; ECONNREFUSED = 111.0; ECONNRESET = 104.0; EDEADLK = 35.0; EDESTADDRREQ = 89.0; EDOM = 33.0; EDQUOT = 122.0; EEXIST = 17.0; EFAULT = 14.0; EFBIG = 27.0; EHOSTUNREACH = 113.0; EIDRM = 43.0; EILSEQ = 84.0; EINPROGRESS = 115.0; EINTR = 4.0; EINVAL = 22.0; EIO = 5.0; EISCONN = 106.0; EISDIR = 21.0; ELOOP = 40.0; EMFILE = 24.0; EMLINK = 31.0; EMSGSIZE = 90.0; EMULTIHOP = 72.0; ENAMETOOLONG = 36.0; ENETDOWN = 100.0; ENETRESET = 102.0; ENETUNREACH = 101.0; ENFILE = 23.0; ENOBUFS = 105.0; ENODATA = 61.0; ENODEV = 19.0; ENOENT = 2.0; ENOEXEC = 8.0; ENOLCK = 37.0; ENOLINK = 67.0; ENOMEM = 12.0; ENOMSG = 42.0; ENOPROTOOPT = 92.0; ENOSPC = 28.0; ENOSR = 63.0; ENOSTR = 60.0; ENOSYS = 38.0; ENOTCONN = 107.0; ENOTDIR = 20.0; ENOTEMPTY = 39.0; ENOTSOCK = 88.0; ENOTSUP = 95.0; ENOTTY = 25.0; ENXIO = 6.0; EOPNOTSUPP = 95.0; EOVERFLOW = 75.0; EPERM = 1.0; EPIPE = 32.0; EPROTO = 71.0; EPROTONOSUPPORT = 93.0; EPROTOTYPE = 91.0; ERANGE = 34.0; EROFS = 30.0; ESPIPE = 29.0; ESRCH = 3.0; ESTALE = 116.0; ETIME = 62.0; ETIMEDOUT = 110.0; ETXTBSY = 26.0; EWOULDBLOCK = 11.0; EXDEV = 18.0; PRIORITY_LOW = 19.0; PRIORITY_BELOW_NORMAL = 10.0; PRIORITY_NORMAL = 0.0; PRIORITY_ABOVE_NORMAL = -7.0; PRIORITY_HIGH = -14.0; PRIORITY_HIGHEST = -20.0; RTLD_LAZY = 1.0; RTLD_NOW = 2.0; RTLD_GLOBAL = 256.0; RTLD_LOCAL = 0.0; RTLD_DEEPBIND = 8.0; OPENSSL_VERSION_NUMBER = 811597840.0; SSL_OP_ALL = 2147485776.0; SSL_OP_ALLOW_NO_DHE_KEX = 1024.0; SSL_OP_ALLOW_UNSAFE_LEGACY_RENEGOTIATION = 262144.0; SSL_OP_CIPHER_SERVER_PREFERENCE = 4194304.0; SSL_OP_CISCO_ANYCONNECT = 32768.0; SSL_OP_COOKIE_EXCHANGE = 8192.0; SSL_OP_CRYPTOPRO_TLSEXT_BUG = 2147483648.0; SSL_OP_DONT_INSERT_EMPTY_FRAGMENTS = 2048.0; SSL_OP_LEGACY_SERVER_CONNECT = 4.0; SSL_OP_NO_COMPRESSION = 131072.0; SSL_OP_NO_ENCRYPT_THEN_MAC = 524288.0; SSL_OP_NO_QUERY_MTU = 4096.0; SSL_OP_NO_RENEGOTIATION = 1073741824.0; SSL_OP_NO_SESSION_RESUMPTION_ON_RENEGOTIATION = 65536.0; SSL_OP_NO_SSLv2 = 0.0; SSL_OP_NO_SSLv3 = 33554432.0; SSL_OP_NO_TICKET = 16384.0; SSL_OP_NO_TLSv1 = 67108864.0; SSL_OP_NO_TLSv1_1 = 268435456.0; SSL_OP_NO_TLSv1_2 = 134217728.0; SSL_OP_NO_TLSv1_3 = 536870912.0; SSL_OP_PRIORITIZE_CHACHA = 2097152.0; SSL_OP_TLS_ROLLBACK_BUG = 8388608.0; ENGINE_METHOD_RSA = 1.0; ENGINE_METHOD_DSA = 2.0; ENGINE_METHOD_DH = 4.0; ENGINE_METHOD_RAND = 8.0; ENGINE_METHOD_EC = 2048.0; ENGINE_METHOD_CIPHERS = 64.0; ENGINE_METHOD_DIGESTS = 128.0; ENGINE_METHOD_PKEY_METHS = 512.0; ENGINE_METHOD_PKEY_ASN1_METHS = 1024.0; ENGINE_METHOD_ALL = 65535.0; ENGINE_METHOD_NONE = 0.0; DH_CHECK_P_NOT_SAFE_PRIME = 2.0; DH_CHECK_P_NOT_PRIME = 1.0; DH_UNABLE_TO_CHECK_GENERATOR = 4.0; DH_NOT_SUITABLE_GENERATOR = 8.0; RSA_PKCS1_PADDING = 1.0; RSA_NO_PADDING = 3.0; RSA_PKCS1_OAEP_PADDING = 4.0; RSA_X931_PADDING = 5.0; RSA_PKCS1_PSS_PADDING = 6.0; RSA_PSS_SALTLEN_DIGEST = -1.0; RSA_PSS_SALTLEN_MAX_SIGN = -2.0; RSA_PSS_SALTLEN_AUTO = -2.0; TLS1_VERSION = 769.0; TLS1_1_VERSION = 770.0; TLS1_2_VERSION = 771.0; TLS1_3_VERSION = 772.0; POINT_CONVERSION_COMPRESSED = 2.0; POINT_CONVERSION_UNCOMPRESSED = 4.0; POINT_CONVERSION_HYBRID = 6.0; UV_DIRENT_UNKNOWN = 0.0; UV_DIRENT_FILE = 1.0; UV_DIRENT_DIR = 2.0; UV_DIRENT_LINK = 3.0; UV_DIRENT_FIFO = 4.0; UV_DIRENT_SOCKET = 5.0; UV_DIRENT_CHAR = 6.0; UV_DIRENT_BLOCK = 7.0; UV_FS_SYMLINK_DIR = 1.0; UV_FS_SYMLINK_JUNCTION = 2.0; UV_FS_O_FILEMAP = 0.0; UV_FS_COPYFILE_EXCL = 1.0; UV_FS_COPYFILE_FICLONE = 2.0; UV_FS_COPYFILE_FICLONE_FORCE = 4.0; S_IFMT = 61440.0; S_IFREG = 32768.0; S_IFDIR = 16384.0; S_IFCHR = 8192.0; S_IFBLK = 24576.0; S_IFIFO = 4096.0; S_IFLNK = 40960.0; S_IFSOCK = 49152.0; S_IRWXU = 448.0; S_IRWXG = 56.0; S_IRWXO = 7.0; O_DIRECTORY = 65536.0; O_NOCTTY = 256.0; O_NONBLOCK = 2048.0; O_SYNC = 1052672.0; O_DSYNC = 4096.0; O_DIRECT = 16384.0; O_NOATIME = 262144.0; defaultCoreCipherList = "TLS_AES_256_GCM_SHA384:TLS_CHACHA20_POLY1305_SHA256:TLS_AES_128_GCM_SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-AES256-GCM-SHA384:DHE-RSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-SHA256:DHE-RSA-AES128-SHA256:ECDHE-RSA-AES256-SHA384:DHE-RSA-AES256-SHA384:ECDHE-RSA-AES256-SHA256:DHE-RSA-AES256-SHA256:HIGH:!aNULL:!eNULL:!EXPORT:!DES:!RC4:!MD5:!PSK:!SRP:!CAMELLIA"; }; "node:crypto.Certificate", members = { __module__ = "crypto.Certificate"; }; "node:crypto.constants", members = { __module__ = "crypto.constants"; }; "node:crypto.default", members = { argon2 = ref("node:crypto.argon2"); argon2Sync = ref("node:crypto.argon2Sync"); checkPrime = ref("node:crypto.checkPrime"); checkPrimeSync = ref("node:crypto.checkPrimeSync"); createCipheriv = ref("node:crypto.createCipheriv"); createDecipheriv = ref("node:crypto.createDecipheriv"); createDiffieHellman = ref("node:crypto.createDiffieHellman"); createDiffieHellmanGroup = ref("node:crypto.createDiffieHellmanGroup"); createECDH = ref("node:crypto.createECDH"); createHash = ref("node:crypto.createHash"); createHmac = ref("node:crypto.createHmac"); createPrivateKey = ref("node:crypto.createPrivateKey"); createPublicKey = ref("node:crypto.createPublicKey"); createSecretKey = ref("node:crypto.createSecretKey"); createSign = ref("node:crypto.createSign"); createVerify = ref("node:crypto.createVerify"); diffieHellman = ref("node:crypto.diffieHellman"); generatePrime = ref("node:crypto.generatePrime"); generatePrimeSync = ref("node:crypto.generatePrimeSync"); getCiphers = ref("node:crypto.getCiphers"); getCipherInfo = ref("node:crypto.getCipherInfo"); getCurves = ref("node:crypto.getCurves"); getDiffieHellman = ref("node:crypto.getDiffieHellman"); getHashes = ref("node:crypto.getHashes"); hkdf = ref("node:crypto.hkdf"); hkdfSync = ref("node:crypto.hkdfSync"); pbkdf2 = ref("node:crypto.pbkdf2"); pbkdf2Sync = ref("node:crypto.pbkdf2Sync"); generateKeyPair = ref("node:crypto.generateKeyPair"); generateKeyPairSync = ref("node:crypto.generateKeyPairSync"); generateKey = ref("node:crypto.generateKey"); generateKeySync = ref("node:crypto.generateKeySync"); privateDecrypt = ref("node:crypto.privateDecrypt"); privateEncrypt = ref("node:crypto.privateEncrypt"); publicDecrypt = ref("node:crypto.publicDecrypt"); publicEncrypt = ref("node:crypto.publicEncrypt"); randomBytes = ref("node:crypto.randomBytes"); randomFill = ref("node:crypto.randomFill"); randomFillSync = ref("node:crypto.randomFillSync"); randomInt = ref("node:crypto.randomInt"); randomUUID = ref("node:crypto.randomUUID"); randomUUIDv7; scrypt = ref("node:crypto.scrypt"); scryptSync = ref("node:crypto.scryptSync"); sign = ref("node:crypto.sign"); setEngine = undefined; timingSafeEqual = ref("node:crypto.timingSafeEqual"); getFips = ref("node:crypto.getFips"); setFips = ref("node:crypto.setFips"); verify = ref("node:crypto.verify"); hash = ref("node:crypto.hash"); encapsulate = ref("node:crypto.encapsulate"); decapsulate = ref("node:crypto.decapsulate"); Certificate; Cipheriv = ref("node:crypto.Cipheriv"); Decipheriv = ref("node:crypto.Decipheriv"); DiffieHellman = ref("node:crypto.DiffieHellman"); DiffieHellmanGroup = ref("node:crypto.DiffieHellmanGroup"); ECDH = ref("node:crypto.ECDH"); Hash = ref("node:crypto.Hash"); Hmac = ref("node:crypto.Hmac"); KeyObject = ref("node:crypto.KeyObject"); Sign = ref("node:crypto.Sign"); Verify = ref("node:crypto.Verify"); X509Certificate = ref("node:crypto.X509Certificate"); secureHeapUsed = ref("node:crypto.secureHeapUsed"); constants; webcrypto = ref("node:crypto.webcrypto"); subtle = ref("node:crypto.subtle"); getRandomValues = ref("node:crypto.getRandomValues"); }; "node:crypto.default.Certificate", members = { __module__ = "crypto.Certificate"; }; "node:crypto.default.constants", members = { __module__ = "crypto.constants"; }; "node:crypto.subtle", extends = SubtleCrypto.prototype, members = { __module__ = "crypto.subtle"; }; "node:crypto.webcrypto", extends = Crypto.prototype, members = { __module__ = "crypto.webcrypto"; }; "node:dgram.default", members = { Socket = ref("node:dgram.Socket"); createSocket = ref("node:dgram.createSocket"); }; "node:dns.default", members = { lookup = ref("node:dns.lookup"); lookupService = ref("node:dns.lookupService"); Resolver = ref("node:dns.Resolver"); getDefaultResultOrder = ref("node:dns.getDefaultResultOrder"); setDefaultResultOrder = ref("node:dns.setDefaultResultOrder"); setServers = ref("node:dns.setServers"); ADDRCONFIG = 32.0; ALL = 16.0; V4MAPPED = 8.0; NODATA = "ENODATA"; FORMERR = "EFORMERR"; SERVFAIL = "ESERVFAIL"; NOTFOUND = "ENOTFOUND"; NOTIMP = "ENOTIMP"; REFUSED = "EREFUSED"; BADQUERY = "EBADQUERY"; BADNAME = "EBADNAME"; BADFAMILY = "EBADFAMILY"; BADRESP = "EBADRESP"; CONNREFUSED = "ECONNREFUSED"; TIMEOUT = "ETIMEOUT"; EOF = "EOF"; FILE = "EFILE"; NOMEM = "ENOMEM"; DESTRUCTION = "EDESTRUCTION"; BADSTR = "EBADSTR"; BADFLAGS = "EBADFLAGS"; NONAME = "ENONAME"; BADHINTS = "EBADHINTS"; NOTINITIALIZED = "ENOTINITIALIZED"; LOADIPHLPAPI = "ELOADIPHLPAPI"; ADDRGETNETWORKPARAMS = "EADDRGETNETWORKPARAMS"; CANCELLED = "ECANCELLED"; getServers = ref("node:dns.getServers"); resolve = ref("node:dns.resolve"); resolve4 = ref("node:dns.resolve4"); resolve6 = ref("node:dns.resolve6"); resolveAny = ref("node:dns.resolveAny"); resolveCaa = ref("node:dns.resolveCaa"); resolveCname = ref("node:dns.resolveCname"); resolveMx = ref("node:dns.resolveMx"); resolveNaptr = ref("node:dns.resolveNaptr"); resolveNs = ref("node:dns.resolveNs"); resolvePtr = ref("node:dns.resolvePtr"); resolveSoa = ref("node:dns.resolveSoa"); resolveSrv = ref("node:dns.resolveSrv"); resolveTlsa = ref("node:dns.resolveTlsa"); resolveTxt = ref("node:dns.resolveTxt"); reverse = ref("node:dns.reverse"); promises = ref("node:dns.promises"); }; "node:dns.promises", members = { lookup = ref("node:dns/promises.lookup"); lookupService = ref("node:dns/promises.lookupService"); Resolver = ref("node:dns/promises.Resolver"); getDefaultResultOrder = ref("node:dns/promises.getDefaultResultOrder"); setDefaultResultOrder = ref("node:dns/promises.setDefaultResultOrder"); setServers = ref("node:dns/promises.setServers"); NODATA = "ENODATA"; FORMERR = "EFORMERR"; SERVFAIL = "ESERVFAIL"; NOTFOUND = "ENOTFOUND"; NOTIMP = "ENOTIMP"; REFUSED = "EREFUSED"; BADQUERY = "EBADQUERY"; BADNAME = "EBADNAME"; BADFAMILY = "EBADFAMILY"; BADRESP = "EBADRESP"; CONNREFUSED = "ECONNREFUSED"; TIMEOUT = "ETIMEOUT"; EOF = "EOF"; FILE = "EFILE"; NOMEM = "ENOMEM"; DESTRUCTION = "EDESTRUCTION"; BADSTR = "EBADSTR"; BADFLAGS = "EBADFLAGS"; NONAME = "ENONAME"; BADHINTS = "EBADHINTS"; NOTINITIALIZED = "ENOTINITIALIZED"; LOADIPHLPAPI = "ELOADIPHLPAPI"; ADDRGETNETWORKPARAMS = "EADDRGETNETWORKPARAMS"; CANCELLED = "ECANCELLED"; getServers = ref("node:dns/promises.getServers"); resolve = ref("node:dns/promises.resolve"); resolve4 = ref("node:dns/promises.resolve4"); resolve6 = ref("node:dns/promises.resolve6"); resolveAny = ref("node:dns/promises.resolveAny"); resolveCaa = ref("node:dns/promises.resolveCaa"); resolveCname = ref("node:dns/promises.resolveCname"); resolveMx = ref("node:dns/promises.resolveMx"); resolveNaptr = ref("node:dns/promises.resolveNaptr"); resolveNs = ref("node:dns/promises.resolveNs"); resolvePtr = ref("node:dns/promises.resolvePtr"); resolveSoa = ref("node:dns/promises.resolveSoa"); resolveSrv = ref("node:dns/promises.resolveSrv"); resolveTlsa = ref("node:dns/promises.resolveTlsa"); resolveTxt = ref("node:dns/promises.resolveTxt"); reverse = ref("node:dns/promises.reverse"); }; "node:domain.default", members = { _stack; Domain = ref("node:domain.Domain"); createDomain = ref("node:domain.create"); create = ref("node:domain.create"); active = null; }; "node:fs.constants", members = { __module__ = "fs.constants"; }; "node:http2.constants", members = { NGHTTP2_ERR_FRAME_SIZE_ERROR = -522.0; NGHTTP2_SESSION_SERVER = 0.0; NGHTTP2_SESSION_CLIENT = 1.0; NGHTTP2_STREAM_STATE_IDLE = 1.0; NGHTTP2_STREAM_STATE_OPEN = 2.0; NGHTTP2_STREAM_STATE_RESERVED_LOCAL = 3.0; NGHTTP2_STREAM_STATE_RESERVED_REMOTE = 4.0; NGHTTP2_STREAM_STATE_HALF_CLOSED_LOCAL = 5.0; NGHTTP2_STREAM_STATE_HALF_CLOSED_REMOTE = 6.0; NGHTTP2_STREAM_STATE_CLOSED = 7.0; NGHTTP2_FLAG_NONE = 0.0; NGHTTP2_FLAG_END_STREAM = 1.0; NGHTTP2_FLAG_END_HEADERS = 4.0; NGHTTP2_FLAG_ACK = 1.0; NGHTTP2_FLAG_PADDED = 8.0; NGHTTP2_FLAG_PRIORITY = 32.0; DEFAULT_SETTINGS_HEADER_TABLE_SIZE = 4096.0; DEFAULT_SETTINGS_ENABLE_PUSH = 1.0; DEFAULT_SETTINGS_MAX_CONCURRENT_STREAMS = 4294967295.0; DEFAULT_SETTINGS_INITIAL_WINDOW_SIZE = 65535.0; DEFAULT_SETTINGS_MAX_FRAME_SIZE = 16384.0; DEFAULT_SETTINGS_MAX_HEADER_LIST_SIZE = 65535.0; DEFAULT_SETTINGS_ENABLE_CONNECT_PROTOCOL = 0.0; MAX_MAX_FRAME_SIZE = 16777215.0; MIN_MAX_FRAME_SIZE = 16384.0; MAX_INITIAL_WINDOW_SIZE = 2147483647.0; NGHTTP2_SETTINGS_HEADER_TABLE_SIZE = 1.0; NGHTTP2_SETTINGS_ENABLE_PUSH = 2.0; NGHTTP2_SETTINGS_MAX_CONCURRENT_STREAMS = 3.0; NGHTTP2_SETTINGS_INITIAL_WINDOW_SIZE = 4.0; NGHTTP2_SETTINGS_MAX_FRAME_SIZE = 5.0; NGHTTP2_SETTINGS_MAX_HEADER_LIST_SIZE = 6.0; NGHTTP2_SETTINGS_ENABLE_CONNECT_PROTOCOL = 8.0; PADDING_STRATEGY_NONE = 0.0; PADDING_STRATEGY_ALIGNED = 1.0; PADDING_STRATEGY_MAX = 2.0; PADDING_STRATEGY_CALLBACK = 1.0; NGHTTP2_NO_ERROR = 0.0; NGHTTP2_PROTOCOL_ERROR = 1.0; NGHTTP2_INTERNAL_ERROR = 2.0; NGHTTP2_FLOW_CONTROL_ERROR = 3.0; NGHTTP2_SETTINGS_TIMEOUT = 4.0; NGHTTP2_STREAM_CLOSED = 5.0; NGHTTP2_FRAME_SIZE_ERROR = 6.0; NGHTTP2_REFUSED_STREAM = 7.0; NGHTTP2_CANCEL = 8.0; NGHTTP2_COMPRESSION_ERROR = 9.0; NGHTTP2_CONNECT_ERROR = 10.0; NGHTTP2_ENHANCE_YOUR_CALM = 11.0; NGHTTP2_INADEQUATE_SECURITY = 12.0; NGHTTP2_HTTP_1_1_REQUIRED = 13.0; NGHTTP2_DEFAULT_WEIGHT = 16.0; HTTP2_HEADER_STATUS = ":status"; HTTP2_HEADER_METHOD = ":method"; HTTP2_HEADER_AUTHORITY = ":authority"; HTTP2_HEADER_SCHEME = ":scheme"; HTTP2_HEADER_PATH = ":path"; HTTP2_HEADER_PROTOCOL = ":protocol"; HTTP2_HEADER_ACCEPT_ENCODING = "accept-encoding"; HTTP2_HEADER_ACCEPT_LANGUAGE = "accept-language"; HTTP2_HEADER_ACCEPT_RANGES = "accept-ranges"; HTTP2_HEADER_ACCEPT = "accept"; HTTP2_HEADER_ACCESS_CONTROL_ALLOW_CREDENTIALS = "access-control-allow-credentials"; HTTP2_HEADER_ACCESS_CONTROL_ALLOW_HEADERS = "access-control-allow-headers"; HTTP2_HEADER_ACCESS_CONTROL_ALLOW_METHODS = "access-control-allow-methods"; HTTP2_HEADER_ACCESS_CONTROL_ALLOW_ORIGIN = "access-control-allow-origin"; HTTP2_HEADER_ACCESS_CONTROL_EXPOSE_HEADERS = "access-control-expose-headers"; HTTP2_HEADER_ACCESS_CONTROL_REQUEST_HEADERS = "access-control-request-headers"; HTTP2_HEADER_ACCESS_CONTROL_REQUEST_METHOD = "access-control-request-method"; HTTP2_HEADER_AGE = "age"; HTTP2_HEADER_AUTHORIZATION = "authorization"; HTTP2_HEADER_CACHE_CONTROL = "cache-control"; HTTP2_HEADER_CONNECTION = "connection"; HTTP2_HEADER_CONTENT_DISPOSITION = "content-disposition"; HTTP2_HEADER_CONTENT_ENCODING = "content-encoding"; HTTP2_HEADER_CONTENT_LENGTH = "content-length"; HTTP2_HEADER_CONTENT_TYPE = "content-type"; HTTP2_HEADER_COOKIE = "cookie"; HTTP2_HEADER_DATE = "date"; HTTP2_HEADER_ETAG = "etag"; HTTP2_HEADER_FORWARDED = "forwarded"; HTTP2_HEADER_HOST = "host"; HTTP2_HEADER_IF_MODIFIED_SINCE = "if-modified-since"; HTTP2_HEADER_IF_NONE_MATCH = "if-none-match"; HTTP2_HEADER_IF_RANGE = "if-range"; HTTP2_HEADER_LAST_MODIFIED = "last-modified"; HTTP2_HEADER_LINK = "link"; HTTP2_HEADER_LOCATION = "location"; HTTP2_HEADER_RANGE = "range"; HTTP2_HEADER_REFERER = "referer"; HTTP2_HEADER_SERVER = "server"; HTTP2_HEADER_SET_COOKIE = "set-cookie"; HTTP2_HEADER_STRICT_TRANSPORT_SECURITY = "strict-transport-security"; HTTP2_HEADER_TRANSFER_ENCODING = "transfer-encoding"; HTTP2_HEADER_TE = "te"; HTTP2_HEADER_UPGRADE_INSECURE_REQUESTS = "upgrade-insecure-requests"; HTTP2_HEADER_UPGRADE = "upgrade"; HTTP2_HEADER_USER_AGENT = "user-agent"; HTTP2_HEADER_VARY = "vary"; HTTP2_HEADER_X_CONTENT_TYPE_OPTIONS = "x-content-type-options"; HTTP2_HEADER_X_FRAME_OPTIONS = "x-frame-options"; HTTP2_HEADER_KEEP_ALIVE = "keep-alive"; HTTP2_HEADER_PROXY_CONNECTION = "proxy-connection"; HTTP2_HEADER_X_XSS_PROTECTION = "x-xss-protection"; HTTP2_HEADER_ALT_SVC = "alt-svc"; HTTP2_HEADER_CONTENT_SECURITY_POLICY = "content-security-policy"; HTTP2_HEADER_EARLY_DATA = "early-data"; HTTP2_HEADER_EXPECT_CT = "expect-ct"; HTTP2_HEADER_ORIGIN = "origin"; HTTP2_HEADER_PURPOSE = "purpose"; HTTP2_HEADER_TIMING_ALLOW_ORIGIN = "timing-allow-origin"; HTTP2_HEADER_X_FORWARDED_FOR = "x-forwarded-for"; HTTP2_HEADER_PRIORITY = "priority"; HTTP2_HEADER_ACCEPT_CHARSET = "accept-charset"; HTTP2_HEADER_ACCESS_CONTROL_MAX_AGE = "access-control-max-age"; HTTP2_HEADER_ALLOW = "allow"; HTTP2_HEADER_CONTENT_LANGUAGE = "content-language"; HTTP2_HEADER_CONTENT_LOCATION = "content-location"; HTTP2_HEADER_CONTENT_MD5 = "content-md5"; HTTP2_HEADER_CONTENT_RANGE = "content-range"; HTTP2_HEADER_DNT = "dnt"; HTTP2_HEADER_EXPECT = "expect"; HTTP2_HEADER_EXPIRES = "expires"; HTTP2_HEADER_FROM = "from"; HTTP2_HEADER_IF_MATCH = "if-match"; HTTP2_HEADER_IF_UNMODIFIED_SINCE = "if-unmodified-since"; HTTP2_HEADER_MAX_FORWARDS = "max-forwards"; HTTP2_HEADER_PREFER = "prefer"; HTTP2_HEADER_PROXY_AUTHENTICATE = "proxy-authenticate"; HTTP2_HEADER_PROXY_AUTHORIZATION = "proxy-authorization"; HTTP2_HEADER_REFRESH = "refresh"; HTTP2_HEADER_RETRY_AFTER = "retry-after"; HTTP2_HEADER_TRAILER = "trailer"; HTTP2_HEADER_TK = "tk"; HTTP2_HEADER_VIA = "via"; HTTP2_HEADER_WARNING = "warning"; HTTP2_HEADER_WWW_AUTHENTICATE = "www-authenticate"; HTTP2_HEADER_HTTP2_SETTINGS = "http2-settings"; HTTP2_METHOD_ACL = "ACL"; HTTP2_METHOD_BASELINE_CONTROL = "BASELINE-CONTROL"; HTTP2_METHOD_BIND = "BIND"; HTTP2_METHOD_CHECKIN = "CHECKIN"; HTTP2_METHOD_CHECKOUT = "CHECKOUT"; HTTP2_METHOD_CONNECT = "CONNECT"; HTTP2_METHOD_COPY = "COPY"; HTTP2_METHOD_DELETE = "DELETE"; HTTP2_METHOD_GET = "GET"; HTTP2_METHOD_HEAD = "HEAD"; HTTP2_METHOD_LABEL = "LABEL"; HTTP2_METHOD_LINK = "LINK"; HTTP2_METHOD_LOCK = "LOCK"; HTTP2_METHOD_MERGE = "MERGE"; HTTP2_METHOD_MKACTIVITY = "MKACTIVITY"; HTTP2_METHOD_MKCALENDAR = "MKCALENDAR"; HTTP2_METHOD_MKCOL = "MKCOL"; HTTP2_METHOD_MKREDIRECTREF = "MKREDIRECTREF"; HTTP2_METHOD_MKWORKSPACE = "MKWORKSPACE"; HTTP2_METHOD_MOVE = "MOVE"; HTTP2_METHOD_OPTIONS = "OPTIONS"; HTTP2_METHOD_ORDERPATCH = "ORDERPATCH"; HTTP2_METHOD_PATCH = "PATCH"; HTTP2_METHOD_POST = "POST"; HTTP2_METHOD_PRI = "PRI"; HTTP2_METHOD_PROPFIND = "PROPFIND"; HTTP2_METHOD_PROPPATCH = "PROPPATCH"; HTTP2_METHOD_PUT = "PUT"; HTTP2_METHOD_REBIND = "REBIND"; HTTP2_METHOD_REPORT = "REPORT"; HTTP2_METHOD_SEARCH = "SEARCH"; HTTP2_METHOD_TRACE = "TRACE"; HTTP2_METHOD_UNBIND = "UNBIND"; HTTP2_METHOD_UNCHECKOUT = "UNCHECKOUT"; HTTP2_METHOD_UNLINK = "UNLINK"; HTTP2_METHOD_UNLOCK = "UNLOCK"; HTTP2_METHOD_UPDATE = "UPDATE"; HTTP2_METHOD_UPDATEREDIRECTREF = "UPDATEREDIRECTREF"; HTTP2_METHOD_VERSION_CONTROL = "VERSION-CONTROL"; HTTP_STATUS_CONTINUE = 100.0; HTTP_STATUS_SWITCHING_PROTOCOLS = 101.0; HTTP_STATUS_PROCESSING = 102.0; HTTP_STATUS_EARLY_HINTS = 103.0; HTTP_STATUS_OK = 200.0; HTTP_STATUS_CREATED = 201.0; HTTP_STATUS_ACCEPTED = 202.0; HTTP_STATUS_NON_AUTHORITATIVE_INFORMATION = 203.0; HTTP_STATUS_NO_CONTENT = 204.0; HTTP_STATUS_RESET_CONTENT = 205.0; HTTP_STATUS_PARTIAL_CONTENT = 206.0; HTTP_STATUS_MULTI_STATUS = 207.0; HTTP_STATUS_ALREADY_REPORTED = 208.0; HTTP_STATUS_IM_USED = 226.0; HTTP_STATUS_MULTIPLE_CHOICES = 300.0; HTTP_STATUS_MOVED_PERMANENTLY = 301.0; HTTP_STATUS_FOUND = 302.0; HTTP_STATUS_SEE_OTHER = 303.0; HTTP_STATUS_NOT_MODIFIED = 304.0; HTTP_STATUS_USE_PROXY = 305.0; HTTP_STATUS_TEMPORARY_REDIRECT = 307.0; HTTP_STATUS_PERMANENT_REDIRECT = 308.0; HTTP_STATUS_BAD_REQUEST = 400.0; HTTP_STATUS_UNAUTHORIZED = 401.0; HTTP_STATUS_PAYMENT_REQUIRED = 402.0; HTTP_STATUS_FORBIDDEN = 403.0; HTTP_STATUS_NOT_FOUND = 404.0; HTTP_STATUS_METHOD_NOT_ALLOWED = 405.0; HTTP_STATUS_NOT_ACCEPTABLE = 406.0; HTTP_STATUS_PROXY_AUTHENTICATION_REQUIRED = 407.0; HTTP_STATUS_REQUEST_TIMEOUT = 408.0; HTTP_STATUS_CONFLICT = 409.0; HTTP_STATUS_GONE = 410.0; HTTP_STATUS_LENGTH_REQUIRED = 411.0; HTTP_STATUS_PRECONDITION_FAILED = 412.0; HTTP_STATUS_PAYLOAD_TOO_LARGE = 413.0; HTTP_STATUS_URI_TOO_LONG = 414.0; HTTP_STATUS_UNSUPPORTED_MEDIA_TYPE = 415.0; HTTP_STATUS_RANGE_NOT_SATISFIABLE = 416.0; HTTP_STATUS_EXPECTATION_FAILED = 417.0; HTTP_STATUS_TEAPOT = 418.0; HTTP_STATUS_MISDIRECTED_REQUEST = 421.0; HTTP_STATUS_UNPROCESSABLE_ENTITY = 422.0; HTTP_STATUS_LOCKED = 423.0; HTTP_STATUS_FAILED_DEPENDENCY = 424.0; HTTP_STATUS_TOO_EARLY = 425.0; HTTP_STATUS_UPGRADE_REQUIRED = 426.0; HTTP_STATUS_PRECONDITION_REQUIRED = 428.0; HTTP_STATUS_TOO_MANY_REQUESTS = 429.0; HTTP_STATUS_REQUEST_HEADER_FIELDS_TOO_LARGE = 431.0; HTTP_STATUS_UNAVAILABLE_FOR_LEGAL_REASONS = 451.0; HTTP_STATUS_INTERNAL_SERVER_ERROR = 500.0; HTTP_STATUS_NOT_IMPLEMENTED = 501.0; HTTP_STATUS_BAD_GATEWAY = 502.0; HTTP_STATUS_SERVICE_UNAVAILABLE = 503.0; HTTP_STATUS_GATEWAY_TIMEOUT = 504.0; HTTP_STATUS_HTTP_VERSION_NOT_SUPPORTED = 505.0; HTTP_STATUS_VARIANT_ALSO_NEGOTIATES = 506.0; HTTP_STATUS_INSUFFICIENT_STORAGE = 507.0; HTTP_STATUS_LOOP_DETECTED = 508.0; HTTP_STATUS_BANDWIDTH_LIMIT_EXCEEDED = 509.0; HTTP_STATUS_NOT_EXTENDED = 510.0; HTTP_STATUS_NETWORK_AUTHENTICATION_REQUIRED = 511.0; }; "node:http2.default.constants", members = { NGHTTP2_ERR_FRAME_SIZE_ERROR = -522.0; NGHTTP2_SESSION_SERVER = 0.0; NGHTTP2_SESSION_CLIENT = 1.0; NGHTTP2_STREAM_STATE_IDLE = 1.0; NGHTTP2_STREAM_STATE_OPEN = 2.0; NGHTTP2_STREAM_STATE_RESERVED_LOCAL = 3.0; NGHTTP2_STREAM_STATE_RESERVED_REMOTE = 4.0; NGHTTP2_STREAM_STATE_HALF_CLOSED_LOCAL = 5.0; NGHTTP2_STREAM_STATE_HALF_CLOSED_REMOTE = 6.0; NGHTTP2_STREAM_STATE_CLOSED = 7.0; NGHTTP2_FLAG_NONE = 0.0; NGHTTP2_FLAG_END_STREAM = 1.0; NGHTTP2_FLAG_END_HEADERS = 4.0; NGHTTP2_FLAG_ACK = 1.0; NGHTTP2_FLAG_PADDED = 8.0; NGHTTP2_FLAG_PRIORITY = 32.0; DEFAULT_SETTINGS_HEADER_TABLE_SIZE = 4096.0; DEFAULT_SETTINGS_ENABLE_PUSH = 1.0; DEFAULT_SETTINGS_MAX_CONCURRENT_STREAMS = 4294967295.0; DEFAULT_SETTINGS_INITIAL_WINDOW_SIZE = 65535.0; DEFAULT_SETTINGS_MAX_FRAME_SIZE = 16384.0; DEFAULT_SETTINGS_MAX_HEADER_LIST_SIZE = 65535.0; DEFAULT_SETTINGS_ENABLE_CONNECT_PROTOCOL = 0.0; MAX_MAX_FRAME_SIZE = 16777215.0; MIN_MAX_FRAME_SIZE = 16384.0; MAX_INITIAL_WINDOW_SIZE = 2147483647.0; NGHTTP2_SETTINGS_HEADER_TABLE_SIZE = 1.0; NGHTTP2_SETTINGS_ENABLE_PUSH = 2.0; NGHTTP2_SETTINGS_MAX_CONCURRENT_STREAMS = 3.0; NGHTTP2_SETTINGS_INITIAL_WINDOW_SIZE = 4.0; NGHTTP2_SETTINGS_MAX_FRAME_SIZE = 5.0; NGHTTP2_SETTINGS_MAX_HEADER_LIST_SIZE = 6.0; NGHTTP2_SETTINGS_ENABLE_CONNECT_PROTOCOL = 8.0; PADDING_STRATEGY_NONE = 0.0; PADDING_STRATEGY_ALIGNED = 1.0; PADDING_STRATEGY_MAX = 2.0; PADDING_STRATEGY_CALLBACK = 1.0; NGHTTP2_NO_ERROR = 0.0; NGHTTP2_PROTOCOL_ERROR = 1.0; NGHTTP2_INTERNAL_ERROR = 2.0; NGHTTP2_FLOW_CONTROL_ERROR = 3.0; NGHTTP2_SETTINGS_TIMEOUT = 4.0; NGHTTP2_STREAM_CLOSED = 5.0; NGHTTP2_FRAME_SIZE_ERROR = 6.0; NGHTTP2_REFUSED_STREAM = 7.0; NGHTTP2_CANCEL = 8.0; NGHTTP2_COMPRESSION_ERROR = 9.0; NGHTTP2_CONNECT_ERROR = 10.0; NGHTTP2_ENHANCE_YOUR_CALM = 11.0; NGHTTP2_INADEQUATE_SECURITY = 12.0; NGHTTP2_HTTP_1_1_REQUIRED = 13.0; NGHTTP2_DEFAULT_WEIGHT = 16.0; HTTP2_HEADER_STATUS = ":status"; HTTP2_HEADER_METHOD = ":method"; HTTP2_HEADER_AUTHORITY = ":authority"; HTTP2_HEADER_SCHEME = ":scheme"; HTTP2_HEADER_PATH = ":path"; HTTP2_HEADER_PROTOCOL = ":protocol"; HTTP2_HEADER_ACCEPT_ENCODING = "accept-encoding"; HTTP2_HEADER_ACCEPT_LANGUAGE = "accept-language"; HTTP2_HEADER_ACCEPT_RANGES = "accept-ranges"; HTTP2_HEADER_ACCEPT = "accept"; HTTP2_HEADER_ACCESS_CONTROL_ALLOW_CREDENTIALS = "access-control-allow-credentials"; HTTP2_HEADER_ACCESS_CONTROL_ALLOW_HEADERS = "access-control-allow-headers"; HTTP2_HEADER_ACCESS_CONTROL_ALLOW_METHODS = "access-control-allow-methods"; HTTP2_HEADER_ACCESS_CONTROL_ALLOW_ORIGIN = "access-control-allow-origin"; HTTP2_HEADER_ACCESS_CONTROL_EXPOSE_HEADERS = "access-control-expose-headers"; HTTP2_HEADER_ACCESS_CONTROL_REQUEST_HEADERS = "access-control-request-headers"; HTTP2_HEADER_ACCESS_CONTROL_REQUEST_METHOD = "access-control-request-method"; HTTP2_HEADER_AGE = "age"; HTTP2_HEADER_AUTHORIZATION = "authorization"; HTTP2_HEADER_CACHE_CONTROL = "cache-control"; HTTP2_HEADER_CONNECTION = "connection"; HTTP2_HEADER_CONTENT_DISPOSITION = "content-disposition"; HTTP2_HEADER_CONTENT_ENCODING = "content-encoding"; HTTP2_HEADER_CONTENT_LENGTH = "content-length"; HTTP2_HEADER_CONTENT_TYPE = "content-type"; HTTP2_HEADER_COOKIE = "cookie"; HTTP2_HEADER_DATE = "date"; HTTP2_HEADER_ETAG = "etag"; HTTP2_HEADER_FORWARDED = "forwarded"; HTTP2_HEADER_HOST = "host"; HTTP2_HEADER_IF_MODIFIED_SINCE = "if-modified-since"; HTTP2_HEADER_IF_NONE_MATCH = "if-none-match"; HTTP2_HEADER_IF_RANGE = "if-range"; HTTP2_HEADER_LAST_MODIFIED = "last-modified"; HTTP2_HEADER_LINK = "link"; HTTP2_HEADER_LOCATION = "location"; HTTP2_HEADER_RANGE = "range"; HTTP2_HEADER_REFERER = "referer"; HTTP2_HEADER_SERVER = "server"; HTTP2_HEADER_SET_COOKIE = "set-cookie"; HTTP2_HEADER_STRICT_TRANSPORT_SECURITY = "strict-transport-security"; HTTP2_HEADER_TRANSFER_ENCODING = "transfer-encoding"; HTTP2_HEADER_TE = "te"; HTTP2_HEADER_UPGRADE_INSECURE_REQUESTS = "upgrade-insecure-requests"; HTTP2_HEADER_UPGRADE = "upgrade"; HTTP2_HEADER_USER_AGENT = "user-agent"; HTTP2_HEADER_VARY = "vary"; HTTP2_HEADER_X_CONTENT_TYPE_OPTIONS = "x-content-type-options"; HTTP2_HEADER_X_FRAME_OPTIONS = "x-frame-options"; HTTP2_HEADER_KEEP_ALIVE = "keep-alive"; HTTP2_HEADER_PROXY_CONNECTION = "proxy-connection"; HTTP2_HEADER_X_XSS_PROTECTION = "x-xss-protection"; HTTP2_HEADER_ALT_SVC = "alt-svc"; HTTP2_HEADER_CONTENT_SECURITY_POLICY = "content-security-policy"; HTTP2_HEADER_EARLY_DATA = "early-data"; HTTP2_HEADER_EXPECT_CT = "expect-ct"; HTTP2_HEADER_ORIGIN = "origin"; HTTP2_HEADER_PURPOSE = "purpose"; HTTP2_HEADER_TIMING_ALLOW_ORIGIN = "timing-allow-origin"; HTTP2_HEADER_X_FORWARDED_FOR = "x-forwarded-for"; HTTP2_HEADER_PRIORITY = "priority"; HTTP2_HEADER_ACCEPT_CHARSET = "accept-charset"; HTTP2_HEADER_ACCESS_CONTROL_MAX_AGE = "access-control-max-age"; HTTP2_HEADER_ALLOW = "allow"; HTTP2_HEADER_CONTENT_LANGUAGE = "content-language"; HTTP2_HEADER_CONTENT_LOCATION = "content-location"; HTTP2_HEADER_CONTENT_MD5 = "content-md5"; HTTP2_HEADER_CONTENT_RANGE = "content-range"; HTTP2_HEADER_DNT = "dnt"; HTTP2_HEADER_EXPECT = "expect"; HTTP2_HEADER_EXPIRES = "expires"; HTTP2_HEADER_FROM = "from"; HTTP2_HEADER_IF_MATCH = "if-match"; HTTP2_HEADER_IF_UNMODIFIED_SINCE = "if-unmodified-since"; HTTP2_HEADER_MAX_FORWARDS = "max-forwards"; HTTP2_HEADER_PREFER = "prefer"; HTTP2_HEADER_PROXY_AUTHENTICATE = "proxy-authenticate"; HTTP2_HEADER_PROXY_AUTHORIZATION = "proxy-authorization"; HTTP2_HEADER_REFRESH = "refresh"; HTTP2_HEADER_RETRY_AFTER = "retry-after"; HTTP2_HEADER_TRAILER = "trailer"; HTTP2_HEADER_TK = "tk"; HTTP2_HEADER_VIA = "via"; HTTP2_HEADER_WARNING = "warning"; HTTP2_HEADER_WWW_AUTHENTICATE = "www-authenticate"; HTTP2_HEADER_HTTP2_SETTINGS = "http2-settings"; HTTP2_METHOD_ACL = "ACL"; HTTP2_METHOD_BASELINE_CONTROL = "BASELINE-CONTROL"; HTTP2_METHOD_BIND = "BIND"; HTTP2_METHOD_CHECKIN = "CHECKIN"; HTTP2_METHOD_CHECKOUT = "CHECKOUT"; HTTP2_METHOD_CONNECT = "CONNECT"; HTTP2_METHOD_COPY = "COPY"; HTTP2_METHOD_DELETE = "DELETE"; HTTP2_METHOD_GET = "GET"; HTTP2_METHOD_HEAD = "HEAD"; HTTP2_METHOD_LABEL = "LABEL"; HTTP2_METHOD_LINK = "LINK"; HTTP2_METHOD_LOCK = "LOCK"; HTTP2_METHOD_MERGE = "MERGE"; HTTP2_METHOD_MKACTIVITY = "MKACTIVITY"; HTTP2_METHOD_MKCALENDAR = "MKCALENDAR"; HTTP2_METHOD_MKCOL = "MKCOL"; HTTP2_METHOD_MKREDIRECTREF = "MKREDIRECTREF"; HTTP2_METHOD_MKWORKSPACE = "MKWORKSPACE"; HTTP2_METHOD_MOVE = "MOVE"; HTTP2_METHOD_OPTIONS = "OPTIONS"; HTTP2_METHOD_ORDERPATCH = "ORDERPATCH"; HTTP2_METHOD_PATCH = "PATCH"; HTTP2_METHOD_POST = "POST"; HTTP2_METHOD_PRI = "PRI"; HTTP2_METHOD_PROPFIND = "PROPFIND"; HTTP2_METHOD_PROPPATCH = "PROPPATCH"; HTTP2_METHOD_PUT = "PUT"; HTTP2_METHOD_REBIND = "REBIND"; HTTP2_METHOD_REPORT = "REPORT"; HTTP2_METHOD_SEARCH = "SEARCH"; HTTP2_METHOD_TRACE = "TRACE"; HTTP2_METHOD_UNBIND = "UNBIND"; HTTP2_METHOD_UNCHECKOUT = "UNCHECKOUT"; HTTP2_METHOD_UNLINK = "UNLINK"; HTTP2_METHOD_UNLOCK = "UNLOCK"; HTTP2_METHOD_UPDATE = "UPDATE"; HTTP2_METHOD_UPDATEREDIRECTREF = "UPDATEREDIRECTREF"; HTTP2_METHOD_VERSION_CONTROL = "VERSION-CONTROL"; HTTP_STATUS_CONTINUE = 100.0; HTTP_STATUS_SWITCHING_PROTOCOLS = 101.0; HTTP_STATUS_PROCESSING = 102.0; HTTP_STATUS_EARLY_HINTS = 103.0; HTTP_STATUS_OK = 200.0; HTTP_STATUS_CREATED = 201.0; HTTP_STATUS_ACCEPTED = 202.0; HTTP_STATUS_NON_AUTHORITATIVE_INFORMATION = 203.0; HTTP_STATUS_NO_CONTENT = 204.0; HTTP_STATUS_RESET_CONTENT = 205.0; HTTP_STATUS_PARTIAL_CONTENT = 206.0; HTTP_STATUS_MULTI_STATUS = 207.0; HTTP_STATUS_ALREADY_REPORTED = 208.0; HTTP_STATUS_IM_USED = 226.0; HTTP_STATUS_MULTIPLE_CHOICES = 300.0; HTTP_STATUS_MOVED_PERMANENTLY = 301.0; HTTP_STATUS_FOUND = 302.0; HTTP_STATUS_SEE_OTHER = 303.0; HTTP_STATUS_NOT_MODIFIED = 304.0; HTTP_STATUS_USE_PROXY = 305.0; HTTP_STATUS_TEMPORARY_REDIRECT = 307.0; HTTP_STATUS_PERMANENT_REDIRECT = 308.0; HTTP_STATUS_BAD_REQUEST = 400.0; HTTP_STATUS_UNAUTHORIZED = 401.0; HTTP_STATUS_PAYMENT_REQUIRED = 402.0; HTTP_STATUS_FORBIDDEN = 403.0; HTTP_STATUS_NOT_FOUND = 404.0; HTTP_STATUS_METHOD_NOT_ALLOWED = 405.0; HTTP_STATUS_NOT_ACCEPTABLE = 406.0; HTTP_STATUS_PROXY_AUTHENTICATION_REQUIRED = 407.0; HTTP_STATUS_REQUEST_TIMEOUT = 408.0; HTTP_STATUS_CONFLICT = 409.0; HTTP_STATUS_GONE = 410.0; HTTP_STATUS_LENGTH_REQUIRED = 411.0; HTTP_STATUS_PRECONDITION_FAILED = 412.0; HTTP_STATUS_PAYLOAD_TOO_LARGE = 413.0; HTTP_STATUS_URI_TOO_LONG = 414.0; HTTP_STATUS_UNSUPPORTED_MEDIA_TYPE = 415.0; HTTP_STATUS_RANGE_NOT_SATISFIABLE = 416.0; HTTP_STATUS_EXPECTATION_FAILED = 417.0; HTTP_STATUS_TEAPOT = 418.0; HTTP_STATUS_MISDIRECTED_REQUEST = 421.0; HTTP_STATUS_UNPROCESSABLE_ENTITY = 422.0; HTTP_STATUS_LOCKED = 423.0; HTTP_STATUS_FAILED_DEPENDENCY = 424.0; HTTP_STATUS_TOO_EARLY = 425.0; HTTP_STATUS_UPGRADE_REQUIRED = 426.0; HTTP_STATUS_PRECONDITION_REQUIRED = 428.0; HTTP_STATUS_TOO_MANY_REQUESTS = 429.0; HTTP_STATUS_REQUEST_HEADER_FIELDS_TOO_LARGE = 431.0; HTTP_STATUS_UNAVAILABLE_FOR_LEGAL_REASONS = 451.0; HTTP_STATUS_INTERNAL_SERVER_ERROR = 500.0; HTTP_STATUS_NOT_IMPLEMENTED = 501.0; HTTP_STATUS_BAD_GATEWAY = 502.0; HTTP_STATUS_SERVICE_UNAVAILABLE = 503.0; HTTP_STATUS_GATEWAY_TIMEOUT = 504.0; HTTP_STATUS_HTTP_VERSION_NOT_SUPPORTED = 505.0; HTTP_STATUS_VARIANT_ALSO_NEGOTIATES = 506.0; HTTP_STATUS_INSUFFICIENT_STORAGE = 507.0; HTTP_STATUS_LOOP_DETECTED = 508.0; HTTP_STATUS_BANDWIDTH_LIMIT_EXCEEDED = 509.0; HTTP_STATUS_NOT_EXTENDED = 510.0; HTTP_STATUS_NETWORK_AUTHENTICATION_REQUIRED = 511.0; }; "node:inspector.default", members = { open = ref("node:inspector.open"); close = ref("node:inspector.close"); url = ref("node:inspector.url"); waitForDebugger = ref("node:inspector.waitForDebugger"); console = ref("node:inspector.console"); Session = ref("node:inspector.Session"); Network = ref("node:inspector.Network"); NetworkResources; DOMStorage; }; "node:inspector.default.DOMStorage", members = { domStorageItemAdded; domStorageItemRemoved; domStorageItemUpdated; domStorageItemsCleared; registerStorage; }; "node:inspector.default.NetworkResources", members = { put; }; "node:inspector/promises.default", members = { open = ref("node:inspector.open"); close = ref("node:inspector.close"); url = ref("node:inspector.url"); waitForDebugger = ref("node:inspector.waitForDebugger"); console = ref("node:inspector.console"); Session = ref("node:inspector/promises.Session"); Network = ref("node:inspector.Network"); NetworkResources = ref("node:inspector.default.NetworkResources"); DOMStorage = ref("node:inspector.default.DOMStorage"); }; "node:os.constants", members = { __module__ = "os.constants"; }; "node:os.default", members = { arch = ref("node:os.arch"); availableParallelism = ref("node:os.availableParallelism"); cpus = ref("node:os.cpus"); endianness = ref("node:os.endianness"); freemem = ref("node:os.freemem"); getPriority = ref("node:os.getPriority"); homedir = ref("node:os.homedir"); hostname = ref("node:os.hostname"); loadavg = ref("node:os.loadavg"); networkInterfaces = ref("node:os.networkInterfaces"); platform = ref("node:os.platform"); release = ref("node:os.release"); setPriority = ref("node:os.setPriority"); tmpdir = ref("node:os.tmpdir"); totalmem = ref("node:os.totalmem"); type = ref("node:os.type"); userInfo = ref("node:os.userInfo"); uptime = ref("node:os.uptime"); version = ref("node:os.version"); machine = ref("node:os.machine"); constants = ref("node:os.constants"); EOL = "\n"; devNull = "/dev/null"; }; "node:path.default", members = { resolve = ref("node:path.resolve"); normalize = ref("node:path.normalize"); isAbsolute = ref("node:path.isAbsolute"); join = ref("node:path.join"); relative = ref("node:path.relative"); toNamespacedPath = ref("node:path._makeLong"); dirname = ref("node:path.dirname"); basename = ref("node:path.basename"); extname = ref("node:path.extname"); format = ref("node:path.format"); parse = ref("node:path.parse"); matchesGlob = ref("node:path.matchesGlob"); sep = "/"; delimiter = ":"; win32 = ref("node:path.win32"); posix = ref("node:path.posix"); _makeLong = ref("node:path._makeLong"); }; "node:perf_hooks.constants", members = { NODE_PERFORMANCE_GC_MAJOR = 4.0; NODE_PERFORMANCE_GC_MINOR = 1.0; NODE_PERFORMANCE_GC_MINOR_MARK_SWEEP = 2.0; NODE_PERFORMANCE_GC_INCREMENTAL = 8.0; NODE_PERFORMANCE_GC_WEAKCB = 16.0; NODE_PERFORMANCE_GC_FLAGS_NO = 0.0; NODE_PERFORMANCE_GC_FLAGS_CONSTRUCT_RETAINED = 2.0; NODE_PERFORMANCE_GC_FLAGS_FORCED = 4.0; NODE_PERFORMANCE_GC_FLAGS_SYNCHRONOUS_PHANTOM_PROCESSING = 8.0; NODE_PERFORMANCE_GC_FLAGS_ALL_AVAILABLE_GARBAGE = 16.0; NODE_PERFORMANCE_GC_FLAGS_ALL_EXTERNAL_MEMORY = 32.0; NODE_PERFORMANCE_GC_FLAGS_SCHEDULE_IDLE = 64.0; }; "node:querystring.default", members = { unescapeBuffer = ref("node:querystring.unescapeBuffer"); unescape = ref("node:querystring.unescape"); escape = ref("node:querystring.escape"); stringify = ref("node:querystring.encode"); encode = ref("node:querystring.encode"); parse = ref("node:querystring.decode"); decode = ref("node:querystring.decode"); }; "node:readline.default", members = { __module__ = "readline"; }; "node:readline.promises", members = { Interface = ref("node:readline/promises.Interface"); Readline = ref("node:readline/promises.Readline"); createInterface = ref("node:readline/promises.createInterface"); }; "node:repl.default", members = { REPLServer = ref("node:repl.REPLServer"); REPL_MODE_SLOPPY = Symbol.for("nodejs.repl.mode.sloppy"); REPL_MODE_STRICT = Symbol.for("nodejs.repl.mode.strict"); Recoverable = ref("node:repl.Recoverable"); builtinModules = ref("node:module.builtinModules"); start = ref("node:repl.start"); }; "node:sea.default", members = { isSea = ref("node:sea.isSea"); getAsset = ref("node:sea.getAsset"); getRawAsset = ref("node:sea.getRawAsset"); getAssetAsBlob = ref("node:sea.getAssetAsBlob"); getAssetKeys = ref("node:sea.getAssetKeys"); }; "node:stream.Stream.promises", members = { __module__ = "stream/promises"; }; "node:string_decoder.default", members = { __module__ = "string_decoder"; }; "node:sys.default", members = { aborted = ref("node:sys.aborted"); callbackify = ref("node:sys.callbackify"); convertProcessSignalToExitCode = ref("node:sys.convertProcessSignalToExitCode"); debug; debuglog = ref("node:sys.debug"); deprecate = ref("node:sys.deprecate"); diff = ref("node:sys.diff"); format = ref("node:sys.format"); formatWithOptions = ref("node:sys.formatWithOptions"); getCallSites = ref("node:sys.getCallSites"); getSystemErrorMap = ref("node:sys.getSystemErrorMap"); getSystemErrorName = ref("node:sys.getSystemErrorName"); getSystemErrorMessage = ref("node:sys.getSystemErrorMessage"); inherits = ref("node:sys.inherits"); inspect = ref("node:sys.inspect"); isArray = ref("node:sys.isArray"); isDeepStrictEqual = ref("node:sys.isDeepStrictEqual"); promisify = ref("node:sys.promisify"); stripVTControlCharacters = ref("node:sys.stripVTControlCharacters"); styleText = ref("node:sys.styleText"); toUSVString = ref("node:sys.toUSVString"); setTraceSigInt = ref("node:sys.setTraceSigInt"); types = ref("node:sys.types"); parseArgs = ref("node:sys.parseArgs"); TextDecoder = ref(TextDecoder); TextEncoder = ref(TextEncoder); transferableAbortController = ref("node:sys.transferableAbortController"); transferableAbortSignal = ref("node:sys.transferableAbortSignal"); }; "node:tls.default", members = { checkServerIdentity = ref("node:tls.checkServerIdentity"); connect = ref("node:tls.connect"); convertALPNProtocols = ref("node:tls.convertALPNProtocols"); createServer = ref("node:tls.createServer"); createSecureContext = ref("node:tls.createSecureContext"); getCACertificates = ref("node:tls.getCACertificates"); getCertificateCompressionAlgorithms = ref("node:tls.getCertificateCompressionAlgorithms"); getCiphers = ref("node:tls.getCiphers"); setDefaultCACertificates = ref("node:tls.setDefaultCACertificates"); Server = ref("node:tls.Server"); SecureContext = ref("node:tls.SecureContext"); TLSSocket = ref("node:tls.TLSSocket"); DEFAULT_ECDH_CURVE = "auto"; DEFAULT_MAX_VERSION = "TLSv1.3"; DEFAULT_MIN_VERSION = "TLSv1.2"; DEFAULT_CIPHERS = "TLS_AES_256_GCM_SHA384:TLS_CHACHA20_POLY1305_SHA256:TLS_AES_128_GCM_SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-AES256-GCM-SHA384:DHE-RSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-SHA256:DHE-RSA-AES128-SHA256:ECDHE-RSA-AES256-SHA384:DHE-RSA-AES256-SHA384:ECDHE-RSA-AES256-SHA256:DHE-RSA-AES256-SHA256:HIGH:!aNULL:!eNULL:!EXPORT:!DES:!RC4:!MD5:!PSK:!SRP:!CAMELLIA"; rootCertificates = ref("node:tls.rootCertificates"); CLIENT_RENEG_LIMIT = 3.0; CLIENT_RENEG_WINDOW = 600.0; }; "node:tty.default", members = { isatty = ref("node:tty.isatty"); ReadStream = ref("node:tty.ReadStream"); WriteStream = ref("node:tty.WriteStream"); }; "node:url.default", members = { Url = ref("node:url.Url"); parse = ref("node:url.parse"); resolve = ref("node:url.resolve"); resolveObject = ref("node:url.resolveObject"); format = ref("node:url.format"); URL = ref(URL); URLSearchParams = ref(URLSearchParams); URLPattern = ref(URLPattern); domainToASCII = ref("node:url.domainToASCII"); domainToUnicode = ref("node:url.domainToUnicode"); pathToFileURL = ref("node:url.pathToFileURL"); fileURLToPath = ref("node:url.fileURLToPath"); fileURLToPathBuffer = ref("node:url.fileURLToPathBuffer"); urlToHttpOptions = ref("node:url.urlToHttpOptions"); }; "node:v8.default", members = { cachedDataVersionTag = ref("node:v8.cachedDataVersionTag"); getHeapSnapshot = ref("node:v8.getHeapSnapshot"); getHeapStatistics = ref("node:v8.getHeapStatistics"); getHeapSpaceStatistics = ref("node:v8.getHeapSpaceStatistics"); getHeapCodeStatistics = ref("node:v8.getHeapCodeStatistics"); setFlagsFromString = ref("node:v8.setFlagsFromString"); Serializer = ref("node:v8.Serializer"); Deserializer = ref("node:v8.Deserializer"); DefaultSerializer = ref("node:v8.DefaultSerializer"); DefaultDeserializer = ref("node:v8.DefaultDeserializer"); deserialize = ref("node:v8.deserialize"); takeCoverage = ref("node:v8.takeCoverage"); stopCoverage = ref("node:v8.stopCoverage"); serialize = ref("node:v8.serialize"); writeHeapSnapshot = ref("node:v8.writeHeapSnapshot"); promiseHooks; startupSnapshot; setHeapSnapshotNearHeapLimit = ref("node:v8.setHeapSnapshotNearHeapLimit"); GCProfiler = ref("node:v8.GCProfiler"); }; "node:v8.default.promiseHooks", members = { __module__ = "v8.promiseHooks"; }; "node:v8.default.startupSnapshot", members = { __module__ = "v8.startupSnapshot"; }; "node:v8.promiseHooks", members = { __module__ = "v8.promiseHooks"; }; "node:v8.startupSnapshot", members = { __module__ = "v8.startupSnapshot"; }; "node:wasi.default", members = { WASI = ref("node:wasi.WASI"); }; "node:worker_threads.default", members = { BroadcastChannel = ref(BroadcastChannel); MessageChannel = ref(MessageChannel); MessagePort = ref(MessagePort); SHARE_ENV = Symbol.for("nodejs.worker_threads.SHARE_ENV"); Worker = ref("node:worker_threads.Worker"); getEnvironmentData = ref("node:worker_threads.getEnvironmentData"); isInternalThread = false; isMainThread = true; isMarkedAsUntransferable = ref("node:worker_threads.isMarkedAsUntransferable"); locks = ref("node:worker_threads.locks"); markAsUncloneable = ref("node:worker_threads.markAsUncloneable"); markAsUntransferable = ref("node:worker_threads.markAsUntransferable"); moveMessagePortToContext = ref("node:worker_threads.moveMessagePortToContext"); parentPort = null; postMessageToThread = ref("node:worker_threads.postMessageToThread"); receiveMessageOnPort = ref("node:worker_threads.receiveMessageOnPort"); resourceLimits; setEnvironmentData = ref("node:worker_threads.setEnvironmentData"); threadId = 0.0; threadName = ""; workerData = null; }; "node:zlib.constants", members = { Z_NO_FLUSH = 0.0, attrs = enumerable; Z_PARTIAL_FLUSH = 1.0, attrs = enumerable; Z_SYNC_FLUSH = 2.0, attrs = enumerable; Z_FULL_FLUSH = 3.0, attrs = enumerable; Z_FINISH = 4.0, attrs = enumerable; Z_BLOCK = 5.0, attrs = enumerable; Z_OK = 0.0, attrs = enumerable; Z_STREAM_END = 1.0, attrs = enumerable; Z_NEED_DICT = 2.0, attrs = enumerable; Z_ERRNO = -1.0, attrs = enumerable; Z_STREAM_ERROR = -2.0, attrs = enumerable; Z_DATA_ERROR = -3.0, attrs = enumerable; Z_MEM_ERROR = -4.0, attrs = enumerable; Z_BUF_ERROR = -5.0, attrs = enumerable; Z_VERSION_ERROR = -6.0, attrs = enumerable; Z_NO_COMPRESSION = 0.0, attrs = enumerable; Z_BEST_SPEED = 1.0, attrs = enumerable; Z_BEST_COMPRESSION = 9.0, attrs = enumerable; Z_DEFAULT_COMPRESSION = -1.0, attrs = enumerable; Z_FILTERED = 1.0, attrs = enumerable; Z_HUFFMAN_ONLY = 2.0, attrs = enumerable; Z_RLE = 3.0, attrs = enumerable; Z_FIXED = 4.0, attrs = enumerable; Z_DEFAULT_STRATEGY = 0.0, attrs = enumerable; ZLIB_VERNUM = 4880.0, attrs = enumerable; DEFLATE = 1.0, attrs = enumerable; INFLATE = 2.0, attrs = enumerable; GZIP = 3.0, attrs = enumerable; GUNZIP = 4.0, attrs = enumerable; DEFLATERAW = 5.0, attrs = enumerable; INFLATERAW = 6.0, attrs = enumerable; UNZIP = 7.0, attrs = enumerable; BROTLI_DECODE = 8.0, attrs = enumerable; BROTLI_ENCODE = 9.0, attrs = enumerable; ZSTD_DECOMPRESS = 11.0, attrs = enumerable; ZSTD_COMPRESS = 10.0, attrs = enumerable; Z_MIN_WINDOWBITS = 8.0, attrs = enumerable; Z_MAX_WINDOWBITS = 15.0, attrs = enumerable; Z_DEFAULT_WINDOWBITS = 15.0, attrs = enumerable; Z_MIN_CHUNK = 64.0, attrs = enumerable; Z_MAX_CHUNK = 2147483647.0, attrs = enumerable; Z_DEFAULT_CHUNK = 16384.0, attrs = enumerable; Z_MIN_MEMLEVEL = 1.0, attrs = enumerable; Z_MAX_MEMLEVEL = 9.0, attrs = enumerable; Z_DEFAULT_MEMLEVEL = 8.0, attrs = enumerable; Z_MIN_LEVEL = -1.0, attrs = enumerable; Z_MAX_LEVEL = 9.0, attrs = enumerable; Z_DEFAULT_LEVEL = -1.0, attrs = enumerable; BROTLI_OPERATION_PROCESS = 0.0, attrs = enumerable; BROTLI_OPERATION_FLUSH = 1.0, attrs = enumerable; BROTLI_OPERATION_FINISH = 2.0, attrs = enumerable; BROTLI_OPERATION_EMIT_METADATA = 3.0, attrs = enumerable; BROTLI_PARAM_MODE = 0.0, attrs = enumerable; BROTLI_MODE_GENERIC = 0.0, attrs = enumerable; BROTLI_MODE_TEXT = 1.0, attrs = enumerable; BROTLI_MODE_FONT = 2.0, attrs = enumerable; BROTLI_DEFAULT_MODE = 0.0, attrs = enumerable; BROTLI_PARAM_QUALITY = 1.0, attrs = enumerable; BROTLI_MIN_QUALITY = 0.0, attrs = enumerable; BROTLI_MAX_QUALITY = 11.0, attrs = enumerable; BROTLI_DEFAULT_QUALITY = 11.0, attrs = enumerable; BROTLI_PARAM_LGWIN = 2.0, attrs = enumerable; BROTLI_MIN_WINDOW_BITS = 10.0, attrs = enumerable; BROTLI_MAX_WINDOW_BITS = 24.0, attrs = enumerable; BROTLI_LARGE_MAX_WINDOW_BITS = 30.0, attrs = enumerable; BROTLI_DEFAULT_WINDOW = 22.0, attrs = enumerable; BROTLI_PARAM_LGBLOCK = 3.0, attrs = enumerable; BROTLI_MIN_INPUT_BLOCK_BITS = 16.0, attrs = enumerable; BROTLI_MAX_INPUT_BLOCK_BITS = 24.0, attrs = enumerable; BROTLI_PARAM_DISABLE_LITERAL_CONTEXT_MODELING = 4.0, attrs = enumerable; BROTLI_PARAM_SIZE_HINT = 5.0, attrs = enumerable; BROTLI_PARAM_LARGE_WINDOW = 6.0, attrs = enumerable; BROTLI_PARAM_NPOSTFIX = 7.0, attrs = enumerable; BROTLI_PARAM_NDIRECT = 8.0, attrs = enumerable; BROTLI_DECODER_RESULT_ERROR = 0.0, attrs = enumerable; BROTLI_DECODER_RESULT_SUCCESS = 1.0, attrs = enumerable; BROTLI_DECODER_RESULT_NEEDS_MORE_INPUT = 2.0, attrs = enumerable; BROTLI_DECODER_RESULT_NEEDS_MORE_OUTPUT = 3.0, attrs = enumerable; BROTLI_DECODER_PARAM_DISABLE_RING_BUFFER_REALLOCATION = 0.0, attrs = enumerable; BROTLI_DECODER_PARAM_LARGE_WINDOW = 1.0, attrs = enumerable; BROTLI_DECODER_NO_ERROR = 0.0, attrs = enumerable; BROTLI_DECODER_SUCCESS = 1.0, attrs = enumerable; BROTLI_DECODER_NEEDS_MORE_INPUT = 2.0, attrs = enumerable; BROTLI_DECODER_NEEDS_MORE_OUTPUT = 3.0, attrs = enumerable; BROTLI_DECODER_ERROR_FORMAT_EXUBERANT_NIBBLE = -1.0, attrs = enumerable; BROTLI_DECODER_ERROR_FORMAT_RESERVED = -2.0, attrs = enumerable; BROTLI_DECODER_ERROR_FORMAT_EXUBERANT_META_NIBBLE = -3.0, attrs = enumerable; BROTLI_DECODER_ERROR_FORMAT_SIMPLE_HUFFMAN_ALPHABET = -4.0, attrs = enumerable; BROTLI_DECODER_ERROR_FORMAT_SIMPLE_HUFFMAN_SAME = -5.0, attrs = enumerable; BROTLI_DECODER_ERROR_FORMAT_CL_SPACE = -6.0, attrs = enumerable; BROTLI_DECODER_ERROR_FORMAT_HUFFMAN_SPACE = -7.0, attrs = enumerable; BROTLI_DECODER_ERROR_FORMAT_CONTEXT_MAP_REPEAT = -8.0, attrs = enumerable; BROTLI_DECODER_ERROR_FORMAT_BLOCK_LENGTH_1 = -9.0, attrs = enumerable; BROTLI_DECODER_ERROR_FORMAT_BLOCK_LENGTH_2 = -10.0, attrs = enumerable; BROTLI_DECODER_ERROR_FORMAT_TRANSFORM = -11.0, attrs = enumerable; BROTLI_DECODER_ERROR_FORMAT_DICTIONARY = -12.0, attrs = enumerable; BROTLI_DECODER_ERROR_FORMAT_WINDOW_BITS = -13.0, attrs = enumerable; BROTLI_DECODER_ERROR_FORMAT_PADDING_1 = -14.0, attrs = enumerable; BROTLI_DECODER_ERROR_FORMAT_PADDING_2 = -15.0, attrs = enumerable; BROTLI_DECODER_ERROR_FORMAT_DISTANCE = -16.0, attrs = enumerable; BROTLI_DECODER_ERROR_FORMAT_BLOCK_SWITCH = -17.0, attrs = enumerable; BROTLI_DECODER_ERROR_DICTIONARY_NOT_SET = -19.0, attrs = enumerable; BROTLI_DECODER_ERROR_INVALID_ARGUMENTS = -20.0, attrs = enumerable; BROTLI_DECODER_ERROR_ALLOC_CONTEXT_MODES = -21.0, attrs = enumerable; BROTLI_DECODER_ERROR_ALLOC_TREE_GROUPS = -22.0, attrs = enumerable; BROTLI_DECODER_ERROR_ALLOC_CONTEXT_MAP = -25.0, attrs = enumerable; BROTLI_DECODER_ERROR_ALLOC_RING_BUFFER_1 = -26.0, attrs = enumerable; BROTLI_DECODER_ERROR_ALLOC_RING_BUFFER_2 = -27.0, attrs = enumerable; BROTLI_DECODER_ERROR_ALLOC_BLOCK_TYPE_TREES = -30.0, attrs = enumerable; BROTLI_DECODER_ERROR_UNREACHABLE = -31.0, attrs = enumerable; ZSTD_e_continue = 0.0, attrs = enumerable; ZSTD_e_flush = 1.0, attrs = enumerable; ZSTD_e_end = 2.0, attrs = enumerable; ZSTD_fast = 1.0, attrs = enumerable; ZSTD_dfast = 2.0, attrs = enumerable; ZSTD_greedy = 3.0, attrs = enumerable; ZSTD_lazy = 4.0, attrs = enumerable; ZSTD_lazy2 = 5.0, attrs = enumerable; ZSTD_btlazy2 = 6.0, attrs = enumerable; ZSTD_btopt = 7.0, attrs = enumerable; ZSTD_btultra = 8.0, attrs = enumerable; ZSTD_btultra2 = 9.0, attrs = enumerable; ZSTD_c_compressionLevel = 100.0, attrs = enumerable; ZSTD_c_windowLog = 101.0, attrs = enumerable; ZSTD_c_hashLog = 102.0, attrs = enumerable; ZSTD_c_chainLog = 103.0, attrs = enumerable; ZSTD_c_searchLog = 104.0, attrs = enumerable; ZSTD_c_minMatch = 105.0, attrs = enumerable; ZSTD_c_targetLength = 106.0, attrs = enumerable; ZSTD_c_strategy = 107.0, attrs = enumerable; ZSTD_c_enableLongDistanceMatching = 160.0, attrs = enumerable; ZSTD_c_ldmHashLog = 161.0, attrs = enumerable; ZSTD_c_ldmMinMatch = 162.0, attrs = enumerable; ZSTD_c_ldmBucketSizeLog = 163.0, attrs = enumerable; ZSTD_c_ldmHashRateLog = 164.0, attrs = enumerable; ZSTD_c_contentSizeFlag = 200.0, attrs = enumerable; ZSTD_c_checksumFlag = 201.0, attrs = enumerable; ZSTD_c_dictIDFlag = 202.0, attrs = enumerable; ZSTD_c_nbWorkers = 400.0, attrs = enumerable; ZSTD_c_jobSize = 401.0, attrs = enumerable; ZSTD_c_overlapLog = 402.0, attrs = enumerable; ZSTD_d_windowLogMax = 100.0, attrs = enumerable; ZSTD_CLEVEL_DEFAULT = 3.0, attrs = enumerable; ZSTD_error_no_error = 0.0, attrs = enumerable; ZSTD_error_GENERIC = 1.0, attrs = enumerable; ZSTD_error_prefix_unknown = 10.0, attrs = enumerable; ZSTD_error_version_unsupported = 12.0, attrs = enumerable; ZSTD_error_frameParameter_unsupported = 14.0, attrs = enumerable; ZSTD_error_frameParameter_windowTooLarge = 16.0, attrs = enumerable; ZSTD_error_corruption_detected = 20.0, attrs = enumerable; ZSTD_error_checksum_wrong = 22.0, attrs = enumerable; ZSTD_error_literals_headerWrong = 24.0, attrs = enumerable; ZSTD_error_dictionary_corrupted = 30.0, attrs = enumerable; ZSTD_error_dictionary_wrong = 32.0, attrs = enumerable; ZSTD_error_dictionaryCreation_failed = 34.0, attrs = enumerable; ZSTD_error_parameter_unsupported = 40.0, attrs = enumerable; ZSTD_error_parameter_combination_unsupported = 41.0, attrs = enumerable; ZSTD_error_parameter_outOfBound = 42.0, attrs = enumerable; ZSTD_error_tableLog_tooLarge = 44.0, attrs = enumerable; ZSTD_error_maxSymbolValue_tooLarge = 46.0, attrs = enumerable; ZSTD_error_maxSymbolValue_tooSmall = 48.0, attrs = enumerable; ZSTD_error_stabilityCondition_notRespected = 50.0, attrs = enumerable; ZSTD_error_stage_wrong = 60.0, attrs = enumerable; ZSTD_error_init_missing = 62.0, attrs = enumerable; ZSTD_error_memory_allocation = 64.0, attrs = enumerable; ZSTD_error_workSpace_tooSmall = 66.0, attrs = enumerable; ZSTD_error_dstSize_tooSmall = 70.0, attrs = enumerable; ZSTD_error_srcSize_wrong = 72.0, attrs = enumerable; ZSTD_error_dstBuffer_null = 74.0, attrs = enumerable; ZSTD_error_noForwardProgress_destFull = 80.0, attrs = enumerable; ZSTD_error_noForwardProgress_inputEmpty = 82.0, attrs = enumerable; }; "node:zlib.default", members = { crc32 = ref("node:zlib.crc32"); Deflate = ref("node:zlib.Deflate"); Inflate = ref("node:zlib.Inflate"); Gzip = ref("node:zlib.Gzip"); Gunzip = ref("node:zlib.Gunzip"); DeflateRaw = ref("node:zlib.DeflateRaw"); InflateRaw = ref("node:zlib.InflateRaw"); Unzip = ref("node:zlib.Unzip"); BrotliCompress = ref("node:zlib.BrotliCompress"); BrotliDecompress = ref("node:zlib.BrotliDecompress"); ZstdCompress = ref("node:zlib.ZstdCompress"); ZstdDecompress = ref("node:zlib.ZstdDecompress"); deflate = ref("node:zlib.deflate"); deflateSync = ref("node:zlib.deflateSync"); gzip = ref("node:zlib.gzip"); gzipSync = ref("node:zlib.gzipSync"); deflateRaw = ref("node:zlib.deflateRaw"); deflateRawSync = ref("node:zlib.deflateRawSync"); unzip = ref("node:zlib.unzip"); unzipSync = ref("node:zlib.unzipSync"); inflate = ref("node:zlib.inflate"); inflateSync = ref("node:zlib.inflateSync"); gunzip = ref("node:zlib.gunzip"); gunzipSync = ref("node:zlib.gunzipSync"); inflateRaw = ref("node:zlib.inflateRaw"); inflateRawSync = ref("node:zlib.inflateRawSync"); brotliCompress = ref("node:zlib.brotliCompress"); brotliCompressSync = ref("node:zlib.brotliCompressSync"); brotliDecompress = ref("node:zlib.brotliDecompress"); brotliDecompressSync = ref("node:zlib.brotliDecompressSync"); zstdCompress = ref("node:zlib.zstdCompress"); zstdCompressSync = ref("node:zlib.zstdCompressSync"); zstdDecompress = ref("node:zlib.zstdDecompress"); zstdDecompressSync = ref("node:zlib.zstdDecompressSync"); createDeflate = ref("node:zlib.createDeflate"); createInflate = ref("node:zlib.createInflate"); createDeflateRaw = ref("node:zlib.createDeflateRaw"); createInflateRaw = ref("node:zlib.createInflateRaw"); createGzip = ref("node:zlib.createGzip"); createGunzip = ref("node:zlib.createGunzip"); createUnzip = ref("node:zlib.createUnzip"); createBrotliCompress = ref("node:zlib.createBrotliCompress"); createBrotliDecompress = ref("node:zlib.createBrotliDecompress"); createZstdCompress = ref("node:zlib.createZstdCompress"); createZstdDecompress = ref("node:zlib.createZstdDecompress"); constants = ref("node:zlib.constants"), attrs = enumerable; codes = ref("node:zlib.codes"), attrs = enumerable; }; performance, extends = Performance.prototype, members = { Performance = ref(Performance); PerformanceEntry = ref(PerformanceEntry); PerformanceMark = ref(PerformanceMark); PerformanceMeasure = ref(PerformanceMeasure); PerformanceObserver = ref(PerformanceObserver); PerformanceObserverEntryList = ref(PerformanceObserverEntryList); PerformanceResourceTiming = ref(PerformanceResourceTiming); monitorEventLoopDelay; eventLoopUtilization; timerify; createHistogram; performance = ref(performance); constants; default; }; performance.constants, members = { NODE_PERFORMANCE_GC_MAJOR = 4.0; NODE_PERFORMANCE_GC_MINOR = 1.0; NODE_PERFORMANCE_GC_MINOR_MARK_SWEEP = 2.0; NODE_PERFORMANCE_GC_INCREMENTAL = 8.0; NODE_PERFORMANCE_GC_WEAKCB = 16.0; NODE_PERFORMANCE_GC_FLAGS_NO = 0.0; NODE_PERFORMANCE_GC_FLAGS_CONSTRUCT_RETAINED = 2.0; NODE_PERFORMANCE_GC_FLAGS_FORCED = 4.0; NODE_PERFORMANCE_GC_FLAGS_SYNCHRONOUS_PHANTOM_PROCESSING = 8.0; NODE_PERFORMANCE_GC_FLAGS_ALL_AVAILABLE_GARBAGE = 16.0; NODE_PERFORMANCE_GC_FLAGS_ALL_EXTERNAL_MEMORY = 32.0; NODE_PERFORMANCE_GC_FLAGS_SCHEDULE_IDLE = 64.0; }; performance.default, members = { Performance = ref(Performance); PerformanceEntry = ref(PerformanceEntry); PerformanceMark = ref(PerformanceMark); PerformanceMeasure = ref(PerformanceMeasure); PerformanceObserver = ref(PerformanceObserver); PerformanceObserverEntryList = ref(PerformanceObserverEntryList); PerformanceResourceTiming = ref(PerformanceResourceTiming); monitorEventLoopDelay = ref(performance.monitorEventLoopDelay); eventLoopUtilization = ref(performance.eventLoopUtilization); timerify = ref(performance.timerify); createHistogram = ref(performance.createHistogram); performance = ref(performance); constants; }; performance.default.constants, members = { NODE_PERFORMANCE_GC_MAJOR = 4.0; NODE_PERFORMANCE_GC_MINOR = 1.0; NODE_PERFORMANCE_GC_MINOR_MARK_SWEEP = 2.0; NODE_PERFORMANCE_GC_INCREMENTAL = 8.0; NODE_PERFORMANCE_GC_WEAKCB = 16.0; NODE_PERFORMANCE_GC_FLAGS_NO = 0.0; NODE_PERFORMANCE_GC_FLAGS_CONSTRUCT_RETAINED = 2.0; NODE_PERFORMANCE_GC_FLAGS_FORCED = 4.0; NODE_PERFORMANCE_GC_FLAGS_SYNCHRONOUS_PHANTOM_PROCESSING = 8.0; NODE_PERFORMANCE_GC_FLAGS_ALL_AVAILABLE_GARBAGE = 16.0; NODE_PERFORMANCE_GC_FLAGS_ALL_EXTERNAL_MEMORY = 32.0; NODE_PERFORMANCE_GC_FLAGS_SCHEDULE_IDLE = 64.0; }; console, members = { log; info; debug; warn; error; dir; time; timeEnd; timeLog; trace; assert; clear; count; countReset; group; groupEnd; table; dirxml; groupCollapsed; Console; profile; profileEnd; timeStamp; context; createTask; }; } };

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
    if module_name == "module" {
        crate::object::js_object_seal(value);
    }
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
        // The subpath module IS the object `path.posix` / `path.win32` / `util.types`
        // read as (the CJS default namespace for the path pair), so identity holds.
        "path/posix" => "path.posix.default",
        "path/win32" => "path.win32.default",
        "util/types" => "util.types",
        // #6563: `@lydell/node-pty` is an API-identical fork of node-pty
        // (opencode's import); both names resolve to the one runtime pty.
        "@lydell/node-pty" | "bun-pty" => "node-pty",
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
    super::define_builtin_data_property(
        proto_obj,
        key,
        ctor_value,
        constructor.to_string(),
        super::PropertyAttrs::new(true, false, true),
    );
}

pub(crate) fn subtle_crypto_namespace() -> f64 {
    js_create_native_module_namespace(b"crypto.subtle".as_ptr(), "crypto.subtle".len())
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
    native_module_property_by_name_impl(
        module_name_ptr,
        module_name_len,
        property_name_ptr,
        property_name_len,
        true,
    )
}

unsafe fn native_module_property_by_name_impl(
    module_name_ptr: *const u8,
    module_name_len: usize,
    property_name_ptr: *const u8,
    property_name_len: usize,
    consult_overrides: bool,
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
    if consult_overrides {
        if let Some(value) = native_namespace_user_value(module_name, property_name) {
            return value;
        }
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
    if matches!(module_name, "perf_hooks" | "perf_hooks.default") && property_name == "performance"
    {
        // Singleton so `require("perf_hooks").performance` and the global
        // `performance` are the same object (Node identity guarantee, #1327).
        return crate::perf_hook_slots::performance_namespace();
    }
    if matches!(module_name, "perf_hooks" | "perf_hooks.default") && property_name == "constants" {
        // Its OWN tag. Sharing the `perf_hooks` tag made every read of the
        // constants object resolve against the MODULE's surface, so
        // `Object.keys(constants)` enumerated the export list instead of the
        // `NODE_PERFORMANCE_GC_*` table.
        let submodule = "perf_hooks.constants";
        return js_create_native_module_namespace(submodule.as_ptr(), submodule.len());
    }
    // #1533: node:stream exposes a `promises` namespace (`await pipeline(...)`
    // / `finished(...)`). Resolve `stream.promises` to a `stream/promises`-
    // tagged namespace object so `typeof stream.promises === "object"` and
    // `stream.promises.pipeline` / `.finished` read as callable exports
    // (same dispatch the `import ... from "node:stream/promises"` form uses).
    if module_name == "stream" && property_name == "promises" {
        return crate::node_submodules::commonjs_value("stream_promises");
    }
    // #2133: same shape for `node:fs.promises`. Route to the populated
    // `fs_promises` singleton so destructured exports + FileHandle methods
    // dispatch correctly.
    if module_name == "fs" && property_name == "promises" {
        return crate::node_submodules::commonjs_value("fs_promises");
    }
    // `readline.promises` is the `readline/promises` submodule namespace.
    if module_name == "readline" && property_name == "promises" {
        return crate::node_submodules::commonjs_value("readline_promises");
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
    // #6560/#9599 — Bun globals shim pack. The stdio properties are
    // BunFile-like handles built by `bun_compat`. In Bun platform mode the
    // compiler installs this namespace on globalThis, so the metadata below is
    // deliberately Perry-specific rather than pretending to be a Bun runtime.
    if module_name == "bun" {
        match property_name {
            "ant" => {
                let submodule = "bun.ant";
                return js_create_native_module_namespace(submodule.as_ptr(), submodule.len());
            }
            "stdin" => return crate::bun_hook_slots::js_bun_stdin(),
            "stdout" => return crate::bun_hook_slots::js_bun_stdout(),
            "stderr" => return crate::bun_hook_slots::js_bun_stderr(),
            "YAML" => return crate::bun_compat::js_bun_yaml(),
            "TOML" => return crate::bun_compat::js_bun_toml(),
            "semver" => return crate::bun_compat::js_bun_semver(),
            "JSONL" => return crate::bun_compat::js_bun_jsonl(),
            "hash" => {
                return crate::bun_compat::decorate_bun_hash(bound_native_callable_export_value(
                    "bun", "hash",
                ))
            }
            "version" => return native_string_value(env!("CARGO_PKG_VERSION")),
            "isStandaloneExecutable" => return crate::embedded::is_standalone_executable_value(),
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
    // Modules the submodule registry implements (`timers/promises`, `stream/web`,
    // ...) have no dispatch bucket; their exports are the registry's own values.
    if let Some(value) =
        crate::node_submodules::registry_backed_module_export(module_name, property_name)
    {
        return value;
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

fn native_module_string_arg(value: f64) -> Option<String> {
    let value = JSValue::from_bits(value.to_bits());
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let bytes = unsafe { crate::string::js_string_key_bytes(value, &mut sso) }?;
    Some(String::from_utf8_lossy(bytes).into_owned())
}

fn native_module_export_value(module: f64, property: f64, observe_namespace_writes: bool) -> f64 {
    let Some(module) = native_module_string_arg(module) else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    let Some(property) = native_module_string_arg(property) else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    let module = normalize_native_module_alias(&module).to_string();
    if observe_namespace_writes {
        // A user write to the member wins over the built-in snapshot below.
        // Default and namespace imports expose Node's live mutable CommonJS
        // exports object. Named imports pass false and retain their ESM cell
        // until syncBuiltinESMExports() refreshes the shared cache.
        if let Some(value) = native_namespace_user_value(&module, &property) {
            return value;
        }
    }
    let live = native_module_constant_is_live(&module, &property);
    let key = format!("{module}\0{property}");
    let cached = (!live)
        .then(|| NATIVE_ESM_EXPORT_VALUES.with(|v| v.borrow().get(&key).copied()))
        .flatten();
    if let Some(bits) = cached {
        return f64::from_bits(bits);
    }
    let value = unsafe {
        native_module_property_by_name_impl(
            module.as_ptr(),
            module.len(),
            property.as_ptr(),
            property.len(),
            false,
        )
    };
    if live || value.to_bits() == crate::value::TAG_UNDEFINED {
        return value;
    }
    NATIVE_ESM_EXPORT_VALUES.with(|values| {
        values.borrow_mut().insert(key, value.to_bits());
    });
    crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
    value
}

/// Mutable property read used by native-module default and namespace objects.
/// User writes to the CommonJS namespace are observable immediately here.
#[no_mangle]
pub extern "C" fn js_native_module_esm_export_value(module: f64, property: f64) -> f64 {
    native_module_export_value(module, property, true)
}

/// Snapshot-backed value used for named ESM imports from builtins. CommonJS
/// namespace writes stay isolated until `syncBuiltinESMExports()` copies them.
#[no_mangle]
pub extern "C" fn js_native_module_named_esm_export_value(module: f64, property: f64) -> f64 {
    native_module_export_value(module, property, false)
}

/// Armed `.constructor` resolver for `MODULE_CJS` instances (binary size).
///
/// The generic field-read tail used to call
/// [`module_constructor_identity_value`] directly, which made every binary
/// retain the `node:module` export machinery. A `MODULE_CJS_CLASS_ID` object
/// is only ever minted by `js_module_module_new`, which arms this slot first,
/// so an unarmed slot and a matching class id cannot coexist.
static MODULE_CJS_CONSTRUCTOR_HOOK: std::sync::atomic::AtomicPtr<()> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

pub(crate) fn arm_module_cjs_constructor_hook() {
    // `black_box`: a single-store slot is otherwise devirtualized back into a
    // direct reference by whole-program optimization (see NM_INSTALL_ALL_HOOK).
    MODULE_CJS_CONSTRUCTOR_HOOK.store(
        std::hint::black_box(module_constructor_identity_value as fn() -> f64 as *mut ()),
        Ordering::Release,
    );
}

pub(crate) fn module_cjs_constructor_via_hook() -> Option<f64> {
    let p = MODULE_CJS_CONSTRUCTOR_HOOK.load(Ordering::Acquire);
    if p.is_null() {
        return None;
    }
    // SAFETY: only ever stores `module_constructor_identity_value`.
    let f: fn() -> f64 = unsafe { std::mem::transmute(p) };
    Some(f())
}

pub(crate) fn module_constructor_identity_value() -> f64 {
    const KEY: &str = "module\0Module";
    if let Some(bits) = NATIVE_ESM_EXPORT_VALUES.with(|values| values.borrow().get(KEY).copied()) {
        return f64::from_bits(bits);
    }
    if let Some(bits) = NATIVE_CALLABLE_EXPORTS.with(|values| values.borrow().get(KEY).copied()) {
        return f64::from_bits(bits);
    }
    bound_native_callable_export_value("module", "Module")
}

#[no_mangle]
pub extern "C" fn js_module_sync_builtin_esm_exports() -> f64 {
    let keys =
        NATIVE_ESM_EXPORT_VALUES.with(|values| values.borrow().keys().cloned().collect::<Vec<_>>());
    for key in keys {
        let Some((module, property)) = key.split_once('\0') else {
            continue;
        };
        let value = unsafe {
            native_module_property_by_name_impl(
                module.as_ptr(),
                module.len(),
                property.as_ptr(),
                property.len(),
                true,
            )
        };
        NATIVE_ESM_EXPORT_VALUES.with(|values| {
            values.borrow_mut().insert(key.clone(), value.to_bits());
        });
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_module_run_main() -> f64 {
    // Perry's AOT entry point has already run before JavaScript can call this
    // compatibility export, so there is no unevaluated main module to dispatch.
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Access a property on a native module namespace object.
/// For method references (e.g., `fs.existsSync`), creates a bound method closure.
/// For constant properties (e.g., `path.sep`, `fs.constants`), returns the value directly.
#[no_mangle]
pub extern "C" fn js_native_module_bind_method(
    namespace_obj: f64,
    property_name_ptr: *const u8,
    property_name_len: usize,
) -> f64 {
    let property_name = unsafe {
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(
            property_name_ptr,
            property_name_len,
        ))
    };

    // Keep the namespace current across constant/callable resolution: both
    // paths may allocate. The module name is an owned Rust string so a short
    // name such as `net` is never materialized into an unrooted GC string.
    let scope = crate::gc::RuntimeHandleScope::new();
    let namespace = scope.root_nanbox_f64(namespace_obj);
    let module_name = unsafe { get_module_name_from_namespace(namespace.get_nanbox_f64()) };

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

    if let Some(value) =
        performance_namespace_method(&module_name, property_name, namespace.get_nanbox_f64())
    {
        return value;
    }

    // Check for known constant properties first
    if let Some(val) = unsafe {
        get_native_module_constant(&module_name, property_name, namespace.get_nanbox_f64())
    } {
        return val;
    }

    // Not a constant. Only synthesize callables for
    // exports that are actually callable on this platform; otherwise namespace
    // reads such as Linux `fs.lchmodSync` must stay `undefined`.
    if is_native_module_callable_export(&module_name, property_name) {
        if let Some(bound) =
            instance_bound_perf_method(&module_name, property_name, namespace.get_nanbox_f64())
        {
            return bound;
        }
        let value = bound_native_callable_export_value(&module_name, property_name);
        return if module_name == "bun" && property_name == "hash" {
            crate::bun_compat::decorate_bun_hash(value)
        } else {
            value
        };
    }

    // Try V8 JS runtime fallback for unknown properties (e.g., ethers.Contract)
    let js_val = crate::value::native_module_try_js_property(&module_name, property_name);
    if js_val.to_bits() != crate::value::TAG_UNDEFINED {
        return js_val;
    }

    // Not a constant or JS-backed property. Only synthesize callables for
    // exports that are actually callable on this platform; otherwise namespace
    // reads such as Linux `fs.lchmodSync` must stay `undefined`.
    if !is_native_module_callable_export(&module_name, property_name) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }

    bound_native_callable_export_value(&module_name, property_name)
}

mod class_method_bind;
use class_method_bind::build_bound_method_closure_with_private_brand;
pub use class_method_bind::{
    js_class_method_bind, js_class_method_bind_by_id, js_class_method_snapshot_bind,
};
#[cfg(test)]
pub(crate) use class_method_bind::{
    test_collect_bound_method_after_capture_init, test_take_bound_method_move,
};

include!("native_module/class_method_values.rs");

/// #6173: sentinel "method name" installed in the name-capture slots (1, 2) of
/// a BOUND_METHOD closure whose target is a SYMBOL-keyed class method. A
/// symbol method has no string name to re-resolve at call time, so the
/// closure instead carries the already-resolved dispatch data in two extra
/// capture slots:
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
    display_name: &str,
) -> f64 {
    // The allocation itself is a safepoint. Keep the receiver current before
    // storing it into the freshly allocated closure.
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver_handle = scope.root_nanbox_f64(receiver);
    let closure_handle = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        &crate::closure::BOUND_METHOD_INFO,
        5,
    ));
    if closure_handle.with_mut_ptr::<crate::closure::ClosureHeader, _>(|c| c.is_null()) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let receiver_value = receiver_handle.get_nanbox_f64();
    let meta: i64 = (param_count as i64) | ((has_rest as i64) << 32) | ((is_static as i64) << 33);
    closure_handle.with_mut_ptr::<crate::closure::ClosureHeader, _>(|closure| {
        crate::closure::js_closure_set_capture_f64(closure, 0, receiver_value);
        crate::closure::closure_set_capture_word(
            closure,
            1,
            SYMBOL_BOUND_METHOD_NAME.as_ptr() as i64,
        );
        crate::closure::closure_set_capture_word(closure, 2, SYMBOL_BOUND_METHOD_NAME.len() as i64);
        crate::closure::closure_set_capture_word(closure, 3, func_ptr as i64);
        crate::closure::closure_set_capture_word(closure, 4, meta);
    });
    // Spec `.length` = declared params minus a trailing rest param.
    let spec_length = if has_rest {
        param_count.saturating_sub(1)
    } else {
        param_count
    };
    closure_handle.with_mut_ptr::<crate::closure::ClosureHeader, _>(|closure| {
        set_bound_native_closure_metadata(closure, display_name, spec_length)
    });
    closure_handle.with_mut_ptr::<crate::closure::ClosureHeader, _>(|closure| {
        crate::gc::runtime_write_barrier_root_raw_ptr(closure as *const u8)
    });
    closure_handle.with_mut_ptr::<crate::closure::ClosureHeader, _>(|closure| {
        crate::value::js_nanbox_pointer(closure as i64)
    })
}

/// Resolve the owning class id for a `js_class_method_bind` receiver: a class
/// constructor/prototype ref (INT32-tagged) or a real class instance pointer.
/// Resolve the effective receiver for a BOUND_METHOD dispatch. When the
/// captured receiver is a canonical class-method marker (a class prototype-ref,
/// produced by `class_prototype_method_value_for_name`), substitute the
/// call-site `this` argument provided it is itself a dispatchable class
/// receiver (an instance or class ref). Otherwise the captured value is the real
/// receiver and is returned unchanged. See `dispatch_bound_method`.
/// Is `value` a bound STATIC-method value — a BOUND_METHOD closure whose
/// captured receiver is a class constructor ref or a per-evaluation class
/// object (`C.staticMethod` read as a value)? Used by the Function.prototype
/// call/apply arms to arm the one-shot static-`this` override with the explicit
/// thisArg, so the static method body sees the receiver
/// (`C.m.call({})` → `this === {}`) and static private brand checks behave per
/// spec.
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
        unsafe { (*closure).code() },
        crate::closure::BOUND_METHOD_FUNC_PTR,
    ) {
        return false;
    }
    let captured = crate::closure::js_closure_get_capture_f64(closure, 0);
    (class_ref_id(captured).is_some() && class_prototype_ref_id(captured).is_none())
        || class_registry::is_class_object_value(captured)
}

pub(crate) fn canonical_bound_method_receiver(
    captured: f64,
    call_this: crate::closure::JsThis,
) -> f64 {
    if class_prototype_ref_id(captured).is_some() {
        // The captured prototype identifies the method's owner, not its receiver.
        // Class methods are strict: every call-site value, including null,
        // undefined, primitives and arrays, must reach the body unchanged.
        // Dispatch resolves the body from the captured owner independently.
        call_this.as_f64()
    } else {
        captured
    }
}

/// The `class_id` of `instance`, when `instance` really is a class instance.
///
/// `pub(super)` so `object::tests` can assert the #7563 invariant directly: a
/// non-object allocation (an array, above all) must resolve to `None` rather
/// than to whatever its bytes happen to hold at the `class_id` offset.
pub(super) fn class_id_from_method_receiver(instance: f64) -> Option<u32> {
    class_id_from_method_receiver_known(instance, class_ref_id(instance))
}

/// [`class_id_from_method_receiver`] for a caller that already asked
/// `class_ref_id(instance)`.
#[inline]
fn class_id_from_method_receiver_known(instance: f64, class_ref: Option<u32>) -> Option<u32> {
    if let Some(cid) = class_ref {
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
            // the call-site `this` was the `assert` function) would mis-substitute the
            // function object as the receiver and dispatch `assert.method(...)`
            // instead of `C.prototype.method`, bypassing the generator wrapper's
            // param prologue. See `canonical_bound_method_receiver`.
            if crate::closure::is_closure_ptr(obj as usize) {
                return None;
            }
            // #7563: the closure guard above fixed ONE instance of that type
            // confusion; a bare `(*obj).class_id` read has it for every other
            // non-object allocation too. `ObjectHeader` is `{ class_id: u32,
            // class_id: u32, … }` while `ArrayHeader` is `{ length: u32,
            // capacity: u32 }`, so the `class_id` slot of an ARRAY overlays its
            // **capacity** — an N-capacity array literal was read back as
            // "class id N". Reached from `arr[Symbol.iterator]`, which resolves via
            // `js_class_method_bind(arr, "values")` (`symbol/get.rs`): whenever
            // class id N happened to own a `values` method, the array's
            // iterator resolved to THAT class's method. With `class Plain {
            // values() { return [777][Symbol.iterator](); } }` the one-element
            // literal read back as class id 1 — `Plain` itself — so `values`
            // called `values` until the stack guard page: a SIGSEGV with no
            // `Map` anywhere in the program.
            //
            // `js_object_get_class_id` is the guarded accessor for exactly this
            // read: it rejects the handle band, the std::alloc'd Map/Set/Regex
            // headers (which have no `GcHeader` to probe), and — the part that
            // matters here — any allocation whose `GcHeader.obj_type` is not
            // `GC_TYPE_OBJECT`. Reading the field directly bypassed all three.
            let cid = crate::object::js_object_get_class_id(obj);
            if cid != 0 {
                return Some(cid);
            }
        }
    }
    None
}

include!("native_module/class_ref_values.rs");

/// Extract an owned module name from a native module namespace object.
///
/// Short-string fields must stay inline here. Materializing one on the GC heap
/// and returning a borrowed slice fabricates a `'static` lifetime over an
/// unrooted allocation; the next allocation can evacuate it while callers are
/// still comparing the module name (#8403).
pub(crate) unsafe fn get_module_name_from_namespace(namespace_obj: f64) -> String {
    let jsval = JSValue::from_bits(namespace_obj.to_bits());
    if !jsval.is_pointer() {
        return String::new();
    }
    let obj = jsval.as_pointer::<ObjectHeader>();
    if crate::value::addr_class::is_handle_band(obj as usize) {
        return String::new();
    }
    read_native_module_name(obj).unwrap_or_default()
}

/// #6667: materialize a native-module namespace's exports into `dst` during
/// object spread (`{ ...crypto }`) or `Object.assign(dst, crypto)`. A
/// native-module object physically stores only the internal `__module__`
/// sentinel — every real export resolves lazily through the vtable — so the
/// raw `keys_array` walk both copy helpers use otherwise copies nothing, and
/// every enumeration-based interop layer (turbopack `e.i`, Babel
/// `interopRequireWildcard`, plain spread) produced an empty namespace. Here
/// we enumerate the module's export names (`native_module_enumerable_keys`, the
/// same list `Object.keys` returns) and resolve each to its live value through
/// the authoritative `[[Get]]` path, handing `(key, value)` to `set`.
///
/// Returns `true` when `src` is a native-module namespace with a known export
/// set (the caller then skips its fallback walk); `false` otherwise, so a
/// namespace with no key table degrades to the pre-existing behavior.
///
/// GC: a callable export resolves to a freshly allocated bound-method closure,
/// so `src`, the key string, and the resolved value are each rooted across the
/// allocations that would otherwise move them out from under the raw pointers.
///
/// # Safety
/// `src` must point to a live `ObjectHeader`.
pub(crate) unsafe fn copy_native_module_exports(
    mut src: *const ObjectHeader,
    mut set: impl FnMut(*const crate::StringHeader, f64),
) -> bool {
    if (*src).class_id != NATIVE_MODULE_CLASS_ID {
        return false;
    }
    let Some(module_name) = read_native_module_name(src) else {
        return false;
    };
    let Some(keys) = native_module_enumerable_keys(&module_name) else {
        return false;
    };
    let include_permission = matches!(
        module_name.as_str(),
        "process" | "process.namespace" | "process.default"
    ) && crate::process::process_permission_enabled();

    for key_bytes in keys
        .iter()
        .copied()
        .chain(include_permission.then_some(b"permission" as &[u8]))
    {
        let scope = crate::gc::RuntimeHandleScope::new();
        let src_h = scope.root_raw_const_ptr(src);
        let key_ptr =
            crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
        let key_h = scope.root_string_ptr(key_ptr);
        let value = js_object_get_field_by_name(
            src_h.get_raw_const_ptr::<ObjectHeader>(),
            key_h.get_raw_const_ptr::<crate::StringHeader>(),
        );
        let value_h = scope.root_nanbox_f64(f64::from_bits(value.bits()));
        set(
            key_h.get_raw_const_ptr::<crate::StringHeader>(),
            value_h.get_nanbox_f64(),
        );
        // The allocations above (key string, resolved-export closure, the `set`
        // store) can trigger a minor GC that evacuates `src`. `src_h` tracked
        // the move; write the refreshed pointer back before its scope drops so
        // the next iteration re-roots the live location, not a stale one.
        src = src_h.get_raw_const_ptr::<ObjectHeader>();
    }
    true
}

mod cjs_default;
// Re-exported so the existing paths keep resolving: `native_module/constants.rs`
// reaches `cjs_default_export_value` through this module, and `process.rs` reaches
// `native_module_get_builtin_module_value` via `crate::object::`.
pub(crate) use cjs_default::{
    cjs_default_base_module, cjs_default_export_value, native_module_get_builtin_module_value,
};

#[cfg(test)]
mod buffer_pool_size_tests;

crate::realm_decl!("native_module.realm.json#*");

#[cfg(test)]
mod namespace_accessor_tests;

#[cfg(test)]
mod realm_native_storage_tests {
    include!(concat!(env!("OUT_DIR"), "/realm_native_storage_test.rs"));
}
