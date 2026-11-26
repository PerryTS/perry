//! Web Streams as ordinary GC objects (design 2026-11-25 §2, P2a): the
//! object layer the access layer (`state.rs`) sits on for the readable
//! family.
//!
//! Every member is an ordinary object of its family class id
//! (`native_class_ids::WEB_*`) whose prototype is the realm's intrinsic
//! prototype of its interface. The ReadableStream carries its scalars in a
//! native payload (`RS_FAMILY`); every member keeps its JS values in one
//! dense hidden array born at its final length (`js_perry_slots_*`). The slot
//! indices below are the layout of each family: no lookup, no name list.
//! Lifetime is reachability: nothing here registers, roots or scans.
//!
//! Moving GC discipline: a Rust local holding an object value is found by the
//! conservative stack scan, which pins it, but the slot array is reachable
//! only through its object and may move at any allocation. Every helper here
//! therefore re-reads the array from the object after it allocates, and no
//! caller keeps an array pointer across an allocation or a JS call.

use super::*;
use perry_runtime::native_class_ids as ids;
use perry_runtime::native_payload::{self, NativePayloadFamily, PayloadPrototype};

#[allow(improper_ctypes)]
extern "C" {
    #[link_name = "js_perry_intrinsic_prototype"]
    fn provider_intrinsic_prototype(name_ptr: *const u8, name_len: usize) -> f64;
    #[link_name = "js_perry_slots_object_alloc"]
    fn provider_slots_object_alloc(class_id: u32, proto: f64, slots: u32) -> f64;
    #[link_name = "js_perry_slots_init"]
    fn provider_slots_init(value: f64, slots: u32) -> i32;
    #[link_name = "js_perry_slots"]
    fn provider_slots(value: f64) -> *mut ArrayHeader;
    #[link_name = "js_perry_object_class_id"]
    fn provider_object_class_id(value: f64) -> u32;
    #[link_name = "js_array_set"]
    fn provider_js_array_set(array: *mut ArrayHeader, index: u32, value: JSValue);
    #[link_name = "js_array_shift_f64"]
    fn provider_js_array_shift_f64(array: *mut ArrayHeader) -> f64;
    #[link_name = "js_array_unshift_f64"]
    fn provider_js_array_unshift_f64(array: *mut ArrayHeader, value: f64) -> *mut ArrayHeader;
}

/// ReadableStream slots.
pub(super) mod rs {
    pub(crate) const CONTROLLER: u32 = 0;
    pub(crate) const READER: u32 = 1;
    pub(crate) const STORED_ERROR: u32 = 2;
    pub(crate) const PENDING_ERROR_AFTER_CHUNKS: u32 = 3;
    pub(crate) const START: u32 = 4;
    pub(crate) const PULL: u32 = 5;
    pub(crate) const CANCEL: u32 = 6;
    pub(crate) const SIZE: u32 = 7;
    /// JS array of queued chunks.
    pub(crate) const QUEUE: u32 = 8;
    /// JS array of pending read() promises.
    pub(crate) const READ_REQUESTS: u32 = 9;
    pub(crate) const TEE_BRANCH1: u32 = 10;
    pub(crate) const TEE_BRANCH2: u32 = 11;
    pub(crate) const TEE_SOURCE: u32 = 12;
    pub(crate) const TEE_CANCEL_PROMISE: u32 = 13;
    /// Readable side of a TransformStream: JS array of parked write promises.
    pub(crate) const PARKED_WRITES: u32 = 14;
    /// Readable side of a TransformStream: JS array of deferred write jobs.
    pub(crate) const BACKPRESSURED_JOBS: u32 = 15;
    /// JS array of outstanding BYOB read(view) entries.
    pub(crate) const BYOB_PENDING: u32 = 16;
    /// The current ReadableStreamBYOBRequest object.
    pub(crate) const BYOB_REQUEST: u32 = 17;
    /// The native producer's pending read promise.
    pub(crate) const NATIVE_PROMISE: u32 = 18;
    pub(crate) const BODY_CONSUMER_PROMISE: u32 = 19;
    pub(crate) const LEN: u32 = 20;
}

/// Default / byte controller slots.
pub(super) mod ctrl {
    pub(crate) const STREAM: u32 = 0;
    pub(crate) const LEN: u32 = 1;
}

/// ReadableStreamBYOBRequest slots.
pub(super) mod req {
    pub(crate) const CONTROLLER: u32 = 0;
    pub(crate) const LEN: u32 = 1;
}

/// Default / BYOB reader slots.
pub(super) mod reader {
    pub(crate) const STREAM: u32 = 0;
    pub(crate) const CLOSED_PROMISE: u32 = 1;
    /// `true` while the reader holds its stream's lock.
    pub(crate) const LOCKED: u32 = 2;
    pub(crate) const LEN: u32 = 3;
}

fn install_nothing(_: &mut PayloadPrototype) {}

