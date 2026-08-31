//! Indexing — length / element get / element set / hybrid string-or-index dispatch.
use super::indexing_support::*;
use super::*;
use std::ptr;
use std::sync::atomic::Ordering;

#[path = "indexing_keyed.rs"]
mod keyed;
pub use keyed::{
    js_array_get_index_or_string, js_array_set_index_or_string,
    js_array_set_index_or_string_strict, js_array_set_string_key,
};

const MAX_DENSE_ARRAY_GROW_LENGTH: u32 = 1_000_000;

/// Largest hole (`index - length`) an extending write may create while still
/// growing the dense backing store, once the array is past
/// `MAX_DENSE_ARRAY_GROW_LENGTH`. Sparse storage is for *jumps* far beyond the
/// current length (`a[2**32-2] = v` on a 3-element array must not allocate
/// 34 GB); sequential growth (`for (i...) arr[i] = v`, gap 0) must stay dense
/// no matter how large the array gets — routing it through string-keyed
/// property sets is quadratic and hung the 10M-element `03_array_write`
/// benchmark for 6 hours (Regression Check, v0.5.1129–v0.5.1150).
const DENSE_ARRAY_GAP_LIMIT: u32 = 1024;

#[inline]
pub(crate) fn invalidate_array_index_fast_path() {
    PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED.store(1, Ordering::Relaxed);
}

#[cfg(test)]
thread_local! {
    static STRICT_DENSE_POINTER_OVERWRITE_HITS: std::cell::Cell<u64> = const {
        std::cell::Cell::new(0)
    };
}

#[cfg(test)]
pub(crate) fn test_strict_dense_pointer_overwrite_hits() -> u64 {
    STRICT_DENSE_POINTER_OVERWRITE_HITS.with(std::cell::Cell::get)
}

// Test-only entry counter for `js_array_get_f64`, the JS-facing element
// accessor. A runtime walk that reaches for it PER ELEMENT is paying the whole
// gauntlet (forward-resolution, Map/Set/typed-array/buffer registry probes,
// descriptor gate, hole translation) for what is a raw slot read, so tests that
// assert "this walk no longer uses the element accessor" count it rather than
// timing it. Same shape as the hit counter above.
#[cfg(test)]
thread_local! {
    static ELEMENT_ACCESSOR_CALLS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn test_element_accessor_calls() -> u64 {
    ELEMENT_ACCESSOR_CALLS.with(std::cell::Cell::get)
}

// The two strict-dense store helpers live in `strict_dense_test_helpers`
// (2000-line cap). Re-exported by name so `super::indexing::…` paths in the
// existing test modules keep resolving — a glob would not propagate.
#[cfg(test)]
pub(crate) use super::strict_dense_test_helpers::{
    test_strict_dense_number_store, test_strict_dense_pointer_overwrite,
};

pub(crate) fn object_prototype_has_index_flag() -> bool {
    OBJECT_PROTO_HAS_INDEX.load(Ordering::Relaxed)
}

/// Record (if `arr` is `Array.prototype`) that the prototype now carries an
/// indexed property, so subsequent out-of-bounds reads consult it. Called from
/// the array element-write paths; cheap (two relaxed atomic loads + compare).
#[inline]
pub(crate) fn note_array_index_write(arr: usize) {
    if !ARRAY_PROTO_HAS_INDEX.load(Ordering::Relaxed) && arr != 0 && arr == array_prototype_addr() {
        ARRAY_PROTO_HAS_INDEX.store(true, Ordering::Relaxed);
        invalidate_array_index_fast_path();
    }
}

/// Out-of-bounds element read fallback: `Array.prototype[index]` when the
/// prototype has indexed properties (see `ARRAY_PROTO_HAS_INDEX`). Returns the
/// inherited value, or `undefined` if absent. Skipped entirely when the
/// receiver IS `Array.prototype` (avoids self-recursion) or the flag is unset.
///
/// #6981: the `proto != receiver` self-recursion guard is an OBJECT IDENTITY
/// test, so both sides must be forwarding-resolved. `js_array_get_f64` resolves
/// its receiver through `clean_arr_ptr`; the prototype address comes from a
/// memoized cache, so it is healed here too. Comparing a stale address against
/// a resolved one makes the guard silently stop firing and
/// `js_array_get_f64` ⇄ this function recurse without bound.
#[inline]
unsafe fn array_oob_prototype_get(receiver: usize, index: u32) -> f64 {
    const TAG_UNDEFINED_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0001u64);
    // A custom [[Prototype]] (`Object.setPrototypeOf(arr, p)`) replaces the
    // default chain — gated on a global relaxed flag. #9192: `p` need not be an
    // array; a plain object / `Object.create(Array.prototype)` result answers
    // the whole lookup through the generic resolver.
    if crate::object::prototype_chain::array_static_proto_recorded() {
        let arr = receiver as *const ArrayHeader;
        match array_custom_prototype(arr) {
            Some(ArrayCustomProto::Null) => return TAG_UNDEFINED_F64,
            Some(ArrayCustomProto::Other(bits)) => {
                return array_object_proto_index_get(arr, bits, index).unwrap_or(TAG_UNDEFINED_F64)
            }
            Some(ArrayCustomProto::Array(proto_arr)) => {
                if index < (*proto_arr).length && array_has_own_index(proto_arr, index) {
                    return js_array_get_f64(proto_arr, index);
                }
            }
            None => {}
        }
    }
    if ARRAY_PROTO_HAS_INDEX.load(Ordering::Relaxed) {
        let proto = array_prototype_addr();
        if proto != 0 && proto != crate::value::resolve_forwarding(receiver) {
            let proto_arr = proto as *const ArrayHeader;
            if index < (*proto_arr).length && array_has_own_index(proto_arr, index) {
                return js_array_get_f64(proto_arr, index);
            }
        }
    }
    // Object.prototype indexed property (data or defineProperty accessor):
    // arr → Array.prototype → Object.prototype (concat/S15.4.4.4_A3_T3).
    if OBJECT_PROTO_HAS_INDEX.load(Ordering::Relaxed)
        && crate::array::object_prototype_has_index_prop(index)
    {
        return crate::array::sort_object_prototype_index_get(index);
    }
    TAG_UNDEFINED_F64
}

#[inline]
unsafe fn array_sparse_index_property_get(arr: *const ArrayHeader, index: u32) -> Option<f64> {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() || index < (*arr).capacity {
        return None;
    }
    let key = index.to_string();
    array_named_property_get_by_name(arr, &key)
}

unsafe fn array_sparse_index_property_set(arr: *mut ArrayHeader, index: u32, value: f64) {
    let key = index.to_string();
    let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    array_named_property_set(arr, key_ptr, value);
    let new_length = index + 1;
    if (*arr).length < new_length {
        (*arr).length = new_length;
    }
}

/// Whether iterating `arr` with the raw dense-store loop would diverge from the
/// spec `[[HasProperty]]`/`[[Get]]` protocol. True ("exotic") when the array has
/// index accessors / custom-attr descriptors, lives in (partly) sparse storage,
/// or the prototype chain carries indexed properties. When false the fast loop
/// is byte-identical to the spec, so callers keep their hot path.
#[inline]
pub(crate) fn array_iteration_is_exotic(arr: *const ArrayHeader) -> bool {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return false;
    }
    if crate::buffer::is_registered_buffer(arr as usize)
        || crate::typedarray::lookup_typed_array_kind(arr as usize).is_some()
    {
        return true;
    }
    // SAFETY: the clean above resolved this exact live head, and the flag read
    // precedes every operation that could allocate or safepoint. The
    // compatible header-less receivers exited above.
    let flags = unsafe { array_object_flags_resolved(arr) };
    unsafe { array_iteration_is_exotic_resolved(arr, flags) }
}

/// [`array_iteration_is_exotic`] for a caller that already resolved the live
/// plain-array head, excluded Buffer/TypedArray receivers, and owns the header
/// word: the policy tests without a second receiver resolution and registry
/// probe (the iteration helpers call this once per invocation).
///
/// # Safety
///
/// `arr` and `flags` must satisfy [`array_object_flags_resolved`]'s contract.
pub(crate) unsafe fn array_iteration_is_exotic_resolved(
    arr: *const ArrayHeader,
    flags: u16,
) -> bool {
    if flags & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0 {
        return true;
    }
    if ARRAY_PROTO_HAS_INDEX.load(Ordering::Relaxed) {
        return true;
    }
    if OBJECT_PROTO_HAS_INDEX.load(Ordering::Relaxed) {
        return true;
    }
    // Live indices beyond the dense backing store are stored in the sparse
    // named-property map, which the raw element loop never reads.
    unsafe { (*arr).length > (*arr).capacity }
}

/// Spec `OrdinaryGetOwnProperty(O, ToString(index)) != undefined` for an Array:
/// is `index` present as an *own* property (dense non-hole slot, sparse named
/// data property, or an accessor descriptor)?
pub(crate) unsafe fn array_has_own_index(arr: *const ArrayHeader, index: u32) -> bool {
    // #6748 grind: gate on the PER-ARRAY descriptor flag (set by every
    // `define_array_property` install), not the process-global
    // `descriptors_in_use()` — the global flag flips during builtin init, so
    // every array element probe paid an `index.to_string()` + accessor-map
    // String-key alloc for arrays that have no descriptors at all.
    if array_object_flags(arr) & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0 {
        let key = index.to_string();
        if crate::object::get_accessor_descriptor(arr as usize, &key).is_some() {
            return true;
        }
    }
    let key = index.to_string();
    if array_named_property_get_by_name(arr, &key).is_some() {
        return true;
    }
    if index < (*arr).length && index < (*arr).capacity {
        let elements = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const u64;
        if ptr::read(elements.add(index as usize)) != crate::value::TAG_HOLE {
            return true;
        }
    }
    false
}

