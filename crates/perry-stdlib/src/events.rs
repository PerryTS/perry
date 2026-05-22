//! EventEmitter implementation
//!
//! Native implementation of Node.js EventEmitter pattern.
//! Rewritten for issue #850 — Node-compatible listener-table semantics
//! covering `on` / `once` / `addListener` / `prependListener` /
//! `prependOnceListener` / `removeListener` / `removeAllListeners` /
//! `listenerCount` / `listeners` / `rawListeners` / `eventNames` /
//! `setMaxListeners` / `getMaxListeners`, plus the module-level
//! `events.once` / `events.getEventListeners` / `events.listenerCount` /
//! `events.setMaxListeners` / `events.getMaxListeners` helpers.
//!
//! ## Storage model
//!
//! Each `EventEmitterHandle` stores an ordered list of events (so
//! `eventNames()` returns insertion order, matching Node) plus per-event
//! `Vec<Listener>` with insert-back (`on`/`addListener`) and insert-front
//! (`prependListener`). Each `Listener` carries a `once` flag — `emit`
//! collects all `once` listeners, fires the whole snapshot, then prunes
//! the fired ones from the live list. Pending `events.once` promises are
//! stored alongside listeners so a single `emit` can resolve them all.

use perry_runtime::{
    js_array_alloc, js_array_length, js_array_push_f64, js_closure_call0, js_closure_call1,
    js_nanbox_pointer, js_nanbox_string, js_object_alloc, js_promise_new, js_promise_resolve,
    js_string_from_bytes, ArrayHeader, ClosureHeader, Promise, StringHeader,
};
use std::collections::HashMap;

const TAG_FALSE_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0003);
const TAG_TRUE_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0004);

fn bool_to_js(value: bool) -> f64 {
    if value {
        TAG_TRUE_F64
    } else {
        TAG_FALSE_F64
    }
}

use crate::common::{for_each_handle_mut_of, get_handle_mut, register_handle, Handle};

/// One registered listener: a raw closure pointer (i64 to satisfy
/// Send + Sync — the underlying ClosureHeader is GC-managed) plus a
/// `once` flag.
#[derive(Copy, Clone)]
struct Listener {
    callback: i64,
    once: bool,
}

/// EventEmitter handle.
///
/// `events` is a `HashMap<String, Vec<Listener>>` for O(1) lookup; the
/// parallel `event_order` `Vec<String>` preserves insertion order so
/// `eventNames()` matches Node's behaviour (first-seen order).
pub struct EventEmitterHandle {
    /// Event name → list of listeners. Order within the Vec is dispatch
    /// order (front-of-Vec fires first).
    events: HashMap<String, Vec<Listener>>,
    /// Insertion-order shadow of `events.keys()`. Names that get fully
    /// drained (e.g. via `removeAllListeners(name)`) are removed.
    event_order: Vec<String>,
    /// Per-event pending `events.once(em, name)` promises. Resolved on
    /// the next `emit(name, ...)` with a single-element array.
    pending_once_promises: HashMap<String, Vec<*mut Promise>>,
    /// `setMaxListeners` ceiling. Node's default is 10 but we don't warn
    /// when the count exceeds it — `getMaxListeners()` just reads back
    /// whatever was written.
    max_listeners: i32,
}

// SAFETY: `*mut Promise` is not Send/Sync by default, but the runtime
// pins Promise allocations and the registry's GC scanner marks them
// through `pending_once_promises` so they survive minor GC cycles.
unsafe impl Send for EventEmitterHandle {}
unsafe impl Sync for EventEmitterHandle {}

static EVENTS_GC_REGISTERED: std::sync::Once = std::sync::Once::new();

/// Register the EventEmitter GC root scanner exactly once. User closures
/// passed to `emitter.on(event, cb)` live inside EventEmitterHandle
/// values in the handle registry; without this scanner, a malloc-triggered
/// GC between `.on(...)` and the next `.emit(...)` would sweep the
/// closure — same root cause as issue #35 for net.Socket listeners.
fn ensure_gc_scanner_registered() {
    EVENTS_GC_REGISTERED.call_once(|| {
        perry_runtime::gc::gc_register_mutable_root_scanner_named(
            "stdlib:events",
            scan_events_roots_mut,
        );
    });
}

/// GC root scanner for EventEmitter listener closures and pending
/// `events.once` promises.
#[allow(dead_code)]
fn scan_events_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = perry_runtime::gc::RuntimeRootVisitor::for_copy(mark);
    scan_events_roots_mut(&mut visitor);
}

