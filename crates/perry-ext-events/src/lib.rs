//! Native bindings for Node's `events` module — `EventEmitter`
//! with the Node-compatible listener-table surface.
//!
//! First wrapper port that exercises perry-ffi's GC-root-scanner
//! surface (added in v0.5.546). User closures passed to
//! `emitter.on(event, cb)` live inside an `EventEmitterHandle`
//! value in the registry; without an explicit mutable GC scanner, a
//! malloc-triggered GC between `.on()` and `.emit()` would sweep the
//! closure or leave copied-minor forwarding pointers stale (issue #35
//! pattern).
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
    error_value_with_code, js_array_alloc, js_array_get, js_array_push, js_array_set,
    nanbox_string_bits, read_string, throw_with_code, ArrayHeader, ErrorKind, Handle, JsPromise,
    JsString, JsValue, ObjectHeader, Promise, RawClosureHeader, StringHeader,
};
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::sync::{Mutex, MutexGuard, Once, OnceLock};

mod error_monitor;
use error_monitor::dispatch_error_monitor;
mod max_listeners;
mod messages;
mod target_helpers;

#[cfg(test)]
mod test_async_shims;

use messages::{
    invalid_arg_type_error, invalid_instance_arg_message, invalid_instance_property_message,
    invalid_type_arg_message, throw_invalid_arg_type, throw_invalid_emitter,
};

use max_listeners::{emit_max_listeners_warning, validate_max_listeners};
use target_helpers::{
    event_helper_target, event_target_array_len, stream_array_len, EventHelperTarget,
};
mod module_iterators;
use module_iterators::{
    events_on_abort_listener, events_on_queue_listener, events_once_abort_listener,
    events_once_event_target_listener, events_once_stream_reject_listener,
    events_once_stream_resolve_listener,
};

const MIN_HEAP_POINTER: u64 = 0x1000;
const EVENT_TARGET_MIN_HEAP_POINTER: u64 = 0x10000;
const MAX_HEAP_POINTER: u64 = 0x0000_FFFF_FFFF_FFFF;
const ABORT_SIGNAL_CLASS_ID: u32 = 0xFFFF_2402;

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
    fn perry_ffi_gc_register_mutable_root_scanner_named(
        source_ptr: *const u8,
        source_len: usize,
        scanner_id: usize,
        scanner: PerryFfiNamedMutableRootScanner,
    );
    fn js_promise_resolve(promise: *mut Promise, value: f64);
    fn js_promise_reject(promise: *mut Promise, reason: f64);
    // events.once allocates its Promise via perry-ffi's `JsPromise::new()`
    // (→ `perry_ffi_promise_new` → a registered native-async token that pins
    // the Promise and reports the event loop as "busy"). Because we settle
    // synchronously through `js_promise_resolve`/`js_promise_reject` above —
    // bypassing the deferred completion machinery that would normally retire
    // the token — the token leaks and `js_native_async_has_active()` keeps the
    // loop alive forever (the `events.once(em, name)` + `emit` hang). Drop the
    // orphaned token right after each synchronous settle.
    fn js_native_async_drop_promise_token(promise: *mut Promise);
    fn js_promise_then(
        promise: *mut Promise,
        on_fulfilled: *mut RawClosureHeader,
        on_rejected: *mut RawClosureHeader,
    ) -> *mut Promise;
    // #1557: closure allocation hooks needed by events.on's queue-listener.
    // Mirrors perry-runtime::closure exports; declared here because perry-ffi
    // doesn't yet expose them and perry-ext-events deliberately avoids a
    // direct perry-runtime dep.
    fn js_closure_alloc(fn_ptr: *const u8, capture_count: u32) -> *mut RawClosureHeader;
    fn js_closure_set_capture_ptr(closure: *mut RawClosureHeader, slot: u32, ptr: i64);
    fn js_closure_get_capture_ptr(closure: *const RawClosureHeader, slot: u32) -> i64;
    fn js_array_push_f64(arr: *mut ArrayHeader, value: f64) -> *mut ArrayHeader;
    // #1557: AbortSignal listener attachment for events.addAbortListener.
    fn js_string_from_bytes(data: *const u8, len: u32) -> *mut StringHeader;
    fn js_object_alloc(class_id: u32, field_count: u32) -> *mut ObjectHeader;
    fn js_error_new_with_message(message: *mut StringHeader) -> *mut ObjectHeader;
    fn js_object_get_field_by_name_f64(obj: *const ObjectHeader, key: *const StringHeader) -> f64;
    fn js_object_set_field_by_name(obj: *mut ObjectHeader, key: *const StringHeader, value: f64);
    fn js_symbol_for(key_f64: f64) -> f64;
    fn js_object_set_symbol_property(obj_f64: f64, sym_f64: f64, value_f64: f64) -> f64;
    fn js_get_global_this() -> f64;
    fn js_array_is_array(value: f64) -> f64;
    fn js_abort_signal_add_listener(signal: *mut u8, event: f64, listener: f64);
    // AbortSignal recognition for the module-level events helpers — Node
    // treats a signal as an EventTarget. `resolve_ptr` returns null for
    // non-signals.
    fn js_abort_signal_resolve_ptr(value: f64) -> *mut u8;
    fn js_abort_signal_listener_count(signal: *mut u8) -> f64;
    fn js_abort_signal_listeners_copy(signal: *mut u8) -> *mut ArrayHeader;
    fn js_event_target_is_event_target(target: *const u8) -> i32;
    fn js_event_target_add_event_listener(
        target: *mut u8,
        event: *const StringHeader,
        listener: i64,
    );
    fn js_event_target_remove_event_listener(
        target: *mut u8,
        event: *const StringHeader,
        listener: i64,
    );
    fn js_event_target_get_event_listeners(
        target: *mut u8,
        event: *const StringHeader,
    ) -> *mut ArrayHeader;
    fn js_event_target_get_max_listeners(target: *mut u8) -> f64;
    fn js_event_target_set_max_listeners(target: *mut u8, n: f64) -> i32;
    fn js_node_stream_method_listeners(stream_handle: i64, event: f64) -> i64;
    fn js_node_stream_method_get_max_listeners(stream_handle: i64) -> f64;
    fn js_node_stream_method_set_max_listeners(stream_handle: i64, value: f64) -> f64;
    fn js_node_stream_method_on(stream_handle: i64, event: f64, cb: f64) -> f64;
    fn js_node_stream_method_once(stream_handle: i64, event: f64, cb: f64) -> f64;
    fn js_node_stream_method_remove_listener(stream_handle: i64, event: f64, cb: f64) -> f64;
    fn js_node_stream_is_readable(stream: f64) -> f64;
    fn js_node_stream_is_writable(stream: f64) -> f64;
    fn js_abort_signal_remove_listener(signal: *mut u8, event: f64, listener: f64);
    fn js_abort_signal_is_aborted(signal: *mut u8) -> i32;
    fn js_abort_error_value() -> f64;
    fn js_throw(value: f64) -> !;
    fn js_domain_emit_error(handle: Handle, error: f64, emitter: f64, domain_thrown: bool) -> bool;
    fn js_implicit_this_set(value: f64) -> f64;
    fn js_jsvalue_to_string(value: f64) -> *mut StringHeader;
    fn js_native_call_value(func_value: f64, args_ptr: *const f64, args_len: usize) -> f64;
    fn js_value_is_promise(value: f64) -> i32;
    fn js_register_event_emitter_handle_probe(f: unsafe extern "C" fn(i64) -> bool);
    fn js_register_event_emitter_get_domain(f: unsafe extern "C" fn(i64) -> i64);
    fn js_register_event_emitter_set_domain(f: unsafe extern "C" fn(i64, i64) -> i32);
    fn js_register_event_emitter_on(f: unsafe extern "C" fn(i64, i64, i64) -> i64);
    // #4995: serve dynamic `new` on the bound `events.EventEmitter` export
    // value (`require('events')`, default import, namespace property read)
    // so the runtime's `js_dynamic_new` constructs a real emitter instead of
    // falling through to the generic empty-object path.
    fn js_set_native_events_construct(
        f: unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64,
    );
    // #3072: shared listener validator. Takes the raw NaN-box bits of the
    // listener arg (codegen routes these methods through NA_JSV) and the arg
    // name; returns the closure pointer when callable, else throws
    // TypeError [ERR_INVALID_ARG_TYPE]. Centralized in perry-runtime so the
    // stdlib and ext-events EventEmitter implementations stay byte-identical.
    fn js_validate_event_listener(listener_bits: i64, name_ptr: *const u8, name_len: u32) -> i64;
    fn js_register_closure_rest(fn_ptr: *const u8, fixed_arity: u32);
}

