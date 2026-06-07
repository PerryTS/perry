//! Generic `Array.prototype` methods over *array-like* receivers (#4597).
//!
//! ECMA-262 §23.1.3 specifies each `Array.prototype` method as operating on
//! `O = ToObject(this)` with `len = LengthOfArrayLike(O)` and indexed
//! `Get(O, k)` / `HasProperty(O, k)` — i.e. the algorithms are *generic* over
//! any object that exposes a `length` and indexed properties (plain objects,
//! `arguments`, functions, strings, typed arrays …), not just genuine
//! `Array` exotic objects.
//!
//! Perry's primary (hot) array methods in the sibling modules are specialised
//! to a real `ArrayHeader` receiver for speed. Those paths are untouched. The
//! functions here are reached *only* from the explicit
//! `Array.prototype.<m>.call(receiver, …)` / `.apply(…)` (and bound-local)
//! forms — lowered to `Expr::ArrayLikeMethod` in the HIR — where `receiver`
//! may be any value. They operate on the **original** receiver value so that:
//!   * the callback observes the original object as its 3rd argument
//!     (`(value, index, O)`), per spec, rather than a materialised clone, and
//!   * element reads are live `Get(O, k)` (data props, getters, function
//!     expandos), and holes are honoured via `HasProperty(O, k)`.
//!
//! Receiver coercion mirrors `ToObject`:
//!   * `undefined` / `null` → `TypeError`,
//!   * a real array → fast direct element access,
//!   * a string → length is the code-unit count, indices are 1-char strings,
//!   * any other heap object/closure → `Get`/`HasProperty` via the polymorphic
//!     object helpers,
//!   * a bare number/boolean → boxed into its `Number`/`Boolean` wrapper object
//!     (so inherited prototype `length`/indices are read and the callback's
//!     3rd argument is `instanceof Number`/`Boolean`),
//!   * a symbol/bigint → no indexed properties → an empty array-like (length 0).

use super::*;
use crate::closure::{js_closure_call3, ClosureHeader};
use crate::value::{JSValue, TAG_HOLE, TAG_NULL, TAG_TRUE, TAG_UNDEFINED};
use std::ptr;

#[inline(always)]
fn undef() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

#[inline(always)]
fn boxed_bool(b: bool) -> f64 {
    f64::from_bits(if b { TAG_TRUE } else { crate::value::TAG_FALSE })
}

#[inline(always)]
fn nanbox_arr(arr: *mut ArrayHeader) -> f64 {
    f64::from_bits(JSValue::pointer(arr as *const u8).bits())
}

#[inline(always)]
fn top16(bits: u64) -> u64 {
    bits >> 48
}

/// `ToObject(recv)` (ECMA-262 §7.1.18). `undefined` / `null` throw a
/// `TypeError`; a bare number / boolean primitive is boxed into its wrapper
/// object so the generic algorithms below (and the callback's 3rd argument)
/// observe an object with the right prototype chain — e.g.
/// `Array.prototype.map.call(false, fn)` must read length/indices inherited
/// from `Boolean.prototype` and pass a `Boolean` wrapper to `fn`. Strings keep
/// their dedicated code-unit path; symbols / bigints (no indexed properties)
/// are returned as-is and read as an empty array-like.
fn to_object(recv: f64) -> f64 {
    let b = recv.to_bits();
    if b == TAG_UNDEFINED || b == TAG_NULL {
        let msg = b"Cannot convert undefined or null to object";
        let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err = crate::error::js_typeerror_new(s);
        crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
    }
    // Already a heap object / array / closure, or a string (handled specially).
    if top16(b) == 0x7FFD || is_string_value(b) {
        return recv;
    }
    if b == TAG_TRUE || b == crate::value::TAG_FALSE {
        return crate::builtins::js_boxed_boolean_new(recv);
    }
    // Numbers: real f64 values (`is_number`) and INT32-tagged small integers
    // (0x7FFE), which `is_number` excludes because the tag sits in the
    // string/special band.
    if top16(b) == 0x7FFE || JSValue::from_bits(b).is_number() {
        return crate::builtins::js_boxed_number_new(recv);
    }
    // Symbol / BigInt — no indexed properties; treated as an empty array-like.
    recv
}

