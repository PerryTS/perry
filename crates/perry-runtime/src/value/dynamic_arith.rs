//! Dynamic arithmetic dispatch: handles BigInt vs float at runtime.
//!
//! When a parameter has Type::Any (is_union=true), it may hold a BigInt
//! (NaN-boxed with BIGINT_TAG) or a regular f64. These functions check
//! the NaN-box tag at runtime and dispatch to the correct operation.

use super::*;

/// Convert a NaN-boxed JSValue to a *mut BigIntHeader for arithmetic.
/// If the value is already a BigInt, extracts the pointer.
/// Otherwise allocates a new BigInt from the f64 value.
#[inline]
unsafe fn coerce_to_bigint_ptr(val: f64) -> *mut crate::bigint::BigIntHeader {
    let jsval = JSValue::from_bits(val.to_bits());
    if jsval.is_bigint() {
        jsval.as_bigint_ptr() as *mut _
    } else {
        crate::bigint::js_bigint_from_f64(val)
    }
}

/// Throw `TypeError: Cannot mix BigInt and other types, use explicit
/// conversions`, matching Node when a BigInt operand is combined with a
/// non-BigInt operand in an arithmetic / bitwise operation (#2908).
#[cold]
unsafe fn throw_mix_bigint() -> ! {
    let msg = b"Cannot mix BigInt and other types, use explicit conversions";
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(js_nanbox_pointer(err as i64))
}

/// Enforce Node's rule that BigInt operators require *both* operands to be
/// BigInt. Returns true when both are BigInt (proceed with the BigInt op),
/// false when neither is (use the numeric path), and throws a TypeError when
/// exactly one operand is a BigInt.
#[inline]
unsafe fn both_bigint_or_throw(a: f64, b: f64) -> bool {
    let a_big = JSValue::from_bits(a.to_bits()).is_bigint();
    let b_big = JSValue::from_bits(b.to_bits()).is_bigint();
    if a_big && b_big {
        true
    } else if a_big || b_big {
        throw_mix_bigint();
    } else {
        false
    }
}

#[cold]
unsafe fn throw_add_type_error(message: &[u8]) -> ! {
    let s = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(js_nanbox_pointer(err as i64))
}

#[inline]
unsafe fn is_symbol_value(value: f64) -> bool {
    crate::symbol::js_is_symbol(value) != 0
}

#[inline]
unsafe fn is_nonprimitive_object_value(value: f64) -> bool {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_pointer() {
        return false;
    }
    let ptr = jsval.as_pointer::<u8>() as usize;
    ptr >= 0x1000 && !is_symbol_value(value)
}

