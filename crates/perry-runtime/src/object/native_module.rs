//! Native-module namespace machinery: allocator (`js_create_native_module_namespace_vt`),
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
mod static_exports;
pub(crate) use perf_instance_bind::instance_bound_perf_method;
pub(crate) use static_exports::construct as static_export_construct;
pub(crate) use static_exports::selected_value as selected_native_export_value;
// `assert`'s constructors are read through their own export bodies: whichever
// path reads one first, the value is the decorated, constructable export.
pub(crate) use static_exports::{
    call_minted as call_minted_native_export,
    js_native_export_617373657274_417373657274 as assert_assert_export,
    js_native_export_617373657274_417373657274696f6e4572726f72 as assert_assertion_error_export,
    js_native_export_6576656e7473_4576656e74456d6974746572 as event_emitter_export_value,
    js_native_export_6576656e7473_4576656e74456d69747465724173796e635265736f75726365 as event_emitter_async_resource_export_value,
    stream_export_value,
};
pub(crate) mod constants;
mod constants_tables;
mod constructor_exports;
pub(crate) mod constructor_shapes;
pub(crate) mod module_keys;
mod name_tables;
mod namespace_builders;
mod namespace_prototype;
pub(crate) mod namespaces;
mod vtable_impls;
mod web_locks;
pub(crate) mod worker_environment;

pub(crate) use callable_export_check::is_native_module_callable_export;
pub use callable_exports::bound_native_callable_export_value;

pub(crate) fn worker_constructor_value() -> f64 {
    // Lowered Worker construction can bypass every namespace/export read.
    // Arm the existing decorator before its canonical constructor is minted.
    super::native_module_registry::js_nm_install_worker_threads();
    bound_native_callable_export_value(
        &namespaces::worker_threads::NM_WORKER_THREADS,
        "worker_threads",
        "Worker",
    )
}

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
/// The vtables perry-stdlib names when it births its own exports.
pub use module_keys::NM_SQLITE;
pub(crate) use module_keys::{
    native_module_has_enumerable_key, vm_modules_vtable, zlib_readonly_property_enumerability,
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
pub use namespaces::crypto::NM_CRYPTO;
pub use namespaces::domain::NM_DOMAIN;
pub use namespaces::events::NM_EVENTS;
pub use namespaces::net::NM_NET;
pub use namespaces::process::NM_PROCESS;
pub use namespaces::worker_threads::NM_WORKER_THREADS;
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
    let key = [module, prop].join("\0");
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
        NATIVE_ESM_EXPORT_VALUES.with(|values| {
            if let Some(slot) = values.borrow_mut().get_mut(&key) {
                *slot = value.to_bits();
            }
        });
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
    }
    NATIVE_NAMESPACE_PROP_OVERRIDES.with(|m| {
        m.borrow_mut().insert(key, value.to_bits());
    });
    // An override of a `console` member (`console.log = f` through a value,
    // or from interpreted code) replaces a member of the builtin namespace,
    // and no holder write funnel sees this table: report it as a write to
    // the namespace, so its guarded sites stop binding the intrinsic. The
    // holder is live once this thread's realm is populated and sealed (a
    // program may reach the console object before anything populates its
    // global): populate it first, so the report is never lost.
    if module == "console" {
        crate::object::js_get_global_this();
        // SAFETY: a namespace family names no object; `note_write` reads none.
        unsafe {
            super::builtin_namespace::note_write(
                std::ptr::null_mut(),
                super::builtin_namespace::CONSOLE_FAMILY,
                None,
                "namespace override",
            )
        };
    }
}

/// `delete ns.prop`: the member reads `undefined` wherever a store would be
/// seen (the store keeps the TLS defaults and the console holder coherent),
/// and its override entry becomes [`DELETED`], so every read, `in`, key
/// enumeration and has-own probe treats it as absent until a store replaces
/// the entry.
pub(crate) fn native_namespace_prop_override_delete(module: &str, prop: &str) {
    // The key first: the store can allocate (and so move `prop`'s string).
    let key = [module, prop].join("\0");
    native_namespace_prop_override_store(module, prop, f64::from_bits(crate::value::TAG_UNDEFINED));
    NATIVE_NAMESPACE_PROP_OVERRIDES.with(|m| {
        m.borrow_mut().insert(key, DELETED);
    });
}

/// The override entry of a deleted member: `TAG_HOLE`, never a JS value.
const DELETED: u64 = crate::value::TAG_HOLE;

/// Read back a stored native-namespace property override, if any. A deleted
/// member reads `undefined` here (a name-keyed read has no object to continue
/// at); `vt_get_own_field` continues at the namespace's prototype instead.
#[inline]
pub(crate) fn native_namespace_prop_override_get(module: &str, prop: &str) -> Option<f64> {
    native_namespace_prop_override_bits(module, prop).map(|bits| {
        f64::from_bits(if bits == DELETED {
            crate::value::TAG_UNDEFINED
        } else {
            bits
        })
    })
}