/// Spec `[[HasProperty]]`(O, ToString(index)) for an ordinary Array receiver:
/// own property OR inherited indexed property from `Array.prototype`.
pub(crate) fn array_spec_has_index(arr: *const ArrayHeader, index: u32) -> bool {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return false;
    }
    unsafe {
        if array_has_own_index(arr, index) {
            return true;
        }
        // An explicit `Object.setPrototypeOf(arr, p)` REPLACES the default
        // chain. A real-array `p` keeps the original lane (its own indices
        // first, then the implicit `Array.prototype` tail below — test262
        // copyWithin/coerced-values-start-change-*). #9192: any other `p`
        // answers the whole question by itself, so the default-chain tail must
        // not run after it.
        match array_custom_prototype(arr) {
            Some(ArrayCustomProto::Null) => return false,
            Some(ArrayCustomProto::Other(bits)) => {
                return array_object_proto_index_has(bits, index)
            }
            Some(ArrayCustomProto::Array(proto_arr)) => {
                if index < (*proto_arr).length && array_has_own_index(proto_arr, index) {
                    return true;
                }
            }
            None => {}
        }
        if ARRAY_PROTO_HAS_INDEX.load(Ordering::Relaxed) {
            let proto = array_prototype_addr();
            if proto != 0 && proto != arr as usize {
                let proto_arr = proto as *const ArrayHeader;
                if index < (*proto_arr).length && array_has_own_index(proto_arr, index) {
                    return true;
                }
            }
        }
        if OBJECT_PROTO_HAS_INDEX.load(Ordering::Relaxed)
            && crate::array::object_prototype_has_index_prop(index)
        {
            return true;
        }
        false
    }
}

/// How a recorded custom `[[Prototype]]` on a real array must be consulted.
///
/// #9192: before this classification the array index paths accepted a recorded
/// prototype ONLY when it was itself a `GC_TYPE_ARRAY`; every other shape (a
/// plain object, an `Object.create(Array.prototype)` result, a class prototype)
/// was recorded — latching the process-wide index deopt — and then silently
/// ignored, so the array inherited nothing at all.
pub(crate) enum ArrayCustomProto {
    /// `Object.setPrototypeOf(arr, null)`: nothing is inherited, and the
    /// implicit `Array.prototype` → `Object.prototype` chain is gone too.
    Null,
    /// The recorded prototype is itself a real array — the original lane, kept
    /// bit-for-bit (test262 copyWithin/coerced-values-start-change-*).
    Array(*const ArrayHeader),
    /// Any other object: resolved through the generic object machinery with the
    /// array as the receiver, so prototype accessors see the right `this` and
    /// further hops (`Object.create(Array.prototype)`, proxies) are walked.
    Other(u64),
}

/// Classify the `[[Prototype]]` an explicit `Object.setPrototypeOf` /
/// `__proto__` / `Reflect.setPrototypeOf` recorded for `arr`. `None` when the
/// array still carries the default `Array.prototype` chain.
pub(crate) unsafe fn array_custom_prototype(arr: *const ArrayHeader) -> Option<ArrayCustomProto> {
    let bits = crate::object::prototype_chain::object_static_prototype(arr as usize)?;
    if bits == crate::value::TAG_NULL {
        return Some(ArrayCustomProto::Null);
    }
    if let Some(proto_arr) = array_custom_array_prototype_from_bits(arr, bits) {
        return Some(ArrayCustomProto::Array(proto_arr));
    }
    // A Proxy prototype keeps its existing dedicated handling in the `in` /
    // property-get arms; routing it through the generic resolver here as well
    // would invoke the `has` trap twice, which is observable.
    if crate::proxy::js_proxy_is_proxy(f64::from_bits(bits)) != 0 {
        return None;
    }
    // A pointer-shaped record that is not a real array is the #9192 case. A
    // record that is not pointer-shaped at all (a stale/garbage entry) is
    // reported as "no custom prototype", exactly as before.
    pointer_bits_of_recorded_prototype(bits).map(|_| ArrayCustomProto::Other(bits))
}

/// The heap address a recorded prototype's bits name, if they are pointer
/// shaped at all. The record may be NaN-boxed (0x7FFD) or a RAW untagged
/// pointer (module-level arrays are stored as raw I64s).
fn pointer_bits_of_recorded_prototype(bits: u64) -> Option<usize> {
    let raw = if (bits >> 48) == 0x7FFD {
        (bits & crate::value::POINTER_MASK) as usize
    } else if (bits >> 48) == 0 && bits > 0x10000 {
        bits as usize
    } else {
        return None;
    };
    if raw == 0 {
        None
    } else {
        Some(raw)
    }
}

/// A custom `[[Prototype]]` installed on `arr` via `Object.setPrototypeOf`
/// that happens to be a real array — `None` for every other recorded shape
/// (which [`array_custom_prototype`] reports as [`ArrayCustomProto::Other`]).
unsafe fn array_custom_array_prototype_from_bits(
    arr: *const ArrayHeader,
    bits: u64,
) -> Option<*const ArrayHeader> {
    let raw = pointer_bits_of_recorded_prototype(bits)?;
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 || raw == arr as usize {
        return None;
    }
    // A Proxy prototype is a small registered id, not a heap allocation — the
    // GC-header read below would deref a fake pointer.
    if crate::proxy::js_proxy_is_proxy(f64::from_bits(bits)) != 0 {
        return None;
    }
    // #5625: the recorded prototype may be a *grown* array whose stored pointer
    // was left FORWARDED by `js_array_grow` — its first 8 bytes now hold the
    // forwarding pointer to the live head instead of length+capacity. (A real
    // array grows when `Object.setPrototypeOf(arr, p)` captured `p` before a
    // later push reallocated it, or the proto itself was built by appends — as
    // in test262 copyWithin/coerced-values-start-change-start, whose
    // `longDenseArray()` fills a `[0]` to 1024 elements.) Resolve the chain so
    // we deref the current array head; reading the defunct old location yields
    // the forwarding pointer's low 32 bits as a garbage `length`, making
    // inherited-index reads silently miss (nondeterministic copyWithin output).
    let resolved = clean_arr_ptr(raw as *const ArrayHeader);
    if resolved.is_null() || resolved as usize == arr as usize {
        return None;
    }
    let hdr = (resolved as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*hdr).obj_type == crate::gc::GC_TYPE_ARRAY {
        Some(resolved)
    } else {
        None
    }
}

/// #9192: `[[Get]]`(index) through a NON-array custom `[[Prototype]]`, binding
/// `arr` as the receiver so an inherited index accessor sees the array as
/// `this`. `None` when the whole (replaced) chain lacks the index.
///
/// Everything here allocates — `index.to_string()` interns a key, and the
/// resolver can run a user getter — so the array and the prototype are rooted
/// and re-read across the call.
unsafe fn array_object_proto_index_get(
    arr: *const ArrayHeader,
    proto_bits: u64,
    index: u32,
) -> Option<f64> {
    // The caller may still hold a pre-grow forwarding stub; the receiver an
    // inherited accessor observes must be the live head.
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(arr as i64));
    let proto = scope.root_heap_word_u64(proto_bits);
    let key = index.to_string();
    let key_hdr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    if key_hdr.is_null() {
        return None;
    }
    let key_handle = scope.root_nanbox_f64(crate::value::nanbox_string_key(key_hdr));
    let receiver_addr = crate::value::js_nanbox_get_pointer(receiver.get_nanbox_f64()) as usize;
    let key_ptr = crate::value::js_nanbox_get_pointer(key_handle.get_nanbox_f64())
        as *const crate::StringHeader;
    crate::object::prototype_chain::resolve_inherited_field_from_prototype(
        receiver_addr,
        proto.get_heap_word_u64(),
        key_ptr,
    )
    .map(|v| f64::from_bits(v.bits()))
}

/// #9192: the first object in a NON-array custom `[[Prototype]]` chain that
/// owns `key` with a descriptor — the owner whose accessor / attributes the
/// spec `Set` must observe before creating an own element on the array. A plain
/// writable data property carries no side-table entry and correctly reports no
/// owner: the Set then creates the own element, as the spec requires.
unsafe fn array_object_proto_index_owner(proto_bits: u64, key: &str) -> usize {
    let mut bits = proto_bits;
    for _ in 0..64 {
        if bits == crate::value::TAG_NULL {
            return 0;
        }
        if crate::proxy::js_proxy_is_proxy(f64::from_bits(bits)) != 0 {
            return 0;
        }
        let Some(addr) = pointer_bits_of_recorded_prototype(bits) else {
            return 0;
        };
        // Pair the band predicate with the validity check (#6279): a handle
        // value sits below HANDLE_BAND_MAX and would otherwise be dereferenced
        // as if it were an object pointer.
        if !crate::value::addr_class::is_above_handle_band(addr as usize)
            || !crate::object::is_valid_obj_ptr(addr as *const u8)
        {
            return 0;
        }
        if crate::object::get_accessor_descriptor(addr, key).is_some()
            || crate::object::get_property_attrs(addr, key).is_some()
        {
            return addr;
        }
        match crate::object::prototype_chain::object_static_prototype(addr) {
            Some(next) => bits = next,
            // #9220: `Object.create(p)` does NOT record `p` in the observable
            // prototype side table — `js_object_create` models the link with a
            // SYNTHETIC CLASS ID whose `class_prototype_object` entry is `p`
            // (#809). The recorded-prototype hop alone therefore stops one link
            // short, and an inherited accessor / non-writable index that the
            // READ side already resolves (`js_object_get_field_by_name`'s
            // `class_id != 0` branch, reached through
            // `resolve_inherited_field_from_prototype`) was silently replaced by
            // a new own element on the array. Take the same hop the read walk
            // takes so `[[Set]]` and `[[Get]]` agree on the chain.
            None => {
                let class_id = (*(addr as *const crate::ObjectHeader)).class_id;
                if class_id == 0 {
                    return 0;
                }
                let synth = crate::object::class_prototype_object(class_id);
                if synth.is_null() || synth as usize == addr {
                    return 0;
                }
                bits = crate::value::js_nanbox_pointer(synth as i64).to_bits();
            }
        }
    }
    0
}

/// #9192: `[[HasProperty]]`(index) through a NON-array custom `[[Prototype]]`.
unsafe fn array_object_proto_index_has(proto_bits: u64, index: u32) -> bool {
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_heap_word_u64(proto_bits);
    let key = index.to_string();
    let key_hdr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    if key_hdr.is_null() {
        return false;
    }
    let key_handle = scope.root_nanbox_f64(crate::value::nanbox_string_key(key_hdr));
    let key_ptr = crate::value::js_nanbox_get_pointer(key_handle.get_nanbox_f64())
        as *const crate::StringHeader;
    crate::object::prototype_value_has_property(proto.get_heap_word_u64(), key_ptr)
}

