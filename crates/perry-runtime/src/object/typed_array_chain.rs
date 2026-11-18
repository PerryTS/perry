//! A typed array's `[[Prototype]]` chain read from structure, for the two
//! operations a renderer runs on typed arrays it holds in untyped places:
//! `ta.set(source, offset)` at a method site the compiler could not fold
//! (an Array-annotated or untyped receiver), and `v instanceof Float32Array`.
//! Each answer is exact or `None`; on `None` the caller performs the
//! complete operation.

use super::ObjectHeader;
use crate::typedarray_hook_slots::TypedArrayHeader;
use crate::value::{POINTER_MASK, POINTER_TAG};

/// The intrinsic prototype of typed-array element kind `kind`, if the realm
/// has bound it.
fn kind_prototype(kind: u8) -> Option<usize> {
    use super::BuiltinConstructorIdentity as I;
    use crate::typedarray as k;
    let identity = match kind {
        k::KIND_INT8 => I::Int8Array,
        k::KIND_UINT8 => I::Uint8Array,
        k::KIND_UINT8_CLAMPED => I::Uint8ClampedArray,
        k::KIND_INT16 => I::Int16Array,
        k::KIND_UINT16 => I::Uint16Array,
        k::KIND_INT32 => I::Int32Array,
        k::KIND_UINT32 => I::Uint32Array,
        k::KIND_FLOAT16 => I::Float16Array,
        k::KIND_FLOAT32 => I::Float32Array,
        k::KIND_FLOAT64 => I::Float64Array,
        k::KIND_BIGINT64 => I::BigInt64Array,
        k::KIND_BIGUINT64 => I::BigUint64Array,
        _ => return None,
    };
    let addr = crate::array::intrinsic_prototype_addr_if_bound(identity);
    (addr != 0).then_some(addr)
}

/// The `[[Prototype]]` of the prototype object `obj`: its recorded link, or
/// `%Object.prototype%` when it keeps its default (`object_static_prototype`).
fn prototype_link(obj: usize) -> Option<u64> {
    match super::prototype_chain::object_static_prototype(obj) {
        Some(bits) => Some(bits),
        None => {
            let object_proto = crate::array::object_prototype_addr_if_resolved();
            (object_proto != 0).then_some(POINTER_TAG | object_proto as u64)
        }
    }
}

/// The own string-keyed property `key` of the ordinary object `obj` as its
/// shape answers it: `Some(Some(bits))` a data value, `Some(None)` no own
/// property of that name, `None` when the shape alone does not decide it (an
/// overflow, an accessor, a family layout's key). One probe of the shape's key
/// index (a direct compare for a short list); allocation-free.
///
/// # Safety
/// `obj` is a live `GC_TYPE_OBJECT` cell.
unsafe fn own_data(obj: usize, key: &[u8]) -> Option<Option<u64>> {
    let obj = obj as *const ObjectHeader;
    if super::overflow::has_overflow(obj) {
        return None;
    }
    match super::layout_desc::family_own_key(obj, key) {
        Some(None) => return Some(None),
        Some(Some(_)) => return None,
        None => {}
    }
    let d = super::shapes::object_shape_descriptor(obj)?;
    let count = d.logical_key_count;
    if d.keys == 0 || count == 0 {
        return Some(None);
    }
    let keys_view = d.keys_view();
    // A short list is compared directly, as a method lookup does
    // (`own_slot::find_method_slot`): cheaper than hashing into the index.
    let verdict = if count <= 8 {
        match super::keys_find_slot_by_bytes(keys_view, count, key) {
            Some(pos) => super::shapes::KeysIndexVerdict::Found(pos),
            None => super::shapes::KeysIndexVerdict::Absent,
        }
    } else {
        let hash = super::key_bytes_hash(key.as_ptr(), key.len());
        super::shapes::shape_slot_lookup_verdict(keys_view, key, hash, count, true)
    };
    let pos = match verdict {
        super::shapes::KeysIndexVerdict::Found(pos) => pos,
        super::shapes::KeysIndexVerdict::Absent => return Some(None),
        super::shapes::KeysIndexVerdict::Unindexed => {
            match super::keys_find_slot_by_bytes(keys_view, count, key) {
                Some(pos) => pos,
                None => return Some(None),
            }
        }
    };
    let keys = d.keys as usize as *const crate::array::ArrayHeader;
    if super::key_attrs::key_is_accessor_at(keys, pos) {
        return None;
    }
    let slot = super::key_attrs::key_storage_slot(keys, pos);
    let value =
        super::field_get_set::object_field_at_with_live(obj, slot, d.live_inline_slot_count).bits();
    // A slot holding the hole is an absent key.
    Some((value != crate::value::TAG_HOLE).then_some(value))
}

