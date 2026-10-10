//! Shared completion of compiler-owned array iterator records.

// Completion flags carry the already-constructed record predicates:
// bit 0 selects the protocol, bit 1 is done, bit 2 is throw completion.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_array_record_finish(
    receiver: f64,
    index: f64,
    error: f64,
    flags: u32,
) -> f64 {
    let throwing = flags & 4 != 0;
    if flags & 2 != 0 {
        return if throwing {
            error
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        };
    }
    if flags & 1 != 0 {
        let done = f64::from_bits(crate::value::TAG_FALSE);
        if throwing {
            super::js_iterator_close_on_throw(receiver, done, error)
        } else {
            super::js_iterator_close_if_not_done(receiver, done)
        }
    } else {
        super::iterator_step::array_record_close(receiver, index, false, error, throwing)
    }
}

/// The generated landing pad keeps only the record operands and root release.
/// Exception capture, close-on-throw and rethrow have one runtime body.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_array_record_abrupt(
    receiver: f64,
    index: f64,
    state: f64,
    flags: u32,
) -> ! {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let error = scope.root_nanbox_f64(crate::exception::js_get_exception());
    crate::exception::js_clear_exception();
    let error = if state != 2.0 {
        js_array_record_finish(
            receiver.get_nanbox_f64(),
            index,
            error.get_nanbox_f64(),
            flags | 4,
        )
    } else {
        error.get_nanbox_f64()
    };
    crate::exception::js_throw(error)
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_ARRAY_RECORD_FINISH: unsafe extern "C-unwind" fn(f64, f64, f64, u32) -> f64 =
    js_array_record_finish;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_ARRAY_RECORD_ABRUPT: unsafe extern "C-unwind" fn(f64, f64, f64, u32) -> ! =
    js_array_record_abrupt;