/// Spec `[[Get]]`(O, ToString(index)) for an ordinary Array receiver: own value
/// (firing index accessors via `js_array_get_f64`) or, for an absent own index,
/// the inherited `Array.prototype[index]`. Returns `undefined` when absent.
pub(crate) fn array_spec_get(arr: *const ArrayHeader, index: u32) -> f64 {
    const TAG_UNDEFINED_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0001u64);
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return TAG_UNDEFINED_F64;
    }
    unsafe {
        let receiver = crate::value::js_nanbox_pointer(arr as i64);
        let scope = crate::gc::RuntimeHandleScope::new();
        let receiver = scope.root_nanbox_f64(receiver);
        if array_has_own_index(arr, index) {
            return js_array_get_f64(arr, index);
        }
        // #9192: see `array_spec_has_index` — a non-array custom prototype
        // replaces the default chain outright.
        match array_custom_prototype(arr) {
            Some(ArrayCustomProto::Null) => return TAG_UNDEFINED_F64,
            Some(ArrayCustomProto::Other(bits)) => {
                return array_object_proto_index_get(arr, bits, index).unwrap_or(TAG_UNDEFINED_F64)
            }
            Some(ArrayCustomProto::Array(proto_arr)) => {
                if index < (*proto_arr).length && array_has_own_index(proto_arr, index) {
                    return array_inherited_index_get(proto_arr, index, receiver.get_nanbox_f64());
                }
            }
            None => {}
        }
        if ARRAY_PROTO_HAS_INDEX.load(Ordering::Relaxed) {
            let proto = array_prototype_addr();
            if proto != 0 && proto != arr as usize {
                let proto_arr = proto as *const ArrayHeader;
                if index < (*proto_arr).length && array_has_own_index(proto_arr, index) {
                    return array_inherited_index_get(proto_arr, index, receiver.get_nanbox_f64());
                }
            }
        }
        if OBJECT_PROTO_HAS_INDEX.load(Ordering::Relaxed)
            && crate::array::object_prototype_has_index_prop(index)
        {
            return crate::array::sort_object_prototype_index_get_with_receiver(
                index,
                receiver.get_nanbox_f64(),
            );
        }
        TAG_UNDEFINED_F64
    }
}

/// Spec `Set(O, ToString(index), value, true)` for an Array receiver. Unlike
/// the internal dense setter, this observes an inherited indexed accessor
/// before creating an own element. Array mutators use it on their exotic path
/// because a prototype setter may mutate the receiver (including freezing it
/// or making `length` non-writable) before the mutator's final length Set.
pub(crate) fn array_spec_set(arr: *mut ArrayHeader, index: u32, value: f64) -> *mut ArrayHeader {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return arr;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_mut_ptr(arr);
    let value_handle = scope.root_nanbox_f64(value);
    let receiver =
        || crate::value::js_nanbox_pointer(arr_handle.get_raw_mut_ptr::<ArrayHeader>() as i64);
    let key = index.to_string();

    unsafe {
        if array_has_own_index(arr_handle.get_raw_mut_ptr::<ArrayHeader>(), index) {
            return js_array_set_f64_extend_strict_impl(
                arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
                index,
                value_handle.get_nanbox_f64(),
                true,
            );
        }

        // #9192: a non-array custom `[[Prototype]]` owns the whole answer — its
        // chain supplies the inherited accessor / non-writable attributes, and
        // the implicit `Array.prototype` / `Object.prototype` tail below must
        // not run. An explicit null prototype inherits nothing at all.
        let mut default_chain = true;
        let mut inherited_owner = 0usize;
        match array_custom_prototype(arr_handle.get_raw_mut_ptr::<ArrayHeader>()) {
            Some(ArrayCustomProto::Null) => default_chain = false,
            Some(ArrayCustomProto::Other(bits)) => {
                default_chain = false;
                inherited_owner = array_object_proto_index_owner(bits, &key);
            }
            Some(ArrayCustomProto::Array(proto_arr)) => {
                if array_has_own_index(proto_arr, index) {
                    inherited_owner = proto_arr as usize;
                }
            }
            None => {}
        }
        if inherited_owner == 0 && default_chain {
            let proto = array_prototype_addr();
            inherited_owner = if proto != 0
                && proto != arr_handle.get_raw_mut_ptr::<ArrayHeader>() as usize
                && array_has_own_index(proto as *const ArrayHeader, index)
            {
                proto
            } else if object_prototype_has_index_flag()
                && crate::array::object_prototype_has_index_prop(index)
            {
                object_prototype_addr()
            } else {
                0
            };
        }

        if inherited_owner != 0 {
            if let Some(accessor) = crate::object::get_accessor_descriptor(inherited_owner, &key) {
                if accessor.set == 0 {
                    crate::collection_iter::throw_type_error(&format!(
                        "Cannot set property {index} which has only a getter"
                    ));
                }
                crate::object::invoke_accessor_setter(
                    accessor.set,
                    receiver(),
                    value_handle.get_nanbox_f64(),
                );
                return arr_handle.get_raw_mut_ptr::<ArrayHeader>();
            }
            if crate::object::get_property_attrs(inherited_owner, &key)
                .is_some_and(|attrs| !attrs.writable())
            {
                throw_frozen_array_index_write(index);
            }
        }

        js_array_set_f64_extend_strict_impl(
            arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
            index,
            value_handle.get_nanbox_f64(),
            true,
        )
    }
}