fn scan_events_roots_mut(visitor: &mut perry_runtime::gc::RuntimeRootVisitor<'_>) {
    for_each_handle_mut_of::<EventEmitterHandle, _>(|emitter| {
        for listeners in emitter.events.values_mut() {
            for l in listeners.iter_mut() {
                visitor.visit_i64_slot(&mut l.callback);
            }
        }
        for proms in emitter.pending_once_promises.values_mut() {
            for p in proms.iter_mut() {
                visitor.visit_raw_mut_ptr_slot(p);
            }
        }
    });
}

impl Default for EventEmitterHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl EventEmitterHandle {
    pub fn new() -> Self {
        EventEmitterHandle {
            events: HashMap::new(),
            event_order: Vec::new(),
            pending_once_promises: HashMap::new(),
            // Node's default is 10. We mirror it so `getMaxListeners()`
            // on a fresh emitter returns 10 (matching Node).
            max_listeners: 10,
        }
    }

    fn note_event(&mut self, name: &str) {
        if !self.events.contains_key(name) {
            self.event_order.push(name.to_string());
        }
    }

    fn prune_event_if_empty(&mut self, name: &str) {
        let drop_it = match self.events.get(name) {
            Some(v) => v.is_empty(),
            None => true,
        };
        if drop_it {
            self.events.remove(name);
            if let Some(pos) = self.event_order.iter().position(|s| s == name) {
                self.event_order.remove(pos);
            }
        }
    }

    fn emit_meta_event(&self, meta_name: &str, event_name: &str) {
        let snapshot = match self.events.get(meta_name) {
            Some(v) if !v.is_empty() => v.clone(),
            _ => return,
        };
        let bytes = event_name.as_bytes();
        let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        let event_arg = js_nanbox_string(str_ptr as i64);
        for l in snapshot {
            if l.callback != 0 {
                let closure_ptr = l.callback as *const ClosureHeader;
                js_closure_call1(closure_ptr, event_arg);
            }
        }
    }

    fn add_listener(&mut self, name: &str, callback: i64, once: bool, prepend: bool) {
        self.emit_meta_event("newListener", name);
        self.note_event(name);
        let vec = self.events.entry(name.to_string()).or_default();
        let listener = Listener { callback, once };
        if prepend {
            vec.insert(0, listener);
        } else {
            vec.push(listener);
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

/// EventEmitter.on(eventName, listener) — also serves as `addListener`.
/// Register a listener for the specified event.
/// Returns the emitter handle for chaining.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_on(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
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
        emitter.add_listener(&event_name, callback_ptr, false, false);
    }
    handle
}

/// EventEmitter.once(eventName, listener) — fires once, then auto-removes.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_once(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
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
        emitter.add_listener(&event_name, callback_ptr, true, false);
    }
    handle
}

/// EventEmitter.prependListener(eventName, listener) — insert at front.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_prepend_listener(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
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
        emitter.add_listener(&event_name, callback_ptr, false, true);
    }
    handle
}

/// EventEmitter.prependOnceListener(eventName, listener).
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_prepend_once_listener(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
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
        emitter.add_listener(&event_name, callback_ptr, true, true);
    }
    handle
}

/// Drain pending `events.once` promises for `event_name` on `handle`,
/// resolving each with the full emitted args array.
unsafe fn drain_pending_once_promises(
    emitter: &mut EventEmitterHandle,
    event_name: &str,
    args_ptr: *mut ArrayHeader,
) {
    let pending = match emitter.pending_once_promises.remove(event_name) {
        Some(v) => v,
        None => return,
    };
    let arr = if args_ptr.is_null() {
        js_array_alloc(0)
    } else {
        args_ptr
    };
    let boxed_arr = js_nanbox_pointer(arr as i64);
    for promise_ptr in pending {
        if !promise_ptr.is_null() {
            js_promise_resolve(promise_ptr, boxed_arr);
        }
    }
}

unsafe fn first_arg_or_undefined(args_ptr: *const ArrayHeader) -> f64 {
    if args_ptr.is_null() || js_array_length(args_ptr) == 0 {
        f64::from_bits(TAG_UNDEFINED_F64_BITS)
    } else {
        perry_runtime::array::js_array_get_f64(args_ptr, 0)
    }
}

const TAG_UNDEFINED_F64_BITS: u64 = 0x7FFC_0000_0000_0001;

