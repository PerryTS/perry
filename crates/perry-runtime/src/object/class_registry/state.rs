use super::*;
use std::collections::HashMap;
use std::sync::RwLock;

crate::perry_thread_local! {
    pub(crate) static CLASS_DELETED_KEYS: std::cell::RefCell<std::collections::HashMap<u32, std::collections::HashSet<String>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

pub(crate) fn is_non_constructable_builtin_function_value(value: f64) -> bool {
    super::super::native_module::builtin_closure_is_non_constructable_value(value)
}

/// True when `value` is a bound native-module method/export closure
/// (`BOUND_METHOD_FUNC_PTR` trampoline — what a `require('stream').Writable`
/// property read produces). These represent real Node classes/functions and
/// must be accepted as `extends` targets.
pub(crate) fn is_bound_native_method_closure_value(value: f64) -> bool {
    // Gate on the native-module metadata, not the raw BOUND_METHOD_FUNC_PTR
    // trampoline: reified `Function.prototype.{bind,call,apply}` values
    // (`reify_function_method_value`) share that trampoline but are NOT native
    // constructors, so matching the sentinel alone would let `class X extends
    // obj.method {}` skip the spec-required TypeError and silently stay
    // parentless. A real native-module export carries a non-empty module name.
    unsafe {
        super::super::native_module::bound_native_callable_module_and_method(value)
            .map(|(module, _)| !module.is_empty())
            .unwrap_or(false)
    }
}

pub(crate) fn throw_non_constructable_builtin_function() -> ! {
    super::super::object_ops::throw_object_type_error(b"Function is not a constructor")
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

/// Record `C.<name> = value` in the class-ref side table that dynamic reads
/// (`const K: any = C; K.name`, `Object.keys(C)`, `getOwnPropertyDescriptor`)
/// consult, and shade the stored value for the incremental marker.
///
/// Takes `&str`, not `String`: every caller had a value it did not own, so the
/// old signature forced an allocation on EVERY store — and then threw it away,
/// because `HashMap::insert` keeps the original key when one is already
/// present. That is the shape of a `static` counter: codegen emits this call
/// after each `Expr::StaticFieldSet`, so `Shape.made = Shape.made + 1` inside a
/// constructor runs it once per construction (144,000 times in
/// gc-handoff/apps/shapes.ts) and the key exists after the first.
///
/// The in-place update also skips the `CLASS_DELETED_KEYS` probe — but only
/// when NO class key has ever been deleted, which is the state of essentially
/// every program (`delete C.x` on a class constructor is vanishingly rare).
/// Once anything has been deleted the original sequence runs verbatim, so the
/// interaction between a deleted PROTOTYPE key and a same-named static field
/// (`class C { m() {} static m = 1 }` — both land under one class_id) keeps
/// whatever behaviour it had.
pub(crate) fn class_dynamic_prop_root_store(class_id: u32, name: &str, value: f64) {
    let nothing_deleted = CLASS_DELETED_KEYS.with(|m| m.borrow().is_empty());
    if nothing_deleted {
        let updated = CLASS_DYNAMIC_PROPS.with(|m| {
            match m
                .borrow_mut()
                .get_mut(&class_id)
                .and_then(|props| props.get_mut(name))
            {
                Some(slot) => {
                    *slot = value;
                    true
                }
                None => false,
            }
        });
        if updated {
            crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
            return;
        }
    } else {
        CLASS_DELETED_KEYS.with(|m| {
            if let Some(keys) = m.borrow_mut().get_mut(&class_id) {
                keys.remove(name);
            }
        });
    }
    CLASS_DYNAMIC_PROPS.with(|m| {
        m.borrow_mut()
            .entry(class_id)
            .or_default()
            .insert(name.to_string(), value);
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
                    .filter(|k| !crate::object::is_internal_runtime_key(k))
                    // #7190: a key installed by `Object.defineProperty` without
                    // `enumerable: true` shares this table with static fields
                    // but is NOT enumerable.
                    .filter(|k| !class_static_key_is_non_enumerable(class_id, k))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// #7190: record a `defineProperty`-installed static key's attributes. Called
/// only from the define path; `static x = …` never touches it, so a declared
/// field keeps its CreateDataPropertyOrThrow `(writable, enumerable) = (true,
/// true)` reporting.
pub(crate) fn class_static_set_defined_attrs(
    class_id: u32,
    name: &str,
    writable: bool,
    enumerable: bool,
    configurable: bool,
) {
    crate::object::CLASS_STATIC_DEFINED_ATTRS.with(|m| {
        m.borrow_mut()
            .entry(class_id)
            .or_default()
            .insert(name.to_string(), (writable, enumerable, configurable));
    });
}

/// `(writable, enumerable)` if this static key was installed by
/// `Object.defineProperty`; `None` for a declared `static x = …` field.
pub(crate) fn class_static_defined_attrs(class_id: u32, name: &str) -> Option<(bool, bool, bool)> {
    crate::object::CLASS_STATIC_DEFINED_ATTRS
        .with(|m| m.borrow().get(&class_id).and_then(|k| k.get(name)).copied())
}

pub(crate) fn class_static_key_is_non_enumerable(class_id: u32, name: &str) -> bool {
    class_static_defined_attrs(class_id, name).is_some_and(|(_, enumerable, _)| !enumerable)
}

/// True when `name` is an own static data property (a static field, or a
/// runtime `C.x = …` assignment) recorded in `CLASS_DYNAMIC_PROPS`. Presence
/// only — does not read the value, so it never invokes a static getter. Used by
/// the `in` operator on a class ref (#6149).
pub(crate) fn class_has_own_dynamic_prop(class_id: u32, name: &str) -> bool {
    CLASS_DYNAMIC_PROPS.with(|m| {
        m.borrow()
            .get(&class_id)
            .map(|props| props.contains_key(name))
            .unwrap_or(false)
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

per_test_global! {
    pub static CLASS_STATIC_ACCESSORS: RwLock<Option<HashMap<u32, HashMap<String, (usize, usize)>>>> =
        RwLock::new(None);
}

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

crate::perry_thread_local! {
    pub static CLASS_SYMBOL_METHODS: RwLock<Option<HashMap<(u32, usize, bool), (usize, u32, bool)>>> =
        RwLock::new(None);

    pub static CLASS_SYMBOL_ACCESSORS: RwLock<Option<HashMap<(u32, usize, bool), (usize, usize)>>> =
        RwLock::new(None);
}

/// Set of all registered class ids. Populated at module init by codegen
/// emitting `js_register_class_id(cid)` for every user class — even
/// classes without any methods. Refs #618 / #420 followup.
pub static REGISTERED_CLASS_IDS: RwLock<Option<std::collections::HashSet<u32>>> = RwLock::new(None);

crate::perry_thread_local! {
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
}
// Stored as `usize` (raw address) so the map is Send + Sync. The
// pointer is always converted back to `*mut ObjectHeader` at call sites
// (`class_prototype_object` / the dispatch walk) where single-threaded
// usage is guaranteed.
crate::perry_thread_local! {
    pub static CLASS_PROTOTYPE_OBJECTS: RwLock<Option<HashMap<u32, usize>>> = RwLock::new(None);
}

crate::perry_thread_local! {
    /// Lazily materialized `Class.prototype` objects for declared ES classes.
    /// These are separate from `CLASS_PROTOTYPE_OBJECTS`: that older table is
    /// intentionally overloaded for synthetic prototype sources and static
    /// inheritance shortcuts. Declared class prototypes need stable heap identity
    /// for `typeof C.prototype`, `Object.getPrototypeOf(new C())`, and
    /// `C.prototype.isPrototypeOf(instance)` without perturbing those paths.
    pub static CLASS_DECL_PROTOTYPE_OBJECTS: RwLock<Option<HashMap<u32, usize>>> = RwLock::new(None);
}

crate::perry_thread_local! {
    /// #5024 followup: prototype methods registered via `Object.defineProperty(
    /// Class.prototype, name, desc)` WITHOUT an explicit `enumerable: true` are
    /// non-enumerable (spec default for defineProperty). The plain
    /// `Class.prototype.m = fn` assignment path makes them enumerable. Both funnel
    /// into `CLASS_PROTOTYPE_METHODS`, which stores only the value — so the
    /// enumerability is tracked here, keyed by `(class_id, name)`. Absence means
    /// "enumerable" (the assignment default). Consulted when mirroring a method
    /// onto a prototype OBJECT so reflective `Object.keys`/`for-in` see the
    /// correct attribute.
    pub static CLASS_PROTOTYPE_METHOD_NONENUM: RwLock<
        Option<std::collections::HashSet<(u32, String)>>,
    > = RwLock::new(None);
}

/// Record the enumerability of the prototype method `(class_id, name)`.
/// `enumerable == false` (a `defineProperty` data descriptor without an
/// explicit `enumerable: true`) inserts the key into the non-enumerable set;
/// `enumerable == true` removes it again, so a later redefine that flips the
/// flag back on isn't left shadowed by a stale marker.
pub(crate) fn class_prototype_method_set_enumerable(class_id: u32, name: &str, enumerable: bool) {
    CLASS_PROTOTYPE_METHOD_NONENUM.with(|table| {
        let mut guard = table.write().unwrap();
        if enumerable {
            if let Some(set) = guard.as_mut() {
                set.remove(&(class_id, name.to_string()));
            }
            return;
        }
        if guard.is_none() {
            *guard = Some(std::collections::HashSet::new());
        }
        guard.as_mut().unwrap().insert((class_id, name.to_string()));
    });
}

/// Whether the prototype method `(class_id, name)` should be enumerable when
/// mirrored onto a prototype object. Defaults to `true` (assignment semantics).
pub(crate) fn class_prototype_method_is_enumerable(class_id: u32, name: &str) -> bool {
    CLASS_PROTOTYPE_METHOD_NONENUM.with(|table| {
        if let Ok(read) = table.read() {
            if let Some(set) = read.as_ref() {
                return !set.contains(&(class_id, name.to_string()));
            }
        }
        true
    })
}

crate::perry_thread_local! {
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
}

crate::perry_thread_local! {
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
    /// undefined"). Stored as raw `u64` bits, covering both ClassRef (INT32-tagged)
    /// and object/closure (POINTER-tagged) parents.
    pub static CLASS_DYNAMIC_PARENT_VALUE: RwLock<Option<HashMap<u32, u64>>> = RwLock::new(None);
}

crate::perry_thread_local! {
    /// #6530: maps a template class_id to the raw NaN-boxed POINTER bits of the
    /// per-evaluation CLASS OBJECT the class statement materialized as (marked by
    /// `js_object_mark_class`). A capture-carrying class has no INT32 ClassRef
    /// value at runtime — the class VALUE is this heap object — so
    /// `instance.constructor` must hand back the same object the module scope /
    /// exports hold, or identity checks (`x.constructor === Sub`, bundled zod's
    /// `describe()` re-construction via `this.constructor`) break. Last-wins
    /// across evaluations of the same class statement, matching the template-cid
    /// compromise used by the sibling tables above.
    pub static CLASS_OBJECT_VALUES: RwLock<Option<HashMap<u32, u64>>> = RwLock::new(None);
}

/// Store the marked class object for its template class id (see
/// `CLASS_OBJECT_VALUES`).
pub(crate) fn class_object_value_root_store(class_id: u32, obj_ptr: *mut ObjectHeader) {
    if class_id == 0 || obj_ptr.is_null() {
        return;
    }
    let bits = crate::value::js_nanbox_pointer(obj_ptr as i64).to_bits();
    CLASS_OBJECT_VALUES.with(|table| {
        let mut guard = table.write().unwrap();
        if guard.is_none() {
            *guard = Some(HashMap::new());
        }
        guard.as_mut().unwrap().insert(class_id, bits);
    });
    crate::gc::runtime_write_barrier_root_raw_ptr(obj_ptr);
}

/// Read back the class object registered for `class_id`, or `None` when the
/// class never materialized as a per-evaluation object (ordinary
/// ClassRef-valued classes).
pub(crate) fn class_object_value_for_cid(class_id: u32) -> Option<f64> {
    if class_id == 0 {
        return None;
    }
    CLASS_OBJECT_VALUES.with(|table| {
        table.read().ok().and_then(|guard| {
            guard
                .as_ref()
                .and_then(|map| map.get(&class_id).copied())
                .map(f64::from_bits)
        })
    })
}

pub(crate) fn class_prototype_object_root_store(class_id: u32, proto_ptr: *mut ObjectHeader) {
    if class_id == 0 || proto_ptr.is_null() {
        return;
    }
    CLASS_PROTOTYPE_OBJECTS.with(|table| {
        let mut guard = table.write().unwrap();
        if guard.is_none() {
            *guard = Some(HashMap::new());
        }
        guard.as_mut().unwrap().insert(class_id, proto_ptr as usize);
    });
    crate::gc::runtime_write_barrier_root_raw_ptr(proto_ptr);
}

pub(crate) fn class_decl_prototype_object_root_store(class_id: u32, proto_ptr: *mut ObjectHeader) {
    if class_id == 0 || proto_ptr.is_null() {
        return;
    }
    CLASS_DECL_PROTOTYPE_OBJECTS.with(|table| {
        let mut guard = table.write().unwrap();
        if guard.is_none() {
            *guard = Some(HashMap::new());
        }
        guard.as_mut().unwrap().insert(class_id, proto_ptr as usize);
    });
    crate::gc::runtime_write_barrier_root_raw_ptr(proto_ptr);
}

pub(crate) fn class_parent_closure_root_store(class_id: u32, closure_addr: usize) {
    if class_id == 0 || closure_addr == 0 {
        return;
    }
    CLASS_PARENT_CLOSURES.with(|table| {
        let mut guard = table.write().unwrap();
        if guard.is_none() {
            *guard = Some(HashMap::new());
        }
        guard.as_mut().unwrap().insert(class_id, closure_addr);
    });
    crate::gc::runtime_write_barrier_root_raw_ptr(closure_addr as *const u8);
}

/// Look up the parent-closure address recorded for a child class_id, if any.
pub(crate) fn class_parent_closure(class_id: u32) -> Option<usize> {
    CLASS_PARENT_CLOSURES.with(|table| {
        table
            .read()
            .ok()
            .and_then(|g| g.as_ref().and_then(|m| m.get(&class_id).copied()))
    })
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
    CLASS_DECL_PROTOTYPE_OBJECTS.with(|table| {
        table
            .read()
            .ok()?
            .as_ref()?
            .iter()
            .find(|(_, &p)| p == ptr)
            .map(|(k, _)| *k)
    })
}

/// #7757: a monomorphized specialization (`Gen$num`) must present the GENERIC's
/// reflective surface. TypeScript erases type arguments, so at runtime there is
/// exactly one `Gen`, one `Gen.prototype` and one `Gen.prototype.constructor` —
/// the specializations are an implementation detail of monomorphization
/// (`monomorph::mangle::generate_specialized_name`), and without this redirect
/// each one materialized its OWN decl-prototype object. That made
/// `a.constructor !== Gen`, `a.constructor !== b.constructor` and
/// `getPrototypeOf(a) !== getPrototypeOf(b)` where node says all three are
/// equal.
///
/// Same origin edge `instanceof` uses (#7575) and the display name uses
/// (#7632), applied to the third and last identity surface. METHOD DISPATCH is
/// unaffected: it runs off the per-class-id vtable, not this object, so each
/// specialization keeps its own monomorphized bodies.
fn decl_prototype_identity_id(class_id: u32) -> u32 {
    crate::object::class_generic_origin(class_id).unwrap_or(class_id)
}

pub(crate) fn class_decl_prototype_object(class_id: u32) -> *mut ObjectHeader {
    let class_id = decl_prototype_identity_id(class_id);
    CLASS_DECL_PROTOTYPE_OBJECTS.with(|table| {
        if let Ok(read) = table.read() {
            if let Some(map) = read.as_ref() {
                return map.get(&class_id).copied().unwrap_or(0) as *mut ObjectHeader;
            }
        }
        std::ptr::null_mut()
    })
}

pub(crate) fn class_decl_prototype_method_names(class_id: u32) -> Vec<String> {
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
    // #7757: a specialization answers with its generic's prototype.
    let class_id = decl_prototype_identity_id(class_id);
    if class_id == crate::wasi::CLASS_ID_WASI {
        crate::wasi::ensure_wasi_prototype_for_subclass();
        let proto = class_prototype_object(class_id);
        return (!proto.is_null())
            .then(|| crate::value::js_nanbox_pointer(proto as i64))
            .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
    }
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
    // #7769 follow-up: materializing a declared class's prototype object is
    // not prototype SURGERY, and it used to invalidate the fast guards as if
    // it were.
    //
    // `invalidate_class_prototype_fast_guards()` trips a process-global,
    // MONOTONIC latch. It disables every `js_method_direct_shape_guard` /
    // `js_typed_feedback_method_direct_call_guard` in the program, retires
    // every outstanding element-shape record (`invalidate_all_element_shapes`)
    // and bumps `VTABLE_GEN`, retiring the `vtable_ic` / `obj_dispatch_ic`
    // dispatch caches. It exists for the one event that can change which
    // member `recv.m()` resolves to: a WRITE to a prototype
    // (`Class.prototype.m = fn`), which is what the two call sites in
    // `prototype_methods.rs` cover.
    //
    // Reaching this line changes none of that. The object being created is
    // fresh and unobserved; the writes immediately below install
    // `constructor` plus exactly the methods the class already declares, i.e.
    // the same answers the vtable already gives. But because ANY demand for
    // `Class.prototype` lands here — `instanceof`, `Object.getPrototypeOf`,
    // a `super` chain — a plain class-hierarchy program disarmed its own
    // dispatch speculation during startup and then ran every `recv.m()`
    // through the `js_native_call_method` tower.
    //
    // Measured on `gc-handoff/apps/shapes.ts`: 384,000 of 384,000 shape-guard
    // probes failed here and nowhere else, and every element read fell back to
    // the generic index path for the same reason.
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

    // #5024 followup: backfill assignment-registered prototype methods
    // (`Class.prototype.m = fn`, stored in CLASS_PROTOTYPE_METHODS) onto the
    // decl-proto object as ordinary enumerable own properties, so reflective
    // own-key enumeration sees them. These typically run at module init,
    // BEFORE any reflective `.prototype` read materialises this object, so the
    // write-through in `class_prototype_method_root_store` had no decl-proto to
    // target. Mirrors the existing CLASS_VTABLE_REGISTRY backfill above.
    let registered: Vec<(String, u64)> = {
        CLASS_PROTOTYPE_METHODS.with(|table| {
            let guard = table.read().unwrap();
            guard
                .as_ref()
                .and_then(|map| map.get(&class_id))
                .map(|per_class| per_class.iter().map(|(k, &v)| (k.clone(), v)).collect())
                .unwrap_or_default()
        })
    };
    for (name, value_bits) in registered {
        let enumerable = class_prototype_method_is_enumerable(class_id, &name);
        unsafe { mirror_prototype_method_on_object(proto, &name, value_bits, enumerable) };
    }

    let dynamic_parent = js_get_dynamic_parent_value(class_id);
    let null_heritage = dynamic_parent.to_bits() == crate::value::TAG_NULL;
    let parent_proto_bits = if null_heritage {
        // A class extending null creates a prototype object whose
        // [[Prototype]] is null, not Object.prototype.  Record TAG_NULL
        // explicitly so "no custom link" is not mistaken for the ordinary
        // Object.prototype default.
        Some(crate::value::TAG_NULL)
    } else {
        get_parent_class_id(class_id)
            .filter(|parent_id| *parent_id != 0 && *parent_id != class_id)
            .and_then(|parent_id| {
                let parent_proto = class_decl_prototype_value(parent_id);
                let parent_bits = parent_proto.to_bits();
                ((parent_bits >> 48) == 0x7FFD).then_some(parent_bits)
            })
            .or_else(global_object_prototype_bits)
    };
    if let Some(bits) = parent_proto_bits {
        super::super::prototype_chain::object_set_static_prototype(proto as usize, bits);
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

pub(crate) fn global_object_prototype_bits() -> Option<u64> {
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

#[cfg(test)]
mod class_realm_isolation_tests {
    use super::*;
    use std::sync::{Arc, Barrier, Mutex};

    /// #8001 — a declared class's lazily materialized `.prototype` belongs to
    /// the agent that materialized it, not to whichever thread first used the
    /// process-wide class id.
    ///
    /// The class id and name are codegen facts shared by both agents. The heap
    /// object stored under that id is not: each thread owns a separate realm and
    /// arena. Bootstraps are serialized because the test harness's globalThis
    /// root slot is process-global, while the barrier keeps both arenas live
    /// until both addresses have been observed. Before #8001, agent B returned
    /// agent A's cached pointer at the first-thread-wins gate.
    #[test]
    fn a_second_agents_declared_prototype_address_is_its_own() {
        const CLASS_ID: u32 = 0x7d01_8001;
        const CLASS_NAME: &[u8] = b"Issue8001Class";

        unsafe {
            js_register_class_name(CLASS_ID, CLASS_NAME.as_ptr(), CLASS_NAME.len() as u32);
        }

        let bootstrap_gate = Arc::new(Mutex::new(()));
        let both_alive = Arc::new(Barrier::new(2));
        let agent = |gate: Arc<Mutex<()>>, barrier: Arc<Barrier>| {
            move || -> usize {
                let addr = {
                    let _serialized = gate.lock().expect("bootstrap gate");
                    let bits = class_decl_prototype_value(CLASS_ID).to_bits();
                    assert_eq!(
                        bits & crate::value::TAG_MASK,
                        crate::value::POINTER_TAG,
                        "the class prototype must materialize before isolation is tested"
                    );
                    (bits & crate::value::POINTER_MASK) as usize
                };
                barrier.wait();
                addr
            }
        };
        let spawn_agent = |gate: Arc<Mutex<()>>, barrier: Arc<Barrier>| {
            std::thread::Builder::new()
                .stack_size(16 << 20)
                .spawn(agent(gate, barrier))
                .expect("spawn realm agent")
        };

        let a = spawn_agent(Arc::clone(&bootstrap_gate), Arc::clone(&both_alive));
        let b = spawn_agent(bootstrap_gate, both_alive);
        let a_addr = a.join().expect("agent A panicked");
        let b_addr = b.join().expect("agent B panicked");

        for (label, addr) in [("agent A", a_addr), ("agent B", b_addr)] {
            assert_ne!(
                addr, 0,
                "{label} did not materialize a real class prototype; distinctness would be vacuous"
            );
        }
        assert_ne!(
            a_addr, b_addr,
            "two live agents must materialize their own Class.prototype objects (#8001)"
        );
    }
}

#[cfg(test)]
mod class_dynamic_prop_store_tests {
    use super::*;

    fn stored(class_id: u32, name: &str) -> Option<f64> {
        class_own_static_field_value(class_id, name)
    }

    /// The in-place update arm must be observationally identical to the
    /// insert arm — same table, same value, same key set. This is the shape
    /// `Shape.made = Shape.made + 1` produces once per construction.
    #[test]
    fn repeated_store_updates_in_place_and_stays_readable() {
        let cid = 0x7c01_0001;
        for i in 0..5u32 {
            class_dynamic_prop_root_store(cid, "made", f64::from(i));
            assert_eq!(stored(cid, "made"), Some(f64::from(i)));
        }
        // A second key on the same class still inserts.
        class_dynamic_prop_root_store(cid, "other", 9.0);
        assert_eq!(stored(cid, "other"), Some(9.0));
        assert_eq!(stored(cid, "made"), Some(4.0));
        let mut keys = class_own_enumerable_field_names(cid);
        keys.sort();
        assert_eq!(keys, vec!["made".to_string(), "other".to_string()]);
    }

    /// The fast path is gated on "nothing has ever been deleted". Once a key
    /// IS deleted, a re-store must still clear it from the deleted set — the
    /// behaviour the unconditional probe used to provide.
    #[test]
    fn store_after_delete_clears_the_deleted_mark() {
        let cid = 0x7c01_0002;
        class_dynamic_prop_root_store(cid, "k", 1.0);
        // Delete the way `delete C.k` does: drop the value AND mark the key.
        class_delete_own_dynamic_prop(cid, "k");
        class_mark_key_deleted(cid, "k");
        assert!(class_is_key_deleted(cid, "k"));
        assert_eq!(stored(cid, "k"), None);

        class_dynamic_prop_root_store(cid, "k", 2.0);
        assert!(
            !class_is_key_deleted(cid, "k"),
            "re-storing a deleted static key must un-delete it"
        );
        assert_eq!(stored(cid, "k"), Some(2.0));

        // And a subsequent store, now on the slow arm (the deleted-keys map
        // is non-empty for the whole process), still updates the value.
        class_dynamic_prop_root_store(cid, "k", 3.0);
        assert_eq!(stored(cid, "k"), Some(3.0));
    }
}
