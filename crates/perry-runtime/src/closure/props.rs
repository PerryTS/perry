//! A function object's own properties, stored IN the function object (D1).
//!
//! `ClosureHeader::props` points at the function's BAG: a runtime-internal,
//! null-prototype `ObjectHeader` whose keys and slots are the function's own
//! string-keyed data properties in ordinary creation order. It is created on
//! the first own-property write and is a traced, rewritten raw-pointer child
//! edge of the closure (`gc::layout`'s `ClosureCaptures` arm), so it moves
//! and dies with the function — no address-keyed table, no re-key hook, no
//! dead-owner prune, no young log.
//!
//! The bag's own `ObjectMeta.expando` (an ordinary child edge of the bag)
//! holds the function's rare internal STATE record, another null-prototype
//! object: the #3655 deleted-synthesized-key markers (`"d:" + key`) and the
//! recorded `[[Prototype]]` (`"p"`). Its keys are namespaced, and it is never
//! reachable from JS.
//!
//! Writers run under `GcSuppressScope`: the ~100 callers of
//! `closure_set_dynamic_prop` hold raw closure addresses across the call, as
//! they always could when the store was a Rust `HashMap`, so the allocations
//! here (bag, key string, state record) must not move anything.
use super::ClosureHeader;
use crate::object::ObjectHeader;
use crate::value::JSValue;

const STATE_PROTO: &str = "p";
const DELETED_PREFIX: &str = "d:";
const INTERNAL_PREFIX: &str = "i:";

/// The bag of the closure at `ptr` (null when it never had an own property).
///
/// # Safety
/// `ptr` is a proven, live closure cell.
#[inline]
pub(crate) unsafe fn bag_of(ptr: usize) -> *mut ObjectHeader {
    (*(ptr as *const ClosureHeader)).props
}

/// Allocate the bag if absent and install it with the store barrier.
pub(crate) unsafe fn bag_ensure(ptr: usize) -> *mut ObjectHeader {
    let existing = bag_of(ptr);
    if !existing.is_null() {
        return existing;
    }
    let bag = crate::object::js_object_alloc_null_proto(0, 0);
    let closure = ptr as *mut ClosureHeader;
    // A closure is born `GC_LAYOUT_POINTER_FREE` when its captures hold no
    // pointer; some collector paths treat that state as "no child edge at
    // all". It now has one, so it leaves that state (#7630's tag-checked
    // `UNKNOWN`, the state a pointer capture store would also produce).
    crate::gc::layout_note_closure_edge_installed(ptr as *mut u8);
    // GC_STORE_AUDIT(BARRIERED): header raw-pointer edge store + object-slot
    // barrier, mirroring `object_meta_ensure`'s `meta` install.
    (*closure).props = bag;
    crate::gc::runtime_write_barrier_slot(ptr, &(*closure).props as *const _ as usize, bag as u64);
    bag
}

/// Own data lookup in an ordinary (or dictionary-mode) bag by key bytes.
unsafe fn object_own_get(obj: *const ObjectHeader, key: &[u8]) -> Option<f64> {
    // One shape lookup for an ordinary bag: keys, count and inline bound all
    // come from the same descriptor.
    if let Some(d) = crate::object::shapes::object_shape_descriptor(obj) {
        if d.object_kind == crate::object::shapes::ShapeObjectKind::Ordinary && d.keys != 0 {
            let keys = d.keys as usize as *const crate::array::ArrayHeader;
            let slot =
                crate::object::keys_find_slot_by_bytes_resolved(keys, d.logical_key_count, key)?;
            // An accessor key's slot holds its getter/setter pair, never a
            // data value.
            if crate::object::key_attrs::key_is_accessor_at(keys, slot as u32) {
                return None;
            }
            let value =
                crate::object::object_field_at_with_live(obj, slot, d.live_inline_slot_count);
            if value.bits() == crate::value::TAG_HOLE {
                return None;
            }
            return Some(f64::from_bits(value.bits()));
        }
    }
    let keys = crate::object::object_keys(obj);
    let arr = keys.arr();
    if arr.is_null() {
        return None;
    }
    let slot = crate::object::keys_find_slot_by_bytes_resolved(arr, keys.count(), key)?;
    if crate::object::key_attrs::key_is_accessor_at(arr, slot as u32) {
        return None;
    }
    let live = crate::object::object_live_slot_count(obj);
    let value = crate::object::object_field_at_with_live(obj, slot, live);
    if value.bits() == crate::value::TAG_HOLE {
        return None;
    }
    Some(f64::from_bits(value.bits()))
}