/// EventEmitter.emit(eventName, ...args)
/// Emit an event with variadic arguments packed into an ArrayHeader.
/// Returns true if there were listeners, false otherwise.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_emit(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    args_ptr: *mut ArrayHeader,
) -> f64 {
    let event_name = match string_from_header(event_name_ptr) {
        Some(name) => name,
        None => return TAG_FALSE_F64,
    };

    let mut had_listeners = false;
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        // Snapshot the listener vec, then prune `once`-listeners from
        // the live vec before dispatching. This matches Node semantics:
        // a once-listener removed mid-dispatch still fires this emit,
        // but is gone for the next one.
        let snapshot: Vec<Listener> = match emitter.events.get(&event_name) {
            Some(v) if !v.is_empty() => v.clone(),
            _ => Vec::new(),
        };
        if !snapshot.is_empty() {
            had_listeners = true;
            if snapshot.iter().any(|l| l.once) {
                if let Some(v) = emitter.events.get_mut(&event_name) {
                    v.retain(|l| !l.once);
                }
                emitter.prune_event_if_empty(&event_name);
            }
        }

        if event_name == "error" && snapshot.is_empty() {
            perry_runtime::exception::js_throw(first_arg_or_undefined(args_ptr));
        }

        // Resolve any pending `events.once` Promises before dispatch.
        drain_pending_once_promises(emitter, &event_name, args_ptr);

        let first_arg = first_arg_or_undefined(args_ptr);
        for l in snapshot {
            if l.callback != 0 {
                let closure_ptr = l.callback as *const ClosureHeader;
                js_closure_call1(closure_ptr, first_arg);
            }
        }
    }

    bool_to_js(had_listeners)
}

/// EventEmitter.emit with no arguments
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_emit0(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> f64 {
    let event_name = match string_from_header(event_name_ptr) {
        Some(name) => name,
        None => return TAG_FALSE_F64,
    };

    let mut had_listeners = false;
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        let snapshot: Vec<Listener> = match emitter.events.get(&event_name) {
            Some(v) if !v.is_empty() => v.clone(),
            _ => Vec::new(),
        };
        if !snapshot.is_empty() {
            had_listeners = true;
            if snapshot.iter().any(|l| l.once) {
                if let Some(v) = emitter.events.get_mut(&event_name) {
                    v.retain(|l| !l.once);
                }
                emitter.prune_event_if_empty(&event_name);
            }
        }

        let empty_args = js_array_alloc(0);
        if event_name == "error" && snapshot.is_empty() {
            perry_runtime::exception::js_throw(f64::from_bits(TAG_UNDEFINED_F64_BITS));
        }
        drain_pending_once_promises(emitter, &event_name, empty_args);

        for l in snapshot {
            if l.callback != 0 {
                let closure_ptr = l.callback as *const ClosureHeader;
                js_closure_call0(closure_ptr);
            }
        }
    }

    bool_to_js(had_listeners)
}

/// EventEmitter.removeListener(eventName, listener)
/// Remove the first matching listener for the event.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_remove_listener(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
) -> Handle {
    let event_name = match string_from_header(event_name_ptr) {
        Some(name) => name,
        None => return handle,
    };

    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        let mut removed = false;
        if let Some(listeners) = emitter.events.get_mut(&event_name) {
            // Node removes only the first matching listener, not all.
            if let Some(pos) = listeners.iter().position(|l| l.callback == callback_ptr) {
                listeners.remove(pos);
                removed = true;
            }
        }
        if removed {
            emitter.prune_event_if_empty(&event_name);
            emitter.emit_meta_event("removeListener", &event_name);
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
            let removed: Vec<String> = emitter
                .event_order
                .iter()
                .filter(|name| name.as_str() != "removeListener")
                .flat_map(|name| {
                    let count = emitter.events.get(name).map(|v| v.len()).unwrap_or(0);
                    std::iter::repeat(name.clone()).take(count)
                })
                .collect();
            emitter.events.clear();
            emitter.event_order.clear();
            for name in removed {
                emitter.emit_meta_event("removeListener", &name);
            }
        } else if let Some(event_name) = string_from_header(event_name_ptr) {
            let removed_count = emitter
                .events
                .get(&event_name)
                .map(|v| v.len())
                .unwrap_or(0);
            emitter.events.remove(&event_name);
            if let Some(pos) = emitter.event_order.iter().position(|s| s == &event_name) {
                emitter.event_order.remove(pos);
            }
            if event_name != "removeListener" {
                for _ in 0..removed_count {
                    emitter.emit_meta_event("removeListener", &event_name);
                }
            }
        }
    }
    handle
}

