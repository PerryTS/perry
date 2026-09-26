//! `OrdinaryToPrimitive`/`ToPrimitive` resolution for objects, functions, and
//! exotic instances (Date/RegExp own-property overrides).
//!
//! Split out of `to_string.rs`, which sits at the 2000-line cap (#8480 series).
//! This module holds the general ToPrimitive machinery that
//! `js_jsvalue_to_string` (kept in `to_string.rs`) and other coercion sites
//! (`+`, `String()`, template literals) dispatch through.

use super::to_string_class_ref::{custom_to_primitive, CustomToPrimitiveOutcome};
use super::*;
use std::cell::Cell;

crate::perry_thread_local! {
    /// Re-entrancy guard for `OrdinaryToPrimitive(string)`. A user
    /// `toString`/`valueOf` whose body coerces `this` back to a string
    /// (e.g. `toString() { return "" + this; }`) would recurse forever;
    /// Node throws `RangeError: Maximum call stack size exceeded`. We cap
    /// the depth and fall back to `[object Object]` instead of overflowing
    /// the Rust stack (which would SIGSEGV the whole process).
    static TO_PRIMITIVE_DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// `OrdinaryToPrimitive(O, "string")` (ES2024 §7.1.1.1) — the fallback
/// `ToPrimitive` step used by `String(obj)` / template literals / `obj + ""`
/// when the object has no `[Symbol.toPrimitive]`. For hint "string" the
/// method order is `toString` then `valueOf`; each is invoked with
/// `this = obj` and the first call returning a *primitive* (non-object)
/// value wins.
///
/// Returns `Some(primitive_f64)` when a callable `toString`/`valueOf` was
/// found on the object (own property or anywhere on its prototype chain,
/// reusing the same `js_object_get_field_by_name` resolution +
/// `clone_closure_rebind_this` receiver-binding the method-dispatch tower
/// uses — see #1969/#1982) and produced a primitive. Returns `None` when
/// neither method exists / is callable / yields a primitive, so the caller
/// falls back to `"[object Object]"`.
///
/// `value` MUST be a NaN-boxed `POINTER_TAG` object whose pointer is a real
/// heap address (`>= 0x10000`); the caller has already excluded symbols,
/// buffers, arrays, and JSX nodes (those carry their own coercion rules).
pub(crate) unsafe fn ordinary_to_primitive_string(value: f64) -> Option<f64> {
    // Bound recursion: a `toString` that itself string-coerces `this`.
    let depth = TO_PRIMITIVE_DEPTH.with(|c| c.get());
    if depth >= 200 {
        return None;
    }
    TO_PRIMITIVE_DEPTH.with(|c| c.set(depth + 1));
    let result = ordinary_to_primitive_string_inner(value);
    TO_PRIMITIVE_DEPTH.with(|c| c.set(depth));
    result
}

unsafe fn ordinary_to_primitive_string_inner(value: f64) -> Option<f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);

    // Hint "string": method order is `toString` then `valueOf` (ES2024
    // §7.1.1.1). CRITICAL: in the real spec EVERY ordinary object inherits
    // `Object.prototype.toString` (callable, returns `"[object Object]"`),
    // so for the string hint `toString` is *always present* — `valueOf` is
    // reached ONLY when a custom `toString` returns a non-primitive. Perry's
    // object model has no discoverable `Object.prototype.toString` field, so
    // `js_object_get_field_by_name(obj, "toString")` returning undefined
    // STANDS IN FOR that default `"[object Object]"`. We must therefore stop
    // and fall back to `"[object Object]"` (return None) rather than
    // proceeding to `valueOf` — otherwise `String({ valueOf() {…} })`
    // (string hint) would wrongly use `valueOf` where Node uses the default
    // `toString`.
    let to_string_result = call_method_for_primitive(&scope, &value_handle, b"toString");
    match to_string_result {
        MethodOutcome::Primitive(p) => return Some(p),
        // Custom toString returned a non-primitive (object): per spec, fall
        // through to `valueOf`.
        MethodOutcome::NonPrimitive => {}
        // No callable custom toString. For an ordinary object this stands in
        // for the default `Object.prototype.toString` → `"[object Object]"`
        // (stop here). But a null-`[[Prototype]]` object (`Object.create(null)`)
        // genuinely has NO toString/valueOf, so OrdinaryToPrimitive must fall
        // through to `valueOf` and, finding none, throw — matching Node
        // (`String(Object.create(null))` throws; Test262 ToPropertyKey on a
        // null-proto computed key).
        MethodOutcome::Absent => {
            // A boxed primitive wrapper (`new Number/String/Boolean/BigInt(x)`,
            // or a reflective `Object(x)`) stores its `[[PrimitiveData]]` in a
            // side table (`BOXED_PRIMITIVE_PAYLOADS`), not as a regular own
            // field — so `call_method_for_primitive`'s `js_object_get_field_by_name`
            // lookup for `toString` misses even though `Number.prototype.toString`
            // /etc. are real installed methods, and this arm would otherwise
            // treat the wrapper like a plain object and render `"[object
            // Object]"`. The "default"/number-hint ToPrimitive path
            // (`OrdinaryToPrimitiveOutcome`) already special-cases this via the
            // same `boxed_primitive_payload` lookup; mirror it here for the
            // string hint so `String(new Boolean(true))`, a reflective
            // `Number.prototype.indexOf = String.prototype.indexOf` receiver
            // coercion, etc. render the wrapped value instead of the object
            // default (test262 indexOf/lastIndexOf/replace/concat
            // generic-receiver cases).
            if let Some((_class_id, payload)) = crate::builtins::boxed_primitive_payload(value) {
                let s = js_jsvalue_to_string(payload);
                return Some(crate::value::js_nanbox_string(s as i64));
            }
            if !value_is_null_proto_object(value) {
                return None;
            }
        }
    }

    match call_method_for_primitive(&scope, &value_handle, b"valueOf") {
        MethodOutcome::Primitive(p) => Some(p),
        // We only reach here when a *custom* `toString` ran and returned a
        // non-primitive (the `Absent` toString case already returned
        // `"[object Object]"` above). Per spec `OrdinaryToPrimitive` then tries
        // `valueOf`; if that also fails to yield a primitive, ToPrimitive throws
        // `TypeError: Cannot convert object to primitive value` (Node agrees:
        // `String({ toString: () => ({}) })` throws). A plain object with no
        // custom `toString` never reaches this throw.
        MethodOutcome::NonPrimitive | MethodOutcome::Absent => throw_cannot_convert_to_primitive(),
    }
}

