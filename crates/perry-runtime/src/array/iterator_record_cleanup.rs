//! Shared completion of compiler-owned array iterator records.
use crate::value::TAG_TRUE;

#[no_mangle]
pub unsafe extern "C-unwind" fn js_array_record_finish(
    protocol: f64,
    source: f64,
    index: f64,
    iterator: f64,
    done: f64,
    error: f64,
    throwing: f64,
) -> f64 {
    if protocol.to_bits() == TAG_TRUE {
        if throwing.to_bits() == TAG_TRUE {
            super::js_iterator_close_on_throw(iterator, done, error)
        } else {
            super::js_iterator_close_if_not_done(iterator, done)
        }
    } else {
        super::js_array_record_close(source, index, done, error, throwing)
    }
}

/// The generated landing pad keeps only the record operands and root release.
/// Exception capture, close-on-throw and rethrow have one runtime body.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_array_record_abrupt(
    protocol: f64,
    source: f64,
    index: f64,
    iterator: f64,
    state: f64,
) -> ! {
    let scope = crate::gc::RuntimeHandleScope::new();
    let source = scope.root_nanbox_f64(source);
    let iterator = scope.root_nanbox_f64(iterator);
    let error = scope.root_nanbox_f64(crate::exception::js_get_exception());
    crate::exception::js_clear_exception();
    let error = if state != 2.0 {
        js_array_record_finish(
            protocol,
            source.get_nanbox_f64(),
            index,
            iterator.get_nanbox_f64(),
            f64::from_bits(crate::value::TAG_FALSE),
            error.get_nanbox_f64(),
            f64::from_bits(TAG_TRUE),
        )
    } else {
        error.get_nanbox_f64()
    };
    crate::exception::js_throw(error)
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_ARRAY_RECORD_FINISH: unsafe extern "C-unwind" fn(
    f64,
    f64,
    f64,
    f64,
    f64,
    f64,
    f64,
) -> f64 = js_array_record_finish;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_ARRAY_RECORD_ABRUPT: unsafe extern "C-unwind" fn(f64, f64, f64, f64, f64) -> ! =
    js_array_record_abrupt;