unsafe fn to_primitive_default_for_add(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_pointer() || is_symbol_value(value) {
        return value;
    }
    let ptr = jsval.as_pointer::<u8>() as usize;
    if ptr < 0x1000 {
        return value;
    }

    // A Proxy is a small registered id, not a heap object — the ToPrimitive
    // machinery below dereferences the fake pointer and segfaults
    // (`"" + new Proxy(fn, {})`). A trap-less default ToPrimitive forwards
    // to the target; a callable target stringifies via
    // Function.prototype.toString (the NativeFunction form).
    if crate::proxy::js_proxy_is_proxy(value) == 1 {
        let s = crate::value::js_jsvalue_to_string(value);
        return crate::value::js_nanbox_string(s as i64);
    }

    // Buffers / TypedArrays carry NO `ObjectHeader` (a `BufferHeader` /
    // `TypedArrayHeader` has a different, smaller layout). The
    // `js_url_href_if_url` / `try_read_as_search_params` /
    // `ordinary_to_primitive_number_for_add` probes below all bit-cast `ptr`
    // to an `ObjectHeader` and read its fields, so a Buffer/TypedArray operand
    // would deref a fake header one word before the data and segfault
    // (issue #5131 — `req.on('data', c => body += c)` on a `node:http` server,
    // where the chunk is an un-typed Buffer and `body += c` lowers to the
    // fully-dynamic add path). Detect via the registries (by-value lookups, no
    // deref) and route to `js_jsvalue_to_string`, which yields the same string
    // form as an explicit `.toString()` (Buffer→utf8, TypedArray→`join(",")`).
    // This matches the guards `js_jsvalue_to_string` itself runs before its
    // ordinary-object dispatch.
    if crate::buffer::is_registered_buffer(ptr)
        || crate::typedarray::lookup_typed_array_kind(ptr).is_some()
    {
        let s = crate::value::js_jsvalue_to_string(value);
        return crate::value::js_nanbox_string(s as i64);
    }

    let primitive = crate::symbol::js_to_primitive(value, 0);
    if primitive.to_bits() != value.to_bits() {
        if is_nonprimitive_object_value(primitive) {
            throw_add_type_error(b"Cannot convert object to primitive value");
        }
        return primitive;
    }

    // A callable closure is NOT an `ObjectHeader`. The URL probe and the
    // ordinary-object `valueOf`/`toString` machinery below all bit-cast `ptr`
    // to an `ObjectHeader`; for a class-method closure
    // (`"" + C.prototype.method`) the `valueOf` field lookup reads a bogus slot
    // that it then calls → EXC_BAD_ACCESS. Resolve the function's ToPrimitive
    // through the closure-aware path (own/inherited `valueOf` if primitive, else
    // the `Function.prototype.toString` source form). `Symbol.toPrimitive` was
    // already consulted by `js_to_primitive` above.
    if crate::closure::is_closure_ptr(ptr) {
        return crate::value::function_to_primitive_for_add(value);
    }

    if crate::date::is_date_cell_addr(ptr) {
        let s = crate::date::js_date_to_string(value);
        return crate::value::js_nanbox_string(s as i64);
    }

    // WHATWG `URL` / `URLSearchParams` have native `toString`s (`href` / the
    // query string) that OrdinaryToPrimitive can't see — it would resolve the
    // inherited `Object.prototype.toString` and yield "[object Object]". Like
    // the Date special-case above, pre-empt with the real string so
    // `"" + url` / `` `${url}` `` match explicit `url.toString()` (#URL coercion).
    // Skip small-handle values (sockets / timers / widget handles): they are
    // registry ids, not heap `ObjectHeader`s, so the shape probe would
    // dereference unmapped memory.
    if !crate::value::addr_class::is_handle_band(ptr) {
        let boxed =
            f64::from_bits(crate::value::POINTER_TAG | ((ptr as u64) & crate::value::POINTER_MASK));
        let href = crate::url::url_class::js_url_href_if_url(boxed);
        if href.to_bits() != crate::value::TAG_UNDEFINED {
            let s = js_jsvalue_to_string(href);
            return crate::value::js_nanbox_string(s as i64);
        }
        let obj = ptr as *mut crate::object::ObjectHeader;
        if crate::url::try_read_as_search_params(obj).is_some() {
            let s = crate::url::search_params::js_url_search_params_to_string(obj);
            return crate::value::js_nanbox_string(s as i64);
        }
    }

    match crate::value::ordinary_to_primitive_number_for_add(value) {
        crate::value::OrdinaryToPrimitiveOutcome::Primitive(p) => p,
        crate::value::OrdinaryToPrimitiveOutcome::DefaultString => {
            let s = js_jsvalue_to_string(value);
            crate::value::js_nanbox_string(s as i64)
        }
        crate::value::OrdinaryToPrimitiveOutcome::TypeError => {
            throw_add_type_error(b"Cannot convert object to primitive value")
        }
    }
}

type BigIntBinaryOp = extern "C" fn(
    *const crate::bigint::BigIntHeader,
    *const crate::bigint::BigIntHeader,
) -> *mut crate::bigint::BigIntHeader;

#[inline]
unsafe fn coerce_to_bigint_handle<'scope>(
    scope: &'scope crate::gc::RuntimeHandleScope,
    value: &crate::gc::RuntimeHandle<'scope>,
) -> crate::gc::RuntimeHandle<'scope> {
    let ptr = coerce_to_bigint_ptr(value.get_nanbox_f64());
    scope.root_bigint_ptr(ptr as *const crate::bigint::BigIntHeader)
}

