//! Spec-level indexed `[[Get]]` / `[[HasProperty]]` / `[[Set]]` for an Array
//! receiver, including the recorded custom `[[Prototype]]` classification
//! (#9192/#9219) and the inherited-descriptor walk an indexed assignment must
//! perform before it may create an own element (#9220/#9221).
//!
//! Split out of `indexing.rs` to keep that file under the repo's 2000-line cap;
//! a pure move. Declared as a CHILD of `indexing`, so parent-private helpers
//! (`clean_arr_ptr`, the prototype-index latches, the strict store entry) stay
//! reachable through `super::*` without widening any visibility.
use super::*;
use std::sync::atomic::Ordering;

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
pub(super) unsafe fn array_oob_prototype_get(receiver: usize, index: u32) -> f64 {
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
