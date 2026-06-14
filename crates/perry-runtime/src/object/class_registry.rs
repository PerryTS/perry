//! Class method vtable registry — enables runtime dispatch for
//! interface-typed and dynamically-typed method calls. Each class
//! registers its methods, getters, and setters at startup;
//! `js_native_call_method` / `js_dynamic_object_get_property` look up
//! the vtable by the object's `class_id` when static dispatch isn't
//! possible. Also home for the per-callsite inline cache
//! (`vtable_ic_*` / `call_vtable_method`) and the parent-chain
//! registration helpers used by codegen.
//!
//! Split out of `object/mod.rs` (issue #1103). Pure relocation — no
//! logic changes.

pub use super::class_handles::{
    event_emitter_async_resource_handle_probe, event_emitter_get_domain,
    event_emitter_handle_probe, event_emitter_on, event_emitter_set_domain,
    fetch_handle_kind_probe, handle_method_dispatch, handle_own_property_names_dispatch,
    handle_property_dispatch, handle_property_set_dispatch, handle_prototype_dispatch,
    js_register_event_emitter_async_resource_handle_probe, js_register_event_emitter_get_domain,
    js_register_event_emitter_handle_probe, js_register_event_emitter_on,
    js_register_event_emitter_set_domain, js_register_fetch_handle_kind_probe,
    js_register_handle_method_dispatch, js_register_handle_own_property_names_dispatch,
    js_register_handle_property_dispatch, js_register_handle_property_set_dispatch,
    js_register_handle_prototype_dispatch, js_register_net_socket_handle_probe,
    js_register_stream_handle_kind_probe, js_register_stream_handle_probe, net_socket_handle_probe,
    stream_handle_kind_probe, stream_handle_probe, EventEmitterAsyncResourceHandleProbeFn,
    EventEmitterGetDomainFn, EventEmitterHandleProbeFn, EventEmitterOnFn, EventEmitterSetDomainFn,
    FetchHandleKindProbeFn, HandleMethodDispatchFn, HandleOwnPropertyNamesDispatchFn,
    HandlePropertyDispatchFn, HandlePropertySetDispatchFn, HandlePrototypeDispatchFn,
    NetSocketHandleProbeFn, StreamHandleKindProbeFn, StreamHandleProbeFn,
};
use super::*;