/// Read an own indexed property from an Array prototype while preserving the
/// original receiver for an inherited accessor's `this` value.
unsafe fn array_inherited_index_get(
    proto_arr: *const ArrayHeader,
    index: u32,
    receiver: f64,
) -> f64 {
    if array_object_flags(proto_arr) & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0 {
        if let Some(acc) =
            crate::object::get_accessor_descriptor(proto_arr as usize, &index.to_string())
        {
            if acc.get != 0 {
                return f64::from_bits(
                    crate::object::invoke_accessor_getter(acc.get, receiver).bits(),
                );
            }
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
    }
    js_array_get_f64(proto_arr, index)
}

pub(crate) fn array_get_property_by_key(
    arr: *const ArrayHeader,
    key: *const crate::StringHeader,
) -> f64 {
    // #7891: an erased Array declaration can feed this ABI a heap StringHeader.
    // The receiver arrived unboxed and no longer carries STRING_TAG, so recover
    // its runtime kind from the GC header before ordinary by-name lookup. A
    // canonical index reads the UTF-16 code unit; `length`, `constructor`, OOB
    // and non-index keys fall through to the established String property path.
    // (SSO strings have no pointer/header and are separated by codegen.)
    if !arr.is_null() && !key.is_null() {
        if let Some(header) = unsafe { crate::value::addr_class::try_read_gc_header(arr as usize) }
        {
            if header.obj_type == crate::gc::GC_TYPE_STRING {
                let key_value = crate::value::JSValue::string_ptr(key as *mut crate::StringHeader);
                let indexed = crate::string::js_string_index_get(
                    arr as *const crate::StringHeader,
                    f64::from_bits(key_value.bits()),
                );
                if indexed.to_bits() != crate::value::TAG_UNDEFINED {
                    return indexed;
                }
            }
        }
    }
    let value =
        crate::object::js_object_get_field_by_name(arr as *const crate::object::ObjectHeader, key);
    f64::from_bits(value.bits())
}

/// Auto-opt dead-strip anchor: codegen emits a bare `js_array_length` symbol in
/// native-region wrappers (`__perry_wrap_*`) and elsewhere, so it must be a
/// `#[no_mangle]` C export AND survive dead-stripping even when no Rust caller
/// keeps it referenced — mirroring the neighbouring `js_array_push`.
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_ARRAY_LENGTH: extern "C" fn(*const ArrayHeader) -> u32 = js_array_length;

#[no_mangle]
pub extern "C" fn js_array_length(arr: *const ArrayHeader) -> u32 {
    // Fast lane: a live plain array on an arena page. Every dynamic `.length`
    // read and every native push lowering (which re-reads the length for the
    // result) lands here; the proxy, Set/Map, object and subclass arms below
    // all begin with probes this receiver cannot satisfy. A proxy id sits in
    // the handle band and a Set/Map/object header has another type, so the
    // lane's own checks exclude them.
    {
        let bits = arr as u64;
        let top16 = bits >> 48;
        let raw = if top16 >= 0x7FF8 {
            if top16 == (crate::value::POINTER_TAG >> 48) {
                (bits & crate::value::POINTER_MASK) as usize
            } else {
                0
            }
        } else {
            bits as usize
        };
        if raw >= crate::gc::GC_HEADER_SIZE
            && raw % std::mem::align_of::<crate::gc::GcHeader>() == 0
            && crate::value::addr_class::is_plausible_heap_addr(raw)
            && !matches!(
                crate::arena::classify_heap_generation(raw),
                crate::arena::HeapGeneration::Unknown
            )
        {
            // SAFETY: owned arena page, header-aligned; the header word
            // precedes every arena block.
            let header = (raw - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            let (obj_type, gc_flags) = unsafe { ((*header).obj_type, (*header).gc_flags) };
            if obj_type == crate::gc::GC_TYPE_ARRAY
                && gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
                && gc_flags & crate::gc::GC_FLAG_ARENA != 0
            {
                let hdr = unsafe { &*(raw as *const ArrayHeader) };
                if hdr.length <= hdr.capacity {
                    return hdr.length;
                }
            }
        }
    }
    // #5135: a Proxy typed (statically) as an array (immer drafts) reaches here
    // with the masked proxy id. Read `length` through the proxy `get` trap
    // rather than deref-ing the id as an `ArrayHeader`.
    if let Some(proxy) = array_ptr_as_proxy(arr) {
        let key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
        let key_f64 = crate::value::js_nanbox_string(key as i64);
        let n = crate::builtins::js_number_coerce(crate::proxy::js_proxy_get(proxy, key_f64));
        return if n.is_finite() && n > 0.0 {
            n.min(u32::MAX as f64) as u32
        } else {
            0
        };
    }
    let arr = {
        let bits = arr as u64;
        let top16 = bits >> 48;
        if top16 >= 0x7FF8 {
            if top16 != (crate::value::POINTER_TAG >> 48) {
                return 0;
            }
            (bits & crate::value::POINTER_MASK) as *const ArrayHeader
        } else {
            arr
        }
    };
    if !arr.is_null() {
        let addr = arr as usize;
        // #7765: gate both probes on the receiver's own type tag — see
        // `js_array_get_f64` for why the tag answers, why it is ABA-proof, and
        // why a header-less buffer receiver still lands on the same result.
        // This reads the byte the `GC_TYPE_LAZY_ARRAY` / `GC_TYPE_OBJECT` block
        // a few lines below already reads, under the same magnitude guard, so
        // it adds no dereference this function did not already perform.
        let receiver_type = array_receiver_gc_tag(arr).0;
        if receiver_type == crate::gc::GC_TYPE_SET && crate::set::is_registered_set(addr) {
            return crate::set::js_set_size(arr as *const crate::set::SetHeader);
        }
        if receiver_type == crate::gc::GC_TYPE_MAP && crate::map::is_registered_map(addr) {
            return crate::map::js_map_size(arr as *const crate::map::MapHeader);
        }
    }
    // Issue #179 Phase 2: lazy array fast path. Check BEFORE
    // `clean_arr_ptr` because that helper rejects pointers whose
    // first two u32s look implausible as (length, capacity) — and a
    // `LazyArrayHeader`'s first fields are (magic, cached_length),
    // which trip the guard. Strip the NaN-box tag manually first.
    unsafe {
        let bits = arr as u64;
        let top16 = bits >> 48;
        let raw_ptr = if top16 >= 0x7FF8 {
            if top16 == 0x7FFC {
                return 0;
            }
            (bits & 0x0000_FFFF_FFFF_FFFF) as *const ArrayHeader
        } else {
            arr
        };
        if !raw_ptr.is_null() && (raw_ptr as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let gc_header =
                (raw_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            // Runtime plain-object receiver behind a statically-Array
            // variable (`var x = []; … x = {0:0}; x.length` — test262
            // splice/S15.4.4.12_A4_T1 #10): reading the ObjectHeader words
            // as (length, capacity) returns garbage. Read the `length`
            // property like any object instead.
            if crate::value::addr_class::is_above_handle_band(raw_ptr as usize)
                && crate::object::is_valid_obj_ptr(raw_ptr as *const u8)
                && ((*gc_header).obj_type == crate::gc::GC_TYPE_OBJECT
                    || (*gc_header).obj_type == crate::gc::GC_TYPE_CLOSURE)
            {
                if let Some(v) = crate::array::subclass::array_subclass_fast_length_raw(raw_ptr) {
                    let n = crate::builtins::js_number_coerce(v);
                    return if n.is_nan() || n <= 0.0 {
                        0
                    } else {
                        n.min(u32::MAX as f64) as u32
                    };
                }
                let key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
                let v = crate::object::js_object_get_field_by_name_f64(
                    raw_ptr as *const crate::object::ObjectHeader,
                    key,
                );
                let n = crate::builtins::js_number_coerce(v);
                return if n.is_nan() || n <= 0.0 {
                    0
                } else {
                    n.min(u32::MAX as f64) as u32
                };
            }
            if (*gc_header).obj_type == crate::gc::GC_TYPE_LAZY_ARRAY {
                let lazy = raw_ptr as *const crate::json_tape::LazyArrayHeader;
                if (*lazy).magic == crate::json_tape::LAZY_ARRAY_MAGIC {
                    // If we've already materialized (e.g. an indexed
                    // access forced it), read the authoritative length
                    // from the materialized tree.
                    if !(*lazy).materialized.is_null() {
                        return (*(*lazy).materialized).length;
                    }
                    return (*lazy).cached_length;
                }
            }
        }
    }
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return 0;
    }
    unsafe { (*arr).length }
}

/// Get the length of an array (i64 bridge for perry-ui-macos)
#[no_mangle]
pub extern "C" fn js_array_get_length(arr: i64) -> i64 {
    js_array_length(arr as *const ArrayHeader) as i64
}

/// Get an element from an array by index (i64 bridge for perry-ui-macos)
#[no_mangle]
pub extern "C" fn js_array_get_element(arr: i64, index: i64) -> f64 {
    js_array_get_f64(arr as *const ArrayHeader, index as u32)
}

/// Alias for js_array_get_element (used by perry-ui-windows dialog)
#[no_mangle]
pub extern "C" fn js_array_get_element_f64(arr: i64, index: i64) -> f64 {
    js_array_get_f64(arr as *const ArrayHeader, index as u32)
}

/// Fast-path array element access: skips all polymorphic registry checks
/// (buffer, set, map). Only does bounds checking and element access.
/// Use when the codegen KNOWS the pointer is a plain Array (not Map/Set/Buffer).
#[no_mangle]
pub extern "C" fn js_array_get_f64_unchecked(arr: *const ArrayHeader, index: u32) -> f64 {
    let cleaned = clean_arr_ptr(arr);
    if cleaned.is_null() {
        // #7574: array-like OBJECT receiver — see `js_array_get_f64`.
        if let Some(value) = crate::array::subclass::array_subclass_fast_index_get_raw(arr, index) {
            return value;
        }
        if crate::array::subclass::array_object_receiver(arr).is_some() {
            return js_array_get_f64(arr, index);
        }
        return f64::NAN;
    }
    let arr = cleaned;
    // Index accessors / custom attrs installed via `Object.defineProperty`
    // need the descriptor-aware getter.
    // SAFETY: `clean_arr_ptr` returned this live head and no safepoint has
    // intervened.
    let flags = unsafe { array_object_flags_resolved(arr) };
    if flags & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0 {
        return js_array_get_f64(arr, index);
    }
    const TAG_UNDEFINED_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0001u64);
    unsafe {
        let length = (*arr).length;
        if index >= length {
            return array_oob_prototype_get(arr as usize, index);
        }
        // Sparse consult only when the index is past the dense backing store:
        // `array_sparse_index_property_get` always returns None below capacity,
        // so checking capacity first keeps the dense hot path call-free.
        if index >= (*arr).capacity {
            if let Some(value) = array_sparse_index_property_get(arr, index) {
                return value;
            }
            return array_oob_prototype_get(arr as usize, index);
        }
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let raw = *elements_ptr.add(index as usize);
        // Issue #323: translate HOLE sentinel (set by `new Array(n)`) back to
        // `undefined`. The sentinel is internal — user code only ever sees
        // TAG_UNDEFINED for unset slots.
        if raw.to_bits() == crate::value::TAG_HOLE {
            return TAG_UNDEFINED_F64;
        }
        raw
    }
}

#[no_mangle]
pub extern "C" fn js_array_numeric_get_f64_unboxed(arr: *mut ArrayHeader, index: u32) -> f64 {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return js_array_get_f64(arr, index);
    }

    // Hot path for guarded raw-f64 arrays. The typed-feedback guard already
    // proved this receiver is a non-forwarded plain Array with raw numeric
    // layout, so keep the helper leaf-small: avoid re-running the expensive
    // rebuild/descriptor path on every indexed read in numeric loops.
    // SAFETY: the clean above resolved this exact live head and no safepoint
    // has intervened.
    let flags = unsafe { array_object_flags_resolved(arr) };
    unsafe {
        if flags & crate::gc::GC_ARRAY_RAW_F64_LAYOUT != 0
            && flags & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS == 0
            && index < (*arr).length
        {
            let elements_ptr =
                (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
            return *elements_ptr.add(index as usize);
        }

        if let Some(value) = array_numeric_raw_f64_get(arr, index) {
            return value;
        }
    }
    js_array_get_f64(arr, index)
}

/// Get an element from an array by index (returns f64)
#[no_mangle]
pub extern "C" fn js_array_get_f64(arr: *const ArrayHeader, index: u32) -> f64 {
    const TAG_UNDEFINED_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0001u64);
    #[cfg(test)]
    ELEMENT_ACCESSOR_CALLS.with(|c| c.set(c.get().wrapping_add(1)));

    // Issue #179 Phase 5: lazy fast path — must run BEFORE
    // `clean_arr_ptr` because that helper force-materializes a lazy
    // pointer into a regular ArrayHeader. For the common read-only
    // shape (`parsed[i]` on a lazy result), force-materializing the
    // whole tree on first access dominates the workload; the sparse
    // per-element cache only materializes the touched subtree.
    //
    // Same tag-strip pattern as `js_array_length`: v0.5.206 added a
    // lazy guard in `clean_arr_ptr` that force-materializes, but
    // for the sparse-cache path we want to keep the LazyArrayHeader
    // around so the cache persists across calls. Strip the NaN-box
    // tag manually and check obj_type without going through the
    // clean-and-validate helper.
    let raw_ptr = {
        let bits = arr as u64;
        let top16 = bits >> 48;
        if top16 >= 0x7FF8 {
            if top16 == 0x7FFC {
                return f64::NAN;
            }
            (bits & 0x0000_FFFF_FFFF_FFFF) as *const ArrayHeader
        } else {
            arr
        }
    };
    unsafe {
        if !raw_ptr.is_null() && (raw_ptr as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let gc_header =
                (raw_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*gc_header).obj_type == crate::gc::GC_TYPE_LAZY_ARRAY {
                let lazy = raw_ptr as *mut crate::json_tape::LazyArrayHeader;
                if (*lazy).magic == crate::json_tape::LAZY_ARRAY_MAGIC {
                    let value = crate::json_tape::lazy_get(lazy, index);
                    return f64::from_bits(value.bits());
                }
            }
        }
    }

    // #7765: ONE `GcHeader` read gates both collection probes and, after the
    // array-only funnel, supplies the descriptor flags which
    // `array_object_flags` used to re-derive through a second `clean_arr_ptr`
    // and a second header read. On `gc-handoff/apps/asyncpipe_big.ts` this call
    // site was 76% of all `is_registered_set` samples and 82% of all
    // `is_registered_map` ones — both registries are non-empty there, so the
    // #7474 latch is armed and each probe really resolves a thread-local and
    // hashes on every ordinary-array element read unless this tag gates it.
    //
    // The tag answers because every registered `Map`/`Set` IS its
    // `arena_alloc_gc(_, _, GC_TYPE_MAP|GC_TYPE_SET)` header, and it is
    // ABA-proof: recycling the address into anything else rewrites the tag
    // before the new pointer is handed out. That is exactly what an
    // address-keyed negative memo could not offer (#7755).
    //
    // A header-less Buffer/TypedArray can expose allocator bookkeeping here,
    // but a coincidental collection tag is harmless: the authoritative
    // registry answers false, and those receivers are routed below.
    //
    // #8060: #8041 correctly made `clean_arr_ptr` reject every tracked
    // non-array. Map/Set indexed reads are an intentional array-like dispatch,
    // though, so classify them before that strict array-only funnel — matching
    // `js_array_length`. The managed-header tag only selects which authority to
    // ask; the registry remains the liveness/layout proof.
    let receiver_tag = array_receiver_gc_tag(raw_ptr);
    if receiver_tag.0 == crate::gc::GC_TYPE_SET && crate::set::is_registered_set(raw_ptr as usize) {
        let set = raw_ptr as *const crate::set::SetHeader;
        unsafe {
            let size = (*set).size;
            if index >= size {
                return TAG_UNDEFINED_F64;
            }
            let elements = (*set).elements as *const f64;
            return std::ptr::read(elements.add(index as usize));
        }
    }
    if receiver_tag.0 == crate::gc::GC_TYPE_MAP && crate::map::is_registered_map(raw_ptr as usize) {
        let map = raw_ptr as *const crate::map::MapHeader;
        unsafe {
            let size = (*map).size;
            if index >= size {
                return TAG_UNDEFINED_F64;
            }
            let entries = (*map).entries as *const f64;
            return std::ptr::read(entries.add(index as usize * 2));
        }
    }

    // A %TypedArray% receiver reaching the generic element read (an untyped
    // `mask[i]` on a `Uint32Array` field) used to pay `clean_arr_ptr`'s
    // tracked-allocation resolver — a guaranteed miss for a typed array —
    // before the registry probe below could route it. The managed header tag
    // already read above selects the typed authority first; the registry
    // remains the liveness/layout proof, exactly as for Map/Set.
    if receiver_tag.0 == crate::gc::GC_TYPE_TYPED_ARRAY
        && crate::typedarray::lookup_typed_array_kind(raw_ptr as usize).is_some()
    {
        return crate::typedarray::js_typed_array_get(
            raw_ptr as *const crate::typedarray::TypedArrayHeader,
            index as i32,
        );
    }

    // An ordinary-object receiver (the object-backed `class X extends Array`
    // instance — the wolf-ecs `Archetype` behind `packed[sparse[x]]`) can
    // never be an `ArrayHeader`, so `clean_arr_ptr`'s tracked-allocation
    // resolver is a guaranteed miss for it. Ask the exact dense-subclass
    // proof first; it re-validates the object header itself. Every rejected
    // case (holes, descriptors, prototype overrides, spilled/unknown layouts)
    // still reaches the complete resolver and spec-generic `Get` below.
    if receiver_tag.0 == crate::gc::GC_TYPE_OBJECT {
        if let Some(value) = crate::array::subclass::array_subclass_fast_index_get_raw(arr, index) {
            return value;
        }
    }

    let cleaned = clean_arr_ptr(arr);
    if cleaned.is_null() {
        // #7574: `a[i]` on a `class X extends Array` instance held in a
        // `T[]`-annotated binding. Read the object's indexed property through
        // the spec-generic `Get`, not the `ObjectHeader` words.
        if let Some(value) = crate::array::subclass::array_subclass_fast_index_get_raw(arr, index) {
            return value;
        }
        if let Some(recv) = crate::array::subclass::array_object_receiver(arr) {
            return crate::array::subclass::array_object_index_get(recv, index);
        }
        return f64::NAN;
    }
    let arr = cleaned;
    // Check if this is actually a TypedArray — dispatch through typed array helper
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_get(
            arr as *const crate::typedarray::TypedArrayHeader,
            index as i32,
        );
    }
    // Check if this is actually a buffer (Uint8Array) — read individual bytes
    if crate::buffer::is_registered_buffer(arr as usize) {
        let byte_val =
            crate::buffer::js_buffer_get(arr as *const crate::buffer::BufferHeader, index as i32);
        return byte_val as f64;
    }
    // The usual case cleans to the same address, so reuse the header tag read
    // above. A forwarded Array resolves to a different address and needs its
    // live head's descriptor flags.
    let receiver_tag = if arr == raw_ptr {
        receiver_tag
    } else {
        array_receiver_gc_tag(arr)
    };
    // #6748 grind: per-array flag, not the process-global gate (see
    // `array_has_own_index`) — this probe allocated two Strings on EVERY
    // checked element read once any descriptor existed process-wide, which
    // taxed every internal keys_array walk (`in`, defineProperty, Object.keys).
    if array_object_flags_from_tag(receiver_tag) & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0 {
        let key = index.to_string();
        if let Some(acc) = crate::object::get_accessor_descriptor(arr as usize, &key) {
            if acc.get != 0 {
                let receiver = crate::value::js_nanbox_pointer(arr as i64);
                return f64::from_bits(
                    unsafe { crate::object::invoke_accessor_getter(acc.get, receiver) }.bits(),
                );
            }
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
    }
    // JS spec: out-of-bounds array access returns `undefined`, not NaN.
    // This matters for destructuring defaults (`const [a, b, c = 30] = [1, 2]`)
    // where the `?? fallback` must see TAG_UNDEFINED, not NaN.
    unsafe {
        let length = (*arr).length;
        if index >= length {
            // Out of bounds: fall through to `Array.prototype[index]` (gated;
            // see `array_oob_prototype_get`). Common case is one atomic load.
            return array_oob_prototype_get(arr as usize, index);
        }
        // Capacity check first: the sparse helper always returns None below
        // capacity, so the dense hot path stays call-free (#4648 put the
        // sparse consult unconditionally first — +28% on 04_array_read).
        if index >= (*arr).capacity {
            if let Some(value) = array_sparse_index_property_get(arr, index) {
                return value;
            }
            return array_oob_prototype_get(arr as usize, index);
        }
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let raw = *elements_ptr.add(index as usize);
        // Issue #323: translate HOLE sentinel back to `undefined` (see
        // `js_array_alloc_with_length` for context). Per OrdinaryGet a hole
        // falls through to the prototype chain — a custom array prototype or
        // an `Array.prototype[i]` element shows through (test262
        // concat/S15.4.4.4_A3_T2 reads `a[2]` with a hole at 2). Both probes
        // are gated (registry lookup / relaxed atomic) so the dense hot path
        // is unchanged.
        if raw.to_bits() == crate::value::TAG_HOLE {
            // #9192: the custom-`[[Prototype]]` probe this arm used to inline
            // accepted only a real-array prototype. `array_oob_prototype_get`
            // now classifies every recorded shape (array / ordinary object /
            // explicit null) behind the same latch, so the duplicate probe is
            // gone and a plain-object prototype fills the hole too.
            return array_oob_prototype_get(arr as usize, index);
        }
        raw
    }
}