#[inline]
unsafe fn dynamic_bigint_binary_op(a: f64, b: f64, op: BigIntBinaryOp) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let a_handle = scope.root_nanbox_f64(a);
    let b_handle = scope.root_nanbox_f64(b);
    dynamic_bigint_binary_op_from_handles(&scope, &a_handle, &b_handle, op)
}

#[inline]
unsafe fn dynamic_bigint_binary_op_from_handles<'scope>(
    scope: &'scope crate::gc::RuntimeHandleScope,
    a: &crate::gc::RuntimeHandle<'scope>,
    b: &crate::gc::RuntimeHandle<'scope>,
    op: BigIntBinaryOp,
) -> f64 {
    let a_bigint = coerce_to_bigint_handle(scope, a);
    let b_bigint = coerce_to_bigint_handle(scope, b);
    let result = op(
        a_bigint.get_raw_const_ptr::<crate::bigint::BigIntHeader>(),
        b_bigint.get_raw_const_ptr::<crate::bigint::BigIntHeader>(),
    );
    js_nanbox_bigint(result as i64)
}

/// Decode an int32-tagged operand to its plain-double value before an f64
/// arithmetic op. An int32 is NaN-boxed (INT32_TAG = 0x7FFE), so its f64 bits
/// ARE a NaN; a raw `a OP b` would propagate the tag through the FPU (ARM64
/// keeps the NaN payload) and hand back the boxed operand instead of the
/// result — e.g. a better-sqlite3 integer column `n` made `n + 100 === n`.
/// Plain doubles (and real NaNs from arithmetic) pass through unchanged.
#[inline]
fn numify_arith_operand(v: f64) -> f64 {
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_int32() {
        jv.as_int32() as f64
    } else {
        v
    }
}

/// Dynamic multiply: BigInt * BigInt if either operand is BigInt, else f64 * f64.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_mul(a: f64, b: f64) -> f64 {
    if both_bigint_or_throw(a, b) {
        return dynamic_bigint_binary_op(a, b, crate::bigint::js_bigint_mul);
    }
    numify_arith_operand(a) * numify_arith_operand(b)
}

/// Dynamic add: BigInt + BigInt if either operand is BigInt, else f64 + f64.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_add(a: f64, b: f64) -> f64 {
    if both_bigint_or_throw(a, b) {
        return dynamic_bigint_binary_op(a, b, crate::bigint::js_bigint_add);
    }
    numify_arith_operand(a) + numify_arith_operand(b)
}

/// `ToNumeric(value)` for the `++`/`--` slow path: a BigInt passes through
/// unchanged, every other value is coerced via `ToNumber`. The codegen's
/// update path uses this for the *operand read* so that the postfix return
/// value and the stepping base keep their BigInt type instead of collapsing
/// to a Number (`let i = 10n; i++` must stay a BigInt — otherwise a later
/// `i + 87n` throws a mixed-type TypeError; test262
/// BigInt/prototype/toString/a-z).
#[no_mangle]
pub unsafe extern "C" fn js_to_numeric(value: f64) -> f64 {
    if JSValue::from_bits(value.to_bits()).is_bigint() {
        value
    } else {
        crate::builtins::js_number_coerce(value)
    }
}

/// Step a ToNumeric operand by `1` of its own numeric type for `++`/`--`.
/// `numeric` is already the result of [`js_to_numeric`]: a BigInt steps by
/// `1n` (staying a BigInt), any Number steps by `1.0`. `is_increment` is
/// nonzero for `++`, zero for `--`.
#[no_mangle]
pub unsafe extern "C" fn js_numeric_step(numeric: f64, is_increment: i32) -> f64 {
    if JSValue::from_bits(numeric.to_bits()).is_bigint() {
        let one_ptr = crate::bigint::js_bigint_from_i64(1);
        // `js_nanbox_bigint` is pure and `dynamic_bigint_binary_op` roots both
        // operands before any further allocation, so `one_ptr` survives.
        let one_val = js_nanbox_bigint(one_ptr as i64);
        let op = if is_increment != 0 {
            crate::bigint::js_bigint_add
        } else {
            crate::bigint::js_bigint_sub
        };
        dynamic_bigint_binary_op(numeric, one_val, op)
    } else if is_increment != 0 {
        numeric + 1.0
    } else {
        numeric - 1.0
    }
}

