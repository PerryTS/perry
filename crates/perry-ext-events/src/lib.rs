//! Native bindings for Node's `events` module — `EventEmitter`
//! with the Node-compatible listener-table surface.
//!
//! First wrapper port that exercises perry-ffi's GC-root-scanner
//! surface (added in v0.5.546). User closures passed to
//! `emitter.on(event, cb)` live inside an `EventEmitterHandle`
//! value in the registry; without an explicit GC scanner, a
//! malloc-triggered GC between `.on()` and `.emit()` would sweep
//! the closure (issue #35 pattern).
//!
//! Issue #850 — rewrote the listener storage to match Node semantics
//! (per-event ordered `Vec<Listener>` with `once` flag, insertion-order
//! event-name shadow, max-listeners ceiling, pending `events.once`
//! promises). Added the previously-missing `.once` / `.addListener` /
//! `.prependListener` / `.prependOnceListener` / `.listeners` /
//! `.rawListeners` / `.eventNames` / `.setMaxListeners` /
//! `.getMaxListeners` instance methods plus the module-level
//! `events.once` / `events.getEventListeners` / `events.listenerCount` /
//! `events.getMaxListeners` / `events.setMaxListeners` helpers.

use perry_ffi::{
    gc_register_root_scanner, get_handle_mut, iter_handles_of, js_array_alloc, js_array_push,
    nanbox_string_bits, read_string, register_handle, ArrayHeader, Handle, JsClosure, JsPromise,
    JsString, JsValue, Promise, RawClosureHeader, StringHeader,
};
use std::collections::HashMap;

// Direct hook into perry-runtime's sync Promise resolve.
//
// `JsPromise::resolve_*` route through `perry_ffi_promise_resolve_bits`
// which calls `async_bridge::queue_promise_resolution` — that requires
// the perry-stdlib pump to be registered before the resolution is
// applied to the Promise. `events.once(em, name)` followed by a
// synchronous `em.emit(...)` and a synchronous `await p` doesn't go
// through any perry-stdlib spawn helper, so the pump is never
// registered and the await hangs forever waiting for a state change
// that's stuck in the deferred queue.
//
// Resolving synchronously instead — same path perry-stdlib's
// `js_promise_resolved(value)` uses — settles the Promise immediately,
// matches the `then`/`await` ordering Node expects, and sidesteps the
// pump-registration coupling entirely.
extern "C" {
    fn js_promise_resolve(promise: *mut Promise, value: f64);
}

/// One registered listener: a raw closure pointer (i64 to satisfy
/// Send + Sync — the underlying ClosureHeader is GC-managed) plus a
/// `once` flag.
#[derive(Copy, Clone)]
struct Listener {
    callback: i64,
    once: bool,
}

/// EventEmitter handle with Node-compatible listener-table semantics
/// (issue #850).
pub struct EventEmitterHandle {
    events: HashMap<String, Vec<Listener>>,
    event_order: Vec<String>,
    pending_once_promises: HashMap<String, Vec<*mut Promise>>,
    max_listeners: i32,
}

