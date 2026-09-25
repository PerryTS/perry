//! EventEmitter methods called on an emitter this provider does not own
//! (#11300). The bundled-events counterpart of perry-ext-events'
//! `foreign_receiver.rs`; see that file for the full story.
//!
//! A value statically typed `EventEmitter` lowers `emitter.on(...)` straight
//! to `js_event_emitter_on(handle, ...)`, but the runtime value can be any
//! EventEmitter. This provider's emitters are small registry handles, so a
//! receiver that is a heap object (a stream, a user subclass, `process`) is
//! someone else's and is re-dispatched by name on the runtime value. For a
//! handle that is one integer compare; the proven-emitter path is otherwise
//! untouched.
//!
//! This cannot recurse with the same receiver: the dynamic dispatcher only
//! routes back here through `dispatch_event_emitter_method`, which is gated on
//! `js_event_emitter_is_handle`, and a heap address is never one of this
//! provider's handles.

use super::*;

/// `return` the foreign-receiver result from the enclosing entry point.
/// The receiver test is inlined at the call site (for a handle this provider
/// owns it is the whole cost); the re-dispatch itself is out of line and
/// `#[cold]`. One line per call site keeps the provider file under the
/// file-size cap.
macro_rules! return_if_foreign {
    ($handle:expr => $result:expr) => {
        if $crate::events::foreign_receiver::is_foreign_receiver($handle) {
            if let Some(result) = $result {
                return result;
            }
        }
    };
}
pub(super) use return_if_foreign;

#[inline(always)]
pub(crate) fn is_foreign_receiver(handle: Handle) -> bool {
    let addr = handle as u64;
    (0x100000..=POINTER_MASK_BITS).contains(&addr) && addr & 0x7 == 0
}

/// Call `name(args...)` on a foreign receiver through the runtime's dynamic
/// method dispatcher; `None` when `handle` may be this provider's.
#[cold]
#[inline(never)]
pub(super) unsafe fn call_fwd(handle: Handle, name: &str, args: &[f64]) -> Option<f64> {
    if !is_foreign_receiver(handle) {
        return None;
    }
    let receiver = js_nanbox_pointer(handle);
    Some(perry_runtime::object::js_native_call_method(
        receiver,
        name.as_ptr() as *const i8,
        name.len(),
        args.as_ptr(),
        args.len(),
    ))
}

/// The listener-registration family, whose native ABI returns the receiver.
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
    let args = [value_from_bits(event_bits), value_from_bits(listener_bits)];
    call_fwd(handle, name, &args).map(|_| handle)
}

/// `removeAllListeners(event?)`, whose native ABI returns the receiver.
#[cold]
#[inline(never)]
pub(super) unsafe fn remove_all_fwd(handle: Handle, rest: *const ArrayHeader) -> Option<Handle> {
    varargs_fwd(handle, "removeAllListeners", None, rest).map(|_| handle)
}

/// A varargs entry point: the optional event, then every element of `rest`.
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
    let mut args: Vec<f64> = event_bits.map(value_from_bits).into_iter().collect();
    if !rest.is_null() {
        for index in 0..js_array_length(rest) {
            args.push(perry_runtime::array::js_array_get_f64(rest, index));
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
    call_fwd(handle, name, &[value_from_bits(event_bits)])
}

/// `listenerCount(event, listener?)`: an absent listener stays absent.
#[cold]
#[inline(never)]
pub(super) unsafe fn count_fwd(handle: Handle, event_bits: i64, listener_bits: i64) -> Option<f64> {
    if !is_foreign_receiver(handle) {
        return None;
    }
    let event = value_from_bits(event_bits);
    if listener_bits as u64 == TAG_UNDEFINED_F64_BITS {
        call_fwd(handle, "listenerCount", &[event])
    } else {
        call_fwd(
            handle,
            "listenerCount",
            &[event, value_from_bits(listener_bits)],
        )
    }
}

/// A foreign `listeners()` / `rawListeners()` / `eventNames()` result as the
/// array pointer the native ABI returns.
pub(super) fn result_array(value: f64) -> *mut ArrayHeader {
    let bits = value.to_bits();
    if (bits >> 48) == (POINTER_TAG_BITS >> 48) && (bits & POINTER_MASK_BITS) != 0 {
        (bits & POINTER_MASK_BITS) as *mut ArrayHeader
    } else {
        js_array_alloc(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_aligned_heap_addresses_are_foreign() {
        assert!(is_foreign_receiver(0x7f00_0000_1000));
        assert!(!is_foreign_receiver(0x7f00_0000_1001));
        // Registry handles and the payloads of undefined/null/booleans.
        for payload in [0, 1, 2, 3, 4, 0x38000, 0xFFFFF] {
            assert!(!is_foreign_receiver(payload), "{payload:#x}");
        }
    }
}