/// Dynamic `a + b` for type-uncertain operands. Per JS spec, when either
/// operand is a string after ToPrimitive, the result is string concatenation;
/// otherwise both operands are coerced to numbers and summed (or BigInt-
/// summed when either is BigInt). The codegen dispatches here for `+` when
/// neither operand has a statically-known type — refs #486 (hono's
/// `Node.buildRegExpStr` does `k + c.buildRegExpStr()` inside a for-of loop
/// over `Object.keys(...)` results, both operands lower to plain f64s with
/// inferred type Any, the static-string-concat fast path doesn't fire, and
/// the previous fallback called `js_number_coerce` on each side and `fadd`d
/// the results — turning `"c" + ""` into `NaN + 0 = NaN`).
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_string_or_number_add(a: f64, b: f64) -> f64 {
    // #5525 hot fast path: both operands are plain IEEE-754 doubles (not a
    // NaN-boxed string / pointer / bigint / int32 / singleton — i.e. top 16 bits
    // below the 0x7FF9 Perry tag band). Then `a + b` is exactly the spec result:
    // ToPrimitive is identity on numbers, neither side is a string (no concat),
    // neither is a BigInt or Symbol, and there are no GC pointers to root. This
    // skips the `RuntimeHandleScope` + four `root_nanbox_f64` thread-local
    // accesses (`_tlv_get_addr`) that otherwise run for *every* dynamic `+`.
    // bcryptjs's Blowfish core does ~hundreds of millions of `n += S[i]` adds on
    // `any`-typed locals whose values are always plain numbers, so this scope +
    // rooting was the single largest remaining cost after the typed-array
    // element-access fix (#5525). Canonical-NaN (0x7FF8) and negative-NaN
    // payloads from real arithmetic stay on this path and add to NaN, matching
    // IEEE semantics. Any tagged operand falls through to the full path below.
    const TAG_BAND_FLOOR: u64 = 0x7FF9_0000_0000_0000;
    let abits = a.to_bits();
    let bbits = b.to_bits();
    if (abits & 0x7FFF_0000_0000_0000) < TAG_BAND_FLOOR
        && (bbits & 0x7FFF_0000_0000_0000) < TAG_BAND_FLOOR
    {
        return a + b;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let a_handle = scope.root_nanbox_f64(a);
    let b_handle = scope.root_nanbox_f64(b);
    let a_prim = to_primitive_default_for_add(a_handle.get_nanbox_f64());
    let a_prim_handle = scope.root_nanbox_f64(a_prim);
    let b_prim = to_primitive_default_for_add(b_handle.get_nanbox_f64());
    let b_prim_handle = scope.root_nanbox_f64(b_prim);
    let a_val = JSValue::from_bits(a_prim_handle.get_nanbox_f64().to_bits());
    let b_val = JSValue::from_bits(b_prim_handle.get_nanbox_f64().to_bits());

    if is_symbol_value(a_prim_handle.get_nanbox_f64())
        || is_symbol_value(b_prim_handle.get_nanbox_f64())
    {
        throw_add_type_error(b"Cannot convert a Symbol value to a string");
    }

    // String concat takes priority: either operand being a string forces
    // ToPrimitive on the other side via the spec's "if either is a string,
    // do concat" branch. js_string_concat_value handles the
    // `string + non-string` case (it calls js_jsvalue_to_string on the
    // non-string side); we use it for both orderings by pre-coercing the
    // other operand to string via js_jsvalue_to_string when it ISN'T a
    // string.
    if a_val.is_any_string() || b_val.is_any_string() {
        let a_str = if JSValue::from_bits(a_prim_handle.get_nanbox_f64().to_bits()).is_any_string()
        {
            js_get_string_pointer_unified(a_prim_handle.get_nanbox_f64())
                as *const crate::string::StringHeader
        } else {
            js_jsvalue_to_string(a_prim_handle.get_nanbox_f64())
                as *const crate::string::StringHeader
        };
        let a_str_handle = scope.root_string_ptr(a_str);
        let b_str = if JSValue::from_bits(b_prim_handle.get_nanbox_f64().to_bits()).is_any_string()
        {
            js_get_string_pointer_unified(b_prim_handle.get_nanbox_f64())
                as *const crate::string::StringHeader
        } else {
            js_jsvalue_to_string(b_prim_handle.get_nanbox_f64())
                as *const crate::string::StringHeader
        };
        let b_str_handle = scope.root_string_ptr(b_str);
        let result = crate::string::js_string_concat(
            a_str_handle.get_raw_const_ptr(),
            b_str_handle.get_raw_const_ptr(),
        );
        return f64::from_bits(JSValue::string_ptr(result).bits());
    }

    // BigInt: same as js_dynamic_add. Neither operand is a string here
    // (the concat branch above already handled that), so a mixed
    // BigInt/Number `+` throws TypeError just like Node.
    if both_bigint_or_throw(
        a_prim_handle.get_nanbox_f64(),
        b_prim_handle.get_nanbox_f64(),
    ) {
        return dynamic_bigint_binary_op_from_handles(
            &scope,
            &a_prim_handle,
            &b_prim_handle,
            crate::bigint::js_bigint_add,
        );
    }

    // Both numeric — coerce non-numbers (booleans, null, undefined) the
    // same way the static fallback path did. An int32 must be DECODED to its
    // plain-double value, not passed through as its NaN-boxed bits: the bits
    // are a NaN, so `a_num + b_num` would propagate the int32 tag through fadd
    // and return the boxed operand (a better-sqlite3 integer column `n` made
    // `n + 100 === n`). Plain numbers already hold their value in the f64.
    let a_num = if a_val.is_number() {
        a_prim_handle.get_nanbox_f64()
    } else if a_val.is_int32() {
        a_val.as_int32() as f64
    } else {
        crate::builtins::js_number_coerce(a_prim_handle.get_nanbox_f64())
    };
    let b_num = if b_val.is_number() {
        b_prim_handle.get_nanbox_f64()
    } else if b_val.is_int32() {
        b_val.as_int32() as f64
    } else {
        crate::builtins::js_number_coerce(b_prim_handle.get_nanbox_f64())
    };
    a_num + b_num
}

/// Dynamic subtract: BigInt - BigInt if either operand is BigInt, else f64 - f64.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_sub(a: f64, b: f64) -> f64 {
    if both_bigint_or_throw(a, b) {
        return dynamic_bigint_binary_op(a, b, crate::bigint::js_bigint_sub);
    }
    numify_arith_operand(a) - numify_arith_operand(b)
}