/// The function's own data property `key`, if present.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn bag_get(ptr: usize, key: &[u8]) -> Option<f64> {
    let bag = bag_of(ptr);
    if bag.is_null() {
        return None;
    }
    object_own_get(bag, key)
}

unsafe fn object_own_set(obj: *mut ObjectHeader, key: &str, value: f64) {
    let key = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    crate::object::js_object_set_field_by_name(obj, key, value);
}

/// Define/overwrite the function's own data property `key` (plain `[[Set]]`
/// on the null-prototype bag: no inherited setter can run).
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn bag_set(ptr: usize, key: &str, value: f64) {
    let _no_move = crate::gc::GcSuppressScope::new();
    let bag = bag_ensure(ptr);
    object_own_set(bag, key, value);
}

/// Remove the function's own data property `key`; true when it existed.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn bag_remove(ptr: usize, key: &str) -> bool {
    let bag = bag_of(ptr);
    if bag.is_null() || !bag_has_own(ptr, key.as_bytes()) {
        return false;
    }
    let _no_move = crate::gc::GcSuppressScope::new();
    let key_hdr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    crate::object::js_object_delete_field(bag, key_hdr);
    true
}

/// Does the function own `key` — a data OR an accessor property?
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn bag_has_own(ptr: usize, key: &[u8]) -> bool {
    let bag = bag_of(ptr);
    if bag.is_null() {
        return false;
    }
    let keys = crate::object::object_keys(bag);
    let arr = keys.arr();
    if arr.is_null() {
        return false;
    }
    let Some(slot) = crate::object::keys_find_slot_by_bytes_resolved(arr, keys.count(), key) else {
        return false;
    };
    crate::object::key_attrs::key_is_accessor_at(arr, slot as u32)
        || crate::object::object_field_at_with_live(
            bag,
            slot,
            crate::object::object_live_slot_count(bag),
        )
        .bits()
            != crate::value::TAG_HOLE
}

/// The function's own ACCESSOR property names, in creation order.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn bag_accessor_names(ptr: usize) -> Vec<String> {
    let bag = bag_of(ptr);
    if bag.is_null() {
        return Vec::new();
    }
    let keys = crate::object::object_keys(bag);
    let arr = keys.arr();
    if arr.is_null() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for i in 0..keys.count() {
        if !crate::object::key_attrs::key_is_accessor_at(arr, i) {
            continue;
        }
        let key = JSValue::from_bits(crate::array::js_array_get_f64(arr, i).to_bits());
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        if let Some(bytes) = crate::string::js_string_key_bytes(key, &mut scratch) {
            out.push(String::from_utf8_lossy(bytes).into_owned());
        }
    }
    out
}

/// Every own data property in ECMA-262 own-key order: integer indices
/// ascending, then other strings in creation order.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn bag_snapshot(ptr: usize) -> Vec<(String, f64)> {
    let bag = bag_of(ptr);
    if bag.is_null() {
        return Vec::new();
    }
    let keys = crate::object::object_keys(bag);
    let arr = keys.arr();
    if arr.is_null() {
        return Vec::new();
    }
    let live = crate::object::object_live_slot_count(bag);
    let mut indexed: Vec<(u32, String, f64)> = Vec::new();
    let mut strings: Vec<(String, f64)> = Vec::new();
    for i in 0..keys.count() {
        let value = crate::object::object_field_at_with_live(bag, i, live);
        if value.bits() == crate::value::TAG_HOLE
            || crate::object::key_attrs::key_is_accessor_at(arr, i)
        {
            continue;
        }
        let key = JSValue::from_bits(crate::array::js_array_get_f64(arr, i).to_bits());
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let Some(bytes) = crate::string::js_string_key_bytes(key, &mut scratch) else {
            continue;
        };
        let name = String::from_utf8_lossy(bytes).into_owned();
        let v = f64::from_bits(value.bits());
        match crate::object::canonical_array_index(&name) {
            Some(index) => indexed.push((index, name, v)),
            None => strings.push((name, v)),
        }
    }
    crate::cold_sort::sort_by_key(&mut indexed, |(index, _, _)| *index);
    indexed
        .into_iter()
        .map(|(_, key, value)| (key, value))
        .chain(strings)
        .collect()
}

/// The closure's internal state record (null when none).
unsafe fn state_of(ptr: usize) -> *mut ObjectHeader {
    let bag = bag_of(ptr);
    if bag.is_null() {
        return std::ptr::null_mut();
    }
    crate::object::cell_expando_get(bag as usize).unwrap_or(std::ptr::null_mut())
}

