//! EventEmitter methods called on an emitter this provider does not own
//! (#11300).
//!
//! A value whose static type is `EventEmitter` (a parameter annotated
//! `emitter: EventEmitter`, or a local typed from such a return) lowers
//! `emitter.on(...)` straight to `js_event_emitter_on(handle, ...)`. The
//! annotation is not a proof: the runtime value can be ANY EventEmitter, such
//! as a stream (a Transform or PassThrough), a `net.Socket`, `process`, or a
//! user subclass. Only handles in this provider's id band are ours. Anything
//! else used to miss the registry lookup and silently do nothing, so mongodb's
//! `onData(emitter: EventEmitter, ...)` never saw a reply from its message
//! stream.
//!
//! Every method entry point therefore asks [`call_fwd`] first. For an
//! in-band handle that is two integer compares, and the proven-emitter path
//! is otherwise untouched. A foreign receiver is re-dispatched by name on the
//! runtime value, exactly as an untyped call would be.

use super::*;

/// `return` the foreign-receiver result from the enclosing entry point.
/// The receiver test is inlined at the call site (for a handle this provider
/// owns it is the whole cost); the re-dispatch itself is out of line and
/// `#[cold]`. One line per call site keeps the provider file under the
/// file-size cap.
macro_rules! return_if_foreign {
    ($handle:expr => $result:expr) => {
        if !$crate::foreign_receiver::in_own_band($handle) {
            if let Some(result) = $result {
                return result;
            }
        }
    };
}
pub(super) use return_if_foreign;

#[inline(always)]
pub(crate) fn in_own_band(handle: Handle) -> bool {
    (EVENT_EMITTER_HANDLE_ID_START..EVENT_EMITTER_HANDLE_ID_END).contains(&handle)
}

/// Is `handle` (the receiver's NaN-box payload) an EventEmitter owned by
/// someone else? Heap objects (streams, user subclasses, `process`) are
/// recognized by address. Small ids must belong to a live registry, so a
/// payload that came from `undefined`/`null` keeps today's no-op.
unsafe fn is_foreign_receiver(handle: Handle) -> bool {
    if in_own_band(handle) || handle <= 0 {
        return false;
    }
    let addr = handle as u64;
    if (0x100000..=MAX_HEAP_POINTER).contains(&addr) {
        return true;
    }
    extern "C" {
        fn js_is_registered_net_socket_handle(handle: i64) -> i32;
        fn js_is_registered_ffi_handle(handle: i64) -> i32;
    }
    js_is_registered_net_socket_handle(handle) != 0 || js_is_registered_ffi_handle(handle) != 0
}

/// Call `name(args...)` on a foreign receiver through the runtime's dynamic
/// method dispatcher. Returns `None` when `handle` is this provider's, and
/// the caller then runs its native body.
///
/// This cannot recurse back into this provider with the same receiver: every
/// runtime path into these entry points (the dynamic dispatcher's emitter
/// arm, the stream `on` hook) is gated on `js_event_emitter_is_handle`,
/// i.e. on this provider's own band, and a foreign receiver is outside it.
/// There is deliberately no in-flight guard: a JS throw out of a listener
/// longjmps past Rust frames, so a guard's cleanup would be skipped and it
/// would disable foreign dispatch for that receiver for good.
#[cold]
#[inline(never)]
pub(super) unsafe fn call_fwd(handle: Handle, name: &str, args: &[f64]) -> Option<f64> {
    if !is_foreign_receiver(handle) {
        return None;
    }
    Some(call_net_socket_method(handle, name, args))
}

/// `call_fwd` for the listener-registration family (`on`, `once`,
/// `prependListener`, ...), whose native ABI returns the receiver handle.
#[cold]
#[inline(never)]
pub(super) unsafe fn listener_fwd(
    handle: Handle,
    name: &str,
    event_bits: i64,
    listener_bits: i64,
) -> Option<Handle> {
    if !is_foreign_receiver(handle) {
        return None;
    }
    let args = [
        event_value_from_bits(event_bits),
        f64::from_bits(listener_bits as u64),
    ];
    call_fwd(handle, name, &args).map(|_| handle)
}

/// `removeAllListeners(event?)`, whose native ABI returns the receiver.
#[cold]
#[inline(never)]
pub(super) unsafe fn remove_all_fwd(handle: Handle, rest: *const ArrayHeader) -> Option<Handle> {
    varargs_fwd(handle, "removeAllListeners", None, rest).map(|_| handle)
}

/// A varargs entry point (`emit`, `removeAllListeners`): the optional event,
/// then every element of `rest`.
#[cold]
#[inline(never)]
pub(super) unsafe fn varargs_fwd(
    handle: Handle,
    name: &str,
    event_bits: Option<i64>,
    rest: *const ArrayHeader,
) -> Option<f64> {
    if !is_foreign_receiver(handle) {
        return None;
    }
    let mut args: Vec<f64> = event_bits.map(event_value_from_bits).into_iter().collect();
    if !rest.is_null() {
        for index in 0..js_array_length(rest) {
            args.push(f64::from_bits(js_array_get(rest, index).bits()));
        }
    }
    call_fwd(handle, name, &args)
}

/// A one-argument entry point whose argument is the event name.
#[cold]
#[inline(never)]
pub(super) unsafe fn event_fwd(handle: Handle, name: &str, event_bits: i64) -> Option<f64> {
    if !is_foreign_receiver(handle) {
        return None;
    }
    call_fwd(handle, name, &[event_value_from_bits(event_bits)])
}

/// `listenerCount(event, listener?)`: an absent listener must stay absent,
/// not become an explicit `undefined`.
#[cold]
#[inline(never)]
pub(super) unsafe fn count_fwd(handle: Handle, event_bits: i64, listener_bits: i64) -> Option<f64> {
    if !is_foreign_receiver(handle) {
        return None;
    }
    let event = event_value_from_bits(event_bits);
    let listener = f64::from_bits(listener_bits as u64);
    if listener.to_bits() == TAG_UNDEFINED_F64_BITS {
        call_fwd(handle, "listenerCount", &[event])
    } else {
        call_fwd(handle, "listenerCount", &[event, listener])
    }
}

/// A foreign `listeners()` / `rawListeners()` / `eventNames()` result as the
/// array pointer the native ABI returns.
pub(super) fn result_array(value: f64) -> *mut ArrayHeader {
    let bits = value.to_bits();
    if (bits >> 48) == (POINTER_TAG >> 48) && (bits & POINTER_MASK) != 0 {
        (bits & POINTER_MASK) as *mut ArrayHeader
    } else {
        unsafe { js_array_alloc(0) }
    }
}