/// Validate `cb` is callable, returning its `ClosureHeader*` (throws a
/// `TypeError` otherwise, reusing the array-method renderer for parity with the
/// specialised paths).
#[inline]
fn callable(cb: f64) -> *const ClosureHeader {
    crate::array::js_validate_array_map_callback(0, cb) as *const ClosureHeader
}

/// `ToIntegerOrInfinity` clamped to a non-negative `i64` length
/// (`LengthOfArrayLike`'s `ToLength`).
#[inline]
fn to_length(v: f64) -> i64 {
    if v.is_nan() {
        return 0;
    }
    let n = v.trunc();
    if n <= 0.0 {
        0
    } else if n > 9_007_199_254_740_991.0 {
        9_007_199_254_740_991
    } else {
        n as i64
    }
}

/// Genuine `ArrayHeader*` if `recv` is a real (or lazy) array, else null.
///
/// Objects and arrays share `POINTER_TAG`, so the GC-header `obj_type` byte —
/// not `clean_arr_ptr` alone — must gate the array fast path: `clean_arr_ptr`
/// accepts an object pointer whose leading `ObjectHeader` words happen to pass
/// its `length <= capacity` bound, then `(*arr).length` / the element buffer
/// read `field_count` / inline slots as garbage (see `normalize_array_receiver`).
#[inline]
fn as_real_array(recv: f64) -> *mut ArrayHeader {
    let b = recv.to_bits();
    if top16(b) != 0x7FFD {
        return ptr::null_mut();
    }
    let raw = (b & 0x0000_FFFF_FFFF_FFFF) as usize;
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return ptr::null_mut();
    }
    let obj_type = unsafe {
        let hdr = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        (*hdr).obj_type
    };
    if obj_type == crate::gc::GC_TYPE_ARRAY || obj_type == crate::gc::GC_TYPE_LAZY_ARRAY {
        return clean_arr_ptr_mut(raw as *mut ArrayHeader);
    }
    ptr::null_mut()
}

#[inline]
fn is_string_value(bits: u64) -> bool {
    let t = top16(bits);
    // Heap string (0x7FFF) or small-string-optimised inline string (0x7FF9).
    t == 0x7FFF || t == 0x7FF9
}

/// `LengthOfArrayLike(ToObject(recv))`.
/// Classification of a (non-array, non-string) `POINTER_TAG` receiver, used to
/// pick a *safe* property-access path. Exotic GC cells (Date, Map, Set, BigInt,
/// Error, …) must NOT be dereferenced as an `ObjectHeader` (that SIGBUSes) nor
/// passed to `js_object_get_index_polymorphic` (whose final fallback reads them
/// as an `ArrayHeader`). Typed arrays / buffers carry NO GC header, so they are
/// detected via their registries *before* the GC-header byte is read.
enum PtrKind {
    /// Plain object or function/closure — `length`/index reads via the object
    /// helpers (prototype-chain aware) are safe.
    Object,
    /// Typed array or buffer — `js_value_length_f64` /
    /// `js_object_get_index_polymorphic` handle these by registry.
    IndexedNative,
    /// Date / Map / Set / Symbol / BigInt / … — no safe array-like access;
    /// treated as an empty array-like.
    Exotic,
}

fn classify_pointer(recv: f64) -> Option<PtrKind> {
    let b = recv.to_bits();
    if top16(b) != 0x7FFD {
        return None;
    }
    let raw = (b & 0x0000_FFFF_FFFF_FFFF) as usize;
    // Typed arrays / buffers are `std::alloc`-backed (no GC header) — probe
    // their registries before any GC-header read.
    if crate::buffer::is_registered_buffer(raw)
        || crate::typedarray::lookup_typed_array_kind(raw).is_some()
    {
        return Some(PtrKind::IndexedNative);
    }
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return Some(PtrKind::Exotic);
    }
    let obj_type = unsafe {
        let hdr = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        (*hdr).obj_type
    };
    if obj_type == crate::gc::GC_TYPE_OBJECT || obj_type == crate::gc::GC_TYPE_CLOSURE {
        Some(PtrKind::Object)
    } else {
        Some(PtrKind::Exotic)
    }
}