/// The ReadableStream payload family. Its instances are born on the realm's
/// `ReadableStream.prototype` (or attach to a subclass `this`), never on a
/// family prototype of their own. `links_owner`: the fetch engine's sink
/// holds the stream's `OwnerLink`.
pub(super) static RS_FAMILY: NativePayloadFamily = NativePayloadFamily {
    class_id: ids::WEB_READABLE_STREAM,
    links_owner: true,
    name: "ReadableStream",
    constructor_export: None,
    constructor_length: 0,
    install_prototype: install_nothing,
};

#[inline]
fn undefined_f64() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

#[inline]
fn is_undefined(value: f64) -> bool {
    value.to_bits() == TAG_UNDEFINED
}

/// The realm's intrinsic prototype of interface `name` (born on first use).
pub(super) fn intrinsic_prototype(name: &str) -> f64 {
    unsafe { provider_intrinsic_prototype(name.as_ptr(), name.len()) }
}

/// A payload-less member: an ordinary object of `class_id` on the intrinsic
/// prototype of `interface`, with a `slots`-long slot array.
pub(super) fn alloc_slots_object(class_id: u32, interface: &str, slots: u32) -> f64 {
    let proto = intrinsic_prototype(interface);
    unsafe { provider_slots_object_alloc(class_id, proto, slots) }
}

/// A ReadableStream owning `payload`, on `proto` (the intrinsic
/// `ReadableStream.prototype` unless a caller passes another), with its slot
/// array.
pub(super) fn alloc_rs_object<T: 'static>(payload: T, external_bytes: usize) -> f64 {
    let proto = intrinsic_prototype("ReadableStream");
    let proto_ptr = perry_runtime::value::JSValue::from_bits(proto.to_bits());
    if !proto_ptr.is_pointer() {
        return undefined_f64();
    }
    let value = native_payload::alloc_with_prototype(
        &RS_FAMILY,
        payload,
        external_bytes,
        &[],
        proto_ptr.as_pointer::<ObjectHeader>() as *mut ObjectHeader,
    );
    if !is_undefined(value) {
        let rc = unsafe { provider_slots_init(value, rs::LEN) };
        debug_assert_eq!(rc, 0, "a fresh ReadableStream has no JS state");
    }
    value
}

/// Attach a ReadableStream payload and its slot array to an existing object
/// (a subclass instance's `this`). False when `this` is no object or already
/// carries a payload or state.
pub(super) fn attach_rs<T: 'static>(this: f64, payload: T, external_bytes: usize) -> bool {
    if unsafe { provider_slots(this) }.is_null()
        && unsafe { provider_slots_init(this, rs::LEN) } != 0
    {
        return false;
    }
    native_payload::attach_to_object(this, &RS_FAMILY, payload, external_bytes)
}

/// The ReadableStream payload of `value` (an instance or a subclass
/// instance); None for any other value. The reference must not live across
/// an allocation or a JS call.
///
/// # Safety
/// `T` is the family's payload type.
pub(super) unsafe fn rs_payload<'a, T: 'static>(value: f64) -> Option<&'a mut T> {
    native_payload::payload_mut_attached::<T>(value, &RS_FAMILY).ok()
}

/// The class id of an ordinary object value (0 for any other value).
#[inline]
pub(super) fn class_id_of(value: f64) -> u32 {
    unsafe { provider_object_class_id(value) }
}

/// Is `value` a payload-less member of `class_id`?
#[cfg(test)]
#[inline]
pub(super) fn is_member(value: f64, class_id: u32) -> bool {
    class_id_of(value) == class_id
}

// ─────────────────────────────────────────────────────────────────────
// Slots
// ─────────────────────────────────────────────────────────────────────

/// Slot `i` of `obj` (`undefined` when `obj` has no slot array).
#[inline]
pub(super) fn slot(obj: f64, i: u32) -> f64 {
    let array = unsafe { provider_slots(obj) };
    if array.is_null() {
        return undefined_f64();
    }
    f64::from_bits(js_array_get(array, i).bits())
}

/// Store `value` into slot `i` of `obj` (barriered).
#[inline]
pub(super) fn set_slot(obj: f64, i: u32, value: f64) {
    let array = unsafe { provider_slots(obj) };
    if !array.is_null() {
        unsafe { provider_js_array_set(array, i, JSValue::from_bits(value.to_bits())) };
    }
}

/// The JS array in slot `i` of `obj`, or null.
#[inline]
fn queue_array(obj: f64, i: u32) -> *mut ArrayHeader {
    let value = JSValue::from_bits(slot(obj, i).to_bits());
    if value.is_pointer() {
        value.as_pointer::<ArrayHeader>() as *mut ArrayHeader
    } else {
        std::ptr::null_mut()
    }
}

