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
            // `LengthOfArrayLike` is `ToLength(ToNumber(Get(O, "length")))`.
            // A non-numeric `length` (e.g. `length: true` → 1, `length: "2"` →
            // 2) must be ToNumber-coerced first — the raw NaN-boxed bool/string
            // bits would otherwise read as NaN → 0.
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
        Some(PtrKind::IndexedNative) => {
            crate::object::js_object_get_index_polymorphic(b as i64, k as f64)
        }
        Some(PtrKind::Object) => {
            let v = crate::object::js_object_get_index_polymorphic(b as i64, k as f64);
            // `js_object_get_index_polymorphic` walks own + explicit-`setPrototypeOf`
            // chains, but not the *default* `Object.prototype` for a plain `{}`
            // object. The generic Array algorithms `Get(O, k)` per spec, so an
            // index living on `Object.prototype[k]` must resolve. Fall back to a
            // chain read only when the direct read missed.
            if v.to_bits() == TAG_UNDEFINED {
                object_get_property_chain((b & 0x0000_FFFF_FFFF_FFFF) as usize, k)
            } else {
                v
            }
        }
        Some(PtrKind::Exotic) | None => undef(),
    }
}

/// `Get(O, ToString(k))` over the prototype chain, reading the first own
/// indexed data property found. Companion to `object_has_property_chain`; used
/// only as a fallback when the polymorphic index read misses (so the default
/// `Object.prototype` is consulted for an inherited indexed element).
fn object_get_property_chain(obj_ptr: usize, k: i64) -> f64 {
    let s = k.to_string();
    let key = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    let mut cur = obj_ptr;
    for _ in 0..1000 {
        if cur == 0 {
            return undef();
        }
        let cur_val = f64::from_bits(crate::value::js_nanbox_pointer(cur as i64).to_bits());
        let key_val = f64::from_bits(JSValue::string_ptr(key).bits());
        if crate::object::js_object_has_own(cur_val, key_val).to_bits() == TAG_TRUE {
            return crate::object::js_object_get_index_polymorphic(cur as i64, k as f64);
        }
        let proto_bits = match crate::object::prototype_chain::object_static_prototype(cur) {
            Some(bits) => bits,
            None => match unsafe {
                crate::object::prototype_chain::default_object_prototype_for_owner(cur)
            } {
                Some(bits) => bits,
                None => return undef(),
            },
        };
        if proto_bits == TAG_NULL {
            return undef();
        }
        let top16 = proto_bits >> 48;
        let next = if top16 == 0x7FFD {
            (proto_bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else if top16 == 0 && proto_bits > 0x10000 {
            proto_bits as usize
        } else {
            return undef();
        };
        if next == cur {
            return undef();
        }
        cur = next;
    }
    undef()
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
            object_has_property_chain((b & 0x0000_FFFF_FFFF_FFFF) as usize, key_val)
        }
        // Typed arrays / buffers are dense over their length.
        Some(PtrKind::IndexedNative) => k < al_length(recv),
        Some(PtrKind::Exotic) | None => false,
    }
}

