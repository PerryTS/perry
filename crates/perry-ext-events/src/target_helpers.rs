use super::*;

pub(super) enum EventHelperTarget {
    EventEmitter(Handle),
    EventTarget(*mut u8),
    Stream(Handle),
}

pub(super) unsafe fn event_helper_target(value: f64) -> Option<EventHelperTarget> {
    let handle = handle_from_value(value);
    if is_local_event_emitter_handle(handle) {
        return Some(EventHelperTarget::EventEmitter(handle));
    }
    if let Some(target) = event_target_ptr(handle) {
        return Some(EventHelperTarget::EventTarget(target));
    }
    if stream_value_from_handle(handle).is_some() {
        return Some(EventHelperTarget::Stream(handle));
    }
    None
}

pub(super) unsafe fn event_target_array_len(
    target: *mut u8,
    event_name_ptr: *const StringHeader,
) -> f64 {
    let listeners = js_event_target_get_event_listeners(target, event_name_ptr);
    if listeners.is_null() {
        0.0
    } else {
        (*listeners).length as f64
    }
}

pub(super) unsafe fn stream_array_len(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> Option<f64> {
    stream_listeners_for_heap_object(handle, event_name_ptr).map(|arr| {
        if arr.is_null() {
            0.0
        } else {
            (*arr).length as f64
        }
    })
}