/// The raw override entry of a member: a value, [`DELETED`], or none.
fn native_namespace_prop_override_bits(module: &str, prop: &str) -> Option<u64> {
    NATIVE_NAMESPACE_PROP_OVERRIDES.with(|m| {
        let m = m.borrow();
        // #10523: every native namespace member read lands here and almost no
        // program monkey-patches one, so skip building the probe key.
        if m.is_empty() {
            return None;
        }
        m.get(&[module, prop].join("\0")).copied()
    })
}

/// Is member `prop` of namespace `module` absent although the module's
/// tables describe it: deleted by the program, or (when the program stored
/// none) hidden in a worker? The one presence predicate of the namespace's
/// own keys (`in`, has-own, descriptors, key enumeration, export copies,
/// name-keyed reads).
#[inline(never)]
pub(crate) fn native_namespace_member_absent(module: &str, prop: &str) -> bool {
    match native_namespace_prop_override_bits(module, prop) {
        Some(bits) => bits == DELETED,
        None => crate::process::worker_restrictions::worker_process_member_restricted(module, prop),
    }
}

/// pi boot blocker: a user write to a builtin namespace member must win every
/// subsequent NAME-KEYED read, no matter which lowering performed the store.
/// Today the stores are split: computed writes (`process[k] = fn`) land in
/// `NATIVE_NAMESPACE_PROP_OVERRIDES` via `nm_field_set_override`, while static
/// writes (`process.chdir = fn`) reach the generic store path and land as an
/// OWN dynamic field on the canonical namespace object. The name-keyed read
/// entries (`js_native_module_property_by_name`,
/// `js_native_module_esm_export_value_vt`) carry no object pointer, so they only
/// consulted the override table — a static write was invisible to them and
/// the read handed back the canonical BOUND_METHOD closure again. graceful-fs
/// then did `Object.setPrototypeOf(process.chdir, chdir)` with the SAME
/// closure on both sides and pi's boot died on the resulting (correct)
/// "Cyclic __proto__ value" self-set rejection. Consult BOTH stores. This
/// never CREATES a namespace: if none was ever built, no user store can have
/// landed on one.
pub(crate) fn native_namespace_user_value(module: &str, prop: &str) -> Option<f64> {
    let namespace = NATIVE_MODULE_NAMESPACES.with(|cache| cache.borrow().get(module).copied());
    if let Some(bits) = namespace {
        if let Some(value) = unsafe {
            super::field_get_set::own_accessor_get_by_bytes(
                (bits & crate::value::POINTER_MASK) as *const ObjectHeader,
                prop.as_bytes(),
                f64::from_bits(bits),
            )
        } {
            return Some(f64::from_bits(value.bits()));
        }
    }
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

pub(crate) fn is_worker_messaging_constructor(property: &str) -> bool {
    matches!(
        property,
        "MessageChannel" | "MessagePort" | "BroadcastChannel"
    )
}

pub(crate) fn publish_worker_messaging_constructor(
    global: *mut ObjectHeader,
    property: &str,
    value: f64,
) {
    if !is_worker_messaging_constructor(property)
        || !super::global_this::fetch_globals::is_thread_realm_global(global)
    {
        return;
    }
    crate::gc::ensure_gc_initialized();
    let key = format!("worker_threads\0{property}");
    NATIVE_CALLABLE_EXPORTS.with(|cache| {
        cache.borrow_mut().insert(key, value.to_bits());
    });
    crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
}

pub(crate) fn worker_messaging_constructor_value(module: &str, property: &str) -> Option<f64> {
    if module != "worker_threads" || !is_worker_messaging_constructor(property) {
        return None;
    }
    // Birth publishes the original value; reading a mutable global here would
    // let reassignment or an accessor redefine a core-module export.
    crate::object::js_get_global_this();
    let key = format!("worker_threads\0{property}");
    NATIVE_CALLABLE_EXPORTS.with(|cache| cache.borrow().get(&key).copied().map(f64::from_bits))
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
pub use perry_abi::NATIVE_MODULE_CLASS_ID;
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
/// every binary, `-dead_strip` notwithstanding. The vtable is a FACT OF THE
/// SHAPE of every namespace-class object (NATIVE_MODULE_CLASS_ID): the birth
/// records its module's vtable as a brand on the shape record
/// (`perry_abi::native_vtable_brand`), every transition carries it, and the
/// generic paths read it off the receiver ([`object_native_vtable`]). Only a
/// birth names a vtable, so a program that never creates a namespace lets the
/// linker drop the tables wholesale.
pub struct NativeModuleVtable {
    pub(crate) dispatch: unsafe fn(*const ObjectHeader, &str, *const f64, usize) -> f64,
    pub(crate) get_own_field:
        unsafe fn(*const ObjectHeader, *const crate::StringHeader) -> Option<JSValue>,
    pub(crate) own_keys_array:
        unsafe fn(*const ObjectHeader) -> Option<*mut crate::array::ArrayHeader>,
    /// Is `key` an enumerable own export of the namespace `module` names?
    pub(crate) has_enumerable_key: fn(&'static NativeModuleVtable, &str, &str) -> bool,
    /// The module's enumerable export keys in Node's order, or `None` for a
    /// module with no key table (its namespace enumerates its key list).
    pub(crate) keys: fn(&'static NativeModuleVtable) -> Option<ModuleKeys>,
    /// The module's export keys in Node's order, as its item-level
    /// `realm_decl! { namespace ... }` declaration lists them; empty for a
    /// module still declared in the native birth block.
    pub(crate) module_keys: &'static [&'static [u8]],
    /// The vtable of the module's CommonJS default namespace (`<mod>.default`),
    /// `None` when the module has none: the default export is born from the
    /// module's own vtable, never from its name.
    pub(crate) default_namespace: Option<&'static NativeModuleVtable>,
    /// On a CommonJS default namespace's vtable (`<mod>.default`): the vtable
    /// of the module that declares it, `None` on every other vtable. A
    /// `<mod>.default` callable dispatches through the base namespace and
    /// births it with this vtable. The declaring module's
    /// `default = NM_X_DEFAULT;` clause sets both links.
    pub(crate) base_namespace: Option<fn() -> &'static NativeModuleVtable>,
}

/// The `base_namespace` of a vtable no `default = ...;` clause names: `None`.
/// A namespace declaration's vtable reads `<NM_X>::BASE_NAMESPACE`; the
/// declaring module's `default = NM_X;` clause gives `NM_X` an inherent
/// `BASE_NAMESPACE`, which takes precedence over this blanket one.
#[doc(hidden)]
pub trait NoBaseNamespace {
    const BASE_NAMESPACE: Option<fn() -> &'static NativeModuleVtable> = None;
}

impl<T: ?Sized> NoBaseNamespace for T {}

/// A module's enumerable export keys: its declared keys, then the keys a
/// derived namespace appends (the deprecated `constants` module is
/// `constants.default` plus `default`).
#[derive(Clone, Copy)]
pub(crate) struct ModuleKeys {
    pub head: &'static [&'static [u8]],
    pub tail: &'static [&'static [u8]],
}

impl ModuleKeys {
    pub(crate) const fn of(head: &'static [&'static [u8]]) -> Self {
        Self { head, tail: &[] }
    }

    pub(crate) fn iter(self) -> impl Iterator<Item = &'static [u8]> {
        self.head.iter().chain(self.tail.iter()).copied()
    }

    pub(crate) fn len(self) -> usize {
        self.head.len() + self.tail.len()
    }

    pub(crate) fn contains(self, key: &[u8]) -> bool {
        self.head.contains(&key) || self.tail.contains(&key)
    }
}

