//! Observe an actual JS Promise without running jobs or retaining raw values in C.
use super::*;

/// Poll one rooted Promise slot. Returns 0 while pending, 1 with a fresh rooted
/// result slot when fulfilled, or -1 with the original rejection in the frame's
/// error slot. Observation marks the Promise handled. This never pumps jobs,
/// waits for I/O or calls a thenable; the guest task scheduler owns progress.
///
/// # Safety
/// `pointer` is an open frame on its creating agent. `output` is null or valid
/// writable C storage for one slot index; null is recorded as a frame error.
/// No other operation may simultaneously borrow the frame or output storage.
#[no_mangle]
pub unsafe extern "C" fn perry_component_frame_promise_poll(
    pointer: *mut c_void,
    promise_slot: u32,
    output: *mut u32,
) -> i32 {
    if !output.is_null() {
        *output = 0;
    }
    let owner = frame(pointer);
    let status = owner.guard(|owner| {
        if output.is_null() {
            fail("Missing component Promise output slot");
        }
        let value = owner.get(promise_slot);
        if crate::promise::js_value_is_promise(value) == 0 {
            fail("Expected a Promise in the component frame");
        }
        let promise = JSValue::from_bits(value.to_bits())
            .as_pointer::<crate::promise::Promise>()
            .cast_mut();
        // This changes native rejection bookkeeping only; no hooks or jobs run.
        crate::promise::js_promise_mark_internally_handled(promise);
        match (*promise).state {
            crate::promise::PromiseState::Pending => 0,
            crate::promise::PromiseState::Fulfilled => {
                // add roots its argument before its backing array can grow.
                *output = owner.add((*promise).value);
                1
            }
            crate::promise::PromiseState::Rejected => {
                crate::exception::js_throw((*promise).reason);
            }
        }
    });
    if owner.failed {
        -1
    } else {
        status
    }
}