/// `[[HasProperty]]` (ECMA-262 §10.1.7) over the recorded prototype chain for an
/// ordinary heap object. `js_object_has_property` (the `in` operator backend)
/// only scans the receiver's *own* keys for the plain-object case, so the
/// generic Array algorithms (which spec on `HasProperty`) missed inherited
/// indexed properties — e.g. an element living on `Object.prototype[k]` or a
/// `proto` from `Object.create(proto)`. Walk own-then-prototype here so
/// `Array.prototype.forEach.call(obj, …)` visits inherited indices.
fn object_has_property_chain(obj_ptr: usize, key_val: f64) -> bool {
    let mut cur = obj_ptr;
    // Bound the walk to guard against user-induced prototype cycles.
    for _ in 0..1000 {
        if cur == 0 {
            return false;
        }
        let cur_val = f64::from_bits(crate::value::js_nanbox_pointer(cur as i64).to_bits());
        if crate::object::js_object_has_own(cur_val, key_val).to_bits() == TAG_TRUE {
            return true;
        }
        // Advance to the recorded [[Prototype]] (explicit `setPrototypeOf`) or
        // the default `Object.prototype` for a plain `{}` object.
        let proto_bits = match crate::object::prototype_chain::object_static_prototype(cur) {
            Some(bits) => bits,
            None => match unsafe {
                crate::object::prototype_chain::default_object_prototype_for_owner(cur)
            } {
                Some(bits) => bits,
                None => return false,
            },
        };
        if proto_bits == TAG_NULL {
            return false;
        }
        let top16 = proto_bits >> 48;
        let next = if top16 == 0x7FFD {
            (proto_bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else if top16 == 0 && proto_bits > 0x10000 {
            proto_bits as usize
        } else {
            return false;
        };
        if next == cur {
            return false;
        }
        cur = next;
    }
    false
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
    // Spec order: LengthOfArrayLike(O) is read *before* the IsCallable(cb)
    // check (ECMA-262 §23.1.3.*), so a `length` getter fires even when the
    // callback is missing/non-callable. Read `len` first, then validate `cb`.
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
    // Spec order: LengthOfArrayLike(O) is read *before* the IsCallable(cb)
    // check (ECMA-262 §23.1.3.*), so a `length` getter fires even when the
    // callback is missing/non-callable. Read `len` first, then validate `cb`.
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
    // Spec order: LengthOfArrayLike(O) is read *before* the IsCallable(cb)
    // check (ECMA-262 §23.1.3.*), so a `length` getter fires even when the
    // callback is missing/non-callable. Read `len` first, then validate `cb`.
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
    // Spec order: LengthOfArrayLike(O) is read *before* the IsCallable(cb)
    // check (ECMA-262 §23.1.3.*), so a `length` getter fires even when the
    // callback is missing/non-callable. Read `len` first, then validate `cb`.
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
    // Spec order: LengthOfArrayLike(O) is read *before* the IsCallable(cb)
    // check (ECMA-262 §23.1.3.*), so a `length` getter fires even when the
    // callback is missing/non-callable. Read `len` first, then validate `cb`.
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
    // Spec order: LengthOfArrayLike(O) is read *before* the IsCallable(cb)
    // check (ECMA-262 §23.1.3.*), so a `length` getter fires even when the
    // callback is missing/non-callable. Read `len` first, then validate `cb`.
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
    // Spec order: LengthOfArrayLike(O) is read *before* the IsCallable(cb)
    // check (ECMA-262 §23.1.3.*), so a `length` getter fires even when the
    // callback is missing/non-callable. Read `len` first, then validate `cb`.
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
    // Spec order: LengthOfArrayLike(O) is read *before* the IsCallable(cb)
    // check (ECMA-262 §23.1.3.*), so a `length` getter fires even when the
    // callback is missing/non-callable. Read `len` first, then validate `cb`.
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
    // Spec order: LengthOfArrayLike(O) is read *before* the IsCallable(cb)
    // check (ECMA-262 §23.1.3.*), so a `length` getter fires even when the
    // callback is missing/non-callable. Read `len` first, then validate `cb`.
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
    // Spec order: LengthOfArrayLike(O) is read *before* the IsCallable(cb)
    // check (ECMA-262 §23.1.3.*), so a `length` getter fires even when the
    // callback is missing/non-callable. Read `len` first, then validate `cb`.
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
    // Spec order: LengthOfArrayLike(O) is read *before* the IsCallable(cb)
    // check (ECMA-262 §23.1.3.*), so a `length` getter fires even when the
    // callback is missing/non-callable. Read `len` first, then validate `cb`.
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
        let n = crate::array::search::from_index_to_integer(from);
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
        let n = crate::array::search::from_index_to_integer(from);
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
        let n = crate::array::search::from_index_to_integer(from);
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

// ---------------------------------------------------------------------------
// Generic array-like MUTATORS over a plain-object receiver (#4742 follow-up).
//
// `Array.prototype.{pop,shift,push,unshift,reverse,splice}` are intentionally
// generic (ECMA-262 §23.1.3) — they operate on `O = ToObject(this)` with live
// `Get`/`Set`/`Delete`/`HasProperty` and a writable `length`. Perry's dense
// fast paths assume a real `ArrayHeader`; when the receiver is a plain object
// (a stored `obj.pop = Array.prototype.pop; obj.pop()` borrow, or
// `Array.prototype.pop.call(obj, …)`), the dense path read the object's
// `ObjectHeader` words as an `ArrayHeader` and corrupted/crashed
// (`TypeError: Cannot convert object to primitive value`).
//
// These helpers run the spec algorithm by mutating the *original* receiver
// object in place via the polymorphic index get/set/delete and a `length`
// property write. They are dispatched from `js_native_call_method` only when
// the receiver classifies as a plain `Object` (never a real array / typed
// array / buffer / primitive), so the hot real-array paths are untouched.
// ---------------------------------------------------------------------------

/// `Set(O, ToString(k), v, true)` for an array-like object receiver.
fn al_set(recv: f64, k: i64, v: f64) {
    crate::object::js_object_set_index_polymorphic(recv.to_bits() as i64, k as f64, v);
}

/// `DeletePropertyOrThrow(O, ToString(k))` for an array-like object receiver.
fn al_delete(recv: f64, k: i64) {
    let raw = (recv.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut crate::object::ObjectHeader;
    crate::object::js_object_delete_dynamic(raw, k as f64);
}

/// `Set(O, "length", len, true)` for an array-like object receiver.
fn al_set_length(recv: f64, len: i64) {
    let raw = (recv.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut crate::object::ObjectHeader;
    let key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
    crate::object::js_object_set_field_by_name(raw, key, len as f64);
}

/// `ToIntegerOrInfinity(v)` as an `f64` (NaN → 0; ±Infinity preserved).
fn to_integer_or_infinity(v: f64) -> f64 {
    if v.is_nan() {
        0.0
    } else if v.is_infinite() {
        v
    } else {
        v.trunc()
    }
}

/// Resolve a relative index argument (`splice` start) to an absolute,
/// clamped `[0, len]` index.
fn relative_index(v: f64, len: i64) -> i64 {
    let n = to_integer_or_infinity(v);
    if n < 0.0 {
        let r = len as f64 + n;
        if r < 0.0 {
            0
        } else {
            r as i64
        }
    } else if n > len as f64 {
        len
    } else {
        n as i64
    }
}

#[inline]
fn arg_at(args_ptr: *const f64, args_len: usize, i: usize) -> f64 {
    if i < args_len && !args_ptr.is_null() {
        unsafe { *args_ptr.add(i) }
    } else {
        undef()
    }
}

/// If `arr` points to a *plain* object (an object literal — `GC_TYPE_OBJECT`
/// with `class_id == 0` or an anonymous shape id), return it NaN-boxed as a
/// receiver value, else `None`. Used by the dense `Array.prototype` mutator
/// entry points to detect a borrowed array-like receiver (`obj.pop =
/// Array.prototype.pop; obj.pop()`, whose thunk calls the dense helper with the
/// object pointer) and route it to the spec-generic engine. Real arrays / typed
/// arrays / buffers / class instances return `None` and keep the dense path.
pub fn plain_object_value(arr: *const ArrayHeader) -> Option<f64> {
    let recv = f64::from_bits(JSValue::pointer(arr as *const u8).bits());
    if !matches!(classify_pointer(recv), Some(PtrKind::Object)) {
        return None;
    }
    let class_id = crate::object::js_object_get_class_id(arr as *const crate::object::ObjectHeader);
    if class_id != 0 && !crate::object::is_anon_shape_class_id(class_id) {
        return None;
    }
    Some(recv)
}

/// `Array.prototype.pop` over an array-like object receiver.
pub(crate) fn object_pop(recv: f64) -> f64 {
    let len = al_length(recv);
    if len <= 0 {
        al_set_length(recv, 0);
        return undef();
    }
    let new_len = len - 1;
    let element = al_get(recv, new_len);
    al_delete(recv, new_len);
    al_set_length(recv, new_len);
    element
}

/// `Array.prototype.shift` over an array-like object receiver.
pub(crate) fn object_shift(recv: f64) -> f64 {
    let len = al_length(recv);
    if len <= 0 {
        al_set_length(recv, 0);
        return undef();
    }
    let first = al_get(recv, 0);
    for k in 1..len {
        if al_has(recv, k) {
            al_set(recv, k - 1, al_get(recv, k));
        } else {
            al_delete(recv, k - 1);
        }
    }
    al_delete(recv, len - 1);
    al_set_length(recv, len - 1);
    first
}

/// `Array.prototype.push` over an array-like object receiver. Returns the new
/// length.
fn object_push(recv: f64, args_ptr: *const f64, args_len: usize) -> f64 {
    let len = al_length(recv);
    for i in 0..args_len {
        al_set(recv, len + i as i64, arg_at(args_ptr, args_len, i));
    }
    let new_len = len + args_len as i64;
    al_set_length(recv, new_len);
    new_len as f64
}

/// `Array.prototype.unshift` over an array-like object receiver. Returns the
/// new length.
fn object_unshift(recv: f64, args_ptr: *const f64, args_len: usize) -> f64 {
    let len = al_length(recv);
    let count = args_len as i64;
    if count > 0 {
        // Move existing elements up by `count`, high index first so we don't
        // clobber not-yet-moved slots.
        let mut k = len;
        while k > 0 {
            let from = k - 1;
            let to = from + count;
            if al_has(recv, from) {
                al_set(recv, to, al_get(recv, from));
            } else {
                al_delete(recv, to);
            }
            k -= 1;
        }
        for j in 0..count {
            al_set(recv, j, arg_at(args_ptr, args_len, j as usize));
        }
    }
    let new_len = len + count;
    al_set_length(recv, new_len);
    new_len as f64
}

/// `Array.prototype.reverse` over an array-like object receiver. Returns the
/// receiver.
fn object_reverse(recv: f64) -> f64 {
    let len = al_length(recv);
    let middle = len / 2;
    let mut lower = 0;
    while lower < middle {
        let upper = len - 1 - lower;
        let lower_exists = al_has(recv, lower);
        let upper_exists = al_has(recv, upper);
        let lower_val = al_get(recv, lower);
        let upper_val = al_get(recv, upper);
        match (lower_exists, upper_exists) {
            (true, true) => {
                al_set(recv, lower, upper_val);
                al_set(recv, upper, lower_val);
            }
            (false, true) => {
                al_set(recv, lower, upper_val);
                al_delete(recv, upper);
            }
            (true, false) => {
                al_delete(recv, lower);
                al_set(recv, upper, lower_val);
            }
            (false, false) => {}
        }
        lower += 1;
    }
    recv
}

/// `Array.prototype.splice` over an array-like object receiver. Returns a fresh
/// plain array of the removed elements (holes preserved).
fn object_splice(recv: f64, args_ptr: *const f64, args_len: usize) -> f64 {
    let len = al_length(recv);
    let actual_start = relative_index(arg_at(args_ptr, args_len, 0), len);
    let delete_count = if args_len == 0 {
        0
    } else if args_len == 1 {
        len - actual_start
    } else {
        let dc = to_integer_or_infinity(arg_at(args_ptr, args_len, 1));
        dc.max(0.0).min((len - actual_start) as f64) as i64
    };
    // Removed elements -> fresh plain array (holes preserved).
    let removed = js_array_alloc_with_length(delete_count.max(0) as u32);
    let removed_elems =
        unsafe { (removed as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64 };
    for k in 0..delete_count {
        let from = actual_start + k;
        if al_has(recv, from) {
            let v = al_get(recv, from);
            unsafe {
                ptr::write(removed_elems.add(k as usize), v);
                note_array_slot(removed, k as usize, v.to_bits());
            }
        }
    }
    let item_count = if args_len > 2 { args_len - 2 } else { 0 } as i64;
    if item_count < delete_count {
        // Shift the tail down to close the gap.
        let mut k = actual_start;
        while k < len - delete_count {
            let from = k + delete_count;
            let to = k + item_count;
            if al_has(recv, from) {
                al_set(recv, to, al_get(recv, from));
            } else {
                al_delete(recv, to);
            }
            k += 1;
        }
        // Delete the now-vacated trailing slots.
        let mut k = len;
        while k > len - delete_count + item_count {
            al_delete(recv, k - 1);
            k -= 1;
        }
    } else if item_count > delete_count {
        // Open a gap by shifting the tail up.
        let mut k = len - delete_count;
        while k > actual_start {
            let from = k + delete_count - 1;
            let to = k + item_count - 1;
            if al_has(recv, from) {
                al_set(recv, to, al_get(recv, from));
            } else {
                al_delete(recv, to);
            }
            k -= 1;
        }
    }
    // Write the inserted items.
    for j in 0..item_count {
        al_set(
            recv,
            actual_start + j,
            arg_at(args_ptr, args_len, 2 + j as usize),
        );
    }
    al_set_length(recv, len - delete_count + item_count);
    nanbox_arr(removed)
}

/// Run a generic `Array.prototype` mutator on `recv` for the reified prototype
/// method thunks (`Array.prototype.pop`, etc.). A real-array receiver routes to
/// the dense helpers; a plain array-like object routes to the spec-generic
/// engine; any other receiver yields `undefined`. `recv` is the call-site
/// `this` (IMPLICIT_THIS) the thunk read.
pub fn array_proto_mutator(recv: f64, method: &str, args_ptr: *const f64, args_len: usize) -> f64 {
    let arr = as_real_array(recv);
    if !arr.is_null() {
        return unsafe { real_array_mutator(arr, method, args_ptr, args_len) };
    }
    run_object_mutator(recv, method, args_ptr, args_len).unwrap_or_else(undef)
}

#[inline]
fn arg_or_undef(args_ptr: *const f64, args_len: usize, i: usize) -> f64 {
    if i < args_len && !args_ptr.is_null() {
        unsafe { *args_ptr.add(i) }
    } else {
        undef()
    }
}

/// Dense-array branch of [`array_proto_mutator`]. Reuses the existing dense
/// runtime helpers (matching the `js_native_call_method` array arms).
unsafe fn real_array_mutator(
    arr: *mut ArrayHeader,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    match method {
        "pop" => crate::array::js_array_pop_f64(arr),
        "shift" => crate::array::js_array_shift_f64(arr),
        "reverse" => {
            crate::array::js_array_reverse(arr);
            nanbox_arr(arr)
        }
        "push" => {
            let mut a = arr;
            for i in 0..args_len {
                a = crate::array::js_array_push_f64(a, arg_or_undef(args_ptr, args_len, i));
            }
            crate::array::js_array_length(a) as f64
        }
        "unshift" => {
            if args_len == 0 || args_ptr.is_null() {
                crate::array::js_array_length(arr) as f64
            } else {
                let r = crate::array::js_array_unshift_variadic(arr, args_ptr, args_len as u32);
                crate::array::js_array_length(r) as f64
            }
        }
        "splice" => {
            let arg_i32 = |i: usize| -> i32 {
                let v = arg_or_undef(args_ptr, args_len, i);
                if v.is_nan() || v.is_infinite() {
                    0
                } else {
                    v as i32
                }
            };
            let start = if args_len >= 1 { arg_i32(0) } else { 0 };
            let delete_count = if args_len == 0 {
                0
            } else if args_len == 1 {
                i32::MAX
            } else {
                arg_i32(1)
            };
            let items: Vec<f64> = if args_len > 2 && !args_ptr.is_null() {
                std::slice::from_raw_parts(args_ptr.add(2), args_len - 2).to_vec()
            } else {
                Vec::new()
            };
            let items_ptr = if items.is_empty() {
                ptr::null()
            } else {
                items.as_ptr()
            };
            let mut out_arr: *mut ArrayHeader = ptr::null_mut();
            let deleted = crate::array::js_array_splice(
                arr,
                start,
                delete_count,
                items_ptr,
                items.len() as u32,
                &mut out_arr,
            );
            nanbox_arr(deleted)
        }
        _ => undef(),
    }
}

/// Classify the own `method_name` slot of an array-like receiver: `Absent`
/// (no callable), `UserMethod` (a genuine user closure/function), or
/// `BorrowedBuiltin` (a `BOUND_METHOD` closure — `obj.pop = Array.prototype.pop`).
/// Only `Absent` and `BorrowedBuiltin` route to the generic engine; a borrowed
/// builtin, dispatched normally, binds the wrong receiver (its captured
/// `Array.prototype`) and loops, so the engine must run on the real receiver.
enum OwnSlot {
    Absent,
    UserMethod,
    BorrowedBuiltin,
}

fn classify_own_slot(v: f64) -> OwnSlot {
    let jv = JSValue::from_bits(v.to_bits());
    if !jv.is_pointer() {
        return OwnSlot::Absent;
    }
    let c = jv.as_pointer::<ClosureHeader>();
    if c.is_null() {
        return OwnSlot::Absent;
    }
    let fp = crate::closure::get_valid_func_ptr(c);
    if fp.is_null() {
        OwnSlot::Absent
    } else if fp == crate::closure::BOUND_METHOD_FUNC_PTR {
        OwnSlot::BorrowedBuiltin
    } else {
        OwnSlot::UserMethod
    }
}

/// Dispatch a generic `Array.prototype` mutator over an array-like receiver.
///
/// Returns `Some(result)` only when `object` is a plain heap object / closure
/// (classified `Object`) — i.e. NOT a real array, typed array, buffer, exotic
/// cell, or primitive — so the dense real-array fast paths in
/// `js_native_call_method` keep their existing behavior. The caller routes
/// `pop` / `shift` / `push` / `unshift` / `reverse` / `splice` here before the
/// dense array arms that would otherwise read the object as an `ArrayHeader`.
pub fn try_object_arraylike_mutator(
    object: f64,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if !matches!(classify_pointer(object), Some(PtrKind::Object)) {
        return None;
    }
    // Restrict to *plain* objects — object literals (`class_id == 0` or an
    // anonymous shape id). A real user class instance (`class Stack { push(){…} }`)
    // owns a registered class id; hijacking its same-named method would be a
    // regression, so leave those to the normal vtable dispatch.
    let raw = (object.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const crate::object::ObjectHeader;
    let class_id = crate::object::js_object_get_class_id(raw);
    if class_id != 0 && !crate::object::is_anon_shape_class_id(class_id) {
        return None;
    }
    // Fire the generic engine when the own `method_name` slot is absent (the
    // `Array.prototype.<m>.call(obj, …)` borrow, dispatched by name) or holds a
    // borrowed builtin method (`obj.pop = Array.prototype.pop`, whose normal
    // dispatch binds its captured `Array.prototype` and loops). A genuine user
    // method (`{ push(x) {…} }`) is left to the normal dispatch.
    let key = crate::string::js_string_from_bytes(method.as_ptr(), method.len() as u32);
    let own = crate::object::js_object_get_field_by_name_f64(raw, key);
    if matches!(classify_own_slot(own), OwnSlot::UserMethod) {
        return None;
    }
    run_object_mutator(object, method, args_ptr, args_len)
}

/// Run a generic `Array.prototype` mutator over a *plain-object* receiver
/// (object literal / anonymous shape — never a real array / typed array /
/// buffer / class instance). Returns `None` for any other receiver so the
/// caller keeps its existing behavior. Unlike [`try_object_arraylike_mutator`]
/// this applies NO own-property gate — callers (e.g. the bound-method dispatch
/// for a borrowed builtin) have already established that the array algorithm
/// must run on `recv`.
pub fn run_object_mutator(
    recv: f64,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if !matches!(classify_pointer(recv), Some(PtrKind::Object)) {
        return None;
    }
    let raw = (recv.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const crate::object::ObjectHeader;
    let class_id = crate::object::js_object_get_class_id(raw);
    if class_id != 0 && !crate::object::is_anon_shape_class_id(class_id) {
        return None;
    }
    let result = match method {
        "pop" => object_pop(recv),
        "shift" => object_shift(recv),
        "push" => object_push(recv, args_ptr, args_len),
        "unshift" => object_unshift(recv, args_ptr, args_len),
        "reverse" => object_reverse(recv),
        "splice" => object_splice(recv, args_ptr, args_len),
        _ => return None,
    };
    Some(result)
}