fn declared_module_keys(vt: &'static NativeModuleVtable) -> Option<ModuleKeys> {
    Some(ModuleKeys::of(vt.module_keys))
}

fn no_module_keys(_: &'static NativeModuleVtable) -> Option<ModuleKeys> {
    None
}

/// The vtable an item-level namespace declaration completes with its own
/// `module_keys`.
pub(crate) const NATIVE_MODULE_VTABLE_BASE: NativeModuleVtable = NativeModuleVtable {
    dispatch: dispatch_native_module_method,
    get_own_field: vt_get_own_field,
    own_keys_array: vt_own_keys_array,
    has_enumerable_key: native_module_has_enumerable_key,
    keys: declared_module_keys,
    module_keys: &[],
    default_namespace: None,
    base_namespace: None,
};

/// The vtable of a namespace whose module declares no key table (the
/// modules of the native birth block, and the runtime's instance objects
/// that share the namespace class).
#[export_name = "perry_nm_vt_NATIVE_MODULE_VTABLE_IMPL"]
pub(crate) static NATIVE_MODULE_VTABLE_IMPL: NativeModuleVtable = NativeModuleVtable {
    keys: no_module_keys,
    ..NATIVE_MODULE_VTABLE_BASE
};

/// The vtable `obj`'s shape records, or `None` when `obj` is no namespace
/// object (generic paths then fall through to their default behavior).
///
/// # Safety
/// `obj` is a live `ObjectHeader`.
#[inline]
pub(crate) unsafe fn object_native_vtable(
    obj: *const ObjectHeader,
) -> Option<&'static NativeModuleVtable> {
    let shape = super::shapes::object_shape_descriptor(obj)?;
    let address = perry_abi::native_vtable_brand::native_vtable_of(shape.brands())?;
    // SAFETY: only `set_native_namespace_keys` and the image-row registration
    // produce this band, each from a `&'static NativeModuleVtable`.
    Some(&*(address as *const NativeModuleVtable))
}