/// Dynamic divide: BigInt / BigInt if either operand is BigInt, else f64 / f64.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_div(a: f64, b: f64) -> f64 {
    if both_bigint_or_throw(a, b) {
        return dynamic_bigint_binary_op(a, b, crate::bigint::js_bigint_div);
    }
    numify_arith_operand(a) / numify_arith_operand(b)
}

/// Dynamic modulo: BigInt % BigInt if either operand is BigInt, else f64 % f64.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_mod(a: f64, b: f64) -> f64 {
    if both_bigint_or_throw(a, b) {
        return dynamic_bigint_binary_op(a, b, crate::bigint::js_bigint_mod);
    }
    // Float modulo: a - trunc(a / b) * b
    let a = numify_arith_operand(a);
    let b = numify_arith_operand(b);
    a - (a / b).trunc() * b
}

/// Dynamic negate: -BigInt if operand is BigInt, else -f64.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_neg(a: f64) -> f64 {
    let a_val = JSValue::from_bits(a.to_bits());
    if a_val.is_bigint() {
        let scope = crate::gc::RuntimeHandleScope::new();
        let a_handle = scope.root_bigint_ptr(a_val.as_bigint_ptr());
        let result = crate::bigint::js_bigint_neg(
            a_handle.get_raw_const_ptr::<crate::bigint::BigIntHeader>(),
        );
        return js_nanbox_bigint(result as i64);
    }
    -a
}