/// #3072: validate an EventEmitter listener argument, returning the closure
/// pointer or throwing `TypeError [ERR_INVALID_ARG_TYPE]`.
#[inline]
unsafe fn validate_event_listener(listener_bits: i64) -> i64 {
    const NAME: &[u8] = b"listener";
    js_validate_event_listener(listener_bits, NAME.as_ptr(), NAME.len() as u32)
}

const TAG_UNDEFINED_F64_BITS: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL_F64_BITS: u64 = 0x7FFC_0000_0000_0002;
const TAG_TRUE_F64_BITS: u64 = 0x7FFC_0000_0000_0004;
const BIGINT_TAG: u64 = 0x7FFA_0000_0000_0000;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const INT32_TAG: u64 = 0x7FFE_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
const EVENT_EMITTER_HANDLE_ID_START: Handle = 0x38000;
const EVENT_EMITTER_HANDLE_ID_END: Handle = 0x40000;
const FFI_ROOT_SLOT_I64: u32 = 1;
const FFI_ROOT_SLOT_RAW_MUT_PTR: u32 = 3;
const FFI_ROOT_SLOT_NANBOX_F64: u32 = 4;

type PerryFfiMutableRootVisitor =
    extern "C" fn(kind: u32, slot: *mut c_void, ctx: *mut c_void) -> bool;
type PerryFfiNamedMutableRootScanner =
    extern "C" fn(scanner_id: usize, visit: PerryFfiMutableRootVisitor, ctx: *mut c_void);

struct EventsRootVisitor {
    visit: PerryFfiMutableRootVisitor,
    ctx: *mut c_void,
}

impl EventsRootVisitor {
    fn visit_i64_slot(&mut self, slot: &mut i64) -> bool {
        (self.visit)(FFI_ROOT_SLOT_I64, slot as *mut i64 as *mut c_void, self.ctx)
    }

    fn visit_raw_mut_ptr_slot<T>(&mut self, slot: &mut *mut T) -> bool {
        (self.visit)(
            FFI_ROOT_SLOT_RAW_MUT_PTR,
            slot as *mut *mut T as *mut c_void,
            self.ctx,
        )
    }

    fn visit_nanbox_f64_slot(&mut self, slot: &mut f64) -> bool {
        (self.visit)(
            FFI_ROOT_SLOT_NANBOX_F64,
            slot as *mut f64 as *mut c_void,
            self.ctx,
        )
    }
}

#[inline]
fn nanbox_pointer_bits(ptr: i64) -> f64 {
    f64::from_bits(POINTER_TAG | ((ptr as u64) & POINTER_MASK))
}

/// One registered listener: the original raw closure pointer plus an optional
/// raw once-wrapper closure exposed by `rawListeners()`.
#[derive(Copy, Clone)]
struct Listener {
    callback: i64,
    raw_wrapper: i64,
    once: bool,
}

#[derive(Copy, Clone)]
struct PendingOnce {
    promise: *mut Promise,
    signal: f64,
    abort_listener: i64,
}

/// EventEmitter handle with Node-compatible listener-table semantics
/// (issue #850).
pub struct EventEmitterHandle {
    events: HashMap<String, Vec<Listener>>,
    event_order: Vec<String>,
    pending_once_promises: HashMap<String, Vec<PendingOnce>>,
    warned_events: HashSet<String>,
    max_listeners: f64,
    capture_rejections: bool,
    domain_handle: Option<Handle>,
}

// SAFETY: `*mut Promise` is not Send/Sync by default, but the registry's
// mutable GC scanner visits pending promise slots so they survive minor
// GC cycles and are rewritten after copied-minor evacuation.
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
            warned_events: HashSet::new(),
            // Node's default — `getMaxListeners()` on a fresh emitter
            // returns 10.
            max_listeners: 10.0,
            capture_rejections: false,
            domain_handle: None,
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
            self.warned_events.remove(name);
            if let Some(pos) = self.event_order.iter().position(|s| s == name) {
                self.event_order.remove(pos);
            }
        }
    }

    fn emit_meta_event(
        &self,
        handle: Handle,
        meta_name: &str,
        event_name: &str,
        listener_arg: i64,
    ) {
        let snapshot = match self.events.get(meta_name) {
            Some(v) if !v.is_empty() => v.clone(),
            _ => return,
        };
        let event_ptr =
            unsafe { js_string_from_bytes(event_name.as_ptr(), event_name.len() as u32) };
        let event_arg = f64::from_bits(nanbox_string_bits(event_ptr));
        let listener_arg = nanbox_pointer_bits(listener_arg);
        let args = [event_arg, listener_arg];
        for listener in snapshot {
            if listener.callback != 0 {
                unsafe {
                    call_emitter_listener(handle, listener.callback, &args);
                }
            }
        }
    }

    fn add_listener(
        &mut self,
        handle: Handle,
        name: &str,
        callback: i64,
        once: bool,
        prepend: bool,
    ) {
        self.emit_meta_event(handle, "newListener", name, callback);
        self.note_event(name);
        let raw_wrapper = if once {
            unsafe { create_once_raw_wrapper(handle, name, callback) }
        } else {
            0
        };
        let count = {
            let vec = self.events.entry(name.to_string()).or_default();
            let listener = Listener {
                callback,
                raw_wrapper,
                once,
            };
            if prepend {
                vec.insert(0, listener);
            } else {
                vec.push(listener);
            }
            vec.len()
        };
        if self.max_listeners > 0.0
            && (count as f64) > self.max_listeners
            && self.warned_events.insert(name.to_string())
        {
            unsafe { emit_max_listeners_warning(handle, name, count, self.max_listeners) };
        }
    }
}

type EventEmitterRegistry = Vec<Option<Box<EventEmitterHandle>>>;

static EVENT_EMITTERS: OnceLock<Mutex<EventEmitterRegistry>> = OnceLock::new();
static EVENTS_RUNTIME_HOOKS_REGISTERED: Once = Once::new();

thread_local! {
    static EVENTS_GC_REGISTERED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn event_emitters() -> &'static Mutex<EventEmitterRegistry> {
    EVENT_EMITTERS.get_or_init(|| Mutex::new(Vec::new()))
}

fn lock_event_emitters() -> MutexGuard<'static, EventEmitterRegistry> {
    event_emitters()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn handle_index(handle: Handle) -> Option<usize> {
    if !(EVENT_EMITTER_HANDLE_ID_START..EVENT_EMITTER_HANDLE_ID_END).contains(&handle) {
        return None;
    }
    Some((handle - EVENT_EMITTER_HANDLE_ID_START) as usize)
}

fn register_event_emitter_handle(value: EventEmitterHandle) -> Handle {
    let mut registry = lock_event_emitters();
    if let Some((idx, slot)) = registry
        .iter_mut()
        .enumerate()
        .find(|(_, slot)| slot.is_none())
    {
        *slot = Some(Box::new(value));
        return EVENT_EMITTER_HANDLE_ID_START + idx as Handle;
    }
    let handle = EVENT_EMITTER_HANDLE_ID_START + registry.len() as Handle;
    if handle >= EVENT_EMITTER_HANDLE_ID_END {
        panic!("perry-ext-events handle id range exhausted");
    }
    registry.push(Some(Box::new(value)));
    handle
}

fn event_emitter_ptr(handle: Handle) -> Option<*mut EventEmitterHandle> {
    let idx = handle_index(handle)?;
    let mut registry = lock_event_emitters();
    let slot = registry.get_mut(idx)?.as_mut()?;
    Some(&mut **slot as *mut EventEmitterHandle)
}

fn get_event_emitter_mut(handle: Handle) -> Option<&'static mut EventEmitterHandle> {
    let ptr = event_emitter_ptr(handle)?;
    Some(unsafe { &mut *ptr })
}

fn is_local_event_emitter_handle(handle: Handle) -> bool {
    let Some(idx) = handle_index(handle) else {
        return false;
    };
    let registry = lock_event_emitters();
    registry.get(idx).is_some_and(|slot| slot.is_some())
}

