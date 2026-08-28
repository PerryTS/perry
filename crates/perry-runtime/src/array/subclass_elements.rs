//! Elements backing store for `class X extends Array` instances.
//!
//! An Array-subclass instance is an ordinary `GC_TYPE_OBJECT` (`super::subclass`);
//! today its indexed elements and `length` are shape-carried properties, so every
//! `push`/`pop`/`obj[i] = v` is a property-shape transition. Under
//! [`array_subclass_elements_enabled`] the instance instead owns a real
//! `GC_TYPE_ARRAY` in `ObjectMeta.elements` (a traced child edge exactly like
//! `spill`) holding its indexed elements and `length`, and the property entry
//! points route canonical array-index keys and `length` to it.
//!
//! This module owns the edge: the gate, the accessor, installation at
//! construction, and the barriered head write-back after a re-allocating append.
use crate::array::ArrayHeader;
use crate::object::ObjectHeader;

use super::subclass::{mutation_receiver_allows_plain_tail, ValidatedObjectReceiver};

/// `PERRY_ARRAY_SUBCLASS_ELEMENTS=1|on|true` — off while the property entry
/// points are being routed; the default flips once the semantics suite is green.
#[inline]
pub(crate) fn array_subclass_elements_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        matches!(
            std::env::var("PERRY_ARRAY_SUBCLASS_ELEMENTS").as_deref(),
            Ok("1") | Ok("on") | Ok("true")
        )
    })
}

/// The elements store of a live `GC_TYPE_OBJECT`, or null when it has none
/// (no meta record, or not an elements-backed Array subclass instance).
///
/// # Safety
/// `obj` must be a live `GC_TYPE_OBJECT` user pointer.
#[inline]
pub(crate) unsafe fn elements_of(obj: *const ObjectHeader) -> *mut ArrayHeader {
    let meta = (*obj).meta;
    if meta.is_null() {
        return std::ptr::null_mut();
    }
    (*meta).elements as *mut ArrayHeader
}

/// Store `elements` as the instance's backing store (a barriered meta-record
/// slot store; `elements` may be null to detach, e.g. on deopt).
///
/// # Safety
/// `obj` must be a live `GC_TYPE_OBJECT` with a meta record.
#[inline]
pub(crate) unsafe fn set_elements_head(obj: *mut ObjectHeader, elements: *mut ArrayHeader) {
    let meta = (*obj).meta;
    debug_assert!(!meta.is_null());
    // GC_STORE_AUDIT(BARRIERED): meta-record child edge, stored exactly as
    // `reserve_object_spill` stores `spill`.
    (*meta).elements = elements as u64;
    crate::gc::runtime_write_barrier_slot(
        meta as usize,
        &(*meta).elements as *const _ as usize,
        elements as u64,
    );
}

/// Install a fresh elements store of `length` holes on `obj` (the `super(n)`
/// shape: `length = n`, every index absent). Idempotent: an instance that
/// already has a store keeps it.
///
/// # Safety
/// `obj` must be a live `GC_TYPE_OBJECT` user pointer.
pub(crate) unsafe fn install_elements(obj: *mut ObjectHeader, length: u32) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let (_, obj) =
        obj_handle.across_mut::<ObjectHeader, _>(|| crate::object::object_meta_ensure(obj));
    if !elements_of(obj).is_null() {
        return;
    }
    let (elements, obj) = obj_handle
        .across_mut::<ObjectHeader, _>(|| crate::array::js_array_alloc_with_length_exact(length));
    if elements_of(obj).is_null() {
        set_elements_head(obj, elements);
    }
}

// ---------------------------------------------------------------------------
// The property funnel: every ordinary-object entry point that can see an
// elements-backed instance's indexed properties or `length` asks here first.
// ---------------------------------------------------------------------------

/// A property key an elements-backed instance answers itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ElementsKey {
    Index(u32),
    Length,
}