thread_local! {
    static CLASS_DELETED_KEYS: std::cell::RefCell<std::collections::HashMap<u32, std::collections::HashSet<String>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

fn is_non_constructable_builtin_function_value(value: f64) -> bool {
    super::native_module::builtin_closure_is_non_constructable_value(value)
}

/// True when `value` is a bound native-module method/export closure
/// (`BOUND_METHOD_FUNC_PTR` trampoline — what a `require('stream').Writable`
/// property read produces). These represent real Node classes/functions and
/// must be accepted as `extends` targets.
fn is_bound_native_method_closure_value(value: f64) -> bool {
    // Gate on the native-module metadata, not the raw BOUND_METHOD_FUNC_PTR
    // trampoline: reified `Function.prototype.{bind,call,apply}` values
    // (`reify_function_method_value`) share that trampoline but are NOT native
    // constructors, so matching the sentinel alone would let `class X extends
    // obj.method {}` skip the spec-required TypeError and silently stay
    // parentless. A real native-module export carries a non-empty module name.
    unsafe {
        super::native_module::bound_native_callable_module_and_method(value)
            .map(|(module, _)| !module.is_empty())
            .unwrap_or(false)
    }
}

fn throw_non_constructable_builtin_function() -> ! {
    super::object_ops::throw_object_type_error(b"Function is not a constructor")
}

pub(crate) fn class_mark_key_deleted(class_id: u32, key: &str) {
    if class_id == 0 {
        return;
    }
    CLASS_DELETED_KEYS.with(|m| {
        m.borrow_mut()
            .entry(class_id)
            .or_default()
            .insert(key.to_string());
    });
}

pub(crate) fn class_is_key_deleted(class_id: u32, key: &str) -> bool {
    CLASS_DELETED_KEYS.with(|m| {
        m.borrow()
            .get(&class_id)
            .map(|keys| keys.contains(key))
            .unwrap_or(false)
    })
}

pub(crate) fn class_dynamic_prop_root_store(class_id: u32, name: String, value: f64) {
    CLASS_DELETED_KEYS.with(|m| {
        if let Some(keys) = m.borrow_mut().get_mut(&class_id) {
            keys.remove(&name);
        }
    });
    CLASS_DYNAMIC_PROPS.with(|m| {
        m.borrow_mut()
            .entry(class_id)
            .or_insert_with(std::collections::HashMap::new)
            .insert(name, value);
    });
    crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
}

/// Own static-field value for a class (no parent-chain walk) — the
/// CLASS_DYNAMIC_PROPS entry codegen registers at module init for every
/// declared static field. Consulted by `getOwnPropertyDescriptor` on a class
/// constructor ref so `verifyProperty(C, "field", …)` sees a real data
/// descriptor (test262 class/elements static-field-declaration & friends).
pub(crate) fn class_own_static_field_value(class_id: u32, name: &str) -> Option<f64> {
    CLASS_DYNAMIC_PROPS.with(|m| {
        m.borrow()
            .get(&class_id)
            .and_then(|props| props.get(name).copied())
    })
}

/// Enumerable own string keys of a class constructor: the static fields (and
/// runtime `C.x = …` assignments) recorded in CLASS_DYNAMIC_PROPS. The built-in
/// `length`/`name`/`prototype` slots and static *methods*/*accessors* are
/// non-enumerable, so they are intentionally excluded — this is exactly the set
/// `Object.keys(C)` / `for (k in C)` must yield. Private (`#`) keys are filtered
/// here too (never reflectable). Returned unsorted; the caller applies ECMA
/// ordering. (test262 class/elements static-field-declaration & friends.)
pub(crate) fn class_own_enumerable_field_names(class_id: u32) -> Vec<String> {
    CLASS_DYNAMIC_PROPS.with(|m| {
        m.borrow()
            .get(&class_id)
            .map(|props| {
                props
                    .keys()
                    .filter(|k| !k.starts_with('#'))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    })
}

pub(crate) fn class_delete_own_dynamic_prop(class_id: u32, name: &str) {
    CLASS_DYNAMIC_PROPS.with(|m| {
        if let Some(props) = m.borrow_mut().get_mut(&class_id) {
            props.remove(name);
        }
    });
}

pub(crate) fn class_prototype_method_value_cache_root_store(
    class_id: u32,
    method_name: String,
    value_bits: u64,
) {
    CLASS_PROTOTYPE_METHOD_VALUES.with(|cache| {
        cache
            .borrow_mut()
            .insert((class_id, method_name), value_bits);
    });
    crate::gc::runtime_write_barrier_root_nanbox(value_bits);
}

// ============================================================================
// Class method vtable registry — enables runtime dispatch for interface-typed
// and dynamically-typed method calls.  Each class registers its methods and
// getters at startup; js_native_call_method / js_dynamic_object_get_property
// look up the vtable by the object's class_id when static dispatch isn't possible.
// ============================================================================

/// Entry in the class method vtable
pub struct VTableMethodEntry {
    pub func_ptr: usize,
    pub param_count: u32,
    pub has_synthetic_arguments: bool,
    /// Trailing user rest param (`method(a, ...rest)`). Distinct from
    /// `has_synthetic_arguments`: the rest slot holds only the args from the
    /// rest position onward, so apply/dynamic dispatch bundles them correctly.
    pub has_rest: bool,
}

/// Per-class vtable with methods, getters, and setters
pub struct ClassVTable {
    pub methods: HashMap<String, VTableMethodEntry>,
    pub getters: HashMap<String, usize>, // getter func_ptr (signature: fn(this_f64) -> f64)
    pub setters: HashMap<String, usize>, // setter func_ptr (signature: fn(this_f64, value_f64) -> f64)
}

/// Global vtable registry: class_id -> vtable
pub static CLASS_VTABLE_REGISTRY: RwLock<Option<HashMap<u32, ClassVTable>>> = RwLock::new(None);

/// #1788: per-class STATIC-method registry: class_id -> { name -> (func_ptr,
/// param_count, has_rest) }. Static methods are emitted as `perry_static_*`
/// (no `this` param — they read `this` from the implicit-this slot) and are
/// NOT in the instance vtable above, so a subclass whose parent is a
/// class-expression value (`class Sub extends make(...) {}`) can't resolve an
/// inherited static method (`Sub.greet()`) at compile time. This table is
/// walked up the class_id parent chain at runtime by
/// `js_class_static_method_call`. `has_rest` marks a trailing rest param
/// (`static pipe(...args)`, effect's `pipe`/`dual`) so the dispatcher bundles
/// the call args into an array for that slot.
pub static CLASS_STATIC_METHODS: RwLock<Option<HashMap<u32, HashMap<String, (usize, u32, bool)>>>> =
    RwLock::new(None);

pub static CLASS_STATIC_ACCESSORS: RwLock<Option<HashMap<u32, HashMap<String, (usize, usize)>>>> =
    RwLock::new(None);

/// Spec `Function.prototype.length` per (class_id, method/accessor name) — the
/// count of formal parameters before the first one with a default or a rest.
/// The vtable only records the *total* param count (needed for call dispatch),
/// which overcounts methods with default-valued params; codegen computes the
/// real `.length` at registration and stashes it here so `C.prototype.m.length`
/// is exact (Test262 .../class/*/dflt-params-trailing-comma).
pub static CLASS_METHOD_BIND_LENGTHS: RwLock<Option<HashMap<(u32, String), u32>>> =
    RwLock::new(None);

/// Default-aware spec `.length` for STATIC methods, keyed (class_id, name).
/// Distinct from `CLASS_METHOD_BIND_LENGTHS` (instance methods) so a class with
/// both `static m(a, b = 1)` and `m(c)` keeps independent lengths instead of
/// colliding on the (class_id, name) key. (Test262 *-method-static
/// dflt-params-trailing-comma.)
pub static CLASS_STATIC_METHOD_BIND_LENGTHS: RwLock<Option<HashMap<(u32, String), u32>>> =
    RwLock::new(None);

pub static CLASS_SYMBOL_METHODS: RwLock<Option<HashMap<(u32, usize, bool), (usize, u32, bool)>>> =
    RwLock::new(None);

pub static CLASS_SYMBOL_ACCESSORS: RwLock<Option<HashMap<(u32, usize, bool), (usize, usize)>>> =
    RwLock::new(None);

/// Set of all registered class ids. Populated at module init by codegen
/// emitting `js_register_class_id(cid)` for every user class — even
/// classes without any methods. Refs #618 / #420 followup.
pub static REGISTERED_CLASS_IDS: RwLock<Option<std::collections::HashSet<u32>>> = RwLock::new(None);

/// Issue #711 part 2: `function Base() {}; Base.prototype = obj` pattern.
/// Effect's `internal/effectable.ts` declares classes via prototype
/// assignment on a plain function, not via `class` syntax. To make
/// `class Derived extends Base {}` walk into `obj`'s methods at dispatch
/// time, we model this as a synthetic class:
///   - `js_set_function_prototype(func, obj)` allocates a synthetic
///     class_id (high-bit-set to avoid collision with codegen-assigned
///     ids), stores `func_bits → synthetic_cid` in `FUNCTION_CLASS_IDS`,
///     and `synthetic_cid → obj_ptr` in `CLASS_PROTOTYPE_OBJECTS`.
///   - `js_register_class_parent_dynamic` extends to detect closure
///     parent values, looks up the synthetic class_id, and registers
///     the (child, synthetic) edge in CLASS_REGISTRY.
///   - The method-dispatch chain walk in `js_native_call_method`
///     consults `CLASS_PROTOTYPE_OBJECTS` when it reaches a synthetic
///     class_id: it resolves the method as a regular field lookup on
///     the prototype object and calls it with `this` bound to the
///     receiver.
pub static FUNCTION_CLASS_IDS: RwLock<Option<HashMap<u64, u32>>> = RwLock::new(None);
// Stored as `usize` (raw address) so the map is Send + Sync. The
// pointer is always converted back to `*mut ObjectHeader` at call sites
// (`class_prototype_object` / the dispatch walk) where single-threaded
// usage is guaranteed.
pub static CLASS_PROTOTYPE_OBJECTS: RwLock<Option<HashMap<u32, usize>>> = RwLock::new(None);

/// Lazily materialized `Class.prototype` objects for declared ES classes.
/// These are separate from `CLASS_PROTOTYPE_OBJECTS`: that older table is
/// intentionally overloaded for synthetic prototype sources and static
/// inheritance shortcuts. Declared class prototypes need stable heap identity
/// for `typeof C.prototype`, `Object.getPrototypeOf(new C())`, and
/// `C.prototype.isPrototypeOf(instance)` without perturbing those paths.
pub static CLASS_DECL_PROTOTYPE_OBJECTS: RwLock<Option<HashMap<u32, usize>>> = RwLock::new(None);

/// #36 / #321: maps a child class_id to the raw address of a parent CLOSURE
/// (function value) when `class Child extends <function value> {}`. effect's
/// `class Svc extends Context.Tag("Svc")<...>() {}` extends the function
/// `TagClass` returned by `Tag(id)()`. In JS this sets `Svc.__proto__ =
/// TagClass` so static-property reads on `Svc` (`Svc.key`, `Svc._op`,
/// `Svc[TagTypeId]`) walk to the parent function's own props + ITS static
/// prototype. Perry's existing dynamic-parent path only models OBJECT parents
/// (class-expression values), so this records the closure-parent axis so the
/// class-ref static getters can reach the closure's props and proto chain.
/// Stored as `usize` (raw address) for Send + Sync; converted back at use.
pub static CLASS_PARENT_CLOSURES: RwLock<Option<HashMap<u32, usize>>> = RwLock::new(None);

/// Maps a child class_id to the raw NaN-boxed bits of the parent constructor
/// VALUE that `js_register_class_parent_dynamic` evaluated at class-definition
/// time. For `class X extends _mod.default {}` (the interop ESM
/// default-export-class pattern), the extends expression references a require
/// alias (`_mod`) that is an IIFE-local — bound only in the module-init scope.
/// The decl-time registration evaluates it there correctly, so we stash the
/// resulting value here keyed by the child's class id. `super()` then reads it
/// back via `js_get_dynamic_parent_value` instead of re-evaluating the extends
/// expression inside the constructor (where the IIFE-local alias is NOT
/// captured and the member read would throw "Cannot read properties of
/// undefined"). Stored as raw `u64` bits (Send + Sync), covering both ClassRef
/// (INT32-tagged) and object/closure (POINTER-tagged) parents.
pub static CLASS_DYNAMIC_PARENT_VALUE: RwLock<Option<HashMap<u32, u64>>> = RwLock::new(None);

pub(crate) fn class_prototype_object_root_store(class_id: u32, proto_ptr: *mut ObjectHeader) {
    if class_id == 0 || proto_ptr.is_null() {
        return;
    }
    let mut guard = CLASS_PROTOTYPE_OBJECTS.write().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard.as_mut().unwrap().insert(class_id, proto_ptr as usize);
    crate::gc::runtime_write_barrier_root_raw_ptr(proto_ptr);
}

pub(crate) fn class_decl_prototype_object_root_store(class_id: u32, proto_ptr: *mut ObjectHeader) {
    if class_id == 0 || proto_ptr.is_null() {
        return;
    }
    let mut guard = CLASS_DECL_PROTOTYPE_OBJECTS.write().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard.as_mut().unwrap().insert(class_id, proto_ptr as usize);
    crate::gc::runtime_write_barrier_root_raw_ptr(proto_ptr);
}

pub(crate) fn class_parent_closure_root_store(class_id: u32, closure_addr: usize) {
    if class_id == 0 || closure_addr == 0 {
        return;
    }
    let mut guard = CLASS_PARENT_CLOSURES.write().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard.as_mut().unwrap().insert(class_id, closure_addr);
    crate::gc::runtime_write_barrier_root_raw_ptr(closure_addr as *const u8);
}

/// Look up the parent-closure address recorded for a child class_id, if any.
pub(crate) fn class_parent_closure(class_id: u32) -> Option<usize> {
    CLASS_PARENT_CLOSURES
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(|m| m.get(&class_id).copied()))
}

/// Walk the class parent chain looking for a registered parent-closure edge.
/// `super()` dispatch needs this because the instance's class_id is the
/// MOST-DERIVED class, while the closure-parent edge is keyed by the class
/// that directly `extends <function value>` — possibly an ancestor.
pub(crate) fn parent_closure_in_chain(class_id: u32) -> Option<usize> {
    let mut cid = class_id;
    let mut depth = 0u32;
    while depth < 32 && cid != 0 {
        if let Some(addr) = class_parent_closure(cid) {
            return Some(addr);
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    None
}

/// Reverse lookup: which declared class's `.prototype` is this heap object?
/// Used by `Object.getOwnPropertyDescriptor(C.prototype, name)` to surface
/// vtable accessors as own properties of the prototype object. Linear scan —
/// the table is small (one entry per materialized declared-class prototype)
/// and this only runs on the reflection slow path.
pub(crate) fn class_id_for_decl_prototype_object(ptr: usize) -> Option<u32> {
    if ptr == 0 {
        return None;
    }
    CLASS_DECL_PROTOTYPE_OBJECTS
        .read()
        .ok()?
        .as_ref()?
        .iter()
        .find(|(_, &p)| p == ptr)
        .map(|(k, _)| *k)
}

pub(crate) fn class_decl_prototype_object(class_id: u32) -> *mut ObjectHeader {
    if let Ok(read) = CLASS_DECL_PROTOTYPE_OBJECTS.read() {
        if let Some(map) = read.as_ref() {
            return map.get(&class_id).copied().unwrap_or(0) as *mut ObjectHeader;
        }
    }
    std::ptr::null_mut()
}

fn class_decl_prototype_method_names(class_id: u32) -> Vec<String> {
    let mut names = Vec::new();
    if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
        if let Some(vtable) = registry.as_ref().and_then(|reg| reg.get(&class_id)) {
            names.extend(
                vtable
                    .methods
                    .keys()
                    .filter(|name| *name != "constructor")
                    .cloned(),
            );
        }
    }
    names.sort();
    names.dedup();
    names
}

fn install_class_decl_prototype_method_fields(proto: *mut ObjectHeader, class_id: u32) {
    let proto_value = crate::value::js_nanbox_pointer(proto as i64);
    for name in class_decl_prototype_method_names(class_id) {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let leaked: &'static [u8] = name.as_bytes().to_vec().leak();
        let method = js_class_method_bind(proto_value, leaked.as_ptr(), leaked.len());
        js_object_set_field_by_name(proto, key, method);
        set_builtin_property_attrs(proto as usize, name, PropertyAttrs::new(true, false, true));
    }
}

pub(crate) fn class_decl_prototype_value(class_id: u32) -> f64 {
    if class_id == 0 || class_name_for_id(class_id).is_none() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }

    let existing = class_decl_prototype_object(class_id);
    if !existing.is_null() {
        return crate::value::js_nanbox_pointer(existing as i64);
    }

    let proto = js_object_alloc(class_id, 0);
    if proto.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    invalidate_class_prototype_fast_guards();
    class_decl_prototype_object_root_store(class_id, proto);

    let constructor_key =
        crate::string::js_string_from_bytes(b"constructor".as_ptr(), "constructor".len() as u32);
    js_object_set_field_by_name(
        proto,
        constructor_key,
        class_constructor_ref_value(class_id),
    );
    set_builtin_property_attrs(
        proto as usize,
        "constructor".to_string(),
        PropertyAttrs::new(true, false, true),
    );
    install_class_decl_prototype_method_fields(proto, class_id);

    let parent_proto_bits = get_parent_class_id(class_id)
        .filter(|parent_id| *parent_id != 0 && *parent_id != class_id)
        .and_then(|parent_id| {
            let parent_proto = class_decl_prototype_value(parent_id);
            let parent_bits = parent_proto.to_bits();
            ((parent_bits >> 48) == 0x7FFD).then_some(parent_bits)
        })
        .or_else(global_object_prototype_bits);
    if let Some(bits) = parent_proto_bits {
        super::prototype_chain::object_set_static_prototype(proto as usize, bits);
    }

    crate::value::js_nanbox_pointer(proto as i64)
}

pub(crate) fn class_decl_prototype_value_for_instance_class(class_id: u32) -> Option<f64> {
    if class_id == 0 || class_name_for_id(class_id).is_none() {
        return None;
    }
    let proto = class_decl_prototype_value(class_id);
    ((proto.to_bits() >> 48) == 0x7FFD).then_some(proto)
}

fn global_object_prototype_bits() -> Option<u64> {
    let object_ctor = js_get_global_this_builtin_value(b"Object".as_ptr(), 6);
    let ctor_bits = object_ctor.to_bits();
    if (ctor_bits >> 48) != 0x7FFD {
        return None;
    }
    let ctor_ptr = (ctor_bits & crate::value::POINTER_MASK) as usize;
    if ctor_ptr == 0 {
        return None;
    }
    let proto = crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype");
    let proto_bits = proto.to_bits();
    if (proto_bits >> 48) == 0x7FFD {
        Some(proto_bits)
    } else {
        None
    }
}

pub(crate) fn ensure_function_prototype_object(
    func_value: f64,
    class_id: u32,
) -> *mut ObjectHeader {
    if class_id == 0 {
        return std::ptr::null_mut();
    }
    // A `Temporal.<X>` constructor pre-populates its `prototype` (a real object
    // with the type's accessor getters / methods) during globalThis init and
    // stamps it on the closure's `prototype` dynamic prop — but intentionally
    // NOT in the GC-scanned class-prototype cache (rooting an init-time arena
    // object there dangles across the test-suite's arena-fixture swaps). So when
    // `new Temporal.X()` / a reflective `.prototype` read lands here, return that
    // pre-set object as-is instead of allocating a fresh empty one (which would
    // overwrite the populated prototype). Gated on `temporal_ctor_kind` so the
    // ordinary class-prototype flow (which relies on the cache for method
    // registration) is unaffected.
    if super::global_this::temporal_ctor_kind(func_value).is_some() {
        let fv_bits = func_value.to_bits();
        let fp = (fv_bits & crate::value::POINTER_MASK) as usize;
        if fp != 0 {
            let dyn_proto = crate::closure::closure_get_dynamic_prop(fp, "prototype");
            let dp = JSValue::from_bits(dyn_proto.to_bits());
            if dp.is_pointer() {
                let pp = dp.as_pointer::<ObjectHeader>();
                if !pp.is_null() {
                    return pp as *mut ObjectHeader;
                }
            }
        }
    }
    let existing = class_prototype_object(class_id);
    if !existing.is_null() {
        return existing;
    }

    let proto = js_object_alloc(0, 0);
    if proto.is_null() {
        return proto;
    }

    let constructor_key =
        crate::string::js_string_from_bytes(b"constructor".as_ptr(), "constructor".len() as u32);
    js_object_set_field_by_name(proto, constructor_key, func_value);
    set_builtin_property_attrs(
        proto as usize,
        "constructor".to_string(),
        PropertyAttrs::new(true, false, true),
    );

    if let Some(object_proto_bits) = global_object_prototype_bits() {
        super::prototype_chain::object_set_static_prototype(proto as usize, object_proto_bits);
    }

    class_prototype_object_root_store(class_id, proto);

    // #5024: methods registered before the prototype object materialized
    // (`F.prototype.m = v` typically runs long before any reflective
    // `F.prototype` read) live only in CLASS_PROTOTYPE_METHODS. Backfill
    // them as ordinary own properties so enumeration sees them; later
    // registrations write through via class_prototype_method_root_store.
    let registered: Vec<(String, u64)> = {
        let guard = CLASS_PROTOTYPE_METHODS.read().unwrap();
        guard
            .as_ref()
            .and_then(|map| map.get(&class_id))
            .map(|per_class| per_class.iter().map(|(k, &v)| (k.clone(), v)).collect())
            .unwrap_or_default()
    };
    for (name, value_bits) in registered {
        unsafe { mirror_prototype_method_on_object(proto, &name, value_bits) };
    }

    let func_bits = func_value.to_bits();
    if (func_bits >> 48) == 0x7FFD {
        let func_ptr = (func_bits & crate::value::POINTER_MASK) as usize;
        if func_ptr != 0 {
            crate::closure::closure_set_dynamic_prop(
                func_ptr,
                "prototype",
                crate::value::js_nanbox_pointer(proto as i64),
            );
            set_builtin_property_attrs(
                func_ptr,
                "prototype".to_string(),
                PropertyAttrs::new(true, false, false),
            );
        }
    }

    proto
}

/// Synthetic class id allocator for prototype-object classes. High bit
/// set (0x8000_0000+) to keep them separate from codegen-assigned ids
/// (which start from 1 and grow by module). u32 wraparound is not a
/// concern in practice — would require ~2 billion `Function.prototype = X`
/// statements at module init.
pub static NEXT_SYNTHETIC_CLASS_ID: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0x8000_0000);

/// Register a function's prototype object. Called by codegen-emitted
/// init code whenever the HIR detects `<expr>.prototype = <expr>` at
/// the assignment-statement level (lower_expr_assignment Member arm).
///
/// Returns the synthetic class_id allocated for this function (0 if
/// validation fails). The synthetic id is folded into CLASS_REGISTRY
/// when a class extends `func` via the #711 dynamic-parent path.
#[no_mangle]
pub extern "C" fn js_set_function_prototype(func: f64, proto: f64) -> u32 {
    let func_bits = func.to_bits();
    let func_tag = func_bits & 0xFFFF_0000_0000_0000;
    let proto_bits = proto.to_bits();
    let proto_tag = proto_bits & 0xFFFF_0000_0000_0000;
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    // The function must be a heap-allocated pointer. Anything else (a
    // primitive `<not-a-function>.prototype = X`) is a no-op — preserves the
    // pre-fix baseline where it was just a property write on a non-function.
    if func_tag != POINTER_TAG {
        return 0;
    }
    // A function may legitimately have a *primitive* (e.g. `null`) prototype:
    // `function f() {} f.prototype = null` — it just doesn't establish an
    // `instanceof` chain. Store it as a plain `prototype` data property so reads
    // reflect it (test262 `GetPrototypeFromConstructor` falls back to the
    // default when `newTarget.prototype` is not an object). Without this the
    // write was dropped and the stale auto-created prototype object lingered.
    if proto_tag != POINTER_TAG {
        let func_ptr = (func_bits & crate::value::POINTER_MASK) as usize;
        if func_ptr != 0 && crate::closure::is_closure_ptr(func_ptr) {
            crate::closure::closure_set_dynamic_prop(func_ptr, "prototype", proto);
            set_builtin_property_attrs(
                func_ptr,
                "prototype".to_string(),
                PropertyAttrs::new(true, false, false),
            );
        }
        return 0;
    }
    // Validate the proto pointer points at a real Object. If it's a
    // builtin header (Set/Map/Regex) or null, bail — Perry can't
    // currently model those as prototype sources.
    let proto_ptr = crate::value::js_nanbox_get_pointer(proto) as *mut ObjectHeader;
    if proto_ptr.is_null() {
        return 0;
    }
    let proto_addr = proto_ptr as usize;
    if crate::set::is_registered_set(proto_addr)
        || crate::map::is_registered_map(proto_addr)
        || crate::regex::is_regex_pointer(proto_ptr as *const u8)
    {
        return 0;
    }
    unsafe {
        if !is_valid_obj_ptr(proto_ptr as *const u8) {
            return 0;
        }
        let gc_header =
            (proto_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let obj_type = (*gc_header).obj_type;
        // `foo.prototype = new Array(...)` — a real-array prototype can't join
        // the class-id machinery (it has no ObjectHeader), but it must not be
        // DROPPED: store it as the closure's `prototype` dynamic prop so reads
        // reflect it and `js_new_function_construct` links instances to it
        // (test262 filter/15.4.4.20-6-*, some/15.4.4.17-8-*, map/15.4.4.19-9-3).
        if obj_type == crate::gc::GC_TYPE_ARRAY || obj_type == crate::gc::GC_TYPE_LAZY_ARRAY {
            let func_ptr = (func_bits & crate::value::POINTER_MASK) as usize;
            if func_ptr != 0 && crate::closure::is_closure_ptr(func_ptr) {
                crate::closure::closure_set_dynamic_prop(func_ptr, "prototype", proto);
                set_builtin_property_attrs(
                    func_ptr,
                    "prototype".to_string(),
                    PropertyAttrs::new(true, false, false),
                );
            }
            return 0;
        }
        if obj_type != crate::gc::GC_TYPE_OBJECT {
            return 0;
        }
    }

    // Allocate or reuse a synthetic class id for this function value.
    // The same `function Base() {}` ident can be assigned a prototype
    // multiple times in pathological code; we keep the FIRST mapping
    // and quietly ignore subsequent calls so existing parent edges
    // don't dangle.
    {
        let read = FUNCTION_CLASS_IDS.read().unwrap();
        if let Some(map) = read.as_ref() {
            if let Some(&existing) = map.get(&func_bits) {
                // Update the prototype object (allow re-pointing)
                // without changing the class_id.
                class_prototype_object_root_store(existing, proto_ptr);
                let func_ptr = (func_bits & crate::value::POINTER_MASK) as usize;
                if func_ptr != 0 {
                    crate::closure::closure_set_dynamic_prop(func_ptr, "prototype", proto);
                    set_builtin_property_attrs(
                        func_ptr,
                        "prototype".to_string(),
                        PropertyAttrs::new(true, false, false),
                    );
                }
                crate::typed_feedback::invalidate_method_change(existing);
                return existing;
            }
        }
    }
    let new_cid = NEXT_SYNTHETIC_CLASS_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    {
        let mut write = FUNCTION_CLASS_IDS.write().unwrap();
        if write.is_none() {
            *write = Some(HashMap::new());
        }
        write.as_mut().unwrap().insert(func_bits, new_cid);
    }
    class_prototype_object_root_store(new_cid, proto_ptr);
    let func_ptr = (func_bits & crate::value::POINTER_MASK) as usize;
    if func_ptr != 0 {
        crate::closure::closure_set_dynamic_prop(func_ptr, "prototype", proto);
        set_builtin_property_attrs(
            func_ptr,
            "prototype".to_string(),
            PropertyAttrs::new(true, false, false),
        );
    }
    // Register the synthetic id so REGISTERED_CLASS_IDS-gated paths
    // (e.g., the #687 ClassRef-as-receiver short-circuit) recognize it.
    unsafe { js_register_class_id(new_cid) };
    crate::typed_feedback::invalidate_method_change(new_cid);
    new_cid
}

/// Lookup helper for the dispatch chain walk: returns the prototype
/// object pointer for a synthetic class id, or null if none.
#[inline]
pub(crate) fn class_prototype_object(class_id: u32) -> *mut ObjectHeader {
    if let Ok(read) = CLASS_PROTOTYPE_OBJECTS.read() {
        if let Some(map) = read.as_ref() {
            return map.get(&class_id).copied().unwrap_or(0) as *mut ObjectHeader;
        }
    }
    std::ptr::null_mut()
}

/// #711 / #809: resolve `key` by walking the synthetic-class-id prototype
/// chain (`CLASS_PROTOTYPE_OBJECTS`), recursing into each prototype object
/// as a normal field lookup. Used both when a receiver's own keys miss AND
/// when it has no `keys_array` at all (an `Object.create(proto)` result, or
/// a `Function.prototype = obj` instance with no own props). Returns the
/// first defined, non-null field found on the chain.
pub(crate) unsafe fn resolve_proto_chain_field(
    class_id: u32,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    resolve_proto_chain_field_inner(class_id, key, None)
}

pub(crate) unsafe fn resolve_proto_chain_field_with_receiver(
    class_id: u32,
    key: *const crate::StringHeader,
    receiver: f64,
) -> Option<JSValue> {
    resolve_proto_chain_field_inner(class_id, key, Some(receiver))
}

unsafe fn inherited_proto_accessor_value(
    proto_obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    receiver: f64,
) -> Option<JSValue> {
    if key.is_null() || !ACCESSORS_IN_USE.with(|c| c.get()) {
        return None;
    }
    let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let key_len = (*key).byte_len as usize;
    let name = std::str::from_utf8(std::slice::from_raw_parts(key_ptr, key_len)).ok()?;
    let acc = get_accessor_descriptor(proto_obj as usize, name)?;
    if acc.get == 0 {
        return Some(JSValue::undefined());
    }
    let closure = (acc.get & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
    if closure.is_null() {
        return Some(JSValue::undefined());
    }
    let previous_this = js_implicit_this_set(receiver);
    let value = crate::closure::js_closure_call0(closure);
    js_implicit_this_set(previous_this);
    Some(JSValue::from_bits(value.to_bits()))
}

unsafe fn resolve_proto_chain_field_inner(
    class_id: u32,
    key: *const crate::StringHeader,
    receiver: Option<f64>,
) -> Option<JSValue> {
    let mut cid = class_id;
    let mut depth = 0usize;
    while depth < 32 {
        let proto_obj = class_prototype_object(cid);
        if !proto_obj.is_null() {
            if let Some(receiver) = receiver {
                if let Some(value) = inherited_proto_accessor_value(proto_obj, key, receiver) {
                    return Some(value);
                }
            }
            let field_val = if let Some(receiver) = receiver {
                let previous_this = js_implicit_this_set(receiver);
                // The recursive `get_field(proto_obj, key)` re-derives a class
                // getter's `this` from `proto_obj`; stash the real instance so an
                // inherited getter (object-literal `get x()` on an
                // `Object.create(proto)` prototype) binds `this` to the instance.
                let prev_override =
                    super::field_get_set::accessor_receiver_override_begin(receiver);
                let value = js_object_get_field_by_name(proto_obj as *const _, key);
                super::field_get_set::accessor_receiver_override_end(prev_override);
                js_implicit_this_set(previous_this);
                value
            } else {
                js_object_get_field_by_name(proto_obj as *const _, key)
            };
            if !field_val.is_undefined() && !field_val.is_null() {
                return Some(field_val);
            }
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    None
}

/// #1758: symbol-keyed analogue of [`resolve_proto_chain_field`]. Walks the
/// `CLASS_PROTOTYPE_OBJECTS` chain and, at each prototype object (a POINTER
/// class-object), looks up its OWN symbol property via `own_symbol_property`.
/// Lets a subclass whose parent is a class-expression value inherit the
/// parent's static *symbol* statics — e.g. effect's
/// `class BigIntFromSelf extends make(bigIntKeyword) {}` inheriting
/// `static [TypeId]`, which `Predicate.hasProperty(.., TypeId)` (`isSchema`)
/// and `u[TypeId]` both read. Returns the first defined value found.
///
/// #26 / #321: the walk must advance along TWO axes, because a synthetic
/// `Object.create(proto)` class id links to its prototype via the *proto
/// object's own class id*, not via `parent_class_id` (which only models the
/// `class A extends B` axis). effect's `Either.right(x)` builds
/// `Object.create(RightProto)` where `RightProto = Object.create(CommonProto)`
/// and `CommonProto[TypeId]` carries the brand. With only the
/// `parent_class_id` axis the walk stopped after the first prototype object
/// (`RightProto`), so `TypeId in either` / `either[TypeId]` missed the brand
/// two links up — making `ParseResult.isEither(...)` false for every struct
/// property parse (`S.is`/`decodeUnknownSync`/`encodeSync` on a `Struct`).
/// At each node we follow the proto object's own class id (the
/// `Object.create` prototype link) first, then fall back to
/// `parent_class_id` (the `extends` link); a `visited` set bounds cycles.
pub(crate) unsafe fn resolve_proto_chain_symbol(class_id: u32, sym_f64: f64) -> Option<f64> {
    let mut cid = class_id;
    let mut depth = 0usize;
    let mut visited: [u32; 32] = [0; 32];
    while depth < 32 {
        if visited[..depth].contains(&cid) {
            break;
        }
        visited[depth] = cid;
        let proto_obj = class_prototype_object(cid);
        let mut next_cid: u32 = 0;
        if !proto_obj.is_null() {
            let proto_f64 = f64::from_bits(JSValue::pointer(proto_obj as *const u8).bits());
            // OWN lookup only — this fn IS the chain walk, so recursing into
            // the full chain-walking getter would re-walk per prototype.
            if let Some(v) = crate::symbol::own_symbol_property(proto_f64, sym_f64) {
                return Some(v);
            }
            // Prefer the `Object.create` prototype link: the next chain node
            // is the proto object's own class id (which maps to ITS proto in
            // CLASS_PROTOTYPE_OBJECTS). Falls back to `parent_class_id` below.
            next_cid = crate::object::js_object_get_class_id(proto_obj as *const ObjectHeader);
        }
        if next_cid != 0 && next_cid != cid {
            cid = next_cid;
            depth += 1;
            continue;
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    None
}

/// Lookup the synthetic class id for a function value, if one was
/// registered via `js_set_function_prototype`.
#[inline]
pub(crate) fn function_class_id(value: f64) -> u32 {
    let bits = value.to_bits();
    if let Ok(read) = FUNCTION_CLASS_IDS.read() {
        if let Some(map) = read.as_ref() {
            return map.get(&bits).copied().unwrap_or(0);
        }
    }
    0
}

pub(crate) fn function_value_for_class_id(class_id: u32) -> Option<f64> {
    if class_id == 0 {
        return None;
    }
    FUNCTION_CLASS_IDS.read().ok().and_then(|guard| {
        guard.as_ref().and_then(|map| {
            map.iter()
                .find_map(|(&bits, &cid)| (cid == class_id).then_some(f64::from_bits(bits)))
        })
    })
}

/// Register a class id so `js_value_typeof` can distinguish class refs
/// (INT32-tagged with class_id payload) from real int32 numeric values.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_id(class_id: u32) {
    if class_id == 0 {
        return;
    }
    let mut guard = REGISTERED_CLASS_IDS.write().unwrap();
    if guard.is_none() {
        *guard = Some(std::collections::HashSet::new());
    }
    guard.as_mut().unwrap().insert(class_id);
}

/// Maps `class_id → user-visible class name`. Populated by codegen via
/// `js_register_class_name`. Read back by V8-bridge code when surfacing a
/// Perry class to JS — NestJS's `ModuleTokenFactory.create()` reads
/// `metatype.name` to build the module token, so the empty default name
/// from `v8::Function::builder(...)` would collide every module under the
/// same token. (#1021.)
pub static CLASS_NAMES: RwLock<Option<HashMap<u32, String>>> = RwLock::new(None);

/// Register the user-visible name of a class so the V8 bridge can label
/// the V8-side wrapper for nice `metatype.name` reads. Idempotent.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_name(class_id: u32, name_ptr: *const u8, name_len: u32) {
    if class_id == 0 || name_ptr.is_null() || name_len == 0 {
        return;
    }
    let slice = std::slice::from_raw_parts(name_ptr, name_len as usize);
    let name = match std::str::from_utf8(slice) {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };
    let mut guard = CLASS_NAMES.write().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard.as_mut().unwrap().insert(class_id, name);
}

/// Look up the user-visible name of a registered class. Returns `None`
/// when the class id was never registered with `js_register_class_name`.
pub fn class_name_for_id(class_id: u32) -> Option<String> {
    let guard = CLASS_NAMES.read().ok()?;
    guard.as_ref()?.get(&class_id).cloned()
}

/// Whether dynamic-dispatch miss diagnostics are enabled (`PERRY_DISPATCH_DIAG`,
/// any non-empty/non-falsey value). Cached on first read.
///
/// When a dynamic dispatch falls through every resolution tower (vtable,
/// static-method, static-field, prototype, field-scan, namespace, symbol), the
/// runtime returns a *silent placeholder* — the receiver class ref, an empty
/// object, `undefined`, etc. — rather than throwing, because some of those
/// placeholders are load-bearing (effect's `.pipe()` chains yield the class ref
/// during module init, #687). The upside is no spurious crashes; the downside
/// is a typo'd / unsupported member surfaces far downstream as a stray
/// `{}`/`1`/`[]`/function, turning each one into a multi-hour localization.
///
/// This flag doesn't change behavior — it just prints a located, typed report
/// at the moment of the miss, so the bug surfaces at its true call site.
pub(crate) fn dispatch_diag_enabled() -> bool {
    use std::sync::OnceLock;
    static EN: OnceLock<bool> = OnceLock::new();
    *EN.get_or_init(|| {
        std::env::var("PERRY_DISPATCH_DIAG")
            .map(|v| !v.is_empty() && v != "0" && v != "off" && v != "false")
            .unwrap_or(false)
    })
}

/// Best-effort one-line description of a dispatch receiver for diagnostics:
/// class refs resolve to their registered name, pointers/primitives to a tag.
fn describe_dispatch_receiver(recv: f64) -> String {
    let bits = recv.to_bits();
    let top16 = bits >> 48;
    if top16 == 0x7FFE {
        let cid = (bits & 0xFFFF_FFFF) as u32;
        return match class_name_for_id(cid) {
            Some(n) => format!("class-ref `{}` (id {})", n, cid),
            None => format!("class-ref (id {})", cid),
        };
    }
    if top16 == 0x7FFF || top16 == 0x7FF9 {
        return "string".to_string();
    }
    if top16 == 0x7FFD {
        return "object/pointer".to_string();
    }
    match bits {
        x if x == crate::value::TAG_UNDEFINED => "undefined".to_string(),
        0x7FFC_0000_0000_0002 => "null".to_string(),
        0x7FFC_0000_0000_0003 => "false".to_string(),
        0x7FFC_0000_0000_0004 => "true".to_string(),
        _ if !recv.is_nan() => format!("number {}", recv),
        _ => "value".to_string(),
    }
}

/// Report a true dynamic-dispatch miss to stderr (only when
/// `PERRY_DISPATCH_DIAG` is set). `tower` names which resolution path fell
/// through; `returning` is the silent placeholder the runtime is about to hand
/// back. No-op (and near-zero cost) when the flag is off.
pub(crate) fn report_dispatch_miss(tower: &str, recv: f64, name: &str, returning: &str) {
    if !dispatch_diag_enabled() {
        return;
    }
    eprintln!(
        "[perry dispatch-miss] {tower}: {}.{:?} did not resolve \u{2192} returning {returning}. \
         A dynamic dispatch fell through every tower; downstream this usually surfaces as a stray \
         {{}}/1/[]/function. Check the call site for {:?}.",
        describe_dispatch_receiver(recv),
        name,
        name
    );
}

/// Resolve a closure-typed JSValue back to a built-in constructor name
/// (`"Date"`/`"Array"`/`"Object"`/...) when it matches one of the
/// singleton-installed thunks. Returns `None` for closures that aren't
/// the globalThis built-in constructors. Used by
/// `js_new_function_construct` to dispatch `new <inst.constructor>(...)`
/// shapes (date-fns `constructFrom`, lodash-style `Array` cloning, ...)
/// to the right runtime factory.
pub(super) fn identify_global_builtin_constructor(func_value: f64) -> Option<&'static str> {
    use crate::value::JSValue;
    let jv = JSValue::from_bits(func_value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let ptr = jv.as_pointer() as *const crate::closure::ClosureHeader;
    if ptr.is_null() {
        return None;
    }
    if (ptr as usize) % std::mem::align_of::<crate::closure::ClosureHeader>() != 0 {
        return None;
    }
    if !is_valid_obj_ptr(ptr as *const u8) {
        return None;
    }
    // Identify by the closure's read-only `func_ptr` rather than the
    // GC-movable ClosureHeader address. Both the date-fns ctor closure
    // and the (later-evacuated) ctor closure carry the same
    // `global_this_builtin_noop_thunk` function pointer, so this match
    // survives GC moves. The per-name lookup must then walk the
    // globalThis singleton's keys to recover the constructor name —
    // accept the extra hop only when the func_ptr matches.
    unsafe {
        if (*ptr).type_tag != crate::closure::CLOSURE_MAGIC {
            return None;
        }
        let func_ptr = (*ptr).func_ptr as usize;
        let is_global_builtin_func = func_ptr
            == global_this_builtin_noop_thunk as *const u8 as usize
            || func_ptr == typed_array_constructor_call_thunk as *const u8 as usize
            // #4102: `Array`/`Object`/`Date` constructor *values* carry their own
            // coercion thunks (not the shared noop thunk), so the dynamic
            // `instanceof` / reflective `@@hasInstance` path could not recover
            // their name. Accept those thunks too; the singleton walk below maps
            // each back to "Array"/"Object"/"Date".
            || func_ptr == global_this_array_thunk as *const u8 as usize
            || func_ptr == global_this_object_thunk as *const u8 as usize
            || func_ptr == global_this_date_thunk as *const u8 as usize
            || func_ptr == global_this_blob_thunk as *const u8 as usize
            || func_ptr == global_this_file_thunk as *const u8 as usize
            || func_ptr == global_this_headers_thunk as *const u8 as usize
            || func_ptr == global_this_request_thunk as *const u8 as usize
            || func_ptr == global_this_response_thunk as *const u8 as usize
            || func_ptr == global_this_string_thunk as *const u8 as usize
            || func_ptr == global_this_number_thunk as *const u8 as usize
            || func_ptr == global_this_boolean_thunk as *const u8 as usize
            || func_ptr == error_constructor_call_thunk as *const u8 as usize
            || func_ptr == type_error_constructor_call_thunk as *const u8 as usize
            || func_ptr == range_error_constructor_call_thunk as *const u8 as usize
            || func_ptr == reference_error_constructor_call_thunk as *const u8 as usize
            || func_ptr == syntax_error_constructor_call_thunk as *const u8 as usize
            || func_ptr == eval_error_constructor_call_thunk as *const u8 as usize
            || func_ptr == uri_error_constructor_call_thunk as *const u8 as usize
            || func_ptr == webcrypto_illegal_constructor_thunk as *const u8 as usize
            // Map/Set/WeakMap/WeakSet/WeakRef constructor *values* carry their
            // own "requires 'new'" thunks (global_this.rs). When obtained as a
            // value and constructed via `new $WeakMap()` (e.g. qs's
            // `side-channel`/`get-intrinsic` reads `%WeakMap%` into a variable),
            // the call lands here, not the static codegen path. Accept the
            // thunks so the singleton walk recovers the name and the match arms
            // below dispatch into the real factory instead of invoking the
            // bare-call thunk (which throws "Constructor WeakMap requires 'new'").
            || func_ptr == map_constructor_call_thunk as *const u8 as usize
            || func_ptr == set_constructor_call_thunk as *const u8 as usize
            || func_ptr == weak_map_constructor_call_thunk as *const u8 as usize
            || func_ptr == weak_set_constructor_call_thunk as *const u8 as usize
            || func_ptr == weak_ref_constructor_call_thunk as *const u8 as usize
            || func_ptr
                == crate::messaging::js_message_channel_constructor_call_error as *const u8
                    as usize
            || func_ptr
                == crate::messaging::js_message_port_constructor_call_error as *const u8 as usize
            || func_ptr
                == crate::messaging::js_broadcast_channel_constructor_call_error as *const u8
                    as usize;
        if !is_global_builtin_func {
            return None;
        }
    }
    // Prefer the per-closure built-in `.name` record. Full-suite Rust tests
    // temporarily seed GLOBAL_THIS_PTR with GC fixture pointers; relying only
    // on the singleton walk below makes unrelated tests race with constructor
    // identity for globals such as TextEncoderStream.
    let name_value = crate::value::JSValue::from_bits(
        crate::closure::closure_get_dynamic_prop(ptr as usize, "name").to_bits(),
    );
    if name_value.is_string() {
        let name_ptr = name_value.as_string_ptr();
        if !name_ptr.is_null() {
            let name_bytes = unsafe {
                let data = (name_ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                std::slice::from_raw_parts(data, (*name_ptr).byte_len as usize)
            };
            if let Ok(name) = std::str::from_utf8(name_bytes) {
                for builtin in GLOBAL_THIS_BUILTIN_CONSTRUCTORS.iter().copied() {
                    if builtin == name {
                        return Some(builtin);
                    }
                }
            }
        }
    }
    // Find which builtin name maps to this exact closure header on the
    // singleton. Walk via the existing
    // `js_get_global_this_builtin_value` helper — short loop (≤ ~50
    // entries), only fires on the constructFrom hot path.
    let global_this_f64 = js_get_global_this();
    let global_obj = crate::value::js_nanbox_get_pointer(global_this_f64) as *const ObjectHeader;
    if global_obj.is_null() {
        return None;
    }
    for name in GLOBAL_THIS_BUILTIN_CONSTRUCTORS.iter().copied() {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let v = unsafe { js_object_get_field_by_name(global_obj, key) };
        if v.bits() == jv.bits() {
            return Some(name);
        }
    }
    None
}

fn text_decoder_bool_option(options: f64, name: &str) -> f64 {
    let jsval = crate::value::JSValue::from_bits(options.to_bits());
    if !jsval.is_pointer() {
        return f64::from_bits(crate::value::TAG_FALSE);
    }
    let obj = jsval.as_pointer::<ObjectHeader>();
    if obj.is_null() {
        return f64::from_bits(crate::value::TAG_FALSE);
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = js_object_get_field_by_name(obj, key);
    let value_f64 = f64::from_bits(value.bits());
    f64::from_bits(crate::value::JSValue::bool(crate::value::js_is_truthy(value_f64) != 0).bits())
}

unsafe fn validate_web_compression_stream_format(format: f64) {
    let ptr = crate::builtins::js_string_coerce(format) as *const crate::StringHeader;
    if ptr.is_null() {
        crate::fs::validate::throw_type_error_with_code(
            "The argument 'format' is invalid.",
            "ERR_INVALID_ARG_VALUE",
        );
    }
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    if matches!(bytes, b"gzip" | b"deflate" | b"deflate-raw" | b"brotli") {
        return;
    }
    let received = String::from_utf8_lossy(bytes);
    let message = format!("The argument 'format' is invalid. Received '{received}'");
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE");
}

pub(crate) const CLASS_ID_TEXT_ENCODER_STREAM: u32 = 0x7FFF_FF30;
pub(crate) const CLASS_ID_TEXT_DECODER_STREAM: u32 = 0x7FFF_FF31;
pub(crate) const CLASS_ID_COMPRESSION_STREAM: u32 = 0x7FFF_FF32;
pub(crate) const CLASS_ID_DECOMPRESSION_STREAM: u32 = 0x7FFF_FF33;

unsafe fn text_encoding_stream_new_with_constructor(constructor: f64, class_id: u32) -> f64 {
    let stream = js_object_alloc(class_id, 0);
    if stream.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }

    for key_bytes in [b"readable".as_slice(), b"writable".as_slice()] {
        let key = crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
        let endpoint = js_object_alloc(0, 0);
        let value = if endpoint.is_null() {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        } else {
            crate::value::js_nanbox_pointer(endpoint as i64)
        };
        js_object_set_field_by_name(stream, key, value);
    }

    let ctor_key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
    js_object_set_field_by_name(stream, ctor_key, constructor);

    crate::value::js_nanbox_pointer(stream as i64)
}

unsafe fn text_encoding_stream_new(constructor_name: &[u8], class_id: u32) -> f64 {
    let ctor = js_get_global_this_builtin_value(constructor_name.as_ptr(), constructor_name.len());
    text_encoding_stream_new_with_constructor(ctor, class_id)
}

#[cfg(test)]
pub(crate) unsafe fn test_text_encoding_stream_new_with_constructor(
    constructor: f64,
    class_id: u32,
) -> f64 {
    text_encoding_stream_new_with_constructor(constructor, class_id)
}

#[no_mangle]
pub unsafe extern "C" fn js_text_encoder_stream_new() -> f64 {
    text_encoding_stream_new(b"TextEncoderStream", CLASS_ID_TEXT_ENCODER_STREAM)
}

#[no_mangle]
pub unsafe extern "C" fn js_text_decoder_stream_new() -> f64 {
    text_encoding_stream_new(b"TextDecoderStream", CLASS_ID_TEXT_DECODER_STREAM)
}

#[no_mangle]
pub unsafe extern "C" fn js_compression_stream_new() -> f64 {
    text_encoding_stream_new(b"CompressionStream", CLASS_ID_COMPRESSION_STREAM)
}

#[no_mangle]
pub unsafe extern "C" fn js_decompression_stream_new() -> f64 {
    text_encoding_stream_new(b"DecompressionStream", CLASS_ID_DECOMPRESSION_STREAM)
}

#[no_mangle]
pub unsafe extern "C" fn js_text_encoding_stream_new() -> f64 {
    js_text_encoder_stream_new()
}

/// Synthetic-anonymous-shape class IDs: classes the HIR generates for
/// bare object literals (`{ x: 1 }` → `__AnonShape_<hash>`). Instances
/// of these shapes should report `Object` from `.constructor`, not the
/// synthetic class itself, so date-fns's `new value.constructor(...)`,
/// drizzle's `value.constructor === Object` duck checks, and the standard
/// `({}).constructor === Object` semantics all match Node. The HIR
/// lowering registers each anon shape's id here at module init.
pub static ANON_SHAPE_CLASS_IDS: RwLock<Option<std::collections::HashSet<u32>>> = RwLock::new(None);

/// Mark `class_id` as a synthetic anon-shape class so `.constructor`
/// reads on instances of that class return the global `Object`
/// constructor rather than the synthetic class ref.
#[no_mangle]
pub unsafe extern "C" fn js_register_anon_shape_class_id(class_id: u32) {
    if class_id == 0 {
        return;
    }
    let mut guard = ANON_SHAPE_CLASS_IDS.write().unwrap();
    if guard.is_none() {
        *guard = Some(std::collections::HashSet::new());
    }
    guard.as_mut().unwrap().insert(class_id);
}

/// True if `class_id` was registered via `js_register_anon_shape_class_id`.
pub fn is_anon_shape_class_id(class_id: u32) -> bool {
    if class_id == 0 {
        return false;
    }
    if let Ok(guard) = ANON_SHAPE_CLASS_IDS.read() {
        if let Some(set) = guard.as_ref() {
            return set.contains(&class_id);
        }
    }
    false
}

/// Register a static field value on a class so `Cls.field` (when `Cls` is
/// accessed via dynamic dispatch — e.g. through an Any-typed local) finds
/// the value via the runtime path. Codegen calls this at module init for
/// every static field initializer in addition to writing the value to the
/// per-field module global. Refs #420 / #618 followup. Static-field values
/// stored in CLASS_DYNAMIC_PROPS keyed by class_id.
#[no_mangle]
pub unsafe extern "C" fn js_class_register_static_field(
    class_id: u32,
    name_ptr: *const u8,
    name_len: usize,
    value: f64,
) {
    if class_id == 0 || name_ptr.is_null() || name_len == 0 {
        return;
    }
    let name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };
    class_dynamic_prop_root_store(class_id, name, value);
}

/// Issue #838: JS-classic prototype method assignment.
///
/// `Class.prototype.method = function() {…}` (and the aliased form
/// `var p = Class.prototype; p.method = function() {…}`) is a pre-ES6
/// idiom dayjs, chalk, and a long tail of libraries still ship.
/// Pre-fix the assignment was lowered to a generic `PropertySet` whose
/// receiver evaluated to a class-prototype-shaped object that nothing
/// downstream consulted, so `(new Class()).method` came back as
/// `undefined`.
///
/// The HIR-level fix routes recognised shapes to
/// `js_register_prototype_method(class_id, name, value)`, which stores
/// the closure value into a per-class side-table here. The dispatch
/// hot paths (`js_object_get_field_by_name` for `inst.method` reads
/// and `js_native_call_method` for `inst.method(...)` calls) consult
/// this table after the regular vtable / proto-object lookups miss,
/// invoking the closure with `this` bound to the receiver.
///
/// Stored values use their full NaN-boxed bits (f64) — typically a
/// POINTER_TAG'd closure, but the dispatch path treats whatever is
/// stored as a callable value and routes it through
/// `js_native_call_value`, which itself accepts both closures and raw
/// `*ClosureHeader` shapes.
pub static CLASS_PROTOTYPE_METHODS: RwLock<Option<HashMap<u32, HashMap<String, u64>>>> =
    RwLock::new(None);
static CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub(crate) fn class_prototype_fast_guards_invalidated() -> bool {
    CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED.load(std::sync::atomic::Ordering::Acquire)
}

fn invalidate_class_prototype_fast_guards() {
    CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED.store(true, std::sync::atomic::Ordering::Release);
}

pub(crate) fn class_prototype_method_root_store(class_id: u32, name: String, value_bits: u64) {
    {
        let mut guard = CLASS_PROTOTYPE_METHODS.write().unwrap();
        if guard.is_none() {
            *guard = Some(HashMap::new());
        }
        guard
            .as_mut()
            .unwrap()
            .entry(class_id)
            .or_insert_with(HashMap::new)
            .insert(name.clone(), value_bits);
    }
    invalidate_class_prototype_fast_guards();
    crate::gc::runtime_write_barrier_root_nanbox(value_bits);
    // #5024: the side table makes the method dispatchable, but own-key
    // enumeration on the prototype OBJECT (Object.keys / getOwnPropertyNames /
    // `in` / hasOwnProperty / for-in / Object.assign) consults the object's
    // keys_array, which the side table never touched — React's
    // `Object.assign(PureComponent.prototype, Component.prototype)` copied
    // nothing, so `isReactComponent` vanished and every `extends PureComponent`
    // class rendered as a function component. Mirror the write onto the
    // materialized prototype object as an ordinary enumerable own property.
    let proto = class_prototype_object(class_id);
    if !proto.is_null() {
        unsafe { mirror_prototype_method_on_object(proto, &name, value_bits) };
    }
}

/// #5024: write a side-table-registered prototype method onto the
/// materialized prototype object so the key lands in its `keys_array`
/// (assignment semantics: enumerable data property). Values keep their
/// full NaN-boxed bits; dispatch paths that find the property on the
/// object see the same value the side table holds.
unsafe fn mirror_prototype_method_on_object(proto: *mut ObjectHeader, name: &str, value_bits: u64) {
    if proto.is_null() || name.is_empty() {
        return;
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(proto, key, f64::from_bits(value_bits));
}

/// Register a JS-classic prototype-method assignment on a class.
/// Called by codegen-emitted init code for each `Class.prototype.<name>
/// = <fn>` (or aliased form) that the HIR recognises. `value` is the
/// NaN-boxed callable to be invoked with `this` bound to the receiver
/// at dispatch time.
#[no_mangle]
pub unsafe extern "C" fn js_register_prototype_method(
    class_id: u32,
    name_ptr: *const u8,
    name_len: usize,
    value: f64,
) {
    invalidate_class_prototype_fast_guards();
    if class_id == 0 || name_ptr.is_null() || name_len == 0 {
        return;
    }
    let name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };
    // `C.prototype.X = v` where X is an instance accessor on the class must
    // invoke the setter, not overwrite the accessor with a data method. This
    // write was lowered as a prototype-method monkey-patch because computed-key
    // accessors (`set [expr](v)`) aren't known at compile time, so the
    // recogniser couldn't route it to the ordinary setter path. If X has a
    // setter, invoke it with `this` = the prototype ref; if it's a getter-only
    // accessor, the (non-strict) assignment is a silent no-op rather than a
    // clobber (Test262 accessor-name-*/computed setters).
    let proto_ref = class_prototype_ref_value(class_id);
    if class_instance_setter_apply(class_id, &name, proto_ref, value) {
        return;
    }
    if class_has_instance_getter(class_id, &name) {
        return;
    }
    class_prototype_method_root_store(class_id, name, value.to_bits());
    // Ensure the receiver class can be `typeof`-detected. Method-less
    // classes that only get extended via `Class.prototype.m = fn`
    // wouldn't otherwise reach js_register_class_id.
    js_register_class_id(class_id);
    crate::typed_feedback::invalidate_method_change(class_id);
}

/// Issue #838 followup (b): function-classic prototype-method dispatch.
/// dayjs's minified bundle declares its instance class via a function
/// declaration inside an IIFE (`function M(cfg) {…}; var m = M.prototype;
/// m.format = function(){…}; return M`). At HIR time `M` is a function
/// (no `class M` block), so the #838 recogniser bailed because
/// `lookup_class("M")` returned None. This helper closes the gap on the
/// runtime side: a single call takes the closure value of `M`, allocates
/// (or reuses) a synthetic class id keyed by the closure's NaN-boxed
/// bits, registers the method on that synthetic class, and returns the
/// id so a paired `new <FuncRef>(args)` allocator can stamp the same id
/// on the instance header. After both arms run, the existing dispatch
/// hot paths (`js_object_get_field_by_name`, `js_native_call_method`)
/// find the method without further changes.
///
/// `func_value` must be a POINTER_TAG'd ClosureHeader (the shape
/// `Expr::FuncRef` lowers to via `js_closure_alloc_singleton`). Anything
/// else is a no-op — preserves the pre-fix baseline where non-callable
/// `.prototype.m = fn` writes were silent property sets.
/// Issue #838 followup (b) — read side: look up a method previously
/// registered via `js_register_function_prototype_method` against the
/// synthetic class id derived from `func_value`. Pre-fix the AST shape
/// `<funcDecl>.prototype.<name>` lowered to a generic PropertyGet on a
/// `Function.prototype` object that never materialised, so the read
/// was always `undefined` — `typeof Foo.prototype.method` came back
/// `'undefined'` even when the method was correctly dispatched through
/// `(new Foo()).method` via the side-table walk. Pairs with the new
/// `Expr::GetFunctionPrototypeMethod` HIR variant.
///
/// Returns the NaN-boxed `undefined` tag if the function value isn't a
/// registered closure, or no method by that name was registered.
#[no_mangle]
pub unsafe extern "C" fn js_get_function_prototype_method(
    func_value: f64,
    name_ptr: *const u8,
    name_len: usize,
) -> f64 {
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    if name_ptr.is_null() || name_len == 0 {
        return undef;
    }
    let name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
        Ok(s) => s,
        Err(_) => return undef,
    };
    // `f.prototype.constructor` — a *data* property (the prototype's back-pointer
    // to its constructor), not a registered method, so `lookup_prototype_method`
    // never finds it and the method allowlist below excludes it. When the inline
    // `<funcref>.prototype.constructor` read folds to this entry (no separate
    // `.prototype` access ran to allocate the synthetic class id), `cid` is 0 and
    // the function returned `undefined`. Route through the real prototype value —
    // `js_function_prototype_value_for_read` materializes the auto-created
    // prototype (whose `constructor` is `func_value`) or returns a replaced
    // `f.prototype = X` — then read its `constructor` field. (Spec
    // language/statements/function/S13.2_A4_*, S13.2.2_A1_*.)
    if name == "constructor" {
        let proto_val = js_function_prototype_value_for_read(func_value);
        let jv = crate::value::JSValue::from_bits(proto_val.to_bits());
        if !jv.is_pointer() {
            return undef;
        }
        let pptr = jv.as_pointer::<ObjectHeader>();
        if pptr.is_null() {
            return undef;
        }
        let key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
        let v = js_object_get_field_by_name(pptr, key as *const crate::StringHeader);
        return f64::from_bits(v.bits());
    }
    // Look up the (already-allocated) synthetic class id for this
    // function value. Don't allocate one here — reads on a function
    // that never had any `.prototype.x = fn` assignment should
    // return `undefined`, matching the spec'd behavior of reading a
    // missing property on the `Function.prototype` object.
    let cid = function_class_id(func_value);
    if cid == 0 {
        return undef;
    }
    match lookup_prototype_method(cid, name) {
        Some(v) => v,
        None if matches!(
            name,
            "toString"
                | "valueOf"
                | "hasOwnProperty"
                | "isPrototypeOf"
                | "propertyIsEnumerable"
                | "toLocaleString"
        ) =>
        {
            let proto = ensure_function_prototype_object(func_value, cid);
            if proto.is_null() {
                return undef;
            }
            let receiver = crate::value::js_nanbox_pointer(proto as i64);
            let method = js_class_method_bind(receiver, name_ptr, name_len);
            f64::from_bits(method.to_bits())
        }
        None => {
            // #5024: properties can land on the prototype OBJECT without a
            // side-table registration — `Object.assign(F.prototype, src)`
            // (React's PureComponent setup), a replaced `F.prototype = obj`,
            // or any generic dynamic write. Read the real prototype value
            // (replaced object, or the materialized auto-created one) so
            // the recognised `<func>.prototype.<name>` read shape agrees
            // with the generic property-get path.
            let proto_val = js_function_prototype_value_for_read(func_value);
            let jv = crate::value::JSValue::from_bits(proto_val.to_bits());
            if !jv.is_pointer() {
                return undef;
            }
            let pptr = jv.as_pointer::<ObjectHeader>();
            if pptr.is_null() {
                return undef;
            }
            let key = crate::string::js_string_from_bytes(name_ptr, name_len as u32);
            let v = js_object_get_field_by_name(pptr, key);
            f64::from_bits(v.bits())
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_register_function_prototype_method(
    func_value: f64,
    name_ptr: *const u8,
    name_len: usize,
    value: f64,
) -> u32 {
    let cid = synthetic_class_id_for_function(func_value);
    if cid == 0 || name_ptr.is_null() || name_len == 0 {
        return cid;
    }
    let name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
        Ok(s) => s.to_string(),
        Err(_) => return cid,
    };
    class_prototype_method_root_store(cid, name, value.to_bits());
    js_register_class_id(cid);
    crate::typed_feedback::invalidate_method_change(cid);
    cid
}

/// Get-or-allocate a synthetic class id keyed by a function value's
/// NaN-boxed bits. Used by `js_register_function_prototype_method` (HIR
/// "Func.prototype.x = fn" recogniser) and `js_new_function_construct`
/// (HIR "new Func(args)" allocator) so both sides agree on the same id
/// — the instance's `(*obj).class_id` lands in the same bucket the
/// method registration stored against. Returns 0 if `func_value` isn't a
/// POINTER_TAG'd value (callable shape requirement).
pub(crate) fn synthetic_class_id_for_function(func_value: f64) -> u32 {
    let func_bits = func_value.to_bits();
    // Require a verified closure shape so we don't store arbitrary
    // POINTER_TAG'd pointers (arrays, objects, etc. all share the tag)
    // in `FUNCTION_CLASS_IDS`. The bits-as-key invariant only makes
    // sense for callable values that produced a stable singleton
    // closure pointer.
    if !is_callable_function_value(func_value) {
        return 0;
    }
    {
        let read = FUNCTION_CLASS_IDS.read().unwrap();
        if let Some(map) = read.as_ref() {
            if let Some(&existing) = map.get(&func_bits) {
                return existing;
            }
        }
    }
    let new_cid = NEXT_SYNTHETIC_CLASS_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    {
        let mut write = FUNCTION_CLASS_IDS.write().unwrap();
        if write.is_none() {
            *write = Some(HashMap::new());
        }
        write.as_mut().unwrap().insert(func_bits, new_cid);
    }
    unsafe { js_register_class_id(new_cid) };
    new_cid
}

thread_local! {
    static CURRENT_NEW_TARGET: std::cell::Cell<u64> =
        const { std::cell::Cell::new(crate::value::TAG_UNDEFINED) };
}

#[no_mangle]
pub extern "C" fn js_new_target_value() -> f64 {
    f64::from_bits(CURRENT_NEW_TARGET.with(|value| value.get()))
}

/// Issue #838 followup (b): construct an instance from a function value.
/// Pairs with `js_register_function_prototype_method` — both arms route
/// through `synthetic_class_id_for_function` so the instance's
/// `class_id` matches the bucket prototype methods were registered
/// against. Allocates a fresh object stamped with the synthetic id,
/// then invokes the function as the constructor with `IMPLICIT_THIS`
/// bound to the new object so any `this.foo = …` writes in the
/// function body land on the instance. Returns the NaN-boxed new
/// instance pointer.
///
/// `func_value` must be a POINTER_TAG'd closure. `args_ptr` is a flat
/// f64 array of length `args_len`. Falls back to a class_id=0
/// empty-object allocation when the function value isn't a closure
/// (preserves the pre-fix baseline for misuse).
#[no_mangle]
pub unsafe extern "C" fn js_new_function_construct(
    func_value: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    // `new <primitive>()` is a TypeError — a primitive is never a constructor
    // (`new undefined()`, `new 5n()`, `new "s"()`, `new true()`). Checked via
    // the unambiguous NaN-box tags only (NOT `is_number`, whose f64 range
    // overlaps the raw-i64 pointer encoding of module-level objects). Without
    // this, `new x.method()` where `x.method` reads back `undefined`, and other
    // primitive callees, silently fell through to the empty-object fallback.
    {
        let jv = crate::value::JSValue::from_bits(func_value.to_bits());
        if jv.is_undefined()
            || jv.is_null()
            || jv.is_bool()
            || (jv.is_int32() && constructor_class_ref_id(func_value).is_none())
            || jv.is_any_string()
            || jv.is_bigint()
        {
            let desc = unsafe { super::object_ops::describe_value_for_type_error(func_value) };
            super::object_ops::throw_object_type_error_with_suffix(
                &format!("{desc} "),
                "is not a constructor",
            );
        }
    }
    // `new (new String(""))` / `new (new Number(1))` — a boxed primitive WRAPPER
    // object is an ordinary object, never a constructor, so `new` on it throws
    // `TypeError` (Test262 `S15.5.5_A2`). Without this it fell through to the
    // empty-object construction fallback and silently produced `{}`.
    if crate::builtins::boxed_primitive_payload(func_value).is_some() {
        super::object_ops::throw_object_type_error(b"is not a constructor");
    }
    // #3656: `new p()` where `p` is a Proxy dispatches through its `construct`
    // trap (or forwards to the target). Reached when the compiler can't prove
    // the callee is a proxy statically (e.g. `new record.proxy()`). newTarget
    // for a plain `new` is the constructor being invoked — the proxy itself.
    if crate::proxy::js_proxy_is_proxy(func_value) == 1 {
        let arr = crate::array::js_array_alloc(0);
        let mut a = arr;
        if !args_ptr.is_null() {
            for i in 0..args_len {
                a = crate::array::js_array_push_f64(a, *args_ptr.add(i));
            }
        }
        let arr_box = f64::from_bits(0x7FFD_0000_0000_0000 | (a as u64 & 0x0000_FFFF_FFFF_FFFF));
        return crate::proxy::js_proxy_construct(func_value, arr_box, func_value);
    }
    if is_non_constructable_builtin_function_value(func_value) {
        throw_non_constructable_builtin_function();
    }
    // `new Function.prototype` — %Function.prototype% is callable but NOT a
    // constructor (ECMA-262 20.2.3: "does not have a [[Construct]] internal
    // method").
    if super::global_this::is_function_prototype_object_value(func_value) {
        super::object_ops::throw_object_type_error(b"is not a constructor");
    }
    if let Some((module, method)) = bound_native_callable_module_and_method(func_value) {
        if module == "sqlite"
            && matches!(
                method.as_str(),
                "DatabaseSync" | "Session" | "StatementSync"
            )
        {
            let ptr =
                crate::value::JS_NATIVE_SQLITE_DISPATCH.load(std::sync::atomic::Ordering::SeqCst);
            if !ptr.is_null() {
                let dispatch: crate::value::JsNativeSqliteDispatchFn = std::mem::transmute(ptr);
                return dispatch(method.as_ptr(), method.len(), args_ptr, args_len, 1);
            }
        }
        if module == "tty" && matches!(method.as_str(), "ReadStream" | "WriteStream") {
            let fd = if !args_ptr.is_null() && args_len > 0 {
                *args_ptr
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            return if method == "ReadStream" {
                crate::tty::js_tty_read_stream_new(fd)
            } else {
                crate::tty::js_tty_write_stream_new(fd)
            };
        }
        if module == "fs" && method == "Utf8Stream" {
            let options = if !args_ptr.is_null() && args_len > 0 {
                *args_ptr
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            return crate::fs::js_fs_utf8_stream_new(options);
        }
        if module == "vm" && method == "Script" {
            let code = if !args_ptr.is_null() && args_len > 0 {
                *args_ptr
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            let options = if !args_ptr.is_null() && args_len > 1 {
                *args_ptr.add(1)
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            return crate::node_vm::js_vm_script_new(code, options);
        }
        if module == "fs"
            && matches!(
                method.as_str(),
                "ReadStream" | "FileReadStream" | "WriteStream" | "FileWriteStream"
            )
        {
            let path = if !args_ptr.is_null() && args_len > 0 {
                *args_ptr
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            let options = if !args_ptr.is_null() && args_len > 1 {
                *args_ptr.add(1)
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            return if matches!(method.as_str(), "ReadStream" | "FileReadStream") {
                crate::fs::js_fs_create_read_stream(path, options)
            } else {
                crate::fs::js_fs_create_write_stream(path, options)
            };
        }
        if module == "tls" && method == "SecureContext" {
            let options = if !args_ptr.is_null() && args_len > 0 {
                *args_ptr
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            return crate::tls::js_tls_secure_context_new(options);
        }
        if module == "wasi" && method == "WASI" {
            let options = if !args_ptr.is_null() && args_len > 0 {
                *args_ptr
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            return crate::wasi::js_wasi_new(options);
        }
        if module == "readline/promises" && method == "Readline" {
            let output = if !args_ptr.is_null() && args_len > 0 {
                *args_ptr
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            let options = if !args_ptr.is_null() && args_len > 1 {
                *args_ptr.add(1)
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            return crate::node_submodules::js_readline_promises_readline_new(output, options);
        }
        if module == "repl" && matches!(method.as_str(), "Recoverable" | "REPLServer") {
            let first = if !args_ptr.is_null() && args_len > 0 {
                *args_ptr
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            return if method == "Recoverable" {
                crate::node_repl::js_repl_recoverable_new(first)
            } else {
                crate::node_repl::js_repl_repl_server_new(first)
            };
        }
        // #3663: `new Readable(opts)` (and Writable/Duplex/Transform/PassThrough)
        // where the constructor binding came through any aliasing path the
        // compiler can't resolve to a bare `Expr::New` — `const { Readable } =
        // require('stream')`, `const s = require('stream'); new s.Readable()`,
        // or `const R = stream.Readable; new R()`. In each case the callee
        // value is the `stream.<Ctor>` bound-method closure, so dispatch to the
        // same runtime constructors the named-import path uses. Without this the
        // call falls through to the empty-object baseline and the resulting
        // object has no EventEmitter/Writable methods, so `.on()`/`.write()`/
        // `.pipe()` throw "is not a function".
        if module == "stream"
            && matches!(
                method.as_str(),
                "Readable" | "Writable" | "Duplex" | "Transform" | "PassThrough"
            )
        {
            let opts = if !args_ptr.is_null() && args_len > 0 {
                *args_ptr
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            return match method.as_str() {
                "Readable" => crate::node_stream::js_node_stream_readable_new(opts),
                "Writable" => crate::node_stream::js_node_stream_writable_new(opts),
                "Duplex" => crate::node_stream::js_node_stream_duplex_new(opts),
                "Transform" => crate::node_stream::js_node_stream_transform_new(opts),
                "PassThrough" => crate::node_stream::js_node_stream_passthrough_new(opts),
                _ => unreachable!(),
            };
        }
        // #4904: `new http.Agent(opts)` / `new http.ClientRequest(opts)` /
        // `new http.IncomingMessage(socket)` / `new http.ServerResponse(req)`
        // (and `new https.Agent(opts)`) through any value-aliasing path —
        // `const { Agent } = require('http')`, `const CR =
        // http.ClientRequest`, etc. The bound export value carries
        // (module, method); forward construction to the stdlib http
        // dispatcher exactly like `OutgoingMessage` below.
        if (module == "http"
            && matches!(
                method.as_str(),
                "OutgoingMessage"
                    | "Agent"
                    | "ClientRequest"
                    | "IncomingMessage"
                    | "ServerResponse"
            ))
            || (module == "https" && method == "Agent")
        {
            let ptr =
                crate::value::JS_NATIVE_HTTP_DISPATCH.load(std::sync::atomic::Ordering::SeqCst);
            if !ptr.is_null() {
                let dispatch: unsafe extern "C" fn(
                    *const u8,
                    usize,
                    *const u8,
                    usize,
                    *const f64,
                    usize,
                ) -> f64 = std::mem::transmute(ptr);
                return dispatch(
                    module.as_ptr(),
                    module.len(),
                    method.as_ptr(),
                    method.len(),
                    args_ptr,
                    args_len,
                );
            }
        }
        // #4995: `new EE()` where `EE = require('events')` or came in as a
        // default / namespace import (`import EE from 'events'`, `import * as
        // ev from 'events'; new ev.EventEmitter()`). The callee is the bound
        // `events.EventEmitter` export value; without this arm construction
        // fell through to the generic empty-object path, so the instance had
        // no `.on`/`.emit`/`.setMaxListeners` (signal-exit's init throws).
        // Route to the linked emitter impl (perry-stdlib `bundled-events` or
        // perry-ext-events) via the construct dispatcher registered at
        // startup — this crate can't call the constructors directly.
        if module == "events"
            && matches!(
                method.as_str(),
                "EventEmitter" | "EventEmitterAsyncResource"
            )
        {
            let ptr =
                crate::value::JS_NATIVE_EVENTS_CONSTRUCT.load(std::sync::atomic::Ordering::SeqCst);
            if !ptr.is_null() {
                let dispatch: crate::value::JsNativeEventsConstructFn = std::mem::transmute(ptr);
                return dispatch(method.as_ptr(), method.len(), args_ptr, args_len);
            }
        }
        if module == "zlib" && matches!(method.as_str(), "ZstdCompress" | "ZstdDecompress") {
            let ptr =
                crate::value::JS_NATIVE_ZLIB_DISPATCH.load(std::sync::atomic::Ordering::SeqCst);
            if !ptr.is_null() {
                let dispatch: unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64 =
                    std::mem::transmute(ptr);
                let factory = if method == "ZstdCompress" {
                    "createZstdCompress"
                } else {
                    "createZstdDecompress"
                };
                return dispatch(factory.as_ptr(), factory.len(), args_ptr, args_len);
            }
        }
    }

    // date-fns `constructFrom` clones a Date via
    // `new date.constructor(value)`. `date.constructor` resolves to
    // the global `Date` closure pointer (the noop thunk installed by
    // `populate_global_this_builtins`). Without this intercept the
    // call falls through to the generic empty-object path and
    // `cloned.getTime()` reads garbage. Detect the global Date /
    // Array / Object constructor pointers and dispatch into the
    // matching real factory. Refs date-fns blocker.
    if let Some(name) = identify_global_builtin_constructor(func_value) {
        let args = if args_ptr.is_null() {
            &[][..]
        } else {
            std::slice::from_raw_parts(args_ptr, args_len)
        };
        match name {
            "Crypto" | "CryptoKey" | "SubtleCrypto" => {
                return crate::object::js_webcrypto_illegal_constructor();
            }
            "Symbol" => {
                return crate::error::js_throw_symbol_constructor_type_error();
            }
            "BigInt" => {
                return crate::error::js_throw_bigint_constructor_type_error();
            }
            "Navigator" => {
                return crate::error::js_throw_illegal_constructor_type_error();
            }
            "Date" => {
                if args.is_empty() {
                    return crate::date::js_date_new();
                }
                if args.len() == 1 {
                    return crate::date::js_date_new_from_value(args[0]);
                }
                let mut vals = [f64::from_bits(crate::value::TAG_UNDEFINED); 7];
                for (i, slot) in vals.iter_mut().enumerate() {
                    if i < args.len() {
                        *slot = args[i];
                    }
                }
                return crate::date::js_date_new_local_components(
                    vals[0], vals[1], vals[2], vals[3], vals[4], vals[5], vals[6],
                );
            }
            "Array" => {
                if args.len() == 1 {
                    let arr = crate::array::js_array_constructor_single(args[0]);
                    return crate::value::js_nanbox_pointer(arr as i64);
                }
                // `new Array(a, b, c)`: array filled with the args.
                let len = args.len() as u32;
                let arr = crate::array::js_array_alloc(len);
                (*arr).length = len;
                for (i, &v) in args.iter().enumerate() {
                    crate::array::js_array_set_f64(arr, i as u32, v);
                }
                return crate::value::js_nanbox_pointer(arr as i64);
            }
            "Object" => {
                let value = args
                    .first()
                    .copied()
                    .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
                return crate::object::js_object_coerce(value);
            }
            // `new $Map()` / `new $Set()` / `new $WeakMap()` / … where the
            // constructor was obtained as a value (alias variable, intrinsic
            // lookup, cross-module re-export). Mirror the static codegen
            // construction in lower_call/builtin.rs: allocate, NaN-box, then
            // initialize from the optional iterable argument.
            "Map" => {
                let map = crate::map::js_map_alloc(4);
                let boxed = crate::value::js_nanbox_pointer(map as i64);
                if let Some(&iterable) = args.first() {
                    let ij = crate::value::JSValue::from_bits(iterable.to_bits());
                    if !ij.is_undefined() && !ij.is_null() {
                        let from = crate::map::js_map_from_iterable(iterable);
                        return crate::value::js_nanbox_pointer(from as i64);
                    }
                }
                return boxed;
            }
            "Set" => {
                let set = crate::set::js_set_alloc(4);
                let boxed = crate::value::js_nanbox_pointer(set as i64);
                if let Some(&iterable) = args.first() {
                    let ij = crate::value::JSValue::from_bits(iterable.to_bits());
                    if !ij.is_undefined() && !ij.is_null() {
                        let from = crate::set::js_set_from_iterable(iterable);
                        return crate::value::js_nanbox_pointer(from as i64);
                    }
                }
                return boxed;
            }
            "WeakMap" => {
                let map = crate::weakref::js_weakmap_new();
                let boxed = crate::value::js_nanbox_pointer(map as i64);
                if let Some(&iterable) = args.first() {
                    let ij = crate::value::JSValue::from_bits(iterable.to_bits());
                    if !ij.is_undefined() && !ij.is_null() {
                        return crate::weakref::js_weakmap_init_iterable(boxed, iterable);
                    }
                }
                return boxed;
            }
            "WeakSet" => {
                let set = crate::weakref::js_weakset_new();
                let boxed = crate::value::js_nanbox_pointer(set as i64);
                if let Some(&iterable) = args.first() {
                    let ij = crate::value::JSValue::from_bits(iterable.to_bits());
                    if !ij.is_undefined() && !ij.is_null() {
                        return crate::weakref::js_weakset_init_iterable(boxed, iterable);
                    }
                }
                return boxed;
            }
            "WeakRef" => {
                let target = args
                    .first()
                    .copied()
                    .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
                let wr = crate::weakref::js_weakref_new(target);
                return crate::value::js_nanbox_pointer(wr as i64);
            }
            "Blob" => {
                let parts = args
                    .first()
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                let options = args
                    .get(1)
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                return crate::object::global_this_blob_thunk(std::ptr::null(), parts, options);
            }
            "File" => {
                let parts = args
                    .first()
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                let name = args
                    .get(1)
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                let options = args
                    .get(2)
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                return crate::object::global_this_file_thunk(
                    std::ptr::null(),
                    parts,
                    name,
                    options,
                );
            }
            "Headers" => {
                let init = args
                    .first()
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                return crate::object::global_this_headers_thunk(std::ptr::null(), init);
            }
            "Request" => {
                let input = args
                    .first()
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                let init = args
                    .get(1)
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                return crate::object::global_this_request_thunk(std::ptr::null(), input, init);
            }
            "Response" => {
                let body = args
                    .first()
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                let init = args
                    .get(1)
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                return crate::object::global_this_response_thunk(std::ptr::null(), body, init);
            }
            "Event" => {
                let event_type = args
                    .first()
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                let options = args
                    .get(1)
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                let event =
                    crate::event_target::js_event_new(event_type, options, args.len() as u32);
                return crate::value::js_nanbox_pointer(event as i64);
            }
            "CustomEvent" => {
                let event_type = args
                    .first()
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                let options = args
                    .get(1)
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                let event = crate::event_target::js_custom_event_new(
                    event_type,
                    options,
                    args.len() as u32,
                );
                return crate::value::js_nanbox_pointer(event as i64);
            }
            "DOMException" => {
                let message = args
                    .first()
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                let name = args
                    .get(1)
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                let exception = crate::event_target::js_dom_exception_new(message, name);
                return crate::value::js_nanbox_pointer(exception as i64);
            }
            // #2889: `new (rebound Error subclass)(msg)` through a global
            // constructor value. Mirrors the bare `new TypeError(msg)`
            // lowering so `const E = TypeError; new E("x")` produces a real
            // error instance with the right `.name`.
            "Error" | "TypeError" | "RangeError" | "ReferenceError" | "SyntaxError"
            | "EvalError" | "URIError" => {
                let kind = match name {
                    "TypeError" => crate::error::ERROR_KIND_TYPE_ERROR,
                    "RangeError" => crate::error::ERROR_KIND_RANGE_ERROR,
                    "ReferenceError" => crate::error::ERROR_KIND_REFERENCE_ERROR,
                    "SyntaxError" => crate::error::ERROR_KIND_SYNTAX_ERROR,
                    "EvalError" => crate::error::ERROR_KIND_EVAL_ERROR,
                    "URIError" => crate::error::ERROR_KIND_URI_ERROR,
                    _ => crate::error::ERROR_KIND_ERROR,
                };
                let message = if args.is_empty() {
                    f64::from_bits(crate::value::TAG_UNDEFINED)
                } else {
                    args[0]
                };
                let error = crate::error::js_error_new_kind_from_value(kind, message);
                return crate::value::js_nanbox_pointer(error as i64);
            }
            // #2889: `new (rebound RegExp)(pattern, flags)`.
            "RegExp" => {
                let pattern = if args.is_empty() {
                    std::ptr::null_mut()
                } else {
                    crate::builtins::js_string_coerce(args[0])
                };
                let flags = if args.len() < 2 || args[1].to_bits() == crate::value::TAG_UNDEFINED {
                    std::ptr::null_mut()
                } else {
                    crate::builtins::js_string_coerce(args[1])
                };
                let re = crate::regex::js_regexp_new(pattern, flags);
                return crate::value::js_nanbox_pointer(re as i64);
            }
            // #2889: `new (rebound TypedArray)(lengthOrSource)`.
            "Int8Array" | "Uint8Array" | "Uint8ClampedArray" | "Int16Array" | "Uint16Array"
            | "Int32Array" | "Uint32Array" | "Float16Array" | "Float32Array" | "Float64Array"
            | "BigInt64Array" | "BigUint64Array" => {
                let kind = match name {
                    "Int8Array" => crate::typedarray::KIND_INT8,
                    "Uint8Array" => crate::typedarray::KIND_UINT8,
                    "Uint8ClampedArray" => crate::typedarray::KIND_UINT8_CLAMPED,
                    "Int16Array" => crate::typedarray::KIND_INT16,
                    "Uint16Array" => crate::typedarray::KIND_UINT16,
                    "Int32Array" => crate::typedarray::KIND_INT32,
                    "Uint32Array" => crate::typedarray::KIND_UINT32,
                    "Float16Array" => crate::typedarray::KIND_FLOAT16,
                    "Float32Array" => crate::typedarray::KIND_FLOAT32,
                    "Float64Array" => crate::typedarray::KIND_FLOAT64,
                    "BigInt64Array" => crate::typedarray::KIND_BIGINT64,
                    _ => crate::typedarray::KIND_BIGUINT64,
                } as i32;
                let arg0 = if args.is_empty() {
                    f64::from_bits(crate::value::JSValue::number(0.0).bits())
                } else {
                    args[0]
                };
                // `new TA(buffer, byteOffset, length?)` via a *dynamic* constructor
                // value (e.g. test262's `testWithTypedArrayConstructors`, where
                // `TA` is a variable) must honor the offset/length arguments. The
                // single-arg `js_typed_array_new` path dropped them, so every
                // view built this way reported `byteOffset === 0`. Route the
                // multi-arg form through the view constructor, which records the
                // backing/offset so `.byteOffset` / `.buffer` are correct and the
                // result aliases the buffer (mirrors the literal-name codegen
                // path in `lower_call::builtin`). A non-ArrayBuffer `arg0` falls
                // back to `js_typed_array_new` inside `js_typed_array_view`.
                let ta = if args.len() >= 2 {
                    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
                    crate::typedarray_view::js_typed_array_view(
                        kind,
                        arg0,
                        args[1],
                        args.get(2).copied().unwrap_or(undefined),
                    )
                } else {
                    crate::typedarray::js_typed_array_new(kind, arg0)
                };
                return crate::value::js_nanbox_pointer(ta as i64);
            }
            "TextEncoderStream" => {
                return text_encoding_stream_new_with_constructor(
                    func_value,
                    CLASS_ID_TEXT_ENCODER_STREAM,
                );
            }
            "TextDecoderStream" => {
                return text_encoding_stream_new_with_constructor(
                    func_value,
                    CLASS_ID_TEXT_DECODER_STREAM,
                );
            }
            "CompressionStream" => {
                let format = args
                    .first()
                    .copied()
                    .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
                validate_web_compression_stream_format(format);
                return text_encoding_stream_new_with_constructor(
                    func_value,
                    CLASS_ID_COMPRESSION_STREAM,
                );
            }
            "DecompressionStream" => {
                let format = args
                    .first()
                    .copied()
                    .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
                validate_web_compression_stream_format(format);
                return text_encoding_stream_new_with_constructor(
                    func_value,
                    CLASS_ID_DECOMPRESSION_STREAM,
                );
            }
            // #4950 (secondary note): react-reconciler captures the global
            // `AbortController` into a local (`AbortControllerLocal = typeof
            // AbortController !== "undefined" ? AbortController : <shim>`) and
            // constructs through the variable. Without this arm the dynamic
            // `new` fell through and threw "AbortController is not a function".
            "AbortController" => {
                let controller = crate::url::js_abort_controller_new();
                return crate::value::js_nanbox_pointer(controller as i64);
            }
            "MessageChannel" => {
                return crate::messaging::js_message_channel_new();
            }
            "MessagePort" => {
                return crate::messaging::js_message_port_constructor_error();
            }
            "Storage" => {
                return crate::web_storage::storage_constructor_illegal(std::ptr::null());
            }
            "BroadcastChannel" => {
                let name = args
                    .first()
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                return crate::messaging::js_broadcast_channel_new(name);
            }
            "URL" => {
                let input = args
                    .first()
                    .copied()
                    .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
                let input_ptr = crate::url::js_url_coerce_string(input);
                let url = if let Some(base) = args.get(1).copied() {
                    let base_ptr = crate::url::js_url_coerce_string(base);
                    crate::url::js_url_new_with_base(input_ptr, base_ptr)
                } else {
                    crate::url::js_url_new(input_ptr)
                };
                return crate::value::js_nanbox_pointer(url as i64);
            }
            "URLSearchParams" => {
                let params = if let Some(init) = args.first().copied() {
                    crate::url::js_url_search_params_new_any(init)
                } else {
                    crate::url::js_url_search_params_new_empty()
                };
                return crate::value::js_nanbox_pointer(params as i64);
            }
            "TextEncoder" => {
                let encoder = crate::text::js_text_encoder_new();
                return crate::value::js_nanbox_pointer(encoder);
            }
            "TextDecoder" => {
                let label = args
                    .first()
                    .copied()
                    .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
                let options = args
                    .get(1)
                    .copied()
                    .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
                let fatal = text_decoder_bool_option(options, "fatal");
                let ignore_bom = text_decoder_bool_option(options, "ignoreBOM");
                let decoder = crate::text::js_text_decoder_new(label, fatal, ignore_bom);
                return crate::value::js_nanbox_pointer(decoder);
            }
            _ => {}
        }
    }
    // #1789/#1787: `new (classObjectValue)(args)` — the callee is a heap
    // class object (the value a class EXPRESSION evaluates to, e.g.
    // `const C = mk(x); new C()`). Read its class_id (the compile-time
    // template) and allocate an instance stamped with it, so instance
    // methods dispatch and `x instanceof C` matches.
    //
    // #1787: then REPLAY the class's constructor on the instance. The
    // constructor can't be inlined at the `new` site — the callee is a
    // runtime value, and the class's captured environment lived where the
    // class EXPRESSION was evaluated (e.g. inside the `mk(tag)` factory),
    // not at the (possibly far-away) construction site. So the codegen
    // ClassExprFresh lowering snapshots those captures onto this class
    // object as the `__perry_ctor_caps` own array, and registers the
    // standalone `<prefix>__<class>_constructor` symbol in
    // `CLASS_CONSTRUCTORS`. Replaying it here runs the instance-field
    // initializers (literal AND captured) and the constructor body —
    // matching what the static `new ClassName()` path does inline.
    if is_class_object_value(func_value) {
        let obj =
            crate::value::JSValue::from_bits(func_value.to_bits()).as_pointer::<ObjectHeader>();
        let class_cid = js_object_get_class_id(obj);
        if class_cid != 0 {
            let inst = js_object_alloc(class_cid, 0);
            // Replay the class's registered constructor (instance-field
            // initializers + body) on the fresh instance, filling the
            // capture params from the snapshotted `__perry_ctor_caps`. The
            // mechanism lives in `class_constructors` to keep this file under
            // the 2,000-line CI gate.
            super::class_constructors::replay_class_object_constructor(
                func_value, class_cid, inst, args_ptr, args_len,
            );
            // `class X extends Request/Response {}` constructed via the dynamic
            // (class-expression value) path: the replayed ctor's `super()`
            // can't statically route an aliased parent, so attach the native
            // fetch handle here when the registered parent is a fetch builtin
            // and the instance didn't already get one. Refs `@hono/node-server`.
            if let Some(kind) = fetch_parent_kind_in_chain(class_cid) {
                if super::field_get_set::fetch_subclass_handle_id(inst as usize).is_none() {
                    super::attach_fetch_handle_for_construction(inst, kind, args_ptr, args_len);
                }
            }
            return crate::value::js_nanbox_pointer(inst as i64);
        }
    }

    // #321/#4530: `new C(args)` where `C` is a first-class ClassRef, including
    // proxy-forwarded construction. Allocate an instance stamped with the
    // registered class id and replay the standalone constructor so field
    // initializers and `this.foo = ...` writes match static `new ClassName()`.
    if let Some(class_cid) = constructor_class_ref_id(func_value) {
        return construct_registered_class_ref(class_cid, class_cid, args_ptr, args_len);
    }
    if is_arrow_function_value(func_value) {
        crate::fs::validate::throw_type_error_with_code(
            "Arrow function is not a constructor",
            "ERR_INVALID_ARG_TYPE",
        );
    }
    let cid = synthetic_class_id_for_function(func_value);
    // Allocate the instance with the synthetic class id (or 0 if the
    // value isn't callable). The object starts with no own props; the
    // constructor body fills `this.<field>` writes through
    // PropertySet, and prototype-method dispatch consults the
    // synthetic class id's entry in CLASS_PROTOTYPE_METHODS.
    let obj_ptr = js_object_alloc(cid, 0);
    let nan_boxed = crate::value::js_nanbox_pointer(obj_ptr as i64);
    // A user-assigned `foo.prototype = <obj/array>` lives as the closure's
    // "prototype" dynamic prop; the instance's [[Prototype]] must be THAT
    // value — notably a real array (`foo.prototype = new Array(1,2,3)`),
    // which `ensure_function_prototype_object` would shadow with a fresh
    // empty object (test262 filter/15.4.4.20-6-*, some/15.4.4.17-8-*).
    let mut linked_user_proto = false;
    {
        let fp = (func_value.to_bits() & crate::value::POINTER_MASK) as usize;
        if fp != 0 && crate::closure::is_closure_ptr(fp) {
            let dyn_proto = crate::closure::closure_get_dynamic_prop(fp, "prototype");
            let dp = JSValue::from_bits(dyn_proto.to_bits());
            if dp.is_pointer() {
                let raw = dp.as_pointer::<u8>() as usize;
                let is_array = raw >= crate::gc::GC_HEADER_SIZE + 0x1000 && {
                    let hdr = unsafe {
                        &*((raw - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader)
                    };
                    hdr.obj_type == crate::gc::GC_TYPE_ARRAY
                        || hdr.obj_type == crate::gc::GC_TYPE_LAZY_ARRAY
                };
                if is_array {
                    super::prototype_chain::object_set_static_prototype(
                        obj_ptr as usize,
                        dyn_proto.to_bits(),
                    );
                    linked_user_proto = true;
                }
            }
        }
    }
    if !linked_user_proto {
        let proto = ensure_function_prototype_object(func_value, cid);
        if !proto.is_null() {
            super::prototype_chain::object_set_static_prototype(
                obj_ptr as usize,
                crate::value::js_nanbox_pointer(proto as i64).to_bits(),
            );
        }
    }
    // Only run the constructor body when the callee is recognised as
    // a closure shape. The codegen LocalGet path widens the route to
    // any local-resolved callee, so we have to gate the
    // `js_native_call_value` dispatch on a verified closure pointer
    // here — otherwise `new <non-callable>()` would dereference an
    // arbitrary pointer as a `ClosureHeader` and crash.
    if is_callable_function_value(func_value) {
        // Bind `this` to the new instance, dispatch the constructor,
        // then restore the previous IMPLICIT_THIS. The dispatch
        // result is discarded — JS `new` semantics use the receiver,
        // not the returned value (object returns would override, but
        // dayjs and siblings rely on the receiver mutation pattern).
        let prev_this = crate::object::js_implicit_this_get();
        let prev_new_target = crate::object::js_new_target_get();
        crate::object::js_implicit_this_set(nan_boxed);
        crate::object::js_new_target_set(func_value);
        let prev_current_new_target =
            CURRENT_NEW_TARGET.with(|value| value.replace(func_value.to_bits()));
        let result = crate::closure::js_native_call_value(func_value, args_ptr, args_len);
        CURRENT_NEW_TARGET.with(|value| value.set(prev_current_new_target));
        crate::object::js_new_target_set(prev_new_target);
        crate::object::js_implicit_this_set(prev_this);
        if constructor_return_overrides_this(result) {
            return result;
        }
    }
    nan_boxed
}

/// `new <callee>(...spread)` — spread-bearing construction. Codegen builds a
/// single JS array containing every argument in evaluation order (regular args
/// pushed, spread sources expanded via `js_array_like_to_array` + concat), then
/// hands the array here. We materialise it into a flat `f64` buffer and forward
/// to `js_new_function_construct`, so the full callee-shape dispatch (primitive
/// → TypeError, proxy `construct` trap, boxed-wrapper TypeError, class refs,
/// closures, native module constructors) is shared with the non-spread path.
///
/// `args_array` is a NaN-boxed Array JSValue (POINTER_TAG). A null/0 handle is
/// treated as an empty argument list.
#[no_mangle]
pub unsafe extern "C" fn js_new_function_construct_apply(func_value: f64, args_array: f64) -> f64 {
    let arr_ptr = (args_array.to_bits() & crate::value::POINTER_MASK) as *const crate::ArrayHeader;
    if arr_ptr.is_null() {
        return js_new_function_construct(func_value, std::ptr::null::<f64>(), 0);
    }
    let len = crate::array::js_array_length(arr_ptr) as usize;
    let mut buf: Vec<f64> = Vec::with_capacity(len);
    for i in 0..len {
        let v = crate::array::js_array_get(arr_ptr, i as u32);
        buf.push(f64::from_bits(v.bits()));
    }
    let (ptr, n) = if buf.is_empty() {
        (std::ptr::null::<f64>(), 0usize)
    } else {
        (buf.as_ptr(), buf.len())
    };
    js_new_function_construct(func_value, ptr, n)
}

fn constructor_class_ref_id(value: f64) -> Option<u32> {
    if super::class_prototype_ref_id(value).is_some() {
        return None;
    }
    super::class_ref_id(value)
}

/// Spec `IsConstructor(value)` — used by `NewPromiseCapability` (the Promise
/// combinators) to validate the `this` constructor argument. Returns true for
/// registered class constructors, the reified builtin constructors, and plain
/// (non-arrow, non-builtin-method) function closures; false for primitives,
/// arrow functions, and non-constructable builtin functions (e.g. `eval`).
pub(crate) fn js_value_is_constructor(value: f64) -> bool {
    if constructor_class_ref_id(value).is_some() {
        return true;
    }
    if crate::proxy::js_proxy_is_proxy(value) == 1 {
        return true;
    }
    if !is_callable_function_value(value) {
        return false;
    }
    if is_arrow_function_value(value) {
        return false;
    }
    if is_non_constructable_builtin_function_value(value) {
        return false;
    }
    true
}

/// Spec ClassDefinitionEvaluation: a non-`null` superclass that is not a
/// constructor makes `class X extends <value>` throw a TypeError before any
/// `.prototype` access. Returns true when `value` is a *definitively* invalid
/// superclass (so the caller throws). `null` is a valid superclass (creates a
/// null-`[[Prototype]]` class) and never throws. Ambiguous heap values (not
/// recognized as callable) return false so legitimate dynamic-extends shapes
/// (mixins, factory-returned classes) keep their parentless baseline rather
/// than mis-throwing. (Test262 subclass/superclass-* and definition/invalid-extends.)
fn extends_target_must_throw(value: f64) -> bool {
    use crate::value::JSValue;
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_null() {
        return false;
    }
    // Registered class refs / heap class objects are constructors.
    if constructor_class_ref_id(value).is_some() || is_class_object_value(value) {
        return false;
    }
    // A Proxy is a constructor iff its `[[ProxyTarget]]` is — recurse.
    if crate::proxy::js_proxy_is_proxy(value) == 1 {
        return extends_target_must_throw(crate::proxy::js_proxy_target(value));
    }
    // Non-object primitives (number, string, boolean, undefined, symbol, bigint)
    // can never be a superclass.
    if !jv.is_pointer() {
        return true;
    }
    if is_callable_function_value(value) {
        if is_arrow_function_value(value) || is_non_constructable_builtin_function_value(value) {
            return true;
        }
        let ptr = jv.as_pointer::<crate::closure::ClosureHeader>();
        if !ptr.is_null() && is_valid_obj_ptr(ptr as *const u8) {
            // A bound *method* (class/instance method read as a value) is never
            // a constructor.
            if crate::closure::closure_is_bound_method(ptr) {
                return true;
            }
            let fp = crate::closure::get_valid_func_ptr(ptr);
            // A bound *function* (`fn.bind(...)`) is a constructor iff its bound
            // target is — recurse on the captured target.
            if fp == crate::closure::BOUND_FUNCTION_FUNC_PTR {
                let target = crate::closure::js_closure_get_capture_f64(ptr, 0);
                return extends_target_must_throw(target);
            }
            // Arrow / async / generator / async-generator function bodies are
            // non-constructors.
            if crate::closure::is_registered_arrow_function(fp)
                || crate::closure::is_registered_async_function(fp)
                || crate::closure::is_registered_generator_function(fp)
                || crate::closure::is_registered_async_generator_function(fp)
            {
                return true;
            }
        }
        // Ordinary function — a constructor.
        return false;
    }
    // A pointer we don't recognize as callable: stay conservative (no throw).
    false
}

fn class_object_class_id(value: f64) -> Option<u32> {
    if !is_class_object_value(value) {
        return None;
    }
    let obj = crate::value::JSValue::from_bits(value.to_bits()).as_pointer::<ObjectHeader>();
    let class_id = js_object_get_class_id(obj);
    if class_id != 0 && is_class_id_registered(class_id) {
        Some(class_id)
    } else {
        None
    }
}

fn new_target_class_id(new_target: f64) -> Option<u32> {
    constructor_class_ref_id(new_target).or_else(|| class_object_class_id(new_target))
}

unsafe fn construct_registered_class_ref(
    target_cid: u32,
    instance_cid: u32,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let inst = if let Some((keys_array, field_count)) = registered_class_keys_array(instance_cid) {
        js_object_alloc_class_inline_keys(instance_cid, 0, field_count, keys_array)
    } else {
        js_object_alloc(instance_cid, 0)
    };
    super::class_constructors::replay_registered_class_constructor(
        target_cid, inst, args_ptr, args_len,
    );
    // ClassRef `new` of a Request/Response subclass — attach the native fetch
    // handle on the dynamic path (mirrors the class-expression arm above).
    if let Some(kind) = fetch_parent_kind_in_chain(target_cid) {
        if super::field_get_set::fetch_subclass_handle_id(inst as usize).is_none() {
            super::attach_fetch_handle_for_construction(inst, kind, args_ptr, args_len);
        }
    }
    crate::value::js_nanbox_pointer(inst as i64)
}

/// `GetPrototypeFromConstructor(newTarget)` restricted to the "use it only when
/// it is an object" rule: returns `newTarget.prototype`'s bits when that value
/// is an object (so a typed-array view should adopt it as its `[[Prototype]]`),
/// or `None` when it is a primitive (so the default per-kind prototype applies).
fn new_target_custom_object_prototype(new_target: f64) -> Option<u64> {
    let bits = new_target.to_bits();
    if (bits >> 48) != 0x7FFD {
        return None;
    }
    let raw = (bits & crate::value::POINTER_MASK) as usize;
    if raw == 0 {
        return None;
    }
    let key = crate::string::js_string_from_bytes(b"prototype".as_ptr(), b"prototype".len() as u32);
    let proto = js_object_get_field_by_name_f64(raw as *const ObjectHeader, key);
    if unsafe { super::value_is_object_like(proto) } || super::class_ref_id(proto).is_some() {
        Some(proto.to_bits())
    } else {
        None
    }
}

fn constructor_prototype_bits(new_target: f64) -> Option<u64> {
    let bits = new_target.to_bits();
    if (bits >> 48) != 0x7FFD {
        return global_object_prototype_bits();
    }
    let raw = (bits & crate::value::POINTER_MASK) as usize;
    if raw == 0 {
        return global_object_prototype_bits();
    }
    let key = crate::string::js_string_from_bytes(b"prototype".as_ptr(), b"prototype".len() as u32);
    let proto = js_object_get_field_by_name_f64(raw as *const ObjectHeader, key);
    if unsafe { super::value_is_object_like(proto) } || super::class_ref_id(proto).is_some() {
        Some(proto.to_bits())
    } else {
        global_object_prototype_bits()
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_new_function_construct_with_new_target(
    func_value: f64,
    args_ptr: *const f64,
    args_len: usize,
    new_target: f64,
) -> f64 {
    let nt = if new_target.to_bits() == crate::value::TAG_UNDEFINED {
        func_value
    } else {
        new_target
    };
    if nt.to_bits() == func_value.to_bits() {
        return js_new_function_construct(func_value, args_ptr, args_len);
    }
    if crate::proxy::js_proxy_is_proxy(func_value) == 1 {
        let arr = crate::array::js_array_alloc(0);
        let mut a = arr;
        if !args_ptr.is_null() {
            for i in 0..args_len {
                a = crate::array::js_array_push_f64(a, *args_ptr.add(i));
            }
        }
        let arr_box = f64::from_bits(0x7FFD_0000_0000_0000 | (a as u64 & 0x0000_FFFF_FFFF_FFFF));
        return crate::proxy::js_proxy_construct(func_value, arr_box, nt);
    }
    if let Some(target_cid) = constructor_class_ref_id(func_value) {
        let instance_cid = new_target_class_id(nt).unwrap_or(target_cid);
        return construct_registered_class_ref(target_cid, instance_cid, args_ptr, args_len);
    }
    // `Reflect.construct(Int8Array, [len], newTarget)` — a typed-array
    // constructor invoked with a distinct newTarget. Build the typed array the
    // normal way, then honor `GetPrototypeFromConstructor(newTarget)`: when
    // `newTarget.prototype` is an object other than the default per-kind
    // prototype, record it as the instance's `[[Prototype]]` so
    // `Object.getPrototypeOf` and `.constructor` resolve through it (test262
    // `ctors*/use-custom-proto-if-object` / `use-default-proto-if-…`).
    if let Some(ta_name) = identify_global_builtin_constructor(func_value) {
        if matches!(
            ta_name,
            "Int8Array"
                | "Uint8Array"
                | "Uint8ClampedArray"
                | "Int16Array"
                | "Uint16Array"
                | "Int32Array"
                | "Uint32Array"
                | "Float16Array"
                | "Float32Array"
                | "Float64Array"
                | "BigInt64Array"
                | "BigUint64Array"
        ) {
            // Read `newTarget.prototype` (GetPrototypeFromConstructor) BEFORE
            // building the view: Node evaluates the proto access as part of
            // AllocateTypedArray, so a throwing `prototype` getter must surface
            // here even when later steps would also throw (test262
            // `throw-type-error-before-custom-proto-access` agreement).
            let proto_bits = new_target_custom_object_prototype(nt);
            let result = js_new_function_construct(func_value, args_ptr, args_len);
            if let Some(addr) = crate::typedarray_props::typed_array_addr_from_value(result) {
                if let Some(proto_bits) = proto_bits {
                    super::prototype_chain::object_set_static_prototype(addr, proto_bits);
                }
            }
            return result;
        }
    }
    if !is_callable_function_value(func_value) {
        return js_new_function_construct(func_value, args_ptr, args_len);
    }
    if is_non_constructable_builtin_function_value(func_value)
        || is_non_constructable_builtin_function_value(nt)
    {
        throw_non_constructable_builtin_function();
    }
    if is_arrow_function_value(func_value) {
        crate::fs::validate::throw_type_error_with_code(
            "Arrow function is not a constructor",
            "ERR_INVALID_ARG_TYPE",
        );
    }

    let obj_ptr = js_object_alloc(0, 0);
    let nan_boxed = crate::value::js_nanbox_pointer(obj_ptr as i64);
    if let Some(proto_bits) = constructor_prototype_bits(nt) {
        super::prototype_chain::object_set_static_prototype(obj_ptr as usize, proto_bits);
    }

    let prev_this = crate::object::js_implicit_this_get();
    let prev_new_target = crate::object::js_new_target_get();
    crate::object::js_implicit_this_set(nan_boxed);
    crate::object::js_new_target_set(nt);
    let prev_current_new_target = CURRENT_NEW_TARGET.with(|value| value.replace(nt.to_bits()));
    let result = crate::closure::js_native_call_value(func_value, args_ptr, args_len);
    CURRENT_NEW_TARGET.with(|value| value.set(prev_current_new_target));
    crate::object::js_new_target_set(prev_new_target);
    crate::object::js_implicit_this_set(prev_this);
    if constructor_return_overrides_this(result) {
        return result;
    }
    nan_boxed
}

fn constructor_return_overrides_this(value: f64) -> bool {
    use crate::value::JSValue;
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    if is_callable_function_value(value) {
        return true;
    }
    let raw = jv.as_pointer::<u8>();
    if raw.is_null() {
        return false;
    }
    if super::is_arguments_object(raw as *const ObjectHeader) {
        return true;
    }
    unsafe {
        let arr = crate::array::clean_arr_ptr(raw as *const crate::array::ArrayHeader);
        if !arr.is_null() {
            return true;
        }
        if !is_valid_obj_ptr(raw as *const u8) {
            return false;
        }
        let gc_header =
            (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        matches!(
            (*gc_header).obj_type,
            // Per spec, a constructor returning ANY Object overrides the
            // implicit `this`. Promises are objects — a user constructor like
            // `function P(exec){ return new Promise(...) }` (the
            // `NewPromiseCapability` shape exercised by the Promise-combinator
            // test262 cases) must yield that Promise, not the empty default.
            // GC_TYPE_TEMPORAL: `new Temporal.Duration(...)` (and every other
            // Temporal constructor) is dispatched through this generic path —
            // the constructor thunk allocates a Temporal cell and returns it, so
            // that cell must override the empty default `this` (#4687).
            crate::gc::GC_TYPE_OBJECT
                | crate::gc::GC_TYPE_ERROR
                | crate::gc::GC_TYPE_PROMISE
                | crate::gc::GC_TYPE_TEMPORAL
        )
    }
}

/// Apply ECMAScript constructor return-override semantics for an inlined
/// constructor body's explicit `return <value>`. Given the implicit `this`
/// and the returned value:
///   - returned value is an Object  → it becomes the construction result;
///   - returned value is `undefined` → result is `this`;
///   - returned value is any other primitive → for a derived constructor
///     (`class X extends Y`) this is a TypeError; for a base constructor the
///     primitive is ignored and the result is `this`.
/// `is_derived` is 1 for a class with an `extends` clause, 0 otherwise.
/// Refs class/subclass/derived-class-return-override-*.
#[no_mangle]
pub extern "C" fn js_ctor_return_override(this_val: f64, return_val: f64, is_derived: i32) -> f64 {
    use crate::value::JSValue;
    if constructor_return_overrides_this(return_val) {
        return return_val;
    }
    let jv = JSValue::from_bits(return_val.to_bits());
    if jv.is_undefined() {
        return this_val;
    }
    if is_derived != 0 {
        crate::collection_iter::throw_type_error(
            "Derived constructors may only return object or undefined",
        );
    }
    // Base constructor: a returned primitive is ignored.
    this_val
}

/// Verify that a JSValue is a NaN-boxed pointer to a registered
/// closure header. `js_native_call_value` itself doesn't validate the
/// pointer shape — it dereferences whatever lower-48 bits it gets — so
/// the `new <LocalGet>(args)` widened path here in
/// `js_new_function_construct` needs to gate the constructor dispatch
/// on a real closure to avoid SIGSEGV'ing on non-callable callees
/// (`new someObject()`, `new someStringVar()`, etc.). Uses the
/// `_reserved` magic word `crate::closure::CLOSURE_MAGIC` that every
/// `js_closure_alloc*` site stamps on allocation.
fn is_callable_function_value(value: f64) -> bool {
    use crate::value::JSValue;
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let ptr = jv.as_pointer() as *const crate::closure::ClosureHeader;
    if ptr.is_null() {
        return false;
    }
    if (ptr as usize) % std::mem::align_of::<crate::closure::ClosureHeader>() != 0 {
        return false;
    }
    if !is_valid_obj_ptr(ptr as *const u8) {
        return false;
    }
    unsafe { (*ptr).type_tag == crate::closure::CLOSURE_MAGIC }
}

fn is_arrow_function_value(value: f64) -> bool {
    use crate::value::JSValue;
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let ptr = jv.as_pointer() as *const crate::closure::ClosureHeader;
    if (ptr as usize) % std::mem::align_of::<crate::closure::ClosureHeader>() != 0 {
        return false;
    }
    if ptr.is_null() || !is_valid_obj_ptr(ptr as *const u8) {
        return false;
    }
    unsafe {
        if (*ptr).type_tag != crate::closure::CLOSURE_MAGIC {
            return false;
        }
    }
    crate::closure::closure_is_arrow(ptr)
}

/// Predicate-only sibling of `ordinary_function_prototype_value_for_read`:
/// would this function have an own `.prototype` slot? Crucially does NOT
/// materialize the prototype object — `fn.hasOwnProperty('prototype')` must
/// not lock the slot's attributes before a later
/// `Object.defineProperty(fn, "prototype", …)` (TypedArrayConstructors
/// custom-proto tests).
pub(crate) fn function_would_have_own_prototype(func_value: f64) -> bool {
    if !is_callable_function_value(func_value) || is_arrow_function_value(func_value) {
        return false;
    }
    if super::native_module::builtin_closure_is_non_constructable_value(func_value) {
        return false;
    }
    synthetic_class_id_for_function(func_value) != 0
}

pub(crate) fn ordinary_function_prototype_value_for_read(func_value: f64) -> Option<f64> {
    if !is_callable_function_value(func_value) || is_arrow_function_value(func_value) {
        return None;
    }
    // Bound-method / bound-function values (class method/getter/setter reads via
    // `C.prototype.m`, instance method reads, `fn.bind(...)`) are non-constructors
    // and have NO `prototype` own property (`C.prototype.m.prototype === undefined`,
    // `'prototype' in C.prototype.m === false`). (Test262 definition method/accessor
    // prop-desc.)
    //
    // #4973 exception: bound NATIVE-MODULE *class* exports (`http.Server`,
    // `https.Server`) are constructors in Node, and the util.inherits-era
    // subclass pattern reads their `.prototype` as a setPrototypeOf operand
    // (`Object.setPrototypeOf(testServer.prototype, http.Server.prototype)`).
    // Returning None here made that read `undefined` and the setPrototypeOf
    // threw "Object prototype may only be an Object or null". These exports
    // are cached singleton closures (NATIVE_CALLABLE_EXPORTS), so the
    // synthetic-class path below gives them a stable prototype object.
    {
        let jv = crate::value::JSValue::from_bits(func_value.to_bits());
        if jv.is_pointer() {
            let cptr = jv.as_pointer::<crate::closure::ClosureHeader>();
            if !cptr.is_null()
                && is_valid_obj_ptr(cptr as *const u8)
                && crate::closure::closure_is_bound_method(cptr)
            {
                let is_native_class_export = unsafe {
                    super::native_module::bound_native_callable_module_and_method(func_value)
                }
                .map(|(module, method)| {
                    matches!(module.as_str(), "http" | "https") && method == "Server"
                })
                .unwrap_or(false);
                if !is_native_class_export {
                    return None;
                }
            }
        }
    }
    // Built-in methods (`String.prototype.charAt`, `Array.prototype.map`, …) are
    // not constructors and have NO `prototype` own property — `String.prototype.
    // charAt.prototype === undefined` (ECMA-262: built-in non-constructor
    // functions don't get the auto-created `.prototype`). Don't lazily synthesize
    // one for them.
    if super::native_module::builtin_closure_is_non_constructable_value(func_value) {
        return None;
    }
    let cid = synthetic_class_id_for_function(func_value);
    if cid == 0 {
        return None;
    }
    let proto = ensure_function_prototype_object(func_value, cid);
    if proto.is_null() {
        return None;
    }
    Some(crate::value::js_nanbox_pointer(proto as i64))
}

#[no_mangle]
pub extern "C" fn js_function_prototype_value_for_read(func_value: f64) -> f64 {
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let jv = crate::value::JSValue::from_bits(func_value.to_bits());
    if !jv.is_pointer() {
        return undef;
    }
    let ptr = jv.as_pointer() as *const crate::closure::ClosureHeader;
    if ptr.is_null() || !is_valid_obj_ptr(ptr as *const u8) {
        return undef;
    }
    unsafe {
        if (*ptr).type_tag != crate::closure::CLOSURE_MAGIC {
            return undef;
        }
    }

    let closure_addr = ptr as usize;
    if crate::closure::closure_is_key_deleted(closure_addr, "prototype") {
        return undef;
    }
    let dynamic = crate::closure::closure_get_dynamic_prop(closure_addr, "prototype");
    if dynamic.to_bits() != crate::value::TAG_UNDEFINED {
        return dynamic;
    }
    if let Some(proto) = generator_function_prototype_of(closure_addr) {
        return proto;
    }
    ordinary_function_prototype_value_for_read(func_value).unwrap_or(undef)
}

/// Lookup helper: returns the registered prototype-method value for
/// `(class_id, name)`, or None if no assignment matched. Walks the
/// parent-class chain so methods registered on a base class are found
/// via subclass instances.
pub(crate) fn lookup_prototype_method(class_id: u32, name: &str) -> Option<f64> {
    let guard = CLASS_PROTOTYPE_METHODS.read().ok()?;
    let map = guard.as_ref()?;
    let mut cid = class_id;
    let mut depth = 0usize;
    while depth < 32 {
        if let Some(per_class) = map.get(&cid) {
            if let Some(&bits) = per_class.get(name) {
                return Some(f64::from_bits(bits));
            }
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    None
}

#[derive(Clone)]
enum ClassSideTableRootSlot {
    DynamicProp {
        class_id: u32,
        name: String,
    },
    PrototypeMethod {
        class_id: u32,
        name: String,
    },
    PrototypeMethodValue {
        class_id: u32,
        name: String,
    },
    PrototypeObject {
        class_id: u32,
    },
    ParentClosure {
        class_id: u32,
    },
    ClassSymbolMethod {
        class_id: u32,
        sym_key: usize,
        is_static: bool,
    },
    ClassSymbolAccessor {
        class_id: u32,
        sym_key: usize,
        is_static: bool,
    },
    FunctionClassIdKey {
        bits: u64,
    },
}

pub(crate) struct ClassSideTableRootScanState {
    slots: Vec<ClassSideTableRootSlot>,
    cursor: usize,
}

pub(crate) fn new_class_side_table_root_scan_state() -> Box<dyn std::any::Any> {
    Box::new(ClassSideTableRootScanState {
        slots: class_side_table_root_snapshot(),
        cursor: 0,
    })
}

pub(crate) fn scan_class_side_table_roots_mut_step(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    state: &mut dyn std::any::Any,
    remaining: &mut usize,
) -> bool {
    let state = state
        .downcast_mut::<ClassSideTableRootScanState>()
        .expect("class side-table root scanner state type");
    while *remaining > 0 && state.cursor < state.slots.len() {
        scan_class_side_table_root_slot(visitor, &state.slots[state.cursor]);
        state.cursor += 1;
        *remaining -= 1;
    }
    state.cursor >= state.slots.len()
}

pub fn scan_class_side_table_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_class_side_table_roots_mut(&mut visitor);
}

pub fn scan_class_side_table_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    CLASS_DYNAMIC_PROPS.with(|m| {
        let mut m = m.borrow_mut();
        for props in m.values_mut() {
            for value in props.values_mut() {
                visitor.visit_nanbox_f64_slot(value);
            }
        }
    });

    if let Ok(mut guard) = CLASS_PROTOTYPE_METHODS.write() {
        if let Some(map) = guard.as_mut() {
            for methods in map.values_mut() {
                for value_bits in methods.values_mut() {
                    visitor.visit_nanbox_u64_slot(value_bits);
                }
            }
        }
    }

    CLASS_PROTOTYPE_METHOD_VALUES.with(|cache| {
        let mut cache = cache.borrow_mut();
        for value_bits in cache.values_mut() {
            visitor.visit_nanbox_u64_slot(value_bits);
        }
    });

    if let Ok(mut guard) = CLASS_PROTOTYPE_OBJECTS.write() {
        if let Some(map) = guard.as_mut() {
            for proto_addr in map.values_mut() {
                visitor.visit_usize_slot(proto_addr);
            }
        }
    }

    if let Ok(mut guard) = CLASS_PARENT_CLOSURES.write() {
        if let Some(map) = guard.as_mut() {
            for closure_addr in map.values_mut() {
                visitor.visit_usize_slot(closure_addr);
            }
        }
    }

    // The dynamic-parent value stash (`class X extends _mod.default`) holds
    // raw NaN-boxed parent-constructor bits. For a ClassRef (INT32-tagged)
    // parent this is inert, but a function/object parent (Effect's
    // `extends <runtime value>`) is a live heap pointer that a moving GC must
    // visit + forward — otherwise `js_get_dynamic_parent_value` later hands
    // `super()` a stale pointer.
    if let Ok(mut guard) = CLASS_DYNAMIC_PARENT_VALUE.write() {
        if let Some(map) = guard.as_mut() {
            for value_bits in map.values_mut() {
                visitor.visit_nanbox_u64_slot(value_bits);
            }
        }
    }

    scan_class_symbol_member_keys_mut(visitor);
    scan_function_class_id_keys_mut(visitor);
}

fn scan_class_symbol_member_keys_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    if let Ok(mut guard) = CLASS_SYMBOL_METHODS.write() {
        if let Some(map) = guard.as_mut() {
            let mut rewrites = Vec::new();
            for key in map.keys().copied().collect::<Vec<_>>() {
                let (class_id, sym_key, is_static) = key;
                let mut new_sym_key = sym_key;
                if visitor.visit_usize_slot(&mut new_sym_key) && new_sym_key != sym_key {
                    rewrites.push((key, (class_id, new_sym_key, is_static)));
                }
            }
            for (old_key, new_key) in rewrites {
                if let Some(entry) = map.remove(&old_key) {
                    map.insert(new_key, entry);
                }
            }
        }
    }
    if let Ok(mut guard) = CLASS_SYMBOL_ACCESSORS.write() {
        if let Some(map) = guard.as_mut() {
            let mut rewrites = Vec::new();
            for key in map.keys().copied().collect::<Vec<_>>() {
                let (class_id, sym_key, is_static) = key;
                let mut new_sym_key = sym_key;
                if visitor.visit_usize_slot(&mut new_sym_key) && new_sym_key != sym_key {
                    rewrites.push((key, (class_id, new_sym_key, is_static)));
                }
            }
            for (old_key, new_key) in rewrites {
                if let Some(entry) = map.remove(&old_key) {
                    map.insert(new_key, entry);
                }
            }
        }
    }
}

fn class_side_table_root_snapshot() -> Vec<ClassSideTableRootSlot> {
    let mut slots = Vec::new();

    CLASS_DYNAMIC_PROPS.with(|m| {
        let m = m.borrow();
        for (&class_id, props) in m.iter() {
            for name in props.keys() {
                slots.push(ClassSideTableRootSlot::DynamicProp {
                    class_id,
                    name: name.clone(),
                });
            }
        }
    });

    if let Ok(guard) = CLASS_PROTOTYPE_METHODS.read() {
        if let Some(map) = guard.as_ref() {
            for (&class_id, methods) in map.iter() {
                for name in methods.keys() {
                    slots.push(ClassSideTableRootSlot::PrototypeMethod {
                        class_id,
                        name: name.clone(),
                    });
                }
            }
        }
    }

    CLASS_PROTOTYPE_METHOD_VALUES.with(|cache| {
        let cache = cache.borrow();
        for ((class_id, name), _) in cache.iter() {
            slots.push(ClassSideTableRootSlot::PrototypeMethodValue {
                class_id: *class_id,
                name: name.clone(),
            });
        }
    });

    if let Ok(guard) = CLASS_PROTOTYPE_OBJECTS.read() {
        if let Some(map) = guard.as_ref() {
            for &class_id in map.keys() {
                slots.push(ClassSideTableRootSlot::PrototypeObject { class_id });
            }
        }
    }

    if let Ok(guard) = CLASS_PARENT_CLOSURES.read() {
        if let Some(map) = guard.as_ref() {
            for &class_id in map.keys() {
                slots.push(ClassSideTableRootSlot::ParentClosure { class_id });
            }
        }
    }

    if let Ok(guard) = CLASS_SYMBOL_METHODS.read() {
        if let Some(map) = guard.as_ref() {
            for &(class_id, sym_key, is_static) in map.keys() {
                slots.push(ClassSideTableRootSlot::ClassSymbolMethod {
                    class_id,
                    sym_key,
                    is_static,
                });
            }
        }
    }

    if let Ok(guard) = CLASS_SYMBOL_ACCESSORS.read() {
        if let Some(map) = guard.as_ref() {
            for &(class_id, sym_key, is_static) in map.keys() {
                slots.push(ClassSideTableRootSlot::ClassSymbolAccessor {
                    class_id,
                    sym_key,
                    is_static,
                });
            }
        }
    }

    if let Ok(guard) = FUNCTION_CLASS_IDS.read() {
        if let Some(map) = guard.as_ref() {
            for &bits in map.keys() {
                slots.push(ClassSideTableRootSlot::FunctionClassIdKey { bits });
            }
        }
    }

    slots
}

fn scan_class_side_table_root_slot(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    slot: &ClassSideTableRootSlot,
) {
    match slot {
        ClassSideTableRootSlot::DynamicProp { class_id, name } => {
            CLASS_DYNAMIC_PROPS.with(|m| {
                if let Some(value) = m
                    .borrow_mut()
                    .get_mut(class_id)
                    .and_then(|props| props.get_mut(name))
                {
                    visitor.visit_nanbox_f64_slot(value);
                }
            });
        }
        ClassSideTableRootSlot::PrototypeMethod { class_id, name } => {
            if let Ok(mut guard) = CLASS_PROTOTYPE_METHODS.write() {
                if let Some(value_bits) = guard
                    .as_mut()
                    .and_then(|map| map.get_mut(class_id))
                    .and_then(|methods| methods.get_mut(name))
                {
                    visitor.visit_nanbox_u64_slot(value_bits);
                }
            }
        }
        ClassSideTableRootSlot::PrototypeMethodValue { class_id, name } => {
            CLASS_PROTOTYPE_METHOD_VALUES.with(|cache| {
                if let Some(value_bits) = cache.borrow_mut().get_mut(&(*class_id, name.clone())) {
                    visitor.visit_nanbox_u64_slot(value_bits);
                }
            });
        }
        ClassSideTableRootSlot::PrototypeObject { class_id } => {
            if let Ok(mut guard) = CLASS_PROTOTYPE_OBJECTS.write() {
                if let Some(proto_addr) = guard.as_mut().and_then(|map| map.get_mut(class_id)) {
                    visitor.visit_usize_slot(proto_addr);
                }
            }
        }
        ClassSideTableRootSlot::ParentClosure { class_id } => {
            if let Ok(mut guard) = CLASS_PARENT_CLOSURES.write() {
                if let Some(closure_addr) = guard.as_mut().and_then(|map| map.get_mut(class_id)) {
                    visitor.visit_usize_slot(closure_addr);
                }
            }
        }
        ClassSideTableRootSlot::ClassSymbolMethod {
            class_id,
            sym_key,
            is_static,
        } => {
            rewrite_class_symbol_method_key_if_forwarded(visitor, *class_id, *sym_key, *is_static);
        }
        ClassSideTableRootSlot::ClassSymbolAccessor {
            class_id,
            sym_key,
            is_static,
        } => {
            rewrite_class_symbol_accessor_key_if_forwarded(
                visitor, *class_id, *sym_key, *is_static,
            );
        }
        ClassSideTableRootSlot::FunctionClassIdKey { bits } => {
            rewrite_function_class_id_key_if_forwarded(visitor, *bits);
        }
    }
}

fn rewrite_class_symbol_method_key_if_forwarded(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    class_id: u32,
    sym_key: usize,
    is_static: bool,
) {
    let mut new_sym_key = sym_key;
    if !visitor.visit_usize_slot(&mut new_sym_key) || new_sym_key == sym_key {
        return;
    }
    if let Ok(mut guard) = CLASS_SYMBOL_METHODS.write() {
        if let Some(map) = guard.as_mut() {
            if let Some(entry) = map.remove(&(class_id, sym_key, is_static)) {
                map.insert((class_id, new_sym_key, is_static), entry);
            }
        }
    }
}

fn rewrite_class_symbol_accessor_key_if_forwarded(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    class_id: u32,
    sym_key: usize,
    is_static: bool,
) {
    let mut new_sym_key = sym_key;
    if !visitor.visit_usize_slot(&mut new_sym_key) || new_sym_key == sym_key {
        return;
    }
    if let Ok(mut guard) = CLASS_SYMBOL_ACCESSORS.write() {
        if let Some(map) = guard.as_mut() {
            if let Some(entry) = map.remove(&(class_id, sym_key, is_static)) {
                map.insert((class_id, new_sym_key, is_static), entry);
            }
        }
    }
}

fn scan_function_class_id_keys_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    if !visitor.is_metadata_rewrite_phase() {
        return;
    }
    let mut rewrites = Vec::new();
    if let Ok(mut guard) = FUNCTION_CLASS_IDS.write() {
        let Some(map) = guard.as_mut() else {
            return;
        };
        for old_bits in map.keys().copied().collect::<Vec<_>>() {
            let mut new_bits = old_bits;
            if visit_metadata_nanbox_key(visitor, &mut new_bits) && new_bits != old_bits {
                rewrites.push((old_bits, new_bits));
            }
        }
        for (old_bits, new_bits) in rewrites {
            if let Some(class_id) = map.remove(&old_bits) {
                map.insert(new_bits, class_id);
            }
        }
    }
}

fn rewrite_function_class_id_key_if_forwarded(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    old_bits: u64,
) {
    if !visitor.is_metadata_rewrite_phase() {
        return;
    }
    let mut new_bits = old_bits;
    if !visit_metadata_nanbox_key(visitor, &mut new_bits) || new_bits == old_bits {
        return;
    }
    if let Ok(mut guard) = FUNCTION_CLASS_IDS.write() {
        if let Some(map) = guard.as_mut() {
            if let Some(class_id) = map.remove(&old_bits) {
                map.insert(new_bits, class_id);
            }
        }
    }
}

fn visit_metadata_nanbox_key(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    bits: &mut u64,
) -> bool {
    let tag = *bits & crate::value::TAG_MASK;
    if tag != crate::value::POINTER_TAG
        && tag != crate::value::STRING_TAG
        && tag != crate::value::BIGINT_TAG
    {
        return false;
    }
    let mut addr = (*bits & crate::value::POINTER_MASK) as usize;
    if visitor.visit_metadata_usize_slot(&mut addr) {
        *bits = tag | (addr as u64 & crate::value::POINTER_MASK);
        true
    } else {
        false
    }
}

#[cfg(test)]
pub(crate) fn test_clear_class_side_table_roots() {
    CLASS_DYNAMIC_PROPS.with(|m| m.borrow_mut().clear());
    CLASS_DELETED_KEYS.with(|m| m.borrow_mut().clear());
    CLASS_PROTOTYPE_METHOD_VALUES.with(|cache| cache.borrow_mut().clear());
    if let Ok(mut guard) = CLASS_PROTOTYPE_METHODS.write() {
        *guard = None;
    }
    CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED.store(false, std::sync::atomic::Ordering::Release);
    if let Ok(mut guard) = FUNCTION_CLASS_IDS.write() {
        *guard = None;
    }
    if let Ok(mut guard) = CLASS_PROTOTYPE_OBJECTS.write() {
        *guard = None;
    }
    if let Ok(mut guard) = CLASS_PARENT_CLOSURES.write() {
        *guard = None;
    }
    if let Ok(mut guard) = CLASS_SYMBOL_METHODS.write() {
        *guard = None;
    }
    if let Ok(mut guard) = CLASS_SYMBOL_ACCESSORS.write() {
        *guard = None;
    }
    if let Ok(mut guard) = CLASS_STATIC_ACCESSORS.write() {
        *guard = None;
    }
    NEXT_SYNTHETIC_CLASS_ID.store(0x8000_0000, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(test)]
pub(crate) fn test_seed_class_dynamic_prop_root(class_id: u32, name: &str, value_bits: u64) {
    class_dynamic_prop_root_store(class_id, name.to_string(), f64::from_bits(value_bits));
}

#[cfg(test)]
pub(crate) fn test_class_dynamic_prop_root_bits(class_id: u32, name: &str) -> u64 {
    CLASS_DYNAMIC_PROPS.with(|m| {
        m.borrow()
            .get(&class_id)
            .and_then(|props| props.get(name))
            .map(|value| value.to_bits())
            .unwrap_or(0)
    })
}

#[cfg(test)]
pub(crate) fn test_seed_class_prototype_method_root(class_id: u32, name: &str, value_bits: u64) {
    class_prototype_method_root_store(class_id, name.to_string(), value_bits);
}

#[cfg(test)]
pub(crate) fn test_class_prototype_method_root_bits(class_id: u32, name: &str) -> u64 {
    CLASS_PROTOTYPE_METHODS
        .read()
        .ok()
        .and_then(|guard| {
            guard
                .as_ref()
                .and_then(|map| map.get(&class_id))
                .and_then(|methods| methods.get(name))
                .copied()
        })
        .unwrap_or(0)
}

#[cfg(test)]
pub(crate) fn test_seed_class_prototype_method_value_root(
    class_id: u32,
    name: &str,
    value_bits: u64,
) {
    class_prototype_method_value_cache_root_store(class_id, name.to_string(), value_bits);
}

#[cfg(test)]
pub(crate) fn test_class_prototype_method_value_root_bits(class_id: u32, name: &str) -> u64 {
    CLASS_PROTOTYPE_METHOD_VALUES.with(|cache| {
        cache
            .borrow()
            .get(&(class_id, name.to_string()))
            .copied()
            .unwrap_or(0)
    })
}

#[cfg(test)]
pub(crate) fn test_seed_class_prototype_object_root(class_id: u32, addr: usize) {
    class_prototype_object_root_store(class_id, addr as *mut ObjectHeader);
}

#[cfg(test)]
pub(crate) fn test_class_prototype_object_root_addr(class_id: u32) -> usize {
    CLASS_PROTOTYPE_OBJECTS
        .read()
        .ok()
        .and_then(|guard| guard.as_ref().and_then(|map| map.get(&class_id).copied()))
        .unwrap_or(0)
}

#[cfg(test)]
pub(crate) fn test_seed_class_parent_closure_root(class_id: u32, addr: usize) {
    class_parent_closure_root_store(class_id, addr);
}

#[cfg(test)]
pub(crate) fn test_class_parent_closure_root_addr(class_id: u32) -> usize {
    CLASS_PARENT_CLOSURES
        .read()
        .ok()
        .and_then(|guard| guard.as_ref().and_then(|map| map.get(&class_id).copied()))
        .unwrap_or(0)
}

#[cfg(test)]
pub(crate) fn test_seed_function_class_id_key(func_bits: u64, class_id: u32) {
    let mut guard = FUNCTION_CLASS_IDS.write().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard.as_mut().unwrap().insert(func_bits, class_id);
}

#[cfg(test)]
pub(crate) fn test_function_class_id_key_for_class(class_id: u32) -> u64 {
    FUNCTION_CLASS_IDS
        .read()
        .ok()
        .and_then(|guard| {
            guard.as_ref().and_then(|map| {
                map.iter()
                    .find_map(|(&bits, &cid)| (cid == class_id).then_some(bits))
            })
        })
        .unwrap_or(0)
}

/// Returns true if `class_id` corresponds to a registered class. Used by
/// `js_value_typeof` (refs #618 / #420 followup) to distinguish a class
/// reference (NaN-boxed INT32 with class_id payload) from a regular int32
/// numeric value — JS spec says `typeof <class>` is "function", but
/// Perry's INT32_TAG storage shape is shared with numeric int32, so the
/// runtime needs an explicit registry check. Consults both
/// REGISTERED_CLASS_IDS (every class) and CLASS_VTABLE_REGISTRY (classes
/// with methods) so even classes registered before the explicit-id call
/// runs still detect via the vtable.
pub fn is_class_id_registered(class_id: u32) -> bool {
    if class_id == 0 {
        return false;
    }
    if let Ok(guard) = REGISTERED_CLASS_IDS.read() {
        if let Some(set) = guard.as_ref() {
            if set.contains(&class_id) {
                return true;
            }
        }
    }
    let registry = match CLASS_VTABLE_REGISTRY.read() {
        Ok(g) => g,
        Err(_) => return false,
    };
    registry
        .as_ref()
        .map(|m| m.contains_key(&class_id))
        .unwrap_or(false)
}

/// Register a class method in the vtable registry.
/// Called at startup from the init function for every class method/getter.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_method(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    func_ptr: i64,
    param_count: i64,
    has_synthetic_arguments: i64,
    has_rest: i64,
) {
    // `name_len == 0` is a legal empty-string member key (`get ''()`), so only
    // reject a negative length / null pointer.
    let name = if name_ptr.is_null() || name_len < 0 {
        return;
    } else {
        match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len as usize)) {
            Ok(s) => s.to_string(),
            Err(_) => return,
        }
    };
    let mut registry = CLASS_VTABLE_REGISTRY.write().unwrap();
    if registry.is_none() {
        *registry = Some(HashMap::new());
    }
    let reg = registry.as_mut().unwrap();
    let vtable = reg.entry(class_id as u32).or_insert_with(|| ClassVTable {
        methods: HashMap::new(),
        getters: HashMap::new(),
        setters: HashMap::new(),
    });
    vtable.methods.insert(
        name,
        VTableMethodEntry {
            func_ptr: func_ptr as usize,
            param_count: param_count as u32,
            has_synthetic_arguments: has_synthetic_arguments != 0,
            has_rest: has_rest != 0,
        },
    );
    VTABLE_GEN.fetch_add(1, Ordering::Release);
}

/// Own (non-inherited) instance accessor func_ptrs for `class_id` + `name`:
/// `(getter_ptr, setter_ptr)`, each 0 when that half is absent. Consulted by
/// `Object.getOwnPropertyDescriptor(C.prototype, name)`.
pub(crate) fn class_own_accessor_ptrs(class_id: u32, name: &str) -> Option<(usize, usize)> {
    let guard = CLASS_VTABLE_REGISTRY.read().ok()?;
    let reg = guard.as_ref()?;
    let vt = reg.get(&class_id)?;
    let g = vt.getters.get(name).copied().unwrap_or(0);
    let s = vt.setters.get(name).copied().unwrap_or(0);
    if g == 0 && s == 0 {
        None
    } else {
        Some((g, s))
    }
}

/// Own static accessor func_ptrs for the class *constructor*. Mirrors
/// `class_own_accessor_ptrs` against `CLASS_STATIC_ACCESSORS`.
pub(crate) fn class_own_static_accessor_ptrs(class_id: u32, name: &str) -> Option<(usize, usize)> {
    let guard = CLASS_STATIC_ACCESSORS.read().ok()?;
    let reg = guard.as_ref()?;
    let pair = reg.get(&class_id)?.get(name).copied()?;
    if pair.0 == 0 && pair.1 == 0 {
        None
    } else {
        Some(pair)
    }
}

/// Trampoline giving a raw vtable getter func_ptr (`fn(this) -> f64`) the
/// closure calling convention. The receiver comes from `IMPLICIT_THIS`, set
/// by the method-call dispatch the closure value travels through.
extern "C" fn class_accessor_getter_thunk(closure: *const crate::closure::ClosureHeader) -> f64 {
    let raw = unsafe { crate::closure::js_closure_get_capture_ptr(closure, 0) } as usize;
    if raw == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let this = crate::object::js_implicit_this_get();
    let f: extern "C" fn(f64) -> f64 = unsafe { std::mem::transmute(raw) };
    f(this)
}

/// Trampoline for a raw vtable setter func_ptr (`fn(this, value) -> f64`).
extern "C" fn class_accessor_setter_thunk(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let raw = unsafe { crate::closure::js_closure_get_capture_ptr(closure, 0) } as usize;
    if raw == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let this = crate::object::js_implicit_this_get();
    let f: extern "C" fn(f64, f64) -> f64 = unsafe { std::mem::transmute(raw) };
    f(this, value)
}

/// Wrap a raw class accessor func_ptr as a callable function VALUE for
/// descriptor reflection (`Object.getOwnPropertyDescriptor(C.prototype,
/// "x").get`). Built-in-shaped: `.length` 0/1, no `.prototype`, native
/// `toString` form.
pub(crate) fn class_accessor_function_value(raw_ptr: usize, is_setter: bool) -> f64 {
    if raw_ptr == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let thunk = if is_setter {
        class_accessor_setter_thunk as *const u8
    } else {
        class_accessor_getter_thunk as *const u8
    };
    let closure = crate::closure::js_closure_alloc(thunk, 1);
    if closure.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    unsafe { crate::closure::js_closure_set_capture_ptr(closure, 0, raw_ptr as i64) };
    super::native_module::set_builtin_closure_length(
        closure as usize,
        if is_setter { 1 } else { 0 },
    );
    super::native_module::set_builtin_closure_non_constructable(closure as usize);
    crate::gc::runtime_write_barrier_root_heap_word(closure as u64);
    crate::value::js_nanbox_pointer(closure as i64)
}

/// Register a class getter in the vtable registry.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_getter(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    func_ptr: i64,
) {
    // `name_len == 0` is a legal empty-string member key (`get ''()`), so only
    // reject a negative length / null pointer.
    let name = if name_ptr.is_null() || name_len < 0 {
        return;
    } else {
        match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len as usize)) {
            Ok(s) => s.to_string(),
            Err(_) => return,
        }
    };
    let mut registry = CLASS_VTABLE_REGISTRY.write().unwrap();
    if registry.is_none() {
        *registry = Some(HashMap::new());
    }
    let reg = registry.as_mut().unwrap();
    let vtable = reg.entry(class_id as u32).or_insert_with(|| ClassVTable {
        methods: HashMap::new(),
        getters: HashMap::new(),
        setters: HashMap::new(),
    });
    vtable.getters.insert(name, func_ptr as usize);
    VTABLE_GEN.fetch_add(1, Ordering::Release);
}

/// Register a class setter in the vtable registry.
///
/// Refs #486 (hono): hono's Context has `set res(_res) { ...; this.#res = _res;
/// this.finalized = true; }`. Without setter dispatch in `js_object_set_field_by_name`,
/// `c.res = response` from inside compose's `await handler(c, next)` chain stored
/// the response into a regular field slot but never ran the setter body — so
/// `this.finalized = true` never executed, `c.finalized` stayed false, and
/// hono-base's `if (!context.finalized) throw …` fired.
///
/// Setter signature: `fn(this_f64, value_f64) -> f64` (returns ignored, but
/// codegen emits a return so the LLVM signature matches a regular method body).
#[no_mangle]
pub unsafe extern "C" fn js_register_class_setter(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    func_ptr: i64,
) {
    // `name_len == 0` is a legal empty-string member key (`get ''()`), so only
    // reject a negative length / null pointer.
    let name = if name_ptr.is_null() || name_len < 0 {
        return;
    } else {
        match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len as usize)) {
            Ok(s) => s.to_string(),
            Err(_) => return,
        }
    };
    let mut registry = CLASS_VTABLE_REGISTRY.write().unwrap();
    if registry.is_none() {
        *registry = Some(HashMap::new());
    }
    let reg = registry.as_mut().unwrap();
    let vtable = reg.entry(class_id as u32).or_insert_with(|| ClassVTable {
        methods: HashMap::new(),
        getters: HashMap::new(),
        setters: HashMap::new(),
    });
    vtable.setters.insert(name, func_ptr as usize);
    VTABLE_GEN.fetch_add(1, Ordering::Release);
}

/// Register a `static get name()` accessor on the class *constructor*
/// (`CLASS_STATIC_ACCESSORS`), not the instance vtable — a static accessor is
/// an own property of `C`, reachable via `C.name` / `C[name]`, and must NOT
/// appear on `C.prototype` or instances. The read/write dispatch already
/// consults `CLASS_STATIC_ACCESSORS` (`class_static_accessor_getter_value` /
/// `class_static_accessor_setter_apply`); this populates it.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_static_getter(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    func_ptr: i64,
) {
    register_class_static_accessor_half(class_id, name_ptr, name_len, func_ptr, true);
}

/// Register a `static set name(v)` accessor. See `js_register_class_static_getter`.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_static_setter(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    func_ptr: i64,
) {
    register_class_static_accessor_half(class_id, name_ptr, name_len, func_ptr, false);
}

// These two are only ever called from codegen-emitted module-init IR (no Rust
// caller), so the auto-optimize whole-program-LLVM build would dead-strip them
// without an anchor. Pin each via a `#[used]` static (mirrors node_v8.rs).
#[used]
static KEEP_REGISTER_STATIC_GETTER: unsafe extern "C" fn(i64, *const u8, i64, i64) =
    js_register_class_static_getter;
#[used]
static KEEP_REGISTER_STATIC_SETTER: unsafe extern "C" fn(i64, *const u8, i64, i64) =
    js_register_class_static_setter;

/// Record the spec `.length` (params before the first default/rest) for a class
/// method or accessor. Codegen emits one call per method at module init.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_method_bind_length(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    length: i64,
) {
    if name_ptr.is_null() || name_len < 0 {
        return;
    }
    let name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len as usize)) {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };
    let mut guard = match CLASS_METHOD_BIND_LENGTHS.write() {
        Ok(g) => g,
        Err(_) => return,
    };
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard
        .as_mut()
        .unwrap()
        .insert((class_id as u32, name), length as u32);
}

#[used]
static KEEP_REGISTER_METHOD_BIND_LENGTH: unsafe extern "C" fn(i64, *const u8, i64, i64) =
    js_register_class_method_bind_length;

/// Record the spec `.length` for a STATIC method (params before the first
/// default/rest). Codegen emits one call per static method at module init.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_static_method_bind_length(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    length: i64,
) {
    if name_ptr.is_null() || name_len < 0 {
        return;
    }
    let name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len as usize)) {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };
    let mut guard = match CLASS_STATIC_METHOD_BIND_LENGTHS.write() {
        Ok(g) => g,
        Err(_) => return,
    };
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard
        .as_mut()
        .unwrap()
        .insert((class_id as u32, name), length as u32);
}

#[used]
static KEEP_REGISTER_STATIC_METHOD_BIND_LENGTH: unsafe extern "C" fn(i64, *const u8, i64, i64) =
    js_register_class_static_method_bind_length;

unsafe fn register_class_static_accessor_half(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    func_ptr: i64,
    is_getter: bool,
) {
    // Empty-string keys (`static get ''()`) are legal — admit `name_len == 0`
    // as long as the pointer is non-null.
    let name = if name_ptr.is_null() || name_len < 0 {
        return;
    } else {
        match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len as usize)) {
            Ok(s) => s.to_string(),
            Err(_) => return,
        }
    };
    let mut guard = CLASS_STATIC_ACCESSORS.write().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    let entry = guard
        .as_mut()
        .unwrap()
        .entry(class_id as u32)
        .or_default()
        .entry(name)
        .or_insert((0, 0));
    if is_getter {
        entry.0 = func_ptr as usize;
    } else {
        entry.1 = func_ptr as usize;
    }
    VTABLE_GEN.fetch_add(1, Ordering::Release);
}

// ============================================================================
// Per-callsite-keyed inline cache for vtable method dispatch.
//
// `js_native_call_method` is the hot dispatch tower for cross-module class
// instance method calls (e.g. `archetype.set(...)` from CommandBuffer.execute
// in the ECS workloads). Per profile, ~12% of perf-comprehensive samples land
// in `core::hash::BuildHasher` from the per-call `HashMap.get(method_name)`
// SipHash on the vtable lookup.
//
// Cache key: `(class_id, method_name_ptr)` where `method_name_ptr` is the
// rodata byte-pointer perry-codegen passes for the interned method name. The
// pointer is stable across calls within a module, so its address acts as a
// faster identity than re-hashing the bytes. Different modules may produce
// different rodata copies of the same name — the cache simply gets one entry
// per (class_id, name_pointer) pair, no correctness impact.
//
// Invalidation: a global `VTABLE_GEN` atomic is bumped on every
// `js_register_class_method` / `js_register_class_getter`. Each cache entry
// records the gen at populate time; lookups skip stale entries. Registration
// is one-shot at init in practice, so steady-state lookups never miss on
// gen.
// ============================================================================

static VTABLE_GEN: AtomicU64 = AtomicU64::new(1);

const VTABLE_IC_SIZE: usize = 4096;
const VTABLE_IC_MASK: usize = VTABLE_IC_SIZE - 1;

#[repr(C)]
#[derive(Copy, Clone)]
struct VTableICEntry {
    gen: u64,
    class_id: u32,
    _pad: u32,
    method_name_ptr: usize,
    func_ptr: usize,
    param_count: u32,
    has_synthetic_arguments: u32,
    has_rest: u32,
}

const EMPTY_VTABLE_IC_ENTRY: VTableICEntry = VTableICEntry {
    gen: 0,
    class_id: 0,
    _pad: 0,
    method_name_ptr: 0,
    func_ptr: 0,
    param_count: 0,
    has_synthetic_arguments: 0,
    has_rest: 0,
};

thread_local! {
    static VTABLE_IC: UnsafeCell<[VTableICEntry; VTABLE_IC_SIZE]> = const {
        UnsafeCell::new([EMPTY_VTABLE_IC_ENTRY; VTABLE_IC_SIZE])
    };
}

#[inline(always)]
fn vtable_ic_slot(class_id: u32, method_name_ptr: usize) -> usize {
    // Mix class_id into the upper bits of the pointer to spread (class, name)
    // pairs across slots. method_name_ptr is at least 1-byte aligned but
    // typically 8+ for rodata strings, so shift by 3 to drop the alignment
    // zeros before masking.
    let key = method_name_ptr
        .rotate_left(13)
        .wrapping_add((class_id as usize).wrapping_mul(0x9E37_79B9));
    (key >> 3) & VTABLE_IC_MASK
}

#[inline(always)]
pub(crate) unsafe fn vtable_ic_lookup(
    class_id: u32,
    method_name_ptr: usize,
) -> Option<(usize, u32, bool, bool)> {
    if method_name_ptr == 0 {
        return None;
    }
    let cur_gen = VTABLE_GEN.load(Ordering::Relaxed);
    let slot = vtable_ic_slot(class_id, method_name_ptr);
    VTABLE_IC.with(|cell| {
        let cache = &*cell.get();
        let entry = &cache[slot];
        if entry.gen == cur_gen
            && entry.class_id == class_id
            && entry.method_name_ptr == method_name_ptr
        {
            Some((
                entry.func_ptr,
                entry.param_count,
                entry.has_synthetic_arguments != 0,
                entry.has_rest != 0,
            ))
        } else {
            None
        }
    })
}

#[inline(always)]
pub(crate) unsafe fn vtable_ic_insert(
    class_id: u32,
    method_name_ptr: usize,
    func_ptr: usize,
    param_count: u32,
    has_synthetic_arguments: bool,
    has_rest: bool,
) {
    if method_name_ptr == 0 {
        return;
    }
    let cur_gen = VTABLE_GEN.load(Ordering::Relaxed);
    let slot = vtable_ic_slot(class_id, method_name_ptr);
    VTABLE_IC.with(|cell| {
        let cache = &mut *cell.get();
        cache[slot] = VTableICEntry {
            gen: cur_gen,
            class_id,
            _pad: 0,
            method_name_ptr,
            func_ptr,
            param_count,
            has_synthetic_arguments: if has_synthetic_arguments { 1 } else { 0 },
            has_rest: if has_rest { 1 } else { 0 },
        };
    });
}

/// Call a vtable method with the correct arity.
/// All method params are f64, `this` is i64.
pub(crate) unsafe fn call_vtable_method(
    func_ptr: usize,
    this: i64,
    args_ptr: *const f64,
    args_len: usize,
    param_count: u32,
    has_synthetic_arguments: bool,
    has_rest: bool,
) -> f64 {
    // A missing trailing argument is `undefined` per spec (NOT NaN): default
    // parameters lower to a `param === undefined ? <default> : param` check in
    // the method prologue, so padding a hole with NaN left the default
    // un-applied (`async method(a, b, c = 99)` called via the dynamic vtable
    // path — e.g. a detached `C.prototype.method` value — saw `c = NaN`). Pad
    // with TAG_UNDEFINED so the prologue's default-check fires.
    #[inline(always)]
    unsafe fn arg_or_undefined(args_ptr: *const f64, args_len: usize, idx: usize) -> f64 {
        if idx < args_len {
            *args_ptr.add(idx)
        } else {
            // A missing argument is `undefined` per spec, not a bare IEEE NaN.
            // This vtable path is reached without call-site padding when a
            // method is invoked as a value (`const f = obj.m; f()`, or a bound
            // method from a getter), so NaN here defeated the callee's
            // default-param / destructuring prologue (`if (p === undefined)`).
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
    }

    // LLVM-generated methods have signature `double(double this, double arg0, ...)`.
    // `this` is NaN-boxed as f64, so we must pass it as f64 — not i64 — to match
    // the calling convention. On ARM64 i64 and f64 share registers, so passing i64
    // works by accident; on Windows x64 ABI they use *different* registers (rcx vs
    // xmm0), causing segfaults when the method reads `this` from the wrong register.
    //
    // Issue #519: all call sites pass `this` as a RAW POINTER (the bottom-48-bit
    // address from `jsval.as_pointer()`). Bit-casting raw pointer bits to f64
    // produces a subnormal float (no NaN-box tag), which the method body
    // interprets as a number — every nested method call inside the body sees
    // `(number).<method>` and either returns garbage or throws TypeError via
    // the issue #510 catch-all (e.g. RegExpRouter.match → `this.buildAllMatchers()`
    // → "(number).buildAllMatchers is not a function" inside SmartRouter's
    // dispatch chain). NaN-box with POINTER_TAG before passing so the body
    // sees a real instance pointer.
    let this_f64: f64 = {
        let bits = this as u64;
        const PTR_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
        if bits != 0 && bits <= PTR_MASK {
            // Raw pointer (no NaN-box tag) — wrap with POINTER_TAG so the
            // method body's `this` arrives as a real instance pointer.
            f64::from_bits(JSValue::pointer(bits as *mut u8).bits())
        } else {
            // Already NaN-boxed (top bits set) or null — pass through.
            f64::from_bits(bits)
        }
    };

    // A trailing param that is either the synthesized `arguments` object or a
    // user rest param (`method(a, ...rest)`) needs the call-site args bundled
    // into a JS array for that slot. Without this, an apply/dynamic dispatch
    // (`recv.method(...spread)` via `js_native_call_method_apply`) passes the
    // raw individual args and the callee reads `rest = args[0]` as a scalar —
    // marked's `new Marked()` -> `this.use(...e)` hit exactly this, throwing
    // `(number).forEach is not a function`. The synthesized-`arguments` slot
    // holds ALL passed args; a user rest slot holds only args from the rest
    // position onward (so `method(a, ...rest)` keeps `a` positional).
    let mut adjusted_args_storage: Option<Vec<f64>> = None;
    let (call_args_ptr, call_args_len) = if has_synthetic_arguments || has_rest {
        let visible_params = (param_count as usize).saturating_sub(1);
        let pack_start = if has_synthetic_arguments {
            0
        } else {
            visible_params.min(args_len)
        };
        let packed_len = args_len.saturating_sub(pack_start);
        let raw_args = crate::array::js_array_alloc_with_length(packed_len as u32);
        for (slot, i) in (pack_start..args_len).enumerate() {
            crate::array::js_array_set_f64(
                raw_args,
                slot as u32,
                arg_or_undefined(args_ptr, args_len, i),
            );
        }
        let raw_args_value = crate::value::js_nanbox_pointer(raw_args as i64);
        let mut args = Vec::with_capacity(param_count as usize);
        for i in 0..visible_params {
            args.push(arg_or_undefined(args_ptr, args_len, i));
        }
        args.push(raw_args_value);
        adjusted_args_storage = Some(args);
        let adjusted_args = adjusted_args_storage.as_ref().unwrap();
        (adjusted_args.as_ptr(), adjusted_args.len())
    } else {
        (args_ptr, args_len)
    };

    match param_count {
        0 => {
            let f: extern "C" fn(f64) -> f64 = std::mem::transmute(func_ptr);
            f(this_f64)
        }
        1 => {
            let f: extern "C" fn(f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(this_f64, arg_or_undefined(call_args_ptr, call_args_len, 0))
        }
        2 => {
            let f: extern "C" fn(f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_undefined(call_args_ptr, call_args_len, 0),
                arg_or_undefined(call_args_ptr, call_args_len, 1),
            )
        }
        3 => {
            let f: extern "C" fn(f64, f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_undefined(call_args_ptr, call_args_len, 0),
                arg_or_undefined(call_args_ptr, call_args_len, 1),
                arg_or_undefined(call_args_ptr, call_args_len, 2),
            )
        }
        4 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_undefined(call_args_ptr, call_args_len, 0),
                arg_or_undefined(call_args_ptr, call_args_len, 1),
                arg_or_undefined(call_args_ptr, call_args_len, 2),
                arg_or_undefined(call_args_ptr, call_args_len, 3),
            )
        }
        5 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_undefined(call_args_ptr, call_args_len, 0),
                arg_or_undefined(call_args_ptr, call_args_len, 1),
                arg_or_undefined(call_args_ptr, call_args_len, 2),
                arg_or_undefined(call_args_ptr, call_args_len, 3),
                arg_or_undefined(call_args_ptr, call_args_len, 4),
            )
        }
        6 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_undefined(call_args_ptr, call_args_len, 0),
                arg_or_undefined(call_args_ptr, call_args_len, 1),
                arg_or_undefined(call_args_ptr, call_args_len, 2),
                arg_or_undefined(call_args_ptr, call_args_len, 3),
                arg_or_undefined(call_args_ptr, call_args_len, 4),
                arg_or_undefined(call_args_ptr, call_args_len, 5),
            )
        }
        7 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_undefined(call_args_ptr, call_args_len, 0),
                arg_or_undefined(call_args_ptr, call_args_len, 1),
                arg_or_undefined(call_args_ptr, call_args_len, 2),
                arg_or_undefined(call_args_ptr, call_args_len, 3),
                arg_or_undefined(call_args_ptr, call_args_len, 4),
                arg_or_undefined(call_args_ptr, call_args_len, 5),
                arg_or_undefined(call_args_ptr, call_args_len, 6),
            )
        }
        8 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_undefined(call_args_ptr, call_args_len, 0),
                arg_or_undefined(call_args_ptr, call_args_len, 1),
                arg_or_undefined(call_args_ptr, call_args_len, 2),
                arg_or_undefined(call_args_ptr, call_args_len, 3),
                arg_or_undefined(call_args_ptr, call_args_len, 4),
                arg_or_undefined(call_args_ptr, call_args_len, 5),
                arg_or_undefined(call_args_ptr, call_args_len, 6),
                arg_or_undefined(call_args_ptr, call_args_len, 7),
            )
        }
        9 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_undefined(call_args_ptr, call_args_len, 0),
                arg_or_undefined(call_args_ptr, call_args_len, 1),
                arg_or_undefined(call_args_ptr, call_args_len, 2),
                arg_or_undefined(call_args_ptr, call_args_len, 3),
                arg_or_undefined(call_args_ptr, call_args_len, 4),
                arg_or_undefined(call_args_ptr, call_args_len, 5),
                arg_or_undefined(call_args_ptr, call_args_len, 6),
                arg_or_undefined(call_args_ptr, call_args_len, 7),
                arg_or_undefined(call_args_ptr, call_args_len, 8),
            )
        }
        _ => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_undefined(call_args_ptr, call_args_len, 0),
                arg_or_undefined(call_args_ptr, call_args_len, 1),
                arg_or_undefined(call_args_ptr, call_args_len, 2),
                arg_or_undefined(call_args_ptr, call_args_len, 3),
                arg_or_undefined(call_args_ptr, call_args_len, 4),
                arg_or_undefined(call_args_ptr, call_args_len, 5),
                arg_or_undefined(call_args_ptr, call_args_len, 6),
                arg_or_undefined(call_args_ptr, call_args_len, 7),
                arg_or_undefined(call_args_ptr, call_args_len, 8),
                arg_or_undefined(call_args_ptr, call_args_len, 9),
            )
        }
    }
}

