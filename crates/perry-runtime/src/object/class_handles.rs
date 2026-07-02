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

/// Optional extension dispatchers used by well-known external native crates.
/// They return 1 when they handled the access and write the result to `out`;
/// returning 0 lets the primary stdlib dispatcher continue unchanged.
pub type HandleMethodDispatchExtensionFn = unsafe extern "C" fn(
    handle: i64,
    method_name_ptr: *const u8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
    out: *mut f64,
) -> i32;
pub type HandlePropertyDispatchExtensionFn = unsafe extern "C" fn(
    handle: i64,
    property_name_ptr: *const u8,
    property_name_len: usize,
    out: *mut f64,
) -> i32;
pub type HandlePropertySetDispatchExtensionFn = unsafe extern "C" fn(
    handle: i64,
    property_name_ptr: *const u8,
    property_name_len: usize,
    value: f64,
) -> i32;

/// Function pointer type for reporting own property names on handle-backed
/// values. Returns a NaN-boxed Array, or `undefined` when the handle has no
/// custom shape.
pub type HandleOwnPropertyNamesDispatchFn = unsafe extern "C" fn(handle: i64) -> f64;

/// Function pointer type for resolving `Object.getPrototypeOf(handle)`.
/// Returns a NaN-boxed object/null, or `undefined` when the handle has no
/// custom prototype.
pub type HandlePrototypeDispatchFn = unsafe extern "C" fn(handle: i64) -> f64;

/// #1545: probe for whether a numeric receiver is a live Web Streams handle.
/// Web Streams handles are returned as `id as f64` (a normal float), not the
/// subnormal bit-cast other handle subsystems use, so `js_native_call_method`
/// can't recognise them by bit pattern. This probe (registered by the stdlib)
/// lets it confirm a numeric whole-number receiver really is a stream handle
/// before routing the call to `handle_method_dispatch` — non-stream numbers
/// fall through to the normal `(number).x is not a function` TypeError.
pub type StreamHandleProbeFn = unsafe extern "C" fn(id: usize) -> bool;
/// #5437: stdlib hook that stores an expando property on a live Web Stream
/// handle (React's `renderToReadableStream` attaches `allReady` to the
/// stream). Returns 1 when the id was a live stream and the value was
/// stored, 0 otherwise.
pub type StreamExpandoSetFn =
    unsafe extern "C" fn(id: usize, key_ptr: *const u8, key_len: usize, value: f64) -> i32;

/// #1545: classify a numeric Web Streams handle for `instanceof` and tags.
/// Returns 0 = not a stream, 1 = ReadableStream, 2 = WritableStream,
/// 3 = reader, 4 = writer, 5 = TransformStream. Lets `x instanceof
/// ReadableStream` / `instanceof WritableStream` resolve for numeric stream
/// handles (`ts.readable`, `rs.pipeThrough(ts)`, …), and lets
/// `Object.prototype.toString.call(handle)` recover Web stream tags.
pub type StreamHandleKindProbeFn = unsafe extern "C" fn(id: usize) -> u8;

/// Probe for WHATWG fetch handles (`Response`/`Request`/`Headers`/`Blob`),
/// which are pointer-tagged small-integer ids, not heap objects with a class
/// chain. Returns 0 = none, 1 = Response, 2 = Request, 3 = Headers, 4 = Blob.
/// Lets `x instanceof Response` (etc.) resolve for fetch handles — Hono guards
/// route fallbacks with `res instanceof Response`, so without this the bare
/// handle fails the `instanceof` and the guard is skipped.
pub type FetchHandleKindProbeFn = unsafe extern "C" fn(id: usize) -> u8;

/// Probe for stdlib `events.EventEmitter` handles. The handles are returned as
/// pointer-tagged small integers, so runtime `instanceof` cannot inspect them
/// as heap objects.
pub type EventEmitterHandleProbeFn = unsafe extern "C" fn(handle: i64) -> bool;
pub type EventEmitterAsyncResourceHandleProbeFn = unsafe extern "C" fn(handle: i64) -> bool;
pub type EventEmitterGetDomainFn = unsafe extern "C" fn(handle: i64) -> i64;
pub type EventEmitterSetDomainFn = unsafe extern "C" fn(handle: i64, domain: i64) -> i32;

/// Probe for stdlib `net.Socket` handles. Socket instances are represented as
/// pointer-tagged small integer handles, not heap objects with class ids.
pub type NetSocketHandleProbeFn = unsafe extern "C" fn(handle: i64) -> bool;

