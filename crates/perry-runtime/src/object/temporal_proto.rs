//! `Temporal.<Type>.prototype` population — real accessor + method properties.
//!
//! A `Temporal.*` value is a NaN-boxed cell with no JS `[[Prototype]]` link, so
//! instance getter reads / method calls dispatch through the runtime's brand
//! arms (see [`crate::temporal::dispatch`]) rather than the prototype chain.
//! Test262 nonetheless *introspects* the prototype directly —
//! `Object.getOwnPropertyDescriptor(Temporal.PlainTime.prototype, "hour").get`,
//! `Temporal.PlainTime.prototype.add.length`, `isConstructor(...)`,
//! `Object.isExtensible(...)`, etc. — so the prototype object must carry the
//! real accessor getters and method functions with spec-correct attributes.
//!
//! Both the getter and method bodies are **generic**: one shared thunk each
//! reads the property/method name back off its own closure (set via
//! [`set_bound_native_closure_name`]) and routes to the per-type dispatch
//! router. Each getter brand-checks `this` (a non-Temporal receiver throws
//! `TypeError`, matching the spec's `RequireInternalSlot`).

use super::*;
use crate::temporal::dispatch as tdispatch;
/// Read a closure's installed `name` dynamic-prop as a Rust string.
fn closure_name(c: *const crate::closure::ClosureHeader) -> String {
    let name_val = crate::closure::closure_get_dynamic_prop(c as usize, "name");
    let jv = crate::value::JSValue::from_bits(name_val.to_bits());
    if !jv.is_string() {
        return String::new();
    }
    let ptr = jv.as_string_ptr();
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

/// Throw `TypeError: get/call Temporal.<Type>.prototype.<member> called on an
/// incompatible receiver` — the brand-check failure shared by every accessor /
/// method when `this` is not the matching Temporal value.
fn throw_brand(member: &str) -> ! {
    let msg = format!("Temporal.prototype.{member} called on an incompatible receiver");
    crate::object::throw_object_type_error(msg.as_bytes())
}

/// Shared accessor getter for every `Temporal.<Type>.prototype` field. Reads
/// `IMPLICIT_THIS`, derives the property name from the closure (`"get hour"` →
/// `"hour"`), and routes to the brand router; a non-Temporal `this` throws.
pub(super) extern "C" fn temporal_proto_getter_thunk(
    c: *const crate::closure::ClosureHeader,
) -> f64 {
    let recv = f64::from_bits(IMPLICIT_THIS.with(|x| x.get()));
    let full = closure_name(c);
    let prop = full.strip_prefix("get ").unwrap_or(&full);
    match tdispatch::get_property(recv, prop) {
        Some(v) => v,
        None => throw_brand(prop),
    }
}

/// Shared method thunk for every `Temporal.<Type>.prototype` method. Reads
/// `IMPLICIT_THIS`, derives the method name from the closure, brand-checks the
/// receiver, and routes to the brand router with the rest-array args.
pub(super) extern "C" fn temporal_proto_method_thunk(
    c: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let recv = f64::from_bits(IMPLICIT_THIS.with(|x| x.get()));
    let name = closure_name(c);
    if crate::temporal::temporal_kind(recv).is_none() {
        throw_brand(&name);
    }
    let args = super::global_this::global_this_rest_array_values(rest);
    tdispatch::call_method(recv, &name, &args)
}

/// Install a brand-checking accessor getter (`{ get, set: undefined,
/// enumerable: false, configurable: true }`) onto a Temporal prototype. Mirrors
/// `regex_proto_thunks::install_getter`.
fn install_getter(proto_obj: *mut ObjectHeader, name: &str) {
    if proto_obj.is_null() {
        return;
    }
    let func_ptr = temporal_proto_getter_thunk as *const u8;
    unsafe {
        crate::closure::js_register_closure_arity(func_ptr, 0);
        let closure = crate::closure::js_closure_alloc(func_ptr, 0);
        if closure.is_null() {
            return;
        }
        super::native_module::set_bound_native_closure_name(closure, &format!("get {name}"));
        super::native_module::set_builtin_closure_length(closure as usize, 0);
        super::native_module::set_builtin_closure_non_constructable(closure as usize);
        let getter_bits = crate::value::js_nanbox_pointer(closure as i64).to_bits();
        // #6809: Temporal cell reads use the brand dispatcher and direct
        // prototype reads use the per-owner descriptor marker installed by
        // this helper. Keep startup gate-neutral.
        super::object_ops::install_builtin_getter(proto_obj, name, getter_bits);
        super::set_builtin_property_attrs(
            closure as usize,
            "name".to_string(),
            super::PropertyAttrs::new(false, false, true),
        );
        super::set_builtin_property_attrs(
            closure as usize,
            "length".to_string(),
            super::PropertyAttrs::new(false, false, true),
        );
    }
}

/// Install one variadic method (spec `length` = `spec_length`) onto a Temporal
/// prototype, sharing the generic method thunk.
fn install_method(proto_obj: *mut ObjectHeader, name: &str, spec_length: u32) {
    super::global_this::install_proto_method_rest_with_length(
        proto_obj,
        name,
        temporal_proto_method_thunk as *const u8,
        spec_length,
        0,
    );
}

/// Resolve a Temporal constructor closure's `.prototype` object pointer,
/// creating + caching it on the closure if absent.
///
/// This populator runs *inside* the globalThis singleton build (Temporal is one
/// of the namespaces installed there), at which point `GLOBAL_THIS_READY` is
/// still false. So we must NOT route through `js_function_prototype_value_for_read`
/// — its `ensure_function_prototype_object` reads `globalThis.Object.prototype`
/// via `js_get_global_this()`, which would spin forever waiting for the very
/// build we're in. Instead we allocate the prototype directly and stamp it onto
/// the closure's `prototype` dynamic prop; a later `Temporal.X.prototype` read
/// (post-init) finds exactly this cached object. The `[[Prototype]]` →
/// `Object.prototype` link is intentionally skipped to stay re-entrancy-free.
fn ctor_prototype(ctor: *mut crate::closure::ClosureHeader) -> *mut ObjectHeader {
    if ctor.is_null() {
        return std::ptr::null_mut();
    }
    // Reuse an already-stamped prototype (idempotent install).
    let existing = crate::closure::closure_get_dynamic_prop(ctor as usize, "prototype");
    let ejv = crate::value::JSValue::from_bits(existing.to_bits());
    if ejv.is_pointer() {
        let p = ejv.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
        if !p.is_null() {
            return p;
        }
    }
    let proto = js_object_alloc(0, 0);
    if proto.is_null() {
        return std::ptr::null_mut();
    }
    // Stamp the prototype onto the closure's `prototype` dynamic prop ONLY — NOT
    // the GC-scanned class-prototype cache. `ensure_function_prototype_object`'s
    // Temporal gate returns this object for `new`/`.prototype` reads, so it is
    // never overwritten, while staying reachable solely through globalThis (the
    // cache would dangle across the test-suite's arena-fixture swaps → SIGSEGV).
    crate::closure::closure_set_dynamic_prop(
        ctor as usize,
        "prototype",
        crate::value::js_nanbox_pointer(proto as i64),
    );
    // A constructor's `.prototype` is `{ writable: false, enumerable: false,
    // configurable: false }` per spec (Temporal.X.prototype prop-desc tests).
    super::set_builtin_property_attrs(
        ctor as usize,
        "prototype".to_string(),
        super::PropertyAttrs::new(false, false, false),
    );
    proto
}

/// Populate `ctor.prototype` with `getters` (accessors), `methods`
/// (`(name, spec_length)`), a `@@toStringTag` of `tag`, and a `constructor`
/// back-reference. Shared by every Temporal type.
pub(super) fn populate_prototype(
    ctor: *mut crate::closure::ClosureHeader,
    tag: &str,
    getters: &[&str],
    methods: &[(&str, u32)],
) {
    let proto = ctor_prototype(ctor);
    if proto.is_null() {
        return;
    }
    for g in getters {
        install_getter(proto, g);
    }
    for (m, len) in methods {
        install_method(proto, m, *len);
    }
    // `constructor` — `{ writable: true, enumerable: false, configurable: true }`.
    let ctor_val = crate::value::js_nanbox_pointer(ctor as i64);
    let ckey = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
    js_object_set_field_by_name(proto, ckey, ctor_val);
    super::set_builtin_property_attrs(
        proto as usize,
        "constructor".to_string(),
        super::PropertyAttrs::new(true, false, true),
    );
    super::global_this::set_intrinsic_to_string_tag(proto, tag);
}