/// Walk the class parent chain looking for a recorded fetch-builtin parent
/// (Request = 1, Response = 2). Returns the kind for the first ancestor (incl.
/// `class_id` itself) that directly extends a global Request/Response.
pub(crate) fn fetch_parent_kind_in_chain(class_id: u32) -> Option<u8> {
    let mut cid = class_id;
    let mut depth = 0u32;
    while depth < 32 {
        if let Some(kind) = super::fetch_parent_kind(cid) {
            return Some(kind);
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    None
}

/// Register a class with its parent class ID in the global registry
pub(crate) fn register_class(class_id: u32, parent_class_id: u32) {
    let mut registry = CLASS_REGISTRY.write().unwrap();
    if registry.is_none() {
        *registry = Some(HashMap::new());
    }
    registry.as_mut().unwrap().insert(class_id, parent_class_id);
}

/// Public registration entry point used by codegen module init.
///
/// The inline bump allocator (codegen-side `new ClassName()` lowering)
/// writes `parent_class_id` directly into the ObjectHeader and skips
/// the per-alloc `register_class` call that the runtime allocators
/// (`js_object_alloc_with_parent`, `js_object_alloc_class_inline_keys`,
/// etc.) make on every allocation. That breaks multi-level
/// `instanceof` chains: `class Square extends Rectangle extends Shape`
/// — `square instanceof Shape` walks the registry chain
/// `Square → Rectangle → Shape`, but if we never registered the
/// `Square → Rectangle` edge the walk stops immediately and returns
/// false.
///
/// Codegen now emits one call to this function per inheriting class
/// in the entry-block init prelude (after `__perry_init_strings_*`),
/// so the registry chain is fully populated before any user code runs.
#[no_mangle]
pub extern "C" fn js_register_class_parent(class_id: u32, parent_class_id: u32) {
    if parent_class_id != 0 {
        register_class(class_id, parent_class_id);
    }
}

/// Issue #711: dynamic parent-class registration for
/// `class X extends fn(...)` shapes where the parent class_id is only
/// known at runtime. Called from codegen-emitted module-init code at
/// the source-order position of the class declaration (so the
/// extends expression's free variables — imports, top-level `let`s,
/// factory functions — are already initialized by the time we
/// evaluate the parent).
///
/// `parent_value` is the evaluated extends expression as a Perry
/// NaN-boxed value. We resolve a parent class_id from it via:
///   1. INT32-tagged ClassRef (the value `String$` produces) — the
///      payload IS the class_id, verified against REGISTERED_CLASS_IDS.
///   2. POINTER-tagged Object instance (the value a `make<T>(...)`
///      factory might return when it constructs and returns an
///      object) — read `class_id` from the ObjectHeader.
/// Anything else (closures, primitives, null/undefined) is a no-op:
/// the class stays parentless, identical to the pre-#711 behavior.
/// Self-registration (`parent_cid == class_id`) is rejected so a
/// recursive helper that returns its receiver can't create a cycle.
#[no_mangle]
pub extern "C" fn js_register_class_parent_dynamic(class_id: u32, parent_value: f64) {
    // Stash the parent VALUE keyed by child class id so `super()` can read it
    // back (`js_get_dynamic_parent_value`) instead of re-evaluating the extends
    // expression inside the constructor scope. The decl-time call here runs in
    // the module-init scope where the extends expression's free variables
    // (require aliases such as `_suffix` in `class X extends _suffix.default`)
    // are bound. Skip undefined (the bare placeholder) — a genuinely undefined
    // superclass throws below anyway.
    {
        const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
        let bits = parent_value.to_bits();
        if bits != TAG_UNDEFINED && class_id != 0 {
            let mut guard = CLASS_DYNAMIC_PARENT_VALUE.write().unwrap();
            if guard.is_none() {
                *guard = Some(HashMap::new());
            }
            guard.as_mut().unwrap().insert(class_id, bits);
        }
    }
    // A globalThis builtin constructor closure is a valid superclass
    // (`class CloseEvent extends Event` — the `ws` package's WebSocket
    // events). Resolve it through the same name table the dynamic
    // `instanceof` path uses and register the edge when the builtin has a
    // runtime class id, so subclass instances satisfy `instanceof Event`
    // and Event-shaped dispatch gates. Builtins without a class id keep the
    // parentless baseline (no throw — they ARE constructors).
    if let Some(name) = identify_global_builtin_constructor(parent_value) {
        let parent_cid = super::instanceof::global_builtin_constructor_class_id(name);
        if parent_cid != 0 && parent_cid != class_id {
            register_class(class_id, parent_cid);
        }
        // A dynamic subclass that resolves its parent through this builtin
        // branch must still record the fetch-parent kind so `new X()` attaches
        // the native Request/Response handle — the bookkeeping below this
        // early return would otherwise be skipped.
        match name {
            "Request" => super::register_fetch_parent_kind(class_id, 1),
            "Response" => super::register_fetch_parent_kind(class_id, 2),
            _ => {}
        }
        return;
    }
    // A bound native-module export (`const { Writable } = require('stream');
    // class Receiver extends Writable` — the `ws` package's shape) is a real
    // Node constructor even though Perry models it as a BOUND_METHOD closure.
    // Keep the parentless baseline rather than mis-throwing; native-parent
    // method inheritance is handled by codegen's extends_name machinery, not
    // by this registry edge.
    if is_bound_native_method_closure_value(parent_value) {
        return;
    }
    // Spec: a non-`null` superclass that is not a constructor throws a TypeError
    // at class-definition time (before any `.prototype` access). (Test262
    // subclass/superclass-* and definition/invalid-extends.)
    if extends_target_must_throw(parent_value) {
        super::object_ops::throw_object_type_error(b"Class extends value is not a constructor");
    }

    let bits = parent_value.to_bits();
    let tag = bits & 0xFFFF_0000_0000_0000;
    const INT32_TAG: u64 = 0x7FFE_0000_0000_0000;
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;

    let parent_cid: u32 = if tag == INT32_TAG {
        // ClassRef: lower 32 bits are the class id. Verify it's
        // actually a registered class id before trusting it.
        let payload = bits as u32;
        if payload == 0 {
            0
        } else {
            let guard = REGISTERED_CLASS_IDS.read().unwrap();
            match guard.as_ref() {
                Some(set) if set.contains(&payload) => payload,
                _ => 0,
            }
        }
    } else if tag == POINTER_TAG {
        // Object instance: read class_id from the ObjectHeader.
        let ptr = crate::value::js_nanbox_get_pointer(parent_value) as *const ObjectHeader;
        let from_obj = js_object_get_class_id(ptr);
        if from_obj != 0 {
            from_obj
        } else {
            // Issue #711 part 2: the value might be a closure whose
            // `.prototype` was assigned to an object via the
            // `function Base() {}; Base.prototype = X` pattern. Look
            // up the synthetic class id assigned at
            // `js_set_function_prototype` time. Returns 0 if the
            // closure has no registered prototype object — falls
            // through to the parentless baseline.
            function_class_id(parent_value)
        }
    } else {
        0
    };

    if parent_cid != 0 && parent_cid != class_id {
        register_class(class_id, parent_cid);
    }

    // Record whether the parent value is the global Request/Response
    // constructor (possibly via an alias like `GlobalRequest = global.Request`),
    // resolved here in the scope where the alias is live. The runtime
    // dynamic-construction path (`new (classExprValue)(...)`) consults this to
    // attach the underlying native fetch handle on the instance — the static
    // codegen `super()` path can't, because the textual parent name is the
    // alias, not "Request". Refs `@hono/node-server`'s `class Request extends
    // GlobalRequest`.
    match identify_global_builtin_constructor(parent_value) {
        Some("Request") => super::register_fetch_parent_kind(class_id, 1),
        Some("Response") => super::register_fetch_parent_kind(class_id, 2),
        _ => {}
    }

    // #1788: when the parent is a per-evaluation class OBJECT (a class
    // expression value, POINTER-tagged), record it as `class_id`'s static
    // prototype so static-field lookups on the subclass walk to the parent
    // object's OWN per-evaluation static fields — effect's
    // `class Number$ extends make(numberKeyword) {}` → `Number$.ast`. Reuses
    // the CLASS_PROTOTYPE_OBJECTS map (the same #711/#809 vehicle), resolved
    // via `resolve_proto_chain_field`; the class_id parent edge above keeps
    // method/`new`/instanceof dispatch on the existing fast path.
    if tag == POINTER_TAG {
        let ptr = crate::value::js_nanbox_get_pointer(parent_value) as *mut ObjectHeader;
        if !ptr.is_null() && js_object_get_class_id(ptr as *const ObjectHeader) != 0 {
            class_prototype_object_root_store(class_id, ptr);
        } else if !ptr.is_null() && crate::closure::is_closure_ptr(ptr as usize) {
            // #36 / #321: the parent is a plain FUNCTION value (closure), e.g.
            // effect's `class Svc extends Context.Tag("Svc")<...>() {}`. Record
            // the closure-parent edge so static-field reads on the subclass
            // (`Svc.key`, `Svc._op`, `Svc[TagTypeId]`) walk to the parent
            // function's own props + ITS static prototype. The parent class_id
            // edge isn't wired (a closure carries no class_id), so this is the
            // only inheritance link for a function-valued superclass.
            class_parent_closure_root_store(class_id, ptr as usize);
        }
    }
}

/// Read back the parent constructor value stashed at class-definition time by
/// `js_register_class_parent_dynamic` (see `CLASS_DYNAMIC_PARENT_VALUE`).
/// `super()` in a `class X extends <runtime-value>` body uses this so the
/// parent is resolved from the value captured in the module-init scope, not
/// re-evaluated in the constructor scope (where an IIFE-local require alias
/// like `_suffix` in `extends _suffix.default` is not in scope). Returns
/// `undefined` when nothing was stashed for this class id — the caller then
/// falls back to re-evaluating its extends expression.
#[no_mangle]
pub extern "C" fn js_get_dynamic_parent_value(class_id: u32) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    if class_id == 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let guard = CLASS_DYNAMIC_PARENT_VALUE.read().unwrap();
    match guard.as_ref().and_then(|m| m.get(&class_id)) {
        Some(&bits) => f64::from_bits(bits),
        None => f64::from_bits(TAG_UNDEFINED),
    }
}

/// #1789: stamp a freshly-allocated object as a heap "class object" (the
/// value a class EXPRESSION evaluates to). Sets `object_type =
/// OBJECT_TYPE_CLASS` so `typeof` reports "function" and `new`/`instanceof`
/// read `class_id` from it. Called by codegen right after `js_object_alloc`
/// in the `ClassExprFresh` lowering.
#[no_mangle]
pub extern "C" fn js_object_mark_class(obj: i64) {
    if obj != 0 {
        unsafe {
            (*(obj as *mut ObjectHeader)).object_type = crate::error::OBJECT_TYPE_CLASS;
        }
    }
}

/// #1789: is `ptr` a heap "class object" (`object_type == OBJECT_TYPE_CLASS`)?
/// Validates the GcHeader is a `GC_TYPE_OBJECT` before reading `object_type`,
/// so raw Map/Set/Buffer pointers (no GcHeader) are never misread. Used by
/// `typeof`, `new`, and `instanceof` to recognize a class value.
pub fn is_class_object_ptr(ptr: *const u8) -> bool {
    // Reject anything in the native-module handle band (see
    // `value::addr_class`). Those are registry ids (net.Socket, zlib stream,
    // crypto, fastify, ioredis, timers, …) bit-OR'd with POINTER_TAG, not real
    // heap pointers — real objects always live above the band. The previous
    // 0x1008 floor only caught the tiny net/fastify id space; a mid-range
    // handle (e.g. zlib's stream base, #1843) sailed past it and this function
    // then segfaulted dereferencing `[handle - 8]` as a GcHeader.
    if crate::value::addr_class::is_handle_band(ptr as usize) {
        return false;
    }
    unsafe {
        let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        (*gc_header).obj_type == crate::gc::GC_TYPE_OBJECT
            && (*(ptr as *const ObjectHeader)).object_type == crate::error::OBJECT_TYPE_CLASS
    }
}

/// #1789: f64-value form of [`is_class_object_ptr`] — true only for a
/// POINTER-tagged value that is a class object.
pub fn is_class_object_value(value: f64) -> bool {
    let jsval = crate::value::JSValue::from_bits(value.to_bits());
    jsval.is_pointer() && is_class_object_ptr(jsval.as_pointer::<u8>())
}

/// #1788: register a class STATIC method (`perry_static_*`, no `this` param)
/// in `CLASS_STATIC_METHODS`, keyed by the (template) class_id. Emitted by
/// codegen at module init alongside the instance-method vtable registration.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_static_method(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    func_ptr: i64,
    param_count: i64,
    has_rest: i64,
) {
    if class_id == 0 || name_ptr.is_null() || name_len <= 0 {
        return;
    }
    let name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len as usize)) {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };
    let mut guard = CLASS_STATIC_METHODS.write().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard
        .as_mut()
        .unwrap()
        .entry(class_id as u32)
        .or_default()
        .insert(name, (func_ptr as usize, param_count as u32, has_rest != 0));
}