/// Probe for live `perry-ffi` registry handles. `register_handle`-issued ids
/// and Node timer ids both occupy the pointer-tagged small-integer band and
/// both count from 1, so a bare id is ambiguous between (say) an HTTP/2 server
/// handle and a `setTimeout` id. The timer-method dispatch arm consults this
/// probe to yield to an authoritative registered handle when the id is one,
/// so `server.close()` (handle 1) is not swallowed by `clearTimeout(1)` when a
/// timer with the colliding id also happens to be alive.
pub type FfiHandleExistsProbeFn = unsafe extern "C" fn(handle: i64) -> bool;

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
static HANDLE_METHOD_EXTENSION_DISPATCH_PTRS: [AtomicPtr<()>; 4] = [
    AtomicPtr::new(ptr::null_mut()),
    AtomicPtr::new(ptr::null_mut()),
    AtomicPtr::new(ptr::null_mut()),
    AtomicPtr::new(ptr::null_mut()),
];
static HANDLE_PROPERTY_EXTENSION_DISPATCH_PTRS: [AtomicPtr<()>; 4] = [
    AtomicPtr::new(ptr::null_mut()),
    AtomicPtr::new(ptr::null_mut()),
    AtomicPtr::new(ptr::null_mut()),
    AtomicPtr::new(ptr::null_mut()),
];
static HANDLE_PROPERTY_SET_EXTENSION_DISPATCH_PTRS: [AtomicPtr<()>; 4] = [
    AtomicPtr::new(ptr::null_mut()),
    AtomicPtr::new(ptr::null_mut()),
    AtomicPtr::new(ptr::null_mut()),
    AtomicPtr::new(ptr::null_mut()),
];
static HANDLE_OWN_PROPERTY_NAMES_DISPATCH_PTR: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());
static HANDLE_PROTOTYPE_DISPATCH_PTR: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());
static STREAM_HANDLE_PROBE_PTR: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());
static STREAM_EXPANDO_SET_PTR: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());
static STREAM_HANDLE_KIND_PROBE_PTR: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());
static FETCH_HANDLE_KIND_PROBE_PTR: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());
static EVENT_EMITTER_HANDLE_PROBE_PTR: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());
static EVENT_EMITTER_ASYNC_RESOURCE_HANDLE_PROBE_PTR: AtomicPtr<()> =
    AtomicPtr::new(ptr::null_mut());
static EVENT_EMITTER_GET_DOMAIN_PTR: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());
static EVENT_EMITTER_SET_DOMAIN_PTR: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());
static NET_SOCKET_HANDLE_PROBE_PTR: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());
static FFI_HANDLE_EXISTS_PROBE_PTR: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());
static EVENT_EMITTER_ON_PTR: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

fn has_extension(slots: &[AtomicPtr<()>]) -> bool {
    slots
        .iter()
        .any(|slot| !slot.load(Ordering::Acquire).is_null())
}

fn register_extension(slots: &[AtomicPtr<()>], f: *mut ()) {
    for slot in slots {
        if slot.load(Ordering::Acquire) == f {
            return;
        }
    }
    for slot in slots {
        if slot
            .compare_exchange(ptr::null_mut(), f, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return;
        }
    }
    slots[slots.len() - 1].store(f, Ordering::Release);
}

unsafe extern "C" fn composite_handle_method_dispatch(
    handle: i64,
    method_name_ptr: *const u8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    for slot in HANDLE_METHOD_EXTENSION_DISPATCH_PTRS.iter() {
        let p = slot.load(Ordering::Acquire);
        if p.is_null() {
            continue;
        }
        let f = std::mem::transmute::<*mut (), HandleMethodDispatchExtensionFn>(p);
        let mut out = f64::from_bits(TAG_UNDEFINED);
        if f(
            handle,
            method_name_ptr,
            method_name_len,
            args_ptr,
            args_len,
            &mut out,
        ) != 0
        {
            return out;
        }
    }

    let p = HANDLE_METHOD_DISPATCH_PTR.load(Ordering::Acquire);
    if p.is_null() {
        f64::from_bits(TAG_UNDEFINED)
    } else {
        let f = std::mem::transmute::<*mut (), HandleMethodDispatchFn>(p);
        f(handle, method_name_ptr, method_name_len, args_ptr, args_len)
    }
}

unsafe extern "C" fn composite_handle_property_dispatch(
    handle: i64,
    property_name_ptr: *const u8,
    property_name_len: usize,
) -> f64 {
    for slot in HANDLE_PROPERTY_EXTENSION_DISPATCH_PTRS.iter() {
        let p = slot.load(Ordering::Acquire);
        if p.is_null() {
            continue;
        }
        let f = std::mem::transmute::<*mut (), HandlePropertyDispatchExtensionFn>(p);
        let mut out = f64::from_bits(TAG_UNDEFINED);
        if f(handle, property_name_ptr, property_name_len, &mut out) != 0 {
            return out;
        }
    }

    let p = HANDLE_PROPERTY_DISPATCH_PTR.load(Ordering::Acquire);
    if p.is_null() {
        f64::from_bits(TAG_UNDEFINED)
    } else {
        let f = std::mem::transmute::<*mut (), HandlePropertyDispatchFn>(p);
        f(handle, property_name_ptr, property_name_len)
    }
}

