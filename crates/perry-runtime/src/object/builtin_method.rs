//! Method calls on builtin receivers (object model step 5, phase 2,
//! OBJECT_MODEL_2026-10-13.md §3.6 "Builtins", §3.7).
//!
//! An array, a string, a Map, a Set, a WeakMap, a WeakSet or a function
//! resolves a method through its family's intrinsic prototype
//! (`Array.prototype`, `String.prototype`, …), a sealed intrinsic layout
//! whose shadow bit says whether the program has written it
//! (`builtin_proto`, step 3). While neither that prototype nor
//! `%Object.prototype%` is shadowed and the receiver owns no property of the
//! name (its kind's own proof: a plain array header, a Map or Set with no
//! meta record, a weak collection on its birth layout, a function shape over `Function.prototype` that lacks the
//! key), the method the call reads is that family's builtin, which the
//! compiler names: `js_bm_<name><argc>` calls its body directly.
//!
//! A method site whose key and argument count name a builtin of some family
//! (perry-codegen `builtin_proto::BUILTIN_METHODS`) calls the entry in place
//! of the universal by-name dispatcher; every receiver the families above do
//! not prove takes that dispatcher ([`super::method_call::js_method_call_slow`])
//! with the site's operands. One header-type test per family, no site state,
//! no name lookup.
//!
//! A string receiver is never wrapped: the string entries
//! (`builtin_proto_entries`) read the primitive itself.

use super::builtin_proto::{pristine, BuiltinProto};
use crate::value::{JSValue, POINTER_MASK, POINTER_TAG, TAG_UNDEFINED};

const UNDEFINED: f64 = f64::from_bits(TAG_UNDEFINED);
/// An array whose header records a named property, a descriptor or its own
/// `[[Prototype]]` is not answered by `Array.prototype` alone.
const ARRAY_NOT_DEFAULT: u16 = crate::gc::GC_ARRAY_KEY_ATTRS | crate::gc::GC_ARRAY_CUSTOM_PROTO;

#[inline(always)]
fn is_string(v: f64) -> bool {
    JSValue::from_bits(v.to_bits()).is_any_string()
}

#[inline(always)]
fn boolean(b: bool) -> f64 {
    f64::from_bits(JSValue::bool(b).bits())
}

#[inline(always)]
fn boxed_ptr(p: *mut u8) -> f64 {
    f64::from_bits(POINTER_TAG | p as u64)
}

#[inline(always)]
fn raw(v: f64) -> *mut u8 {
    (v.to_bits() & POINTER_MASK) as *mut u8
}

/// The live, unforwarded heap cell `v` points at, with its GC type: a
/// POINTER-tagged value above the handle band.
#[inline(always)]
unsafe fn heap_cell(v: f64) -> Option<*const crate::gc::GcHeader> {
    let header = crate::value::addr_class::pointer_value_header(v.to_bits())?;
    (header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0).then_some(header)
}

/// [`heap_cell`] without the forwarding test (a forwarded array alias is
/// resolved by [`array_recv`]).
#[inline(always)]
unsafe fn heap_cell_any(v: f64) -> Option<*const crate::gc::GcHeader> {
    crate::value::addr_class::pointer_value_header(v.to_bits()).map(|header| header as *const _)
}

/// A plain array (`Array.prototype` answers every key it does not hold as
/// an element) while `Array.prototype` and `%Object.prototype%` are pristine:
/// the array's live head (a pre-growth alias the program still holds is
/// answered from the head it forwards to, whose header carries the facts).
#[inline(always)]
unsafe fn array_recv(v: f64) -> Option<f64> {
    let bits = v.to_bits();
    let mut header = crate::value::addr_class::pointer_value_header(bits)?;
    if header.obj_type != crate::gc::GC_TYPE_ARRAY {
        return None;
    }
    let mut live = (bits & POINTER_MASK) as usize;
    if header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
        live = forwarded_head(live)?;
        // `clean_arr_ptr` resolved `live` to the array the alias forwards to.
        header = crate::value::addr_class::honest_header(live);
        if header.obj_type != crate::gc::GC_TYPE_ARRAY
            || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return None;
        }
    }
    (header._reserved & ARRAY_NOT_DEFAULT == 0 && pristine(BuiltinProto::Array))
        .then(|| f64::from_bits(POINTER_TAG | live as u64))
}