unsafe fn state_ensure(ptr: usize) -> Option<*mut ObjectHeader> {
    let bag = bag_ensure(ptr);
    let existing = state_of(ptr);
    if !existing.is_null() {
        return Some(existing);
    }
    // A null-prototype record (its `"p"` key is not a JS property, and no
    // inherited setter may run on it), hung off the bag's `meta.expando` —
    // an ordinary traced edge of the bag.
    let meta = crate::object::object_meta_ensure(bag);
    if meta.is_null() {
        return None;
    }
    let state = crate::object::js_object_alloc_null_proto(0, 0);
    let meta = (*bag).meta;
    let boxed = crate::value::js_nanbox_pointer(state as i64).to_bits();
    // GC_STORE_AUDIT(BARRIERED): metadata-record slot store + object barrier.
    (*meta).expando = boxed;
    crate::gc::runtime_write_barrier_slot(
        meta as usize,
        &(*meta).expando as *const _ as usize,
        boxed,
    );
    Some(state)
}

/// Is the #3655 deleted marker for `key` set on the closure at `ptr`?
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn state_is_deleted(ptr: usize, key: &str) -> bool {
    let state = state_of(ptr);
    if state.is_null() {
        return false;
    }
    let mut marker = String::with_capacity(DELETED_PREFIX.len() + key.len());
    marker.push_str(DELETED_PREFIX);
    marker.push_str(key);
    object_own_get(state, marker.as_bytes()).is_some()
}

/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn state_mark_deleted(ptr: usize, key: &str) {
    let _no_move = crate::gc::GcSuppressScope::new();
    let Some(state) = state_ensure(ptr) else {
        return;
    };
    object_own_set(
        state,
        &format!("{DELETED_PREFIX}{key}"),
        f64::from_bits(crate::value::TAG_TRUE),
    );
}

/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn state_clear_deleted(ptr: usize, key: &str) {
    if !state_is_deleted(ptr, key) {
        return;
    }
    let _no_move = crate::gc::GcSuppressScope::new();
    let state = state_of(ptr);
    let marker = format!("{DELETED_PREFIX}{key}");
    let key_hdr = crate::string::js_string_from_bytes(marker.as_ptr(), marker.len() as u32);
    crate::object::js_object_delete_field(state, key_hdr);
}

/// The recorded `[[Prototype]]` bits, if any.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn state_prototype(ptr: usize) -> Option<u64> {
    let state = state_of(ptr);
    if state.is_null() {
        return None;
    }
    object_own_get(state, STATE_PROTO.as_bytes()).map(f64::to_bits)
}

/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn state_set_prototype(ptr: usize, proto_bits: u64) {
    let _no_move = crate::gc::GcSuppressScope::new();
    let Some(state) = state_ensure(ptr) else {
        return;
    };
    object_own_set(state, STATE_PROTO, f64::from_bits(proto_bits));
}

/// A runtime-internal own slot of the function object — never a JS property
/// (class private statics, computed-key records, class captures): kept in the
/// state record under `"i:" + key`, so no reflection or enumeration of the
/// function can reach it.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn state_internal_get(ptr: usize, key: &str) -> Option<f64> {
    let state = state_of(ptr);
    if state.is_null() {
        return None;
    }
    let mut marker = String::with_capacity(INTERNAL_PREFIX.len() + key.len());
    marker.push_str(INTERNAL_PREFIX);
    marker.push_str(key);
    object_own_get(state, marker.as_bytes())
}

/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn state_internal_set(ptr: usize, key: &str, value: f64) {
    let _no_move = crate::gc::GcSuppressScope::new();
    let Some(state) = state_ensure(ptr) else {
        return;
    };
    object_own_set(state, &format!("{INTERNAL_PREFIX}{key}"), value);
}

/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn state_internal_remove(ptr: usize, key: &str) -> bool {
    if state_internal_get(ptr, key).is_none() {
        return false;
    }
    let _no_move = crate::gc::GcSuppressScope::new();
    let state = state_of(ptr);
    let marker = format!("{INTERNAL_PREFIX}{key}");
    let key_hdr = crate::string::js_string_from_bytes(marker.as_ptr(), marker.len() as u32);
    crate::object::js_object_delete_field(state, key_hdr);
    true
}

/// True when the closure carries internal state a base/keyed Function shape
/// cannot describe (a deleted marker or a recorded prototype).
///
/// # Safety
/// `ptr` is a proven, live closure cell.
pub(crate) unsafe fn has_state(ptr: usize) -> bool {
    !state_of(ptr).is_null()
}