/// Relaxed read of the `Array.prototype`-has-indexed-properties flag, for the
/// typed-feedback guards (a polluted prototype invalidates the raw-slot fast
/// path: holes must read through the chain).
pub(crate) fn array_prototype_has_index_flag() -> bool {
    ARRAY_PROTO_HAS_INDEX.load(Ordering::Relaxed)
}

/// Fast-path array element write: skips all polymorphic registry checks
/// (buffer). Only does bounds checking and element write.
/// Use when the codegen KNOWS the pointer is a plain Array (not Buffer).
#[no_mangle]
pub extern "C" fn js_array_set_f64_unchecked(arr: *mut ArrayHeader, index: u32, value: f64) {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return;
    }
    // SAFETY: the clean above resolved this exact live head and no safepoint
    // has intervened.
    let flags = unsafe { array_object_flags_resolved(arr) };
    if flags & crate::gc::OBJ_FLAG_FROZEN != 0 {
        return;
    }
    // Index accessors / non-writable attrs need the descriptor-aware setter.
    if flags & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0 {
        js_array_set_f64_extend(arr, index, value);
        return;
    }
    unsafe {
        let length = (*arr).length;
        if index >= length {
            return;
        }
        if index >= (*arr).capacity {
            array_sparse_index_property_set(arr, index, value);
            return;
        }
        // GC_STORE_AUDIT(BARRIERED): the resolved store performs the layout
        // note and write barrier as part of the slot write.
        store_array_slot_resolved(arr, index as usize, value, flags);
    }
}

#[no_mangle]
pub extern "C" fn js_array_numeric_set_f64_unboxed(
    arr: *mut ArrayHeader,
    index: u32,
    value: f64,
) -> i32 {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return 0;
    }

    // SAFETY: the clean above resolved this exact live head and no safepoint
    // has intervened.
    let flags = unsafe { array_object_flags_resolved(arr) };
    if flags & (crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS) != 0 {
        return 0;
    }

    // Hot path for the codegen's guarded numeric-array store. Raw-f64 arrays
    // are pointer-free, so an in-bounds numeric overwrite can update the
    // payload directly without per-slot layout notes or revalidating/rebuilding
    // the whole layout on every iteration. Preserve the helper fallback for
    // direct runtime calls and arrays that have not been converted yet.
    unsafe {
        if index < (*arr).length && flags & crate::gc::GC_ARRAY_RAW_F64_LAYOUT != 0 {
            let Some(number) = value_bits_to_number(value.to_bits()) else {
                clear_array_numeric_layout(arr);
                return 0;
            };
            let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
            // GC_STORE_AUDIT(POINTER_FREE): RawF64-layout payload slot —
            // `number` is a plain f64, never a NaN-boxed pointer, so no
            // write barrier is needed.
            ptr::write(elements_ptr.add(index as usize), number);
            return 1;
        }

        if array_numeric_raw_f64_set_inbounds(arr, index, value) {
            return 1;
        }
    }
    0
}

// These raw numeric-array helpers are called from generated code, so release/LTO
// builds may otherwise internalize and strip the `#[no_mangle]` exports.
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_ARRAY_NUMERIC_GET_F64_UNBOXED: extern "C" fn(*mut ArrayHeader, u32) -> f64 =
    js_array_numeric_get_f64_unboxed;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_ARRAY_NUMERIC_SET_F64_UNBOXED: extern "C" fn(*mut ArrayHeader, u32, f64) -> i32 =
    js_array_numeric_set_f64_unboxed;

/// Set an element in an array by index
/// Note: This does NOT extend the array if index >= length
#[no_mangle]
pub extern "C" fn js_array_set_f64(arr: *mut ArrayHeader, index: u32, value: f64) {
    // A uniquely-owned string assigned to an element (`arr[i] = s`) aliases this
    // slot — demote it to shared so a later `s += x` doesn't mutate the stored
    // element in place. No-op for SSO / non-string.
    crate::string::js_string_addref_if_heap_string(value);
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return;
    }
    // Check if this is actually a buffer (Uint8Array) — write individual bytes
    if crate::buffer::is_registered_buffer(arr as usize) {
        crate::buffer::js_buffer_set(
            arr as *mut crate::buffer::BufferHeader,
            index as i32,
            value as i32,
        );
        return;
    }
    // Check if this is a typed array — route through per-kind store.
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        crate::typedarray::js_typed_array_set(
            arr as *mut crate::typedarray::TypedArrayHeader,
            index as i32,
            value,
        );
        return;
    }
    // SAFETY: the clean above resolved this exact plain-array head; the
    // Buffer/TypedArray exits precede this direct header read.
    let flags = unsafe { array_object_flags_resolved(arr) };
    if flags & crate::gc::OBJ_FLAG_FROZEN != 0 {
        return;
    }
    unsafe {
        let length = (*arr).length;
        if index >= length {
            return;
        }
        if index >= (*arr).capacity {
            array_sparse_index_property_set(arr, index, value);
            return;
        }
        // GC_STORE_AUDIT(BARRIERED): the resolved store performs the layout
        // note and write barrier as part of the slot write.
        store_array_slot_resolved(arr, index as usize, value, flags);
    }
}

