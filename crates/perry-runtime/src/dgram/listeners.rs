//! `node:dgram` listener storage, add/remove/emit, and `eventNames()`.
//!
//! Split out of `dgram.rs` (pure code move). See the trunk module for the data
//! model and shared helpers.

use super::*;

use crate::array::ArrayHeader;
use crate::object::{
    js_object_get_field_by_name_f64, js_object_keys, js_object_set_field_by_name, ObjectHeader,
};
use crate::value::TAG_UNDEFINED;

pub(crate) fn listener_event_key(prefix: &[u8], event: f64) -> Option<*mut crate::StringHeader> {
    let event = string_to_rust(event)?;
    let mut bytes = prefix.to_vec();
    bytes.extend_from_slice(event.as_bytes());
    Some(hidden_key(&bytes))
}

pub(crate) fn listener_storage(socket: f64, event: f64) -> Option<(f64, f64)> {
    let listener_key = listener_event_key(EVENT_LISTENERS_PREFIX, event)?;
    let once_key = listener_event_key(EVENT_ONCE_PREFIX, event)?;
    let listeners = {
        let obj = object_ptr_from_value(socket)?;
        let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, listener_key);
        if value.to_bits() == TAG_UNDEFINED {
            return None;
        }
        value
    };
    let once = {
        let obj = object_ptr_from_value(socket)?;
        let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, once_key);
        if value.to_bits() == TAG_UNDEFINED {
            return None;
        }
        value
    };
    Some((listeners, once))
}

pub(crate) fn ensure_listener_storage(socket: f64, event: f64) -> Option<(f64, f64)> {
    let listener_key = listener_event_key(EVENT_LISTENERS_PREFIX, event)?;
    let once_key = listener_event_key(EVENT_ONCE_PREFIX, event)?;
    let obj = object_ptr_from_value(socket)?;
    let listeners = {
        let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, listener_key);
        if value.to_bits() == TAG_UNDEFINED {
            let arr = crate::array::js_array_alloc(0);
            let arr_value = boxed_pointer(arr as *const u8);
            js_object_set_field_by_name(obj, listener_key, arr_value);
            arr_value
        } else {
            value
        }
    };
    let once = {
        let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, once_key);
        if value.to_bits() == TAG_UNDEFINED {
            let arr = crate::array::js_array_alloc(0);
            let arr_value = boxed_pointer(arr as *const u8);
            js_object_set_field_by_name(obj, once_key, arr_value);
            arr_value
        } else {
            value
        }
    };
    Some((listeners, once))
}

pub(crate) fn set_listener_storage(socket: f64, event: f64, listeners: f64, once: f64) {
    let Some(obj) = object_ptr_from_value(socket) else {
        return;
    };
    if let Some(listener_key) = listener_event_key(EVENT_LISTENERS_PREFIX, event) {
        js_object_set_field_by_name(obj, listener_key, listeners);
    }
    if let Some(once_key) = listener_event_key(EVENT_ONCE_PREFIX, event) {
        js_object_set_field_by_name(obj, once_key, once);
    }
}

pub(crate) fn add_listener(socket: f64, event: f64, listener: f64, once: bool) {
    if string_to_rust(event).is_none() {
        return;
    }
    if !is_callable_value(listener) {
        throw_invalid_listener(listener);
    }
    let Some((listeners, once_flags)) = ensure_listener_storage(socket, event) else {
        return;
    };
    let listeners_raw = raw_ptr_from_value(listeners) as *const ArrayHeader;
    let once_raw = raw_ptr_from_value(once_flags) as *const ArrayHeader;
    let len = crate::array::js_array_length(listeners_raw);
    let mut out_listeners = crate::array::js_array_alloc(len + 1);
    let mut out_once = crate::array::js_array_alloc(len + 1);
    for i in 0..len {
        out_listeners = crate::array::js_array_push_f64(
            out_listeners,
            crate::array::js_array_get_f64(listeners_raw, i),
        );
        out_once =
            crate::array::js_array_push_f64(out_once, crate::array::js_array_get_f64(once_raw, i));
    }
    out_listeners = crate::array::js_array_push_f64(out_listeners, listener);
    out_once = crate::array::js_array_push_f64(out_once, bool_value(once));
    set_listener_storage(
        socket,
        event,
        boxed_pointer(out_listeners as *const u8),
        boxed_pointer(out_once as *const u8),
    );
}

pub(crate) fn listener_snapshot(socket: f64, event: f64) -> Vec<(f64, bool)> {
    let Some((listeners, once_flags)) = listener_storage(socket, event) else {
        return Vec::new();
    };
    let listeners_raw = raw_ptr_from_value(listeners) as *const ArrayHeader;
    let once_raw = raw_ptr_from_value(once_flags) as *const ArrayHeader;
    if listeners_raw.is_null() || once_raw.is_null() {
        return Vec::new();
    }
    let len = crate::array::js_array_length(listeners_raw);
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len {
        out.push((
            crate::array::js_array_get_f64(listeners_raw, i),
            crate::value::js_is_truthy(crate::array::js_array_get_f64(once_raw, i)) != 0,
        ));
    }
    out
}