/// `LengthOfArrayLike(ToObject(recv))`.
fn al_length(recv: f64) -> i64 {
    let arr = as_real_array(recv);
    if !arr.is_null() {
        return unsafe { (*arr).length as i64 };
    }
    let b = recv.to_bits();
    if is_string_value(b) {
        let sh = crate::value::js_jsvalue_to_string(recv);
        if sh.is_null() {
            return 0;
        }
        return crate::string::js_string_length(sh) as i64;
    }
    match classify_pointer(recv) {
        Some(PtrKind::Object) => {
            // Plain object / function: read the `length` property (its absence
            // ToLength-coerces to 0). Safe — guaranteed `GC_TYPE_OBJECT/CLOSURE`.
            let key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
            let len_val = crate::object::js_object_get_field_by_name_f64(
                (b & 0x0000_FFFF_FFFF_FFFF) as *const crate::object::ObjectHeader,
                key,
            );
            // `LengthOfArrayLike` is `ToLength(Get(O, "length"))`, and `ToLength`
            // begins with `ToNumber`. The raw property value may be a string
            // (`length: "2"`), boolean, `null`, etc.; without coercion those
            // NaN-boxed bit patterns are themselves NaN floats and `to_length`
            // would collapse them to 0. Run `ToNumber` first so e.g.
            // `length: "0x0002"` → 2.
            to_length(crate::builtins::js_number_coerce(len_val))
        }
        // Typed arrays / buffers expose a real length via the safe dispatcher.
        Some(PtrKind::IndexedNative) => to_length(crate::value::js_value_length_f64(recv)),
        // Exotic cells / bare primitives → empty array-like.
        Some(PtrKind::Exotic) | None => 0,
    }
}

/// `Get(ToObject(recv), k)` (returns `undefined` for absent/out-of-range).
fn al_get(recv: f64, k: i64) -> f64 {
    let arr = as_real_array(recv);
    if !arr.is_null() {
        if k < 0 {
            return undef();
        }
        return js_array_get_f64(arr, k as u32);
    }
    let b = recv.to_bits();
    if is_string_value(b) {
        return crate::object::js_object_get_index_polymorphic(b as i64, k as f64);
    }
    match classify_pointer(recv) {
        // `js_object_get_index_polymorphic` is safe for objects/closures and
        // for typed arrays / buffers (handled at its top).
        Some(PtrKind::Object) | Some(PtrKind::IndexedNative) => {
            crate::object::js_object_get_index_polymorphic(b as i64, k as f64)
        }
        Some(PtrKind::Exotic) | None => undef(),
    }
}