unsafe extern "C" fn composite_handle_property_set_dispatch(
    handle: i64,
    property_name_ptr: *const u8,
    property_name_len: usize,
    value: f64,
) {
    for slot in HANDLE_PROPERTY_SET_EXTENSION_DISPATCH_PTRS.iter() {
        let p = slot.load(Ordering::Acquire);
        if p.is_null() {
            continue;
        }
        let f = std::mem::transmute::<*mut (), HandlePropertySetDispatchExtensionFn>(p);
        if f(handle, property_name_ptr, property_name_len, value) != 0 {
            return;
        }
    }

    let p = HANDLE_PROPERTY_SET_DISPATCH_PTR.load(Ordering::Acquire);
    if !p.is_null() {
        let f = std::mem::transmute::<*mut (), HandlePropertySetDispatchFn>(p);
        f(handle, property_name_ptr, property_name_len, value);
    }
}

#[inline]
pub fn handle_method_dispatch() -> Option<HandleMethodDispatchFn> {
    if has_extension(&HANDLE_METHOD_EXTENSION_DISPATCH_PTRS) {
        return Some(composite_handle_method_dispatch);
    }
    let p = HANDLE_METHOD_DISPATCH_PTR.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut (), HandleMethodDispatchFn>(p) })
    }
}

#[inline]
pub fn handle_property_dispatch() -> Option<HandlePropertyDispatchFn> {
    if has_extension(&HANDLE_PROPERTY_EXTENSION_DISPATCH_PTRS) {
        return Some(composite_handle_property_dispatch);
    }
    let p = HANDLE_PROPERTY_DISPATCH_PTR.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut (), HandlePropertyDispatchFn>(p) })
    }
}

/// #4973: the PRIMARY (stdlib) handle method dispatcher only, skipping the
/// extension dispatchers. The inherits-alias forwarding uses this because it
/// KNOWS its handle is an http(s) server handle — going through the
/// composite would let an id-colliding extension registry (an ext-net
/// socket with the same small id) claim shared method names like
/// `address`/`on` first and answer for the wrong object.
#[inline]
pub(crate) fn handle_method_dispatch_primary() -> Option<HandleMethodDispatchFn> {
    let p = HANDLE_METHOD_DISPATCH_PTR.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut (), HandleMethodDispatchFn>(p) })
    }
}

/// Primary-only sibling of `handle_property_dispatch` — see
/// `handle_method_dispatch_primary`.
#[inline]
pub(crate) fn handle_property_dispatch_primary() -> Option<HandlePropertyDispatchFn> {
    let p = HANDLE_PROPERTY_DISPATCH_PTR.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut (), HandlePropertyDispatchFn>(p) })
    }
}

#[inline]
pub fn handle_property_set_dispatch() -> Option<HandlePropertySetDispatchFn> {
    if has_extension(&HANDLE_PROPERTY_SET_EXTENSION_DISPATCH_PTRS) {
        return Some(composite_handle_property_set_dispatch);
    }
    let p = HANDLE_PROPERTY_SET_DISPATCH_PTR.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut (), HandlePropertySetDispatchFn>(p) })
    }
}

#[inline]
pub fn handle_own_property_names_dispatch() -> Option<HandleOwnPropertyNamesDispatchFn> {
    let p = HANDLE_OWN_PROPERTY_NAMES_DISPATCH_PTR.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut (), HandleOwnPropertyNamesDispatchFn>(p) })
    }
}

#[inline]
pub fn handle_prototype_dispatch() -> Option<HandlePrototypeDispatchFn> {
    let p = HANDLE_PROTOTYPE_DISPATCH_PTR.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut (), HandlePrototypeDispatchFn>(p) })
    }
}

/// Register a function to handle method calls on handle-based objects.
#[no_mangle]
pub unsafe extern "C" fn js_register_handle_method_dispatch(f: HandleMethodDispatchFn) {
    HANDLE_METHOD_DISPATCH_PTR.store(f as *mut (), Ordering::Release);
}

/// Register an extension method dispatcher. External native crates use this
/// when their handles must coexist with the default stdlib dispatcher.
#[no_mangle]
pub unsafe extern "C" fn js_register_handle_method_dispatch_extension(
    f: HandleMethodDispatchExtensionFn,
) {
    register_extension(&HANDLE_METHOD_EXTENSION_DISPATCH_PTRS, f as *mut ());
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

/// #5437: expando-set getter — see `StreamExpandoSetFn`.
#[inline]
pub fn stream_expando_set() -> Option<StreamExpandoSetFn> {
    let p = STREAM_EXPANDO_SET_PTR.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut (), StreamExpandoSetFn>(p) })
    }
}