pub(crate) fn remove_listener(socket: f64, event: f64, listener: f64) -> bool {
    let Some((listeners, once_flags)) = listener_storage(socket, event) else {
        return false;
    };
    let listeners_raw = raw_ptr_from_value(listeners) as *const ArrayHeader;
    let once_raw = raw_ptr_from_value(once_flags) as *const ArrayHeader;
    if listeners_raw.is_null() || once_raw.is_null() {
        return false;
    }
    let len = crate::array::js_array_length(listeners_raw);
    let mut remove_idx = None;
    for i in (0..len).rev() {
        if crate::array::js_array_get_f64(listeners_raw, i).to_bits() == listener.to_bits() {
            remove_idx = Some(i);
            break;
        }
    }
    let Some(remove_idx) = remove_idx else {
        return false;
    };
    let mut out_listeners = crate::array::js_array_alloc(len.saturating_sub(1));
    let mut out_once = crate::array::js_array_alloc(len.saturating_sub(1));
    for i in 0..len {
        if i == remove_idx {
            continue;
        }
        out_listeners = crate::array::js_array_push_f64(
            out_listeners,
            crate::array::js_array_get_f64(listeners_raw, i),
        );
        out_once =
            crate::array::js_array_push_f64(out_once, crate::array::js_array_get_f64(once_raw, i));
    }
    set_listener_storage(
        socket,
        event,
        boxed_pointer(out_listeners as *const u8),
        boxed_pointer(out_once as *const u8),
    );
    true
}

pub(crate) fn remove_once_listeners(socket: f64, event: f64) {
    let Some((listeners, once_flags)) = listener_storage(socket, event) else {
        return;
    };
    let listeners_raw = raw_ptr_from_value(listeners) as *const ArrayHeader;
    let once_raw = raw_ptr_from_value(once_flags) as *const ArrayHeader;
    if listeners_raw.is_null() || once_raw.is_null() {
        return;
    }
    let len = crate::array::js_array_length(listeners_raw);
    let mut out_listeners = crate::array::js_array_alloc(len);
    let mut out_once = crate::array::js_array_alloc(len);
    for i in 0..len {
        let once = crate::value::js_is_truthy(crate::array::js_array_get_f64(once_raw, i)) != 0;
        if !once {
            out_listeners = crate::array::js_array_push_f64(
                out_listeners,
                crate::array::js_array_get_f64(listeners_raw, i),
            );
            out_once = crate::array::js_array_push_f64(
                out_once,
                crate::array::js_array_get_f64(once_raw, i),
            );
        }
    }
    set_listener_storage(
        socket,
        event,
        boxed_pointer(out_listeners as *const u8),
        boxed_pointer(out_once as *const u8),
    );
}

pub(crate) fn emit_event_value(socket: f64, event: f64, args: &[f64]) -> bool {
    let snapshot = listener_snapshot(socket, event);
    if snapshot.is_empty() {
        return false;
    }
    if snapshot.iter().any(|(_, once)| *once) {
        remove_once_listeners(socket, event);
    }
    for (listener, _) in snapshot {
        call_function(listener, socket, args);
    }
    true
}

pub(crate) fn emit_event(socket: f64, event: &str, args: &[f64]) -> bool {
    emit_event_value(socket, str_value(event), args)
}

/// `socket.eventNames()` — the list of events with at least one registered
/// listener, in registration order. Recomputed from the socket's hidden
/// listener-storage fields (keyed by `EVENT_LISTENERS_PREFIX`) so it self-
/// corrects when `once` listeners fire or listeners are removed, matching
/// Node's EventEmitter.eventNames().
pub(crate) fn event_names_impl(socket: f64) -> f64 {
    let Some(obj) = object_ptr_from_value(socket) else {
        return boxed_pointer(crate::array::js_array_alloc(0) as *const u8);
    };
    let keys = js_object_keys(obj);
    let mut out = crate::array::js_array_alloc(0);
    if !keys.is_null() {
        let len = crate::array::js_array_length(keys);
        for i in 0..len {
            let Some(key_name) = string_to_rust(crate::array::js_array_get_f64(keys, i)) else {
                continue;
            };
            let Some(event) = key_name
                .as_bytes()
                .strip_prefix(EVENT_LISTENERS_PREFIX)
                .map(|rest| String::from_utf8_lossy(rest).into_owned())
            else {
                continue;
            };
            let event_value = str_value(&event);
            if !listener_snapshot(socket, event_value).is_empty() {
                out = crate::array::js_array_push_f64(out, event_value);
            }
        }
    }
    boxed_pointer(out as *const u8)
}