/// True iff `value` is a heap object stamped `OBJ_FLAG_NULL_PROTO`
/// (`Object.create(null)` and friends) — i.e. it has no `[[Prototype]]`, so it
/// does not inherit the default `Object.prototype.toString`/`valueOf`.
unsafe fn value_is_null_proto_object(value: f64) -> bool {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_pointer() {
        return false;
    }
    let obj = jsval.as_pointer::<crate::ObjectHeader>();
    if obj.is_null() || (obj as usize) < 0x10000 {
        return false;
    }
    if !crate::object::is_valid_obj_ptr(obj as *const u8) {
        return false;
    }
    if (obj as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return false;
    }
    let gc = (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    (*gc)._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0
}

#[cold]
pub(crate) fn throw_cannot_convert_to_primitive() -> ! {
    let msg = b"Cannot convert object to primitive value";
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Spec-faithful `OrdinaryToPrimitive(O, hint)` (ES2024 §7.1.1.1) for the
/// `Date.prototype[@@toPrimitive]` thunk. Unlike the coercion helpers above
/// (which special-case a *missing* `toString` as the inherited
/// `Object.prototype.toString` → `"[object Object]"` for `String()`/`+`), this
/// implements the abstract operation directly: it `Get`s each of the ordered
/// method names off the receiver (firing accessor getters + walking the
/// prototype chain via `js_reflect_get`), and — only when the resolved value
/// `IsCallable` — invokes it with `this = value` and returns the first
/// *primitive* result. A non-callable slot (including `null`/`undefined`) is
/// SKIPPED, not treated as the default. If no method yields a primitive, it
/// throws `TypeError`.
///
/// `try_string_first`: `true` for hint "string"/"default" (order `toString`
/// then `valueOf`), `false` for hint "number" (order `valueOf` then
/// `toString`). `value` MUST already be an Object (the thunk brand-checks
/// `Type(O) is Object` first).
pub(crate) unsafe fn ordinary_to_primitive_for_toprimitive(
    value: f64,
    try_string_first: bool,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);
    let order: [&[u8]; 2] = if try_string_first {
        [b"toString", b"valueOf"]
    } else {
        [b"valueOf", b"toString"]
    };
    // #9445: the displaced receiver is rooted ONCE here, not once per callback.
    let prev_this = scope.root_nanbox_f64(crate::object::js_implicit_this_get());
    for name in order {
        let recv = value_handle.get_nanbox_f64();
        // #10510: a runtime-spelled literal — reuse the interned key instead
        // of allocating a fresh string on every conversion.
        let key_ptr = crate::string::canonical_key(name);
        let key = f64::from_bits(crate::value::js_nanbox_string(key_ptr as i64).to_bits());
        // `Get(O, name)` — fires accessor getters and walks the prototype chain,
        // exactly like the spec's abstract `Get` (so `{ get valueOf() {…} }` is
        // observed and a getter throw propagates).
        let method = crate::proxy::js_reflect_get(recv, key, recv);
        if !crate::collection_iter::is_callable(method) {
            // Non-callable (or absent) — skip to the next name.
            continue;
        }
        // #10510: the builtin Date `valueOf` on a Date is its time value.
        if let Some(time) = crate::object::date_proto_thunks::builtin_date_value_of_result(
            method,
            value_handle.get_nanbox_f64(),
        ) {
            return time;
        }
        let method_handle = scope.root_nanbox_f64(method);
        let recv = value_handle.get_nanbox_f64();
        crate::object::js_implicit_this_set(recv);
        let result = crate::closure::js_native_call_value(
            method_handle.get_nanbox_f64(),
            std::ptr::null(),
            0,
        );
        crate::object::js_implicit_this_set(prev_this.get_nanbox_f64());
        if is_primitive_value(result) {
            return result;
        }
        // A callable that returned an Object: continue to the next name (spec
        // step 5.a.iii only returns when the result is NOT an Object).
    }
    throw_cannot_convert_to_primitive()
}

/// Outcome of resolving a function/closure's custom `toString`/`valueOf` for
/// the string-hint `ToPrimitive`. Distinct from a bare `Option` so the
/// "no custom method at all" case (fall back to the function's own source
/// text) can be told apart from "a custom method existed and ran, but
/// neither it nor the `valueOf` fallback produced a primitive" (must throw,
/// per `OrdinaryToPrimitive` — rendering source text there would be wrong).
pub(crate) enum FunctionToStringOutcome {
    /// Neither `toString` nor `valueOf` resolved to a callable — use the
    /// function's own source-text default.
    NoCustomMethod,
    Primitive(*mut crate::string::StringHeader),
    /// A custom `toString`/`valueOf` was callable but exhausted without
    /// producing a primitive.
    TypeError,
}

/// Function objects are closure headers, not `ObjectHeader`s, so the ordinary
/// object helper cannot see the default `%Function.prototype%` chain. Resolve
/// the function `toString`/`valueOf` methods explicitly so monkeypatching
/// `Function.prototype.toString` affects `String(fn)` and template coercion.
/// Faithful to `OrdinaryToPrimitive(fn, "string")`: try `toString` first: a
/// primitive result wins; a non-primitive result falls through to `valueOf`
/// (ECMA-262 §7.1.1.1) instead of giving up and rendering the function's own
/// source text — test262 `S15.5.2.1_A1_T11` overrides `toString` to return a
/// non-primitive and expects the `valueOf` result to be used. If BOTH are
/// callable and neither yields a primitive, `OrdinaryToPrimitive` throws
/// rather than falling back to the source-text default.
pub(crate) unsafe fn function_to_string_via_prototype(value: f64) -> FunctionToStringOutcome {
    let depth = TO_PRIMITIVE_DEPTH.with(|c| c.get());
    if depth >= 200 {
        return FunctionToStringOutcome::NoCustomMethod;
    }
    TO_PRIMITIVE_DEPTH.with(|c| c.set(depth + 1));
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);
    let mut tried_custom_method = false;
    let mut result = None;
    if let FunctionMethodOutcome::Value(ret) =
        call_function_method(&scope, &value_handle, b"toString")
    {
        tried_custom_method = true;
        if is_primitive_value(ret) {
            result = Some(js_jsvalue_to_string(ret));
        }
    }
    if result.is_none() {
        if let FunctionMethodOutcome::Value(ret) =
            call_function_method(&scope, &value_handle, b"valueOf")
        {
            tried_custom_method = true;
            if is_primitive_value(ret) {
                result = Some(js_jsvalue_to_string(ret));
            }
        }
    }
    TO_PRIMITIVE_DEPTH.with(|c| c.set(depth));
    match result {
        Some(s) => FunctionToStringOutcome::Primitive(s),
        None if tried_custom_method => FunctionToStringOutcome::TypeError,
        None => FunctionToStringOutcome::NoCustomMethod,
    }
}

