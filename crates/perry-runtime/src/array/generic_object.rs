//! Generic `Array.prototype` MUTATORS over an *array-like object* receiver
//! (`pop` / `shift` / `push` / `unshift` / `reverse` / `splice` / `sort` /
//! `concat`), plus the `Set` / `Delete` / `length`-write helpers they need.
//!
//! Split verbatim out of the sibling [`super::generic`] module to keep that
//! file under the 2000-line gate; the read-only generic methods
//! (`forEach` / `map` / `filter` / …), the shared `ToObject` / `Get` /
//! `HasProperty` helpers, and the dynamic-dispatch entry points stay there.

use super::generic::{
    al_get, al_has, al_length, as_real_array, nanbox_arr, proxy_string_key, real_array_mutator,
    to_object, undef,
};
use super::*;
use crate::closure::ClosureHeader;
use crate::value::TAG_UNDEFINED;
use std::ptr;

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
    if crate::proxy::js_proxy_is_proxy(recv) != 0 {
        crate::proxy::js_proxy_set(recv, proxy_string_key(k), v);
        return;
    }
    crate::object::js_object_set_index_polymorphic(recv.to_bits() as i64, k as f64, v);
}

/// `DeletePropertyOrThrow(O, ToString(k))` for an array-like object receiver.
fn al_delete(recv: f64, k: i64) {
    if crate::proxy::js_proxy_is_proxy(recv) != 0 {
        crate::proxy::js_proxy_delete(recv, proxy_string_key(k));
        return;
    }
    let raw = (recv.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut crate::object::ObjectHeader;
    crate::object::js_object_delete_dynamic(raw, k as f64);
}

/// `Set(O, "length", len, true)` for an array-like object receiver. An own
/// `length` ACCESSOR fires its setter — and throws TypeError when there is
/// none (getter-only `length`, test262 splice/S15.4.4.12_A6.1_T3), matching
/// `Set(..., true)` on a non-writable slot.
fn al_set_length(recv: f64, len: i64) {
    let raw_addr = (recv.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
    if let Some(acc) = crate::object::get_accessor_descriptor(raw_addr, "length") {
        if acc.set != 0 {
            unsafe { crate::object::invoke_accessor_setter(acc.set, recv, len as f64) };
            return;
        }
        crate::collection_iter::throw_type_error(
            "Cannot set property length of object which has only a getter",
        );
    }
    // An object-LITERAL `get length()` lives in the anon-shape class vtable,
    // not the defineProperty descriptor table — a getter with no setter makes
    // `Set(O, "length", ..., true)` throw (test262 splice/S15.4.4.12_A6.1_T3).
    {
        let raw = raw_addr as *const crate::object::ObjectHeader;
        let class_id = crate::object::js_object_get_class_id(raw);
        if class_id != 0 {
            if let Some((getter, setter)) =
                crate::object::class_own_accessor_ptrs(class_id, "length")
            {
                if setter == 0 && getter != 0 {
                    crate::collection_iter::throw_type_error(
                        "Cannot set property length of object which has only a getter",
                    );
                }
            }
        }
    }
    // `Set(O, "length", …, true)` (Throw=true) must throw a TypeError when the
    // set fails: a frozen array, an array/object whose `length` is a
    // non-writable data property (`defineProperty(o,"length",{writable:false})`
    // or a function's intrinsic `length`), or a frozen plain object. The
    // by-name setter below silently no-ops in those cases (PutValue's
    // non-strict fall-through), so detect the failure up front and throw.
    if al_length_write_would_fail(recv, raw_addr) {
        crate::collection_iter::throw_type_error(
            "Cannot assign to read only property 'length' of object",
        );
    }
    let raw = raw_addr as *mut crate::object::ObjectHeader;
    let key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
    crate::object::js_object_set_field_by_name(raw, key, len as f64);
}

/// True when `Set(recv, "length", …)` would fail (and so must throw under
/// `Throw=true`): a frozen array, a non-writable `length` data property, or a
/// callable receiver (a function's `length` is non-writable by spec).
fn al_length_write_would_fail(recv: f64, raw_addr: usize) -> bool {
    // Frozen array: `length` is non-writable after `Object.freeze`.
    let arr = as_real_array(recv);
    if !arr.is_null() {
        if crate::array::array_is_frozen(arr) {
            return true;
        }
        return crate::object::get_property_attrs(raw_addr, "length")
            .map(|a| !a.writable())
            .unwrap_or(false);
    }
    // Non-array receivers: a recorded non-writable `length` descriptor
    // (`defineProperty(o,"length",{writable:false})`) or a callable receiver
    // whose intrinsic `length` is non-writable.
    if crate::object::get_property_attrs(raw_addr, "length")
        .map(|a| !a.writable())
        .unwrap_or(false)
    {
        return true;
    }
    crate::closure::is_closure_ptr(raw_addr)
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
pub(super) fn arg_at(args_ptr: *const f64, args_len: usize, i: usize) -> f64 {
    if i < args_len && !args_ptr.is_null() {
        unsafe { *args_ptr.add(i) }
    } else {
        undef()
    }
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
pub(super) fn object_push(recv: f64, args_ptr: *const f64, args_len: usize) -> f64 {
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
pub(super) fn object_unshift(recv: f64, args_ptr: *const f64, args_len: usize) -> f64 {
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
pub(super) fn object_reverse(recv: f64) -> f64 {
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
pub(crate) fn object_splice(recv: f64, args_ptr: *const f64, args_len: usize) -> f64 {
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
    // Removed elements -> fresh plain array (holes preserved). ArrayCreate
    // throws RangeError for a count ≥ 2^32 (splice/create-non-array-invalid-len).
    if delete_count > u32::MAX as i64 {
        crate::array::array_length_range_error();
    }
    let removed = js_array_alloc_with_length(delete_count.max(0) as u32);
    let removed_elems =
        unsafe { (removed as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64 };
    for k in 0..delete_count {
        let from = actual_start + k;
        if al_has(recv, from) {
            let v = al_get(recv, from);
            unsafe {
                // GC_STORE_AUDIT(BARRIERED): note_array_slot below re-stores this slot with the barrier.
                ptr::write(removed_elems.add(k as usize), v);
                note_array_slot(removed, k as usize, v.to_bits());
            }
        }
    }
    let item_count = args_len.saturating_sub(2) as i64;
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

/// `Array.prototype.sort` over an array-like (non-real-array) receiver:
/// ECMA-262 SortIndexedProperties with holes skipped — collect via
/// `HasProperty`/`Get`, sort (undefined trailing, never compared), write back
/// via `Set` and `Delete` the trailing range. Returns the receiver.
/// `cmp_validated` is the already-validated comparator (null = default sort).
pub(crate) fn object_sort(recv: f64, cmp_validated: *const ClosureHeader) -> f64 {
    let cmp = if cmp_validated.is_null() {
        None
    } else {
        Some(super::sort::ComparatorCall::new(cmp_validated))
    };
    let len = al_length(recv);
    unsafe {
        // Root BOTH the receiver value and the collection temp for the whole
        // protocol: `al_has`/`al_get`/`al_set` fire user accessors (and the
        // comparator runs inside `sort_rooted_values`) — any of them can
        // allocate and sweep or move either object, so every raw pointer is
        // re-derived from its rooted handle after each such call.
        let scope = crate::gc::RuntimeHandleScope::new();
        let recv_handle = scope.root_nanbox_f64(recv);
        let temp = super::sort::RootedArrayElems::new(
            &scope,
            js_array_alloc_with_length(len.clamp(0, u32::MAX as i64) as u32),
        );
        let mut count = 0usize;
        let mut undef_count = 0usize;
        for j in 0..len {
            if al_has(recv_handle.get_nanbox_f64(), j) {
                let v = al_get(recv_handle.get_nanbox_f64(), j);
                if v.to_bits() == TAG_UNDEFINED {
                    undef_count += 1;
                } else {
                    temp.set(count, v);
                    count += 1;
                }
            }
        }
        (*temp.arr()).length = count as u32;
        rebuild_array_layout(temp.arr());
        let _ = super::sort::sort_rooted_values(temp.arr(), count, cmp);
        for j in 0..count {
            al_set(recv_handle.get_nanbox_f64(), j as i64, temp.get(j));
        }
        for j in count..count + undef_count {
            al_set(recv_handle.get_nanbox_f64(), j as i64, undef());
        }
        for j in (count + undef_count) as i64..len {
            al_delete(recv_handle.get_nanbox_f64(), j);
        }
        recv_handle.get_nanbox_f64()
    }
}

/// `Array.prototype.concat` over a non-real-array receiver: the receiver is
/// the first concat element (spread only when `@@isConcatSpreadable` says so —
/// a plain object/wrapper lands as a single element), then each argument is
/// appended with the usual spreadability rules.
pub(super) fn object_concat(recv: f64, args_ptr: *const f64, args_len: usize) -> f64 {
    let mut result = super::from_concat::append_concat_arg(js_array_alloc(0), recv);
    for i in 0..args_len {
        result = super::from_concat::append_concat_arg(result, arg_at(args_ptr, args_len, i));
    }
    nanbox_arr(result)
}

/// Generic `Array.prototype.sort.call(receiver, comparator?)` entry
/// (#4597 extension): ToObject + route real arrays to the dense/spec sort,
/// everything else through the array-like engine. Returns the receiver.
#[no_mangle]
pub extern "C" fn js_arraylike_sort(recv: f64, comparator: f64) -> f64 {
    // Spec step 1: comparator must be undefined or callable — BEFORE ToObject.
    let cmp = crate::array::js_validate_array_comparator(comparator) as *const ClosureHeader;
    let o = to_object(recv);
    let arr = as_real_array(o);
    if !arr.is_null() {
        let r = crate::array::js_array_sort_with_comparator(arr, cmp);
        return nanbox_arr(r);
    }
    object_sort(o, cmp)
}

/// Generic `Array.prototype.concat.call(receiver, ...items)` entry.
#[no_mangle]
pub extern "C" fn js_arraylike_concat(recv: f64, args_ptr: *const f64, count: i32) -> f64 {
    let o = to_object(recv);
    let arr = as_real_array(o);
    if !arr.is_null() {
        let r = crate::array::js_array_concat_variadic(arr, args_ptr, count.max(0));
        return nanbox_arr(r);
    }
    object_concat(o, args_ptr, count.max(0) as usize)
}

/// Generic `Array.prototype.splice.call(receiver, start?, deleteCount?, ...items)`.
#[no_mangle]
pub extern "C" fn js_arraylike_splice(recv: f64, args_ptr: *const f64, count: i32) -> f64 {
    let o = to_object(recv);
    let count = count.max(0) as usize;
    let arr = as_real_array(o);
    if !arr.is_null() {
        return unsafe { real_array_mutator(arr, "splice", args_ptr, count) };
    }
    object_splice(o, args_ptr, count)
}

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_ARRAYLIKE_SORT: extern "C" fn(f64, f64) -> f64 = js_arraylike_sort;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_ARRAYLIKE_VARIADIC: [extern "C" fn(f64, *const f64, i32) -> f64; 2] =
    [js_arraylike_concat, js_arraylike_splice];