/// The live head a forwarded array alias names, or `None`.
#[cold]
#[inline(never)]
unsafe fn forwarded_head(addr: usize) -> Option<usize> {
    let live = crate::array::clean_arr_ptr(addr as *const crate::array::ArrayHeader) as usize;
    (live != 0 && live != addr && crate::value::addr_class::is_above_handle_band(live))
        .then_some(live)
}

/// A Map with no meta record (no own property, no own `[[Prototype]]`)
/// while `Map.prototype` and `%Object.prototype%` are pristine.
#[inline(always)]
unsafe fn map_ok(v: f64) -> bool {
    heap_cell(v).is_some_and(|h| {
        (*h).obj_type == crate::gc::GC_TYPE_MAP
            && (*(raw(v) as *const crate::map::MapHeader)).meta.is_null()
    }) && pristine(BuiltinProto::Map)
}

/// [`map_ok`] for a Set.
#[inline(always)]
unsafe fn set_ok(v: f64) -> bool {
    heap_cell(v).is_some_and(|h| {
        (*h).obj_type == crate::gc::GC_TYPE_SET
            && (*(raw(v) as *const crate::set::SetHeader)).meta.is_null()
    }) && pristine(BuiltinProto::Set)
}

/// A WeakMap (`brand` `CLASS_ID_WEAKMAP`) or WeakSet still on its birth
/// layout (`shapes::branded_birth_layout`: created by its own constructor,
/// no own key, no `[[Prototype]]` of its own, no exotic mark; a subclass
/// instance is born on another layout) while its family's prototype and
/// `%Object.prototype%` are pristine. The layout word is the whole proof.
#[inline(always)]
unsafe fn weak_ok(v: f64, brand: u32) -> bool {
    heap_cell(v).is_some_and(|h| {
        crate::gc::gc_type_is_object((*h).obj_type)
            && (*(raw(v) as *const super::ObjectHeader)).parent_class_id
                == super::shapes::branded_birth_layout(brand)
    }) && pristine(if brand == crate::weakref::CLASS_ID_WEAKSET {
        BuiltinProto::WeakSet
    } else {
        BuiltinProto::WeakMap
    })
}

const WEAKMAP: u32 = crate::weakref::CLASS_ID_WEAKMAP;
const WEAKSET: u32 = crate::weakref::CLASS_ID_WEAKSET;

mod function_receiver;
use function_receiver::{function_call, function_ok};

/// `Array.prototype.push` on a plain array (the dispatcher's dense arm).
/// `recv.apply(args…)` on a function `function_ok` admitted: `Get(recv,
/// "apply")` is the intrinsic, so this runs it. A builtin body, and a
/// compiled body with a packed list or an `arguments` object, are entered
/// directly; anything else runs the intrinsic's full arm. `None` only where
/// the intrinsic declines (the caller's dispatcher answers).
#[inline(never)]
unsafe fn function_apply(recv: f64, args: &[f64]) -> Option<f64> {
    let this = args.first().copied().unwrap_or(UNDEFINED);
    let list = args.get(1).copied().unwrap_or(UNDEFINED);
    if let Some(value) = crate::closure::try_native_apply(recv, this, list) {
        return Some(value);
    }
    super::native_call_method::run_function_intrinsic(recv, "apply", args.as_ptr(), args.len())
}

/// `target.apply(this, arguments)` whose lookup answered "by name"
/// (`js_bm_lookup` admitted a function receiver): the operands between the
/// lookup and the call may have run code, so the same O(1) proof is taken
/// again before the intrinsic runs. `None`: the site's ordinary call half.
pub(crate) unsafe fn by_name_function_apply(target: f64, args: &[f64]) -> Option<f64> {
    if function_ok(target, b"apply") {
        return function_apply(target, args);
    }
    None
}

