//! Generic `Array.prototype.{pop,shift,push,unshift}` over a *value* receiver.
//!
//! These `#[no_mangle]` entry points back the explicit
//! `Array.prototype.<m>.call/apply(recv, …)` (and bound-local) lowering
//! (`Expr::ArrayLikeMethod`), where `recv` may be a primitive or a plain
//! array-like object — distinct from the hot `arr.<m>(…)` member-call paths.
//! They forward to the shared [`array_proto_mutator`] engine in the sibling
//! [`super::generic`] module: a real array routes to the dense helpers, a plain
//! array-like object to the spec-generic Get/Set/Delete engine.
//!
//! Split out of `generic.rs` to keep that file under the 2000-line gate.

use super::generic::{
    al_length, array_proto_mutator, as_real_array, is_string_value, real_array_mutator,
    run_object_mutator, to_object,
};
use std::ptr;

/// A primitive `string` receiver boxes to a `String` exotic wrapper whose
/// indexed elements and `length` are non-writable / non-configurable, so every
/// `Array.prototype` stack/queue mutator — each performs `Set(O, "length", …,
/// true)` and (for `pop`/`shift`) `DeletePropertyOrThrow` — fails with a
/// **TypeError** (ECMA-262 §23.1.3.*). Guard up front so the mutators throw
/// rather than silently no-op (test262 `{pop,push,shift,unshift}/
/// throws-with-string-receiver`, `shift/throws-when-this-value-length-is-
/// writable-false`).
#[cold]
fn throw_string_receiver_mutation() -> ! {
    crate::collection_iter::throw_type_error(
        "Cannot assign to read only property of a String object",
    );
}

#[inline]
fn guard_string_receiver(recv: f64) {
    if is_string_value(recv.to_bits()) {
        throw_string_receiver_mutation();
    }
}

#[no_mangle]
pub extern "C" fn js_arraylike_pop(recv: f64) -> f64 {
    guard_string_receiver(recv);
    array_proto_mutator(recv, "pop", ptr::null(), 0)
}

#[no_mangle]
pub extern "C" fn js_arraylike_shift(recv: f64) -> f64 {
    guard_string_receiver(recv);
    array_proto_mutator(recv, "shift", ptr::null(), 0)
}

#[no_mangle]
pub extern "C" fn js_arraylike_push(recv: f64, args_ptr: *const f64, count: i32) -> f64 {
    guard_string_receiver(recv);
    let n = count.max(0) as usize;
    let arr = as_real_array(recv);
    if !arr.is_null() {
        return unsafe { real_array_mutator(arr, "push", args_ptr, n) };
    }
    if let Some(r) = run_object_mutator(recv, "push", args_ptr, n) {
        return r;
    }
    pushlike_primitive_result(recv, n)
}

#[no_mangle]
pub extern "C" fn js_arraylike_unshift(recv: f64, args_ptr: *const f64, count: i32) -> f64 {
    guard_string_receiver(recv);
    let n = count.max(0) as usize;
    let arr = as_real_array(recv);
    if !arr.is_null() {
        return unsafe { real_array_mutator(arr, "unshift", args_ptr, n) };
    }
    if let Some(r) = run_object_mutator(recv, "unshift", args_ptr, n) {
        return r;
    }
    pushlike_primitive_result(recv, n)
}

/// `push`/`unshift` over a receiver with no mutable array backing (boolean /
/// number primitive boxed to a fresh wrapper, or a symbol/bigint empty
/// array-like). `ToObject(recv)` throws a `TypeError` for `null`/`undefined`
/// (spec step 1), then the index/length writes land on the discarded wrapper
/// and the observable result is `ToLength(len) + count` — e.g.
/// `Array.prototype.unshift.call(true)` is `0` (test262
/// unshift/call-with-boolean), not `undefined`. A string receiver is already
/// rejected by [`guard_string_receiver`] before reaching here.
fn pushlike_primitive_result(recv: f64, count: usize) -> f64 {
    let o = to_object(recv);
    (al_length(o) + count as i64) as f64
}

/// `Array.prototype.reduce`/`reduceRight` on an empty array-like with no
/// initial value throws `TypeError`. Split out of `generic.rs` (called by
/// `js_arraylike_reduce`/`js_arraylike_reduceRight`) to keep that file under
/// the 2000-line gate.
pub(super) fn throw_reduce_empty() -> ! {
    let msg = b"Reduce of empty array with no initial value";
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// #5989: `Set`/`Map` fallback for the generic array-like `forEach` engine.
///
/// Codegen routes `<expr>.forEach(cb, thisArg)` to `js_arraylike_forEach` when
/// it cannot statically prove the receiver is a collection — the case for a
/// member-access receiver such as React's
/// `renderState.bootstrapScripts.forEach(flushResource, destination)` in
/// `writePreamble`. A Set/Map is NOT array-like (no `.length`, no
/// integer-indexed elements), so the array-like loop reads length 0 and
/// iterates nothing, silently dropping every entry (the Next.js dropped Float
/// preload-`<link>` bug). This runs the collection's real `forEach` and returns
/// `true` when `recv` is a *registered* Set/Map; a genuine array-like receiver
/// returns `false` and falls through to the array-like loop unchanged.
pub(super) fn arraylike_collection_foreach(recv: f64, cb: f64, this_arg: f64) -> bool {
    let bits = recv.to_bits();
    if let Some(set) = crate::set::set_ptr_from_receiver_bits(bits) {
        crate::set::js_set_foreach(set, cb, this_arg);
        return true;
    }
    if let Some(map) = crate::map::map_ptr_from_receiver_bits(bits) {
        crate::map::js_map_foreach(map, cb, this_arg);
        return true;
    }
    false
}