/// The vtable the namespace value `ns`'s shape records, or `None` when `ns`
/// is no namespace object (0.0 included).
pub(crate) fn namespace_vtable(ns: f64) -> Option<&'static NativeModuleVtable> {
    let value = JSValue::from_bits(ns.to_bits());
    if !value.is_pointer() {
        return None;
    }
    let obj = (ns.to_bits() & crate::value::POINTER_MASK) as *const ObjectHeader;
    if obj.is_null() {
        return None;
    }
    // SAFETY: a pointer-tagged value is a live heap object.
    unsafe { object_native_vtable(obj) }
}

/// Set the newborn namespace `obj`'s key list `key_list`: its first shape
/// is born carrying `vt` (the vtable's root shape for these keys), so no
/// brand transition ever runs and every later transition carries the brand.
/// The publish can collect: returns `obj` re-resolved.
///
/// # Safety
/// `obj` is a live newborn namespace-class `ObjectHeader` with no key list;
/// `key_list` is its freshly built, exclusively owned key list.
pub(crate) unsafe fn set_native_namespace_keys(
    obj: *mut ObjectHeader,
    key_list: *mut crate::array::ArrayHeader,
    vt: &'static NativeModuleVtable,
) -> *mut ObjectHeader {
    super::arm_nm_namespace_ops(&NM_NAMESPACE_OPS_IMPL);
    // A brand word is never 0: it carries `NATIVE_VTABLE_BRAND`.
    let brand = perry_abi::native_vtable_brand::native_vtable_brand(
        vt as *const NativeModuleVtable as usize,
    );
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    // `js_object_set_keys`'s prelude: a shared-layout receiver publishes the
    // canonical list for these keys.
    let keys = super::ObjectKeys::owned(key_list);
    let len = keys.count();
    let proof = super::canonical_keys::SharedLayout::of_receiver(obj);
    let (published, _) = obj_handle.across_mut::<ObjectHeader, _>(|| match proof {
        Some(proof) => super::canonical_keys::canonicalize(&proof, key_list, len).view(),
        None => keys,
    });
    obj_handle.with_mut_ptr::<ObjectHeader, _>(|obj| {
        debug_assert!(
            len <= super::object_live_slot_count(obj),
            "a namespace's keys fit its birth slots (no spill reserve)"
        );
        super::shapes::birth_publish_branded_keys(obj, published, brand);
    });
    // The publish can collect: hand back the receiver's current address.
    obj_handle.with_mut_ptr::<ObjectHeader, _>(|obj| {
        debug_assert!(
            object_native_vtable(obj).is_some_and(|have| std::ptr::eq(have, vt)),
            "a namespace shape must carry exactly its module vtable"
        );
        obj
    })
}

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
    member: super::descriptors::nm_member,
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

/// Is `key` an enumerable own export of namespace `obj` (of `module_name`)?
///
/// # Safety
/// `obj` is a live `ObjectHeader`.
pub(crate) unsafe fn namespace_has_enumerable_key(
    obj: *const ObjectHeader,
    module_name: &str,
    key: &str,
) -> bool {
    object_native_vtable(obj).is_some_and(|vt| (vt.has_enumerable_key)(vt, module_name, key))
}

/// Arm the generic-path namespace ops (see `nm_namespace_hooks`). Called by
/// the paths that mint native-module-backed values without a namespace
/// object (bound callables, selected exports); a namespace birth arms them in
/// [`set_native_namespace_keys`].
pub(crate) fn install_native_module_vtable() {
    super::arm_nm_namespace_ops(&NM_NAMESPACE_OPS_IMPL);
}

/// Route a NATIVE_MODULE_CLASS_ID method call through the vtable its shape
/// records. Every namespace-class birth brands the shape, so a receiver
/// without one is unreachable in practice.
#[inline]
pub(crate) unsafe fn call_native_module_dispatch_hook(
    obj: *const ObjectHeader,
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    match object_native_vtable(obj) {
        Some(vt) => (vt.dispatch)(obj, method_name, args_ptr, args_len),
        None => {
            debug_assert!(false, "native-module method call on an unbranded receiver");
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
    }
}

/// One row of the program's `(name, vtable)` array: the namespaces a program
/// with a dynamic `require(expr)` / `getBuiltinModule(id)` can reach by a
/// runtime string. Codegen emits the array (`@perry_native_modules`) only in
/// such programs; every other birth names its vtable.
#[repr(C)]
pub struct NativeModuleRow {
    name_ptr: *const u8,
    /// `u64` on every target: the row is `{ ptr, i64, ptr }` in the image.
    name_len: u64,
    vt: &'static NativeModuleVtable,
}

static NATIVE_MODULE_ROWS: std::sync::atomic::AtomicPtr<NativeModuleRow> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());
static NATIVE_MODULE_ROW_COUNT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Program entry: the codegen-emitted `(name, vtable)` array of a program with
/// a dynamic require. The array is a constant of the program image.
#[no_mangle]
pub extern "C" fn js_nm_set_module_table(rows: *const NativeModuleRow, count: usize) {
    NATIVE_MODULE_ROW_COUNT.store(count, std::sync::atomic::Ordering::Relaxed);
    NATIVE_MODULE_ROWS.store(rows as *mut _, std::sync::atomic::Ordering::Release);
}

