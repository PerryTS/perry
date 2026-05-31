//! Handle-dispatch registry split out of `class_registry.rs` to keep that file
//! under the 2,000-line CI gate.

use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};

/// Function pointer type for dispatching method calls on handle-based objects.
/// Handle-based objects use small integer IDs (1, 2, 3...) instead of real heap pointers.
/// This is registered by perry-stdlib to dispatch to Fastify, ioredis, etc.
pub type HandleMethodDispatchFn = unsafe extern "C" fn(
    handle: i64,
    method_name_ptr: *const u8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64;

/// Function pointer type for dispatching property access on handle-based objects.
pub type HandlePropertyDispatchFn = unsafe extern "C" fn(
    handle: i64,
    property_name_ptr: *const u8,
    property_name_len: usize,
) -> f64;

/// Function pointer type for dispatching property set on handle-based objects.
pub type HandlePropertySetDispatchFn = unsafe extern "C" fn(
    handle: i64,
    property_name_ptr: *const u8,
    property_name_len: usize,
    value: f64,
);

/// #1545: probe for whether a numeric receiver is a live Web Streams handle.
/// Web Streams handles are returned as `id as f64` (a normal float), not the
/// subnormal bit-cast other handle subsystems use, so `js_native_call_method`
/// can't recognise them by bit pattern. This probe (registered by the stdlib)
/// lets it confirm a numeric whole-number receiver really is a stream handle
/// before routing the call to `handle_method_dispatch` — non-stream numbers
/// fall through to the normal `(number).x is not a function` TypeError.
pub type StreamHandleProbeFn = unsafe extern "C" fn(id: usize) -> bool;

/// #1545: classify a numeric Web Streams handle for `instanceof` and tags.
/// Returns 0 = not a stream, 1 = ReadableStream, 2 = WritableStream,
/// 3 = reader, 4 = writer, 5 = TransformStream. Lets `x instanceof
/// ReadableStream` / `instanceof WritableStream` resolve for numeric stream
/// handles (`ts.readable`, `rs.pipeThrough(ts)`, …), and lets
/// `Object.prototype.toString.call(handle)` recover Web stream tags.
pub type StreamHandleKindProbeFn = unsafe extern "C" fn(id: usize) -> u8;

/// Probe for stdlib `events.EventEmitter` handles. The handles are returned as
/// pointer-tagged small integers, so runtime `instanceof` cannot inspect them
/// as heap objects.
pub type EventEmitterHandleProbeFn = unsafe extern "C" fn(handle: i64) -> bool;

/// Probe for stdlib `net.Socket` handles. Socket instances are represented as
/// pointer-tagged small integer handles, not heap objects with class ids.
pub type NetSocketHandleProbeFn = unsafe extern "C" fn(handle: i64) -> bool;

/// Narrow registration hook for runtime code that needs to attach an
/// EventEmitter listener without routing through the generic handle dispatcher.
pub type EventEmitterOnFn =
    unsafe extern "C" fn(handle: i64, event_bits: i64, callback: i64) -> i64;

// Dispatch tables are written once at startup (by `js_register_handle_*_dispatch`)
// and read from many threads thereafter (perry/thread workers run user code that
// hits these). Stored as AtomicPtr to make reads/writes data-race-free; the
// underlying value is still a single function pointer with Option semantics
// (null = unset).
static HANDLE_METHOD_DISPATCH_PTR: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());
static HANDLE_PROPERTY_DISPATCH_PTR: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());
static HANDLE_PROPERTY_SET_DISPATCH_PTR: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());
static STREAM_HANDLE_PROBE_PTR: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());
static STREAM_HANDLE_KIND_PROBE_PTR: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());
static EVENT_EMITTER_HANDLE_PROBE_PTR: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());
static NET_SOCKET_HANDLE_PROBE_PTR: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());
static EVENT_EMITTER_ON_PTR: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());

#[inline]
pub fn handle_method_dispatch() -> Option<HandleMethodDispatchFn> {
    let p = HANDLE_METHOD_DISPATCH_PTR.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut (), HandleMethodDispatchFn>(p) })
    }
}

