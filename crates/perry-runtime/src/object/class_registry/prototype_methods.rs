use super::*;
use std::collections::HashMap;
use std::sync::RwLock;

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
pub(crate) static CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub(crate) fn class_prototype_fast_guards_invalidated() -> bool {
    CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED.load(std::sync::atomic::Ordering::Acquire)
}

pub(crate) fn invalidate_class_prototype_fast_guards() {
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
            .or_default()
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
    let enumerable = class_prototype_method_is_enumerable(class_id, &name);
    let proto = class_prototype_object(class_id);
    if !proto.is_null() {
        unsafe { mirror_prototype_method_on_object(proto, &name, value_bits, enumerable) };
    }
    // #5024 followup: reflective `ClassName.prototype` enumeration
    // (`Object.keys` / `getOwnPropertyNames` / `in` / `hasOwnProperty` /
    // `for-in`) reads the DECL-prototype object (CLASS_DECL_PROTOTYPE_OBJECTS),
    // which is a DIFFERENT object than the #711/#809 synthetic prototype cache
    // (CLASS_PROTOTYPE_OBJECTS) the mirror above targets. Without mirroring
    // here too, an assignment-registered method (`Class.prototype.m = fn`) was
    // dispatchable (side table) but invisible to own-key enumeration on the
    // reflective prototype — zod's `b1` trait factory copies base methods onto
    // instances via `for (let H in O.prototype) ...`, which enumerated nothing,
    // so `z.number().optional()` threw "Cannot read properties of undefined".
    // When the decl-proto isn't materialised yet, `class_decl_prototype_value`
    // backfills CLASS_PROTOTYPE_METHODS at materialisation time, so we only
    // need to write through to an already-live decl-proto here.
    let decl_proto = class_decl_prototype_object(class_id);
    if !decl_proto.is_null() && decl_proto != proto {
        unsafe { mirror_prototype_method_on_object(decl_proto, &name, value_bits, enumerable) };
    }
}

/// #5024: write a side-table-registered prototype method onto the
/// materialized prototype object so the key lands in its `keys_array`.
/// `enumerable` carries assignment semantics (`Class.prototype.m = fn` →
/// enumerable) vs `Object.defineProperty` default (non-enumerable). Values
/// keep their full NaN-boxed bits; dispatch paths that find the property on
/// the object see the same value the side table holds.
pub(crate) unsafe fn mirror_prototype_method_on_object(
    proto: *mut ObjectHeader,
    name: &str,
    value_bits: u64,
    enumerable: bool,
) {
    if proto.is_null() || name.is_empty() {
        return;
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(proto, key, f64::from_bits(value_bits));
    if !enumerable {
        // `js_object_set_field_by_name` records the default (enumerable) attrs;
        // override so reflective own-key enumeration skips a defineProperty-
        // registered non-enumerable method.
        set_builtin_property_attrs(
            proto as usize,
            name.to_string(),
            PropertyAttrs::new(true, false, true),
        );
    }
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