/// Same lookup as `function_to_string_via_prototype`, but returns the raw
/// method-call result for explicit `fn.toString()` dispatch.
pub(crate) unsafe fn function_to_string_method_result(value: f64) -> Option<f64> {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_pointer() {
        return None;
    }
    let raw = jsval.as_pointer::<u8>() as usize;
    if raw == 0 || !crate::closure::is_closure_ptr(raw) {
        return None;
    }

    let depth = TO_PRIMITIVE_DEPTH.with(|c| c.get());
    if depth >= 200 {
        return None;
    }
    TO_PRIMITIVE_DEPTH.with(|c| c.set(depth + 1));

    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);
    let result = match call_function_method(&scope, &value_handle, b"toString") {
        FunctionMethodOutcome::Value(result) => Some(result),
        FunctionMethodOutcome::NonCallable | FunctionMethodOutcome::Absent => None,
    };

    TO_PRIMITIVE_DEPTH.with(|c| c.set(depth));
    result
}

enum FunctionMethodOutcome {
    /// Method was callable and returned a value.
    Value(f64),
    /// A property was found, but it was not callable.
    NonCallable,
    /// No own/inherited method with that name was found.
    Absent,
}

enum MethodOutcome {
    /// Method was callable and returned a primitive.
    Primitive(f64),
    /// Method was callable but returned a non-primitive (object/array).
    NonPrimitive,
    /// No own/inherited callable method with that name was found.
    Absent,
}