#[cfg(test)]
fn drop_event_emitter_handle(handle: Handle) -> bool {
    let Some(idx) = handle_index(handle) else {
        return false;
    };
    let mut registry = lock_event_emitters();
    let Some(slot) = registry.get_mut(idx) else {
        return false;
    };
    slot.take().is_some()
}

unsafe extern "C" fn event_emitter_handle_probe(handle: i64) -> bool {
    is_local_event_emitter_handle(handle)
}

unsafe extern "C" fn event_emitter_on_hook(
    handle: i64,
    event_bits: i64,
    listener_bits: i64,
) -> i64 {
    js_event_emitter_on(handle, event_bits, listener_bits)
}

/// #4995: construct `new events.EventEmitter(...)` reached through a
/// value-aliasing path (`require('events')`, default import, namespace
/// property read). Registered with the runtime's `js_dynamic_new` so the
/// instance is a real emitter handle instead of an empty object.
unsafe extern "C" fn events_native_construct(
    class_name_ptr: *const u8,
    class_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let class_name = std::slice::from_raw_parts(class_name_ptr, class_name_len);
    if class_name != b"EventEmitter" {
        return f64::from_bits(TAG_UNDEFINED_F64_BITS);
    }
    let options = if !args_ptr.is_null() && args_len > 0 {
        *args_ptr
    } else {
        f64::from_bits(TAG_UNDEFINED_F64_BITS)
    };
    nanbox_pointer_bits(js_event_emitter_new_with_options(options))
}

fn ensure_runtime_hooks_registered() {
    EVENTS_RUNTIME_HOOKS_REGISTERED.call_once(|| unsafe {
        js_register_event_emitter_handle_probe(event_emitter_handle_probe);
        js_register_event_emitter_get_domain(js_event_emitter_get_domain);
        js_register_event_emitter_set_domain(js_event_emitter_set_domain);
        js_register_event_emitter_on(event_emitter_on_hook);
        js_set_native_events_construct(events_native_construct);
    });
}

fn ensure_gc_scanner_registered() {
    EVENTS_GC_REGISTERED.with(|registered| {
        if registered.get() {
            return;
        }
        unsafe {
            const SOURCE: &str = "perry-ext-events";
            perry_ffi_gc_register_mutable_root_scanner_named(
                SOURCE.as_ptr(),
                SOURCE.len(),
                0,
                scan_events_roots_trampoline,
            );
        }
        registered.set(true);
    });
}

extern "C" fn scan_events_roots_trampoline(
    _scanner_id: usize,
    visit: PerryFfiMutableRootVisitor,
    ctx: *mut c_void,
) {
    let mut visitor = EventsRootVisitor { visit, ctx };
    scan_events_roots(&mut visitor);
}

/// GC root scanner: visit every registered EventEmitterHandle,
/// and expose every listener closure pointer + pending Promise slot.
fn scan_events_roots(visitor: &mut EventsRootVisitor) {
    let mut registry = lock_event_emitters();
    for emitter in registry.iter_mut().filter_map(|slot| slot.as_deref_mut()) {
        for listeners in emitter.events.values_mut() {
            for l in listeners.iter_mut() {
                if is_heap_pointer_candidate(l.callback) {
                    visitor.visit_i64_slot(&mut l.callback);
                }
                if is_heap_pointer_candidate(l.raw_wrapper) {
                    visitor.visit_i64_slot(&mut l.raw_wrapper);
                }
            }
        }
        for pending in emitter.pending_once_promises.values_mut() {
            for p in pending.iter_mut() {
                visitor.visit_raw_mut_ptr_slot(&mut p.promise);
                visitor.visit_nanbox_f64_slot(&mut p.signal);
                if is_heap_pointer_candidate(p.abort_listener) {
                    visitor.visit_i64_slot(&mut p.abort_listener);
                }
            }
        }
    }
}

fn is_heap_pointer_candidate(callback: i64) -> bool {
    if callback <= 0 {
        return false;
    }
    let addr = callback as u64;
    (MIN_HEAP_POINTER..=MAX_HEAP_POINTER).contains(&addr) && addr & 0x7 == 0
}

unsafe fn event_target_ptr(handle: Handle) -> Option<*mut u8> {
    let addr = handle as u64;
    if !(EVENT_TARGET_MIN_HEAP_POINTER..=MAX_HEAP_POINTER).contains(&addr) || addr & 0x7 != 0 {
        return None;
    }
    let ptr = handle as *mut u8;
    if js_event_target_is_event_target(ptr as *const u8) != 0 {
        Some(ptr)
    } else {
        None
    }
}

unsafe fn stream_listeners_for_heap_object(
    handle: Handle,
    event_name_ptr: *const StringHeader,
) -> Option<*mut ArrayHeader> {
    let event_name_ptr = string_header_ptr_from_arg(event_name_ptr);
    let addr = handle as u64;
    if event_name_ptr.is_null()
        || !(EVENT_TARGET_MIN_HEAP_POINTER..=MAX_HEAP_POINTER).contains(&addr)
        || addr & 0x7 != 0
    {
        return None;
    }
    let event = f64::from_bits(nanbox_string_bits(event_name_ptr as *mut StringHeader));
    Some(js_node_stream_method_listeners(handle, event) as *mut ArrayHeader)
}

unsafe fn stream_value_from_handle(handle: Handle) -> Option<f64> {
    let addr = handle as u64;
    if !(EVENT_TARGET_MIN_HEAP_POINTER..=MAX_HEAP_POINTER).contains(&addr) || addr & 0x7 != 0 {
        return None;
    }
    let value = nanbox_pointer_bits(handle);
    let readable = js_node_stream_is_readable(value);
    let writable = js_node_stream_is_writable(value);
    if readable.to_bits() == TAG_NULL_F64_BITS && writable.to_bits() == TAG_NULL_F64_BITS {
        None
    } else {
        Some(value)
    }
}

fn handle_from_value(value: f64) -> Handle {
    let bits = value.to_bits();
    if (bits & 0xFFFF_0000_0000_0000) == POINTER_TAG {
        (bits & POINTER_MASK) as Handle
    } else if value.is_finite() && value > 0.0 && value.fract() == 0.0 {
        value as Handle
    } else {
        bits as Handle
    }
}

fn string_header_ptr_from_arg(ptr: *const StringHeader) -> *const StringHeader {
    let raw = ptr as u64;
    if (raw & 0xFFFF_0000_0000_0000) == STRING_TAG {
        (raw & POINTER_MASK) as *const StringHeader
    } else {
        ptr
    }
}

unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    let ptr = string_header_ptr_from_arg(ptr);
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        return None;
    }
    let handle = JsString::from_raw(ptr as *mut StringHeader);
    read_string(handle).map(String::from)
}

unsafe fn event_name_from_bits(event_bits: i64) -> Option<String> {
    let raw = event_bits as u64;
    if (0x10000..MAX_HEAP_POINTER).contains(&raw) && (raw & TAG_MASK) == 0 {
        return string_from_header(raw as *const StringHeader);
    }

    let rendered = js_jsvalue_to_string(f64::from_bits(raw));
    string_from_header(rendered as *const StringHeader)
}

fn event_bits_from_string_ptr(ptr: *const StringHeader) -> i64 {
    f64::from_bits(nanbox_string_bits(ptr as *mut StringHeader)).to_bits() as i64
}

fn undefined_value() -> f64 {
    f64::from_bits(TAG_UNDEFINED_F64_BITS)
}

fn object_ptr_from_value(value: f64) -> Option<*mut ObjectHeader> {
    let jsval = JsValue::from_bits(value.to_bits());
    if jsval.is_undefined() || jsval.is_null() || !jsval.is_pointer() {
        return None;
    }
    let ptr = (value.to_bits() & POINTER_MASK) as *mut ObjectHeader;
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        None
    } else {
        Some(ptr)
    }
}

unsafe fn is_abort_signal_value(value: f64) -> bool {
    let Some(ptr) = object_ptr_from_value(value) else {
        return false;
    };
    (*ptr).class_id == ABORT_SIGNAL_CLASS_ID
}

unsafe fn validate_abort_signal_arg(value: f64, name: &str) -> f64 {
    if is_abort_signal_value(value) {
        return value;
    }
    throw_invalid_arg_type(&invalid_instance_arg_message(name, "AbortSignal", value))
}

