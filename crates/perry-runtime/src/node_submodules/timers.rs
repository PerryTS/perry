//! `node:timers/promises` + `node:timers` namespace thunks (#1213).
//!
//! Extracted from `mod.rs` so the parent module stays under the file-size
//! gate. Pure code movement — no logic changes.

use super::TAG_UNDEFINED;
use crate::closure::ClosureHeader;

/// node:timers/promises.setTimeout(delay, value?) — a Promise that resolves
/// with `value` (or undefined) after `delay` ms. Composes the existing
/// promise-returning timer primitive; the closure dispatch pads a missing
/// `value` arg with undefined (arity registered in `ensure_export_singleton`).
/// Refs #1213.
pub(crate) extern "C" fn timers_promises_set_timeout(
    _closure: *const ClosureHeader,
    delay_ms: f64,
    value: f64,
) -> f64 {
    let promise = crate::timer::js_set_timeout_value(delay_ms, value);
    crate::value::js_nanbox_pointer(promise as i64)
}

/// node:timers/promises.setImmediate(value?) — a Promise that resolves with
/// `value` (or undefined) on a later turn. Refs #1213.
pub(crate) extern "C" fn timers_promises_set_immediate(
    _closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    let promise = crate::timer::js_set_timeout_value(0.0, value);
    crate::value::js_nanbox_pointer(promise as i64)
}

// ── node:timers namespace (`import * as timers from "node:timers"`) ──────────
// Route to the SAME global timer runtime fns the bare globals use, so
// `timers.setTimeout(...)` matches `setTimeout(...)`. NOTE: named imports
// (`import { setTimeout } from "node:timers"`) deliberately bypass this and
// keep the codegen global fast-path (which handles `setTimeout(fn, delay,
// ...args)` varargs) — compile.rs skips registering node:timers named imports
// as submodule exports. Refs #1213.
fn callback_arg_to_i64(v: f64) -> i64 {
    (v.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64
}
pub(crate) extern "C" fn timers_ns_set_timeout(_c: *const ClosureHeader, cb: f64, ms: f64) -> f64 {
    crate::value::js_nanbox_pointer(crate::timer::js_set_timeout_callback(
        callback_arg_to_i64(cb),
        ms,
    ))
}
pub(crate) extern "C" fn timers_ns_set_interval(_c: *const ClosureHeader, cb: f64, ms: f64) -> f64 {
    crate::value::js_nanbox_pointer(crate::timer::setInterval(callback_arg_to_i64(cb), ms))
}
pub(crate) extern "C" fn timers_ns_set_immediate(_c: *const ClosureHeader, cb: f64) -> f64 {
    crate::value::js_nanbox_pointer(crate::timer::js_set_immediate_callback(
        callback_arg_to_i64(cb),
    ))
}
pub(crate) extern "C" fn timers_ns_clear_timeout(_c: *const ClosureHeader, arg: f64) -> f64 {
    crate::timer::js_clear_timeout_value(arg);
    f64::from_bits(TAG_UNDEFINED)
}
pub(crate) extern "C" fn timers_ns_clear_interval(_c: *const ClosureHeader, arg: f64) -> f64 {
    crate::timer::js_clear_interval_value(arg);
    f64::from_bits(TAG_UNDEFINED)
}
// Immediates live in the shared timer pool; clearTimeout retains-out both pools.
pub(crate) extern "C" fn timers_ns_clear_immediate(_c: *const ClosureHeader, arg: f64) -> f64 {
    crate::timer::js_clear_timeout_value(arg);
    f64::from_bits(TAG_UNDEFINED)
}

thunk!(
    thunk_timers_setInterval,
    "node:timers/promises.setInterval is not yet implemented in Perry (tracked by issue #793)."
);
thunk!(
    thunk_timers_scheduler,
    "node:timers/promises.scheduler is not yet implemented in Perry (tracked by issue #793)."
);
