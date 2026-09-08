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
                return array_object_proto_index_get(
                    crate::value::js_nanbox_pointer(receiver as i64),
                    bits,
                    index,
                )
                .unwrap_or(TAG_UNDEFINED_F64)
            }
            Some(ArrayCustomProto::Array(proto_arr)) => {
                return array_spec_get_with_receiver(
                    proto_arr,
                    index,
                    crate::value::js_nanbox_pointer(receiver as i64),
                );
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
        // chain. Every custom prototype answers the whole lookup, including
        // an array whose own prototype may be retargeted or null (#9785).
        match array_custom_prototype(arr) {
            Some(ArrayCustomProto::Null) => return false,
            Some(ArrayCustomProto::Other(bits)) => {
                return array_object_proto_index_has(bits, index)
            }
            Some(ArrayCustomProto::Array(proto_arr)) => {
                return array_spec_has_index(proto_arr, index);
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
    /// The recorded prototype is itself a real array. Its own prototype is
    /// authoritative after an own-index miss, just as for any other object.
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
    // Indexed reads and array algorithms must dispatch the Proxy's internal
    // methods too. Returning None here incorrectly restored the default
    // Array.prototype chain and skipped the traps (#9786).
    if crate::proxy::js_proxy_is_proxy(f64::from_bits(bits)) != 0 {
        return Some(ArrayCustomProto::Other(bits));
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
unsafe fn array_object_proto_index_get(receiver: f64, proto_bits: u64, index: u32) -> Option<f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
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

/// Find the first own indexed property in the actual custom prototype chain.
/// Stop at writable data too: it shadows a non-writable ancestor. The runtime's
/// GetPrototypeOf handles real arrays and synthetic Object.create prototypes
/// without interpreting an ArrayHeader as an ObjectHeader (#9785).
pub(crate) unsafe fn array_object_proto_index_owner(proto_bits: u64, key: &str) -> usize {
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_heap_word_u64(proto_bits);
    let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    if key_ptr.is_null() {
        return 0;
    }
    let key_handle = scope.root_nanbox_f64(crate::value::nanbox_string_key(key_ptr));
    for _ in 0..64 {
        let bits = proto.get_heap_word_u64();
        if bits == crate::value::TAG_NULL
            || crate::proxy::js_proxy_is_proxy(f64::from_bits(bits)) != 0
        {
            return 0;
        }
        let Some(addr) = pointer_bits_of_recorded_prototype(bits) else {
            return 0;
        };
        if !crate::value::addr_class::is_above_handle_band(addr)
            || !crate::object::is_valid_obj_ptr(addr as *const u8)
        {
            return 0;
        }
        let addr = crate::value::resolve_forwarding(addr);
        let value = crate::value::js_nanbox_pointer(addr as i64);
        proto.set_heap_word_u64(value.to_bits());
        if crate::object::obj_value_has_own_key(value, key_handle.get_nanbox_f64()) {
            return crate::value::js_nanbox_get_pointer(f64::from_bits(proto.get_heap_word_u64()))
                as usize;
        }
        let next =
            crate::object::js_object_get_prototype_of(f64::from_bits(proto.get_heap_word_u64()));
        if next.to_bits() == proto.get_heap_word_u64() {
            return 0;
        }
        proto.set_heap_word_u64(next.to_bits());
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
    let arr = clean_arr_ptr(arr);
    array_spec_get_with_receiver(arr, index, crate::value::js_nanbox_pointer(arr as i64))
}

fn array_spec_get_with_receiver(arr: *const ArrayHeader, index: u32, receiver: f64) -> f64 {
    const TAG_UNDEFINED_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0001u64);
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return TAG_UNDEFINED_F64;
    }
    unsafe {
        let scope = crate::gc::RuntimeHandleScope::new();
        let receiver = scope.root_nanbox_f64(receiver);
        if array_has_own_index(arr, index) {
            return array_inherited_index_get(arr, index, receiver.get_nanbox_f64());
        }
        // #9192: see `array_spec_has_index` — a non-array custom prototype
        // replaces the default chain outright.
        match array_custom_prototype(arr) {
            Some(ArrayCustomProto::Null) => return TAG_UNDEFINED_F64,
            Some(ArrayCustomProto::Other(bits)) => {
                return array_object_proto_index_get(receiver.get_nanbox_f64(), bits, index)
                    .unwrap_or(TAG_UNDEFINED_F64)
            }
            Some(ArrayCustomProto::Array(proto_arr)) => {
                return array_spec_get_with_receiver(proto_arr, index, receiver.get_nanbox_f64());
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