/// `(kind, kind prototype)` of the typed array `addr` when its
/// `[[Prototype]]` is its kind's intrinsic prototype and it has no own named
/// property: both live only in the cell's property bag (`buffer::store`), so
/// a cell without one has neither.
///
/// # Safety
/// `addr` is any address.
unsafe fn default_chain(addr: usize) -> Option<(u8, usize)> {
    let kind = crate::typedarray_hook_slots::lookup_typed_array_kind(addr)?;
    if !crate::buffer::store::bag(addr).is_null() {
        return None;
    }
    Some((kind, kind_prototype(kind)?))
}

fn typed_array_prototype() -> Option<usize> {
    let addr =
        super::TYPED_ARRAY_INTRINSIC_PROTO_PTR.load(std::sync::atomic::Ordering::Acquire) as usize;
    (addr != 0).then_some(addr)
}

/// The typed array `recv` when `Get(recv, "set")` is `%TypedArray%.prototype.set`,
/// read from structure. Link by link:
///
/// * `recv` is on its default chain ([`default_chain`]), so it has no own
///   `set` and its prototype is its kind's intrinsic prototype;
/// * that prototype has no own `set` and links to `%TypedArray%.prototype`;
/// * `%TypedArray%.prototype` holds `set` as own data, and the value is the
///   realm's `%TypedArray%.prototype.set` function.
///
/// GC leaf: it allocates nothing on the heap and runs no code.
///
/// # Safety
/// `recv` is any NaN-boxed value.
pub(crate) unsafe fn builtin_set_receiver(recv: f64) -> Option<*mut TypedArrayHeader> {
    let bits = recv.to_bits();
    if bits & !POINTER_MASK != POINTER_TAG {
        return None;
    }
    let addr = (bits & POINTER_MASK) as usize;
    let (_, kind_proto) = default_chain(addr)?;
    let ta_proto = typed_array_prototype()?;
    let key = b"set";
    if own_data(kind_proto, key)? != None
        || prototype_link(kind_proto)? != POINTER_TAG | ta_proto as u64
    {
        return None;
    }
    let method_bits = own_data(ta_proto, key)??;
    if method_bits & !POINTER_MASK != POINTER_TAG {
        return None;
    }
    let closure = (method_bits & POINTER_MASK) as *const crate::closure::ClosureHeader;
    let info = crate::closure::closure_info(closure)?;
    let set_thunk: extern "C" fn(
        *const crate::closure::ClosureHeader,
        crate::closure::JsThis,
        f64,
        f64,
        f64,
    ) -> f64 = super::typed_array_proto_thunks::ta_set_thunk;
    (info.code == set_thunk as *const u8).then_some(addr as *mut TypedArrayHeader)
}

/// OrdinaryHasInstance(C, `value`) for `C` the realm's typed-array
/// constructor with instance class id `class_id`, whose `prototype` (not
/// writable, not configurable) is its kind's intrinsic prototype `P`, when
/// `value` is a typed array on its default chain `K.prototype`,
/// `%TypedArray%.prototype`, ...:
///
/// * the same kind: `P` is the first link, `true`;
/// * another kind, while `K.prototype` links to `%TypedArray%.prototype` and
///   that to `%Object.prototype%` (whose `[[Prototype]]` is immutably null):
///   the whole chain is those three objects, none of them `P`, `false`.
///
/// `None` for every other value or chain. Reads no property and runs no code.
///
/// # Safety
/// `value` is any NaN-boxed value.
pub(crate) unsafe fn typed_array_has_instance(value: f64, class_id: u32) -> Option<bool> {
    let first = crate::typedarray_hook_slots::CLASS_ID_INT8_ARRAY;
    let last = crate::typedarray_hook_slots::CLASS_ID_FLOAT16_ARRAY;
    if !(first..=last).contains(&class_id) {
        return None;
    }
    let bits = value.to_bits();
    if bits & !POINTER_MASK != POINTER_TAG {
        return None;
    }
    let (kind, kind_proto) = default_chain((bits & POINTER_MASK) as usize)?;
    let target = (class_id - first) as u8;
    if kind == target {
        return Some(true);
    }
    let ta_proto = typed_array_prototype()?;
    let object_proto = crate::array::object_prototype_addr_if_resolved();
    (object_proto != 0
        && prototype_link(kind_proto)? == POINTER_TAG | ta_proto as u64
        && prototype_link(ta_proto)? == POINTER_TAG | object_proto as u64)
        .then_some(false)
}