/// The live `(object, elements)` pair behind a raw or NaN-box-tagged object
/// address, or `None` for anything that is not an elements-backed instance.
///
/// # Safety
/// `addr` must be a raw user pointer or a POINTER-tagged NaN-box; it is
/// classified before any dereference.
pub(crate) unsafe fn backed(addr: usize) -> Option<(*mut ObjectHeader, *mut ArrayHeader)> {
    let bits = addr as u64;
    let raw = if (bits >> 48) == 0x7FFD {
        (bits & crate::value::POINTER_MASK) as usize
    } else {
        addr
    };
    let header = crate::value::addr_class::try_read_gc_header(raw)?;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    let obj = raw as *mut ObjectHeader;
    let elements = elements_of(obj);
    (!elements.is_null()).then_some((obj, elements))
}

/// [`backed`] for a NaN-boxed value.
pub(crate) fn backed_value(value: f64) -> Option<(*mut ObjectHeader, *mut ArrayHeader)> {
    let js = crate::JSValue::from_bits(value.to_bits());
    if !js.is_pointer() {
        return None;
    }
    unsafe { backed(value.to_bits() as usize) }
}

pub(crate) fn key_of_str(name: &str) -> Option<ElementsKey> {
    if name == "length" {
        return Some(ElementsKey::Length);
    }
    crate::object::canonical_array_index(name).map(ElementsKey::Index)
}