pub(crate) enum OrdinaryToPrimitiveOutcome {
    Primitive(f64),
    DefaultString,
    TypeError,
}

pub(crate) fn is_primitive_value(value: f64) -> bool {
    let jsval = JSValue::from_bits(value.to_bits());
    jsval.is_any_string()
        || jsval.is_number()
        || (jsval.is_int32() && crate::object::class_ref_id(value).is_none())
        || jsval.is_bool()
        || jsval.is_null()
        || jsval.is_undefined()
        || jsval.is_bigint()
        || ((value.to_bits() & 0xFFFF_0000_0000_0000) == POINTER_TAG
            && crate::symbol::is_registered_symbol((value.to_bits() & POINTER_MASK) as usize))
}

/// Result of consulting an exotic instance's OWN `toString` (#6370).
pub(crate) enum ExoticOwnToString {
    /// No own `toString` — the caller runs the built-in prototype conversion
    /// (`RegExp.prototype.toString` → `/source/flags`,
    /// `Date.prototype.toString` → the full local date string).
    UseBuiltin,
    /// An own override produced a primitive; ToString *that* instead.
    Primitive(f64),
}

/// `OrdinaryToPrimitive(O, "string")` step 1 for an exotic instance whose own
/// properties live in the `exotic_expando` side table.
///
/// A `RegExpHeader` / `DateCell` is NOT an `ObjectHeader`, so the generic
/// `ordinary_to_primitive_string` (which resolves `toString` with
/// `js_object_get_field_by_name`) cannot see their own properties — the regex
/// and date arms of [`js_jsvalue_to_string`] therefore jumped straight to the
/// built-in conversion and an own `toString` was silently ignored. That made
/// the SAME regex stringify two different ways depending on how you asked:
/// `re.toString()` honoured the override (the method fold, #6358) while
/// `String(re)` / `` `${re}` `` / `[re].join("")` printed `/source/flags`.
/// Ordinary `[[Get]]` consults own properties before the prototype chain, so
/// the override must win on EVERY ToString site (#6370).
///
/// Hot-path note: `js_jsvalue_to_string` runs on every string concat, so this
/// is only ever reached behind the existing `is_date_cell_addr` /
/// `is_regex_pointer` gates, and `exotic_get_own_property` itself early-outs
/// before any map lookup while no expando/descriptor has been installed on the
/// thread. A value that is not a Date/RegExp pays nothing.
pub(crate) unsafe fn exotic_own_to_string(
    addr: usize,
    kind: crate::object::exotic_expando::ExoticKind,
    receiver: f64,
) -> ExoticOwnToString {
    // Bound the recursion exactly as `ordinary_to_primitive_string` does. An
    // override whose body string-coerces `this`
    // (`re.toString = function () { return "" + this; }`) re-enters this
    // helper through `js_jsvalue_to_string` and would recurse until the Rust
    // stack overflows and SIGSEGVs the process. Node raises
    // `RangeError: Maximum call stack size exceeded`; Perry's convention in
    // this file is to cap the depth and fall back to the built-in conversion.
    let depth = TO_PRIMITIVE_DEPTH.with(|c| c.get());
    if depth >= 200 {
        return ExoticOwnToString::UseBuiltin;
    }
    TO_PRIMITIVE_DEPTH.with(|c| c.set(depth + 1));
    let outcome = exotic_own_to_string_inner(addr, kind, receiver);
    TO_PRIMITIVE_DEPTH.with(|c| c.set(depth));
    match outcome {
        ExoticOwnOutcome::UseBuiltin => ExoticOwnToString::UseBuiltin,
        ExoticOwnOutcome::Primitive(primitive) => ExoticOwnToString::Primitive(primitive),
        // Thrown out here, AFTER the depth counter is restored.
        ExoticOwnOutcome::NoPrimitive => throw_cannot_convert_to_primitive(),
    }
}