/// EventEmitter.listenerCount(eventName)
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_listener_count(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
) -> f64 {
    let event_name = match string_from_header(event_name_ptr) {
        Some(name) => name,
        None => return 0.0,
    };
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        if let Some(listeners) = emitter.events.get(&event_name) {
            if callback_ptr != 0 {
                return listeners
                    .iter()
                    .filter(|listener| listener.callback == callback_ptr)
                    .count() as f64;
            }
            return listeners.len() as f64;
        }
    }
    0.0
}

/// EventEmitter.setMaxListeners(n).
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_set_max_listeners(handle: Handle, n: f64) -> Handle {
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        emitter.max_listeners = n as i32;
    }
    handle
}

/// EventEmitter.getMaxListeners().
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_get_max_listeners(handle: Handle) -> f64 {
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        return emitter.max_listeners as f64;
    }
    // Node's default for a stranger emitter is 10.
    10.0
}

/// EventEmitter.eventNames() — returns an array of strings in insertion
/// order (matches Node).
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_event_names(handle: Handle) -> *mut ArrayHeader {
    let arr = js_array_alloc(0);
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        let mut result = arr;
        for name in emitter.event_order.iter() {
            // Skip events that have been emptied without prune (shouldn't
            // happen, but defensive).
            let alive = emitter
                .events
                .get(name)
                .map(|v| !v.is_empty())
                .unwrap_or(false);
            if !alive {
                continue;
            }
            let bytes = name.as_bytes();
            let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            let nanboxed = js_nanbox_string(str_ptr as i64);
            result = js_array_push_f64(result, nanboxed);
        }
        return result;
    }
    arr
}

/// EventEmitter.listeners(eventName) — returns an array of the registered
/// listener closures (NaN-boxed POINTER_TAG). For the `once` case Node
/// returns the *unwrapped* user closure; we already store the user
/// closure directly so the result matches.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_listeners(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> *mut ArrayHeader {
    let arr = js_array_alloc(0);
    let event_name = match string_from_header(event_name_ptr) {
        Some(name) => name,
        None => return arr,
    };
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        if let Some(listeners) = emitter.events.get(&event_name) {
            let mut result = arr;
            for l in listeners.iter() {
                if l.callback != 0 {
                    let nanboxed = js_nanbox_pointer(l.callback);
                    result = js_array_push_f64(result, nanboxed);
                }
            }
            return result;
        }
    }
    arr
}

