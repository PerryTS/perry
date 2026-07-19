//! Host-runtime bridging for the #6559 interpreter.
//!
//! Everything the interpreter does to real values routes through the SAME
//! generic helpers compiled code uses for dynamic (unknown-shape) receivers:
//! `js_get_property` / `js_dyn_index_get/set` for member access,
//! `js_native_call_method` for `obj.m(...)` (builtin prototypes, host
//! closures stored as fields — with `this` bound to the receiver — plus
//! `call`/`apply`/`bind`), `js_native_call_value` for plain calls, and
//! `js_new_function_construct` for `new` on host constructors. That is what
//! makes the bridging bidirectional for free: an interpreted closure IS a
//! runtime closure, so host code dispatches into it through the exact same
//! towers.

use std::cell::RefCell;
use std::collections::HashMap;

use super::{root_get, root_push, roots_truncate};

thread_local! {
    /// Property name → its canonical INTERNED `StringHeader`. The interpreter's
    /// member read (`get_member`) went through `js_get_property`, which
    /// allocates a fresh, NON-interned key every call — and a non-interned key
    /// makes the object getter's inline-cache fast lane bail to the full slow
    /// scan (the lane gates on `GC_FLAG_INTERNED`). For property-heavy codegen
    /// (TypeBox's `value.providers` / `value.name` / … millions of reads) that
    /// is the dominant `get_field_by_name` cost (#6693). Interning each name
    /// once (in the longlived arena, then registered canonical) lets repeated
    /// reads hit the read-plan IC. Rooted by `scan_member_key_cache_mut`.
    static MEMBER_KEY_CACHE: RefCell<HashMap<Box<str>, *const crate::string::StringHeader>> =
        RefCell::new(HashMap::new());
}

/// Canonical interned key for `name` (cached). First use allocates it in the
/// longlived arena and interns it; the canonical pointer is reused thereafter.
fn interned_member_key(name: &str) -> *const crate::string::StringHeader {
    if let Some(ptr) = MEMBER_KEY_CACHE.with(|c| c.borrow().get(name).copied()) {
        return ptr;
    }
    let ll = crate::string::js_string_from_bytes_longlived(name.as_ptr(), name.len() as u32);
    let hash = crate::object::key_content_hash(ll);
    let canonical = crate::string::js_string_intern(ll, hash);
    MEMBER_KEY_CACHE.with(|c| {
        c.borrow_mut().insert(name.into(), canonical);
    });
    canonical
}

/// Mark + rewrite the cached interned member keys (they may be canonicals in a
/// moving arena, unlike the always-longlived env keys, so the rewrite matters).
pub(super) fn scan_member_key_cache_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    MEMBER_KEY_CACHE.with(|c| {
        for ptr in c.borrow_mut().values_mut() {
            visitor.visit_tagged_raw_const_ptr_slot(ptr, crate::value::STRING_TAG);
        }
    });
}