/// Non-throwing core of [`exotic_own_to_string`], so the depth counter can be
/// restored before the `TypeError` leaves the helper.
enum ExoticOwnOutcome {
    UseBuiltin,
    Primitive(f64),
    NoPrimitive,
}

unsafe fn exotic_own_to_string_inner(
    addr: usize,
    kind: crate::object::exotic_expando::ExoticKind,
    receiver: f64,
) -> ExoticOwnOutcome {
    // Accessor-aware: the override may be installed as
    // `Object.defineProperty(re, "toString", { get() {…} })`, which a
    // data-only expando read cannot see. `exotic_get_own_property` checks
    // accessor descriptors first (invoking the getter with `receiver` as the
    // receiver) and falls back to the expando data lookup.
    let Some(own) =
        crate::object::exotic_expando::exotic_get_own_property(addr, kind, "toString", receiver)
    else {
        return ExoticOwnOutcome::UseBuiltin;
    };
    if let Some(primitive) = call_own_method_for_primitive(own, receiver) {
        return ExoticOwnOutcome::Primitive(primitive);
    }
    // An own `toString` that is NOT callable (`re.toString = 5`) or that
    // returns an object still SHADOWS the built-in — it is never a licence to
    // fall back to `RegExp.prototype.toString`. OrdinaryToPrimitive continues
    // with `valueOf`, and only an OWN `valueOf` can yield a primitive here:
    // the inherited `Object.prototype.valueOf` returns `this`, an object. When
    // neither yields, ToPrimitive throws — Node agrees
    // (`re.toString = 5; String(re)` → "TypeError: Cannot convert object to
    // primitive value").
    if let Some(own_value_of) =
        crate::object::exotic_expando::exotic_get_own_property(addr, kind, "valueOf", receiver)
    {
        if let Some(primitive) = call_own_method_for_primitive(own_value_of, receiver) {
            return ExoticOwnOutcome::Primitive(primitive);
        }
    }
    ExoticOwnOutcome::NoPrimitive
}

/// Invoke `method` with `this = receiver` when it is a callable closure.
/// `None` means "not callable" — the value is an own property that shadows the
/// builtin but cannot be called (`re.toString = 5`).
pub(crate) unsafe fn call_own_method(method: f64, receiver: f64) -> Option<f64> {
    let bits = method.to_bits();
    if (bits & TAG_MASK) != POINTER_TAG
        || !crate::closure::is_closure_ptr((bits & POINTER_MASK) as usize)
    {
        return None;
    }
    // Rebind `this` to the receiver — an assigned closure may have baked a
    // different value into its reserved `this` slot (an inherited or bound
    // method), exactly as the method-dispatch tower does (#1982).
    let bound = crate::closure::clone_closure_rebind_this(bits, receiver);
    let this_scope = crate::gc::RuntimeHandleScope::new(); // #9445
    let prev_this = this_scope.root_nanbox_f64(crate::object::js_implicit_this_set(receiver));
    let ret = crate::closure::js_native_call_value(f64::from_bits(bound), std::ptr::null(), 0);
    crate::object::js_implicit_this_set(prev_this.get_nanbox_f64());
    Some(ret)
}

/// [`call_own_method`], but the result counts only when it is a primitive —
/// a non-callable value or an object result makes OrdinaryToPrimitive move on
/// to the next method name.
unsafe fn call_own_method_for_primitive(method: f64, receiver: f64) -> Option<f64> {
    call_own_method(method, receiver).filter(|ret| is_primitive_value(*ret))
}

/// `OrdinaryToPrimitive(O, "default"|"number")` step 1 for an exotic instance:
/// the `valueOf` step, restricted to the receiver's OWN property.
///
/// The "default" hint (`"" + re`) tries `valueOf` BEFORE `toString`, unlike the
/// "string" hint. `RegExp.prototype` has no `valueOf`, so only an OWN one can
/// yield a primitive here — the inherited `Object.prototype.valueOf` returns
/// `this`, an object, and OrdinaryToPrimitive then moves on to `toString`.
/// `None` therefore means "caller continues with the toString step".
pub(crate) unsafe fn exotic_own_value_of_primitive(
    addr: usize,
    kind: crate::object::exotic_expando::ExoticKind,
    receiver: f64,
) -> Option<f64> {
    let own =
        crate::object::exotic_expando::exotic_get_own_property(addr, kind, "valueOf", receiver)?;
    call_own_method_for_primitive(own, receiver)
}

