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
unsafe fn ordinary_to_primitive_string(value: f64) -> Option<f64> {
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
        // No callable custom toString — this is the default
        // `Object.prototype.toString` → `"[object Object]"`. Stop here.
        MethodOutcome::Absent => return None,
    }

    match call_method_for_primitive(&scope, &value_handle, b"valueOf") {
        MethodOutcome::Primitive(p) => Some(p),
        // Both toString and valueOf failed to produce a primitive — Node
        // throws `TypeError: Cannot convert object to primitive value`. We
        // approximate by falling back to the default `"[object Object]"`.
        MethodOutcome::NonPrimitive | MethodOutcome::Absent => None,
    }
}

enum MethodOutcome {
    /// Method was callable and returned a primitive.
    Primitive(f64),
    /// Method was callable but returned a non-primitive (object/array).
    NonPrimitive,
    /// No own/inherited callable method with that name was found.
    Absent,
}

/// Resolve `obj[method_name]` (own + prototype chain) and, if it is a
/// callable closure, invoke it with `this = obj` (no args). Returns whether
/// the result was a primitive, a non-primitive, or whether the method was
/// absent / non-callable.
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
    let method = crate::object::js_object_get_field_by_name(
        obj_ptr,
        key_handle.get_raw_const_ptr::<crate::string::StringHeader>(),
    );
    // Must be a callable closure value (POINTER_TAG + CLOSURE_MAGIC).
    let method_bits = method.bits();
    if (method_bits & 0xFFFF_0000_0000_0000) != POINTER_TAG {
        return MethodOutcome::Absent;
    }
    let method_ptr = (method_bits & POINTER_MASK) as usize;
    if !crate::closure::is_closure_ptr(method_ptr) {
        return MethodOutcome::Absent;
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
        || ret_jsv.is_bigint();
    if is_primitive {
        MethodOutcome::Primitive(ret)
    } else {
        MethodOutcome::NonPrimitive
    }
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
        // Convert int32 to string
        let n = jsval.as_int32();
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
        if !ptr.is_null() && (ptr as usize) >= 0x10000 {
            // Symbols: detect via the side-table before any GC header read.
            if crate::symbol::is_registered_symbol(ptr as usize) {
                return unsafe {
                    crate::symbol::js_symbol_to_string(value) as *mut crate::string::StringHeader
                };
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
            // BUFFER_REGISTRY before computing gc_header (which would read
            // garbage one word before the buffer). `Buffer.toString()` with
            // no arg defaults to UTF-8 — Node prints the raw bytes.
            if crate::buffer::is_registered_buffer(ptr as usize) {
                return crate::buffer::js_buffer_to_string(
                    ptr as *const crate::buffer::BufferHeader,
                    0,
                );
            }
            unsafe {
                let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
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
            }
        }
        crate::string::js_string_from_bytes(b"[object Object]".as_ptr(), 15)
    } else {
        // Regular number - use js_number_to_string
        crate::string::js_number_to_string(value)
    }
}

/// Convert a NaN-boxed f64 value to a string with the given radix.
/// Handles BigInt (uses bigint_to_string_radix), numbers, strings, etc.
#[no_mangle]
pub extern "C" fn js_jsvalue_to_string_radix(
    value: f64,
    radix: i32,
) -> *mut crate::string::StringHeader {
    let jsval = JSValue::from_bits(value.to_bits());

    if jsval.is_bigint() {
        let ptr = jsval.as_bigint_ptr();
        crate::bigint::js_bigint_to_string_radix(ptr, radix)
    } else if jsval.is_string() {
        jsval.as_string_ptr() as *mut crate::string::StringHeader
    } else if jsval.is_int32() {
        let n = jsval.as_int32();
        let s = if radix == 16 {
            format!("{:x}", n)
        } else if radix == 10 || radix == 0 {
            n.to_string()
        } else {
            // General radix conversion
            let mut result = String::new();
            let mut val = if n < 0 { -(n as i64) as u64 } else { n as u64 };
            let r = radix as u64;
            if val == 0 {
                return crate::string::js_string_from_bytes(b"0".as_ptr(), 1);
            }
            while val > 0 {
                let digit = (val % r) as u8;
                result.push(if digit < 10 {
                    (b'0' + digit) as char
                } else {
                    (b'a' + digit - 10) as char
                });
                val /= r;
            }
            if n < 0 {
                result.push('-');
            }
            let s: String = result.chars().rev().collect();
            return crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
        };
        crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
    } else {
        // Regular f64 number
        let n = value;
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
        if radix == 10 || radix == 0 {
            return crate::string::js_number_to_string(value);
        }
        // For hex and other radixes, convert via integer
        let n_i64 = n as i64;
        let s = if radix == 16 {
            if n_i64 < 0 {
                format!("-{:x}", -n_i64)
            } else {
                format!("{:x}", n_i64)
            }
        } else {
            let mut result = String::new();
            let mut val = if n_i64 < 0 {
                (-n_i64) as u64
            } else {
                n_i64 as u64
            };
            let r = radix as u64;
            if val == 0 {
                return crate::string::js_string_from_bytes(b"0".as_ptr(), 1);
            }
            while val > 0 {
                let digit = (val % r) as u8;
                result.push(if digit < 10 {
                    (b'0' + digit) as char
                } else {
                    (b'a' + digit - 10) as char
                });
                val /= r;
            }
            if n_i64 < 0 {
                result.push('-');
            }
            result.chars().rev().collect()
        };
        crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
    }
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
