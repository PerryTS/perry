//! #7764: the four `common::async_bridge` entry points `worker_threads` needs,
//! available in BOTH feature configurations.
//!
//! `common/mod.rs` states the contract this exists to satisfy:
//!
//! > Tokio-backed promise/runtime bridge — only needed when an async feature …
//! > pulls in `async-runtime`. **Always-on code that references it must also be
//! > `#[cfg(feature = "async-runtime")]`-gated.**
//!
//! `worker_threads` is always-on and referenced it in eleven places across five
//! files, so `cargo build -p perry-stdlib --no-default-features` did not
//! compile. That is the configuration the auto-optimize relink uses, so every
//! `perry` compile that triggered auto-optimize fell back to the prebuilt
//! archives with a warning, and ad-hoc builds needed `PERRY_NO_AUTO_OPTIMIZE=1`.
//!
//! Gating each call site individually was not an option: two of them are
//! value-producing (`js_promise_new_for_native_resolution`) and the rest settle
//! a promise, so `#[cfg]` on the statement leaves nothing to return. Gating the
//! whole `worker_threads` module was worse — it has no feature of its own, so
//! its FFI symbols would vanish from the stripped archive and a program that
//! imports `node:worker_threads` would fail to LINK, which is the #7629 family
//! of failure rather than a fix.
//!
//! So: forward when the bridge is compiled in, and settle INLINE when it is not.
//! That is not an invented semantic. The queue exists to hand work to the pump;
//! with no pump there is nothing to hand it to, and doing the same work
//! synchronously reaches the same observable end state (the promise settles).
//! The pinning `js_promise_new_for_native_resolution` performs is likewise a
//! consequence of deferral — it keeps the promise alive across the window
//! between creation and the pump's resolution — and an inline settle spans no
//! collection point, so a plain `js_promise_new_cross_thread` is the correct counterpart.

#[cfg(feature = "async-runtime")]
pub(crate) use crate::common::async_bridge::{
    ensure_pump_registered, js_promise_new_for_native_resolution, queue_deferred_resolution,
    queue_promise_resolution,
};

#[cfg(not(feature = "async-runtime"))]
mod inline {
    /// No bridge means no pump to register.
    pub(crate) fn ensure_pump_registered() {}

    /// # Safety
    /// Mirrors `async_bridge::js_promise_new_for_native_resolution`.
    ///
    /// No pinning: pinning guards the deferral window, and there is none here.
    pub(crate) unsafe fn js_promise_new_for_native_resolution() -> *mut perry_runtime::Promise {
        perry_runtime::js_promise_new_cross_thread()
    }

    /// Settle now rather than queueing for a pump that does not exist.
    pub(crate) fn queue_promise_resolution(promise_ptr: usize, is_success: bool, result_bits: u64) {
        if promise_ptr == 0 {
            return;
        }
        let promise = promise_ptr as *mut perry_runtime::Promise;
        let value = f64::from_bits(result_bits);
        if is_success {
            perry_runtime::js_promise_resolve(promise, value);
        } else {
            perry_runtime::js_promise_reject(promise, value);
        }
    }

    /// As above, running the converter inline. The `Send + 'static` bound is
    /// kept so the two configurations accept the same call sites.
    pub(crate) fn queue_deferred_resolution<F>(promise_ptr: usize, is_success: bool, converter: F)
    where
        F: FnOnce() -> u64 + Send + 'static,
    {
        queue_promise_resolution(promise_ptr, is_success, converter());
    }
}

#[cfg(not(feature = "async-runtime"))]
pub(crate) use inline::{
    ensure_pump_registered, js_promise_new_for_native_resolution, queue_deferred_resolution,
    queue_promise_resolution,
};