// SAFETY: `*mut Promise` is not Send/Sync by default, but the runtime
// pins Promise allocations and the registry's GC scanner marks them
// through `pending_once_promises` so they survive minor GC cycles.
unsafe impl Send for EventEmitterHandle {}
unsafe impl Sync for EventEmitterHandle {}

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
            // Node's default — `getMaxListeners()` on a fresh emitter
            // returns 10.
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

    fn add_listener(&mut self, name: &str, callback: i64, once: bool, prepend: bool) {
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

static EVENTS_GC_REGISTERED: std::sync::Once = std::sync::Once::new();

fn ensure_gc_scanner_registered() {
    EVENTS_GC_REGISTERED.call_once(|| {
        gc_register_root_scanner(scan_events_roots);
    });
}

/// GC root scanner: visit every registered EventEmitterHandle,
/// mark every listener closure pointer + pending Promise as a root.
fn scan_events_roots(mark: &mut dyn FnMut(f64)) {
    iter_handles_of::<EventEmitterHandle, _>(|emitter| {
        for listeners in emitter.events.values() {
            for l in listeners.iter() {
                if l.callback != 0 {
                    // POINTER_TAG (0x7FFD) over the closure pointer.
                    let boxed = f64::from_bits(
                        0x7FFD_0000_0000_0000 | (l.callback as u64 & 0x0000_FFFF_FFFF_FFFF),
                    );
                    mark(boxed);
                }
            }
        }
        for proms in emitter.pending_once_promises.values() {
            for &p in proms.iter() {
                if !p.is_null() {
                    let boxed = f64::from_bits(
                        0x7FFD_0000_0000_0000 | ((p as u64) & 0x0000_FFFF_FFFF_FFFF),
                    );
                    mark(boxed);
                }
            }
        }
    });
}

unsafe fn read_str(ptr: *const StringHeader) -> Option<String> {
    let handle = JsString::from_raw(ptr as *mut StringHeader);
    read_string(handle).map(String::from)
}

/// `new EventEmitter()` — returns a handle to the emitter.
#[no_mangle]
pub extern "C" fn js_event_emitter_new() -> Handle {
    ensure_gc_scanner_registered();
    register_handle(EventEmitterHandle::new())
}

/// `emitter.on(eventName, listener)` — register a listener.
/// Also serves as `addListener` (wired at the codegen layer).
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
/// `callback_ptr` is a raw closure pointer (the runtime's
/// `ClosureHeader` cast to i64); 0 is the no-op sentinel.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_on(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
) -> Handle {
    ensure_gc_scanner_registered();
    let Some(event_name) = read_str(event_name_ptr) else {
        return handle;
    };
    if callback_ptr == 0 {
        return handle;
    }
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        emitter.add_listener(&event_name, callback_ptr, false, false);
    }
    handle
}

/// `emitter.once(eventName, listener)` — fires once then auto-removes.
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_once(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
) -> Handle {
    ensure_gc_scanner_registered();
    let Some(event_name) = read_str(event_name_ptr) else {
        return handle;
    };
    if callback_ptr == 0 {
        return handle;
    }
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        emitter.add_listener(&event_name, callback_ptr, true, false);
    }
    handle
}

/// `emitter.prependListener(eventName, listener)` — insert at front.
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_prepend_listener(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
) -> Handle {
    ensure_gc_scanner_registered();
    let Some(event_name) = read_str(event_name_ptr) else {
        return handle;
    };
    if callback_ptr == 0 {
        return handle;
    }
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        emitter.add_listener(&event_name, callback_ptr, false, true);
    }
    handle
}

/// `emitter.prependOnceListener(eventName, listener)`.
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_prepend_once_listener(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
) -> Handle {
    ensure_gc_scanner_registered();
    let Some(event_name) = read_str(event_name_ptr) else {
        return handle;
    };
    if callback_ptr == 0 {
        return handle;
    }
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        emitter.add_listener(&event_name, callback_ptr, true, true);
    }
    handle
}

/// Drain pending `events.once` promises for `event_name` on the given
/// emitter, resolving each with a single-element array `[arg]` (or
/// `[]` if `has_arg = false`).
unsafe fn drain_pending_once_promises(
    emitter: &mut EventEmitterHandle,
    event_name: &str,
    arg: f64,
    has_arg: bool,
) {
    let pending = match emitter.pending_once_promises.remove(event_name) {
        Some(v) => v,
        None => return,
    };
    for promise_ptr in pending {
        if promise_ptr.is_null() {
            continue;
        }
        let arr = js_array_alloc(1);
        let arr = if has_arg {
            js_array_push(arr, JsValue::from_bits(arg.to_bits()))
        } else {
            arr
        };
        let boxed_arr = JsValue::from_object_ptr(arr);
        // Synchronous resolve — see the comment on the extern at the
        // top of this file for why we bypass `JsPromise::resolve`.
        let bits = boxed_arr.bits();
        js_promise_resolve(promise_ptr, f64::from_bits(bits));
    }
}

/// `emitter.emit(eventName, arg)` — fire `arg` to every listener.
/// Returns true if any listeners ran.
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_emit(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    arg: f64,
) -> bool {
    let Some(event_name) = read_str(event_name_ptr) else {
        return false;
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

        drain_pending_once_promises(emitter, &event_name, arg, true);

        for l in snapshot {
            if l.callback != 0 {
                let closure = JsClosure::from_raw(l.callback as *const RawClosureHeader);
                let _ = closure.call1(arg);
            }
        }
    }
    had_listeners
}