/// Dynamic bitwise NOT: `~BigInt` stays BigInt, otherwise use JS ToInt32.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_bitnot(a: f64) -> f64 {
    let a_val = JSValue::from_bits(a.to_bits());
    if a_val.is_bigint() {
        let scope = crate::gc::RuntimeHandleScope::new();
        let a_handle = scope.root_bigint_ptr(a_val.as_bigint_ptr());
        let result = crate::bigint::js_bigint_not(
            a_handle.get_raw_const_ptr::<crate::bigint::BigIntHeader>(),
        );
        return js_nanbox_bigint(result as i64);
    }
    // Apply ToNumber first so that ~"3", ~true, ~new Boolean(true), etc.
    // coerce correctly before the ToInt32 truncation.
    let a_num = crate::builtins::js_number_coerce(a);
    // ES ToInt32: NaN, ±0, ±Infinity all map to 0; finite values truncate
    // toward zero and reduce modulo 2^32. `as i64` is NOT equivalent — it
    // saturates for |v| >= 2^63, so `~(1e20)` came out as `~(-1)` == 0
    // instead of -1661992961 (CodeRabbit review on #5466).
    let a_i32 = if a_num.is_nan() || !a_num.is_finite() {
        0i32
    } else {
        a_num.trunc().rem_euclid(4294967296.0) as u32 as i32
    };
    (!a_i32) as f64
}

/// Dynamic right shift: BigInt >> if either operand is BigInt, else i32 >> for numbers.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_shr(a: f64, b: f64) -> f64 {
    if both_bigint_or_throw(a, b) {
        return dynamic_bigint_binary_op(a, b, crate::bigint::js_bigint_shr);
    }
    // JS ToInt32: f64 -> i64 -> i32 (wrapping), NOT f64 -> i32 (saturating).
    // Rust `f64 as i32` saturates at i32::MAX for values >= 2^31, but JS wraps.
    let ai = (a as i64) as i32;
    let bi = ((b as i64) as i32) & 0x1f;
    (ai >> bi) as f64
}

/// Dynamic left shift: BigInt << if either operand is BigInt, else i32 << for numbers.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_shl(a: f64, b: f64) -> f64 {
    if both_bigint_or_throw(a, b) {
        return dynamic_bigint_binary_op(a, b, crate::bigint::js_bigint_shl);
    }
    // JS ToInt32: f64 -> i64 -> i32 (wrapping), NOT f64 -> i32 (saturating).
    let ai = (a as i64) as i32;
    let bi = ((b as i64) as i32) & 0x1f;
    (ai << bi) as f64
}

/// Dynamic bitwise AND: BigInt & if either operand is BigInt, else i32 & for numbers.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_bitand(a: f64, b: f64) -> f64 {
    if both_bigint_or_throw(a, b) {
        return dynamic_bigint_binary_op(a, b, crate::bigint::js_bigint_and);
    }
    // JS ToInt32: f64 -> i64 -> i32 (wrapping), NOT f64 -> i32 (saturating).
    (((a as i64) as i32) & ((b as i64) as i32)) as f64
}

/// Dynamic bitwise OR: BigInt | if either operand is BigInt, else i32 | for numbers.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_bitor(a: f64, b: f64) -> f64 {
    if both_bigint_or_throw(a, b) {
        return dynamic_bigint_binary_op(a, b, crate::bigint::js_bigint_or);
    }
    // JS ToInt32: f64 -> i64 -> i32 (wrapping), NOT f64 -> i32 (saturating).
    (((a as i64) as i32) | ((b as i64) as i32)) as f64
}