/// #5437: register the Web Streams expando-set hook (called by the stdlib at init).
#[no_mangle]
pub unsafe extern "C" fn js_register_stream_expando_set(f: StreamExpandoSetFn) {
    STREAM_EXPANDO_SET_PTR.store(f as *mut (), Ordering::Release);
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

/// Fetch-handle kind-probe getter — see `FetchHandleKindProbeFn`.
#[inline]
pub fn fetch_handle_kind_probe() -> Option<FetchHandleKindProbeFn> {
    let p = FETCH_HANDLE_KIND_PROBE_PTR.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut (), FetchHandleKindProbeFn>(p) })
    }
}

/// Register the fetch-handle kind probe (called by the stdlib at init).
#[no_mangle]
pub unsafe extern "C" fn js_register_fetch_handle_kind_probe(f: FetchHandleKindProbeFn) {
    FETCH_HANDLE_KIND_PROBE_PTR.store(f as *mut (), Ordering::Release);
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
pub fn event_emitter_async_resource_handle_probe() -> Option<EventEmitterAsyncResourceHandleProbeFn>
{
    let p = EVENT_EMITTER_ASYNC_RESOURCE_HANDLE_PROBE_PTR.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut (), EventEmitterAsyncResourceHandleProbeFn>(p) })
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_register_event_emitter_async_resource_handle_probe(
    f: EventEmitterAsyncResourceHandleProbeFn,
) {
    EVENT_EMITTER_ASYNC_RESOURCE_HANDLE_PROBE_PTR.store(f as *mut (), Ordering::Release);
}

#[inline]
pub fn event_emitter_get_domain() -> Option<EventEmitterGetDomainFn> {
    let p = EVENT_EMITTER_GET_DOMAIN_PTR.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut (), EventEmitterGetDomainFn>(p) })
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_register_event_emitter_get_domain(f: EventEmitterGetDomainFn) {
    EVENT_EMITTER_GET_DOMAIN_PTR.store(f as *mut (), Ordering::Release);
}

#[inline]
pub fn event_emitter_set_domain() -> Option<EventEmitterSetDomainFn> {
    let p = EVENT_EMITTER_SET_DOMAIN_PTR.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut (), EventEmitterSetDomainFn>(p) })
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_register_event_emitter_set_domain(f: EventEmitterSetDomainFn) {
    EVENT_EMITTER_SET_DOMAIN_PTR.store(f as *mut (), Ordering::Release);
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
pub fn ffi_handle_exists_probe() -> Option<FfiHandleExistsProbeFn> {
    let p = FFI_HANDLE_EXISTS_PROBE_PTR.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut (), FfiHandleExistsProbeFn>(p) })
    }
}

/// Returns `true` only when a probe is registered AND it confirms `id` is a
/// live `perry-ffi` registry handle. Absent a probe (no native wrapper linked)
/// this is `false`, preserving the prior behavior.
#[inline]
pub fn ffi_handle_exists(id: i64) -> bool {
    match ffi_handle_exists_probe() {
        Some(probe) => unsafe { probe(id) },
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_register_ffi_handle_exists_probe(f: FfiHandleExistsProbeFn) {
    FFI_HANDLE_EXISTS_PROBE_PTR.store(f as *mut (), Ordering::Release);
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

#[no_mangle]
pub unsafe extern "C" fn js_register_handle_property_dispatch_extension(
    f: HandlePropertyDispatchExtensionFn,
) {
    register_extension(&HANDLE_PROPERTY_EXTENSION_DISPATCH_PTRS, f as *mut ());
}

/// Register a function to handle property set on handle-based objects.
#[no_mangle]
pub unsafe extern "C" fn js_register_handle_property_set_dispatch(f: HandlePropertySetDispatchFn) {
    HANDLE_PROPERTY_SET_DISPATCH_PTR.store(f as *mut (), Ordering::Release);
}

#[no_mangle]
pub unsafe extern "C" fn js_register_handle_property_set_dispatch_extension(
    f: HandlePropertySetDispatchExtensionFn,
) {
    register_extension(&HANDLE_PROPERTY_SET_EXTENSION_DISPATCH_PTRS, f as *mut ());
}

/// Register a function to report own property names on handle-backed objects.
#[no_mangle]
pub unsafe extern "C" fn js_register_handle_own_property_names_dispatch(
    f: HandleOwnPropertyNamesDispatchFn,
) {
    HANDLE_OWN_PROPERTY_NAMES_DISPATCH_PTR.store(f as *mut (), Ordering::Release);
}

/// Register a function to resolve prototypes for handle-backed objects.
#[no_mangle]
pub unsafe extern "C" fn js_register_handle_prototype_dispatch(f: HandlePrototypeDispatchFn) {
    HANDLE_PROTOTYPE_DISPATCH_PTR.store(f as *mut (), Ordering::Release);
}