/// `ToPrimitive(O, "number"|"default")`: consult a user
/// `[Symbol.toPrimitive]("number")` method first, then fall back to the
/// ordinary `valueOf`/`toString` order.
pub(crate) unsafe fn to_primitive_number(value: f64) -> OrdinaryToPrimitiveOutcome {
    if is_primitive_value(value) {
        return OrdinaryToPrimitiveOutcome::Primitive(value);
    }

    match custom_to_primitive(value, b"number") {
        CustomToPrimitiveOutcome::Absent => {}
        CustomToPrimitiveOutcome::Primitive(p) => return OrdinaryToPrimitiveOutcome::Primitive(p),
        CustomToPrimitiveOutcome::TypeError => return OrdinaryToPrimitiveOutcome::TypeError,
    }

    ordinary_to_primitive_number_for_add(value)
}

/// `OrdinaryToPrimitive(O, "number"|"default")` for addition. The method
/// order is `valueOf` then `toString`; Perry synthesizes the usual inherited
/// defaults for boxed primitives, arrays, and plain objects because those
/// built-ins are not stored as ordinary fields on every object.
pub(crate) unsafe fn ordinary_to_primitive_number_for_add(
    value: f64,
) -> OrdinaryToPrimitiveOutcome {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);

    // A TypedArray is an exotic object whose header is NOT an ObjectHeader;
    // the `valueOf`/`toString` field lookups below would bit-cast garbage
    // (heap-dependent: sometimes a fake "method" → spurious TypeError, e.g.
    // `"" + new Float64Array([1])`). Its ToPrimitive resolves to
    // %TypedArray%.prototype.toString (= `join(",")`); detect via the
    // registry (no deref) and join. `Symbol.toPrimitive` was already
    // consulted by the caller (`js_to_primitive`).
    if (value.to_bits() & 0xFFFF_0000_0000_0000) == POINTER_TAG {
        let addr = (value.to_bits() & POINTER_MASK) as usize;
        if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
            let joined = crate::typedarray::js_typed_array_join(
                addr as *const crate::typedarray::TypedArrayHeader,
                std::ptr::null(),
            );
            return OrdinaryToPrimitiveOutcome::Primitive(crate::value::js_nanbox_string(
                joined as i64,
            ));
        }
        // A callable closure is not an `ObjectHeader`; the `valueOf`/`toString`
        // field lookups below bit-cast it and crash on class-method closures
        // (see `function_to_primitive_for_add`). Resolve via the closure-aware
        // path: own/inherited `valueOf` if primitive, else the function source.
        if crate::closure::is_closure_ptr(addr) {
            return OrdinaryToPrimitiveOutcome::Primitive(function_to_primitive_for_add(value));
        }
    }

    match call_method_for_primitive(&scope, &value_handle, b"valueOf") {
        MethodOutcome::Primitive(p) => return OrdinaryToPrimitiveOutcome::Primitive(p),
        MethodOutcome::NonPrimitive => {}
        MethodOutcome::Absent => {
            if let Some((_class_id, payload)) =
                crate::builtins::boxed_primitive_payload(value_handle.get_nanbox_f64())
            {
                return OrdinaryToPrimitiveOutcome::Primitive(payload);
            }
        }
    }

    match call_method_for_primitive(&scope, &value_handle, b"toString") {
        MethodOutcome::Primitive(p) => OrdinaryToPrimitiveOutcome::Primitive(p),
        MethodOutcome::NonPrimitive => OrdinaryToPrimitiveOutcome::TypeError,
        MethodOutcome::Absent => {
            let value = value_handle.get_nanbox_f64();
            const TAG_TRUE_BITS: u64 = 0x7FFC_0000_0000_0004;
            if crate::array::js_array_is_array(value).to_bits() == TAG_TRUE_BITS {
                let arr_ptr =
                    JSValue::from_bits(value.to_bits()).as_pointer::<crate::array::ArrayHeader>();
                let comma = crate::string::js_string_from_bytes(b",".as_ptr(), 1);
                let joined = crate::array::js_array_join(arr_ptr, comma);
                return OrdinaryToPrimitiveOutcome::Primitive(crate::value::js_nanbox_string(
                    joined as i64,
                ));
            }
            OrdinaryToPrimitiveOutcome::DefaultString
        }
    }
}