/// #6693 fast member read (gated by `PERRY_DYN_FAST_SCOPE`): for a plain heap
/// object receiver, read the field with a cached INTERNED key so the object
/// getter's inline-cache fast lane engages, skipping the generic
/// `js_dynamic_object_get_property` receiver-dispatch cascade + per-call key
/// allocation. Returns `None` (→ generic path) for any receiver that isn't a
/// plain arena `GC_TYPE_OBJECT` — the generic read is authoritative for
/// strings / arrays / handles / proxies / closures / errors. For a plain
/// object the generic path ends in the very same `js_object_get_field_by_name_f64`,
/// so results are identical.
fn fast_object_get(base: f64, name: &str) -> Option<f64> {
    let jv = crate::value::JSValue::from_bits(base.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let addr = crate::value::js_nanbox_get_pointer(base) as usize;
    if crate::value::addr_class::is_handle_band(addr) {
        return None;
    }
    let h = unsafe { crate::value::addr_class::try_read_gc_header(addr) }?;
    if h.obj_type != crate::gc::GC_TYPE_OBJECT {
        return None;
    }
    let key = interned_member_key(name);
    let v = crate::object::js_object_get_field_by_name_f64(
        addr as *const crate::object::ObjectHeader,
        key,
    );
    Some(f64::from_bits(v.to_bits()))
}

pub(crate) fn undefined() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

pub(crate) fn null() -> f64 {
    f64::from_bits(crate::value::TAG_NULL)
}

pub(crate) fn boolean(b: bool) -> f64 {
    f64::from_bits(crate::value::JSValue::bool(b).bits())
}

pub(crate) fn is_undefined(v: f64) -> bool {
    crate::value::JSValue::from_bits(v.to_bits()).is_undefined()
}

pub(crate) fn is_nullish(v: f64) -> bool {
    let jv = crate::value::JSValue::from_bits(v.to_bits());
    jv.is_undefined() || jv.is_null()
}

pub(crate) fn truthy(v: f64) -> bool {
    crate::value::js_is_truthy(v) != 0
}

pub(crate) fn make_string(s: &str) -> f64 {
    let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    crate::value::js_nanbox_string(ptr as i64)
}

/// Decode any runtime string (heap or SSO) into an owned Rust String.
pub(crate) fn read_string(v: f64) -> Option<String> {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    match crate::string::str_bytes_from_jsvalue(v, &mut scratch) {
        Some((ptr, len)) if !ptr.is_null() || len == 0 => {
            let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
            Some(String::from_utf8_lossy(bytes).into_owned())
        }
        _ => None,
    }
}

/// JS ToString (spec coercion, matches the runtime's own paths).
pub(crate) fn to_string_value(v: f64) -> f64 {
    let ptr = crate::value::js_jsvalue_to_string(v);
    crate::value::js_nanbox_string(ptr as i64)
}

pub(crate) fn to_rust_string(v: f64) -> String {
    let v_idx = root_push(v);
    let s = to_string_value(root_get(v_idx));
    roots_truncate(v_idx);
    read_string(s).unwrap_or_default()
}

// ── throws ─────────────────────────────────────────────────────────────────

fn throw_error_kind(kind: u32, message: &str) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_error_new_kind_with_options(
        kind,
        msg,
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

pub(crate) fn throw_type_error(message: &str) -> ! {
    throw_error_kind(crate::error::ERROR_KIND_TYPE_ERROR, message)
}

pub(crate) fn throw_reference_error(message: &str) -> ! {
    throw_error_kind(crate::error::ERROR_KIND_REFERENCE_ERROR, message)
}

pub(crate) fn throw_range_error(message: &str) -> ! {
    throw_error_kind(crate::error::ERROR_KIND_RANGE_ERROR, message)
}

pub(crate) fn throw_syntax_error(message: &str) -> ! {
    throw_error_kind(crate::error::ERROR_KIND_SYNTAX_ERROR, message)
}

/// The diagnostic contract of #6559: anything outside the interpreter subset
/// throws a TypeError that NAMES the construct, so gaps met in the wild show
/// up as actionable errors, never as silent miscomputation.
pub(crate) fn throw_unsupported(construct: &str) -> ! {
    throw_type_error(&format!(
        "perry runtime interpreter (new Function, #6559): unsupported construct: {construct}"
    ))
}

// ── property access ────────────────────────────────────────────────────────

/// `base.name` — generic named property read on any value.
pub(crate) fn get_member(base: f64, name: &str) -> f64 {
    if is_nullish(base) {
        throw_type_error(&format!(
            "Cannot read properties of {} (reading '{name}')",
            if is_undefined(base) { "undefined" } else { "null" }
        ));
    }
    // `.length` mirrors codegen's dynamic-receiver lowering: the dedicated
    // helper handles EVERY receiver flavor — heap arrays (incl. forwarded /
    // lazy), the STATIC arrays compiled inline literals become (no GC
    // header, so the generic property path misses them — ajv's
    // `data3.length` on a schema-provided array), strings/SSO, buffers,
    // typed arrays, closures, and plain objects carrying an own `length`.
    if name == "length" {
        return crate::value::js_value_length_f64(base);
    }
    if super::fast_scope_enabled() {
        if let Some(v) = fast_object_get(base, name) {
            return v;
        }
    }
    unsafe { crate::value::js_get_property(base, name.as_ptr() as i64, name.len() as i64) }
}

/// `base[key]` — generic computed read (arrays, strings, objects, maps…).
pub(crate) fn get_index(base: f64, key: f64) -> f64 {
    crate::value::js_dyn_index_get(base, key)
}

/// `base[key] = value` (also used for `base.name = value` with a string key).
pub(crate) fn set_index(base: f64, key: f64, value: f64) {
    crate::value::js_dyn_index_set(base, key, value);
}

pub(crate) fn set_member(base: f64, name: &str, value: f64) {
    let base_idx = root_push(base);
    let value_idx = root_push(value);
    let key = make_string(name);
    set_index(root_get(base_idx), key, root_get(value_idx));
    roots_truncate(base_idx);
}

// ── calls ──────────────────────────────────────────────────────────────────

/// Plain call `f(args…)` with an explicit `this` (undefined for ordinary
/// calls). Mirrors the runtime's own `Function.prototype.call` plumbing:
/// arm the implicit-`this` slot, rebind explicit-`this` closures, dispatch
/// through the arity-aware value-call tower, restore.
pub(crate) fn call_function(callee: f64, this: f64, args: &[f64]) -> f64 {
    let prev = crate::object::js_implicit_this_set(this);
    let prev_idx = root_push(prev);
    let result = unsafe {
        crate::closure::js_native_call_value(callee, args.as_ptr(), args.len())
    };
    let result_idx = root_push(result);
    crate::object::js_implicit_this_set(root_get(prev_idx));
    let result = root_get(result_idx);
    roots_truncate(prev_idx);
    result
}

/// Method call `base.name(args…)` — the generic dispatch tower compiled code
/// uses (builtin prototype methods, host/interpreted closures stored as
/// fields with `this` bound to `base`, `call`/`apply`/`bind`, Map/Set/RegExp
/// methods, …).
pub(crate) fn call_method(base: f64, name: &str, args: &[f64]) -> f64 {
    unsafe {
        crate::object::js_native_call_method(
            base,
            name.as_ptr() as *const i8,
            name.len(),
            args.as_ptr(),
            args.len(),
        )
    }
}

/// Computed method call `base[key](args…)`.
pub(crate) fn call_method_value(base: f64, key: f64, args: &[f64]) -> f64 {
    unsafe { crate::object::js_native_call_method_value(base, key, args.as_ptr(), args.len()) }
}

/// `new callee(args…)` on a host value (find-my-way's `new NullObject()`
/// where `NullObject` arrives as a Function-constructor parameter).
pub(crate) fn construct(callee: f64, args: &[f64]) -> f64 {
    unsafe { crate::object::js_new_function_construct(callee, args.as_ptr(), args.len()) }
}

// ── globals ────────────────────────────────────────────────────────────────

/// Look `name` up on the real `globalThis` (Math, JSON, Array, isNaN, …).
pub(crate) fn global_lookup(name: &str) -> f64 {
    crate::object::js_get_global_this_builtin_value(name.as_ptr(), name.len())
}

// ── operators ──────────────────────────────────────────────────────────────

pub(crate) fn loose_equals(a: f64, b: f64) -> bool {
    crate::value::js_jsvalue_loose_equals(a, b) != 0
}

pub(crate) fn strict_equals(a: f64, b: f64) -> bool {
    crate::value::js_jsvalue_equals(a, b) != 0
}

/// Relational compare. `js_jsvalue_compare` returns -1/0/1, or 2 for
/// incomparable operands (undefined/null/NaN-ish) which makes every
/// relational operator false — matching the compiled lowering.
pub(crate) fn compare(a: f64, b: f64) -> i32 {
    // A NaN NUMBER operand must defeat `<=` / `>=` (compare returns 0 for
    // NaN vs NaN, which would make `NaN <= NaN` true).
    let a_jv = crate::value::JSValue::from_bits(a.to_bits());
    let b_jv = crate::value::JSValue::from_bits(b.to_bits());
    if (a_jv.is_number() && a.is_nan() && !a_jv.is_int32())
        || (b_jv.is_number() && b.is_nan() && !b_jv.is_int32())
    {
        return 2;
    }
    crate::value::js_jsvalue_compare(a, b)
}

pub(crate) fn typeof_value(v: f64) -> f64 {
    let ptr = crate::builtins::js_value_typeof(v);
    crate::value::js_nanbox_string(ptr as i64)
}

pub(crate) fn instanceof(value: f64, ctor: f64) -> bool {
    truthy(crate::object::js_instanceof_dynamic(value, ctor))
}

pub(crate) fn in_operator(key: f64, obj: f64) -> bool {
    truthy(crate::object::js_in_operator(obj, key))
}

/// JS ToNumber for unary `+` / `-` / numeric coercions the dynamic-arith
/// helpers don't already cover.
pub(crate) fn to_number(v: f64) -> f64 {
    let jv = crate::value::JSValue::from_bits(v.to_bits());
    if jv.is_number() {
        return v;
    }
    if jv.is_int32() {
        return jv.as_int32() as f64;
    }
    // `x * 1` runs the runtime's own ToNumber coercion for strings/bools/
    // null/undefined/objects without duplicating it here.
    unsafe { crate::value::js_dynamic_mul(v, 1.0_f64) }
}

pub(crate) fn make_number(n: f64) -> f64 {
    f64::from_bits(crate::value::JSValue::number(n).bits())
}

/// ToInt32 for bitwise ops.
pub(crate) fn to_int32(v: f64) -> i32 {
    let n = to_number(v);
    if !n.is_finite() {
        return 0;
    }
    n as i64 as i32
}

pub(crate) fn make_regex(pattern: &str, flags: &str) -> f64 {
    #[cfg(feature = "regex-engine")]
    {
        let pat = crate::string::js_string_from_bytes(pattern.as_ptr(), pattern.len() as u32);
        let pat_idx = root_push(crate::value::js_nanbox_string(pat as i64));
        let flg = crate::string::js_string_from_bytes(flags.as_ptr(), flags.len() as u32);
        let pat_value = root_get(pat_idx);
        let pat = crate::value::js_nanbox_get_pointer(pat_value) as *const crate::string::StringHeader;
        let re = crate::regex::js_regexp_new(pat, flg);
        roots_truncate(pat_idx);
        crate::value::js_nanbox_pointer(re as i64)
    }
    #[cfg(not(feature = "regex-engine"))]
    {
        let _ = (pattern, flags);
        throw_unsupported("regex literal (runtime built without the regex-engine feature)")
    }
}

/// `Array.isArray`-grade check used for spread/for-of fast paths.
pub(crate) fn is_array_value(v: f64) -> bool {
    let jv = crate::value::JSValue::from_bits(v.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let addr = crate::value::js_nanbox_get_pointer(v) as usize;
    if crate::value::addr_class::is_handle_band(addr) {
        return false;
    }
    match unsafe { crate::value::addr_class::try_read_gc_header(addr) } {
        Some(h) => {
            h.obj_type == crate::gc::GC_TYPE_ARRAY || h.obj_type == crate::gc::GC_TYPE_LAZY_ARRAY
        }
        None => false,
    }
}

// ── arrays ─────────────────────────────────────────────────────────────────

pub(crate) fn array_new() -> f64 {
    let arr = crate::array::js_array_alloc(4);
    crate::value::js_nanbox_pointer(arr as i64)
}

/// Append to a rooted array value (push may reallocate — the root slot is
/// updated with the returned header).
pub(crate) fn array_push_rooted(arr_idx: usize, value: f64) {
    let value_idx = root_push(value);
    let arr_ptr =
        crate::value::js_nanbox_get_pointer(root_get(arr_idx)) as *mut crate::array::ArrayHeader;
    let new_ptr = crate::array::js_array_push_f64(arr_ptr, root_get(value_idx));
    super::root_set(arr_idx, crate::value::js_nanbox_pointer(new_ptr as i64));
    roots_truncate(value_idx);
}

pub(crate) fn array_length(v: f64) -> u32 {
    let ptr = crate::value::js_nanbox_get_pointer(v) as *const crate::array::ArrayHeader;
    if ptr.is_null() {
        0
    } else {
        crate::array::js_array_length(ptr)
    }
}

pub(crate) fn array_get(v: f64, index: u32) -> f64 {
    let ptr = crate::value::js_nanbox_get_pointer(v) as *const crate::array::ArrayHeader;
    if ptr.is_null() {
        undefined()
    } else {
        crate::array::js_array_get_f64(ptr, index)
    }
}

// ── objects ────────────────────────────────────────────────────────────────

pub(crate) fn object_new() -> f64 {
    let obj = crate::object::js_object_alloc(0, 0);
    crate::value::js_nanbox_pointer(obj as i64)
}

/// Own enumerable keys of any object-ish value, as a runtime array value.
pub(crate) fn own_keys(v: f64) -> f64 {
    let ptr = crate::value::js_nanbox_get_pointer(v);
    let arr = unsafe { crate::value::js_dynamic_object_keys(ptr) };
    crate::value::js_nanbox_pointer(arr as i64)
}

/// `for (… in obj)` key list (proto-chain enumerable string keys, matching
/// the runtime's compiled for-in lowering).
pub(crate) fn for_in_keys(v: f64) -> f64 {
    let arr = crate::object::js_for_in_keys_value(v);
    crate::value::js_nanbox_pointer(arr as i64)
}
