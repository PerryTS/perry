//! NaN-boxed value to-string conversion helpers.

use super::*;
use std::cell::Cell;
use std::sync::atomic::Ordering;

thread_local! {
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
fn throw_cannot_convert_to_primitive() -> ! {
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
    for name in order {
        let recv = value_handle.get_nanbox_f64();
        let key_ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let key = f64::from_bits(crate::value::js_nanbox_string(key_ptr as i64).to_bits());
        // `Get(O, name)` — fires accessor getters and walks the prototype chain,
        // exactly like the spec's abstract `Get` (so `{ get valueOf() {…} }` is
        // observed and a getter throw propagates).
        let method = crate::proxy::js_reflect_get(recv, key, recv);
        if !crate::collection_iter::is_callable(method) {
            // Non-callable (or absent) — skip to the next name.
            continue;
        }
        let method_handle = scope.root_nanbox_f64(method);
        let recv = value_handle.get_nanbox_f64();
        let prev_this = crate::object::js_implicit_this_set(recv);
        let result = crate::closure::js_native_call_value(
            method_handle.get_nanbox_f64(),
            std::ptr::null(),
            0,
        );
        crate::object::js_implicit_this_set(prev_this);
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
enum FunctionToStringOutcome {
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
unsafe fn function_to_string_via_prototype(value: f64) -> FunctionToStringOutcome {
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

enum CustomToPrimitiveOutcome {
    Absent,
    Primitive(f64),
    TypeError,
}

fn is_primitive_value(value: f64) -> bool {
    let jsval = JSValue::from_bits(value.to_bits());
    jsval.is_any_string()
        || jsval.is_number()
        || jsval.is_int32()
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
    let prev_this = crate::object::js_implicit_this_set(receiver);
    let ret = crate::closure::js_native_call_value(f64::from_bits(bound), std::ptr::null(), 0);
    crate::object::js_implicit_this_set(prev_this);
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

    match custom_to_primitive_number(value) {
        CustomToPrimitiveOutcome::Absent => {}
        CustomToPrimitiveOutcome::Primitive(p) => return OrdinaryToPrimitiveOutcome::Primitive(p),
        CustomToPrimitiveOutcome::TypeError => return OrdinaryToPrimitiveOutcome::TypeError,
    }

    ordinary_to_primitive_number_for_add(value)
}

unsafe fn custom_to_primitive_number(value: f64) -> CustomToPrimitiveOutcome {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);
    let to_primitive = crate::symbol::well_known_symbol("toPrimitive");
    let sym_value = f64::from_bits(POINTER_TAG | (to_primitive as u64 & POINTER_MASK));
    let method =
        crate::symbol::js_object_get_symbol_property(value_handle.get_nanbox_f64(), sym_value);
    let method_jsv = JSValue::from_bits(method.to_bits());
    if method_jsv.is_undefined() || method_jsv.is_null() {
        return CustomToPrimitiveOutcome::Absent;
    }

    let method_bits = method.to_bits();
    if (method_bits & 0xFFFF_0000_0000_0000) != POINTER_TAG {
        return CustomToPrimitiveOutcome::TypeError;
    }
    let method_ptr = (method_bits & POINTER_MASK) as usize;
    if !crate::closure::is_closure_ptr(method_ptr) {
        return CustomToPrimitiveOutcome::TypeError;
    }

    let method_handle = scope.root_nanbox_f64(method);
    let hint_ptr = crate::string::js_string_from_bytes(b"number".as_ptr(), 6);
    let hint_handle = scope.root_string_ptr(hint_ptr);
    let hint = f64::from_bits(
        STRING_TAG
            | (hint_handle.get_raw_const_ptr::<crate::string::StringHeader>() as u64
                & POINTER_MASK),
    );
    let receiver = value_handle.get_nanbox_f64();
    let prev_this = crate::object::js_implicit_this_set(receiver);
    let result = crate::closure::js_native_call_value(method_handle.get_nanbox_f64(), &hint, 1);
    crate::object::js_implicit_this_set(prev_this);

    if is_primitive_value(result) {
        CustomToPrimitiveOutcome::Primitive(result)
    } else {
        CustomToPrimitiveOutcome::TypeError
    }
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

/// Coerce a NaN-boxed value to a `*const StringHeader` suitable for FFI calls
/// that expect string/JSON input.
#[no_mangle]
pub extern "C" fn js_value_to_str_ptr_for_ffi(value: f64) -> i64 {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_string() {
        return jsval.as_string_ptr() as i64;
    }
    if jsval.is_short_string() {
        return crate::string::js_string_materialize_to_heap(value) as i64;
    }
    unsafe { crate::json::js_json_stringify(value, 0) as i64 }
}

/// Resolve `obj[method_name]` (own + prototype chain) and, if it is a
/// callable closure, invoke it with `this = obj` (no args). Returns whether
/// the result was a primitive, a non-primitive, or whether the method was
/// absent / non-callable.
/// `Array.prototype.toString` override check for `String(arr)` / `` `${arr}` ``
/// / `"" + arr` (test262 `S15.5.1.1_A1_T8`). Arrays are `GC_TYPE_ARRAY`, not
/// `ObjectHeader`s, so unlike ordinary objects their `toString` can't be
/// looked up as an "own/inherited field" on the array value itself — the
/// hardcoded `Array.prototype.join(",")` fallback normally used for arrays
/// must instead be skipped when `Array.prototype.toString` has been
/// reassigned away from its installed default (a noop thunk kept only for
/// `typeof`/`.name` introspection, see `populate_builtin_prototype_methods`).
/// Outcome of consulting `Array.prototype.toString` for the string-hint
/// `ToPrimitive`. `UseDefaultJoin` covers both "still the installed noop
/// default" and any shape we don't have a callable method for (e.g. the
/// property was overwritten with a non-callable value) — those fall back to
/// the ordinary `join(",")` behavior. A callable override that's actually
/// invoked must otherwise follow `OrdinaryToPrimitive`: a primitive result
/// wins, a non-primitive result exhausts `ToPrimitive` (no separate
/// `valueOf` override path exists for arrays here) and throws, rather than
/// silently falling back to `join`.
enum ArrayToStringOutcome {
    UseDefaultJoin,
    Primitive(f64),
    TypeError,
}

unsafe fn array_prototype_to_string_override(value: f64) -> ArrayToStringOutcome {
    let proto = crate::object::builtin_prototype_value("Array");
    let proto_bits = proto.to_bits();
    if (proto_bits & 0xFFFF_0000_0000_0000) != POINTER_TAG {
        return ArrayToStringOutcome::UseDefaultJoin;
    }
    let proto_ptr = (proto_bits & POINTER_MASK) as *mut crate::object::ObjectHeader;
    if proto_ptr.is_null() {
        return ArrayToStringOutcome::UseDefaultJoin;
    }
    let key = crate::string::js_string_from_bytes(b"toString".as_ptr(), 8);
    let method = crate::object::js_object_get_field_by_name(proto_ptr, key);
    let method_bits = method.bits();
    if (method_bits & 0xFFFF_0000_0000_0000) != POINTER_TAG {
        return ArrayToStringOutcome::UseDefaultJoin;
    }
    let method_ptr = (method_bits & POINTER_MASK) as usize;
    if !crate::closure::is_closure_ptr(method_ptr) {
        return ArrayToStringOutcome::UseDefaultJoin;
    }
    let closure = method_ptr as *const crate::closure::ClosureHeader;
    if (*closure).func_ptr == crate::object::global_this_builtin_noop_thunk as *const u8 {
        return ArrayToStringOutcome::UseDefaultJoin;
    }
    let bound = crate::closure::clone_closure_rebind_this(method_bits, value);
    let prev_this = crate::object::js_implicit_this_set(value);
    let ret = crate::closure::js_native_call_value(f64::from_bits(bound), std::ptr::null(), 0);
    crate::object::js_implicit_this_set(prev_this);
    if is_primitive_value(ret) {
        ArrayToStringOutcome::Primitive(ret)
    } else {
        ArrayToStringOutcome::TypeError
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
    let key = crate::string::js_string_from_bytes(method_name.as_ptr(), method_name.len() as u32);
    let key_handle = scope.root_string_ptr(key);
    let key_ptr = key_handle.get_raw_const_ptr::<crate::string::StringHeader>();
    let has_own_method_key = crate::object::own_key_present(obj_ptr as *mut _, key_ptr);
    let method = crate::object::js_object_get_field_by_name(obj_ptr, key_ptr);
    // Must be a callable closure value (POINTER_TAG + CLOSURE_MAGIC).
    let method_bits = method.bits();
    if (method_bits & 0xFFFF_0000_0000_0000) != POINTER_TAG {
        return if has_own_method_key || (!method.is_undefined() && !method.is_null()) {
            MethodOutcome::NonPrimitive
        } else {
            MethodOutcome::Absent
        };
    }
    let method_ptr = (method_bits & POINTER_MASK) as usize;
    if !crate::closure::is_closure_ptr(method_ptr) {
        return if has_own_method_key {
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
    let prev_this = crate::object::js_implicit_this_set(recv);
    let ret = crate::closure::js_native_call_value(f64::from_bits(bound), std::ptr::null(), 0);
    crate::object::js_implicit_this_set(prev_this);
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

    let key = crate::string::js_string_from_bytes(method_name.as_ptr(), method_name.len() as u32);
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
    let prev_this = crate::object::js_implicit_this_set(recv);
    let ret = crate::closure::js_native_call_value(f64::from_bits(bound), std::ptr::null(), 0);
    crate::object::js_implicit_this_set(prev_this);

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

/// Read an object's own/inherited property by name and coerce it to an owned
/// `String`, or `None` when the property is absent (undefined/null). Used by
/// the Error-subclass `toString` path (#2135).
unsafe fn object_field_to_owned_string(
    obj: *const crate::object::ObjectHeader,
    key: &[u8],
) -> Option<String> {
    let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    let v = crate::object::js_object_get_field_by_name(obj, key_ptr);
    if v.is_undefined() || v.is_null() {
        return None;
    }
    let s_ptr = js_jsvalue_to_string(f64::from_bits(v.bits()));
    if s_ptr.is_null() {
        return None;
    }
    let len = (*s_ptr).byte_len as usize;
    let data = (s_ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
    Some(String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned())
}

/// Convert a NaN-boxed f64 value to a string pointer.
/// Handles all value types: strings (extract pointer), numbers (convert), JS handles, etc.
#[no_mangle]
pub extern "C" fn js_jsvalue_to_string(value: f64) -> *mut crate::string::StringHeader {
    // Check for JS handle first - these come from the JS runtime (e.g., process.env values)
    if is_js_handle(value) {
        let func_ptr = JS_HANDLE_TO_STRING.load(Ordering::SeqCst);
        if !func_ptr.is_null() {
            let func: JsHandleToStringFn = unsafe { std::mem::transmute(func_ptr) };
            return func(value);
        }
        // Fallback if no handler registered
        return crate::string::js_string_from_bytes(b"[JS Handle]".as_ptr(), 11);
    }

    let jsval = JSValue::from_bits(value.to_bits());

    if jsval.is_string() {
        // Already a heap string — return the pointer directly.
        jsval.as_string_ptr() as *mut crate::string::StringHeader
    } else if jsval.is_short_string() {
        // Inline SSO — materialize into a heap StringHeader so the
        // caller gets a uniform `*mut StringHeader`. This defeats
        // the SSO benefit for this particular conversion, but it's
        // a correctness-preserving compatibility shim for the many
        // call sites that currently expect a heap pointer.
        crate::string::js_string_materialize_to_heap(value)
    } else if jsval.is_undefined() {
        crate::string::js_string_from_bytes(b"undefined".as_ptr(), 9)
    } else if jsval.is_null() {
        crate::string::js_string_from_bytes(b"null".as_ptr(), 4)
    } else if jsval.is_bool() {
        if jsval.as_bool() {
            crate::string::js_string_from_bytes(b"true".as_ptr(), 4)
        } else {
            crate::string::js_string_from_bytes(b"false".as_ptr(), 5)
        }
    } else if jsval.is_int32() {
        // A registered class id shares the INT32 encoding (`Expr::ClassRef`)
        // — `String(C)` / `"" + C` must produce function source, not the
        // numeric id. Perry keeps no class source, so the NativeFunction
        // form with the class name.
        let n = jsval.as_int32();
        let cid = (value.to_bits() & 0xFFFF_FFFF) as u32;
        if crate::object::is_class_id_registered(cid) {
            let name = crate::object::class_name_for_id(cid).unwrap_or_default();
            let s = format!("function {name}() {{ [native code] }}");
            return crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
        }
        let s = n.to_string();
        crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
    } else if jsval.is_bigint() {
        // BigInt - convert to decimal string
        let ptr = jsval.as_bigint_ptr();
        crate::bigint::js_bigint_to_string(ptr)
    } else if jsval.is_pointer() {
        // Pointer: could be an array, object, or other heap type. Arrays
        // stringify via `Array.prototype.join(",")` per JS semantics; other
        // objects fall back to "[object Object]".
        let ptr: *const u8 = jsval.as_pointer();
        // Proxy ids can be SMALLER than the 0x10000 heap floor — check the
        // registry (a by-value lookup, no deref) before the gate.
        if crate::proxy::js_proxy_is_proxy(value) != 0 {
            if crate::proxy::proxy_wraps_callable(value) {
                let s = "function () { [native code] }";
                return crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
            }
            let target = crate::proxy::js_proxy_target(value);
            if target.to_bits() != value.to_bits() {
                return js_jsvalue_to_string(target);
            }
            return crate::string::js_string_from_bytes(b"[object Object]".as_ptr(), 15);
        }
        // Only a *real heap pointer* (above the whole synthetic handle band)
        // may enter the object-deref block below. The prior `>= 0x10000` floor
        // let fetch/Blob/socket/stream handle ids (0x40000+) through, so
        // `String(new Blob())` reached the ToPrimitive/GC-header derefs and
        // segfaulted (#6240/#6241). Proxies (a handle-band id) are already
        // resolved above; any other handle falls through to "[object Object]".
        if !ptr.is_null() && crate::value::addr_class::is_above_handle_band(ptr as usize) {
            // A Proxy is a small registered id, not a heap object — the GC-header
            // probes / ToPrimitive dispatch below would deref the fake pointer
            // and segfault (e.g. `String(proxy)`). Default `ToString` has no
            // toString/valueOf trap of its own, so resolve to the target and
            // stringify that ("[object Object]" for an ordinary object target),
            // which matches Node for the trap-less case. (Proxy crash cluster.)
            if crate::proxy::js_proxy_is_proxy(value) != 0 {
                // A callable-target proxy's default ToString runs
                // Function.prototype.toString with the PROXY as receiver —
                // never introspectable, so the NativeFunction form (matches
                // Node: `String(new Proxy(fn, {}))`). Non-callable targets
                // resolve through the target.
                if crate::proxy::proxy_wraps_callable(value) {
                    let s = "function () { [native code] }";
                    return crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                }
                let target = crate::proxy::js_proxy_target(value);
                if target.to_bits() != value.to_bits() {
                    return js_jsvalue_to_string(target);
                }
                return crate::string::js_string_from_bytes(b"[object Object]".as_ptr(), 15);
            }
            // Symbols: detect via the side-table before any GC header read.
            if crate::symbol::is_registered_symbol(ptr as usize) {
                return unsafe {
                    crate::symbol::js_symbol_to_string(value) as *mut crate::string::StringHeader
                };
            }
            // #4101: a function/closure stringifies to its source text via
            // Function.prototype.toString — covers `String(fn)` and
            // `` `${fn}` `` rather than "[object Object]".
            if crate::closure::is_closure_ptr(ptr as usize) {
                match unsafe { function_to_string_via_prototype(value) } {
                    FunctionToStringOutcome::Primitive(result) => return result,
                    FunctionToStringOutcome::TypeError => throw_cannot_convert_to_primitive(),
                    FunctionToStringOutcome::NoCustomMethod => {}
                }
                let func_ptr =
                    unsafe { (*(ptr as *const crate::closure::ClosureHeader)).func_ptr as usize };
                let s = crate::builtins::function_source_for_func_ptr(func_ptr);
                return crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
            }
            // Consult `[Symbol.toPrimitive]("string")` if the object has a
            // custom toPrimitive method registered in the symbol side-table.
            // A changed result means the user-defined method produced a
            // string-hint primitive — recurse so strings pass through as-is
            // and numbers get js_number_to_string.
            let primitive = unsafe { crate::symbol::js_to_primitive(value, 2) };
            if primitive.to_bits() != value.to_bits() {
                return js_jsvalue_to_string(primitive);
            }
            // Buffers: BufferHeader has no GC header, so we must detect via
            // BUFFER_REGISTRY before any GC-header probe (which would read
            // garbage one word before the buffer). `Buffer.toString()` with
            // no arg defaults to UTF-8 — Node prints the raw bytes.
            if crate::buffer::is_registered_buffer(ptr as usize) {
                return crate::buffer::js_buffer_to_string(
                    ptr as *const crate::buffer::BufferHeader,
                    0,
                );
            }
            // A TypedArray stringifies via %TypedArray%.prototype.toString
            // (= `Array.prototype.join(",")`), not "[object Object]". Detected
            // via the registry (a by-value lookup, no deref) before any
            // GC-header probe — a TypedArrayHeader is NOT an ObjectHeader, so
            // the ordinary toString/valueOf field path below would bit-cast
            // garbage. Covers `String(ta)`, `` `${ta}` ``, and the `+` add
            // fallback. (`Symbol.toPrimitive` overrides were already consulted
            // above via `js_to_primitive`.)
            if crate::typedarray::lookup_typed_array_kind(ptr as usize).is_some() {
                return crate::typedarray::js_typed_array_join(
                    ptr as *const crate::typedarray::TypedArrayHeader,
                    std::ptr::null(),
                );
            }
            // #2089: a Date is a NaN-boxed `DateCell` pointer. `String(date)`,
            // `` `${date}` ``, and `date.toString()` produce the full local
            // date string (or "Invalid Date"), not "[object Object]". Detect
            // before GC-header object dispatch (the 8-byte cell is smaller
            // than an ObjectHeader), after non-GC native buffer handles.
            if crate::date::is_date_cell_addr(ptr as usize) {
                // #6370: an own `toString` (data or accessor) shadows
                // `Date.prototype.toString` on every coercion site, exactly as
                // it already does for the explicit `date.toString()` call.
                match unsafe {
                    exotic_own_to_string(
                        ptr as usize,
                        crate::object::exotic_expando::ExoticKind::Date,
                        value,
                    )
                } {
                    ExoticOwnToString::Primitive(primitive) => {
                        return js_jsvalue_to_string(primitive)
                    }
                    ExoticOwnToString::UseBuiltin => {}
                }
                return crate::date::js_date_to_string(value);
            }
            // Temporal (#4686): `String(temporal)`, `` `${temporal}` ``, and
            // `temporal.toString()` produce the value's canonical ISO-8601 /
            // IXDTF string, not "[object Object]". Detected here for the same
            // reason as Date — the cell is smaller than an ObjectHeader.
            #[cfg(feature = "temporal")]
            if crate::temporal::is_temporal_cell_addr(ptr as usize) {
                if let Some(s) = crate::temporal::temporal_iso_string(value) {
                    return crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                }
            }
            // A RegExp stringifies to `/source/flags` (RegExp.prototype.toString),
            // not "[object Object]" — covers `String(re)` and `` `${re}` ``.
            if crate::regex::is_regex_pointer(ptr) {
                // …unless an own `toString` shadows the prototype method
                // (#6370). This is the SAME lookup the `re.toString()` method
                // fold performs (#6358); doing it here too is what makes the
                // two agree, and it reaches every implicit ToString —
                // `String(re)`, `` `${re}` ``, `[re].join("")`,
                // `"".concat(re)`, `[re].toString()`.
                match unsafe {
                    exotic_own_to_string(
                        ptr as usize,
                        crate::object::exotic_expando::ExoticKind::RegExp,
                        value,
                    )
                } {
                    ExoticOwnToString::Primitive(primitive) => {
                        return js_jsvalue_to_string(primitive)
                    }
                    ExoticOwnToString::UseBuiltin => {}
                }
                return crate::regex::js_regexp_to_string(ptr as *const crate::regex::RegExpHeader);
            }
            unsafe {
                let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
                    // A reassigned `Array.prototype.toString` must run instead
                    // of the hardcoded join (test262 S15.5.1.1_A1_T8).
                    match array_prototype_to_string_override(value) {
                        ArrayToStringOutcome::Primitive(primitive) => {
                            return js_jsvalue_to_string(primitive)
                        }
                        ArrayToStringOutcome::TypeError => throw_cannot_convert_to_primitive(),
                        ArrayToStringOutcome::UseDefaultJoin => {}
                    }
                    // Use js_array_join with a "," separator to match Array.prototype.toString.
                    let sep = crate::string::js_string_from_bytes(b",".as_ptr(), 1);
                    return crate::array::js_array_join(
                        ptr as *const crate::array::ArrayHeader,
                        sep as *const crate::string::StringHeader,
                    );
                }
                // #1653: a boxed server-rendered JSX node stringifies to its
                // stored HTML (field 0), so `String(<div/>)` / `c.html(<X/>)`
                // emit real markup instead of "[object Object]".
                let obj = ptr as *const crate::object::ObjectHeader;
                if (*obj).class_id == crate::jsx::JSX_NODE_CLASS_ID {
                    let html = crate::object::js_object_get_field(obj, 0);
                    return js_jsvalue_to_string(f64::from_bits(html.bits()));
                }
            }
            // WHATWG `URL` / `URLSearchParams` have native `toString`s
            // (`href` / the query string) that aren't discoverable as object
            // fields. They must be checked BEFORE OrdinaryToPrimitive, which
            // would otherwise find the inherited `Object.prototype.toString`
            // and return "[object Object]" — so `String(url)`, `` `${url}` ``
            // and `"" + url` diverged from explicit `url.toString()`. Detected
            // before the GC-header object dispatch like the other native types.
            //
            // Normalize the raw heap pointer to a `POINTER_TAG` value first:
            // the `+`/template concat path delivers the operand as a raw
            // pointer (upper-16 == 0), and `js_url_href_if_url`'s
            // `object_from_f64` only recognizes `POINTER_TAG`. `String(url)`
            // already arrives tagged. Skip the probe for small-handle values
            // (sockets / timers / widget handles): those are registry ids, not
            // heap `ObjectHeader`s, so the shape check would dereference
            // unmapped memory.
            if !crate::value::addr_class::is_handle_band(ptr as usize) {
                let boxed = f64::from_bits(POINTER_TAG | ((ptr as u64) & POINTER_MASK));
                let url_href = crate::url::url_class::js_url_href_if_url(boxed);
                if url_href.to_bits() != crate::value::TAG_UNDEFINED {
                    return js_jsvalue_to_string(url_href);
                }
                if crate::url::try_read_as_search_params(ptr as *mut crate::object::ObjectHeader)
                    .is_some()
                {
                    return crate::url::search_params::js_url_search_params_to_string(
                        ptr as *mut crate::object::ObjectHeader,
                    );
                }
            }
            // OrdinaryToPrimitive(obj, "string"): the object has no
            // `[Symbol.toPrimitive]` (checked above) and is not an
            // array/buffer/JSX/symbol with its own coercion. Per spec, call
            // the object's own/inherited `toString` (then `valueOf`) with
            // `this = obj`. A custom `toString` on a plain object, an
            // `Object.create(proto)` result, or a class instance resolves
            // here; a primitive result is re-coerced (strings pass through,
            // numbers via `js_number_to_string`). A plain `{}` (no callable
            // toString/valueOf) returns None and falls through to the default
            // `"[object Object]"`. (Built-in Error/Date prototype `toString`s
            // are not discoverable as object fields in Perry's model, so they
            // still hit the fallback — a separate, pre-existing gap.)
            if let Some(primitive) = unsafe { ordinary_to_primitive_string(value) } {
                if primitive.to_bits() != value.to_bits() {
                    return js_jsvalue_to_string(primitive);
                }
            }
            // #2135: a built-in Error with no user-overridden `toString`
            // resolves here. `Error.prototype.toString` is `name`/`message`/
            // `"name: message"`, not Object.prototype's `"[object Object]"`.
            unsafe {
                let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                if (*gc_header).obj_type == crate::gc::GC_TYPE_ERROR {
                    return crate::error::js_error_to_string(ptr as *mut crate::error::ErrorHeader);
                }
                // #2135: an Error *subclass* (`class X extends Error`) is a
                // plain class instance, not a `GC_TYPE_ERROR` ErrorHeader, so
                // it reaches here. `Error.prototype.toString` reads the `name`
                // and `message` properties (own or inherited) — resolve them
                // and format `name`/`message`/`"name: message"` rather than
                // falling through to `"[object Object]"`. `extends_builtin_error`
                // walks the class-id chain (the same check that backs
                // `instanceof Error`); its registry lookup never dereferences
                // `class_id`, so it is safe even for non-class pointers.
                let obj = ptr as *const crate::object::ObjectHeader;
                let class_id = (*obj).class_id;
                if class_id != 0 && crate::object::extends_builtin_error(class_id) {
                    let name = object_field_to_owned_string(obj, b"name")
                        .unwrap_or_else(|| "Error".to_string());
                    let message = object_field_to_owned_string(obj, b"message").unwrap_or_default();
                    let result = if name.is_empty() {
                        message
                    } else if message.is_empty() {
                        name
                    } else {
                        format!("{name}: {message}")
                    };
                    return crate::string::js_string_from_bytes(
                        result.as_ptr(),
                        result.len() as u32,
                    );
                }
            }
        }
        // An object with no `toString` override inherits
        // `Object.prototype.toString`, which brands Map / Set / WeakMap /
        // WeakSet / Promise (and any `Symbol.toStringTag`) as "[object Map]"
        // etc. — not the bare "[object Object]". `String(new Map())` reached
        // here after finding no override, so reuse that brand detection instead
        // of hardcoding the generic tag. Ordinary objects still come back
        // "[object Object]".
        let branded = unsafe { crate::object::js_object_to_string(value) };
        (branded.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut crate::StringHeader
    } else {
        // Regular number - use js_number_to_string
        crate::string::js_number_to_string(value)
    }
}

/// ECMAScript `ToNumber` for a radix argument value (NaN-boxed f64). Numbers
/// pass through; strings are parsed with `Number()` semantics (trim + full
/// numeric parse, NOT `parseFloat` prefix parse — `"16px"` → NaN); booleans →
/// 0/1; null → 0; undefined → NaN (signals "use the default radix 10").
/// Returns NaN for anything that does not coerce to a finite number.
unsafe fn radix_arg_to_number(radix_value: f64) -> f64 {
    let jsval = JSValue::from_bits(radix_value.to_bits());
    // ToInteger(radix) → ToNumber(radix): a Symbol or BigInt radix throws a
    // TypeError (must precede the NaN→RangeError path). e.g.
    // `(0n).toString(Symbol())` / `(123).toString(2n)` → TypeError, not RangeError.
    if jsval.is_bigint() {
        crate::collection_iter::throw_type_error("Cannot convert a BigInt value to a number");
    }
    if crate::symbol::js_is_symbol(radix_value) != 0 {
        crate::collection_iter::throw_type_error("Cannot convert a Symbol value to a number");
    }
    if jsval.is_int32() {
        jsval.as_int32() as f64
    } else if jsval.is_bool() {
        if jsval.as_bool() {
            1.0
        } else {
            0.0
        }
    } else if jsval.is_null() {
        0.0
    } else if jsval.is_undefined() {
        // Signals the "no radix supplied" / default path.
        f64::NAN
    } else if jsval.is_any_string() {
        let s_ptr = js_jsvalue_to_string(radix_value);
        if s_ptr.is_null() {
            return f64::NAN;
        }
        let len = (*s_ptr).byte_len as usize;
        let data = (s_ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        let trimmed = std::str::from_utf8(bytes).unwrap_or("").trim();
        if trimmed.is_empty() {
            // `Number("")` === 0
            0.0
        } else {
            trimmed.parse::<f64>().unwrap_or(f64::NAN)
        }
    } else if jsval.is_number() {
        radix_value
    } else {
        // An object radix runs ToNumber → OrdinaryToPrimitive(number), i.e. its
        // `valueOf`/`toString`. An abrupt completion there must propagate rather
        // than be swallowed into a `RangeError` (test262 Number/prototype/
        // toString/numeric-literal-tostring-radix-poisoned:
        // `0..toString({valueOf(){throw}})` must throw the poison, not a
        // RangeError). `js_number_coerce` performs that coercion (and yields NaN
        // for a non-coercible object, still landing on the RangeError path).
        crate::builtins::js_number_coerce(radix_value)
    }
}

/// Coerce + validate a radix argument per ECMAScript `Number.prototype.toString`
/// (and `BigInt.prototype.toString`). Returns the validated integer radix in
/// `2..=36`, or `None` when the argument was `undefined` (caller uses the
/// default radix 10). Throws (diverges via `js_throw`) with a `RangeError` for
/// any other out-of-range / non-coercible value, matching Node.
pub(crate) unsafe fn coerce_validate_radix(radix_value: f64) -> Option<i32> {
    let n = radix_arg_to_number(radix_value);
    if n.is_nan() {
        // `undefined` → default radix (None); everything else NaN → RangeError.
        if JSValue::from_bits(radix_value.to_bits()).is_undefined() {
            return None;
        }
        throw_radix_range_error();
    }
    // ToInteger: truncate toward zero.
    let r = n.trunc();
    if !(2.0..=36.0).contains(&r) {
        throw_radix_range_error();
    }
    Some(r as i32)
}

/// `value.toString()` as an explicit METHOD CALL (#3146). Unlike the abstract
/// `js_jsvalue_to_string` (used for `String(x)`, template literals, and `+`
/// coercion, where a nullish operand stringifies to "undefined"/"null"), a
/// member call `u.toString()` on `undefined`/`null` is a property read on a
/// nullish base and must throw a `TypeError`. For every non-nullish value this
/// delegates to `js_jsvalue_to_string`, so ordinary `.toString()` behaviour is
/// unchanged.
#[no_mangle]
pub extern "C" fn js_jsvalue_to_string_method(value: f64) -> *mut crate::string::StringHeader {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_undefined() || jsval.is_null() {
        let is_null = if jsval.is_null() { 1u32 } else { 0u32 };
        let prop = b"toString";
        crate::error::js_throw_type_error_property_access(is_null, prop.as_ptr(), prop.len());
    }
    if jsval.is_pointer() {
        let handle = jsval.as_pointer::<u8>() as usize;
        if crate::value::addr_class::is_small_handle(handle) {
            if let Some(dispatch) = crate::object::handle_method_dispatch() {
                let result = unsafe {
                    dispatch(handle as i64, b"toString".as_ptr(), 8, std::ptr::null(), 0)
                };
                let result_jsval = JSValue::from_bits(result.to_bits());
                if result_jsval.is_string() {
                    return result_jsval.as_string_ptr() as *mut crate::string::StringHeader;
                }
                if result_jsval.is_short_string() {
                    return crate::string::js_string_materialize_to_heap(result);
                }
            }
        }
    }
    // An OWN `toString` shadows the built-in conversion. Codegen's "universal
    // `.toString()`" fold (lower_call/property_get/number_string.rs) rewrites
    // EVERY `x.toString()` into a direct call to this function whenever the
    // receiver isn't a user class that declares `toString` — bypassing the
    // runtime method-dispatch arms entirely. So the own-property check that
    // `dispatch_primitive` applies to `re.exec` / `re.test` has to be repeated
    // at this fold target, or an assigned `re.toString` is silently ignored and
    // the regex renders as its `/source/flags` literal instead:
    //
    //     __re.toString = Object.prototype.toString;
    //     __re.toString()   // must be "[object RegExp]", was "/(?:)/"
    //
    // (test262 built-ins/RegExp/S15.10.4.1_A6_T1, #5897.) Expandos on a RegExp
    // live in the `exotic_expando` side table — a `RegExpHeader` is not an
    // `ObjectHeader` — and the `is_regex_pointer` gate keeps every other
    // receiver on the existing fast path.
    //
    // Accessor-aware: the override may be installed via
    // `Object.defineProperty(re, "toString", { get() {…} })`, which a data-only
    // `value_lookup` cannot see (it would silently fall back to the
    // `/source/flags` literal). `exotic_get_own_property` checks accessor
    // descriptors first, invoking the getter with `value` as the receiver, then
    // falls back to the same expando data lookup.
    //
    // A non-callable own `toString` (`re.toString = 5`) declines here and lands
    // in `js_jsvalue_to_string` below, whose own-property arm (#6370) reports
    // the same TypeError the coercion path does.
    #[cfg(feature = "regex-engine")]
    if jsval.is_pointer() {
        let p = jsval.as_pointer::<u8>();
        if crate::regex::is_regex_pointer(p) {
            let own = unsafe {
                crate::object::exotic_expando::exotic_get_own_property(
                    p as usize,
                    crate::object::exotic_expando::ExoticKind::RegExp,
                    "toString",
                    value,
                )
            };
            if let Some(result) = own.and_then(|own| unsafe { call_own_method(own, value) }) {
                return js_jsvalue_to_string(result);
            }
        }
    }
    js_jsvalue_to_string(value)
}

/// Spec `ToString(value)` for argument coercion (e.g. `RegExp.prototype.exec`'s
/// `ToString(string)`, the RegExp constructor's pattern/flags). Unlike
/// [`js_jsvalue_to_string_method`] — which models an explicit `x.toString()`
/// method call and therefore throws on `undefined`/`null` — `ToString(undefined)`
/// is `"undefined"` and `ToString(null)` is `"null"`. For every other value it
/// defers to the method path so object receivers dispatch their own
/// `toString`/`valueOf` (and a throwing one propagates).
#[no_mangle]
pub extern "C" fn js_jsvalue_to_string_coerce(value: f64) -> *mut crate::string::StringHeader {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_undefined() {
        return crate::string::js_string_from_bytes(b"undefined".as_ptr(), 9);
    }
    if jsval.is_null() {
        return crate::string::js_string_from_bytes(b"null".as_ptr(), 4);
    }
    js_jsvalue_to_string_method(value)
}

fn throw_radix_range_error() -> ! {
    // Node/V8 message verbatim: includes the word "argument" (#3146).
    let message = b"toString() radix argument must be between 2 and 36";
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_rangeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// V8-style `DoubleToRadixCString`: render a finite, non-integer f64 in
/// `radix` (2..=36) producing the shortest digit sequence that round-trips
/// back to the same double. Mirrors ECMAScript `Number::toString` for
/// non-decimal radices, including the fractional part (`(10.5).toString(2)`
/// === `"1010.1"`). Assumes `radix` is already validated.
fn double_to_radix_string(value: f64, radix: u32) -> String {
    debug_assert!((2..=36).contains(&radix));
    const CHARS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

    let negative = value < 0.0;
    let abs = value.abs();

    // Split into integer and fractional parts.
    let mut integer = abs.floor();
    let mut fraction = abs - integer;

    // `delta` is half the distance to the next representable double, the
    // tolerance used to decide when enough fractional digits have been
    // emitted to uniquely identify `value` (shortest round-trip).
    let mut delta = 0.5 * (next_double(abs) - abs);
    delta = next_double(0.0).max(delta);

    let mut frac_buf = String::new();
    if fraction >= delta {
        frac_buf.push('.');
        loop {
            // Shift up by the radix.
            fraction *= radix as f64;
            delta *= radix as f64;
            // Extract the digit.
            let digit = fraction.floor() as usize;
            frac_buf.push(CHARS[digit] as char);
            fraction -= digit as f64;
            if fraction >= 0.5 && fraction > delta {
                // Round up: carry into the already-emitted digits.
                if fraction + delta > 1.0 {
                    // Propagate the carry through fraction digits, possibly
                    // into the integer part.
                    loop {
                        // Pop the last char; if it was '.', carry into integer.
                        let last = frac_buf.pop();
                        match last {
                            None => {
                                integer += 1.0;
                                break;
                            }
                            Some('.') => {
                                frac_buf.push('.');
                                integer += 1.0;
                                break;
                            }
                            Some(c) => {
                                let idx = CHARS.iter().position(|&b| b as char == c).unwrap();
                                if idx + 1 < radix as usize {
                                    frac_buf.push(CHARS[idx + 1] as char);
                                    break;
                                }
                                // Was the max digit (e.g. 'f' in hex): becomes
                                // '0' and the carry continues leftward.
                            }
                        }
                    }
                    break;
                }
            }
            if fraction < delta {
                break;
            }
        }
        // A trailing '.' with no fraction digits (carry consumed all) is junk.
        if frac_buf == "." {
            frac_buf.clear();
        }
    }

    // Integer part: repeated division. `integer` may have grown via carry.
    let mut int_buf = String::new();
    if integer == 0.0 {
        int_buf.push('0');
    } else {
        while integer >= 1.0 {
            let remainder = (integer % radix as f64) as usize;
            int_buf.push(CHARS[remainder] as char);
            integer = (integer / radix as f64).floor();
        }
    }
    let int_part: String = int_buf.chars().rev().collect();

    let mut result = String::new();
    if negative {
        result.push('-');
    }
    result.push_str(&int_part);
    result.push_str(&frac_buf);
    result
}

/// Smallest representable double strictly greater than `x` (for finite `x`).
fn next_double(x: f64) -> f64 {
    if x.is_nan() || x == f64::INFINITY {
        return x;
    }
    let bits = x.to_bits();
    let next = if x >= 0.0 {
        bits + 1
    } else if bits == (1u64 << 63) {
        // -0.0 → smallest positive subnormal
        1
    } else {
        bits - 1
    };
    f64::from_bits(next)
}

/// Convert a NaN-boxed f64 value to a string with the given radix argument.
/// `radix_value` is the *raw* NaN-boxed radix argument (number/string/bool/
/// undefined); it is ToNumber/ToInteger-coerced and validated to `2..=36`
/// here, throwing `RangeError` for out-of-range values (#2864). Handles
/// BigInt (uses bigint_to_string_radix), numbers, strings, etc.
#[no_mangle]
pub extern "C" fn js_jsvalue_to_string_radix(
    value: f64,
    radix_value: f64,
) -> *mut crate::string::StringHeader {
    let jsval = JSValue::from_bits(value.to_bits());

    // A Temporal value's `toString` takes an *options object*, not a radix —
    // the codegen routes any single-arg `.toString(x)` here. Dispatch back to
    // the Temporal method router so the options bag flows through, instead of
    // ToNumber-coercing it as a radix (which throws a spurious RangeError).
    #[cfg(feature = "temporal")]
    if crate::temporal::is_temporal_value(value) {
        let result = crate::temporal::dispatch::call_method(value, "toString", &[radix_value]);
        let rv = JSValue::from_bits(result.to_bits());
        if rv.is_string() {
            return rv.as_string_ptr() as *mut crate::string::StringHeader;
        }
        return js_jsvalue_to_string(result);
    }

    // Numeric receivers (Number / BigInt / Int32 / boxed Number): the second
    // argument is a radix — coerce + validate it (throws on out-of-range). Other
    // object receivers (Date, user `toString(opts)` methods) reach this with a
    // non-radix argument, so we lazily validate the radix only on the numeric
    // arms and otherwise dispatch the receiver's own `toString` with the
    // argument forwarded — never ToNumber-coercing an options object as a radix.
    macro_rules! radix {
        () => {
            match unsafe { coerce_validate_radix(radix_value) } {
                Some(r) => r,
                None => 10,
            }
        };
    }

    if jsval.is_bigint() {
        let ptr = jsval.as_bigint_ptr();
        crate::bigint::js_bigint_to_string_radix(ptr, radix!())
    } else if jsval.is_string() {
        jsval.as_string_ptr() as *mut crate::string::StringHeader
    } else if jsval.is_int32() {
        let radix = radix!();
        let n = jsval.as_int32();
        if radix == 10 {
            let s = n.to_string();
            return crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
        }
        let s = double_to_radix_string(n as f64, radix as u32);
        crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
    } else if jsval.is_number() {
        number_to_radix_string(value, radix!())
    } else {
        // Pointer / object receiver. `Number.prototype.toString` brand
        // semantics (ECMA-262 21.1.3): a boxed `Number` exposes its
        // [[NumberData]]; `Number.prototype` itself has [[NumberData]] +0;
        // any other object has no number value and dispatches its own
        // `toString` with the argument forwarded (so a Temporal/Date receiver
        // honours its options bag instead of treating it as a radix).
        const CLASS_ID_BOXED_NUMBER: u32 = 0xFFFF_00D0;
        if let Some((cid, payload)) = crate::builtins::boxed_primitive_payload(value) {
            if cid == CLASS_ID_BOXED_NUMBER {
                return number_to_radix_string(payload, radix!());
            }
        }
        if value.to_bits() == crate::object::builtin_prototype_value("Number").to_bits() {
            return number_to_radix_string(0.0, radix!());
        }
        // Forward the argument to the receiver's own `toString`. A Temporal
        // value routes to its options-aware `toString`; a plain object falls
        // back to `Object.prototype.toString` ([object Object]).
        if jsval.is_pointer() {
            let args = [radix_value];
            let result = unsafe {
                crate::object::js_native_call_method(
                    value,
                    b"toString".as_ptr() as *const i8,
                    8,
                    args.as_ptr(),
                    1,
                )
            };
            let rjv = JSValue::from_bits(result.to_bits());
            if rjv.is_string() {
                return rjv.as_string_ptr() as *mut crate::string::StringHeader;
            }
            if rjv.is_short_string() {
                return crate::string::js_string_materialize_to_heap(result);
            }
        }
        js_jsvalue_to_string(value)
    }
}

/// Format a real f64 `n` in the given `radix` (2..=36), matching
/// `Number.prototype.toString`'s NaN/Infinity/decimal handling.
fn number_to_radix_string(n: f64, radix: i32) -> *mut crate::string::StringHeader {
    if n.is_nan() {
        return crate::string::js_string_from_bytes(b"NaN".as_ptr(), 3);
    }
    if n.is_infinite() {
        if n > 0.0 {
            return crate::string::js_string_from_bytes(b"Infinity".as_ptr(), 8);
        } else {
            return crate::string::js_string_from_bytes(b"-Infinity".as_ptr(), 9);
        }
    }
    if radix == 10 {
        return crate::string::js_number_to_string(n);
    }
    let s = double_to_radix_string(n, radix as u32);
    crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

/// Ensure a value is a native string pointer.
/// This is specifically for fetch headers where we need to handle:
/// 1. Raw string pointers (literal strings - f64 bits ARE the pointer)
/// 2. NaN-boxed strings (STRING_TAG)
/// 3. JS handle strings (from process.env)
/// Returns the string pointer as i64.
#[no_mangle]
pub extern "C" fn js_ensure_string_ptr(value: f64) -> i64 {
    let bits = value.to_bits();

    // Check for JS handle first - these need conversion
    if is_js_handle(value) {
        let func_ptr = JS_HANDLE_TO_STRING.load(Ordering::SeqCst);
        if !func_ptr.is_null() {
            let func: JsHandleToStringFn = unsafe { std::mem::transmute(func_ptr) };
            return func(value) as i64;
        }
        // Fallback - create a placeholder string
        return crate::string::js_string_from_bytes(b"[JS Handle]".as_ptr(), 11) as i64;
    }

    // Check for NaN-boxed string (STRING_TAG)
    if (bits & TAG_MASK) == STRING_TAG {
        let ptr = (bits & POINTER_MASK) as i64;
        if ptr != 0 {
            let str_header = ptr as *const crate::string::StringHeader;
            unsafe {
                let length = (*str_header).byte_len;
                // Make a copy of the string to ensure we have a Perry-allocated string
                let data_ptr = (str_header as *const u8)
                    .add(std::mem::size_of::<crate::string::StringHeader>());
                let copy = crate::string::js_string_from_bytes(data_ptr, length);
                return copy as i64;
            }
        }
        return ptr;
    }

    // Otherwise, treat the f64 bits directly as a pointer (raw string literal)
    bits as i64
}

#[cfg(test)]
mod error_subclass_tostring_tests {
    use super::*;

    #[test]
    fn object_field_to_owned_string_reads_and_misses() {
        unsafe {
            let obj = crate::object::js_object_alloc(0, 2);
            let key = crate::string::js_string_from_bytes(b"message".as_ptr(), 7);
            let val = crate::string::js_string_from_bytes(b"hi".as_ptr(), 2);
            crate::object::js_object_set_field_by_name(
                obj,
                key,
                crate::value::js_nanbox_string(val as i64),
            );
            assert_eq!(
                object_field_to_owned_string(obj, b"message").as_deref(),
                Some("hi")
            );
            assert_eq!(object_field_to_owned_string(obj, b"missing"), None);
        }
    }
}

#[cfg(test)]
mod radix_tostring_tests {
    use super::*;

    #[test]
    fn integer_radix_formatting() {
        assert_eq!(double_to_radix_string(255.0, 16), "ff");
        assert_eq!(double_to_radix_string(10.0, 2), "1010");
        assert_eq!(double_to_radix_string(255.0, 2), "11111111");
        assert_eq!(double_to_radix_string(-255.0, 16), "-ff");
        assert_eq!(double_to_radix_string(0.0, 2), "0");
        assert_eq!(double_to_radix_string(35.0, 36), "z");
    }

    #[test]
    fn fractional_radix_formatting_matches_v8() {
        // Terminating fractions.
        assert_eq!(double_to_radix_string(10.5, 2), "1010.1");
        assert_eq!(double_to_radix_string(10.5, 16), "a.8");
        assert_eq!(double_to_radix_string(10.5, 36), "a.i");
        assert_eq!(double_to_radix_string(-10.5, 2), "-1010.1");
        assert_eq!(double_to_radix_string(255.5, 16), "ff.8");
        assert_eq!(double_to_radix_string(1.5, 2), "1.1");
        assert_eq!(double_to_radix_string(100.25, 2), "1100100.01");
        // Repeating fraction — shortest round-trip (matches Node v25).
        assert_eq!(
            double_to_radix_string(0.1, 2),
            "0.0001100110011001100110011001100110011001100110011001101"
        );
    }

    #[test]
    fn coerce_validate_radix_semantics() {
        unsafe {
            // undefined → None (default radix path).
            assert_eq!(
                coerce_validate_radix(f64::from_bits(crate::value::TAG_UNDEFINED)),
                None
            );
            // Plain number radices.
            assert_eq!(coerce_validate_radix(16.0), Some(16));
            assert_eq!(coerce_validate_radix(2.0), Some(2));
            assert_eq!(coerce_validate_radix(36.0), Some(36));
            // ToInteger truncation.
            assert_eq!(coerce_validate_radix(2.9), Some(2));
            // int32-boxed radix.
            assert_eq!(
                coerce_validate_radix(f64::from_bits(JSValue::int32(16).bits())),
                Some(16)
            );
            // String radix coerces via ToNumber.
            let s = crate::string::js_string_from_bytes(b"16".as_ptr(), 2);
            assert_eq!(
                coerce_validate_radix(crate::value::js_nanbox_string(s as i64)),
                Some(16)
            );
        }
    }
}