/// Test-only: run `f` with the `(name, vtable)` rows a program with a dynamic
/// import or require installs at its entry, then put the previous table back.
#[cfg(test)]
pub(crate) fn with_test_module_rows<R>(
    rows: &[(&'static str, &'static NativeModuleVtable)],
    f: impl FnOnce() -> R,
) -> R {
    use std::sync::atomic::Ordering;
    let rows: &'static [NativeModuleRow] = Box::leak(
        rows.iter()
            .map(|(name, vt)| NativeModuleRow {
                name_ptr: name.as_ptr(),
                name_len: name.len() as u64,
                vt,
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );
    let saved = (
        NATIVE_MODULE_ROWS.load(Ordering::Acquire),
        NATIVE_MODULE_ROW_COUNT.load(Ordering::Relaxed),
    );
    js_nm_set_module_table(rows.as_ptr(), rows.len());
    let result = f();
    js_nm_set_module_table(saved.0, saved.1);
    result
}

/// The vtable of the namespace a runtime string `module_name` reaches: its row
/// in the program's array, else `None` (the program never names the module).
pub(crate) fn native_module_vtable_by_name(
    module_name: &str,
) -> Option<&'static NativeModuleVtable> {
    let module_name = normalize_native_module_alias(module_name);
    let rows = NATIVE_MODULE_ROWS.load(std::sync::atomic::Ordering::Acquire);
    if !rows.is_null() {
        let count = NATIVE_MODULE_ROW_COUNT.load(std::sync::atomic::Ordering::Relaxed);
        // SAFETY: the array and its name bytes are constants of the image.
        let rows = unsafe { std::slice::from_raw_parts(rows, count) };
        for row in rows {
            let name = unsafe { std::slice::from_raw_parts(row.name_ptr, row.name_len as usize) };
            if name == module_name.as_bytes() {
                return Some(row.vt);
            }
        }
    }
    None
}