/// Strict-mode `arr[i] = v` — the user-visible element assignment, i.e.
/// `Set(O, ToString(i), v, true)` (PutValue with `Throw = true`). On a frozen
/// array this must throw a **TypeError** instead of silently no-oping: writing
/// an existing index is a read-only violation; adding a new index (or writing
/// any index of a sealed / preventExtensions'd array past its length) is a
/// not-extensible violation.
///
/// Kept separate from `js_array_set_f64_extend` so the *internal* callers of the
/// latter — `Object.defineProperty(arr, i, …)` (which uses it as a raw
/// `[[DefineOwnProperty]]` slot-writer after clearing attrs),
/// `polymorphic_index`, and freshly-allocated runtime arrays — retain their
/// silent, non-throwing contract. Only the `arr[i] = v` assignment codegen
/// (`index_set` / `index` / `field_set_by_name`) routes here.
/// test262 built-ins/Array element/add on frozen|sealed|non-extensible.
/// Strict-mode guard for a would-be `arr[index] = v` element write: throws the
/// spec `Set`-with-`Throw` TypeError when an own data descriptor is read-only,
/// an accessor has no setter, `length` is read-only and would grow, the array
/// is frozen, or a non-extensible array would gain a new element. No-op for
/// writable slots, buffers, and typed arrays (which own their store semantics).
/// Shared by the strict element-write entry points.
#[inline]
pub(crate) fn array_strict_index_write_guard(arr: *mut ArrayHeader, index: u32) {
    let clean = clean_arr_ptr_mut(arr);
    if clean.is_null()
        || crate::buffer::is_registered_buffer(clean as usize)
        || crate::typedarray::lookup_typed_array_kind(clean as usize).is_some()
    {
        return;
    }
    // SAFETY: `clean_arr_ptr_mut` returned this live plain-array head and the
    // registry exits above exclude the compatible header-less receivers.
    let flags = unsafe { array_object_flags_resolved(clean) };
    array_strict_index_write_guard_resolved(clean, index, flags);
}

/// Strict element-write policy check for a live plain array whose header word
/// the caller already owns. This contains no Perry allocation or safepoint, so
/// the same resolved pointer and flags remain valid for the following store.
#[inline]
fn array_strict_index_write_guard_resolved(clean: *mut ArrayHeader, index: u32, flags: u16) {
    let length = unsafe { (*clean).length };

    // A descriptor-bearing array is rare, so keep all key construction and
    // side-table probes off the ordinary dense-array path. An accessor with a
    // setter remains writable even when the object is frozen; return early and
    // let `js_array_set_f64_extend` invoke it. Every other rejected descriptor
    // must throw here because that lower-level helper deliberately retains a
    // silent contract for internal DefineOwnProperty callers.
    if flags & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0 {
        let key = index.to_string();
        if let Some(accessor) = crate::object::get_accessor_descriptor(clean as usize, &key) {
            if accessor.set == 0 {
                throw_frozen_array_index_write(index);
            }
            return;
        }
        if crate::object::get_property_attrs(clean as usize, &key)
            .is_some_and(|attrs| !attrs.writable())
        {
            throw_frozen_array_index_write(index);
        }
        if index >= length
            && crate::object::get_property_attrs(clean as usize, "length")
                .is_some_and(|attrs| !attrs.writable())
        {
            crate::collection_iter::throw_type_error(
                "Cannot assign to read only property 'length' of object '[object Array]'",
            );
        }
    }

    if index < length {
        if flags & crate::gc::OBJ_FLAG_FROZEN != 0 {
            throw_frozen_array_index_write(index);
        }
        // `length` includes holes. Filling one creates a new own property, so
        // sealed/preventExtensions arrays must reject it even though the index
        // is numerically in bounds. This probe is confined to the already-cold
        // restricted-object branch.
        if flags & (crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND) != 0
            && !unsafe { array_has_own_index(clean, index) }
        {
            throw_array_not_extensible_add(index);
        }
    } else if flags
        & (crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND)
        != 0
    {
        // New index on a non-extensible array: cannot add the property.
        throw_array_not_extensible_add(index);
    }
}

/// Fast lane for the dominant element-store shape: a plain number written
/// into an in-bounds slot of a live, unrestricted array whose GC layout is
/// either pointer-free or tag-scanned.
///
/// The general path resolves the receiver through the tracked-header
/// classifier, probes two registries, roots both operands in a handle scope,
/// canonicalizes the value, and then funnels the slot write through the
/// layout note and the write barrier. For this shape every one of those steps
/// is provably a no-op, so it is answered here with a handful of header
/// tests: the receiver is validated the same way `clean_arr_ptr` starts
/// (tag strip, address band) and is then required to sit on a page the arena
/// owns (`classify_heap_generation`, the cached lookup the write barrier
/// itself relies on) before its header is read. Everything else — forwarded
/// stubs, descriptors, frozen/sealed/non-extensible arrays, side-mask or typed
/// or element-shape layouts, typed arrays and buffers, out-of-range indices,
/// tagged or NaN values, `Array.prototype` — returns `false` untouched and
/// takes the general path exactly as before.
///
/// A plain double needs no numeric canonicalization (it is already the raw
/// `f64` the raw-f64 layout stores), cannot be a heap pointer (no barrier, no
/// pointer-mask update), and keeps a pointer-free or tag-scanned layout valid.
#[inline]
pub(crate) unsafe fn try_strict_dense_number_store(
    arr: *mut ArrayHeader,
    index: u32,
    value: f64,
) -> Option<*mut ArrayHeader> {
    const PAYLOAD_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
    let value_bits = value.to_bits();
    // A plain double or an INT32 box (`value_bits_to_number` already refuses
    // the class-reference values that share that tag). NaN keeps the general
    // path so its canonical encoding stays in one place.
    let Some(number) = super::header::value_bits_to_number(value_bits) else {
        return None;
    };
    if number.is_nan() {
        return None;
    }
    let bits = arr as u64;
    let top16 = bits >> 48;
    let raw = if top16 >= 0x7FF8 {
        if top16 == 0x7FFC || bits & PAYLOAD_MASK == 0 {
            return None;
        }
        (bits & PAYLOAD_MASK) as usize
    } else {
        bits as usize
    };
    if raw < crate::gc::GC_HEADER_SIZE
        || raw % std::mem::align_of::<crate::gc::GcHeader>() != 0
        || !crate::value::addr_class::is_plausible_heap_addr(raw)
    {
        return None;
    }
    if matches!(
        crate::arena::classify_heap_generation(raw),
        crate::arena::HeapGeneration::Unknown
    ) {
        return None;
    }
    let mut raw = raw;
    let mut header = (raw - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*header).obj_type != crate::gc::GC_TYPE_ARRAY {
        return None;
    }
    if (*header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
        // An alias that kept a growth stub (the resolver path-compresses
        // longer chains to one edge): follow it once, re-proving the target
        // exactly like the stub. Anything else stays on the full resolver.
        let target = crate::gc::forwarding_address(header) as usize;
        if target < crate::gc::GC_HEADER_SIZE
            || target % std::mem::align_of::<crate::gc::GcHeader>() != 0
            || !crate::value::addr_class::is_plausible_heap_addr(target)
            || matches!(
                crate::arena::classify_heap_generation(target),
                crate::arena::HeapGeneration::Unknown
            )
        {
            return None;
        }
        let target_header = (target - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*target_header).obj_type != crate::gc::GC_TYPE_ARRAY
            || (*target_header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return None;
        }
        raw = target;
        header = target_header;
    }
    let flags = (*header)._reserved;
    // Array header bits only: for `GC_TYPE_ARRAY` the 0x1000 bit is
    // `GC_ARRAY_RAW_F64_HOLES`, not the object typed-layout flag.
    const REJECT: u16 = crate::gc::OBJ_FLAG_FROZEN
        | crate::gc::OBJ_FLAG_SEALED
        | crate::gc::OBJ_FLAG_NO_EXTEND
        | crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS
        | crate::gc::GC_ARRAY_ELEMENT_SHAPE
        | crate::gc::GC_LAYOUT_ALL_POINTERS;
    if flags & REJECT != 0 {
        return None;
    }
    let layout = flags & crate::gc::GC_LAYOUT_STATE_MASK;
    if layout != crate::gc::GC_LAYOUT_POINTER_FREE && layout != 0 {
        return None;
    }
    let arr = raw as *mut ArrayHeader;
    if index >= (*arr).length || index >= (*arr).capacity {
        return None;
    }
    // No registry probes: a `GC_TYPE_ARRAY` header is never a Buffer
    // (`GC_TYPE_BUFFER`), a %TypedArray% (`GC_TYPE_TYPED_ARRAY`) or a native
    // view (`GC_TYPE_NATIVE_TYPED_VIEW`) — every registration carries its own
    // object type — so the obj_type test above already answered both. Only
    // `Array.prototype` itself still needs the address compare: an index
    // write there must flip `ARRAY_PROTO_HAS_INDEX` on the slow path.
    if raw == array_prototype_addr() {
        return None;
    }
    // The raw-f64 layouts store the canonical double (what the general path's
    // canonicalization and `note_array_numeric_index_write` produce); every
    // other layout keeps the value's own encoding.
    let store_bits =
        if flags & (crate::gc::GC_ARRAY_RAW_F64_LAYOUT | crate::gc::GC_ARRAY_RAW_F64_HOLES) != 0 {
            number.to_bits()
        } else {
            value_bits
        };
    let slot = super::header::array_elements_ptr(arr).add(index as usize);
    // #9220: this lane used to fill a hole directly. A hole is not an own
    // property, so an inherited setter / non-writable data descriptor must be
    // consulted before an own element can be created. A raw-f64 DENSE layout
    // proves there are no holes and keeps its bit-for-bit old hot path; every
    // other admitted layout proves ownership with the slot Perry is about to
    // overwrite.
    let may_have_holes = flags & crate::gc::GC_ARRAY_RAW_F64_LAYOUT == 0
        || flags & crate::gc::GC_ARRAY_RAW_F64_HOLES != 0;
    if may_have_holes && ptr::read(slot) == crate::value::TAG_HOLE {
        return None;
    }
    // GC_STORE_AUDIT(POINTER_FREE): a number never holds a heap pointer, and
    // the receiver's layout was proved pointer-free or tag-scanned above.
    ptr::write(slot, store_bits);
    Some(arr)
}