fn property_key_string(key: f64) -> Option<String> {
    let property_key = unsafe { crate::object::js_to_property_key(key) };
    if unsafe { crate::symbol::js_is_symbol(property_key) } != 0 {
        return None;
    }
    let str_ptr = crate::value::js_jsvalue_to_string(property_key);
    if str_ptr.is_null() {
        return Some(String::new());
    }
    unsafe {
        let len = (*str_ptr).byte_len as usize;
        let data = (str_ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        Some(std::str::from_utf8(bytes).unwrap_or("").to_string())
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_register_class_computed_method(
    class_id: i64,
    key: f64,
    func_ptr: i64,
    param_count: i64,
    is_static: i64,
    has_rest: i64,
) {
    if class_id == 0 || func_ptr == 0 {
        return;
    }
    let property_key = crate::object::js_to_property_key(key);
    let class_id = class_id as u32;
    if crate::symbol::js_is_symbol(property_key) != 0 {
        let sym_key = crate::symbol::sym_key_from_f64(property_key);
        if sym_key == 0 {
            return;
        }
        let mut guard = CLASS_SYMBOL_METHODS.write().unwrap();
        if guard.is_none() {
            *guard = Some(HashMap::new());
        }
        guard.as_mut().unwrap().insert(
            (class_id, sym_key, is_static != 0),
            (func_ptr as usize, param_count as u32, has_rest != 0),
        );
        VTABLE_GEN.fetch_add(1, Ordering::Release);
        return;
    }
    let name = match property_key_string(property_key) {
        Some(name) => name,
        None => return,
    };
    if is_static != 0 && name == "prototype" {
        throw_object_type_error(b"Classes may not have a static property named 'prototype'");
    }
    if is_static != 0 {
        let mut guard = CLASS_STATIC_METHODS.write().unwrap();
        if guard.is_none() {
            *guard = Some(HashMap::new());
        }
        guard
            .as_mut()
            .unwrap()
            .entry(class_id)
            .or_default()
            .insert(name, (func_ptr as usize, param_count as u32, has_rest != 0));
    } else {
        let mut registry = CLASS_VTABLE_REGISTRY.write().unwrap();
        if registry.is_none() {
            *registry = Some(HashMap::new());
        }
        let vtable = registry
            .as_mut()
            .unwrap()
            .entry(class_id)
            .or_insert_with(|| ClassVTable {
                methods: HashMap::new(),
                getters: HashMap::new(),
                setters: HashMap::new(),
            });
        vtable.methods.insert(
            name,
            VTableMethodEntry {
                func_ptr: func_ptr as usize,
                param_count: param_count as u32,
                // Computed class methods don't carry synthetic-`arguments`
                // metadata through this registration path (only `has_rest`),
                // so they never receive a synthesized arguments object.
                has_synthetic_arguments: false,
                has_rest: has_rest != 0,
            },
        );
    }
    VTABLE_GEN.fetch_add(1, Ordering::Release);
}

#[no_mangle]
pub unsafe extern "C" fn js_register_class_computed_accessor(
    class_id: i64,
    key: f64,
    getter_ptr: i64,
    setter_ptr: i64,
    is_static: i64,
) {
    if class_id == 0 || (getter_ptr == 0 && setter_ptr == 0) {
        return;
    }
    let property_key = crate::object::js_to_property_key(key);
    let class_id = class_id as u32;
    if crate::symbol::js_is_symbol(property_key) != 0 {
        let sym_key = crate::symbol::sym_key_from_f64(property_key);
        if sym_key == 0 {
            return;
        }
        let mut guard = CLASS_SYMBOL_ACCESSORS.write().unwrap();
        if guard.is_none() {
            *guard = Some(HashMap::new());
        }
        let entry = guard
            .as_mut()
            .unwrap()
            .entry((class_id, sym_key, is_static != 0))
            .or_insert((0, 0));
        if getter_ptr != 0 {
            entry.0 = getter_ptr as usize;
        }
        if setter_ptr != 0 {
            entry.1 = setter_ptr as usize;
        }
        VTABLE_GEN.fetch_add(1, Ordering::Release);
        return;
    }
    if let Some(name) = property_key_string(property_key) {
        if is_static != 0 && name == "prototype" {
            throw_object_type_error(b"Classes may not have a static property named 'prototype'");
        }
        if is_static == 0 {
            let mut registry = CLASS_VTABLE_REGISTRY.write().unwrap();
            if registry.is_none() {
                *registry = Some(HashMap::new());
            }
            let vtable = registry
                .as_mut()
                .unwrap()
                .entry(class_id)
                .or_insert_with(|| ClassVTable {
                    methods: HashMap::new(),
                    getters: HashMap::new(),
                    setters: HashMap::new(),
                });
            if getter_ptr != 0 {
                vtable.getters.insert(name.clone(), getter_ptr as usize);
            }
            if setter_ptr != 0 {
                vtable.setters.insert(name, setter_ptr as usize);
            }
        } else {
            let mut guard = CLASS_STATIC_ACCESSORS.write().unwrap();
            if guard.is_none() {
                *guard = Some(HashMap::new());
            }
            let entry = guard
                .as_mut()
                .unwrap()
                .entry(class_id)
                .or_default()
                .entry(name)
                .or_insert((0, 0));
            if getter_ptr != 0 {
                entry.0 = getter_ptr as usize;
            }
            if setter_ptr != 0 {
                entry.1 = setter_ptr as usize;
            }
        }
    }
    VTABLE_GEN.fetch_add(1, Ordering::Release);
}

/// Look up a static method by name in `CLASS_STATIC_METHODS`, walking the
/// class_id parent chain (so a subclass inherits a parent's static method).
/// Own-only static method lookup (no parent-chain walk) — for
/// `getOwnPropertyDescriptor(C, name)`, where inherited statics must NOT be
/// reported as own properties of `C`.
pub(crate) fn class_has_own_static_method(class_id: u32, name: &str) -> bool {
    CLASS_STATIC_METHODS
        .read()
        .ok()
        .and_then(|g| {
            g.as_ref()
                .and_then(|m| m.get(&class_id).map(|inner| inner.contains_key(name)))
        })
        .unwrap_or(false)
}

pub(crate) fn lookup_static_method_in_chain(
    class_id: u32,
    name: &str,
) -> Option<(usize, u32, bool)> {
    let guard = CLASS_STATIC_METHODS.read().ok()?;
    let map = guard.as_ref()?;
    let mut cid = class_id;
    let mut depth = 0usize;
    while cid != 0 && depth < 32 {
        if let Some(m) = map.get(&cid) {
            if let Some(&entry) = m.get(name) {
                return Some(entry);
            }
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    None
}

pub(crate) fn lookup_class_symbol_method_in_chain(
    class_id: u32,
    sym_key: usize,
    is_static: bool,
) -> Option<(usize, u32, bool)> {
    let guard = CLASS_SYMBOL_METHODS.read().ok()?;
    let map = guard.as_ref()?;
    let mut cid = class_id;
    let mut depth = 0usize;
    while cid != 0 && depth < 32 {
        if let Some(&entry) = map.get(&(cid, sym_key, is_static)) {
            return Some(entry);
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    None
}

pub(crate) fn class_own_symbol_member_keys(class_id: u32, is_static: bool) -> Vec<usize> {
    let mut keys = Vec::new();
    if let Ok(methods) = CLASS_SYMBOL_METHODS.read() {
        if let Some(map) = methods.as_ref() {
            for &(cid, sym_key, static_flag) in map.keys() {
                if cid == class_id && static_flag == is_static && !keys.contains(&sym_key) {
                    keys.push(sym_key);
                }
            }
        }
    }
    if let Ok(accessors) = CLASS_SYMBOL_ACCESSORS.read() {
        if let Some(map) = accessors.as_ref() {
            for &(cid, sym_key, static_flag) in map.keys() {
                if cid == class_id && static_flag == is_static && !keys.contains(&sym_key) {
                    keys.push(sym_key);
                }
            }
        }
    }
    keys.sort_by_key(|sym_key| unsafe {
        let ptr = *sym_key as *const crate::symbol::SymbolHeader;
        if ptr.is_null() {
            u64::MAX
        } else {
            (*ptr).id
        }
    });
    keys
}

pub(crate) unsafe fn class_symbol_getter_value(
    class_id: u32,
    sym_key: usize,
    receiver: f64,
    is_static: bool,
) -> Option<f64> {
    let guard = CLASS_SYMBOL_ACCESSORS.read().ok()?;
    let map = guard.as_ref()?;
    let mut cid = class_id;
    let mut depth = 0usize;
    while cid != 0 && depth < 32 {
        if let Some(&(getter, _)) = map.get(&(cid, sym_key, is_static)) {
            if getter == 0 {
                return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
            }
            let result = if is_static {
                let prev_this = crate::object::js_implicit_this_set(receiver);
                let f: extern "C" fn() -> f64 = std::mem::transmute(getter);
                let result = f();
                crate::object::js_implicit_this_set(prev_this);
                result
            } else {
                let f: extern "C" fn(f64) -> f64 = std::mem::transmute(getter);
                f(receiver)
            };
            return Some(result);
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    None
}

pub(crate) unsafe fn class_symbol_setter_apply(
    class_id: u32,
    sym_key: usize,
    receiver: f64,
    value: f64,
    is_static: bool,
) -> bool {
    let guard = match CLASS_SYMBOL_ACCESSORS.read() {
        Ok(g) => g,
        Err(_) => return false,
    };
    let Some(map) = guard.as_ref() else {
        return false;
    };
    let mut cid = class_id;
    let mut depth = 0usize;
    while cid != 0 && depth < 32 {
        if let Some(&(_, setter)) = map.get(&(cid, sym_key, is_static)) {
            if setter != 0 {
                if is_static {
                    let prev_this = crate::object::js_implicit_this_set(receiver);
                    let f: extern "C" fn(f64) -> f64 = std::mem::transmute(setter);
                    let _ = f(value);
                    crate::object::js_implicit_this_set(prev_this);
                } else {
                    let f: extern "C" fn(f64, f64) -> f64 = std::mem::transmute(setter);
                    let _ = f(receiver, value);
                }
            }
            return true;
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    false
}

pub(crate) unsafe fn class_static_accessor_getter_value(
    class_id: u32,
    name: &str,
    receiver: f64,
) -> Option<f64> {
    let guard = CLASS_STATIC_ACCESSORS.read().ok()?;
    let map = guard.as_ref()?;
    let mut cid = class_id;
    let mut depth = 0usize;
    while cid != 0 && depth < 32 {
        if let Some(accessors) = map.get(&cid) {
            if let Some(&(getter, _)) = accessors.get(name) {
                if getter == 0 {
                    return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
                }
                let prev_this = crate::object::js_implicit_this_set(receiver);
                let f: extern "C" fn() -> f64 = std::mem::transmute(getter);
                let result = f();
                crate::object::js_implicit_this_set(prev_this);
                return Some(result);
            }
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    None
}

pub(crate) unsafe fn class_static_accessor_setter_apply(
    class_id: u32,
    name: &str,
    receiver: f64,
    value: f64,
) -> bool {
    let guard = match CLASS_STATIC_ACCESSORS.read() {
        Ok(g) => g,
        Err(_) => return false,
    };
    let Some(map) = guard.as_ref() else {
        return false;
    };
    let mut cid = class_id;
    let mut depth = 0usize;
    while cid != 0 && depth < 32 {
        if let Some(accessors) = map.get(&cid) {
            if let Some(&(_, setter)) = accessors.get(name) {
                if setter != 0 {
                    let prev_this = crate::object::js_implicit_this_set(receiver);
                    let f: extern "C" fn(f64) -> f64 = std::mem::transmute(setter);
                    let _ = f(value);
                    crate::object::js_implicit_this_set(prev_this);
                }
                return true;
            }
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    false
}

/// Apply an instance `set name(v)` accessor from the class vtable chain,
/// invoking it with the `(this, value)` calling convention class setters use.
/// Returns `true` if a setter was found and called. Used when a write targets
/// a class prototype ref (`C.prototype[key] = v`) whose `key` is an accessor
/// defined on the prototype itself (Test262 accessor-name-inst setters).
/// Whether the class (or an ancestor) has an instance `get name()` accessor.
pub(crate) fn class_has_instance_getter(class_id: u32, name: &str) -> bool {
    let Ok(guard) = CLASS_VTABLE_REGISTRY.read() else {
        return false;
    };
    let Some(reg) = guard.as_ref() else {
        return false;
    };
    let mut cid = class_id;
    let mut depth = 0usize;
    while cid != 0 && depth < 32 {
        if let Some(vt) = reg.get(&cid) {
            if vt.getters.contains_key(name) {
                return true;
            }
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    false
}

pub(crate) unsafe fn class_instance_setter_apply(
    class_id: u32,
    name: &str,
    receiver: f64,
    value: f64,
) -> bool {
    let guard = match CLASS_VTABLE_REGISTRY.read() {
        Ok(g) => g,
        Err(_) => return false,
    };
    let Some(reg) = guard.as_ref() else {
        return false;
    };
    let mut cid = class_id;
    let mut depth = 0usize;
    while cid != 0 && depth < 32 {
        if let Some(vtable) = reg.get(&cid) {
            if let Some(&setter_ptr) = vtable.setters.get(name) {
                if setter_ptr != 0 {
                    let f: extern "C" fn(f64, f64) -> f64 = std::mem::transmute(setter_ptr);
                    let _ = f(receiver, value);
                }
                return true;
            }
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    false
}

/// Spec `Function.prototype.length` for a class method named `name` — the
/// count of formal parameters, excluding a trailing rest param and the
/// synthesized `arguments` slot (neither contributes to `.length`). Walks the
/// instance vtable chain, then the static-method table. Used to stamp the
/// bound-method closure's length so `C.prototype.m.length` is correct
/// (Test262 .../class/{gen,async}-method/...-trailing-comma + length tests).
/// Note: does not subtract for default-valued params (the registry doesn't
/// record the first-default position); methods with defaults already reported
/// the wrong length, so this is a strict improvement, never a regression.
pub(crate) fn class_method_bind_length(class_id: u32, name: &str) -> Option<u32> {
    // Exact spec length (default-aware) when codegen recorded it; walk the
    // parent chain so an inherited method's `.length` resolves too.
    if let Ok(guard) = CLASS_METHOD_BIND_LENGTHS.read() {
        if let Some(map) = guard.as_ref() {
            let mut cid = class_id;
            let mut depth = 0usize;
            while cid != 0 && depth < 32 {
                if let Some(&len) = map.get(&(cid, name.to_string())) {
                    return Some(len);
                }
                match get_parent_class_id(cid) {
                    Some(p) if p != 0 && p != cid => {
                        cid = p;
                        depth += 1;
                    }
                    _ => break,
                }
            }
        }
    }
    if let Ok(guard) = CLASS_VTABLE_REGISTRY.read() {
        if let Some(reg) = guard.as_ref() {
            let mut cid = class_id;
            let mut depth = 0usize;
            while cid != 0 && depth < 32 {
                if let Some(vt) = reg.get(&cid) {
                    if let Some(e) = vt.methods.get(name) {
                        let mut len = e.param_count;
                        if e.has_rest {
                            len = len.saturating_sub(1);
                        }
                        if e.has_synthetic_arguments {
                            len = len.saturating_sub(1);
                        }
                        return Some(len);
                    }
                }
                match get_parent_class_id(cid) {
                    Some(p) if p != 0 && p != cid => {
                        cid = p;
                        depth += 1;
                    }
                    _ => break,
                }
            }
        }
    }
    // Static methods: prefer the default-aware spec length recorded by codegen
    // (params before the first default/rest), walking the parent chain; fall
    // back to the raw `CLASS_STATIC_METHODS` param_count otherwise.
    if let Ok(guard) = CLASS_STATIC_METHOD_BIND_LENGTHS.read() {
        if let Some(map) = guard.as_ref() {
            let mut cid = class_id;
            let mut depth = 0usize;
            while cid != 0 && depth < 32 {
                if let Some(&len) = map.get(&(cid, name.to_string())) {
                    return Some(len);
                }
                match get_parent_class_id(cid) {
                    Some(p) if p != 0 && p != cid => {
                        cid = p;
                        depth += 1;
                    }
                    _ => break,
                }
            }
        }
    }
    // CLASS_STATIC_METHODS stores (func_ptr, param_count, has_rest).
    if let Some((_, param_count, has_rest)) = lookup_static_method_in_chain(class_id, name) {
        let mut len = param_count;
        if has_rest {
            len = len.saturating_sub(1);
        }
        return Some(len);
    }
    None
}

/// Call a static method func_ptr with `args` (no `this` prepend — static
/// methods read `this` from the implicit-this slot, set by the caller).
/// Mirrors the arity dispatch of `call_vtable_method` minus the receiver arg.
pub(crate) unsafe fn call_static_method(
    func_ptr: usize,
    args_ptr: *const f64,
    args_len: usize,
    param_count: u32,
) -> f64 {
    // Missing trailing args pad with `undefined` (NOT NaN) so default
    // parameters fire — see `call_vtable_method::arg_or_undefined`.
    #[inline(always)]
    unsafe fn a(args_ptr: *const f64, args_len: usize, idx: usize) -> f64 {
        if idx < args_len {
            *args_ptr.add(idx)
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
    }
    match param_count {
        0 => (std::mem::transmute::<usize, extern "C" fn() -> f64>(func_ptr))(),
        1 => (std::mem::transmute::<usize, extern "C" fn(f64) -> f64>(func_ptr))(a(
            args_ptr, args_len, 0,
        )),
        2 => (std::mem::transmute::<usize, extern "C" fn(f64, f64) -> f64>(func_ptr))(
            a(args_ptr, args_len, 0),
            a(args_ptr, args_len, 1),
        ),
        3 => (std::mem::transmute::<usize, extern "C" fn(f64, f64, f64) -> f64>(func_ptr))(
            a(args_ptr, args_len, 0),
            a(args_ptr, args_len, 1),
            a(args_ptr, args_len, 2),
        ),
        4 => (std::mem::transmute::<usize, extern "C" fn(f64, f64, f64, f64) -> f64>(func_ptr))(
            a(args_ptr, args_len, 0),
            a(args_ptr, args_len, 1),
            a(args_ptr, args_len, 2),
            a(args_ptr, args_len, 3),
        ),
        5 => {
            (std::mem::transmute::<usize, extern "C" fn(f64, f64, f64, f64, f64) -> f64>(func_ptr))(
                a(args_ptr, args_len, 0),
                a(args_ptr, args_len, 1),
                a(args_ptr, args_len, 2),
                a(args_ptr, args_len, 3),
                a(args_ptr, args_len, 4),
            )
        }
        6 => (std::mem::transmute::<usize, extern "C" fn(f64, f64, f64, f64, f64, f64) -> f64>(
            func_ptr,
        ))(
            a(args_ptr, args_len, 0),
            a(args_ptr, args_len, 1),
            a(args_ptr, args_len, 2),
            a(args_ptr, args_len, 3),
            a(args_ptr, args_len, 4),
            a(args_ptr, args_len, 5),
        ),
        7 => {
            (std::mem::transmute::<usize, extern "C" fn(f64, f64, f64, f64, f64, f64, f64) -> f64>(
                func_ptr,
            ))(
                a(args_ptr, args_len, 0),
                a(args_ptr, args_len, 1),
                a(args_ptr, args_len, 2),
                a(args_ptr, args_len, 3),
                a(args_ptr, args_len, 4),
                a(args_ptr, args_len, 5),
                a(args_ptr, args_len, 6),
            )
        }
        _ => (std::mem::transmute::<
            usize,
            extern "C" fn(f64, f64, f64, f64, f64, f64, f64, f64) -> f64,
        >(func_ptr))(
            a(args_ptr, args_len, 0),
            a(args_ptr, args_len, 1),
            a(args_ptr, args_len, 2),
            a(args_ptr, args_len, 3),
            a(args_ptr, args_len, 4),
            a(args_ptr, args_len, 5),
            a(args_ptr, args_len, 6),
            a(args_ptr, args_len, 7),
        ),
    }
}

pub(crate) unsafe fn call_registered_static_method(
    func_ptr: usize,
    args_ptr: *const f64,
    args_len: usize,
    param_count: u32,
    has_rest: bool,
) -> f64 {
    if has_rest {
        let fixed = (param_count as usize).saturating_sub(1);
        let arr = crate::array::js_array_alloc(args_len.saturating_sub(fixed) as u32);
        let mut i = fixed;
        while i < args_len {
            crate::array::js_array_push_f64(arr, *args_ptr.add(i));
            i += 1;
        }
        let rest_box = crate::value::js_nanbox_pointer(arr as i64);
        let mut buf: Vec<f64> = Vec::with_capacity(param_count as usize);
        for j in 0..fixed {
            buf.push(if j < args_len {
                *args_ptr.add(j)
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            });
        }
        buf.push(rest_box);
        call_static_method(func_ptr, buf.as_ptr(), buf.len(), param_count)
    } else {
        call_static_method(func_ptr, args_ptr, args_len, param_count)
    }
}

unsafe fn try_native_static_method_in_proto_chain(
    class_id: u32,
    name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let mut cid = class_id;
    let mut depth = 0u32;
    while cid != 0 && depth < 64 {
        if let Some(parent_addr) = class_parent_closure(cid) {
            let parent_value = crate::value::js_nanbox_pointer(parent_addr as i64);
            if is_buffer_constructor_value(parent_value) {
                let module = b"buffer.Buffer";
                let ns = js_create_native_module_namespace(module.as_ptr(), module.len());
                let ns_obj = JSValue::from_bits(ns.to_bits()).as_pointer::<ObjectHeader>();
                let result = crate::object::native_module::call_native_module_dispatch_hook(
                    ns_obj, name, args_ptr, args_len,
                );
                if !JSValue::from_bits(result.to_bits()).is_undefined() {
                    return Some(result);
                }
            }
        }
        let proto_obj = class_prototype_object(cid);
        if !proto_obj.is_null() && (*proto_obj).class_id == NATIVE_MODULE_CLASS_ID {
            if read_native_module_name(proto_obj as *const ObjectHeader).as_deref()
                == Some("buffer.Buffer")
            {
                let result = crate::object::native_module::call_native_module_dispatch_hook(
                    proto_obj, name, args_ptr, args_len,
                );
                if !JSValue::from_bits(result.to_bits()).is_undefined() {
                    return Some(result);
                }
            }
        }
        cid = get_parent_class_id(cid).unwrap_or(0);
        depth += 1;
    }
    None
}

/// #1788: dispatch a static method on a class value (`Sub.greet()` where
/// `Sub extends make(...)`, or a class-object value) by walking the class_id
/// parent chain in `CLASS_STATIC_METHODS`. Binds `this` to the receiver (so
/// `this.<field>` resolves through the subclass's static-field chain), calls
/// the method, and restores the previous implicit-this. On miss returns the
/// receiver unchanged — preserving the prior "yield the class ref for a
/// chained call during module init" behavior for genuinely-absent methods.
#[no_mangle]
pub unsafe extern "C" fn js_class_static_method_call(
    receiver: f64,
    name_ptr: *const u8,
    name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    if name_ptr.is_null() || name_len == 0 {
        return receiver;
    }
    let name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
        Ok(s) => s,
        Err(_) => return receiver,
    };
    // Resolve the receiver's class_id: INT32 ClassRef payload, or the
    // class_id stamped on a POINTER class object's ObjectHeader.
    let bits = receiver.to_bits();
    let top16 = bits >> 48;
    let class_id = if top16 == 0x7FFE {
        (bits & 0xFFFF_FFFF) as u32
    } else if is_class_object_value(receiver) {
        let obj = crate::value::JSValue::from_bits(bits).as_pointer::<ObjectHeader>();
        js_object_get_class_id(obj)
    } else {
        0
    };
    if class_id == 0 {
        return receiver;
    }
    if let Some((func_ptr, param_count, has_rest)) = lookup_static_method_in_chain(class_id, name) {
        let prev_this = crate::object::js_implicit_this_set(receiver);
        // Receiver-sensitive static `this`: arm the one-shot override so the
        // method prologue (`js_static_this_resolve`) sees the DYNAMIC receiver
        // (e.g. subclass `D` for an inherited `D.f()`). If an outer
        // call/apply already armed an explicit thisArg, that wins.
        crate::object::static_this_arm_if_unarmed(receiver);
        let result = if has_rest {
            // `static foo(a, b, ...rest)` / `static pipe(...args)` (effect's
            // `pipe`/`dual`): pass the first `param_count-1` positional args
            // as-is, then bundle the remaining call args into a JS array for
            // the rest slot — matching JS `arguments`/rest semantics and the
            // direct-call (#1787 / #915) static-dispatch path.
            let fixed = (param_count as usize).saturating_sub(1);
            let arr = crate::array::js_array_alloc(args_len.saturating_sub(fixed) as u32);
            let mut i = fixed;
            while i < args_len {
                crate::array::js_array_push_f64(arr, *args_ptr.add(i));
                i += 1;
            }
            let rest_box = crate::value::js_nanbox_pointer(arr as i64);
            // Build the [param_count]-slot effective-args buffer:
            // positional fixed args, then the bundled rest array.
            let mut buf: Vec<f64> = Vec::with_capacity(param_count as usize);
            for j in 0..fixed {
                buf.push(if j < args_len {
                    *args_ptr.add(j)
                } else {
                    f64::from_bits(crate::value::TAG_UNDEFINED)
                });
            }
            buf.push(rest_box);
            call_static_method(func_ptr, buf.as_ptr(), buf.len(), param_count)
        } else {
            call_static_method(func_ptr, args_ptr, args_len, param_count)
        };
        crate::object::static_this_disarm();
        crate::object::js_implicit_this_set(prev_this);
        return result;
    }
    // #1787 / #321: not a static METHOD — try a static FIELD holding a
    // callable (effect's `static make = (...) => ...` / `static unify = ...`
    // on `SchemaAST.Union`). Walk the class_id chain in CLASS_DYNAMIC_PROPS
    // (where `js_class_register_static_field` records each static field) and,
    // if `name` resolves to a non-nullish value, invoke it as a closure with
    // the call args. Static-field arrows capture lexical `this` (the class) and
    // don't read dynamic `this`, so a plain closure call is correct. Without
    // this, `Class.staticField(args)` fell through to `receiver` (the class
    // ref / INT32 class id), which is why `Union.make([...])` returned `1`/
    // undefined and Schema decode died reading `_tag`.
    {
        let mut cid = class_id;
        let mut depth = 0u32;
        while cid != 0 && depth < 64 {
            let field_val = CLASS_DYNAMIC_PROPS
                .with(|m| m.borrow().get(&cid).and_then(|f| f.get(name).copied()));
            if let Some(v) = field_val {
                let fv = crate::value::JSValue::from_bits(v.to_bits());
                if !fv.is_undefined() && !fv.is_null() {
                    return crate::closure::js_native_call_value(v, args_ptr, args_len);
                }
            }
            cid = get_parent_class_id(cid).unwrap_or(0);
            depth += 1;
        }
    }
    if let Some(result) =
        try_native_static_method_in_proto_chain(class_id, name, args_ptr, args_len)
    {
        return result;
    }
    // True miss: no static method and no callable static field resolved on the
    // class chain. We hand back the receiver (load-bearing for effect's
    // `.pipe()`-during-init chains, #687) — but that silent class-ref is exactly
    // what surfaces downstream as a stray `1`. Surface it at the call site.
    report_dispatch_miss(
        "static-member-call",
        receiver,
        name,
        "the receiver (class ref)",
    );
    receiver
}

/// Look up parent class ID from the registry
pub(crate) fn get_parent_class_id(class_id: u32) -> Option<u32> {
    let registry = CLASS_REGISTRY.read().unwrap();
    registry.as_ref().and_then(|r| r.get(&class_id).copied())
}

/// Look up a method by name in the class vtable, walking the parent chain.
/// Returns `Some((func_ptr, param_count, has_synthetic_arguments, has_rest))`
/// if found, `None` otherwise.
/// Used by `js_assimilate_thenable` (refs #586) and other runtime callers
/// that need to probe a class for a method without invoking it.
pub fn lookup_class_method_in_chain(class_id: u32, name: &str) -> Option<(usize, u32, bool, bool)> {
    let registry = CLASS_VTABLE_REGISTRY.read().unwrap();
    let reg = registry.as_ref()?;
    let mut cur = class_id;
    for _ in 0..32 {
        if let Some(vt) = reg.get(&cur) {
            if let Some(entry) = vt.methods.get(name) {
                return Some((
                    entry.func_ptr,
                    entry.param_count,
                    entry.has_synthetic_arguments,
                    entry.has_rest,
                ));
            }
        }
        match get_parent_class_id(cur) {
            Some(pid) if pid != 0 => cur = pid,
            _ => return None,
        }
    }
    None
}

/// True when `ptr` is the prototype OBJECT of some registered class. Class
/// methods are installed as own fields on the prototype object, so a method-as-
/// value read whose receiver *is* the prototype must return the shared canonical
/// method value (for identity), not the raw stored field — i.e. the own-property
/// shadow rule applies to genuine instances, not to the prototype itself.
pub fn is_registered_class_prototype_object(ptr: usize) -> bool {
    if crate::value::addr_class::is_handle_band(ptr) {
        return false;
    }
    if let Ok(guard) = CLASS_PROTOTYPE_OBJECTS.read() {
        if let Some(map) = guard.as_ref() {
            return map.values().any(|&p| p == ptr);
        }
    }
    false
}

/// Walk the prototype chain of `class_id` and return the id of the class that
/// actually OWNS the method `name` (the prototype where it is defined). Used to
/// make method-as-value identity stable: a class method is a single shared
/// function object, so every read of it — `c.m`, `C.prototype.m`, `c2.m` —
/// must resolve to the canonical value keyed by the OWNING class, not the
/// (possibly derived) class of the receiver. Returns `None` when no class in
/// the chain declares the method.
pub fn method_owner_class_id(class_id: u32, name: &str) -> Option<u32> {
    let registry = CLASS_VTABLE_REGISTRY.read().unwrap();
    let reg = registry.as_ref()?;
    let mut cur = class_id;
    for _ in 0..32 {
        if let Some(vt) = reg.get(&cur) {
            if vt.methods.contains_key(name) {
                return Some(cur);
            }
        }
        match get_parent_class_id(cur) {
            Some(pid) if pid != 0 => cur = pid,
            _ => return None,
        }
    }
    None
}