unsafe fn get_object_property(value: f64, name: &[u8]) -> Option<f64> {
    let obj = object_ptr_from_value(value)?;
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, key);
    if JsValue::from_bits(value.to_bits()).is_undefined() {
        None
    } else {
        Some(value)
    }
}

unsafe fn options_signal_result(options: f64) -> Result<Option<f64>, f64> {
    let jsval = JsValue::from_bits(options.to_bits());
    if jsval.is_undefined() {
        return Ok(None);
    }
    if object_ptr_from_value(options).is_none() {
        return Err(invalid_arg_type_error(&invalid_type_arg_message(
            "options", "object", options,
        )));
    }
    let Some(signal) = get_object_property(options, b"signal") else {
        return Ok(None);
    };
    if is_abort_signal_value(signal) {
        Ok(Some(signal))
    } else {
        Err(invalid_arg_type_error(&invalid_instance_property_message(
            "options.signal",
            "AbortSignal",
            signal,
        )))
    }
}

unsafe fn options_signal_or_throw(options: f64) -> Option<f64> {
    match options_signal_result(options) {
        Ok(signal) => signal,
        Err(error) => js_throw(error),
    }
}

unsafe fn options_capture_rejections(options: f64) -> bool {
    let jsval = JsValue::from_bits(options.to_bits());
    if jsval.is_undefined() || jsval.is_null() {
        return false;
    }
    get_object_property(options, b"captureRejections")
        .map(|value| {
            let jsval = JsValue::from_bits(value.to_bits());
            jsval.is_bool() && jsval.to_bool()
        })
        .unwrap_or(false)
}

fn signal_is_aborted(signal: f64) -> bool {
    let Some(signal_ptr) = object_ptr_from_value(signal) else {
        return false;
    };
    unsafe { js_abort_signal_is_aborted(signal_ptr as *mut u8) != 0 }
}

unsafe fn abort_event_value() -> f64 {
    let event_name = b"abort";
    let event_str = js_string_from_bytes(event_name.as_ptr(), event_name.len() as u32);
    f64::from_bits(nanbox_string_bits(event_str))
}

unsafe fn cleanup_pending_abort_listener(pending: &PendingOnce) {
    if pending.abort_listener == 0 {
        return;
    }
    let Some(signal_ptr) = object_ptr_from_value(pending.signal) else {
        return;
    };
    js_abort_signal_remove_listener(
        signal_ptr as *mut u8,
        abort_event_value(),
        nanbox_pointer_bits(pending.abort_listener),
    );
}

fn remove_pending_once_promise(
    emitter: &mut EventEmitterHandle,
    promise: *mut Promise,
) -> Option<PendingOnce> {
    let event_names: Vec<String> = emitter.pending_once_promises.keys().cloned().collect();
    for event_name in event_names {
        let mut should_prune = false;
        let removed = emitter
            .pending_once_promises
            .get_mut(&event_name)
            .and_then(|pending| {
                let pos = pending.iter().position(|p| p.promise == promise)?;
                let removed = pending.remove(pos);
                should_prune = pending.is_empty();
                Some(removed)
            });
        if should_prune {
            emitter.pending_once_promises.remove(&event_name);
        }
        if removed.is_some() {
            return removed;
        }
    }
    None
}

fn listener_matches(listener: &Listener, candidate: i64) -> bool {
    candidate != 0 && (listener.callback == candidate || listener.raw_wrapper == candidate)
}

fn remove_one_matching_listener(
    emitter: &mut EventEmitterHandle,
    event_name: &str,
    candidate: i64,
) -> Option<i64> {
    let removed = emitter.events.get_mut(event_name).and_then(|listeners| {
        let pos = listeners
            .iter()
            .rposition(|listener| listener_matches(listener, candidate))?;
        Some(listeners.remove(pos).callback)
    });
    if removed.is_some() {
        emitter.prune_event_if_empty(event_name);
    }
    removed
}

fn remove_listener_by_callback(emitter: &mut EventEmitterHandle, callback: i64) {
    if callback == 0 {
        return;
    }
    let event_names: Vec<String> = emitter.events.keys().cloned().collect();
    for event_name in event_names {
        let removed = if let Some(listeners) = emitter.events.get_mut(&event_name) {
            let before = listeners.len();
            listeners.retain(|listener| !listener_matches(listener, callback));
            before != listeners.len()
        } else {
            false
        };
        if removed {
            emitter.prune_event_if_empty(&event_name);
        }
    }
}

static RAW_ONCE_WRAPPER_REST_REGISTERED: Once = Once::new();

fn ensure_raw_once_wrapper_rest_registered() {
    RAW_ONCE_WRAPPER_REST_REGISTERED.call_once(|| unsafe {
        js_register_closure_rest(event_emitter_once_wrapper as *const u8, 0);
    });
}

unsafe fn set_closure_dynamic_prop(closure: *mut RawClosureHeader, name: &[u8], value: f64) {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(closure as *mut ObjectHeader, key, value);
}

unsafe fn create_once_raw_wrapper(handle: Handle, event_name: &str, callback: i64) -> i64 {
    if callback == 0 {
        return 0;
    }
    ensure_raw_once_wrapper_rest_registered();

    let wrapper = js_closure_alloc(event_emitter_once_wrapper as *const u8, 4);
    let event_ptr = js_string_from_bytes(event_name.as_ptr(), event_name.len() as u32);
    js_closure_set_capture_ptr(wrapper, 0, handle);
    js_closure_set_capture_ptr(wrapper, 1, event_ptr as i64);
    js_closure_set_capture_ptr(wrapper, 2, callback);
    js_closure_set_capture_ptr(wrapper, 3, wrapper as i64);

    set_closure_dynamic_prop(wrapper, b"listener", nanbox_pointer_bits(callback));
    let name_ptr = js_string_from_bytes(b"bound onceWrapper".as_ptr(), 17);
    set_closure_dynamic_prop(
        wrapper,
        b"name",
        f64::from_bits(nanbox_string_bits(name_ptr)),
    );

    wrapper as i64
}

extern "C" fn event_emitter_once_wrapper(closure: *const RawClosureHeader, rest: f64) -> f64 {
    unsafe {
        let handle = js_closure_get_capture_ptr(closure, 0) as Handle;
        let event_name_ptr = js_closure_get_capture_ptr(closure, 1) as *const StringHeader;
        let callback = js_closure_get_capture_ptr(closure, 2);
        let wrapper = js_closure_get_capture_ptr(closure, 3);
        if handle != 0 && callback != 0 {
            if let Some(event_name) = string_from_header(event_name_ptr) {
                let removed = get_event_emitter_mut(handle).and_then(|emitter| {
                    remove_one_matching_listener(emitter, &event_name, wrapper)
                });
                if removed.is_some() {
                    if let Some(emitter) = get_event_emitter_mut(handle) {
                        emitter.emit_meta_event(handle, "removeListener", &event_name, callback);
                    }
                }
            }
        }

        let args_ptr = if JsValue::from_bits(rest.to_bits()).is_pointer() {
            (rest.to_bits() & POINTER_MASK) as *const ArrayHeader
        } else {
            std::ptr::null()
        };
        let args = collect_emit_args(args_ptr);
        if callback == 0 {
            undefined_value()
        } else {
            call_emitter_listener(handle, callback, &args)
        }
    }
}

/// `new EventEmitter()` — returns a handle to the emitter.
#[no_mangle]
pub extern "C" fn js_event_emitter_new() -> Handle {
    ensure_runtime_hooks_registered();
    ensure_gc_scanner_registered();
    register_event_emitter_handle(EventEmitterHandle::new())
}