/// Dynamic bitwise XOR: BigInt ^ if either operand is BigInt, else i32 ^ for numbers.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_bitxor(a: f64, b: f64) -> f64 {
    if both_bigint_or_throw(a, b) {
        return dynamic_bigint_binary_op(a, b, crate::bigint::js_bigint_xor);
    }
    // JS ToInt32: f64 -> i64 -> i32 (wrapping), NOT f64 -> i32 (saturating).
    (((a as i64) as i32) ^ ((b as i64) as i32)) as f64
}

/// Dynamic exponentiation: `BigInt ** BigInt` when both operands are BigInt
/// (#2908), else numeric `Math.pow`. A mixed BigInt/Number `**` throws
/// TypeError; a negative BigInt exponent throws RangeError (handled inside
/// `js_bigint_pow`).
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_pow(a: f64, b: f64) -> f64 {
    if both_bigint_or_throw(a, b) {
        return dynamic_bigint_binary_op(a, b, crate::bigint::js_bigint_pow);
    }
    crate::math::js_math_pow(a, b)
}

/// Dynamic unsigned right shift. BigInts have no `>>>` operator in
/// ECMAScript, so any BigInt operand throws TypeError (#2908); otherwise
/// numeric ToUint32 `>>>`.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_ushr(a: f64, b: f64) -> f64 {
    let a_big = JSValue::from_bits(a.to_bits()).is_bigint();
    let b_big = JSValue::from_bits(b.to_bits()).is_bigint();
    if a_big || b_big {
        let msg = b"BigInts have no unsigned right shift, use >> instead";
        let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err = crate::error::js_typeerror_new(s);
        crate::exception::js_throw(js_nanbox_pointer(err as i64));
    }
    // JS ToUint32 then logical shift, count masked to 5 bits.
    let ai = (a as i64) as u32;
    let bi = ((b as i64) as i32 as u32) & 0x1f;
    (ai >> bi) as f64
}

#[used]
static KEEP_DYNAMIC_POW: unsafe extern "C" fn(f64, f64) -> f64 = js_dynamic_pow;
#[used]
static KEEP_DYNAMIC_USHR: unsafe extern "C" fn(f64, f64) -> f64 = js_dynamic_ushr;
#[used]
static KEEP_DYNAMIC_BITNOT: unsafe extern "C" fn(f64) -> f64 = js_dynamic_bitnot;
#[used]
static KEEP_TO_NUMERIC: unsafe extern "C" fn(f64) -> f64 = js_to_numeric;
#[used]
static KEEP_NUMERIC_STEP: unsafe extern "C" fn(f64, i32) -> f64 = js_numeric_step;

#[cfg(test)]
mod tests {
    use super::*;

    fn int32(n: i32) -> f64 {
        f64::from_bits(JSValue::int32(n).bits())
    }

    // An int32-tagged operand (e.g. a better-sqlite3 integer column) must be
    // decoded to its value before the f64 op; otherwise its NaN-boxed bits
    // propagate through the FPU and the op returns the boxed operand. Pre-fix,
    // `n + 100 === n` for an integer DB column.
    #[test]
    fn dynamic_arith_decodes_int32_operands() {
        unsafe {
            assert_eq!(js_dynamic_add(int32(42), 100.0), 142.0);
            assert_eq!(js_dynamic_add(100.0, int32(42)), 142.0);
            assert_eq!(js_dynamic_sub(int32(42), 1.0), 41.0);
            assert_eq!(js_dynamic_mul(int32(42), 2.0), 84.0);
            assert_eq!(js_dynamic_div(int32(84), 2.0), 42.0);
            assert_eq!(js_dynamic_mod(int32(43), 10.0), 3.0);
        }
    }

    #[test]
    fn dynamic_string_or_number_add_decodes_int32() {
        unsafe {
            assert_eq!(js_dynamic_string_or_number_add(int32(42), 100.0), 142.0);
            assert_eq!(js_dynamic_string_or_number_add(100.0, int32(42)), 142.0);
            assert_eq!(js_dynamic_string_or_number_add(int32(2), int32(3)), 5.0);
        }
    }
}
