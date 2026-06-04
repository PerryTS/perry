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
    js_array_alloc, js_array_get, js_array_push, js_array_set, nanbox_string_bits, read_string,
    ArrayHeader, Handle, JsPromise, JsString, JsValue, ObjectHeader, Promise, RawClosureHeader,
    StringHeader,
};
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Mutex, MutexGuard, Once, OnceLock};

const MIN_HEAP_POINTER: u64 = 0x1000;
const EVENT_TARGET_MIN_HEAP_POINTER: u64 = 0x10000;
const MAX_HEAP_POINTER: u64 = 0x0000_FFFF_FFFF_FFFF;

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
    fn js_object_get_field_by_name_f64(obj: *const ObjectHeader, key: *const StringHeader) -> f64;
    fn js_abort_signal_add_listener(signal: *mut u8, event: f64, listener: f64);
    fn js_event_target_is_event_target(target: *const u8) -> i32;
    fn js_event_target_get_event_listeners(
        target: *mut u8,
        event: *const StringHeader,
    ) -> *mut ArrayHeader;
    fn js_event_target_get_max_listeners(target: *mut u8) -> f64;
    fn js_event_target_set_max_listeners(target: *mut u8, n: f64) -> i32;
    fn js_node_stream_method_listeners(stream_handle: i64, event: f64) -> i64;
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
    // #3072: shared listener validator. Takes the raw NaN-box bits of the
    // listener arg (codegen routes these methods through NA_JSV) and the arg
    // name; returns the closure pointer when callable, else throws
    // TypeError [ERR_INVALID_ARG_TYPE]. Centralized in perry-runtime so the
    // stdlib and ext-events EventEmitter implementations stay byte-identical.
    fn js_validate_event_listener(listener_bits: i64, name_ptr: *const u8, name_len: u32) -> i64;
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
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
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