/// `emitter.emit(eventName)` — no-args variant.
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_emit0(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> bool {
    let Some(event_name) = read_str(event_name_ptr) else {
        return false;
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

        drain_pending_once_promises(
            emitter,
            &event_name,
            f64::from_bits(0x7FFC_0000_0000_0001),
            false,
        );

        for l in snapshot {
            if l.callback != 0 {
                let closure = JsClosure::from_raw(l.callback as *const RawClosureHeader);
                let _ = closure.call0();
            }
        }
    }
    had_listeners
}

/// `emitter.removeListener(event, listener)`. Removes the first
/// matching listener only (matches Node).
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_remove_listener(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
) -> Handle {
    let Some(event_name) = read_str(event_name_ptr) else {
        return handle;
    };
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        let mut removed = false;
        if let Some(listeners) = emitter.events.get_mut(&event_name) {
            if let Some(pos) = listeners.iter().position(|l| l.callback == callback_ptr) {
                listeners.remove(pos);
                removed = true;
            }
        }
        if removed {
            emitter.prune_event_if_empty(&event_name);
        }
    }
    handle
}

/// `emitter.removeAllListeners()` (or `(event)` to scope by event).
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_remove_all_listeners(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> Handle {
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        if event_name_ptr.is_null() {
            emitter.events.clear();
            emitter.event_order.clear();
        } else if let Some(event_name) = read_str(event_name_ptr) {
            emitter.events.remove(&event_name);
            if let Some(pos) = emitter.event_order.iter().position(|s| s == &event_name) {
                emitter.event_order.remove(pos);
            }
        }
    }
    handle
}

/// `emitter.listenerCount(eventName)`.
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_listener_count(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> f64 {
    let Some(event_name) = read_str(event_name_ptr) else {
        return 0.0;
    };
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        if let Some(listeners) = emitter.events.get(&event_name) {
            return listeners.len() as f64;
        }
    }
    0.0
}

/// `emitter.setMaxListeners(n)`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_set_max_listeners(handle: Handle, n: f64) -> Handle {
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        emitter.max_listeners = n as i32;
    }
    handle
}

/// `emitter.getMaxListeners()`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_get_max_listeners(handle: Handle) -> f64 {
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        return emitter.max_listeners as f64;
    }
    10.0
}

/// `emitter.eventNames()` — returns an array of strings in insertion
/// order (matches Node).
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_event_names(handle: Handle) -> *mut ArrayHeader {
    let arr = js_array_alloc(0);
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        let mut result = arr;
        for name in emitter.event_order.iter() {
            let alive = emitter
                .events
                .get(name)
                .map(|v| !v.is_empty())
                .unwrap_or(false);
            if !alive {
                continue;
            }
            let s = perry_ffi::alloc_string(name);
            let bits = nanbox_string_bits(s.as_raw());
            result = js_array_push(result, JsValue::from_bits(bits));
        }
        return result;
    }
    arr
}

/// `emitter.listeners(eventName)` — returns an array of the registered
/// listener closures (NaN-boxed POINTER_TAG).
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_listeners(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> *mut ArrayHeader {
    let arr = js_array_alloc(0);
    let Some(event_name) = read_str(event_name_ptr) else {
        return arr;
    };
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        if let Some(listeners) = emitter.events.get(&event_name) {
            let mut result = arr;
            for l in listeners.iter() {
                if l.callback != 0 {
                    let v = JsValue::from_object_ptr(l.callback as *mut u8);
                    result = js_array_push(result, v);
                }
            }
            return result;
        }
    }
    arr
}