unsafe fn call_method_for_primitive(
    scope: &crate::gc::RuntimeHandleScope,
    value_handle: &crate::gc::RuntimeHandle<'_>,
    method_name: &[u8],
) -> MethodOutcome {
    let recv = value_handle.get_nanbox_f64();
    let obj_ptr = (recv.to_bits() & POINTER_MASK) as *const crate::object::ObjectHeader;
    if obj_ptr.is_null() || (obj_ptr as usize) < 0x10000 {
        return MethodOutcome::Absent;
    }
    let key = crate::string::canonical_key(method_name);
    let key_handle = scope.root_string_ptr(key);
    // Presence is independent from the value returned by Get. In particular,
    // an inherited accessor may exist yet return undefined/null; that is a
    // present but non-callable method, so OrdinaryToPrimitive must continue to
    // the other candidate rather than synthesizing a boxed-primitive default.
    // `own_key_present(receiver)` cannot see that inherited descriptor.
    let key_value = key_handle.with_const_ptr::<crate::string::StringHeader, _>(|key_ptr| {
        f64::from_bits(
            crate::value::JSValue::string_ptr(key_ptr as *mut crate::string::StringHeader).bits(),
        )
    });
    let has_method_key =
        crate::object::js_object_has_property(value_handle.get_nanbox_f64(), key_value).to_bits()
            == crate::value::TAG_TRUE;
    // `HasProperty` can run a Proxy trap and collect. Refresh the receiver and
    // key from their handles before the subsequent ordinary Get.
    let recv = value_handle.get_nanbox_f64();
    let obj_ptr = (recv.to_bits() & POINTER_MASK) as *const crate::object::ObjectHeader;
    let method = key_handle.with_const_ptr::<crate::string::StringHeader, _>(|key_ptr| {
        crate::object::js_object_get_field_by_name(obj_ptr, key_ptr)
    });
    // Must be a callable closure value (POINTER_TAG + CLOSURE_MAGIC).
    let method_bits = method.bits();
    if (method_bits & 0xFFFF_0000_0000_0000) != POINTER_TAG {
        return if has_method_key || (!method.is_undefined() && !method.is_null()) {
            MethodOutcome::NonPrimitive
        } else {
            MethodOutcome::Absent
        };
    }
    let method_ptr = (method_bits & POINTER_MASK) as usize;
    if !crate::closure::is_closure_ptr(method_ptr) {
        return if has_method_key {
            MethodOutcome::NonPrimitive
        } else {
            MethodOutcome::Absent
        };
    }
    // Rebind `this` to the receiver: an INHERITED object-literal method
    // (`Object.create(proto)`) bakes its reserved `this` slot to the
    // prototype at construction time, and a bound-method closure carries the
    // wrong `this` until rebound. For OWN methods the slot already is the
    // receiver, so rebinding is a correct no-op. Mirrors #1982.
    let recv = value_handle.get_nanbox_f64();
    let bound = crate::closure::clone_closure_rebind_this(method_bits, recv);
    let prev_this = scope.root_nanbox_f64(crate::object::js_implicit_this_set(recv));
    let ret = crate::closure::js_native_call_value(f64::from_bits(bound), std::ptr::null(), 0);
    crate::object::js_implicit_this_set(prev_this.get_nanbox_f64());
    let ret_jsv = JSValue::from_bits(ret.to_bits());
    let is_primitive = ret_jsv.is_any_string()
        || ret_jsv.is_number()
        || ret_jsv.is_int32()
        || ret_jsv.is_bool()
        || ret_jsv.is_null()
        || ret_jsv.is_undefined()
        || ret_jsv.is_bigint()
        || crate::symbol::js_is_symbol(ret) != 0;
    if is_primitive {
        MethodOutcome::Primitive(ret)
    } else {
        MethodOutcome::NonPrimitive
    }
}