/// The namespace of a module named by a runtime string: the module's
/// namespace when one is born, else one born with the vtable the program's
/// row names. A module with no row is one the program never names, and its
/// namespace is `undefined`: no birth takes a vtable from a name.
pub(crate) fn native_namespace_by_name(module_name: &str) -> f64 {
    match native_module_vtable_by_name(module_name) {
        Some(vt) => create_native_module_namespace(vt, module_name),
        None => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}

/// The namespace of `module_name` born with the vtable its birth site names.
/// A site that names its module statically passes the module's vtable; the
/// shape carries it from here on. Every caller passes the bytes of a Rust
/// `&str` (codegen interns the specifier), so they are UTF-8 by construction.
#[no_mangle]
pub extern "C" fn js_create_native_module_namespace_vt(
    vt: &'static NativeModuleVtable,
    module_name_ptr: *const u8,
    module_name_len: usize,
) -> f64 {
    let module_name = unsafe {
        let bytes = std::slice::from_raw_parts(module_name_ptr, module_name_len);
        debug_assert!(
            std::str::from_utf8(bytes).is_ok(),
            "native module name is not UTF-8"
        );
        std::str::from_utf8_unchecked(bytes)
    };
    create_native_module_namespace(vt, module_name)
}

/// The namespace of `module_name` born with `vt`, the vtable the caller names.
pub(crate) fn create_native_module_namespace(
    vt: &'static NativeModuleVtable,
    module_name: &str,
) -> f64 {
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
    crate::realm_decl! { "%NativeNamespace:assert%", provider = native, native_namespace = "assert", births = [Object]; "%NativeNamespace:buffer.Buffer%", provider = native, native_namespace = "buffer.Buffer", births = [Object]; "%NativeNamespace:constants%", provider = native, native_namespace = "constants", vtable = crate::object::native_module::module_keys::NM_CONSTANTS, births = [Object]; "%NativeNamespace:crypto.KeyObject%", provider = native, native_namespace = "crypto.KeyObject", births = [Object]; "%NativeNamespace:crypto.Certificate%", provider = native, native_namespace = "crypto.Certificate", births = [Object]; "%NativeNamespace:inspector.DOMStorage%", provider = native, native_namespace = "inspector.DOMStorage", vtable = crate::object::native_module::namespaces::inspector::NM_INSPECTOR_DOMSTORAGE, births = [Object]; "%NativeNamespace:inspector.NetworkResources%", provider = native, native_namespace = "inspector.NetworkResources", vtable = crate::object::native_module::namespaces::inspector::NM_INSPECTOR_NETWORKRESOURCES, births = [Object]; "node:@lydell/node-pty", provider = native, native_namespace = "node-pty", vtable = crate::object::native_module::module_keys::NM_NODE_PTY, births = [Array, Error, Function, Object]; "node:@parcel/watcher", provider = native, births = [Array, Error, Function, Object]; "node:@parcel/watcher-darwin-arm64", provider = native, births = [Array, Error, Function, Object]; "node:@parcel/watcher-darwin-x64", provider = native, births = [Array, Error, Function, Object]; "node:@parcel/watcher-linux-arm64-glibc", provider = native, births = [Array, Error, Function, Object]; "node:@parcel/watcher-linux-arm64-musl", provider = native, births = [Array, Error, Function, Object]; "node:@parcel/watcher-linux-x64-glibc", provider = native, births = [Array, Error, Function, Object]; "node:@parcel/watcher-linux-x64-musl", provider = native, births = [Array, Error, Function, Object]; "node:@parcel/watcher-win32-arm64", provider = native, births = [Array, Error, Function, Object]; "node:@parcel/watcher-win32-x64", provider = native, births = [Array, Error, Function, Object]; "node:@perryts/pdf", provider = native, births = [Array, Error, Function, Object]; "node:__disposable__", provider = native, births = [Array, Error, Function, Object]; "node:argon2", provider = native, births = [Array, Error, Function, Object]; "node:bcrypt", provider = native, births = [Array, Error, Function, Object]; "node:better-sqlite3", provider = native, births = [Array, Error, Function, Object]; "node:buffer.Buffer", provider = native, births = [Array, Error, Function, Object]; "node:bun", provider = native, vtable = crate::object::native_module::module_keys::NM_BUN, births = [Array, Buffer, Error, Function, Object, Promise, /* Bun.spawn stdio pipes are net.Socket objects (child_process::reactor::js_child_process_spawn_streams): node:net */ "node:net"]; "node:bun-pty", provider = native, native_namespace = "node-pty", vtable = crate::object::native_module::module_keys::NM_NODE_PTY, births = [Array, Error, Function, Object]; "node:bun:ffi", provider = native, vtable = crate::object::native_module::module_keys::NM_BUN_FFI, births = [Array, Error, Function, Object]; "node:bun:jsc", provider = native, vtable = crate::object::native_module::module_keys::NM_BUN_JSC, births = [Array, Error, Function, Object]; "node:bun:sqlite", provider = native, births = [Array, Error, Function, Object]; "node:cheerio", provider = native, births = [Array, Error, Function, Object]; "node:ethers", provider = native, births = [Array, Error, Function, Object]; "node:ffi", provider = native, vtable = crate::object::native_module::module_keys::NM_FFI, births = [Array, Error, Function, Object]; "node:lodash", provider = native, births = [Array, Error, Function, Object]; "node:node-fetch", provider = native, births = [Array, Error, Function, Headers, Object, Promise, Request, Response, fetch]; "node:node-pty", provider = native, vtable = crate::object::native_module::module_keys::NM_NODE_PTY, births = [Array, Error, Function, Object]; "node:nodemailer", provider = native, births = [Array, Error, Function, Object]; "node:perry", provider = native, births = [Array, Error, Function, Object]; "node:perry/ads", provider = native, births = [Array, Error, Function, Object]; "node:perry/audio", provider = native, births = [Array, Error, Function, Object]; "node:perry/background", provider = native, births = [Array, Error, Function, Object]; "node:perry/component", provider = native, births = [Array, Error, Function, Object]; "node:perry/compose", provider = native, births = [Array, Error, Function, Object]; "node:perry/container", provider = native, births = [Array, Error, Function, Object]; "node:perry/container-compose", provider = native, births = [Array, Error, Function, Object]; "node:perry/gc", provider = native, births = [Array, Error, Function, Object]; "node:perry/i18n", provider = native, births = [Array, Error, Function, Object]; "node:perry/ios", provider = native, births = [Array, Error, Function, Object]; "node:perry/media", provider = native, births = [Array, Error, Function, Object]; "node:perry/native", provider = native, births = [Array, Error, Function, Object]; "node:perry/plugin", provider = native, births = [Array, Error, Function, Object]; "node:perry/system", provider = native, births = [Array, Error, Function, Object]; "node:perry/thread", provider = native, births = [Array, Error, Function, Object]; "node:perry/tui", provider = native, births = [Array, Error, Function, Object]; "node:perry/ui", provider = native, births = [Array, Error, Function, Object]; "node:perry/updater", provider = native, births = [Array, Error, Function, Object]; "node:perry/widget", provider = native, births = [Array, Error, Function, Object]; "node:perry/workloads", provider = native, births = [Array, Error, Function, Object]; "node:perry/yoga", provider = native, births = [Array, Error, Function, Object]; "node:sharp", provider = native, births = [Array, Error, Function, Object]; "node:sqlite", provider = native, vtable = crate::object::native_module::module_keys::NM_SQLITE, births = [Array, Error, Function, Object]; "node:typescript", provider = native, births = [Array, Error, Function, Object]; "node:undici", provider = native, births = [Array, ArrayBuffer, Error, Function, Headers, Object, Promise, Request, Response, fetch]; "node:util.types", provider = native, vtable = crate::object::native_module::namespaces::util::NM_UTIL_TYPES, births = [Array, Error, Function, Object]; "node:ws", provider = native, births = [Array, Error, Function, Object]; }
    // Create an object with one field to store the module name
    let obj = crate::realm_decl! { birth js_object_alloc(NATIVE_MODULE_CLASS_ID, 1); realm { namespaces = native_namespaces; } };

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
    // The module's vtable is a fact of the namespace's shape, from its birth.
    let vt = if module_name == "vm" && crate::node_vm::vm_modules_enabled() {
        native_module::vm_modules_vtable()
    } else {
        vt
    };
    let obj = unsafe { set_native_namespace_keys(obj, keys_array, vt) };

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
    crate::object::native_module::create_native_module_namespace(
        &crate::object::native_module::namespaces::crypto::NM_CRYPTO_WEBCRYPTO,
        "crypto.webcrypto",
    )
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

/// The `string_decoder` namespace (perry-stdlib reads its `StringDecoder`).
pub fn string_decoder_namespace() -> f64 {
    create_native_module_namespace(
        &crate::object::native_module::namespaces::string_decoder::NM_STRING_DECODER,
        "string_decoder",
    )
}

pub(crate) fn subtle_crypto_namespace() -> f64 {
    crate::object::native_module::create_native_module_namespace(
        &crate::object::native_module::namespaces::crypto::NM_CRYPTO_SUBTLE,
        "crypto.subtle",
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
/// [`js_create_native_module_namespace_vt`]).
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
        None,
    )
}

/// [`js_native_module_property_by_name`] from a codegen site: `vt` is the
/// vtable of the module's own namespace (the read holds no receiver).
#[no_mangle]
pub unsafe extern "C" fn js_native_module_property_by_name_vt(
    vt: &'static NativeModuleVtable,
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
        Some(vt),
    )
}

unsafe fn native_module_property_by_name_impl(
    module_name_ptr: *const u8,
    module_name_len: usize,
    property_name_ptr: *const u8,
    property_name_len: usize,
    consult_overrides: bool,
    base: Option<&'static NativeModuleVtable>,
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
        return cjs_default_export_value(
            "process",
            Some(&crate::object::native_module::namespaces::process::NM_PROCESS),
        )
        .unwrap_or_else(|| {
            crate::object::native_module::create_native_module_namespace(
                &crate::object::native_module::namespaces::process::NM_PROCESS,
                "process",
            )
        });
    }
    let module_name = if module_name == "process.namespace" {
        "process"
    } else {
        module_name
    };
    if native_namespace_member_absent(module_name, property_name) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if matches!(module_name, "process" | "process.default") {
        if let Some(value) = crate::process::process_ipc_property(property_name) {
            return value;
        }
    }
    if matches!(module_name, "perf_hooks" | "perf_hooks.default") && property_name == "constants" {
        // Its OWN tag. Sharing the `perf_hooks` tag made every read of the
        // constants object resolve against the MODULE's surface, so
        // `Object.keys(constants)` enumerated the export list instead of the
        // `NODE_PERFORMANCE_GC_*` table.
        return crate::object::native_module::create_native_module_namespace(
            &crate::object::native_module::namespaces::perf_hooks::NM_PERF_HOOKS_CONSTANTS,
            "perf_hooks.constants",
        );
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
        return cjs_default_export_value(
            "dns/promises",
            Some(&crate::object::native_module::namespaces::dns::NM_DNS_PROMISES),
        )
        .unwrap_or_else(|| {
            crate::object::native_module::create_native_module_namespace(
                &crate::object::native_module::namespaces::dns::NM_DNS_PROMISES,
                "dns/promises",
            )
        });
    }

    // #5731 — `perry.isStandaloneExecutable` value export (always `true` at
    // runtime). `embeddedFiles` / `readEmbedded` are callable exports dispatched
    // via the native call table, not value reads.
    if module_name == "perry" && property_name == "isStandaloneExecutable" {
        return crate::embedded::is_standalone_executable_value();
    }

    if module_name == "util" && property_name == "debug" {
        return bound_native_callable_export_value(
            &crate::object::native_module::namespaces::util::NM_UTIL,
            "util",
            "debuglog",
        );
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
                return crate::object::native_module::create_native_module_namespace(
                    &crate::object::native_module::module_keys::NM_BUN_ANT,
                    "bun.ant",
                );
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
                    &crate::object::native_module::module_keys::NM_BUN,
                    "bun",
                    "hash",
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
        return bound_native_callable_export_value(
            &crate::object::native_module::namespaces::cluster::NM_CLUSTER,
            "cluster.default",
            property_name,
        );
    }

    if let Some(val) = get_native_module_constant(module_name, property_name, 0.0, base) {
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
        // The program names no row for a module it never imports: nothing to bind.
        return match base {
            Some(vt) => bound_native_callable_export_value(vt, module_name, property_name),
            None => f64::from_bits(crate::value::TAG_UNDEFINED),
        };
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

fn native_module_export_value(
    module: f64,
    property: f64,
    observe_namespace_writes: bool,
    base: Option<&'static NativeModuleVtable>,
) -> f64 {
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
    let live = native_module_constant_is_live(&module, &property)
        || (observe_namespace_writes
            && matches!(
                module.as_str(),
                "process" | "process.namespace" | "process.default"
            )
            && property == "title");
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
            base,
        )
    };
    if value.to_bits() == crate::value::TAG_UNDEFINED {
        if observe_namespace_writes && is_namespace_inherited_method(&property) {
            return object_prototype_member(&property);
        }
        return value;
    }
    if live {
        return value;
    }
    NATIVE_ESM_EXPORT_VALUES.with(|values| {
        values.borrow_mut().insert(key, value.to_bits());
    });
    crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
    value
}

