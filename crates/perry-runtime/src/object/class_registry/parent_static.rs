use super::*;
use crate::object::*;
use crate::{ArrayHeader, JSValue};
use std::cell::{Cell, RefCell, UnsafeCell};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicU8, Ordering};
use std::sync::RwLock;

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
        let parent_cid = super::super::instanceof::global_builtin_constructor_class_id(name);
        if parent_cid != 0 && parent_cid != class_id {
            register_class(class_id, parent_cid);
        }
        // A dynamic subclass that resolves its parent through this builtin
        // branch must still record the fetch-parent kind so `new X()` attaches
        // the native Request/Response handle — the bookkeeping below this
        // early return would otherwise be skipped.
        match name {
            "Request" => super::super::register_fetch_parent_kind(class_id, 1),
            "Response" => super::super::register_fetch_parent_kind(class_id, 2),
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
        super::super::object_ops::throw_object_type_error(
            b"Class extends value is not a constructor",
        );
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
        Some("Request") => super::super::register_fetch_parent_kind(class_id, 1),
        Some("Response") => super::super::register_fetch_parent_kind(class_id, 2),
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
    const INT32_TAG: u64 = 0x7FFE_0000_0000_0000;
    if class_id == 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    {
        let guard = CLASS_DYNAMIC_PARENT_VALUE.read().unwrap();
        if let Some(&bits) = guard.as_ref().and_then(|m| m.get(&class_id)) {
            return f64::from_bits(bits);
        }
    }
    // #5957/#806: no dynamic VALUE stashed — fall back to the STATIC
    // parent-id edge as a ClassRef. An `extends <call>(...)` mixin
    // materialized by the HIR inline-init can resolve the chain statically
    // (module init registers `js_register_class_parent(child, parent)`)
    // while the per-value `js_register_class_parent_dynamic` side effect
    // lived in a body that never runs; before this fallback the
    // dynamic-parent super leg dispatched `undefined` and silently no-op'd
    // — the ancestor ctor never saw the forwarded args (the #806 mixin's
    // `seed` stayed undefined). A ClassRef routes the caller into the
    // registered-constructor flat dispatch, which fills user args and
    // snapshot caps by the signature split.
    if let Some(parent_cid) = crate::object::get_parent_class_id(class_id) {
        if parent_cid != 0 {
            return f64::from_bits(INT32_TAG | parent_cid as u64);
        }
    }
    f64::from_bits(TAG_UNDEFINED)
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
    // #5226: small typed arrays and `Buffer`s (incl. `new Uint8Array(n)`, which
    // lowers to a slab-allocated Buffer) are off-GC-heap with no GcHeader, so
    // the `ptr - GC_HEADER_SIZE` back-read below faults when the block sits at
    // the start of a freshly mapped region. They are never class objects —
    // reject via the side tables first (no back-read).
    if crate::typedarray::is_offheap_sidetable_alloc(ptr as usize) {
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
        {
            let mut guard = CLASS_SYMBOL_METHODS.write().unwrap();
            if guard.is_none() {
                *guard = Some(HashMap::new());
            }
            guard.as_mut().unwrap().insert(
                (class_id, sym_key, is_static != 0),
                (func_ptr as usize, param_count as u32, has_rest != 0),
            );
        }
        // A computed key that evaluates to a WELL-KNOWN symbol — e.g. the
        // minified `[(gm = new WeakMap, Symbol.asyncIterator)]() {…}` comma
        // form, whose key expression the lowering can't see through
        // statically — must land in the same synthetic vtable slot the
        // static `[Symbol.asyncIterator]` lowering uses. Every consumer
        // (GetIterator(async), the #5128 symbol-read binder,
        // `js_to_primitive`, the using-block desugar) resolves these by the
        // synthetic NAME on the class; `CLASS_SYMBOL_METHODS` above is not
        // consulted for instance dispatch. Without the alias,
        // `for await (… of instance)` threw `TypeError: value is not
        // iterable` for the comma-keyed form.
        if is_static == 0 {
            let alias = [
                ("iterator", "@@iterator"),
                ("asyncIterator", "@@asyncIterator"),
                ("toPrimitive", "@@toPrimitive"),
                ("dispose", "__perry_dispose__"),
                ("asyncDispose", "__perry_async_dispose__"),
            ]
            .iter()
            .find_map(|(wk, method_name)| {
                let s = crate::symbol::well_known_symbol(wk);
                if s.is_null() {
                    return None;
                }
                let f = f64::from_bits(crate::value::JSValue::pointer(s as *const u8).bits());
                if sym_key == crate::symbol::sym_key_from_f64(f) {
                    Some(*method_name)
                } else {
                    None
                }
            });
            if let Some(method_name) = alias {
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
                    method_name.to_string(),
                    VTableMethodEntry {
                        func_ptr: func_ptr as usize,
                        param_count: param_count as u32,
                        has_synthetic_arguments: false,
                        has_rest: has_rest != 0,
                    },
                );
            }
        }
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

/// Presence-only check (`[[HasProperty]]`, never `[[Get]]`) for a Symbol-keyed
/// METHOD or ACCESSOR declared on `class_id` or any ancestor. These computed
/// members register into `CLASS_SYMBOL_METHODS` / `CLASS_SYMBOL_ACCESSORS`, which
/// the generic symbol resolver (`js_object_get_symbol_property`) does NOT consult
/// — so `sym in Class` reported false even though `Class[sym](...)` dispatches
/// fine through the direct-call path. Walks the parent chain like
/// `lookup_class_symbol_method_in_chain`, but returns a bool and also covers
/// accessors so a static/instance `get [sym]()` is detected without invoking the
/// getter. Refs #6160.
pub(crate) fn class_has_symbol_member_in_chain(
    class_id: u32,
    sym_key: usize,
    is_static: bool,
) -> bool {
    let mut cid = class_id;
    let mut depth = 0usize;
    while cid != 0 && depth < 32 {
        let key = (cid, sym_key, is_static);
        let in_methods = CLASS_SYMBOL_METHODS
            .read()
            .ok()
            .and_then(|g| g.as_ref().map(|m| m.contains_key(&key)))
            .unwrap_or(false);
        if in_methods {
            return true;
        }
        let in_accessors = CLASS_SYMBOL_ACCESSORS
            .read()
            .ok()
            .and_then(|g| g.as_ref().map(|m| m.contains_key(&key)))
            .unwrap_or(false);
        if in_accessors {
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

/// Whether the class chain rooted at `class_id` defines an instance getter OR
/// setter named `name` (on `Class.prototype`, via `js_register_class_getter` /
/// `js_register_class_setter`). These accessors live in the per-class vtable,
/// NOT in the address-keyed descriptor tables, so a prototype-object descriptor
/// scan would miss them — the dynamic-write fast path must consult this before
/// treating `instance[name] = v` as a plain own-data store (an inherited
/// accessor must intercept instead). Walks the `extends` chain like
/// [`class_has_instance_getter`].
pub(crate) fn class_chain_has_instance_accessor(class_id: u32, name: &str) -> bool {
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
            if vt.getters.contains_key(name) || vt.setters.contains_key(name) {
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
        if !proto_obj.is_null()
            && (*proto_obj).class_id == NATIVE_MODULE_CLASS_ID
            && read_native_module_name(proto_obj as *const ObjectHeader).as_deref()
                == Some("buffer.Buffer")
        {
            let result = crate::object::native_module::call_native_module_dispatch_hook(
                proto_obj, name, args_ptr, args_len,
            );
            if !JSValue::from_bits(result.to_bits()).is_undefined() {
                return Some(result);
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
    // `class X extends Promise` — inherited builtin static (`X.all(...)`,
    // `X.resolve(...)`, …). Dispatch the spec static with `this` = the subclass
    // receiver so `NewPromiseCapability(X)` constructs the subclass. Resolves the
    // reified static value and calls it (its thunk reads `this` from the
    // implicit-this slot, already bound to `receiver` by the caller above).
    if super::promise_parent_in_chain(class_id)
        && crate::object::promise_static_function_spec(name).is_some()
    {
        let static_val = crate::object::js_promise_static_function_value(name.as_ptr(), name.len());
        if static_val.to_bits() != crate::value::TAG_UNDEFINED {
            // The reified static thunk reads its `this` constructor from the
            // implicit-this slot, so bind it to the subclass receiver for the
            // duration of the call — `NewPromiseCapability(receiver)` then
            // constructs the subclass.
            let prev_this = crate::object::js_implicit_this_set(receiver);
            let result = crate::closure::js_native_call_value(static_val, args_ptr, args_len);
            crate::object::js_implicit_this_set(prev_this);
            return result;
        }
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
