//! `Bun.sleep(ms | Date)` and `Bun.sleepSync(ms)`.
//!
//! `Bun.sleep` resolves a Promise with `undefined` once `ms` milliseconds have
//! passed, or once the given `Date` is reached (a past Date resolves on the
//! next timer turn). It is the same ref'ed timer `setTimeout` arms, so a
//! pending sleep keeps the event loop alive, as in Bun. `Bun.sleepSync`
//! blocks the calling thread.

use crate::value::TAG_UNDEFINED;

/// Milliseconds to wait for a `Bun.sleep` argument: a Date waits until its
/// time value, anything else is `ToNumber`; NaN and negatives wait 0.
fn sleep_delay_ms(value: f64) -> f64 {
    let delay = if crate::date::is_date_value(value) {
        crate::date::date_cell_timestamp(value) - crate::clock::wall_millis()
    } else {
        crate::builtins::js_number_coerce(value)
    };
    if delay.is_nan() || delay < 0.0 {
        0.0
    } else {
        delay
    }
}

#[no_mangle]
pub extern "C" fn js_bun_sleep(value: f64) -> f64 {
    let delay = sleep_delay_ms(value);
    let promise = crate::timer::js_set_timeout_value_ref(delay, f64::from_bits(TAG_UNDEFINED), 1);
    crate::value::js_nanbox_pointer(promise as i64)
}

#[no_mangle]
pub extern "C" fn js_bun_sleep_sync(value: f64) -> f64 {
    let delay = sleep_delay_ms(value);
    if delay > 0.0 && delay.is_finite() {
        std::thread::sleep(std::time::Duration::from_micros((delay * 1000.0) as u64));
    }
    f64::from_bits(TAG_UNDEFINED)
}