/// `new EventEmitter(options?)` — the constructor shape codegen actually
/// emits for `new EventEmitter()` (see `lower_call/builtin.rs`). The bundled
/// perry-stdlib EventEmitter already exposes this entry point, but the
/// default `node:events` flip routes to perry-ext-events (well_known_bindings
/// .toml), which previously only defined `js_event_emitter_new` — so any
/// `new EventEmitter(...)` failed to link with
/// `Undefined symbols: _js_event_emitter_new_with_options`. perry-ext-events
/// models Node's `captureRejections` option for listener promise rejections.
///
/// # Safety
///
/// `_options` is a NaN-boxed JS value; it is not dereferenced.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_new_with_options(_options: f64) -> Handle {
    ensure_runtime_hooks_registered();
    ensure_gc_scanner_registered();
    let mut emitter = EventEmitterHandle::new();
    emitter.capture_rejections = options_capture_rejections(_options);
    register_event_emitter_handle(emitter)
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
    event_bits: i64,
    listener_bits: i64,
) -> Handle {
    ensure_gc_scanner_registered();
    // #3072: reject non-function listeners with TypeError [ERR_INVALID_ARG_TYPE].
    let callback_ptr = validate_event_listener(listener_bits);
    let Some(event_name) = event_name_from_bits(event_bits) else {
        return handle;
    };
    if let Some(emitter) = get_event_emitter_mut(handle) {
        emitter.add_listener(handle, &event_name, callback_ptr, false, false);
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
    event_bits: i64,
    listener_bits: i64,
) -> Handle {
    ensure_gc_scanner_registered();
    let callback_ptr = validate_event_listener(listener_bits);
    let Some(event_name) = event_name_from_bits(event_bits) else {
        return handle;
    };
    if let Some(emitter) = get_event_emitter_mut(handle) {
        emitter.add_listener(handle, &event_name, callback_ptr, true, false);
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
    event_bits: i64,
    listener_bits: i64,
) -> Handle {
    ensure_gc_scanner_registered();
    let callback_ptr = validate_event_listener(listener_bits);
    let Some(event_name) = event_name_from_bits(event_bits) else {
        return handle;
    };
    if let Some(emitter) = get_event_emitter_mut(handle) {
        emitter.add_listener(handle, &event_name, callback_ptr, false, true);
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
    event_bits: i64,
    listener_bits: i64,
) -> Handle {
    ensure_gc_scanner_registered();
    let callback_ptr = validate_event_listener(listener_bits);
    let Some(event_name) = event_name_from_bits(event_bits) else {
        return handle;
    };
    if let Some(emitter) = get_event_emitter_mut(handle) {
        emitter.add_listener(handle, &event_name, callback_ptr, true, true);
    }
    handle
}

unsafe fn first_arg_or_undefined(args_ptr: *const ArrayHeader) -> f64 {
    if args_ptr.is_null() || (*args_ptr).length == 0 {
        return f64::from_bits(JsValue::UNDEFINED.bits());
    }
    f64::from_bits(js_array_get(args_ptr, 0).bits())
}

unsafe fn collect_emit_args(args_ptr: *const ArrayHeader) -> Vec<f64> {
    if args_ptr.is_null() {
        return Vec::new();
    }
    let len = (*args_ptr).length as usize;
    let mut args = Vec::with_capacity(len);
    for index in 0..len {
        args.push(f64::from_bits(js_array_get(args_ptr, index as u32).bits()));
    }
    args
}

unsafe fn call_emitter_listener(handle: Handle, callback: i64, args: &[f64]) -> f64 {
    let receiver = nanbox_pointer_bits(handle);
    let callback_value = nanbox_pointer_bits(callback);
    let previous_this = js_implicit_this_set(receiver);
    let result = if args.is_empty() {
        js_native_call_value(callback_value, std::ptr::null(), 0)
    } else {
        js_native_call_value(callback_value, args.as_ptr(), args.len())
    };
    js_implicit_this_set(previous_this);
    result
}

extern "C" fn events_capture_rejection_handler(
    closure: *const RawClosureHeader,
    reason: f64,
) -> f64 {
    unsafe {
        let handle = js_closure_get_capture_ptr(closure, 0) as Handle;
        if handle != 0 {
            let event_name = b"error";
            let event_str = js_string_from_bytes(event_name.as_ptr(), event_name.len() as u32);
            let mut args = js_array_alloc(0);
            args = js_array_push_f64(args, reason);
            js_event_emitter_emit(handle, event_bits_from_string_ptr(event_str), args);
        }
    }
    undefined_value()
}

unsafe fn capture_listener_rejection(handle: Handle, result: f64) {
    if js_value_is_promise(result) == 0 {
        return;
    }
    let promise = (result.to_bits() & POINTER_MASK) as *mut Promise;
    if promise.is_null() {
        return;
    }
    let on_rejected = js_closure_alloc(events_capture_rejection_handler as *const u8, 1);
    js_closure_set_capture_ptr(on_rejected, 0, handle);
    js_promise_then(promise, std::ptr::null_mut(), on_rejected);
}

/// Drain pending `events.once` promises for `event_name` on the given
/// emitter, resolving each with the full args array passed to `emit`.
///
/// Pre-#1186 we synthesized a fresh 1-element `[arg]` array because
/// `emit` only received a single `f64` arg. Post-#1186 the codegen
/// passes the full variadic args as `*mut ArrayHeader` (`NA_VARARGS`),
/// so we resolve each Promise with that array directly — matches
/// `perry-stdlib::events::drain_pending_once_promises` and Node's
/// `events.once` semantics where the resolution value is the full args
/// tuple.
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
    let boxed_arr = JsValue::from_object_ptr(arr);
    let bits = boxed_arr.bits();
    for pending in pending {
        cleanup_pending_abort_listener(&pending);
        if pending.promise.is_null() {
            continue;
        }
        // Synchronous resolve — see the comment on the extern at the
        // top of this file for why we bypass `JsPromise::resolve`.
        js_promise_resolve(pending.promise, f64::from_bits(bits));
        js_native_async_drop_promise_token(pending.promise);
    }
}

unsafe fn reject_pending_once_promises_for_error(
    emitter: &mut EventEmitterHandle,
    error_value: f64,
) -> bool {
    let event_names: Vec<String> = emitter
        .pending_once_promises
        .keys()
        .filter(|name| name.as_str() != "error")
        .cloned()
        .collect();
    let mut rejected_any = false;
    for event_name in event_names {
        let Some(pending) = emitter.pending_once_promises.remove(&event_name) else {
            continue;
        };
        for pending in pending {
            cleanup_pending_abort_listener(&pending);
            if !pending.promise.is_null() {
                js_promise_reject(pending.promise, error_value);
                js_native_async_drop_promise_token(pending.promise);
                rejected_any = true;
            }
        }
    }
    rejected_any
}

/// `emitter.emit(eventName, ...args)` — fire variadic `args` to every
/// listener (and to any pending `events.once` Promise for the event).
///
/// Signature must match `perry-stdlib::events::js_event_emitter_emit`
/// because `perry-codegen`'s native_table lowers `emit` calls via
/// `NA_VARARGS` (single `*mut ArrayHeader` ABI slot) and `NR_F64`
/// return. Pre-#1186 perry-ext-events still used the legacy
/// `(handle, name, arg: f64) -> bool` ABI; codegen then mis-passed the
/// args-array pointer as `arg: f64`, which the listener interpreted as
/// `NaN` because the high pointer bits set the NaN-box quiet tag —
/// see issue #1274.
///
/// Returns `TAG_TRUE` / `TAG_FALSE` (NaN-boxed booleans) so
/// `had_listeners` round-trips through codegen's f64 ABI.
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_emit(
    handle: Handle,
    event_bits: i64,
    args_ptr: *mut ArrayHeader,
) -> f64 {
    const TAG_FALSE_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0003);
    const TAG_TRUE_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0004);
    let Some(event_name) = event_name_from_bits(event_bits) else {
        return TAG_FALSE_F64;
    };
    let mut had_listeners = false;
    let mut domain_error: Option<(Handle, f64)> = None;
    let mut throw_error: Option<f64> = None;
    if let Some(emitter) = get_event_emitter_mut(handle) {
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

        let first_arg = first_arg_or_undefined(args_ptr);
        let emitted_args = collect_emit_args(args_ptr);
        if event_name == "error" {
            dispatch_error_monitor(emitter, handle, Some(first_arg));
            let has_error_once = emitter
                .pending_once_promises
                .get("error")
                .is_some_and(|pending| !pending.is_empty());
            let rejected_once = reject_pending_once_promises_for_error(emitter, first_arg);
            had_listeners = had_listeners || has_error_once || rejected_once;
            if snapshot.is_empty() && !has_error_once && !rejected_once {
                if let Some(domain) = emitter.domain_handle {
                    domain_error = Some((domain, first_arg));
                } else {
                    throw_error = Some(first_arg);
                }
            }
        }

        if domain_error.is_none() && throw_error.is_none() {
            drain_pending_once_promises(emitter, &event_name, args_ptr);

            let capture_rejections = emitter.capture_rejections && event_name != "error";
            for l in snapshot {
                if l.callback != 0 {
                    let result = call_emitter_listener(handle, l.callback, &emitted_args);
                    if capture_rejections {
                        capture_listener_rejection(handle, result);
                    }
                }
            }
        }
    }
    if let Some((domain, error)) = domain_error {
        let _ = js_domain_emit_error(domain, error, nanbox_pointer_bits(handle), false);
        return TAG_FALSE_F64;
    }
    if let Some(error) = throw_error {
        js_throw(error);
    }
    if had_listeners {
        TAG_TRUE_F64
    } else {
        TAG_FALSE_F64
    }
}