#[inline(never)]
unsafe fn array_push(recv: f64, values: &[f64]) -> f64 {
    let arr = raw(recv) as *mut crate::array::ArrayHeader;
    if crate::array::array_is_frozen(arr) {
        crate::collection_iter::throw_type_error("Cannot mutate a frozen array");
    }
    crate::array::guard_writable_length(arr);
    let mut a = arr;
    for v in values {
        a = crate::array::js_array_push_f64_spec(a, *v);
    }
    crate::array::js_array_length(a) as f64
}

/// `recv.add(a0)` (families T, WeakSet).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_add1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if set_ok(recv) {
        return boxed_ptr(crate::set::js_set_add(raw(recv).cast(), a0).cast());
    }
    if weak_ok(recv, WEAKSET) {
        return crate::weakref::operations::set(
            recv,
            a0,
            UNDEFINED,
            crate::weakref::operations::Receiver::Folded(WEAKSET),
        );
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.apply()` (families F).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_apply0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    let args: [f64; 0] = [];
    if function_ok(recv, b"apply") {
        if let Some(value) = function_apply(recv, &args) {
            return value;
        }
    }
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.apply(a0)` (families F).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_apply1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    let args: [f64; 1] = [a0];
    if function_ok(recv, b"apply") {
        if let Some(value) = function_apply(recv, &args) {
            return value;
        }
    }
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.apply(a0, a1)` (families F).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_apply2(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
    a1: f64,
) -> f64 {
    let args: [f64; 2] = [a0, a1];
    if function_ok(recv, b"apply") {
        if let Some(value) = function_apply(recv, &args) {
            return value;
        }
    }
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.at()` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_at0(site: u64, recv: f64, key: u64, method_id: i64) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_at0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.at(a0)` (families AS).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_at1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_at1(recv, a0);
    }
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_at1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.call()` (families F).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_call0(site: u64, recv: f64, key: u64, method_id: i64) -> f64 {
    let args: [f64; 0] = [];
    if function_ok(recv, b"call") {
        return function_call(recv, &args);
    }
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.call(a0)` (families F).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_call1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    let args: [f64; 1] = [a0];
    if function_ok(recv, b"call") {
        return function_call(recv, &args);
    }
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.call(a0, a1)` (families F).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_call2(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
    a1: f64,
) -> f64 {
    let args: [f64; 2] = [a0, a1];
    if function_ok(recv, b"call") {
        return function_call(recv, &args);
    }
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.call(a0, a1, a2)` (families F).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_call3(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
    a1: f64,
    a2: f64,
) -> f64 {
    let args: [f64; 3] = [a0, a1, a2];
    if function_ok(recv, b"call") {
        return function_call(recv, &args);
    }
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.call(a0, a1, a2, a3)` (families F).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_call4(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
    a1: f64,
    a2: f64,
    a3: f64,
) -> f64 {
    let args: [f64; 4] = [a0, a1, a2, a3];
    if function_ok(recv, b"call") {
        return function_call(recv, &args);
    }
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.charAt()` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_charAt0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_charAt0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.charAt(a0)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_charAt1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_charAt1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.charCodeAt()` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_charCodeAt0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_charCodeAt0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.charCodeAt(a0)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_charCodeAt1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_charCodeAt1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.codePointAt()` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_codePointAt0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_codePointAt0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.codePointAt(a0)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_codePointAt1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_codePointAt1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.concat()` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_concat0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_concat0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.concat(a0)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_concat1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_concat1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.concat(a0, a1)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_concat2(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
    a1: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_concat2(recv, a0, a1);
    }
    let args: [f64; 2] = [a0, a1];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.delete(a0)` (families MT, WeakMap, WeakSet).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_delete1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if map_ok(recv) {
        return boolean(crate::map::js_map_delete(raw(recv).cast(), a0) != 0);
    }
    if set_ok(recv) {
        return boolean(crate::set::js_set_delete(raw(recv).cast(), a0) != 0);
    }
    if weak_ok(recv, WEAKMAP) {
        return crate::weakref::operations::delete(
            recv,
            a0,
            crate::weakref::operations::Receiver::Folded(WEAKMAP),
        );
    }
    if weak_ok(recv, WEAKSET) {
        return crate::weakref::operations::delete(
            recv,
            a0,
            crate::weakref::operations::Receiver::Folded(WEAKSET),
        );
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.endsWith()` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_endsWith0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_endsWith0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.endsWith(a0)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_endsWith1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_endsWith1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.endsWith(a0, a1)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_endsWith2(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
    a1: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_endsWith2(recv, a0, a1);
    }
    let args: [f64; 2] = [a0, a1];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.entries()` (families A).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_entries0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_entries0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.every(a0)` (families A).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_every1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_every1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.filter(a0)` (families A).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_filter1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_filter1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.find(a0)` (families A).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_find1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_find1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.findIndex(a0)` (families A).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_findIndex1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_findIndex1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.findLast(a0)` (families A).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_findLast1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_findLast1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.findLastIndex(a0)` (families A).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_findLastIndex1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_findLastIndex1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.flat()` (families A).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_flat0(site: u64, recv: f64, key: u64, method_id: i64) -> f64 {
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_flat0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.flatMap(a0)` (families A).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_flatMap1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_flatMap1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.forEach(a0)` (families A).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_forEach1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_forEach1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.get(a0)` (families M, WeakMap).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_get1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if map_ok(recv) {
        return crate::map::js_map_get(raw(recv).cast(), a0);
    }
    if weak_ok(recv, WEAKMAP) {
        return crate::weakref::operations::get(
            recv,
            a0,
            crate::weakref::operations::Receiver::Folded(WEAKMAP),
        );
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.has(a0)` (families MT, WeakMap, WeakSet).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_has1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if map_ok(recv) {
        return boolean(crate::map::js_map_has(raw(recv).cast(), a0) != 0);
    }
    if set_ok(recv) {
        return boolean(crate::set::js_set_has(raw(recv).cast(), a0) != 0);
    }
    if weak_ok(recv, WEAKMAP) {
        return crate::weakref::operations::has(
            recv,
            a0,
            crate::weakref::operations::Receiver::Folded(WEAKMAP),
        );
    }
    if weak_ok(recv, WEAKSET) {
        return crate::weakref::operations::has(
            recv,
            a0,
            crate::weakref::operations::Receiver::Folded(WEAKSET),
        );
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.includes()` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_includes0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_includes0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.includes(a0)` (families AS).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_includes1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_includes1(recv, a0);
    }
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_includes1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.includes(a0, a1)` (families AS).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_includes2(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
    a1: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_includes2(recv, a0, a1);
    }
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_includes2(recv, a0, a1);
    }
    let args: [f64; 2] = [a0, a1];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.indexOf()` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_indexOf0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_indexOf0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.indexOf(a0)` (families AS).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_indexOf1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_indexOf1(recv, a0);
    }
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_indexOf1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.indexOf(a0, a1)` (families AS).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_indexOf2(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
    a1: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_indexOf2(recv, a0, a1);
    }
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_indexOf2(recv, a0, a1);
    }
    let args: [f64; 2] = [a0, a1];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.join()` (families A).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_join0(site: u64, recv: f64, key: u64, method_id: i64) -> f64 {
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_join0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.join(a0)` (families A).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_join1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_join1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.keys()` (families A).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_keys0(site: u64, recv: f64, key: u64, method_id: i64) -> f64 {
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_keys0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.lastIndexOf()` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_lastIndexOf0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_lastIndexOf0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.lastIndexOf(a0)` (families AS).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_lastIndexOf1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_lastIndexOf1(recv, a0);
    }
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_lastIndexOf1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.lastIndexOf(a0, a1)` (families AS).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_lastIndexOf2(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
    a1: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_lastIndexOf2(recv, a0, a1);
    }
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_lastIndexOf2(recv, a0, a1);
    }
    let args: [f64; 2] = [a0, a1];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.localeCompare()` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_localeCompare0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_localeCompare0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.localeCompare(a0)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_localeCompare1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_localeCompare1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.map(a0)` (families A).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_map1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_map1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.match()` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_match0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_match0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.match(a0)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_match1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_match1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.matchAll()` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_matchAll0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_matchAll0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.matchAll(a0)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_matchAll1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_matchAll1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.normalize()` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_normalize0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_normalize0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.normalize(a0)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_normalize1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_normalize1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.padEnd(a0)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_padEnd1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_padEnd1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.padEnd(a0, a1)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_padEnd2(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
    a1: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_padEnd2(recv, a0, a1);
    }
    let args: [f64; 2] = [a0, a1];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.padStart(a0)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_padStart1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_padStart1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.padStart(a0, a1)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_padStart2(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
    a1: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_padStart2(recv, a0, a1);
    }
    let args: [f64; 2] = [a0, a1];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.pop()` (families A).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_pop0(site: u64, recv: f64, key: u64, method_id: i64) -> f64 {
    if let Some(recv) = array_recv(recv) {
        return crate::array::js_array_pop_f64(raw(recv).cast());
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.push(a0)` (families A).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_push1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if let Some(recv) = array_recv(recv) {
        return array_push(recv, &[a0]);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.reduce(a0)` (families A).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_reduce1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_reduce1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.reduce(a0, a1)` (families A).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_reduce2(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
    a1: f64,
) -> f64 {
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_reduce2(recv, a0, a1);
    }
    let args: [f64; 2] = [a0, a1];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.reduceRight(a0)` (families A).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_reduceRight1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_reduceRight1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.reduceRight(a0, a1)` (families A).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_reduceRight2(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
    a1: f64,
) -> f64 {
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_reduceRight2(recv, a0, a1);
    }
    let args: [f64; 2] = [a0, a1];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.repeat(a0)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_repeat1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_repeat1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.replace()` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_replace0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_replace0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.replace(a0)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_replace1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_replace1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.replace(a0, a1)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_replace2(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
    a1: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_replace2(recv, a0, a1);
    }
    let args: [f64; 2] = [a0, a1];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.replaceAll()` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_replaceAll0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_replaceAll0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.replaceAll(a0)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_replaceAll1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_replaceAll1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.replaceAll(a0, a1)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_replaceAll2(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
    a1: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_replaceAll2(recv, a0, a1);
    }
    let args: [f64; 2] = [a0, a1];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.search()` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_search0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_search0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.search(a0)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_search1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_search1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.set(a0, a1)` (families M, WeakMap).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_set2(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
    a1: f64,
) -> f64 {
    if map_ok(recv) {
        return boxed_ptr(crate::map::js_map_set(raw(recv).cast(), a0, a1).cast());
    }
    if weak_ok(recv, WEAKMAP) {
        return crate::weakref::operations::set(
            recv,
            a0,
            a1,
            crate::weakref::operations::Receiver::Folded(WEAKMAP),
        );
    }
    let args: [f64; 2] = [a0, a1];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.shift()` (families A).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_shift0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_shift0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.slice()` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_slice0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_slice0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.slice(a0)` (families AS).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_slice1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_slice1(recv, a0);
    }
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_slice1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.slice(a0, a1)` (families AS).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_slice2(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
    a1: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_slice2(recv, a0, a1);
    }
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_slice2(recv, a0, a1);
    }
    let args: [f64; 2] = [a0, a1];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.some(a0)` (families A).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_some1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_some1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.sort(a0)` (families A).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_sort1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_sort1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.split()` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_split0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_split0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.split(a0)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_split1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_split1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.split(a0, a1)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_split2(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
    a1: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_split2(recv, a0, a1);
    }
    let args: [f64; 2] = [a0, a1];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.startsWith()` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_startsWith0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_startsWith0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.startsWith(a0)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_startsWith1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_startsWith1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.startsWith(a0, a1)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_startsWith2(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
    a1: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_startsWith2(recv, a0, a1);
    }
    let args: [f64; 2] = [a0, a1];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.substr(a0)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_substr1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_substr1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.substr(a0, a1)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_substr2(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
    a1: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_substr2(recv, a0, a1);
    }
    let args: [f64; 2] = [a0, a1];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.substring()` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_substring0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_substring0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.substring(a0)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_substring1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_substring1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.substring(a0, a1)` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_substring2(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
    a1: f64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_substring2(recv, a0, a1);
    }
    let args: [f64; 2] = [a0, a1];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.toLowerCase()` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_toLowerCase0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_toLowerCase0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.toReversed()` (families A).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_toReversed0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_toReversed0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.toSorted()` (families A).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_toSorted0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_toSorted0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.toSorted(a0)` (families A).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_toSorted1(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
) -> f64 {
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_toSorted1(recv, a0);
    }
    let args: [f64; 1] = [a0];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.toUpperCase()` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_toUpperCase0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_toUpperCase0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.trim()` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_trim0(site: u64, recv: f64, key: u64, method_id: i64) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_trim0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.trimEnd()` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_trimEnd0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_trimEnd0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.trimStart()` (families S).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_trimStart0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    if is_string(recv) {
        return super::builtin_proto_entries::js_string_m_trimStart0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.values()` (families A).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_values0(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
) -> f64 {
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_values0(recv);
    }
    let args: [f64; 0] = [];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// `recv.with(a0, a1)` (families A).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_with2(
    site: u64,
    recv: f64,
    key: u64,
    method_id: i64,
    a0: f64,
    a1: f64,
) -> f64 {
    if let Some(recv) = array_recv(recv) {
        return super::builtin_proto_entries::js_array_b_with2(recv, a0, a1);
    }
    let args: [f64; 2] = [a0, a1];
    super::method_call::js_method_call_slow(site, recv, key, method_id, args.as_ptr(), args.len())
}

/// The families a [`js_bm_lookup`] site names (perry-codegen
/// `builtin_proto::BUILTIN_METHODS`).
const FAMILY_STRING: u64 = 1;
const FAMILY_ARRAY: u64 = 2;
const FAMILY_MAP: u64 = 4;
const FAMILY_SET: u64 = 8;
const FAMILY_FUNCTION: u64 = 16;

/// The lookup half of a split method call (#11910) whose key names a builtin
/// method of the families in `families`: a receiver of one of them that the
/// family's pristine prototype answers reads nothing observable, so the
/// answer is "by name after the arguments" (`METHOD_CALL_BY_NAME_DIRECT`),
/// and the call half calls the `js_bm_` entry; anything else is
/// [`super::method_call::js_method_lookup`].
///
/// # Safety
/// As `js_method_lookup`; `key_bits` is the site's key atom.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_bm_lookup(
    dir: *const u8,
    recv: f64,
    key_bits: u64,
    hash: u64,
    method_id: i64,
    families: u64,
) -> f64 {
    let direct = f64::from_bits(super::method_call::METHOD_CALL_BY_NAME_DIRECT);
    if is_string(recv) {
        if families & FAMILY_STRING != 0 && pristine(BuiltinProto::String) {
            return direct;
        }
    } else if let Some(h) = heap_cell_any(recv) {
        // One header read picks the family; an ordinary object takes the
        // probe's lookup at once.
        let admitted = match (*h).obj_type {
            crate::gc::GC_TYPE_ARRAY => families & FAMILY_ARRAY != 0 && array_recv(recv).is_some(),
            crate::gc::GC_TYPE_MAP => families & FAMILY_MAP != 0 && map_ok(recv),
            crate::gc::GC_TYPE_SET => families & FAMILY_SET != 0 && set_ok(recv),
            // The Map and Set families' entries name only methods a WeakMap
            // and a WeakSet also have (`get`/`set`/`has`/`delete`,
            // `add`/`has`/`delete`).
            ty if crate::gc::gc_type_is_object(ty) => {
                (families & FAMILY_MAP != 0 && weak_ok(recv, WEAKMAP))
                    || (families & FAMILY_SET != 0 && weak_ok(recv, WEAKSET))
            }
            crate::gc::GC_TYPE_CLOSURE => {
                families & FAMILY_FUNCTION != 0
                    && key_bits & !POINTER_MASK == crate::value::STRING_TAG
                    && {
                        let key = (key_bits & POINTER_MASK) as *const crate::StringHeader;
                        let name = std::slice::from_raw_parts(
                            crate::string::string_data(key),
                            (*key).byte_len as usize,
                        );
                        if let Some(value) = function_receiver::lookup_value(recv, name, direct) {
                            return value;
                        }
                        false
                    }
            }
            _ => false,
        };
        if admitted {
            return direct;
        }
    }
    super::method_call::js_method_lookup(dir, recv, key_bits, hash, method_id)
}

#[cfg(test)]
mod native_constructor_tests;
