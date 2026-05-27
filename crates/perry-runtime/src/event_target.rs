//! Minimal WHATWG `EventTarget` storage used by Node's `events` helpers.
//!
//! Perry models `EventTarget` as a regular runtime object with hidden fields:
//! a marker, a listener-bag object keyed by event type, and a max-listener
//! number. Keeping the listener arrays in object fields lets the normal GC
//! trace callbacks without a separate native handle registry.

use crate::{
    js_array_alloc, js_array_get, js_array_length, js_array_push_f64, js_nanbox_pointer,
    js_object_alloc, js_object_get_field_by_name_f64, js_object_set_field_by_name,
    js_string_from_bytes, ArrayHeader, JSValue, ObjectHeader, StringHeader,
};

fn key(bytes: &[u8]) -> *mut StringHeader {
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

fn boxed_ptr<T>(ptr: *mut T) -> f64 {
    js_nanbox_pointer(ptr as i64)
}

fn value_as_ptr<T>(value: f64) -> Option<*mut T> {
    let value = JSValue::from_bits(value.to_bits());
    if value.is_pointer() {
        Some(value.as_pointer::<T>() as *mut T)
    } else {
        None
    }
}

unsafe fn is_event_target(target: *const ObjectHeader) -> bool {
    if target.is_null() {
        return false;
    }
    if (target as usize) < crate::gc::GC_HEADER_SIZE + 0x10000 {
        return false;
    }
    let gc_header =
        (target as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT {
        return false;
    }
    let marker = js_object_get_field_by_name_f64(target, key(b"_eventTarget"));
    marker.to_bits() == JSValue::bool(true).bits()
}

unsafe fn listeners_bag(target: *mut ObjectHeader) -> Option<*mut ObjectHeader> {
    if !is_event_target(target) {
        return None;
    }
    let bag = js_object_get_field_by_name_f64(target, key(b"_eventTargetListeners"));
    value_as_ptr::<ObjectHeader>(bag)
}

unsafe fn event_array(
    bag: *mut ObjectHeader,
    event_name_ptr: *const StringHeader,
    create: bool,
) -> Option<*mut ArrayHeader> {
    if bag.is_null() || event_name_ptr.is_null() {
        return None;
    }
    let existing = js_object_get_field_by_name_f64(bag, event_name_ptr);
    if let Some(arr) = value_as_ptr::<ArrayHeader>(existing) {
        return Some(arr);
    }
    if !create {
        return None;
    }
    let arr = js_array_alloc(0);
    js_object_set_field_by_name(bag, event_name_ptr, boxed_ptr(arr));
    Some(arr)
}

/// `new EventTarget()`.
#[no_mangle]
pub extern "C" fn js_event_target_new() -> *mut ObjectHeader {
    let target = js_object_alloc(0, 0);
    let bag = js_object_alloc(0, 0);
    js_object_set_field_by_name(
        target,
        key(b"_eventTarget"),
        f64::from_bits(JSValue::bool(true).bits()),
    );
    js_object_set_field_by_name(target, key(b"_eventTargetListeners"), boxed_ptr(bag));
    js_object_set_field_by_name(target, key(b"_eventTargetMaxListeners"), 10.0);
    target
}

/// `target.addEventListener(type, listener)`.
#[no_mangle]
pub unsafe extern "C" fn js_event_target_add_event_listener(
    target: *mut ObjectHeader,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
) {
    if callback_ptr == 0 {
        return;
    }
    let Some(bag) = listeners_bag(target) else {
        return;
    };
    let Some(arr) = event_array(bag, event_name_ptr, true) else {
        return;
    };
    let listener = boxed_ptr(callback_ptr as *mut u8);
    let len = js_array_length(arr);
    for i in 0..len {
        if js_array_get(arr, i).bits() == listener.to_bits() {
            return;
        }
    }
    let updated = js_array_push_f64(arr, listener);
    if updated != arr {
        js_object_set_field_by_name(bag, event_name_ptr, boxed_ptr(updated));
    }
}

/// `target.removeEventListener(type, listener)`.
#[no_mangle]
pub unsafe extern "C" fn js_event_target_remove_event_listener(
    target: *mut ObjectHeader,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
) {
    if callback_ptr == 0 {
        return;
    }
    let Some(bag) = listeners_bag(target) else {
        return;
    };
    let Some(arr) = event_array(bag, event_name_ptr, false) else {
        return;
    };
    let listener = boxed_ptr(callback_ptr as *mut u8);
    let out = js_array_alloc(0);
    let len = js_array_length(arr);
    let mut changed = false;
    let mut result = out;
    for i in 0..len {
        let current = js_array_get(arr, i);
        if current.bits() == listener.to_bits() {
            changed = true;
            continue;
        }
        result = js_array_push_f64(result, f64::from_bits(current.bits()));
    }
    if changed {
        js_object_set_field_by_name(bag, event_name_ptr, boxed_ptr(result));
    }
}

/// Runtime predicate used by the Node `events` module helpers.
#[no_mangle]
pub unsafe extern "C" fn js_event_target_is_event_target(target: *const ObjectHeader) -> i32 {
    if is_event_target(target) {
        1
    } else {
        0
    }
}

/// `events.getEventListeners(target, type)` for EventTarget receivers.
#[no_mangle]
pub unsafe extern "C" fn js_event_target_get_event_listeners(
    target: *mut ObjectHeader,
    event_name_ptr: *const StringHeader,
) -> *mut ArrayHeader {
    let out = js_array_alloc(0);
    let Some(bag) = listeners_bag(target) else {
        return out;
    };
    let Some(arr) = event_array(bag, event_name_ptr, false) else {
        return out;
    };
    let len = js_array_length(arr);
    let mut result = out;
    for i in 0..len {
        let current = js_array_get(arr, i);
        result = js_array_push_f64(result, f64::from_bits(current.bits()));
    }
    result
}

/// `events.getMaxListeners(target)` for EventTarget receivers.
#[no_mangle]
pub unsafe extern "C" fn js_event_target_get_max_listeners(target: *mut ObjectHeader) -> f64 {
    if !is_event_target(target) {
        return 10.0;
    }
    let value = js_object_get_field_by_name_f64(target, key(b"_eventTargetMaxListeners"));
    if JSValue::from_bits(value.to_bits()).is_number() {
        value
    } else {
        10.0
    }
}

/// `events.setMaxListeners(n, target)` for EventTarget receivers.
#[no_mangle]
pub unsafe extern "C" fn js_event_target_set_max_listeners(
    target: *mut ObjectHeader,
    n: f64,
) -> i32 {
    if !is_event_target(target) {
        return 0;
    }
    js_object_set_field_by_name(target, key(b"_eventTargetMaxListeners"), n);
    1
}