/// `HasProperty(ToObject(recv), k)`.
fn al_has(recv: f64, k: i64) -> bool {
    if k < 0 {
        return false;
    }
    let arr = as_real_array(recv);
    if !arr.is_null() {
        unsafe {
            if k >= (*arr).length as i64 {
                return false;
            }
            let el = *((arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64)
                .add(k as usize);
            return el.to_bits() != TAG_HOLE;
        }
    }
    let b = recv.to_bits();
    if is_string_value(b) {
        return k < al_length(recv);
    }
    match classify_pointer(recv) {
        Some(PtrKind::Object) => {
            let s = k.to_string();
            let key = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
            let key_val = f64::from_bits(JSValue::string_ptr(key).bits());
            crate::object::js_object_has_property(recv, key_val).to_bits() == TAG_TRUE
        }
        // Typed arrays / buffers are dense over their length.
        Some(PtrKind::IndexedNative) => k < al_length(recv),
        Some(PtrKind::Exotic) | None => false,
    }
}

/// RAII-ish guard binding the callback `this` (the optional `thisArg`) for the
/// duration of a generic iteration, restoring the previous binding on drop.
struct ThisGuard(f64);
impl ThisGuard {
    fn new(this_arg: f64) -> Self {
        ThisGuard(crate::object::js_implicit_this_set(this_arg))
    }
}
impl Drop for ThisGuard {
    fn drop(&mut self) {
        crate::object::js_implicit_this_set(self.0);
    }
}

// ---------------------------------------------------------------------------
// Callback iteration methods. The callback receives `(value, index, O)` with
// `O` the *original* receiver value; `this_arg` binds the callback's `this`.
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn js_arraylike_forEach(recv: f64, cb: f64, this_arg: f64) -> f64 {
    let recv = to_object(recv);
    // Spec order: `LengthOfArrayLike` (step 2) runs *before* the `IsCallable`
    // check (step 3) — a `length` getter's side effects / throw must be
    // observed even when the callback is non-callable.
    let len = al_length(recv);
    let cb = callable(cb);
    let _g = ThisGuard::new(this_arg);
    for k in 0..len {
        if !al_has(recv, k) {
            continue;
        }
        let v = al_get(recv, k);
        js_closure_call3(cb, v, k as f64, recv);
    }
    undef()
}

#[no_mangle]
pub extern "C" fn js_arraylike_map(recv: f64, cb: f64, this_arg: f64) -> f64 {
    let recv = to_object(recv);
    // Spec order: `LengthOfArrayLike` (step 2) runs *before* the `IsCallable`
    // check (step 3) — a `length` getter's side effects / throw must be
    // observed even when the callback is non-callable.
    let len = al_length(recv);
    let cb = callable(cb);
    let result = js_array_alloc_with_length(len.max(0) as u32);
    let elems = unsafe { (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64 };
    let _g = ThisGuard::new(this_arg);
    for k in 0..len {
        if !al_has(recv, k) {
            continue; // preserve holes
        }
        let v = al_get(recv, k);
        let mapped = js_closure_call3(cb, v, k as f64, recv);
        unsafe {
            ptr::write(elems.add(k as usize), mapped);
            note_array_slot(result, k as usize, mapped.to_bits());
        }
    }
    nanbox_arr(result)
}

#[no_mangle]
pub extern "C" fn js_arraylike_filter(recv: f64, cb: f64, this_arg: f64) -> f64 {
    let recv = to_object(recv);
    // Spec order: `LengthOfArrayLike` (step 2) runs *before* the `IsCallable`
    // check (step 3) — a `length` getter's side effects / throw must be
    // observed even when the callback is non-callable.
    let len = al_length(recv);
    let cb = callable(cb);
    let mut result = js_array_alloc(0);
    let _g = ThisGuard::new(this_arg);
    for k in 0..len {
        if !al_has(recv, k) {
            continue;
        }
        let v = al_get(recv, k);
        let keep = js_closure_call3(cb, v, k as f64, recv);
        if crate::value::js_is_truthy(keep) != 0 {
            result = js_array_push_f64(result, v);
        }
    }
    nanbox_arr(result)
}

#[no_mangle]
pub extern "C" fn js_arraylike_some(recv: f64, cb: f64, this_arg: f64) -> f64 {
    let recv = to_object(recv);
    // Spec order: `LengthOfArrayLike` (step 2) runs *before* the `IsCallable`
    // check (step 3) — a `length` getter's side effects / throw must be
    // observed even when the callback is non-callable.
    let len = al_length(recv);
    let cb = callable(cb);
    let _g = ThisGuard::new(this_arg);
    for k in 0..len {
        if !al_has(recv, k) {
            continue;
        }
        let v = al_get(recv, k);
        if crate::value::js_is_truthy(js_closure_call3(cb, v, k as f64, recv)) != 0 {
            return boxed_bool(true);
        }
    }
    boxed_bool(false)
}

#[no_mangle]
pub extern "C" fn js_arraylike_every(recv: f64, cb: f64, this_arg: f64) -> f64 {
    let recv = to_object(recv);
    // Spec order: `LengthOfArrayLike` (step 2) runs *before* the `IsCallable`
    // check (step 3) — a `length` getter's side effects / throw must be
    // observed even when the callback is non-callable.
    let len = al_length(recv);
    let cb = callable(cb);
    let _g = ThisGuard::new(this_arg);
    for k in 0..len {
        if !al_has(recv, k) {
            continue;
        }
        let v = al_get(recv, k);
        if crate::value::js_is_truthy(js_closure_call3(cb, v, k as f64, recv)) == 0 {
            return boxed_bool(false);
        }
    }
    boxed_bool(true)
}

// find / findIndex / findLast / findLastIndex do NOT skip holes (spec uses
// Get, treating absent as undefined).

#[no_mangle]
pub extern "C" fn js_arraylike_find(recv: f64, cb: f64, this_arg: f64) -> f64 {
    let recv = to_object(recv);
    // Spec order: `LengthOfArrayLike` (step 2) runs *before* the `IsCallable`
    // check (step 3) — a `length` getter's side effects / throw must be
    // observed even when the callback is non-callable.
    let len = al_length(recv);
    let cb = callable(cb);
    let _g = ThisGuard::new(this_arg);
    for k in 0..len {
        let v = al_get(recv, k);
        if crate::value::js_is_truthy(js_closure_call3(cb, v, k as f64, recv)) != 0 {
            return v;
        }
    }
    undef()
}

#[no_mangle]
pub extern "C" fn js_arraylike_findIndex(recv: f64, cb: f64, this_arg: f64) -> f64 {
    let recv = to_object(recv);
    // Spec order: `LengthOfArrayLike` (step 2) runs *before* the `IsCallable`
    // check (step 3) — a `length` getter's side effects / throw must be
    // observed even when the callback is non-callable.
    let len = al_length(recv);
    let cb = callable(cb);
    let _g = ThisGuard::new(this_arg);
    for k in 0..len {
        let v = al_get(recv, k);
        if crate::value::js_is_truthy(js_closure_call3(cb, v, k as f64, recv)) != 0 {
            return k as f64;
        }
    }
    -1.0
}

#[no_mangle]
pub extern "C" fn js_arraylike_findLast(recv: f64, cb: f64, this_arg: f64) -> f64 {
    let recv = to_object(recv);
    // Spec order: `LengthOfArrayLike` (step 2) runs *before* the `IsCallable`
    // check (step 3) — a `length` getter's side effects / throw must be
    // observed even when the callback is non-callable.
    let len = al_length(recv);
    let cb = callable(cb);
    let _g = ThisGuard::new(this_arg);
    let mut k = len - 1;
    while k >= 0 {
        let v = al_get(recv, k);
        if crate::value::js_is_truthy(js_closure_call3(cb, v, k as f64, recv)) != 0 {
            return v;
        }
        k -= 1;
    }
    undef()
}

#[no_mangle]
pub extern "C" fn js_arraylike_findLastIndex(recv: f64, cb: f64, this_arg: f64) -> f64 {
    let recv = to_object(recv);
    // Spec order: `LengthOfArrayLike` (step 2) runs *before* the `IsCallable`
    // check (step 3) — a `length` getter's side effects / throw must be
    // observed even when the callback is non-callable.
    let len = al_length(recv);
    let cb = callable(cb);
    let _g = ThisGuard::new(this_arg);
    let mut k = len - 1;
    while k >= 0 {
        let v = al_get(recv, k);
        if crate::value::js_is_truthy(js_closure_call3(cb, v, k as f64, recv)) != 0 {
            return k as f64;
        }
        k -= 1;
    }
    -1.0
}

// ---------------------------------------------------------------------------
// reduce / reduceRight — accumulator, optional initial value.
// ---------------------------------------------------------------------------

fn throw_reduce_empty() -> ! {
    let msg = b"Reduce of empty array with no initial value";
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

#[no_mangle]
pub extern "C" fn js_arraylike_reduce(recv: f64, cb: f64, has_init: i32, init: f64) -> f64 {
    let recv = to_object(recv);
    // Spec order: `LengthOfArrayLike` (step 2) runs *before* the `IsCallable`
    // check (step 3) — a `length` getter's side effects / throw must be
    // observed even when the callback is non-callable.
    let len = al_length(recv);
    let cb = callable(cb);
    let mut acc = init;
    let mut k = 0i64;
    if has_init == 0 {
        // Seed from the first present element.
        loop {
            if k >= len {
                throw_reduce_empty();
            }
            if al_has(recv, k) {
                acc = al_get(recv, k);
                k += 1;
                break;
            }
            k += 1;
        }
    }
    while k < len {
        if al_has(recv, k) {
            let v = al_get(recv, k);
            acc = crate::closure::js_closure_call4(cb, acc, v, k as f64, recv);
        }
        k += 1;
    }
    acc
}

#[no_mangle]
pub extern "C" fn js_arraylike_reduceRight(recv: f64, cb: f64, has_init: i32, init: f64) -> f64 {
    let recv = to_object(recv);
    // Spec order: `LengthOfArrayLike` (step 2) runs *before* the `IsCallable`
    // check (step 3) — a `length` getter's side effects / throw must be
    // observed even when the callback is non-callable.
    let len = al_length(recv);
    let cb = callable(cb);
    let mut acc = init;
    let mut k = len - 1;
    if has_init == 0 {
        loop {
            if k < 0 {
                throw_reduce_empty();
            }
            if al_has(recv, k) {
                acc = al_get(recv, k);
                k -= 1;
                break;
            }
            k -= 1;
        }
    }
    while k >= 0 {
        if al_has(recv, k) {
            let v = al_get(recv, k);
            acc = crate::closure::js_closure_call4(cb, acc, v, k as f64, recv);
        }
        k -= 1;
    }
    acc
}

// ---------------------------------------------------------------------------
// Search methods.
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn js_arraylike_indexOf(recv: f64, value: f64, from: f64, has_from: i32) -> f64 {
    let recv = to_object(recv);
    let len = al_length(recv);
    if len == 0 {
        return -1.0;
    }
    // ToIntegerOrInfinity(fromIndex), clamped.
    let mut start = if has_from == 0 {
        0
    } else {
        let n = if from.is_nan() { 0.0 } else { from.trunc() };
        if n >= len as f64 {
            return -1.0;
        } else if n >= 0.0 {
            n as i64
        } else if n >= -(len as f64) {
            len + n as i64
        } else {
            0
        }
    };
    if start < 0 {
        start = 0;
    }
    for k in start..len {
        if !al_has(recv, k) {
            continue;
        }
        let v = al_get(recv, k);
        if crate::value::js_jsvalue_equals(v, value) == 1 {
            return k as f64;
        }
    }
    -1.0
}

#[no_mangle]
pub extern "C" fn js_arraylike_lastIndexOf(recv: f64, value: f64, from: f64, has_from: i32) -> f64 {
    let recv = to_object(recv);
    let len = al_length(recv);
    if len == 0 {
        return -1.0;
    }
    let mut start = if has_from == 0 {
        len - 1
    } else {
        let n = if from.is_nan() { 0.0 } else { from.trunc() };
        if n >= 0.0 {
            (n as i64).min(len - 1)
        } else if n >= -(len as f64) {
            len + n as i64
        } else {
            return -1.0;
        }
    };
    while start >= 0 {
        if al_has(recv, start) {
            let v = al_get(recv, start);
            if crate::value::js_jsvalue_equals(v, value) == 1 {
                return start as f64;
            }
        }
        start -= 1;
    }
    -1.0
}

#[no_mangle]
pub extern "C" fn js_arraylike_includes(recv: f64, value: f64, from: f64, has_from: i32) -> f64 {
    let recv = to_object(recv);
    let len = al_length(recv);
    if len == 0 {
        return boxed_bool(false);
    }
    let mut start = if has_from == 0 {
        0
    } else {
        let n = if from.is_nan() { 0.0 } else { from.trunc() };
        if n >= len as f64 {
            return boxed_bool(false);
        } else if n >= 0.0 {
            n as i64
        } else if n >= -(len as f64) {
            len + n as i64
        } else {
            0
        }
    };
    if start < 0 {
        start = 0;
    }
    // includes does NOT skip holes — absent indices read as undefined.
    for k in start..len {
        let v = al_get(recv, k);
        if crate::value::js_jsvalue_same_value_zero(v, value) == 1 {
            return boxed_bool(true);
        }
    }
    boxed_bool(false)
}

// ---------------------------------------------------------------------------
// at / join / slice — no callback identity concerns; materialise where it
// keeps the implementation simple (slice/join build fresh results anyway).
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn js_arraylike_at(recv: f64, index: f64) -> f64 {
    let recv = to_object(recv);
    let len = al_length(recv);
    let n = if index.is_nan() { 0.0 } else { index.trunc() };
    let mut k = n as i64;
    if k < 0 {
        k += len;
    }
    if k < 0 || k >= len {
        return undef();
    }
    al_get(recv, k)
}

/// Materialise `recv` into a fresh real array (holes preserved as `TAG_HOLE`),
/// for the delegating `join` / `slice` paths.
fn materialize(recv: f64) -> *mut ArrayHeader {
    let len = al_length(recv);
    let arr = js_array_alloc_with_length(len.max(0) as u32);
    let elems = unsafe { (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64 };
    for k in 0..len {
        if !al_has(recv, k) {
            continue; // leave the hole
        }
        let v = al_get(recv, k);
        unsafe {
            ptr::write(elems.add(k as usize), v);
            note_array_slot(arr, k as usize, v.to_bits());
        }
    }
    arr
}

#[no_mangle]
pub extern "C" fn js_arraylike_join(recv: f64, sep: f64) -> f64 {
    let recv = to_object(recv);
    let arr = materialize(recv);
    let sep_ptr = if sep.to_bits() == TAG_UNDEFINED {
        ptr::null()
    } else {
        crate::value::js_jsvalue_to_string(sep) as *const crate::string::StringHeader
    };
    let s = crate::array::js_array_join(arr, sep_ptr);
    f64::from_bits(JSValue::string_ptr(s).bits())
}

#[no_mangle]
pub extern "C" fn js_arraylike_slice(
    recv: f64,
    start: f64,
    has_start: i32,
    end: f64,
    has_end: i32,
) -> f64 {
    let recv = to_object(recv);
    let arr = materialize(recv);
    let len = unsafe { (*arr).length as i64 };
    let s = if has_start == 0 {
        0
    } else {
        clamp_index(start, len)
    };
    let e = if has_end == 0 {
        len
    } else {
        clamp_index(end, len)
    };
    let result = js_array_slice(arr, s as i32, e as i32);
    nanbox_arr(result)
}

/// ECMA-262 relative-index clamp used by `slice` (negative counts from the end,
/// `NaN`/`-Infinity` → 0, `+Infinity` → len).
fn clamp_index(v: f64, len: i64) -> i64 {
    let n = if v.is_nan() { 0.0 } else { v.trunc() };
    if n < 0.0 {
        let r = len + n as i64;
        r.max(0)
    } else if n > len as f64 {
        len
    } else {
        n as i64
    }
}

// Keep the generic entry points anchored against dead-strip in the default
// (codegen-only reference) compile path.
// Keep the generic entry points anchored against dead-strip in the default
// (codegen-only reference) compile path (see #3320 — `#[no_mangle]` alone is
// not enough once the bitcode is re-linked).
#[used]
static KEEP_ARRAYLIKE_CB: [extern "C" fn(f64, f64, f64) -> f64; 9] = [
    js_arraylike_forEach,
    js_arraylike_map,
    js_arraylike_filter,
    js_arraylike_some,
    js_arraylike_every,
    js_arraylike_find,
    js_arraylike_findIndex,
    js_arraylike_findLast,
    js_arraylike_findLastIndex,
];
#[used]
static KEEP_ARRAYLIKE_REDUCE: [extern "C" fn(f64, f64, i32, f64) -> f64; 2] =
    [js_arraylike_reduce, js_arraylike_reduceRight];
#[used]
static KEEP_ARRAYLIKE_SEARCH: [extern "C" fn(f64, f64, f64, i32) -> f64; 3] = [
    js_arraylike_indexOf,
    js_arraylike_lastIndexOf,
    js_arraylike_includes,
];
#[used]
static KEEP_ARRAYLIKE_AT: extern "C" fn(f64, f64) -> f64 = js_arraylike_at;
#[used]
static KEEP_ARRAYLIKE_JOIN: extern "C" fn(f64, f64) -> f64 = js_arraylike_join;
#[used]
static KEEP_ARRAYLIKE_SLICE: extern "C" fn(f64, f64, i32, f64, i32) -> f64 = js_arraylike_slice;