/// One registered listener: a raw closure pointer (i64 to satisfy
/// Send + Sync — the underlying ClosureHeader is GC-managed) plus a
/// `once` flag.
#[derive(Copy, Clone)]
struct Listener {
    callback: i64,
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
    max_listeners: i32,
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
            // Node's default — `getMaxListeners()` on a fresh emitter
            // returns 10.
            max_listeners: 10,
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

type EventEmitterRegistry = Vec<Option<Box<EventEmitterHandle>>>;

static EVENT_EMITTERS: OnceLock<Mutex<EventEmitterRegistry>> = OnceLock::new();
static EVENTS_RUNTIME_HOOKS_REGISTERED: Once = Once::new();

thread_local! {
    static EVENTS_GC_REGISTERED: std::cell::Cell<bool> = std::cell::Cell::new(false);
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

fn ensure_runtime_hooks_registered() {
    EVENTS_RUNTIME_HOOKS_REGISTERED.call_once(|| unsafe {
        js_register_event_emitter_handle_probe(event_emitter_handle_probe);
        js_register_event_emitter_get_domain(js_event_emitter_get_domain);
        js_register_event_emitter_set_domain(js_event_emitter_set_domain);
        js_register_event_emitter_on(event_emitter_on_hook);
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

fn handle_from_js_value_bits(bits: u64) -> Handle {
    if (bits & 0xFFFF_0000_0000_0000) == POINTER_TAG {
        (bits & POINTER_MASK) as Handle
    } else {
        let value = f64::from_bits(bits);
        if value.is_finite() && value > 0.0 && value.fract() == 0.0 {
            value as Handle
        } else {
            bits as Handle
        }
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

unsafe fn options_signal(options: f64) -> Option<f64> {
    let jsval = JsValue::from_bits(options.to_bits());
    if jsval.is_undefined() || jsval.is_null() {
        return None;
    }
    get_object_property(options, b"signal")
        .filter(|signal| object_ptr_from_value(*signal).is_some())
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

fn remove_listener_by_callback(emitter: &mut EventEmitterHandle, callback: i64) {
    if callback == 0 {
        return;
    }
    let event_names: Vec<String> = emitter.events.keys().cloned().collect();
    for event_name in event_names {
        let removed = if let Some(listeners) = emitter.events.get_mut(&event_name) {
            let before = listeners.len();
            listeners.retain(|listener| listener.callback != callback);
            before != listeners.len()
        } else {
            false
        };
        if removed {
            emitter.prune_event_if_empty(&event_name);
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
    event_bits: i64,
    listener_bits: i64,
) -> Handle {
    ensure_gc_scanner_registered();
    let callback_ptr = validate_event_listener(listener_bits);
    let Some(event_name) = event_name_from_bits(event_bits) else {
        return handle;
    };
    if let Some(emitter) = get_event_emitter_mut(handle) {
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
    event_bits: i64,
    listener_bits: i64,
) -> Handle {
    ensure_gc_scanner_registered();
    let callback_ptr = validate_event_listener(listener_bits);
    let Some(event_name) = event_name_from_bits(event_bits) else {
        return handle;
    };
    if let Some(emitter) = get_event_emitter_mut(handle) {
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
    event_bits: i64,
    listener_bits: i64,
) -> Handle {
    ensure_gc_scanner_registered();
    let callback_ptr = validate_event_listener(listener_bits);
    let Some(event_name) = event_name_from_bits(event_bits) else {
        return handle;
    };
    if let Some(emitter) = get_event_emitter_mut(handle) {
        emitter.add_listener(&event_name, callback_ptr, true, true);
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

/// `emitter.removeListener(event, listener)`. Removes the first
/// matching listener only (matches Node).
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
    args_ptr: *const ArrayHeader,
) -> Handle {
    if let Some(emitter) = get_event_emitter_mut(handle) {
        if args_ptr.is_null() || (*args_ptr).length == 0 {
            emitter.events.clear();
            emitter.event_order.clear();
        } else if let Some(event_name) =
            event_name_from_bits(js_array_get(args_ptr, 0).bits() as i64)
        {
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
pub unsafe extern "C" fn js_event_emitter_listener_count(handle: Handle, event_bits: i64) -> f64 {
    let Some(event_name) = event_name_from_bits(event_bits) else {
        return 0.0;
    };
    if let Some(emitter) = get_event_emitter_mut(handle) {
        if let Some(listeners) = emitter.events.get(&event_name) {
            return listeners.len() as f64;
        }
    }
    0.0
}

/// `emitter.setMaxListeners(n)`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_set_max_listeners(handle: Handle, n: f64) -> Handle {
    if let Some(emitter) = get_event_emitter_mut(handle) {
        emitter.max_listeners = n as i32;
    }
    handle
}

/// `emitter.getMaxListeners()`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_get_max_listeners(handle: Handle) -> f64 {
    if let Some(emitter) = get_event_emitter_mut(handle) {
        return emitter.max_listeners as f64;
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

/// `emitter.rawListeners(eventName)` — identical to `listeners` in our
/// model (we don't wrap once-listeners at registration time).
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_event_emitter_raw_listeners(
    handle: Handle,
    event_bits: i64,
) -> *mut ArrayHeader {
    js_event_emitter_listeners(handle, event_bits)
}

// ============================================================================
// Module-level helpers — `events.once(em, name)` / `events.on(em, name)` /
// `events.getEventListeners(em, name)` / `events.listenerCount(em, name)` /
// `events.setMaxListeners(n, em)` / `events.getMaxListeners(em)`.
// ============================================================================

extern "C" fn events_once_abort_listener(closure: *const RawClosureHeader) -> f64 {
    unsafe {
        let handle = js_closure_get_capture_ptr(closure, 0) as Handle;
        let promise = js_closure_get_capture_ptr(closure, 1) as *mut Promise;
        let pending = get_event_emitter_mut(handle)
            .and_then(|emitter| remove_pending_once_promise(emitter, promise));
        if let Some(pending) = pending {
            cleanup_pending_abort_listener(&pending);
            if !pending.promise.is_null() {
                js_promise_reject(pending.promise, js_abort_error_value());
            }
        }
    }
    undefined_value()
}

/// `events.once(emitter, eventName[, options])` — returns a Promise that resolves
/// to a 1-element array `[arg]` on the next `emit(eventName, arg)`.
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
    let handle = handle_from_value(target_value);
    let Some(event_name) = event_name_from_bits(event_name_ptr as i64) else {
        return raw;
    };
    let signal = options_signal(options);
    if signal.is_some_and(signal_is_aborted) {
        js_promise_reject(raw, js_abort_error_value());
        return raw;
    }
    if let Some(emitter) = get_event_emitter_mut(handle) {
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
    }
    raw
}

/// Queue listener for `events.on(...)` — captures the queue array in
/// slot 0 and pushes `[arg]` onto it for each emitted event. The
/// `for await (... of iter)` loop pulls items off the array as the
/// stream produces them.
extern "C" fn events_on_queue_listener(closure: *const RawClosureHeader, arg0: f64) -> f64 {
    unsafe {
        let queue = js_closure_get_capture_ptr(closure, 0) as *mut ArrayHeader;
        let abort_promise = js_closure_get_capture_ptr(closure, 1) as *mut Promise;
        if !queue.is_null() {
            let mut args = js_array_alloc(0);
            args = js_array_push_f64(args, arg0);
            let args_val = nanbox_pointer_bits(args as i64);
            if abort_promise.is_null() {
                let _ = js_array_push_f64(queue, args_val);
            } else {
                let abort_val = nanbox_pointer_bits(abort_promise as i64);
                let len = (*queue).length;
                if len == 0 {
                    let _ = js_array_push_f64(queue, args_val);
                    let _ = js_array_push_f64(queue, abort_val);
                } else {
                    js_array_set(queue, len - 1, JsValue::from_bits(args_val.to_bits()));
                    let _ = js_array_push_f64(queue, abort_val);
                }
            }
        }
    }
    f64::from_bits(TAG_UNDEFINED_F64_BITS)
}

extern "C" fn events_on_abort_listener(closure: *const RawClosureHeader) -> f64 {
    unsafe {
        let handle = js_closure_get_capture_ptr(closure, 0) as Handle;
        let data_listener = js_closure_get_capture_ptr(closure, 1);
        let signal_ptr = js_closure_get_capture_ptr(closure, 2) as *mut u8;
        let abort_promise = js_closure_get_capture_ptr(closure, 3) as *mut Promise;

        if let Some(emitter) = get_event_emitter_mut(handle) {
            remove_listener_by_callback(emitter, data_listener);
        }
        if !signal_ptr.is_null() {
            js_abort_signal_remove_listener(
                signal_ptr,
                abort_event_value(),
                nanbox_pointer_bits(closure as i64),
            );
        }
        if !abort_promise.is_null() {
            js_promise_reject(abort_promise, js_abort_error_value());
        }
    }
    undefined_value()
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
    let queue = js_array_alloc(0);
    let handle = handle_from_value(target_value);
    let Some(event_name) = event_name_from_bits(event_name_ptr as i64) else {
        return queue;
    };
    let signal = options_signal(options);
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

    if let Some(emitter) = get_event_emitter_mut(handle) {
        emitter.add_listener(&event_name, listener as i64, false, false);
        if let Some(signal) = signal {
            if let Some(signal_ptr) = object_ptr_from_value(signal) {
                let abort_listener = js_closure_alloc(events_on_abort_listener as *const u8, 4);
                js_closure_set_capture_ptr(abort_listener, 0, handle);
                js_closure_set_capture_ptr(abort_listener, 1, listener as i64);
                js_closure_set_capture_ptr(abort_listener, 2, signal_ptr as i64);
                js_closure_set_capture_ptr(abort_listener, 3, abort_promise as i64);
                js_abort_signal_add_listener(
                    signal_ptr as *mut u8,
                    abort_event_value(),
                    nanbox_pointer_bits(abort_listener as i64),
                );
            }
        }
    }
    queue
}

/// `events.addAbortListener(signal, listener)` — attach `listener` to the
/// AbortSignal's "abort" event and return a `Disposable`-shaped plain object
/// (currently a function-shaped placeholder — listener removal can be
/// tightened later). Ported from `perry-stdlib/src/events.rs` (#1557).
///
/// # Safety
///
/// `signal_ptr` must be null or a Perry-runtime `ObjectHeader` (the
/// AbortSignal instance); `callback_ptr` must be null or a closure pointer.
#[no_mangle]
pub unsafe extern "C" fn js_events_add_abort_listener(signal: f64, listener: f64) -> i64 {
    let Some(signal_ptr) = object_ptr_from_value(signal) else {
        return 0;
    };
    let callback_ptr = handle_from_value(listener);
    if callback_ptr == 0 {
        return 0;
    }
    let event_name = b"abort";
    let event_str = js_string_from_bytes(event_name.as_ptr(), event_name.len() as u32);
    let event_val = f64::from_bits(nanbox_string_bits(event_str));
    let listener_val = nanbox_pointer_bits(callback_ptr);
    js_abort_signal_add_listener(signal_ptr as *mut u8, event_val, listener_val);
    // Perry currently surfaces the disposable as the listener itself
    // (matching node's `{ [Symbol.dispose]: () => signal.removeEventListener(...) }`
    // shape requires symbol-property writes that this crate doesn't expose
    // yet; perry-stdlib's version uses perry_runtime::symbol helpers). The
    // returned pointer is callable, so `disposable[Symbol.dispose]?.()` won't
    // crash — it just won't actually unsubscribe. Tightening this is tracked
    // in #1557's follow-up bullet.
    callback_ptr
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
    let handle = handle_from_value(target_value);
    if get_event_emitter_mut(handle).is_some() {
        return js_event_emitter_listeners(handle, event_name_ptr as i64);
    }
    if let Some(target) = event_target_ptr(handle) {
        return js_event_target_get_event_listeners(target, event_name_ptr);
    }
    if let Some(listeners) = stream_listeners_for_heap_object(handle, event_name_ptr) {
        return listeners;
    }
    js_array_alloc(0)
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
    let handle = handle_from_value(target_value);
    js_event_emitter_listener_count(handle, event_name_ptr as i64)
}

/// `events.getMaxListeners(emitter)` — alias.
#[no_mangle]
pub unsafe extern "C" fn js_events_get_max_listeners(target_value: f64) -> f64 {
    let handle = handle_from_value(target_value);
    if let Some(emitter) = get_event_emitter_mut(handle) {
        return emitter.max_listeners as f64;
    }
    if let Some(target) = event_target_ptr(handle) {
        return js_event_target_get_max_listeners(target);
    }
    10.0
}

/// `events.setMaxListeners(n, ...emitters)` — Perry FFI takes a single
/// array of target handles from the codegen varargs lowering.
#[no_mangle]
pub unsafe extern "C" fn js_events_set_max_listeners(
    n: f64,
    handles_ptr: *const ArrayHeader,
) -> f64 {
    if !handles_ptr.is_null() {
        let len = (*handles_ptr).length;
        for i in 0..len {
            let handle = handle_from_js_value_bits(js_array_get(handles_ptr, i).bits());
            if let Some(emitter) = get_event_emitter_mut(handle) {
                emitter.max_listeners = n as i32;
            } else if let Some(target) = event_target_ptr(handle) {
                let _ = js_event_target_set_max_listeners(target, n);
            }
        }
    }
    f64::from_bits(0x7FFC_0000_0000_0001)
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_ffi::alloc_string;
    use std::collections::HashMap;
    use std::sync::{Mutex, MutexGuard};

    static GC_TEST_LOCK: Mutex<()> = Mutex::new(());

    struct GcTestGuard {
        frame: u64,
        _lock: MutexGuard<'static, ()>,
    }

    impl GcTestGuard {
        fn new() -> Self {
            let lock = GC_TEST_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            perry_runtime::gc::js_gc_write_barriers_emitted(1);
            let frame = perry_runtime::gc::js_shadow_frame_push(0);
            Self { frame, _lock: lock }
        }
    }

    impl Drop for GcTestGuard {
        fn drop(&mut self) {
            perry_runtime::gc::js_shadow_frame_pop(self.frame);
            perry_runtime::gc::js_gc_write_barriers_emitted(0);
        }
    }

    fn young_gc_root() -> i64 {
        perry_runtime::arena::arena_alloc_gc(32, 8, perry_runtime::gc::GC_TYPE_STRING) as i64
    }

    fn young_promise_root() -> *mut Promise {
        let ptr = perry_runtime::arena::arena_alloc_gc(
            std::mem::size_of::<perry_runtime::Promise>(),
            std::mem::align_of::<perry_runtime::Promise>(),
            perry_runtime::gc::GC_TYPE_PROMISE,
        );
        unsafe {
            std::ptr::write_bytes(ptr, 0, std::mem::size_of::<perry_runtime::Promise>());
        }
        ptr as *mut Promise
    }

    /// #3072: the listener-registration FFIs now take the raw NaN-box bits of
    /// the listener and validate it is a closure. Allocate a real closure and
    /// return its NaN-boxed bits (as i64) so the tests pass a value that the
    /// shared validator accepts. `fn_ptr` is never invoked here (we never
    /// `emit`), so a sentinel function pointer suffices.
    extern "C" fn noop_listener(_c: *const RawClosureHeader) -> f64 {
        f64::from_bits(TAG_UNDEFINED_F64_BITS)
    }

    fn fake_listener() -> i64 {
        let closure = unsafe { js_closure_alloc(noop_listener as *const u8, 0) };
        nanbox_pointer_bits(closure as i64).to_bits() as i64
    }

    fn assert_rewritten(before: usize, after: usize) {
        assert_ne!(after, before);
        assert!(perry_runtime::arena::pointer_in_nursery(after));
    }

    #[test]
    fn new_emitter_starts_empty() {
        let h = js_event_emitter_new();
        let event_name = alloc_string("foo");
        let event_bits = event_bits_from_string_ptr(event_name.as_raw() as *const _);
        let count = unsafe { js_event_emitter_listener_count(h, event_bits) };
        assert_eq!(count, 0.0);
        drop_event_emitter_handle(h);
    }

    #[test]
    fn add_then_count_listeners() {
        let h = js_event_emitter_new();
        let event_name = alloc_string("change");
        let event_bits = event_bits_from_string_ptr(event_name.as_raw() as *const _);
        // Real closures — we never emit so the bodies aren't invoked.
        let _ = unsafe { js_event_emitter_on(h, event_bits, fake_listener()) };
        let _ = unsafe { js_event_emitter_on(h, event_bits, fake_listener()) };
        let count = unsafe { js_event_emitter_listener_count(h, event_bits) };
        assert_eq!(count, 2.0);
        drop_event_emitter_handle(h);
    }

    #[test]
    fn remove_listener_drops_one() {
        let h = js_event_emitter_new();
        let event_name = alloc_string("data");
        let event_bits = event_bits_from_string_ptr(event_name.as_raw() as *const _);
        let a = fake_listener();
        let b = fake_listener();
        unsafe {
            js_event_emitter_on(h, event_bits, a);
            js_event_emitter_on(h, event_bits, b);
            js_event_emitter_remove_listener(h, event_bits, a);
        }
        let count = unsafe { js_event_emitter_listener_count(h, event_bits) };
        assert_eq!(count, 1.0);
        drop_event_emitter_handle(h);
    }

    #[test]
    fn remove_all_clears() {
        let h = js_event_emitter_new();
        let event_name = alloc_string("x");
        let event_bits = event_bits_from_string_ptr(event_name.as_raw() as *const _);
        unsafe {
            js_event_emitter_on(h, event_bits, fake_listener());
            js_event_emitter_on(h, event_bits, fake_listener());
            js_event_emitter_remove_all_listeners(h, std::ptr::null());
        }
        let count = unsafe { js_event_emitter_listener_count(h, event_bits) };
        assert_eq!(count, 0.0);
        drop_event_emitter_handle(h);
    }

    #[test]
    fn prepend_listener_inserts_at_front() {
        let h = js_event_emitter_new();
        let event_name = alloc_string("ord");
        let event_bits = event_bits_from_string_ptr(event_name.as_raw() as *const _);
        unsafe {
            js_event_emitter_on(h, event_bits, fake_listener());
            js_event_emitter_prepend_listener(h, event_bits, fake_listener());
        }
        let arr = unsafe { js_event_emitter_listeners(h, event_bits) };
        assert!(!arr.is_null());
        drop_event_emitter_handle(h);
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
        drop_event_emitter_handle(h);
    }

    #[test]
    fn gc_mutable_scanner_rewrites_listener_and_pending_promise_roots() {
        let _guard = GcTestGuard::new();
        ensure_gc_scanner_registered();

        let listener = young_gc_root();
        let promise = young_promise_root();
        let mut events = HashMap::new();
        events.insert(
            "ready".to_string(),
            vec![Listener {
                callback: listener,
                once: false,
            }],
        );
        let mut pending_once_promises = HashMap::new();
        pending_once_promises.insert(
            "ready".to_string(),
            vec![PendingOnce {
                promise,
                signal: undefined_value(),
                abort_listener: 0,
            }],
        );
        let handle = register_event_emitter_handle(EventEmitterHandle {
            events,
            event_order: vec!["ready".to_string()],
            pending_once_promises,
            max_listeners: 10,
            capture_rejections: false,
            domain_handle: None,
        });

        let _ = perry_runtime::gc::gc_collect_minor();

        {
            let emitter = get_event_emitter_mut(handle).expect("emitter handle should remain live");
            assert_rewritten(
                listener as usize,
                emitter.events["ready"][0].callback as usize,
            );
            assert_rewritten(
                promise as usize,
                emitter.pending_once_promises["ready"][0].promise as usize,
            );
        }
        drop_event_emitter_handle(handle);
    }
}