/// `emitter.emit(eventName)` — no-args variant.
///
/// Same return-type contract as `js_event_emitter_emit`: returns a
/// NaN-boxed boolean (`TAG_TRUE` / `TAG_FALSE`) so callers see the
/// same shape regardless of which path they hit.
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_emit0(handle: Handle, event_bits: i64) -> f64 {
    const TAG_FALSE_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0003);
    const TAG_TRUE_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0004);
    let Some(event_name) = event_name_from_bits(event_bits) else {
        return TAG_FALSE_F64;
    };
    let mut had_listeners = false;
    let mut domain_error: Option<(Handle, f64)> = None;
    let mut throw_error: Option<f64> = None;
    if let Some(emitter) = get_event_emitter_mut(handle) {
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
        if event_name == "error" {
            let error_value = undefined_value();
            dispatch_error_monitor(emitter, handle, None);
            let has_error_once = emitter
                .pending_once_promises
                .get("error")
                .is_some_and(|pending| !pending.is_empty());
            let rejected_once = reject_pending_once_promises_for_error(emitter, error_value);
            had_listeners = had_listeners || has_error_once || rejected_once;
            if snapshot.is_empty() && !has_error_once && !rejected_once {
                if let Some(domain) = emitter.domain_handle {
                    domain_error = Some((domain, error_value));
                } else {
                    throw_error = Some(error_value);
                }
            }
        }
        if domain_error.is_none() && throw_error.is_none() {
            drain_pending_once_promises(emitter, &event_name, empty_args);

            let capture_rejections = emitter.capture_rejections && event_name != "error";
            for l in snapshot {
                if l.callback != 0 {
                    let result = call_emitter_listener(handle, l.callback, &[]);
                    if capture_rejections {
                        capture_listener_rejection(handle, result);
                    }
                }
            }
        }
    }
    if let Some((domain, error)) = domain_error {
        let _ = js_domain_emit_error(domain, error, nanbox_pointer_bits(handle), false);
        return TAG_FALSE_F64;
    }
    if let Some(error) = throw_error {
        js_throw(error);
    }
    if had_listeners {
        TAG_TRUE_F64
    } else {
        TAG_FALSE_F64
    }
}

/// `emitter.removeListener(event, listener)`. Removes the most recently added
/// matching listener, including once raw-wrapper aliases.
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_remove_listener(
    handle: Handle,
    event_bits: i64,
    listener_bits: i64,
) -> Handle {
    // #3072: `removeListener`/`off` require a callable listener too.
    let callback_ptr = validate_event_listener(listener_bits);
    let Some(event_name) = event_name_from_bits(event_bits) else {
        return handle;
    };
    if let Some(emitter) = get_event_emitter_mut(handle) {
        if let Some(removed_callback) =
            remove_one_matching_listener(emitter, &event_name, callback_ptr)
        {
            emitter.emit_meta_event(handle, "removeListener", &event_name, removed_callback);
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
    args_ptr: *const ArrayHeader,
) -> Handle {
    if let Some(emitter) = get_event_emitter_mut(handle) {
        if args_ptr.is_null() || (*args_ptr).length == 0 {
            let removed: Vec<(String, i64)> = emitter
                .event_order
                .iter()
                .filter(|name| name.as_str() != "removeListener")
                .flat_map(|name| {
                    emitter.events.get(name).into_iter().flat_map(|listeners| {
                        listeners
                            .iter()
                            .map(|listener| (name.clone(), listener.callback))
                    })
                })
                .collect();
            emitter.events.clear();
            emitter.event_order.clear();
            emitter.warned_events.clear();
            for (name, callback) in removed {
                emitter.emit_meta_event(handle, "removeListener", &name, callback);
            }
        } else if let Some(event_name) =
            event_name_from_bits(js_array_get(args_ptr, 0).bits() as i64)
        {
            let removed: Vec<i64> = emitter
                .events
                .get(&event_name)
                .map(|listeners| listeners.iter().map(|listener| listener.callback).collect())
                .unwrap_or_default();
            emitter.events.remove(&event_name);
            emitter.warned_events.remove(&event_name);
            if let Some(pos) = emitter.event_order.iter().position(|s| s == &event_name) {
                emitter.event_order.remove(pos);
            }
            if event_name != "removeListener" {
                for callback in removed {
                    emitter.emit_meta_event(handle, "removeListener", &event_name, callback);
                }
            }
        }
    }
    handle
}

fn listener_filter_from_bits(listener_bits: i64) -> Option<i64> {
    let value = JsValue::from_bits(listener_bits as u64);
    if value.is_undefined() || value.is_null() {
        None
    } else if value.is_pointer() {
        Some((listener_bits as u64 & POINTER_MASK) as i64)
    } else {
        Some(0)
    }
}

/// `emitter.listenerCount(eventName, listener?)`.
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_listener_count(
    handle: Handle,
    event_bits: i64,
    listener_bits: i64,
) -> f64 {
    let Some(event_name) = event_name_from_bits(event_bits) else {
        return 0.0;
    };
    let listener_filter = listener_filter_from_bits(listener_bits);
    if let Some(emitter) = get_event_emitter_mut(handle) {
        if let Some(listeners) = emitter.events.get(&event_name) {
            if let Some(callback_ptr) = listener_filter {
                if callback_ptr == 0 {
                    return 0.0;
                }
                return listeners
                    .iter()
                    .filter(|listener| listener_matches(listener, callback_ptr))
                    .count() as f64;
            }
            return listeners.len() as f64;
        }
    }
    0.0
}

/// `emitter.setMaxListeners(n)`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_set_max_listeners(handle: Handle, n: f64) -> Handle {
    let n = validate_max_listeners(n);
    if let Some(emitter) = get_event_emitter_mut(handle) {
        emitter.max_listeners = n;
    }
    handle
}

/// `emitter.getMaxListeners()`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_get_max_listeners(handle: Handle) -> f64 {
    if let Some(emitter) = get_event_emitter_mut(handle) {
        return emitter.max_listeners;
    }
    10.0
}

#[no_mangle]
pub extern "C" fn js_event_emitter_is_handle(handle: Handle) -> bool {
    is_local_event_emitter_handle(handle)
}

