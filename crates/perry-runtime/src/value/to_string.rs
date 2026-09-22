//! NaN-boxed value to-string conversion helpers.

use super::to_string_array::{array_prototype_to_string_override, ArrayToStringOutcome};
pub(crate) use super::to_string_class_ref::class_ref_to_primitive;
use super::to_string_primitive::{
    exotic_own_to_string, function_to_string_via_prototype, ordinary_to_primitive_string,
    throw_cannot_convert_to_primitive, ExoticOwnToString, FunctionToStringOutcome,
};
// The only caller left in this file after the split sits behind `regex-engine`,
// so an unconditional import is unused under a feature set that turns it off
// (`cargo check -p perry --bins`, which is the `warnings` gate's product step).
#[cfg(feature = "regex-engine")]
use super::to_string_primitive::call_own_method;
use super::*;
use std::cell::Cell;
use std::sync::atomic::Ordering;

crate::perry_thread_local! {
    /// One-shot request to skip the `[Symbol.toPrimitive]` shortcut inside
    /// `js_jsvalue_to_string` for the very next top-level call. Set by the
    /// explicit `x.toString()` path (`js_jsvalue_to_string_method`): a
    /// `.toString()` call resolves `Object.prototype.toString` (or an own
    /// `toString`) and must NOT consult `[Symbol.toPrimitive]` — only the
    /// coercion paths (`String(x)`, `x + ""`, `` `${x}` ``, and the ToString
    /// argument coercion in `js_jsvalue_to_string_coerce`) do ToPrimitive.
    /// Consumed (read + cleared) at the top of `js_jsvalue_to_string` so it
    /// applies to a single top-level object and never leaks into recursion or
    /// the next unrelated conversion. (#6373)
    static SKIP_TO_PRIMITIVE_ONESHOT: Cell<bool> = const { Cell::new(false) };
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
#[no_mangle]
pub extern "C" fn js_jsvalue_to_string(value: f64) -> *mut crate::string::StringHeader {
    js_jsvalue_to_string_impl(value, false)
}

pub(crate) fn js_jsvalue_to_string_impl(
    value: f64,
    reject_symbol: bool,
) -> *mut crate::string::StringHeader {
    // Explicit `.toString()` skips `[Symbol.toPrimitive]` for this value (#6373).
    let skip_to_primitive = SKIP_TO_PRIMITIVE_ONESHOT.with(|c| c.replace(false));
    if is_js_handle(value) {
        let func_ptr = JS_HANDLE_TO_STRING.load(Ordering::SeqCst);
        if !func_ptr.is_null() {
            let func: JsHandleToStringFn = unsafe { std::mem::transmute(func_ptr) };
            return func(value);
        }
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
    } else if jsval.is_undefined() || jsval.bits() == crate::value::TAG_HOLE {
        // #9462: an empty-slot sentinel that reached a string coercion. Its
        // bits are a NaN, so the numeric tail below rendered it "NaN"; node
        // prints "undefined" for `String(a[i])` on an empty slot. Template
        // interpolation and `x.toString()` both funnel through here, so this
        // one arm covers all three spellings.
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
        // — `String(C)` / `"" + C` must produce the class's source text, not
        // the numeric id. #9413 gave codegen a class-source side table
        // (`js_register_class_source`), so this is the real source when the
        // class came from user code and the NativeFunction placeholder only
        // for classes perry synthesized.
        let n = jsval.as_int32();
        let cid = (value.to_bits() & 0xFFFF_FFFF) as u32;
        if crate::object::is_class_id_registered(cid) {
            if !skip_to_primitive {
                let primitive = unsafe { class_ref_to_primitive(value, 2) };
                return js_jsvalue_to_string_impl(primitive, reject_symbol);
            }
            let s = crate::object::class_ref_to_string(cid);
            return crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
        }
        let s = n.to_string();
        crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
    } else if jsval.is_bigint() {
        // BigInt - convert to decimal string
        let ptr = jsval.as_bigint_ptr();
        crate::bigint::js_bigint_to_string(ptr)
    } else if jsval.is_pointer() {
        // Arrays stringify via join; other objects use their own conversion.
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
                return js_jsvalue_to_string_impl(target, reject_symbol);
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
                    return js_jsvalue_to_string_impl(target, reject_symbol);
                }
                return crate::string::js_string_from_bytes(b"[object Object]".as_ptr(), 15);
            }
            if crate::symbol::is_registered_symbol(ptr as usize) {
                if reject_symbol {
                    crate::collection_iter::throw_type_error(
                        "Cannot convert a Symbol value to a string",
                    );
                }
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
                let s = crate::node_vm::function_source_for_closure(ptr as usize);
                return crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
            }
            // Consult `[Symbol.toPrimitive]("string")` if the object has a
            // custom toPrimitive method registered in the symbol side-table.
            // A changed result means the user-defined method produced a
            // string-hint primitive — recurse so strings pass through as-is
            // and numbers get js_number_to_string. Skipped on the explicit
            // `x.toString()` path (#6373): a `.toString()` call resolves
            // `Object.prototype.toString` / an own `toString`, never
            // `[Symbol.toPrimitive]`.
            if !skip_to_primitive {
                let primitive = unsafe { crate::symbol::js_to_primitive(value, 2) };
                if primitive.to_bits() != value.to_bits() {
                    return js_jsvalue_to_string_impl(primitive, reject_symbol);
                }
            }
            // BufferHeader-backed values need handling before GC-header probes.
            // ArrayBuffer, SharedArrayBuffer, and DataView inherit object tags.
            if crate::buffer::is_registered_buffer(ptr as usize) {
                if crate::buffer::is_non_indexed_buffer_view(ptr as usize) {
                    let branded = unsafe { crate::object::js_object_to_string(value) };
                    return (branded.to_bits() & 0x0000_FFFF_FFFF_FFFF)
                        as *mut crate::string::StringHeader;
                }
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
                        return js_jsvalue_to_string_impl(primitive, reject_symbol)
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
                        return js_jsvalue_to_string_impl(primitive, reject_symbol)
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
                            return js_jsvalue_to_string_impl(primitive, reject_symbol)
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
                    return js_jsvalue_to_string_impl(f64::from_bits(html.bits()), reject_symbol);
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
                // Binary size: these are SHAPE probes on a generic path, so the
                // static reference keeps the whole URL class + parser alive in
                // every binary even though the runtime check can never pass
                // without `url-engine`. `uses_url` (zero-false-negative by
                // construction) is what turns that feature on, so a program
                // with no URL API cannot own a URL or URLSearchParams here.
                // `boxed` is bound inside the gate: it feeds only these probes.
                #[cfg(feature = "url-engine")]
                {
                    let boxed = f64::from_bits(POINTER_TAG | ((ptr as u64) & POINTER_MASK));
                    let url_href = crate::url::url_class::js_url_href_if_url(boxed);
                    if url_href.to_bits() != crate::value::TAG_UNDEFINED {
                        return js_jsvalue_to_string_impl(url_href, reject_symbol);
                    }
                    if crate::url::try_read_as_search_params(
                        ptr as *mut crate::object::ObjectHeader,
                    )
                    .is_some()
                    {
                        return crate::url::search_params::js_url_search_params_to_string(
                            ptr as *mut crate::object::ObjectHeader,
                        );
                    }
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
                    return js_jsvalue_to_string_impl(primitive, reject_symbol);
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

/// `value.toString()` as an explicit METHOD CALL (#3146). Unlike the abstract
/// `js_jsvalue_to_string` (used for `String(x)`, template literals, and `+`
/// coercion, where a nullish operand stringifies to "undefined"/"null"), a
/// member call `u.toString()` on `undefined`/`null` is a property read on a
/// nullish base and must throw a `TypeError`. For every non-nullish value this
/// delegates to `js_jsvalue_to_string`, so ordinary `.toString()` behaviour is
/// unchanged.
#[no_mangle]
pub extern "C" fn js_jsvalue_to_string_method(value: f64) -> *mut crate::string::StringHeader {
    // `n.toString()` on a plain number is `Number::toString(n)` and nothing
    // else, but it reached that answer through four frames:
    // `to_string_method_impl` (nullish guard, pointer/regex probes, then a
    // thread-local one-shot WRITE) -> `js_jsvalue_to_string` (which READS and
    // clears that same one-shot, probes for a JS handle, then walks its own
    // eight-arm tag ladder) -> `js_number_to_string`. None of it can change a
    // plain double's answer: a number is never nullish, never a pointer, never
    // a regex, and every arm of both ladders is keyed on a perry tag in the
    // `0x7FF9..=0x7FFF` band that `is_number()` excludes by definition. The
    // one-shot is only ever consumed by the object dispatch this value cannot
    // reach, so not setting it leaves nothing stale behind.
    if crate::value::JSValue::from_bits(value.to_bits()).is_number() {
        return crate::string::js_number_to_string(value);
    }
    // Explicit `x.toString()`: resolve `Object.prototype.toString` / an own
    // `toString`, never `[Symbol.toPrimitive]`. (#6373)
    to_string_method_impl(value, /* skip_to_primitive */ true)
}

/// Shared body of the `.toString()` method / ToString-coercion paths.
///
/// `skip_to_primitive` distinguishes the two callers that reach the object
/// dispatch below:
/// - `js_jsvalue_to_string_method` (explicit `x.toString()`) passes `true`:
///   `.toString()` must not consult `[Symbol.toPrimitive]`.
/// - `js_jsvalue_to_string_coerce` (spec `ToString(argument)`) passes `false`:
///   `ToString` of an object does `ToPrimitive(argument, string)` first, so
///   `[Symbol.toPrimitive]` is honored.
fn to_string_method_impl(value: f64, skip_to_primitive: bool) -> *mut crate::string::StringHeader {
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
    // Arm the one-shot skip so the object dispatch inside `js_jsvalue_to_string`
    // bypasses `[Symbol.toPrimitive]` for the explicit `.toString()` caller.
    if skip_to_primitive {
        SKIP_TO_PRIMITIVE_ONESHOT.with(|c| c.set(true));
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
    // Spec `ToString(argument)` does `ToPrimitive(argument, string)` for an
    // object receiver, so `[Symbol.toPrimitive]` IS consulted here (unlike the
    // explicit `x.toString()` path). (#6373)
    to_string_method_impl(value, /* skip_to_primitive */ false)
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