/// # Safety
/// `key` must be null or a live string header.
pub(crate) unsafe fn key_of_header(key: *const crate::StringHeader) -> Option<ElementsKey> {
    if key.is_null() {
        return None;
    }
    // Cheap pre-filter: only a leading ASCII digit (an index) or `l`
    // (`length`) can be an elements key, and no canonical index is longer
    // than ten digits. Every ordinary named property — the common case on
    // these receivers — is rejected on one byte, before the UTF-8 decode and
    // the canonical-index parse below.
    let byte_len = (*key).byte_len as usize;
    if byte_len == 0 || byte_len > 10 {
        return None;
    }
    let first = *(key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    if !first.is_ascii_digit() && first != b'l' {
        return None;
    }
    crate::object::has_own_helpers::str_from_string_header(key).and_then(key_of_str)
}

/// A NaN-boxed property key: a canonical-index number, or a string naming an
/// index or `length`. Symbols and everything else are never elements keys.
pub(crate) fn key_of_value(key: f64) -> Option<ElementsKey> {
    let js = crate::JSValue::from_bits(key.to_bits());
    if js.is_number() {
        let n = js.as_number();
        if n.is_finite() && n >= 0.0 && n < 4_294_967_295.0 && n.fract() == 0.0 {
            return Some(ElementsKey::Index(n as u32));
        }
        return None;
    }
    if js.is_any_string() {
        let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        return unsafe { crate::string::js_string_key_bytes(js, &mut sso) }
            .and_then(|b| std::str::from_utf8(b).ok())
            .and_then(key_of_str);
    }
    None
}

#[inline]
unsafe fn slot_bits(elements: *const ArrayHeader, index: u32) -> u64 {
    *(elements as *const u8)
        .add(std::mem::size_of::<ArrayHeader>())
        .cast::<u64>()
        .add(index as usize)
}

/// Own value for `key`: `length`, or an in-bounds non-hole element. `None`
/// means "not an own property" — the caller continues with its ordinary
/// lookup (the shape carries no such key, so that reaches the prototype
/// chain exactly as a hole on a plain Array does).
///
/// # Safety
/// `elements` must be the live store of a validated instance.
pub(crate) unsafe fn get_by_key(elements: *const ArrayHeader, key: ElementsKey) -> Option<f64> {
    match key {
        ElementsKey::Length => Some(f64::from((*elements).length)),
        ElementsKey::Index(index) => {
            if index >= (*elements).length {
                return None;
            }
            let bits = slot_bits(elements, index);
            (bits != crate::value::TAG_HOLE).then_some(f64::from_bits(bits))
        }
    }
}

/// `[[Set]]` of an elements key: an in-bounds store, an append, a hole-creating
/// extension, or a `length` write (Array semantics: truncate or extend with
/// holes; a non-index `length` is a RangeError). The owner is rooted across
/// the re-allocating cases and the new head is written back.
///
/// # Safety
/// `obj` must be a validated elements-backed instance and `elements` its store.
pub(crate) unsafe fn set_by_key(
    obj: *mut ObjectHeader,
    elements: *mut ArrayHeader,
    key: ElementsKey,
    value: f64,
) {
    match key {
        ElementsKey::Length => set_length(obj, elements, value),
        ElementsKey::Index(index) => {
            let length = (*elements).length;
            if index < length {
                crate::array::js_array_set_f64(elements, index, value);
                return;
            }
            let scope = crate::gc::RuntimeHandleScope::new();
            let obj_handle = scope.root_raw_mut_ptr(obj);
            let value_root = scope.root_nanbox_f64(value);
            let (head, obj) = obj_handle.across_mut::<ObjectHeader, _>(|| {
                if index == length {
                    crate::array::js_array_push_f64(elements, value_root.get_nanbox_f64())
                } else {
                    crate::array::js_array_set_f64_extend(
                        elements,
                        index,
                        value_root.get_nanbox_f64(),
                    )
                }
            });
            if !head.is_null() && head != elements_of(obj) {
                set_elements_head(obj, head);
            }
        }
    }
}

/// `length = n` on an elements-backed instance.
///
/// # Safety
/// As [`set_by_key`].
pub(crate) unsafe fn set_length(
    obj: *mut ObjectHeader,
    elements: *mut ArrayHeader,
    new_length: f64,
) {
    if !new_length.is_finite()
        || new_length < 0.0
        || new_length.fract() != 0.0
        || new_length >= 4_294_967_296.0
    {
        crate::array::array_length_range_error();
    }
    let target = new_length as u32;
    let current = (*elements).length;
    if target <= current {
        // Truncation never allocates.
        crate::array::js_array_set_length_strict(elements, new_length);
        return;
    }
    // Extension: grow through the index store (which returns the head), then
    // punch the written slot back out into a hole.
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let (head, obj) = obj_handle.across_mut::<ObjectHeader, _>(|| {
        crate::array::js_array_set_f64_extend(
            elements,
            target - 1,
            f64::from_bits(crate::value::TAG_UNDEFINED),
        )
    });
    let head = if head.is_null() { elements } else { head };
    if head != elements_of(obj) {
        set_elements_head(obj, head);
    }
    crate::array::js_array_delete(head, target - 1);
}

/// # Safety
/// `elements` must be a live store.
pub(crate) unsafe fn has_own_key(elements: *const ArrayHeader, key: ElementsKey) -> bool {
    match key {
        ElementsKey::Length => true,
        ElementsKey::Index(index) => {
            index < (*elements).length && slot_bits(elements, index) != crate::value::TAG_HOLE
        }
    }
}

/// `delete obj[key]`: an index becomes a hole (1); `length` is
/// non-configurable (0).
///
/// # Safety
/// `elements` must be a live store.
pub(crate) unsafe fn delete_key(elements: *mut ArrayHeader, key: ElementsKey) -> i32 {
    match key {
        ElementsKey::Length => 0,
        ElementsKey::Index(index) => {
            if index < (*elements).length {
                crate::array::js_array_delete(elements, index)
            } else {
                1
            }
        }
    }
}

/// The present (non-hole) indices, ascending.
///
/// # Safety
/// `elements` must be a live store.
pub(crate) unsafe fn own_index_keys(elements: *const ArrayHeader) -> Vec<u32> {
    let length = (*elements).length;
    let mut out = Vec::new();
    for index in 0..length {
        if slot_bits(elements, index) != crate::value::TAG_HOLE {
            out.push(index);
        }
    }
    out
}

/// A fresh keys array: the present indices as strings (ascending), then
/// `"length"` when `with_length` (getOwnPropertyNames), then every key of
/// `shape_keys` in order. `shape_keys` and the result are rooted across the
/// string allocations.
///
/// # Safety
/// `elements` must be a live store; `shape_keys` a live array (or null).
pub(crate) unsafe fn prepend_index_keys(
    elements: *const ArrayHeader,
    shape_keys: *mut ArrayHeader,
    with_length: bool,
) -> *mut ArrayHeader {
    let indices = own_index_keys(elements);
    let scope = crate::gc::RuntimeHandleScope::new();
    let shape_h = scope.root_raw_mut_ptr(shape_keys);
    let out_h = scope.root_raw_mut_ptr(crate::array::js_array_alloc(0));
    let push_key = |bytes: &[u8]| {
        let (s, _) = out_h.across_mut::<ArrayHeader, _>(|| {
            crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
        });
        let (grown, _) = out_h.across_mut::<ArrayHeader, _>(|| {
            let out: *mut ArrayHeader = out_h.with_mut_ptr(|p| p);
            crate::array::js_array_push_f64(out, crate::value::js_nanbox_string(s as i64))
        });
        out_h.set_raw_mut_ptr(grown);
    };
    for index in indices {
        push_key(index.to_string().as_bytes());
    }
    if with_length {
        push_key(b"length");
    }
    let shape_len = shape_h.with_mut_ptr(|p: *mut ArrayHeader| {
        if p.is_null() {
            0
        } else {
            crate::array::js_array_length(p)
        }
    });
    for i in 0..shape_len {
        let key = shape_h.with_mut_ptr(|p: *mut ArrayHeader| crate::array::js_array_get(p, i));
        let (grown, _) = out_h.across_mut::<ArrayHeader, _>(|| {
            let out: *mut ArrayHeader = out_h.with_mut_ptr(|p| p);
            crate::array::js_array_push_f64(out, f64::from_bits(key.bits()))
        });
        out_h.set_raw_mut_ptr(grown);
    }
    out_h.with_mut_ptr(|p| p)
}

/// The own property descriptor of an elements key, or `None` when the key is
/// not an own property (a hole, an index past `length`).
///
/// # Safety
/// `obj` must be a validated instance and `elements` its live store.
pub(crate) unsafe fn own_property_descriptor(
    obj: *const ObjectHeader,
    elements: *const ArrayHeader,
    key: ElementsKey,
) -> Option<f64> {
    let value = get_by_key(elements, key)?;
    let frozen = crate::value::addr_class::try_read_gc_header(obj as usize)
        .is_some_and(|h| h._reserved & crate::gc::OBJ_FLAG_FROZEN != 0);
    Some(match key {
        ElementsKey::Index(_) => {
            crate::object::descriptors::build_data_descriptor(value, !frozen, true, !frozen)
        }
        ElementsKey::Length => {
            crate::object::descriptors::build_data_descriptor(value, !frozen, false, false)
        }
    })
}

/// Leave the elements representation for good: every present element becomes
/// a shape-carried index property (ascending), then `length`, and the store is
/// detached. Everything after this runs on the shape-carried machinery
/// (`super::subclass`, `object/array_tail_transition.rs`) exactly as before.
/// Called by the exotic operations that machinery already models —
/// `defineProperty` on an index or `length`, accessor installs, freeze/seal/
/// preventExtensions, `setPrototypeOf` — before they do their work.
///
/// # Safety
/// `obj` must be a validated elements-backed instance.
pub(crate) unsafe fn deopt_to_shape(obj: *mut ObjectHeader) {
    let elements = elements_of(obj);
    if elements.is_null() {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_raw_mut_ptr(obj);
    let elements_h = scope.root_raw_mut_ptr(elements);
    // Detach first: the index stores below must not be routed back here.
    set_elements_head(obj, std::ptr::null_mut());
    let length = (*elements).length;
    for index in own_index_keys(elements) {
        let value =
            elements_h.with_mut_ptr(|e: *mut ArrayHeader| f64::from_bits(slot_bits(e, index)));
        let name = index.to_string();
        let (key, obj) = obj_h.across_mut::<ObjectHeader, _>(|| {
            crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
        });
        crate::object::js_object_set_field_by_name(obj, key, value);
    }
    let (key, obj) = obj_h.across_mut::<ObjectHeader, _>(|| {
        crate::string::js_string_from_bytes(b"length".as_ptr(), 6)
    });
    crate::object::set_field_by_name_object_tail(obj, key, f64::from(length));
}

/// [`deopt_to_shape`] for a NaN-boxed receiver that may not be elements-backed.
pub(crate) fn deopt_value(value: f64) {
    if let Some((obj, _)) = backed_value(value) {
        unsafe { deopt_to_shape(obj) };
    }
}

/// A fresh values array: present elements (ascending), then every value of
/// `shape_values` in order (`Object.values`).
///
/// # Safety
/// `elements` must be a live store; `shape_values` a live array (or null).
pub(crate) unsafe fn prepend_index_values(
    elements: *const ArrayHeader,
    shape_values: *mut ArrayHeader,
) -> *mut ArrayHeader {
    let indices = own_index_keys(elements);
    let scope = crate::gc::RuntimeHandleScope::new();
    let elements_h = scope.root_raw_const_ptr(elements);
    let shape_h = scope.root_raw_mut_ptr(shape_values);
    let out_h = scope.root_raw_mut_ptr(crate::array::js_array_alloc(0));
    let push = |value: f64| {
        let (grown, _) = out_h.across_mut::<ArrayHeader, _>(|| {
            let out: *mut ArrayHeader = out_h.with_mut_ptr(|p| p);
            crate::array::js_array_push_f64(out, value)
        });
        out_h.set_raw_mut_ptr(grown);
    };
    for index in indices {
        let value =
            elements_h.with_const_ptr(|e: *const ArrayHeader| f64::from_bits(slot_bits(e, index)));
        push(value);
    }
    let shape_len = shape_h.with_mut_ptr(|p: *mut ArrayHeader| {
        if p.is_null() {
            0
        } else {
            crate::array::js_array_length(p)
        }
    });
    for i in 0..shape_len {
        let value = shape_h.with_mut_ptr(|p: *mut ArrayHeader| crate::array::js_array_get(p, i));
        push(f64::from_bits(value.bits()));
    }
    out_h.with_mut_ptr(|p| p)
}

/// A fresh entries array: `[String(index), element]` pairs for the present
/// elements (ascending), then every pair of `shape_entries` in order
/// (`Object.entries`).
///
/// # Safety
/// `elements` must be a live store; `shape_entries` a live array (or null).
pub(crate) unsafe fn prepend_index_entries(
    elements: *const ArrayHeader,
    shape_entries: *mut ArrayHeader,
) -> *mut ArrayHeader {
    let indices = own_index_keys(elements);
    let scope = crate::gc::RuntimeHandleScope::new();
    let elements_h = scope.root_raw_const_ptr(elements);
    let shape_h = scope.root_raw_mut_ptr(shape_entries);
    let out_h = scope.root_raw_mut_ptr(crate::array::js_array_alloc(0));
    let push = |value: f64| {
        let (grown, _) = out_h.across_mut::<ArrayHeader, _>(|| {
            let out: *mut ArrayHeader = out_h.with_mut_ptr(|p| p);
            crate::array::js_array_push_f64(out, value)
        });
        out_h.set_raw_mut_ptr(grown);
    };
    for index in indices {
        let name = index.to_string();
        let (key, _) = out_h.across_mut::<ArrayHeader, _>(|| {
            crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
        });
        let key_h = scope.root_nanbox_f64(crate::value::js_nanbox_string(key as i64));
        let (pair, _) = out_h.across_mut::<ArrayHeader, _>(|| crate::array::js_array_alloc(2));
        let pair_h = scope.root_raw_mut_ptr(pair);
        let (pair, _) = pair_h.across_mut::<ArrayHeader, _>(|| {
            let pair: *mut ArrayHeader = pair_h.with_mut_ptr(|p| p);
            crate::array::js_array_push_f64(pair, key_h.get_nanbox_f64())
        });
        pair_h.set_raw_mut_ptr(pair);
        let value =
            elements_h.with_const_ptr(|e: *const ArrayHeader| f64::from_bits(slot_bits(e, index)));
        let (pair, _) = pair_h.across_mut::<ArrayHeader, _>(|| {
            let pair: *mut ArrayHeader = pair_h.with_mut_ptr(|p| p);
            crate::array::js_array_push_f64(pair, value)
        });
        pair_h.set_raw_mut_ptr(pair);
        let pair_value =
            pair_h.with_mut_ptr(|p: *mut ArrayHeader| crate::value::js_nanbox_pointer(p as i64));
        push(pair_value);
    }
    let shape_len = shape_h.with_mut_ptr(|p: *mut ArrayHeader| {
        if p.is_null() {
            0
        } else {
            crate::array::js_array_length(p)
        }
    });
    for i in 0..shape_len {
        let value = shape_h.with_mut_ptr(|p: *mut ArrayHeader| crate::array::js_array_get(p, i));
        push(f64::from_bits(value.bits()));
    }
    out_h.with_mut_ptr(|p| p)
}

// ---------------------------------------------------------------------------
// Hot-entry helpers used by `super::subclass` (moved here to keep that file
// under the size gate).
// ---------------------------------------------------------------------------

/// The elements store of an elements-backed instance (`super::subclass_elements`),
/// or `None` for the shape-carried representation. Non-null only when the
/// store was installed at construction, so no gate check is needed here.
#[inline]
pub(super) fn elements_for_validated(
    receiver: &ValidatedObjectReceiver,
) -> Option<*mut ArrayHeader> {
    let elements = unsafe { elements_of(receiver.object) };
    (!elements.is_null()).then_some(elements)
}

/// In-bounds, non-hole element of the inner array; `None` sends the caller
/// to the same prototype-chain fallback a hole in the shape-carried form does.
#[inline]
pub(super) fn elements_index_get(elements: *const ArrayHeader, index: u32) -> Option<f64> {
    unsafe {
        if index >= (*elements).length {
            return None;
        }
        let slot = (elements as *const u8)
            .add(std::mem::size_of::<ArrayHeader>())
            .cast::<u64>()
            .add(index as usize);
        let bits = *slot;
        (bits != crate::value::TAG_HOLE).then_some(f64::from_bits(bits))
    }
}

/// Elements-backed `receiver[index] = value` for an in-bounds index or the
/// appending index (`== length`); anything else (holes past the end, a
/// frozen/sealed/non-extensible receiver) declines to the generic path.
pub(super) fn elements_index_set(
    receiver: &ValidatedObjectReceiver,
    index: u32,
    value: f64,
) -> Option<bool> {
    let elements = elements_for_validated(receiver)?;
    if !mutation_receiver_allows_plain_tail(receiver.object_flags) {
        return Some(false);
    }
    let length = unsafe { (*elements).length };
    if index < length {
        crate::array::js_array_set_f64(elements, index, value);
        return Some(true);
    }
    if index == length {
        return elements_push(receiver, value).map(|_| true);
    }
    Some(false)
}

/// Elements-backed append: the owner is rooted across the (possibly
/// re-allocating) push and the new head is written back through the
/// barriered meta slot. Returns the new length.
pub(super) fn elements_push(receiver: &ValidatedObjectReceiver, value: f64) -> Option<f64> {
    let elements = elements_for_validated(receiver)?;
    if !mutation_receiver_allows_plain_tail(receiver.object_flags) {
        return None;
    }
    let obj = receiver.object as *mut ObjectHeader;
    unsafe {
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj_handle = scope.root_raw_mut_ptr(obj);
        let value_root = scope.root_nanbox_f64(value);
        let (grown, obj) = obj_handle.across_mut::<ObjectHeader, _>(|| {
            crate::array::js_array_push_f64(elements, value_root.get_nanbox_f64())
        });
        let current = elements_of(obj);
        if grown != current {
            set_elements_head(obj, grown);
        }
        Some(f64::from((*grown).length))
    }
}

/// Elements-backed `pop`: the inner array's own pop (no allocation, so no
/// rooting), declining like `elements_push` on a non-plain receiver.
pub(super) fn elements_pop(receiver: &ValidatedObjectReceiver) -> Option<f64> {
    let elements = elements_for_validated(receiver)?;
    if !mutation_receiver_allows_plain_tail(receiver.object_flags) {
        return None;
    }
    Some(crate::array::js_array_pop_f64(elements))
}
