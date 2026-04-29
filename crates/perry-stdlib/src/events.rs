//! EventEmitter implementation
//!
//! Native implementation of Node.js EventEmitter pattern.
//! Provides on(), emit(), and removeListener() for event handling.

use perry_runtime::{
    js_closure_call0, js_closure_call1, js_string_from_bytes, ClosureHeader, StringHeader,
};
use std::collections::HashMap;

use crate::common::{for_each_handle_of, get_handle_mut, register_handle, Handle};

/// EventEmitter handle storing event listeners
/// We store closure pointers as i64 to satisfy Send + Sync requirements
/// (raw pointers aren't Send/Sync, but the underlying data is managed by the runtime)
pub struct EventEmitterHandle {
    /// Event name -> list of closure pointers (stored as i64 for Send + Sync)
    listeners: HashMap<String, Vec<i64>>,
}

static EVENTS_GC_REGISTERED: std::sync::Once = std::sync::Once::new();

/// Register the EventEmitter GC root scanner exactly once. User closures
/// passed to `emitter.on(event, cb)` are stored inside EventEmitterHandle
/// values in the handle registry; without this scanner, a malloc-triggered
/// GC between `.on(...)` and the next `.emit(...)` would sweep the
/// closure — same root cause as issue #35 for net.Socket listeners.
fn ensure_gc_scanner_registered() {
    EVENTS_GC_REGISTERED.call_once(|| {
        perry_runtime::gc::gc_register_root_scanner(scan_events_roots);
    });
}

/// GC root scanner for EventEmitter listener closures.
fn scan_events_roots(mark: &mut dyn FnMut(f64)) {
    for_each_handle_of::<EventEmitterHandle, _>(|emitter| {
        for cb_vec in emitter.listeners.values() {
            for &cb in cb_vec.iter() {
                if cb != 0 {
                    let boxed =
                        f64::from_bits(0x7FFD_0000_0000_0000 | (cb as u64 & 0x0000_FFFF_FFFF_FFFF));
                    mark(boxed);
                }
            }
        }
    });
}

impl EventEmitterHandle {
    pub fn new() -> Self {
        EventEmitterHandle {
            listeners: HashMap::new(),
        }
    }
}

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(String::from_utf8_lossy(bytes).to_string())
}

/// Create a new EventEmitter
/// Returns a handle (i64) to the emitter
#[no_mangle]
pub extern "C" fn js_event_emitter_new() -> Handle {
    ensure_gc_scanner_registered();
    register_handle(EventEmitterHandle::new())
}

/// EventEmitter.on(eventName, listener)
/// Register a listener for the specified event.
/// Returns the emitter handle for chaining.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_on(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64, // Closure pointer passed as i64
) -> Handle {
    ensure_gc_scanner_registered();
    let event_name = match string_from_header(event_name_ptr) {
        Some(name) => name,
        None => return handle,
    };

    if callback_ptr == 0 {
        return handle;
    }

    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        emitter
            .listeners
            .entry(event_name)
            .or_insert_with(Vec::new)
            .push(callback_ptr);
    }

    handle
}

/// EventEmitter.emit(eventName, arg)
/// Emit an event with a single argument.
/// Returns true if there were listeners, false otherwise.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_emit(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    arg: f64,
) -> bool {
    let event_name = match string_from_header(event_name_ptr) {
        Some(name) => name,
        None => return false,
    };

    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        if let Some(listeners) = emitter.listeners.get(&event_name) {
            if listeners.is_empty() {
                return false;
            }

            // Clone the listeners to avoid borrowing issues during iteration
            let listeners_copy: Vec<i64> = listeners.clone();

            // Call each listener with the argument
            for callback_ptr in listeners_copy {
                if callback_ptr != 0 {
                    let closure_ptr = callback_ptr as *const ClosureHeader;
                    js_closure_call1(closure_ptr, arg);
                }
            }

            return true;
        }
    }

    false
}

/// EventEmitter.emit with no arguments
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_emit0(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> bool {
    let event_name = match string_from_header(event_name_ptr) {
        Some(name) => name,
        None => return false,
    };

    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        if let Some(listeners) = emitter.listeners.get(&event_name) {
            if listeners.is_empty() {
                return false;
            }

            // Clone the listeners to avoid borrowing issues during iteration
            let listeners_copy: Vec<i64> = listeners.clone();

            // Call each listener with no arguments
            for callback_ptr in listeners_copy {
                if callback_ptr != 0 {
                    let closure_ptr = callback_ptr as *const ClosureHeader;
                    js_closure_call0(closure_ptr);
                }
            }

            return true;
        }
    }

    false
}

/// EventEmitter.removeListener(eventName, listener)
/// Remove a specific listener from an event.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_remove_listener(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64, // Closure pointer passed as i64
) -> Handle {
    let event_name = match string_from_header(event_name_ptr) {
        Some(name) => name,
        None => return handle,
    };

    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        if let Some(listeners) = emitter.listeners.get_mut(&event_name) {
            listeners.retain(|&p| p != callback_ptr);
        }
    }

    handle
}

/// EventEmitter.removeAllListeners(eventName?)
/// Remove all listeners for an event (or all events if no name given).
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_remove_all_listeners(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> Handle {
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        if event_name_ptr.is_null() {
            // Remove all listeners for all events
            emitter.listeners.clear();
        } else if let Some(event_name) = string_from_header(event_name_ptr) {
            // Remove all listeners for specific event
            emitter.listeners.remove(&event_name);
        }
    }

    handle
}

/// EventEmitter.listenerCount(eventName)
/// Get the number of listeners for an event.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_listener_count(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> f64 {
    let event_name = match string_from_header(event_name_ptr) {
        Some(name) => name,
        None => return 0.0,
    };

    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        if let Some(listeners) = emitter.listeners.get(&event_name) {
            return listeners.len() as f64;
        }
    }

    0.0
}