/// Append `value` to the queue (a JS array, born on first push) in slot `i`.
pub(super) fn queue_push(obj: f64, i: u32, value: f64) {
    let mut array = queue_array(obj, i);
    if array.is_null() {
        // Allocation: the slot array may move; `set_slot` re-reads it.
        array = js_array_alloc(4);
        set_slot(
            obj,
            i,
            perry_runtime::value::js_nanbox_pointer(array as i64),
        );
    }
    let grown = js_array_push_f64(array, value);
    if grown != array {
        set_slot(
            obj,
            i,
            perry_runtime::value::js_nanbox_pointer(grown as i64),
        );
    }
}

/// Put `value` at the head of the queue in slot `i`.
pub(super) fn queue_unshift(obj: f64, i: u32, value: f64) {
    let array = queue_array(obj, i);
    if array.is_null() {
        queue_push(obj, i, value);
        return;
    }
    let grown = unsafe { provider_js_array_unshift_f64(array, value) };
    if grown != array {
        set_slot(
            obj,
            i,
            perry_runtime::value::js_nanbox_pointer(grown as i64),
        );
    }
}

/// Remove and return the queue head in slot `i`.
pub(super) fn queue_shift(obj: f64, i: u32) -> Option<f64> {
    let array = queue_array(obj, i);
    if array.is_null() || js_array_length(array) == 0 {
        return None;
    }
    Some(unsafe { provider_js_array_shift_f64(array) })
}

/// The number of entries of the queue in slot `i`.
pub(super) fn queue_len(obj: f64, i: u32) -> u32 {
    let array = queue_array(obj, i);
    if array.is_null() {
        0
    } else {
        js_array_length(array)
    }
}

/// Entry `k` of the queue in slot `i` (`undefined` past its end).
pub(super) fn queue_get(obj: f64, i: u32, k: u32) -> f64 {
    let array = queue_array(obj, i);
    if array.is_null() || k >= js_array_length(array) {
        return undefined_f64();
    }
    f64::from_bits(js_array_get(array, k).bits())
}

/// Detach the whole queue in slot `i`: the slot becomes empty and the
/// caller gets the JS array (`undefined` when there was none) to walk with
/// [`array_len`] / [`array_get`]. Hold the result in a local, never in Rust
/// heap memory.
pub(super) fn queue_take(obj: f64, i: u32) -> f64 {
    let taken = slot(obj, i);
    set_slot(obj, i, undefined_f64());
    taken
}

/// Length of a JS array value (0 for any other value).
pub(super) fn array_len(array: f64) -> u32 {
    let value = JSValue::from_bits(array.to_bits());
    if value.is_pointer() {
        js_array_length(value.as_pointer::<ArrayHeader>())
    } else {
        0
    }
}

/// Element `k` of a JS array value.
pub(super) fn array_get(array: f64, k: u32) -> f64 {
    let value = JSValue::from_bits(array.to_bits());
    if value.is_pointer() {
        f64::from_bits(js_array_get(value.as_pointer::<ArrayHeader>(), k).bits())
    } else {
        undefined_f64()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Probe(u32);

    #[test]
    fn readable_object_has_its_family_prototype_payload_and_slots() {
        let _serial = super::super::tests::serial_guard();
        crate::test_runtime::initialize();
        let value = alloc_rs_object(Probe(7), 0);
        assert!(!is_undefined(value));
        assert_eq!(class_id_of(value), ids::WEB_READABLE_STREAM);
        assert_eq!(unsafe { rs_payload::<Probe>(value) }.map(|p| p.0), Some(7));
        assert_eq!(unsafe { rs_payload::<Probe>(1.0) }.map(|p| p.0), None);
        for i in 0..rs::LEN {
            assert!(is_undefined(slot(value, i)));
        }
        set_slot(value, rs::STORED_ERROR, 3.0);
        assert_eq!(slot(value, rs::STORED_ERROR), 3.0);
    }

    #[test]
    fn slot_queue_push_shift_take() {
        let _serial = super::super::tests::serial_guard();
        crate::test_runtime::initialize();
        let value = alloc_slots_object(
            ids::WEB_RS_DEFAULT_READER,
            "ReadableStreamDefaultReader",
            reader::LEN,
        );
        assert!(is_member(value, ids::WEB_RS_DEFAULT_READER));
        assert!(!is_member(value, ids::WEB_RS_BYOB_READER));
        assert_eq!(queue_len(value, reader::STREAM), 0);
        assert_eq!(queue_shift(value, reader::STREAM), None);
        for k in 0..10 {
            queue_push(value, reader::STREAM, k as f64);
        }
        assert_eq!(queue_len(value, reader::STREAM), 10);
        assert_eq!(queue_shift(value, reader::STREAM), Some(0.0));
        assert_eq!(queue_get(value, reader::STREAM, 0), 1.0);
        let taken = queue_take(value, reader::STREAM);
        assert_eq!(array_len(taken), 9);
        assert_eq!(array_get(taken, 8), 9.0);
        assert_eq!(queue_len(value, reader::STREAM), 0);
    }
}