#[inline]
pub fn handle_property_dispatch() -> Option<HandlePropertyDispatchFn> {
    let p = HANDLE_PROPERTY_DISPATCH_PTR.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut (), HandlePropertyDispatchFn>(p) })
    }
}

#[inline]
pub fn handle_property_set_dispatch() -> Option<HandlePropertySetDispatchFn> {
    let p = HANDLE_PROPERTY_SET_DISPATCH_PTR.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut (), HandlePropertySetDispatchFn>(p) })
    }
}

/// Register a function to handle method calls on handle-based objects.
#[no_mangle]
pub unsafe extern "C" fn js_register_handle_method_dispatch(f: HandleMethodDispatchFn) {
    HANDLE_METHOD_DISPATCH_PTR.store(f as *mut (), Ordering::Release);
}

/// #1545: probe getter — see `StreamHandleProbeFn`.
#[inline]
pub fn stream_handle_probe() -> Option<StreamHandleProbeFn> {
    let p = STREAM_HANDLE_PROBE_PTR.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut (), StreamHandleProbeFn>(p) })
    }
}

/// #1545: register the Web Streams handle probe (called by the stdlib at init).
#[no_mangle]
pub unsafe extern "C" fn js_register_stream_handle_probe(f: StreamHandleProbeFn) {
    STREAM_HANDLE_PROBE_PTR.store(f as *mut (), Ordering::Release);
}

/// #1545: kind-probe getter — see `StreamHandleKindProbeFn`.
#[inline]
pub fn stream_handle_kind_probe() -> Option<StreamHandleKindProbeFn> {
    let p = STREAM_HANDLE_KIND_PROBE_PTR.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut (), StreamHandleKindProbeFn>(p) })
    }
}

/// #1545: register the Web Streams kind probe (called by the stdlib at init).
#[no_mangle]
pub unsafe extern "C" fn js_register_stream_handle_kind_probe(f: StreamHandleKindProbeFn) {
    STREAM_HANDLE_KIND_PROBE_PTR.store(f as *mut (), Ordering::Release);
}

#[inline]
pub fn event_emitter_handle_probe() -> Option<EventEmitterHandleProbeFn> {
    let p = EVENT_EMITTER_HANDLE_PROBE_PTR.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut (), EventEmitterHandleProbeFn>(p) })
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_register_event_emitter_handle_probe(f: EventEmitterHandleProbeFn) {
    EVENT_EMITTER_HANDLE_PROBE_PTR.store(f as *mut (), Ordering::Release);
}

#[inline]
pub fn net_socket_handle_probe() -> Option<NetSocketHandleProbeFn> {
    let p = NET_SOCKET_HANDLE_PROBE_PTR.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut (), NetSocketHandleProbeFn>(p) })
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_register_net_socket_handle_probe(f: NetSocketHandleProbeFn) {
    NET_SOCKET_HANDLE_PROBE_PTR.store(f as *mut (), Ordering::Release);
}

#[inline]
pub fn event_emitter_on() -> Option<EventEmitterOnFn> {
    let p = EVENT_EMITTER_ON_PTR.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut (), EventEmitterOnFn>(p) })
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_register_event_emitter_on(f: EventEmitterOnFn) {
    EVENT_EMITTER_ON_PTR.store(f as *mut (), Ordering::Release);
}

/// Register a function to handle property access on handle-based objects.
#[no_mangle]
pub unsafe extern "C" fn js_register_handle_property_dispatch(f: HandlePropertyDispatchFn) {
    HANDLE_PROPERTY_DISPATCH_PTR.store(f as *mut (), Ordering::Release);
}

/// Register a function to handle property set on handle-based objects.
#[no_mangle]
pub unsafe extern "C" fn js_register_handle_property_set_dispatch(f: HandlePropertySetDispatchFn) {
    HANDLE_PROPERTY_SET_DISPATCH_PTR.store(f as *mut (), Ordering::Release);
}