/// Exercised by the unit tests: `true` when the fast lane answered the store.

/// The strict store's third exact lane: an in-range overwrite of a slot that
/// holds a heap pointer with another heap pointer — `column[index] = record`
/// on an ECS archetype's component column, once per command.
///
/// Both number lanes decline a pointer value at their first test, and the
/// store then paid the whole tower: the registry-probing head resolver, a
/// second flag resolution, the descriptor guard, the string add-ref probe,
/// a handle scope, the prototype note and the extend path's own descriptor
/// checks, to reach the same one-slot write. The admission here is the
/// plain-number lane's receiver discipline (the same decode, magnitude,
/// alignment, plausibility and generation classification before the header
/// is read), the same integrity / descriptor / element-shape / raw-f64
/// rejections, plus what a pointer overwrite specifically needs: the head's
/// layout is not pointer-free and the old slot already holds a pointer (its
/// per-slot mask bit is therefore set, or the array is tag-scanned), so the
/// store changes no layout claim. The write itself is
/// [`store_array_slot_resolved`] — the exact layout note and write barrier
/// the general path performs — so what is skipped is bookkeeping the proven
/// shape cannot need, never a barrier. A store onto `Array.prototype` keeps
/// the general path (it must flip `ARRAY_PROTO_HAS_INDEX`).
///
/// # Safety
/// `arr` is decoded and validated before any dereference, exactly as in
/// [`try_strict_dense_number_store`].
pub(crate) unsafe fn try_strict_dense_pointer_overwrite(
    arr: *mut ArrayHeader,
    index: u32,
    value: f64,
) -> Option<*mut ArrayHeader> {
    const PAYLOAD_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
    let value_bits = value.to_bits();
    if value_bits & crate::value::TAG_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let bits = arr as u64;
    let top16 = bits >> 48;
    let raw = if top16 >= 0x7FF8 {
        if top16 == 0x7FFC || bits & PAYLOAD_MASK == 0 {
            return None;
        }
        (bits & PAYLOAD_MASK) as usize
    } else {
        bits as usize
    };
    if raw < crate::gc::GC_HEADER_SIZE
        || raw % std::mem::align_of::<crate::gc::GcHeader>() != 0
        || !crate::value::addr_class::is_plausible_heap_addr(raw)
    {
        return None;
    }
    if matches!(
        crate::arena::classify_heap_generation(raw),
        crate::arena::HeapGeneration::Unknown
    ) {
        return None;
    }
    let header = (raw - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*header).obj_type != crate::gc::GC_TYPE_ARRAY
        || (*header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    let flags = (*header)._reserved;
    const REJECT: u16 = crate::gc::OBJ_FLAG_FROZEN
        | crate::gc::OBJ_FLAG_SEALED
        | crate::gc::OBJ_FLAG_NO_EXTEND
        | crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS
        | crate::gc::GC_ARRAY_ELEMENT_SHAPE
        | crate::gc::GC_ARRAY_RAW_F64_LAYOUT
        | crate::gc::GC_ARRAY_RAW_F64_HOLES;
    if flags & REJECT != 0 {
        return None;
    }
    if flags & crate::gc::GC_LAYOUT_STATE_MASK == crate::gc::GC_LAYOUT_POINTER_FREE {
        return None;
    }
    let arr = raw as *mut ArrayHeader;
    if index >= (*arr).length || index >= (*arr).capacity {
        return None;
    }
    let old_bits = ptr::read(super::header::array_elements_ptr(arr).add(index as usize));
    if old_bits & crate::value::TAG_MASK != crate::value::POINTER_TAG {
        return None;
    }
    if raw == array_prototype_addr() {
        return None;
    }
    // GC_STORE_AUDIT(BARRIERED): the resolved store performs the layout note
    // and write barrier as part of the slot write.
    super::header_gc_slots::store_array_slot_resolved(arr, index as usize, value, flags);
    Some(arr)
}

/// Exercised by the unit tests: `true` when the pointer-overwrite lane
/// answered the store.

#[no_mangle]
pub extern "C" fn js_array_set_f64_extend_strict(
    arr: *mut ArrayHeader,
    index: u32,
    value: f64,
) -> *mut ArrayHeader {
    js_array_set_f64_extend_strict_impl(arr, index, value, false)
}

/// Strict indexed assignment after optionally completing the inherited
/// descriptor walk. `prototype_already_checked` is true only for the callback
/// from [`array_spec_set`]; it prevents a writable inherited data property
/// from recursing when the spec walk proceeds to create the receiver's own
/// element.
fn js_array_set_f64_extend_strict_impl(
    arr: *mut ArrayHeader,
    index: u32,
    value: f64,
    prototype_already_checked: bool,
) -> *mut ArrayHeader {
    // Two exact fast lanes, each storing only what the general path below
    // would store and declining every shape it cannot prove. The plain-number
    // lane (#8885) resolves the head itself, so a hit returns that head; the
    // dense-index lane (#8876) covers the remaining in-range existing-slot
    // stores. The #8885/#8876 composition on `main` had kept only the second,
    // leaving the first unreachable outside its unit tests.
    // SAFETY: the lane validates the receiver before every dereference.
    if let Some(resolved) = unsafe { try_strict_dense_number_store(arr, index, value) } {
        return resolved;
    }
    // SAFETY: as above — the lane validates the receiver before every dereference.
    if let Some(resolved) = unsafe { try_strict_dense_pointer_overwrite(arr, index, value) } {
        return resolved;
    }
    if let Some(resolved) = try_strict_dense_index_set(arr, index, value) {
        return resolved;
    }
    let clean = clean_arr_ptr_mut(arr);
    if clean.is_null()
        || crate::buffer::is_registered_buffer(clean as usize)
        || crate::typedarray::lookup_typed_array_kind(clean as usize).is_some()
    {
        // Preserve the existing polymorphic/subclass behavior on receivers
        // that are not live plain arrays. These are cold and cannot use the
        // resolved-header contract below.
        array_strict_index_write_guard(arr, index);
        return js_array_set_f64_extend(arr, index, value);
    }

    // #9220: only a retargeted array with no own index pays the inherited
    // [[Set]] walk. `array_custom_prototype` is the #9219 classification shared
    // with reads/HasProperty and deliberately returns None for a Proxy
    // prototype, whose dedicated dispatch must remain single-shot. Existing
    // own elements have already had every applicable dense lane above; the
    // fallback still needs the ownership check for descriptor/restricted
    // shapes that correctly declined those lanes.
    if !prototype_already_checked
        && unsafe { array_custom_prototype(clean).is_some() }
        && unsafe { !array_has_own_index(clean, index) }
    {
        return array_spec_set(clean, index, value);
    }

    // SAFETY: the clean above resolved this exact live plain-array head. The
    // guard performs no Perry allocation/safepoint, so the proof remains live
    // for the store core.
    let flags = unsafe { array_object_flags_resolved(clean) };
    array_strict_index_write_guard_resolved(clean, index, flags);
    crate::string::js_string_addref_if_heap_string(value);
    unsafe { js_array_set_f64_extend_resolved(clean, index, value, flags) }
}

/// Complete a strict existing-slot assignment without redispatching
/// the receiver through the guard, extending setter, layout classifier, and
/// write barrier independently.
///
/// This is deliberately narrower than the ordinary dense-array setter:
///
/// - a plain Array must have an existing own dense slot (not a hole); a dense
///   raw-f64 Number-to-Number overwrite takes the metadata-free sub-path;
/// - an object-backed Array subclass must prove the exact dense shape and a
///   writable existing numeric slot through its own guarded fast path; and
/// - frozen arrays, descriptors, growth, holes, and forwarding failures
///   decline to the unchanged strict implementation.
///
/// General values retain the ordinary numeric-layout note, element-shape note,
/// slot-layout update, and write barrier, but reuse the receiver flags already
/// read here instead of reclassifying the Array in each layer.
#[inline]
pub(crate) fn try_strict_dense_index_set(
    arr: *mut ArrayHeader,
    index: u32,
    value: f64,
) -> Option<*mut ArrayHeader> {
    let value_bits = value.to_bits();
    let number = value_bits_to_number(value_bits);
    // Complete the overwhelmingly common Number-to-Number ordinary-Array
    // overwrites from the live header and slot themselves. The generated
    // guarded store already uses this exact magnitude/header discipline; this
    // tier is for dynamic-key sites that reach the feedback helper instead
    // (notably both sparse-set number moves and ECS archetype pointer moves).
    //
    // Both values are constructively classified as Numbers, so the store
    // cannot add or remove a GC edge, change the per-slot pointer mask, demote
    // a unique string, or require a write barrier.  Requiring an existing own
    // non-hole slot plus the same frozen/descriptor/prototype guards as the
    // resolved path preserves every observable assignment case.  Forwarding,
    // growth, sparse holes, accessors and non-number values retain the complete
    // implementation below.
    if number.is_some() {
        if let Some(header) = unsafe { crate::value::addr_class::try_read_gc_header(arr as usize) }
        {
            if header.obj_type == crate::gc::GC_TYPE_ARRAY
                && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
                && header._reserved
                    & (crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS)
                    == 0
                && super::PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED.load(Ordering::Relaxed) == 0
            {
                unsafe {
                    let length = (*arr).length;
                    let capacity = (*arr).capacity;
                    if index < length && length <= capacity && length <= 100_000_000 {
                        let elements = (arr as *mut u8)
                            .add(std::mem::size_of::<ArrayHeader>())
                            .cast::<f64>();
                        let slot = elements.add(index as usize);
                        let old = ptr::read(slot);
                        let old_bits = old.to_bits();
                        if let Some(new_number) = number.filter(|_| {
                            old_bits != crate::value::TAG_HOLE
                                && value_bits_to_number(old_bits).is_some()
                        }) {
                            // GC_STORE_AUDIT(POINTER_FREE): old and new were
                            // constructively decoded as ECMAScript Numbers.
                            ptr::write(slot, new_number);
                            return Some(arr);
                        }
                    }
                }
            }
        }
    }

    // Object-backed Array subclasses are rejected by `clean_arr_ptr_mut`.
    // Ask their exact shape/descriptor proof first so a hit performs only its
    // one validated-object resolution rather than two failed Array cleans.
    if let Some(number) = number {
        if crate::array::subclass::array_subclass_fast_index_set_raw(arr, index, number) {
            return Some(arr);
        }
    }

    let resolved = clean_arr_ptr_mut(arr);
    if resolved.is_null() {
        return None;
    }
    let flags = unsafe { array_object_flags_resolved(resolved) };
    if flags & (crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS) != 0 {
        return None;
    }
    if super::PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED.load(Ordering::Relaxed) != 0 {
        return None;
    }
    unsafe {
        if index >= (*resolved).length || index >= (*resolved).capacity {
            return None;
        }
        let elements = (resolved as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        if flags & crate::gc::GC_ARRAY_RAW_F64_LAYOUT != 0 {
            if let Some(number) = number {
                // GC_STORE_AUDIT(POINTER_FREE): `GC_ARRAY_RAW_F64_LAYOUT`
                // proves the retired value is a Number, and
                // `value_bits_to_number` constructively produced its
                // replacement above.
                ptr::write(elements.add(index as usize), number);
                return Some(resolved);
            }
        }

        // An in-range hole is not an existing own property: a prototype
        // accessor may intercept it and sealed/non-extensible Arrays may
        // reject creating it. The unchanged strict fallback owns that case.
        let slot = elements.add(index as usize);
        let old_bits = ptr::read(slot).to_bits();
        if old_bits == crate::value::TAG_HOLE {
            return None;
        }

        let pointer_tag = crate::value::POINTER_TAG;
        let pointer_mask = crate::value::POINTER_MASK;
        let raw_numeric_flags =
            crate::gc::GC_ARRAY_RAW_F64_LAYOUT | crate::gc::GC_ARRAY_RAW_F64_HOLES;
        if flags & raw_numeric_flags == 0
            && value_bits & crate::value::TAG_MASK == pointer_tag
            && value_bits & pointer_mask != 0
            && old_bits & crate::value::TAG_MASK == pointer_tag
            && old_bits & pointer_mask != 0
        {
            #[cfg(test)]
            STRICT_DENSE_POINTER_OVERWRITE_HITS.with(|hits| hits.set(hits.get().wrapping_add(1)));
            // GC_STORE_AUDIT(BARRIERED): old and new are constructively
            // pointer-bearing, so the slot mask is unchanged. Maintain the
            // independent element proof and the mandatory generational/SATB
            // edge.
            ptr::write(slot, value);
            crate::array::element_shape::note_element_store_resolved_flags(
                resolved,
                index as usize,
                value_bits,
                flags,
            );
            crate::gc::runtime_write_barrier_slot(resolved as usize, slot as usize, value_bits);
            return Some(resolved);
        }

        // A heap string assigned into an existing slot becomes shared before
        // the store, exactly as in `js_array_set_f64_extend`. This call does
        // not allocate or safepoint, so the resolved receiver remains live.
        crate::string::js_string_addref_if_heap_string(value);
        crate::array::note_array_slot_resolved_flags(resolved, index as usize, value, flags);
    }
    Some(resolved)
}

/// Set an element in an array by index, extending the array if needed
/// Returns the (possibly reallocated) array pointer
/// This mimics JavaScript's arr[i] = value behavior
#[no_mangle]
pub extern "C" fn js_array_set_f64_extend(
    arr: *mut ArrayHeader,
    index: u32,
    value: f64,
) -> *mut ArrayHeader {
    // Demote a uniquely-owned string source — see `js_array_set_f64`.
    crate::string::js_string_addref_if_heap_string(value);
    let cleaned = clean_arr_ptr_mut(arr);
    if cleaned.is_null() {
        // #7574: `a[i] = v` on a `class X extends Array` instance held in a
        // `T[]`-annotated binding. Pre-fix this stored the value into
        // `ObjectHeader.keys_array` / `.meta`. Run the object `[[Set]]` plus
        // the Array-exotic `length` maintenance, and return the ORIGINAL
        // receiver so the caller's realloc write-back keeps the binding.
        if crate::array::subclass::array_subclass_fast_index_set_raw(arr, index, value) {
            return arr;
        }
        if let Some(recv) = crate::array::subclass::array_object_receiver(arr) {
            crate::array::subclass::array_object_index_set(recv, index, value);
            return arr;
        }
        return js_array_alloc(0);
    }
    let arr = cleaned;
    // Check if this is actually a buffer (Uint8Array) — write individual bytes
    if crate::buffer::is_registered_buffer(arr as usize) {
        crate::buffer::js_buffer_set(
            arr as *mut crate::buffer::BufferHeader,
            index as i32,
            value as i32,
        );
        return arr;
    }
    // Check if this is a typed array — route through per-kind store (no extension).
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        crate::typedarray::js_typed_array_set(
            arr as *mut crate::typedarray::TypedArrayHeader,
            index as i32,
            value,
        );
        return arr;
    }
    // SAFETY: the clean above resolved this live plain-array head, and the
    // compatible Buffer/TypedArray receivers have exited.
    let flags = unsafe { array_object_flags_resolved(arr) };
    unsafe { js_array_set_f64_extend_resolved(arr, index, value, flags) }
}

/// Plain-array body of [`js_array_set_f64_extend`], entered after one shared
/// ownership/forwarding proof. `flags` is the header word for `arr`.
///
/// # Safety
///
/// `arr` and `flags` must satisfy [`array_object_flags_resolved`]'s contract.
#[inline]
unsafe fn js_array_set_f64_extend_resolved(
    arr: *mut ArrayHeader,
    index: u32,
    value: f64,
    flags: u16,
) -> *mut ArrayHeader {
    // If this write targets `Array.prototype`, mark the prototype as carrying an
    // indexed property so out-of-bounds element reads on ordinary arrays consult
    // it (ECMA-262 OrdinaryGet → prototype chain). Cheap no-op otherwise.
    note_array_index_write(arr as usize);
    let is_frozen = flags & crate::gc::OBJ_FLAG_FROZEN != 0;
    let blocks_extension =
        flags & (crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND) != 0;
    let scope = crate::gc::RuntimeHandleScope::new();
    let _arr_handle = scope.root_raw_mut_ptr(arr);
    let value_handle = scope.root_nanbox_f64(value);
    unsafe {
        let length = (*arr).length;

        if index == u32::MAX {
            return arr;
        }

        // Index properties customized via `Object.defineProperty`: dispatch
        // accessor setters and honor non-writable data attributes before the
        // dense-element store. Gated on the per-array descriptor flag so the
        // common fast path pays one header-flag test.
        if flags & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0 {
            let key = index.to_string();
            if let Some(acc) = crate::object::get_accessor_descriptor(arr as usize, &key) {
                if acc.set != 0 {
                    crate::object::invoke_accessor_setter(
                        acc.set,
                        crate::value::js_nanbox_pointer(arr as i64),
                        value_handle.get_nanbox_f64(),
                    );
                }
                return arr;
            }
            if let Some(attrs) = crate::object::get_property_attrs(arr as usize, &key) {
                if !attrs.writable() {
                    return arr;
                }
            }
            // Extending past `length` requires a writable `length`.
            if index >= length {
                let len_writable = crate::object::get_property_attrs(arr as usize, "length")
                    .map(|a| a.writable())
                    .unwrap_or(true);
                if !len_writable {
                    return arr;
                }
            }
        }

        // If index is within bounds, just set it
        if index < length {
            if is_frozen {
                return arr;
            }
            if index >= (*arr).capacity {
                let value = value_handle.get_nanbox_f64();
                array_sparse_index_property_set(arr, index, value);
                return arr;
            }
            // GC_STORE_AUDIT(BARRIERED): the resolved store performs the
            // layout note and write barrier as part of the slot write.
            store_array_slot_resolved(arr, index as usize, value, flags);
            return arr;
        }

        if is_frozen || blocks_extension {
            return arr;
        }

        // Need to extend the array
        let new_length = index + 1;
        if new_length > (*arr).capacity
            && new_length > MAX_DENSE_ARRAY_GROW_LENGTH
            && index - length > DENSE_ARRAY_GAP_LIMIT
        {
            let value = value_handle.get_nanbox_f64();
            array_sparse_index_property_set(arr, index, value);
            return arr;
        }
        let arr = if new_length > (*arr).capacity {
            js_array_grow(arr, new_length)
        } else {
            arr
        };
        let value = value_handle.get_nanbox_f64();

        // Fill any gap with TAG_HOLE so subsequent reads / iteration /
        // JSON.stringify treat them as holes (per ECMA-262 §22.1.3.30
        // step 5.b: holes serialize to "null"). Pre-fix this wrote 0.0
        // which was indistinguishable from a real numeric 0 — sparse
        // arrays serialized as `[0, 0, ...]` instead of `[null, null,
        // ...]`. Read paths translate TAG_HOLE → TAG_UNDEFINED via
        // `js_array_get_f64`'s post-#323 hole handling.
        //
        // Repsel 4a.2 (#6904): the gap fill goes through the hole-aware
        // note — TAG_HOLE is part of the raw-f64-or-holes invariant, so it
        // must not clear the layout flags the way a genuine non-numeric
        // store does. When the array carried a raw-f64 invariant before the
        // extend AND the stored value is numeric, the invariant still holds
        // afterwards: record it (dense drops to holes) instead of demoting
        // to the permanent O(n) verify walk.
        let had_raw_layout = crate::array::header::array_has_raw_f64_layout_or_holes(arr);
        for i in length..index {
            // GC_STORE_AUDIT(BARRIERED): sparse gap sentinel is layout-noted + barriered by the hole-aware note.
            crate::array::header::note_array_hole_fill_slot(arr, i as usize);
        }

        // Set the value
        // `js_array_grow` may have replaced the backing allocation, so refresh
        // the header word from the returned live head before canonicalizing.
        let store_flags = array_object_flags_resolved(arr);
        // GC_STORE_AUDIT(BARRIERED): the resolved store performs the layout
        // note and write barrier as part of the slot write.
        let value_bits = store_array_slot_resolved(arr, index as usize, value, store_flags);
        (*arr).length = new_length;
        if had_raw_layout
            && index > length
            && crate::array::header::value_bits_are_numeric(value_bits)
        {
            crate::array::header::demote_array_raw_f64_dense_to_holes(arr);
        }

        arr
    }
}