/// `emitter.rawListeners(eventName)` — identical to `listeners` in our
/// model (we don't wrap once-listeners at registration time).
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_raw_listeners(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> *mut ArrayHeader {
    js_event_emitter_listeners(handle, event_name_ptr)
}

// ============================================================================
// Module-level helpers — `events.once(em, name)` / `events.on(em, name)` /
// `events.getEventListeners(em, name)` / `events.listenerCount(em, name)` /
// `events.setMaxListeners(n, em)` / `events.getMaxListeners(em)`.
// ============================================================================

/// `events.once(emitter, eventName)` — returns a Promise that resolves
/// to a 1-element array `[arg]` on the next `emit(eventName, arg)`.
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_events_once(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> *mut Promise {
    ensure_gc_scanner_registered();
    let prom = JsPromise::new();
    let raw = prom.as_raw();
    let Some(event_name) = read_str(event_name_ptr) else {
        return raw;
    };
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        emitter
            .pending_once_promises
            .entry(event_name)
            .or_default()
            .push(raw);
    }
    raw
}

/// `events.getEventListeners(emitter, eventName)` — alias for
/// `emitter.listeners(eventName)`.
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_events_get_event_listeners(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> *mut ArrayHeader {
    js_event_emitter_listeners(handle, event_name_ptr)
}

/// `events.listenerCount(emitter, eventName)` — alias.
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_events_listener_count(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> f64 {
    js_event_emitter_listener_count(handle, event_name_ptr)
}

/// `events.getMaxListeners(emitter)` — alias.
#[no_mangle]
pub unsafe extern "C" fn js_events_get_max_listeners(handle: Handle) -> f64 {
    js_event_emitter_get_max_listeners(handle)
}

/// `events.setMaxListeners(n, ...emitters)` — Perry FFI takes a single
/// emitter handle. Multi-target callers should emit N FFI calls; for
/// the single-emitter case below this is exactly the right shape.
#[no_mangle]
pub unsafe extern "C" fn js_events_set_max_listeners(n: f64, handle: Handle) -> f64 {
    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
        emitter.max_listeners = n as i32;
    }
    f64::from_bits(0x7FFC_0000_0000_0001)
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_ffi::alloc_string;

    #[test]
    fn new_emitter_starts_empty() {
        let h = js_event_emitter_new();
        let event_name = alloc_string("foo");
        let count = unsafe { js_event_emitter_listener_count(h, event_name.as_raw() as *const _) };
        assert_eq!(count, 0.0);
    }

    #[test]
    fn add_then_count_listeners() {
        let h = js_event_emitter_new();
        let event_name = alloc_string("change");
        // Non-zero sentinel — we never emit so the closures aren't invoked.
        let _ = unsafe { js_event_emitter_on(h, event_name.as_raw() as *const _, 0xDEADBEEF_i64) };
        let _ = unsafe { js_event_emitter_on(h, event_name.as_raw() as *const _, 0xCAFEBABE_i64) };
        let count = unsafe { js_event_emitter_listener_count(h, event_name.as_raw() as *const _) };
        assert_eq!(count, 2.0);
    }

    #[test]
    fn remove_listener_drops_one() {
        let h = js_event_emitter_new();
        let event_name = alloc_string("data");
        unsafe {
            js_event_emitter_on(h, event_name.as_raw() as *const _, 1);
            js_event_emitter_on(h, event_name.as_raw() as *const _, 2);
            js_event_emitter_remove_listener(h, event_name.as_raw() as *const _, 1);
        }
        let count = unsafe { js_event_emitter_listener_count(h, event_name.as_raw() as *const _) };
        assert_eq!(count, 1.0);
    }

    #[test]
    fn remove_all_clears() {
        let h = js_event_emitter_new();
        let event_name = alloc_string("x");
        unsafe {
            js_event_emitter_on(h, event_name.as_raw() as *const _, 1);
            js_event_emitter_on(h, event_name.as_raw() as *const _, 2);
            js_event_emitter_remove_all_listeners(h, std::ptr::null());
        }
        let count = unsafe { js_event_emitter_listener_count(h, event_name.as_raw() as *const _) };
        assert_eq!(count, 0.0);
    }

    #[test]
    fn prepend_listener_inserts_at_front() {
        let h = js_event_emitter_new();
        let event_name = alloc_string("ord");
        unsafe {
            js_event_emitter_on(h, event_name.as_raw() as *const _, 100);
            js_event_emitter_prepend_listener(h, event_name.as_raw() as *const _, 99);
        }
        let arr = unsafe { js_event_emitter_listeners(h, event_name.as_raw() as *const _) };
        assert!(!arr.is_null());
    }

    #[test]
    fn max_listeners_round_trips() {
        let h = js_event_emitter_new();
        // Default = 10.
        assert_eq!(unsafe { js_event_emitter_get_max_listeners(h) }, 10.0);
        unsafe {
            js_event_emitter_set_max_listeners(h, 42.0);
        }
        assert_eq!(unsafe { js_event_emitter_get_max_listeners(h) }, 42.0);
    }
}