unsafe fn call_function_method(
    scope: &crate::gc::RuntimeHandleScope,
    value_handle: &crate::gc::RuntimeHandle<'_>,
    method_name: &[u8],
) -> FunctionMethodOutcome {
    let recv = value_handle.get_nanbox_f64();
    let recv_jsv = JSValue::from_bits(recv.to_bits());
    if !recv_jsv.is_pointer() {
        return FunctionMethodOutcome::Absent;
    }
    let closure_ptr = recv_jsv.as_pointer::<u8>() as usize;
    if closure_ptr == 0 || !crate::closure::is_closure_ptr(closure_ptr) {
        return FunctionMethodOutcome::Absent;
    }

    let key = crate::string::canonical_key(method_name);
    let key_handle = scope.root_string_ptr(key);
    let key_ptr = key_handle.get_raw_const_ptr::<crate::string::StringHeader>();
    let method = function_method_value(closure_ptr, key_ptr, method_name);
    let method_bits = method.to_bits();
    if (method_bits & TAG_MASK) != POINTER_TAG {
        return if JSValue::from_bits(method_bits).is_undefined()
            || JSValue::from_bits(method_bits).is_null()
        {
            FunctionMethodOutcome::Absent
        } else {
            FunctionMethodOutcome::NonCallable
        };
    }
    let method_ptr = (method_bits & POINTER_MASK) as usize;
    if !crate::closure::is_closure_ptr(method_ptr) {
        return FunctionMethodOutcome::NonCallable;
    }

    let method_handle = scope.root_nanbox_f64(method);
    let bound = crate::closure::clone_closure_rebind_this(method_handle.get_nanbox_u64(), recv);
    let prev_this = scope.root_nanbox_f64(crate::object::js_implicit_this_set(recv));
    let ret = crate::closure::js_native_call_value(f64::from_bits(bound), std::ptr::null(), 0);
    crate::object::js_implicit_this_set(prev_this.get_nanbox_f64());

    FunctionMethodOutcome::Value(ret)
}

/// `OrdinaryToPrimitive` for a callable closure under the "number"/"default"
/// hint (method order `valueOf` then `toString`) — used by the `+` operator
/// and numeric coercion.
///
/// A function/closure is NOT an `ObjectHeader`: the ordinary-object
/// `valueOf`/`toString` field lookups (`call_method_for_primitive` →
/// `js_object_get_field_by_name`) bit-cast it as one and, for a class-method
/// closure, read a bogus `valueOf` slot that they then *call* → EXC_BAD_ACCESS
/// (`"" + C.prototype.method`). Resolve via the closure-aware lookup instead.
///
/// Faithful to OrdinaryToPrimitive: try `valueOf` then `toString`; the first
/// callable returning a *primitive* wins and is returned **as-is** (so
/// `f.valueOf = () => 42; 1 + f` is `43` and `f.toString = () => 42; 1 + f` is
/// `43`, not the stringified `"42"`). A callable `toString` returning a
/// non-primitive object exhausts both steps → `TypeError` (Node: `1 + g` where
/// `g.toString = () => ({})` throws). For a plain function the inherited
/// `valueOf` returns the function itself (non-primitive) and `toString`
/// resolves to `Function.prototype.toString` → the source / native form. The
/// trailing `js_jsvalue_to_string` is only a guard for the (shouldn't-happen)
/// case where no callable `toString` resolves at all.
pub(crate) unsafe fn function_to_primitive_for_add(value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);
    if let FunctionMethodOutcome::Value(ret) =
        call_function_method(&scope, &value_handle, b"valueOf")
    {
        if is_primitive_value(ret) {
            return ret;
        }
    }
    if let FunctionMethodOutcome::Value(ret) =
        call_function_method(&scope, &value_handle, b"toString")
    {
        if is_primitive_value(ret) {
            return ret;
        }
        // Both `valueOf` and a callable `toString` yielded non-primitives:
        // OrdinaryToPrimitive throws `TypeError: Cannot convert object to
        // primitive value`.
        throw_cannot_convert_to_primitive();
    }
    let s = js_jsvalue_to_string(value_handle.get_nanbox_f64());
    crate::value::js_nanbox_string(s as i64)
}

unsafe fn function_method_value(
    closure_ptr: usize,
    key_ptr: *const crate::string::StringHeader,
    method_name: &[u8],
) -> f64 {
    let Ok(name) = std::str::from_utf8(method_name) else {
        return f64::from_bits(TAG_UNDEFINED);
    };

    if crate::closure::closure_has_own_dynamic_prop(closure_ptr, name) {
        return crate::closure::closure_get_dynamic_prop(closure_ptr, name);
    }

    let explicit_proto_value = crate::closure::closure_get_dynamic_prop(closure_ptr, name);
    let explicit_proto_jsv = JSValue::from_bits(explicit_proto_value.to_bits());
    if !explicit_proto_jsv.is_undefined() && !explicit_proto_jsv.is_null() {
        return explicit_proto_value;
    }
    if crate::closure::closure_static_prototype(closure_ptr).is_some() {
        return explicit_proto_value;
    }

    let function_proto = crate::object::builtin_prototype_value("Function");
    let proto_jsv = JSValue::from_bits(function_proto.to_bits());
    if !proto_jsv.is_pointer() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let proto_ptr = proto_jsv.as_pointer::<crate::object::ObjectHeader>();
    if proto_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let value = crate::object::js_object_get_field_by_name(proto_ptr, key_ptr);
    f64::from_bits(value.bits())
}