#[no_mangle]
pub extern "C" fn js_event_emitter_get_domain(handle: Handle) -> Handle {
    get_event_emitter_mut(handle)
        .and_then(|emitter| emitter.domain_handle)
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn js_event_emitter_set_domain(handle: Handle, domain: Handle) -> i32 {
    if let Some(emitter) = get_event_emitter_mut(handle) {
        emitter.domain_handle = if domain == 0 { None } else { Some(domain) };
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn js_event_emitter_domain_value(handle: Handle) -> f64 {
    let domain = js_event_emitter_get_domain(handle);
    if domain == 0 {
        f64::from_bits(TAG_NULL_F64_BITS)
    } else {
        nanbox_pointer_bits(domain)
    }
}

/// `emitter.eventNames()` — returns an array of strings in insertion
/// order (matches Node).
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_event_names(handle: Handle) -> *mut ArrayHeader {
    let arr = js_array_alloc(0);
    if let Some(emitter) = get_event_emitter_mut(handle) {
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
    event_bits: i64,
) -> *mut ArrayHeader {
    let arr = js_array_alloc(0);
    let Some(event_name) = event_name_from_bits(event_bits) else {
        return arr;
    };
    if let Some(emitter) = get_event_emitter_mut(handle) {
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

/// `emitter.rawListeners(eventName)` — returns once wrappers while
/// `listeners()` exposes the original user closures.
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_raw_listeners(
    handle: Handle,
    event_bits: i64,
) -> *mut ArrayHeader {
    let arr = js_array_alloc(0);
    let Some(event_name) = event_name_from_bits(event_bits) else {
        return arr;
    };
    if let Some(emitter) = get_event_emitter_mut(handle) {
        if let Some(listeners) = emitter.events.get(&event_name) {
            let mut result = arr;
            for listener in listeners.iter() {
                let callback = if listener.once && listener.raw_wrapper != 0 {
                    listener.raw_wrapper
                } else {
                    listener.callback
                };
                if callback != 0 {
                    result = js_array_push(result, JsValue::from_object_ptr(callback as *mut u8));
                }
            }
            return result;
        }
    }
    arr
}

// ============================================================================
// Module-level helpers — `events.once(em, name)` / `events.on(em, name)` /
// `events.getEventListeners(em, name)` / `events.listenerCount(em, name)` /
// `events.setMaxListeners(n, em)` / `events.getMaxListeners(em)`.
// The `events.once` listener trampolines (abort / stream-resolve /
// stream-reject) live in `module_iterators.rs` alongside
// `events_once_event_target_listener`.
// ============================================================================

/// `events.once(emitter, eventName[, options])` — returns a Promise that resolves
/// to the args array from the next matching event.
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_events_once(
    target_value: f64,
    event_name_ptr: *const StringHeader,
    options: f64,
) -> *mut Promise {
    ensure_gc_scanner_registered();
    let prom = JsPromise::new();
    let raw = prom.as_raw();
    let target = match event_helper_target(target_value) {
        Some(target) => target,
        None => {
            js_promise_reject(
                raw,
                invalid_arg_type_error(&invalid_instance_arg_message(
                    "emitter",
                    "EventEmitter",
                    target_value,
                )),
            );
            js_native_async_drop_promise_token(raw);
            return raw;
        }
    };
    let Some(event_name) = event_name_from_bits(event_name_ptr as i64) else {
        return raw;
    };
    let signal = match options_signal_result(options) {
        Ok(signal) => signal,
        Err(error) => {
            js_promise_reject(raw, error);
            js_native_async_drop_promise_token(raw);
            return raw;
        }
    };
    if signal.is_some_and(signal_is_aborted) {
        js_promise_reject(raw, js_abort_error_value());
        js_native_async_drop_promise_token(raw);
        return raw;
    }
    if let EventHelperTarget::EventEmitter(handle) = target {
        let Some(emitter) = get_event_emitter_mut(handle) else {
            return raw;
        };
        let mut pending = PendingOnce {
            promise: raw,
            signal: undefined_value(),
            abort_listener: 0,
        };
        if let Some(signal) = signal {
            if let Some(signal_ptr) = object_ptr_from_value(signal) {
                let abort_listener = js_closure_alloc(events_once_abort_listener as *const u8, 2);
                js_closure_set_capture_ptr(abort_listener, 0, handle);
                js_closure_set_capture_ptr(abort_listener, 1, raw as i64);
                js_abort_signal_add_listener(
                    signal_ptr as *mut u8,
                    abort_event_value(),
                    nanbox_pointer_bits(abort_listener as i64),
                );
                pending.signal = signal;
                pending.abort_listener = abort_listener as i64;
            }
        }
        emitter
            .pending_once_promises
            .entry(event_name)
            .or_default()
            .push(pending);
        return raw;
    }
    if let EventHelperTarget::EventTarget(target) = target {
        let event_name_ptr = string_header_ptr_from_arg(event_name_ptr);
        if event_name_ptr.is_null() {
            return raw;
        }
        let listener = js_closure_alloc(events_once_event_target_listener as *const u8, 3);
        js_closure_set_capture_ptr(listener, 0, raw as i64);
        js_closure_set_capture_ptr(listener, 1, target as i64);
        js_closure_set_capture_ptr(listener, 2, event_name_ptr as i64);
        js_event_target_add_event_listener(target, event_name_ptr, listener as i64);
        return raw;
    }
    if let EventHelperTarget::Stream(handle) = target {
        let event_name_ptr = string_header_ptr_from_arg(event_name_ptr);
        if event_name_ptr.is_null() {
            return raw;
        }
        js_register_closure_rest(events_once_stream_resolve_listener as *const u8, 0);
        js_register_closure_rest(events_once_stream_reject_listener as *const u8, 0);
        let listener = js_closure_alloc(events_once_stream_resolve_listener as *const u8, 4);
        js_closure_set_capture_ptr(listener, 0, raw as i64);
        js_closure_set_capture_ptr(listener, 1, handle);
        js_closure_set_capture_ptr(listener, 2, 0);
        js_closure_set_capture_ptr(listener, 3, 0);
        let event_value = f64::from_bits(nanbox_string_bits(event_name_ptr as *mut StringHeader));
        let listener_value = nanbox_pointer_bits(listener as i64);
        if event_name != "error" {
            let error_event_name = b"error";
            let error_event_ptr =
                js_string_from_bytes(error_event_name.as_ptr(), error_event_name.len() as u32);
            let reject_listener =
                js_closure_alloc(events_once_stream_reject_listener as *const u8, 4);
            js_closure_set_capture_ptr(reject_listener, 0, raw as i64);
            js_closure_set_capture_ptr(reject_listener, 1, handle);
            js_closure_set_capture_ptr(reject_listener, 2, event_name_ptr as i64);
            js_closure_set_capture_ptr(reject_listener, 3, listener as i64);
            js_closure_set_capture_ptr(listener, 2, reject_listener as i64);
            js_closure_set_capture_ptr(listener, 3, error_event_ptr as i64);
            let error_event = f64::from_bits(nanbox_string_bits(error_event_ptr));
            let reject_listener_value = nanbox_pointer_bits(reject_listener as i64);
            let _ = js_node_stream_method_once(handle, error_event, reject_listener_value);
        }
        let _ = js_node_stream_method_once(handle, event_value, listener_value);
    }
    raw
}

/// `events.on(emitter, eventName)` — returns an async-iterable queue of
/// argument arrays. Perry's `for await` lowering already accepts plain arrays
/// as async-iterable inputs, so the implementation backs the iterator with an
/// Array and appends one `[arg]` entry per emitted event. Ported from
/// `perry-stdlib/src/events.rs` (#1557).
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_events_on(
    target_value: f64,
    event_name_ptr: *const StringHeader,
    options: f64,
) -> *mut ArrayHeader {
    ensure_gc_scanner_registered();
    let target =
        event_helper_target(target_value).unwrap_or_else(|| throw_invalid_emitter(target_value));
    let queue = js_array_alloc(0);
    let Some(event_name) = event_name_from_bits(event_name_ptr as i64) else {
        return queue;
    };
    let event_name_ptr = string_header_ptr_from_arg(event_name_ptr);
    let signal = options_signal_or_throw(options);
    if signal.is_some_and(signal_is_aborted) {
        js_throw(js_abort_error_value());
    }
    let abort_promise = if signal.is_some() {
        JsPromise::new().as_raw()
    } else {
        std::ptr::null_mut()
    };

    let listener = js_closure_alloc(events_on_queue_listener as *const u8, 2);
    js_closure_set_capture_ptr(listener, 0, queue as i64);
    js_closure_set_capture_ptr(listener, 1, abort_promise as i64);
    if !abort_promise.is_null() {
        let _ = js_array_push_f64(queue, nanbox_pointer_bits(abort_promise as i64));
    }

    let handle = match target {
        EventHelperTarget::EventEmitter(handle) => {
            if let Some(emitter) = get_event_emitter_mut(handle) {
                emitter.add_listener(handle, &event_name, listener as i64, false, false);
            }
            handle
        }
        EventHelperTarget::EventTarget(target) => {
            if !event_name_ptr.is_null() {
                js_event_target_add_event_listener(target, event_name_ptr, listener as i64);
            }
            target as Handle
        }
        EventHelperTarget::Stream(handle) => {
            if !event_name_ptr.is_null() {
                let event = f64::from_bits(nanbox_string_bits(event_name_ptr as *mut StringHeader));
                let listener_value = nanbox_pointer_bits(listener as i64);
                let _ = js_node_stream_method_on(handle, event, listener_value);
            }
            handle
        }
    };

    if let Some(signal) = signal {
        if let Some(signal_ptr) = object_ptr_from_value(signal) {
            let abort_listener = js_closure_alloc(events_on_abort_listener as *const u8, 5);
            js_closure_set_capture_ptr(abort_listener, 0, handle);
            js_closure_set_capture_ptr(abort_listener, 1, listener as i64);
            js_closure_set_capture_ptr(abort_listener, 2, signal_ptr as i64);
            js_closure_set_capture_ptr(abort_listener, 3, abort_promise as i64);
            js_closure_set_capture_ptr(abort_listener, 4, event_name_ptr as i64);
            js_abort_signal_add_listener(
                signal_ptr as *mut u8,
                abort_event_value(),
                nanbox_pointer_bits(abort_listener as i64),
            );
        }
    }
    queue
}

extern "C" fn events_abort_listener_dispose(closure: *const RawClosureHeader) -> f64 {
    unsafe {
        let signal_ptr = js_closure_get_capture_ptr(closure, 0);
        let callback_ptr = js_closure_get_capture_ptr(closure, 1);
        if signal_ptr != 0 && callback_ptr != 0 {
            js_abort_signal_remove_listener(
                signal_ptr as *mut u8,
                abort_event_value(),
                nanbox_pointer_bits(callback_ptr),
            );
        }
    }
    undefined_value()
}

/// `events.addAbortListener(signal, listener)` — attach `listener` to the
/// AbortSignal's "abort" event and return a `Disposable`-shaped plain object.
///
/// # Safety
///
/// `signal` and `listener` are NaN-boxed JS values, matching codegen's
/// module-helper ABI.
#[no_mangle]
pub unsafe extern "C" fn js_events_add_abort_listener(signal: f64, listener: f64) -> i64 {
    let signal = validate_abort_signal_arg(signal, "signal");
    let signal_ptr = object_ptr_from_value(signal).unwrap_or_else(|| {
        throw_invalid_arg_type(&invalid_instance_arg_message(
            "signal",
            "AbortSignal",
            signal,
        ))
    });
    let callback_ptr = validate_event_listener(listener.to_bits() as i64);

    let listener_val = nanbox_pointer_bits(callback_ptr);
    js_abort_signal_add_listener(signal_ptr as *mut u8, abort_event_value(), listener_val);

    let dispose_closure = js_closure_alloc(events_abort_listener_dispose as *const u8, 2);
    js_closure_set_capture_ptr(dispose_closure, 0, signal_ptr as i64);
    js_closure_set_capture_ptr(dispose_closure, 1, callback_ptr);
    let dispose_val = nanbox_pointer_bits(dispose_closure as i64);

    let disposable = js_object_alloc(0, 0);
    let disposable_val = nanbox_pointer_bits(disposable as i64);
    let dispose_key = b"@@__perry_wk_dispose";
    let dispose_key_ptr = js_string_from_bytes(dispose_key.as_ptr(), dispose_key.len() as u32);
    let dispose_key_val = f64::from_bits(nanbox_string_bits(dispose_key_ptr));
    let dispose_sym_val = js_symbol_for(dispose_key_val);
    js_object_set_symbol_property(disposable_val, dispose_sym_val, dispose_val);
    disposable as i64
}

/// `events.getEventListeners(emitter, eventName)` — alias for
/// `emitter.listeners(eventName)`.
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_events_get_event_listeners(
    target_value: f64,
    event_name_ptr: *const StringHeader,
) -> *mut ArrayHeader {
    // AbortSignal is an EventTarget in Node, but Perry represents it as its
    // own native object that `event_helper_target` doesn't recognize. A
    // signal only ever tracks "abort" listeners.
    let signal_ptr = js_abort_signal_resolve_ptr(target_value);
    if !signal_ptr.is_null() {
        if string_from_header(event_name_ptr).as_deref() == Some("abort") {
            return js_abort_signal_listeners_copy(signal_ptr);
        }
        return js_array_alloc(0);
    }
    match event_helper_target(target_value).unwrap_or_else(|| {
        throw_invalid_arg_type(&invalid_instance_arg_message(
            "emitter",
            "EventEmitter or EventTarget",
            target_value,
        ))
    }) {
        EventHelperTarget::EventEmitter(handle) => {
            js_event_emitter_listeners(handle, event_name_ptr as i64)
        }
        EventHelperTarget::EventTarget(target) => {
            js_event_target_get_event_listeners(target, event_name_ptr)
        }
        EventHelperTarget::Stream(handle) => {
            stream_listeners_for_heap_object(handle, event_name_ptr)
                .unwrap_or_else(|| js_array_alloc(0))
        }
    }
}

/// `events.listenerCount(emitter, eventName)` — alias.
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_events_listener_count(
    target_value: f64,
    event_name_ptr: *const StringHeader,
) -> f64 {
    // AbortSignal: see `js_events_get_event_listeners`.
    let signal_ptr = js_abort_signal_resolve_ptr(target_value);
    if !signal_ptr.is_null() {
        if string_from_header(event_name_ptr).as_deref() == Some("abort") {
            return js_abort_signal_listener_count(signal_ptr);
        }
        return 0.0;
    }
    match event_helper_target(target_value).unwrap_or_else(|| {
        throw_invalid_arg_type(&invalid_instance_arg_message(
            "emitter",
            "EventEmitter or EventTarget",
            target_value,
        ))
    }) {
        EventHelperTarget::EventEmitter(handle) => js_event_emitter_listener_count(
            handle,
            event_name_ptr as i64,
            TAG_UNDEFINED_F64_BITS as i64,
        ),
        EventHelperTarget::EventTarget(target) => event_target_array_len(target, event_name_ptr),
        EventHelperTarget::Stream(handle) => {
            stream_array_len(handle, event_name_ptr).unwrap_or(0.0)
        }
    }
}

/// `events.getMaxListeners(emitter)` — alias.
#[no_mangle]
pub unsafe extern "C" fn js_events_get_max_listeners(target_value: f64) -> f64 {
    // AbortSignal: Node's default EventTarget listener cap. Perry stores no
    // per-signal override (`setMaxListeners` below is an accepted no-op), so
    // the default is always reported.
    if !js_abort_signal_resolve_ptr(target_value).is_null() {
        return 10.0;
    }
    match event_helper_target(target_value).unwrap_or_else(|| {
        throw_invalid_arg_type(&invalid_instance_arg_message(
            "emitter",
            "EventEmitter or EventTarget",
            target_value,
        ))
    }) {
        EventHelperTarget::EventEmitter(handle) => js_event_emitter_get_max_listeners(handle),
        EventHelperTarget::EventTarget(target) => js_event_target_get_max_listeners(target),
        EventHelperTarget::Stream(handle) => js_node_stream_method_get_max_listeners(handle),
    }
}

/// `events.setMaxListeners(n, ...emitters)` — Perry FFI takes a single
/// array of target handles from the codegen varargs lowering.
#[no_mangle]
pub unsafe extern "C" fn js_events_set_max_listeners(
    n: f64,
    handles_ptr: *const ArrayHeader,
) -> f64 {
    let n = validate_max_listeners(n);
    if !handles_ptr.is_null() {
        let len = (*handles_ptr).length;
        for i in 0..len {
            let value = f64::from_bits(js_array_get(handles_ptr, i).bits());
            // AbortSignal is an EventTarget in Node — SDKs routinely call
            // `events.setMaxListeners(n, controller.signal)` to raise the
            // MaxListenersExceededWarning threshold on a shared signal. Perry
            // represents signals as their own native object that
            // `event_helper_target` doesn't recognize, so this threw
            // ERR_INVALID_ARG_TYPE and rejected the caller's whole request
            // path. Accept the signal; the warning threshold is the call's
            // only Node-observable effect and Perry never emits that warning
            // for signals, so accepting is a faithful no-op.
            if !js_abort_signal_resolve_ptr(value).is_null() {
                continue;
            }
            match event_helper_target(value).unwrap_or_else(|| {
                throw_invalid_arg_type(&invalid_instance_arg_message(
                    "eventTargets",
                    "EventEmitter or EventTarget",
                    value,
                ))
            }) {
                EventHelperTarget::EventEmitter(handle) => {
                    if let Some(emitter) = get_event_emitter_mut(handle) {
                        emitter.max_listeners = n;
                    }
                }
                EventHelperTarget::EventTarget(target) => {
                    let _ = js_event_target_set_max_listeners(target, n);
                }
                EventHelperTarget::Stream(handle) => {
                    let _ = js_node_stream_method_set_max_listeners(handle, n);
                }
            }
        }
    }
    f64::from_bits(0x7FFC_0000_0000_0001)
}

#[cfg(test)]
mod tests;
