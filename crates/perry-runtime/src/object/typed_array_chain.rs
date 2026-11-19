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
/// the chain is walked link by link from `K.prototype` while each hop is a
/// kind prototype or `%TypedArray%.prototype` (intrinsics whose link the
/// realm records, relinked or not): `P` met is `true`; `%Object.prototype%`
/// (whose `[[Prototype]]` is immutably null) or null met is `false`.
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
    let (_, kind_proto) = default_chain((bits & POINTER_MASK) as usize)?;
    let target = kind_prototype((class_id - first) as u8)?;
    // The chain from `K.prototype`, link by link, while each hop is one of the
    // intrinsics whose link the realm records (a kind prototype or
    // `%TypedArray%.prototype`, relinked or not); `%Object.prototype%`'s
    // `[[Prototype]]` is immutably null. A hop elsewhere is the complete walk's.
    let ta_proto = typed_array_prototype()?;
    let object_proto = crate::array::object_prototype_addr_if_resolved();
    let mut hop = kind_proto;
    for step in 0..16 {
        if hop == target {
            return Some(true);
        }
        if object_proto != 0 && hop == object_proto {
            return Some(false);
        }
        // The first hop is the value's own kind prototype.
        if step != 0 && hop != ta_proto && !is_kind_prototype(hop) {
            return None;
        }
        let link = prototype_link(hop)?;
        if link == crate::value::TAG_NULL {
            return Some(false);
        }
        if link & !POINTER_MASK != POINTER_TAG {
            return None;
        }
        hop = (link & POINTER_MASK) as usize;
    }
    None
}

/// Is `addr` one of the realm's typed-array kind prototypes?
fn is_kind_prototype(addr: usize) -> bool {
    use crate::typedarray as k;
    [
        k::KIND_INT8,
        k::KIND_UINT8,
        k::KIND_UINT8_CLAMPED,
        k::KIND_INT16,
        k::KIND_UINT16,
        k::KIND_INT32,
        k::KIND_UINT32,
        k::KIND_FLOAT16,
        k::KIND_FLOAT32,
        k::KIND_FLOAT64,
        k::KIND_BIGINT64,
        k::KIND_BIGUINT64,
    ]
    .into_iter()
    .any(|kind| kind_prototype(kind) == Some(addr))
}

/// [`typed_array_has_instance`] for every value a typed-array constructor
/// meets in practice, answered from structure (`P`, its kind's intrinsic
/// prototype, is a typed-array prototype: no other object is it):
///
/// * a primitive: OrdinaryHasInstance step 3, `false`;
/// * a typed array on its default chain: [`typed_array_has_instance`];
/// * an Array with no recorded `[[Prototype]]`: its chain is
///   `%Array.prototype%` (a locked intrinsic, never relinked) and
///   `%Object.prototype%` (immutably null-linked), neither `P`: `false`;
/// * a plain record (no class surface) whose shape names the realm's
///   `%Object.prototype%` and which records no other link: `false`.
///
/// `None` for every other value (a proxy, a class instance, a relinked
/// object, a function): the complete operation answers.
///
/// # Safety
/// `value` is any NaN-boxed value.
pub(crate) unsafe fn typed_array_constructor_has_instance(
    value: f64,
    class_id: u32,
) -> Option<bool> {
    let first = crate::typedarray_hook_slots::CLASS_ID_INT8_ARRAY;
    let last = crate::typedarray_hook_slots::CLASS_ID_FLOAT16_ARRAY;
    if !(first..=last).contains(&class_id) {
        return None;
    }
    let bits = value.to_bits();
    if bits & !POINTER_MASK != POINTER_TAG {
        // No heap pointer: a Number, a boolean, nullish, a short string, an
        // INT32 word (a class reference is a function: refused below by
        // its tag, never answered here).
        let js = crate::JSValue::from_bits(bits);
        return (js.is_number() && bits >> 48 != 0
            || js.is_bool()
            || js.is_null()
            || js.is_undefined()
            || js.is_short_string())
        .then_some(false);
    }
    let addr = (bits & POINTER_MASK) as usize;
    if let Some(answer) = typed_array_has_instance(value, class_id) {
        return Some(answer);
    }
    let header = crate::value::addr_class::try_read_gc_header(addr)?;
    if header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
        return None;
    }
    if crate::gc_type_is_array!(header.obj_type) {
        return super::prototype_chain::object_static_prototype(addr)
            .is_none()
            .then_some(false);
    }
    if crate::gc_type_is_object!(header.obj_type) {
        let obj = addr as *const ObjectHeader;
        let record = super::shapes::object_shape_record(obj)?;
        return (record.kind_is(super::shapes::ShapeObjectKind::Ordinary)
            || record.kind_is(super::shapes::ShapeObjectKind::OrdinaryIndexed))
        .then_some(())
        .filter(|_| {
            record.proto_id() == super::shapes::PROTO_ID_DEFAULT
                && super::shapes::object_prototype_word(obj as *mut ObjectHeader) == 0
                && super::shapes::object_is_plain_record(obj)
        })
        .map(|_| false);
    }
    None
}

/// `recv.set(source, offset)` at a site the compiler folded to the typed-array
/// copy (an Array- or TypedArray-typed receiver): the copy when
/// `Get(recv, "set")` is `%TypedArray%.prototype.set` ([`builtin_set_receiver`]),
/// else the ordinary method call of whatever `Get(recv, "set")` is, with the
/// site's `argc` arguments (a user `set` on the receiver, its kind prototype
/// or a relinked chain; on an Array, the missing method's TypeError).
///
/// # Safety
/// `recv`, `source` and `offset` are NaN-boxed values; `argc` is 0, 1 or 2.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_typed_array_set_call(
    recv: f64,
    source: f64,
    offset: f64,
    argc: i32,
) -> f64 {
    if let Some(ta) = builtin_set_receiver(recv) {
        let offset = if argc > 1 { offset } else { 0.0 };
        return crate::typedarray_hook_slots::js_typed_array_set_from(ta, source, offset);
    }
    let args = [source, offset];
    crate::object::js_native_call_method(
        recv,
        b"set".as_ptr() as *const i8,
        3,
        args.as_ptr(),
        argc.clamp(0, 2) as usize,
    )
}