/// EventEmitter.rawListeners(eventName) — identical to `listeners` in
/// our model since we don't wrap `once` listeners at registration time
/// (the `once` flag is stored alongside the user closure).
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_raw_listeners(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> *mut ArrayHeader {
    js_event_emitter_listeners(handle, event_name_ptr)
}

// ============================================================================
// Module-level helpers — `events.once(em, name)`, `events.on(em, name)`,
// `events.getEventListeners(em, name)`, `events.listenerCount(em, name)`,
// `events.setMaxListeners(n, em)`, `events.getMaxListeners(em)`.
// ============================================================================

/// `events.once(emitter, eventName)` — returns a Promise that resolves
/// to an array of the args fired by the next `emit(eventName, ...)`.
///
/// Node returns the *full* args array (e.g. `emit('x', 1, 2)` resolves
/// to `[1, 2]`). Perry's emit FFI today is single-arg, so the resolved
/// array is single-element. That's enough for the parity probe in
/// issue #850; multi-arg parity is a follow-up.
#[no_mangle]
pub unsafe extern "C" fn js_events_once(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> *mut Promise {
    ensure_gc_scanner_registered();
    let promise = js_promise_new();
    let event_name = match string_from_header(event_name_ptr) {
        Some(name) => name,
        None => return promise,
    };
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        emitter
            .pending_once_promises
            .entry(event_name)
            .or_default()
            .push(promise);
    }
    promise
}

extern "C" fn events_on_queue_listener(closure: *const ClosureHeader, arg0: f64) -> f64 {
    use perry_runtime::closure::js_closure_get_capture_ptr;

    let queue = js_closure_get_capture_ptr(closure, 0) as *mut ArrayHeader;
    if !queue.is_null() {
        let mut args = js_array_alloc(0);
        args = js_array_push_f64(args, arg0);
        let args_val = js_nanbox_pointer(args as i64);
        let _ = js_array_push_f64(queue, args_val);
    }

    f64::from_bits(TAG_UNDEFINED_F64_BITS)
}

/// `events.on(emitter, eventName)` — returns an async-iterable queue of
/// argument arrays. Perry's `for await` lowering already accepts plain arrays
/// as async-iterable inputs, so the current implementation backs the iterator
/// with an Array and appends one `[arg]` entry per emitted event.
#[no_mangle]
pub unsafe extern "C" fn js_events_on(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> *mut ArrayHeader {
    use perry_runtime::closure::{js_closure_alloc, js_closure_set_capture_ptr};

    ensure_gc_scanner_registered();
    let queue = js_array_alloc(0);
    let event_name = match string_from_header(event_name_ptr) {
        Some(name) => name,
        None => return queue,
    };

    let listener = js_closure_alloc(events_on_queue_listener as *const u8, 1);
    js_closure_set_capture_ptr(listener, 0, queue as i64);

    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        emitter.add_listener(&event_name, listener as i64, false, false);
    }

    queue
}

/// `events.addAbortListener(signal, listener)` — attach listener to AbortSignal
/// and return a disposable-shaped object. The dispose method is currently a
/// function-shaped placeholder; listener removal can be tightened later.
#[no_mangle]
pub unsafe extern "C" fn js_events_add_abort_listener(signal_ptr: i64, callback_ptr: i64) -> i64 {
    if signal_ptr != 0 && callback_ptr != 0 {
        let event_name = b"abort";
        let event_str = js_string_from_bytes(event_name.as_ptr(), event_name.len() as u32);
        let event_val = js_nanbox_string(event_str as i64);
        let listener_val = js_nanbox_pointer(callback_ptr);
        perry_runtime::url::js_abort_signal_add_listener(
            signal_ptr as *mut perry_runtime::ObjectHeader,
            event_val,
            listener_val,
        );

        let disposable = js_object_alloc(0, 0);
        let disposable_val = js_nanbox_pointer(disposable as i64);
        let dispose_sym = perry_runtime::symbol::well_known_symbol("dispose");
        let dispose_sym_val = js_nanbox_pointer(dispose_sym as i64);
        perry_runtime::symbol::js_object_set_symbol_property(
            disposable_val,
            dispose_sym_val,
            listener_val,
        );
        disposable as i64
    } else {
        0
    }
}

/// `events.getEventListeners(emitter, eventName)` — alias for
/// `emitter.listeners(eventName)`.
#[no_mangle]
pub unsafe extern "C" fn js_events_get_event_listeners(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> *mut ArrayHeader {
    js_event_emitter_listeners(handle, event_name_ptr)
}

/// `events.listenerCount(emitter, eventName)` — alias for
/// `emitter.listenerCount(eventName)`.
#[no_mangle]
pub unsafe extern "C" fn js_events_listener_count(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> f64 {
    js_event_emitter_listener_count(handle, event_name_ptr, 0)
}

/// `events.getMaxListeners(emitter)` — alias.
#[no_mangle]
pub unsafe extern "C" fn js_events_get_max_listeners(handle: Handle) -> f64 {
    js_event_emitter_get_max_listeners(handle)
}

/// `events.setMaxListeners(n, ...emitters)` — Perry FFI takes a single
/// emitter handle. The codegen wraps multi-target calls by emitting
/// one FFI call per target; for the common single-emitter case below
/// this is exactly the right shape.
#[no_mangle]
pub unsafe extern "C" fn js_events_set_max_listeners(
    n: f64,
    handles_ptr: *const ArrayHeader,
) -> f64 {
    if !handles_ptr.is_null() {
        let len = js_array_length(handles_ptr);
        for i in 0..len {
            let handle_val = perry_runtime::array::js_array_get_f64(handles_ptr, i);
            let handle = handle_val.to_bits() as Handle;
            let handle = if (handle_val.to_bits() & 0xFFFF_0000_0000_0000) == 0x7FFD_0000_0000_0000
            {
                (handle_val.to_bits() & 0x0000_FFFF_FFFF_FFFF) as Handle
            } else {
                handle
            };
            if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
                emitter.max_listeners = n as i32;
            }
        }
    }
    f64::from_bits(TAG_UNDEFINED_F64_BITS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_scanner_emits_listeners_and_pending_promises() {
        let mut emitter = EventEmitterHandle::new();
        emitter.add_listener("data", 0x1234_5678, false, false);
        emitter
            .pending_once_promises
            .entry("ready".to_string())
            .or_default()
            .push(0x2345_6780 as *mut Promise);
        let handle = register_handle(emitter);

        let mut emitted = Vec::new();
        scan_events_roots(&mut |value| emitted.push(value.to_bits()));

        assert!(emitted.contains(&(0x7FFD_0000_0000_0000 | 0x1234_5678)));
        assert!(emitted.contains(&(0x7FFD_0000_0000_0000 | 0x2345_6780)));
        crate::common::drop_handle(handle);
    }
}