/// `Object.prototype` methods a native-module default object inherits rather
/// than exports (`buffer.hasOwnProperty`, which safer-buffer calls). Neither
/// a read nor a call by one of these names belongs to the module's export
/// table or dispatch bucket, which answer a name they lack with `undefined`.
pub(crate) fn is_namespace_inherited_method(name: &str) -> bool {
    matches!(
        name,
        "toString"
            | "toLocaleString"
            | "valueOf"
            | "hasOwnProperty"
            | "isPrototypeOf"
            | "propertyIsEnumerable"
            | "constructor"
    )
}

fn object_prototype_member(name: &str) -> f64 {
    let (_, proto) = crate::object::ensure_object_intrinsics();
    if proto.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = js_object_get_field_by_name(proto, key);
    f64::from_bits(value.bits())
}

/// Mutable property read used by native-module default and namespace objects.
/// User writes to the CommonJS namespace are observable immediately here.
/// `vt` is the vtable of the module's own namespace, named by the codegen site.
#[no_mangle]
pub extern "C" fn js_native_module_esm_export_value_vt(
    vt: &'static NativeModuleVtable,
    module: f64,
    property: f64,
) -> f64 {
    native_module_export_value(module, property, true, Some(vt))
}

/// Snapshot-backed value used for named ESM imports from builtins. CommonJS
/// namespace writes stay isolated until `syncBuiltinESMExports()` copies them.
/// `vt` is the vtable of the module's own namespace, named by the codegen site.
#[no_mangle]
pub extern "C" fn js_native_module_named_esm_export_value_vt(
    vt: &'static NativeModuleVtable,
    module: f64,
    property: f64,
) -> f64 {
    native_module_export_value(module, property, false, Some(vt))
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
    bound_native_callable_export_value(
        &crate::object::native_module::namespaces::module::NM_MODULE,
        "module",
        "Module",
    )
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
                None,
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

    // Check for known constant properties first
    if let Some(val) = unsafe {
        get_native_module_constant(
            &module_name,
            property_name,
            namespace.get_nanbox_f64(),
            None,
        )
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
        let value = bound_native_callable_export_value(
            namespace_vtable(namespace.get_nanbox_f64()).unwrap_or(&NATIVE_MODULE_VTABLE_IMPL),
            &module_name,
            property_name,
        );
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

    bound_native_callable_export_value(
        namespace_vtable(namespace.get_nanbox_f64()).unwrap_or(&NATIVE_MODULE_VTABLE_IMPL),
        &module_name,
        property_name,
    )
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
///   slot 0: receiver (NaN-boxed instance, or the class constructor for a
///           static method)
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

/// Materialize a raw ClassBody method without a closure-convention entry.
/// The body and calling convention come from its declaration.
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
    class_ref_id(captured).is_some() || class_registry::is_class_object_value(captured)
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
            // param prologue. See `dispatch_bound_method`.
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
/// we enumerate the module's export names (its shape's vtable keys, the
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
    let Some(vt) = object_native_vtable(src) else {
        return false;
    };
    let Some(keys) = (vt.keys)(vt) else {
        return false;
    };
    let Some(module_name) = read_native_module_name(src) else {
        return false;
    };
    let include_permission = matches!(
        module_name.as_str(),
        "process" | "process.namespace" | "process.default"
    ) && crate::process::process_permission_enabled();

    for key_bytes in keys
        .iter()
        .chain(include_permission.then_some(b"permission" as &[u8]))
    {
        if native_namespace_member_absent(
            &module_name,
            std::str::from_utf8(key_bytes).unwrap_or(""),
        ) {
            continue;
        }
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

#[cfg(test)]
mod namespace_accessor_tests;
#[cfg(test)]
#[path = "native_module/namespace_vtable_shape_tests.rs"]
mod namespace_vtable_shape_tests;

#[cfg(test)]
mod builtin_parent_tests;

#[cfg(test)]
mod realm_native_storage_tests {
    include!(concat!(env!("OUT_DIR"), "/realm_native_storage_test.rs"));
}

#[cfg(test)]
mod realm_native_owner_truth_tests {
    include!(concat!(
        env!("OUT_DIR"),
        "/realm_native_owner_truth_test.rs"
    ));
}
