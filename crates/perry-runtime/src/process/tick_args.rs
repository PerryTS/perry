//! Shared process.nextTick and emitted-argument decoding.
use crate::array::ArrayHeader;

pub(crate) fn collect_emit_args(args: *const ArrayHeader) -> Vec<f64> {
    if args.is_null() {
        return Vec::new();
    }
    let len = crate::array::js_array_length(args) as usize;
    let mut values = Vec::with_capacity(len);
    for i in 0..len {
        values.push(crate::array::js_array_get_f64(args, i as u32));
    }
    values
}

/// process.nextTick(callback, ...args) — schedule callback as a tick,
/// forwarding trailing args (#3046). `callback_bits`/`args` are raw
/// NaN-boxed values. A non-callable callback throws Node's
/// `TypeError [ERR_INVALID_ARG_TYPE]` (`"callback"` message) synchronously.
/// Used by the method-value dispatch path (`const nt = process.nextTick`)
/// and the zero-arg `process.nextTick()` call form; the direct lowered form
/// with trailing args goes through codegen's `js_queue_next_tick_args`.
///
/// # Safety
/// `args` must be a valid NaN-boxed args array pointer, or null.
#[no_mangle]
pub unsafe extern "C" fn js_process_next_tick(callback_bits: i64, args: *const ArrayHeader) {
    // Mirror codegen's setTimeout/queueMicrotask validation: the timer
    // validator always reports the `"callback"` argument name, which is the
    // wording Node uses for `process.nextTick`.
    let callback =
        crate::timer::js_timer_validate_callback(f64::from_bits(callback_bits as u64), 3);
    let values = collect_emit_args(args);
    if values.is_empty() {
        crate::builtins::js_queue_next_tick(callback);
    } else {
        crate::builtins::js_queue_next_tick_args(callback, values.as_ptr(), values.len() as i32);
    }
}
