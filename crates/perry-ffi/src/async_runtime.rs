//! Async-runtime bridge for native bindings (added in v0.5.1 of
//! the perry-ffi v0.5 surface — non-breaking; pure additions).
//!
//! Many wrappers (bcrypt, argon2, ws, mysql2, …) need to do CPU-
//! bound or blocking work without stalling Perry's main thread.
//! perry-stdlib already runs a global tokio runtime + a
//! main-thread-pumped resolution queue for its own modules; this
//! module exposes that surface through a stable C ABI so external
//! wrappers can use the same runtime instead of spawning their
//! own (which would deadlock under contention).
//!
//! # Layered design
//!
//! 1. perry-stdlib provides `#[no_mangle] extern "C"` shims (see
//!    `crates/perry-stdlib/src/perry_ffi_async.rs`).
//! 2. This module declares those symbols `extern "C"` and exposes
//!    safe Rust wrappers — [`JsPromise`] and [`spawn_blocking`].
//! 3. External wrappers depend only on perry-ffi. At link time,
//!    perry-stdlib's archive resolves the `perry_ffi_*` symbols.
//!
//! # Invariants
//!
//! - A `JsPromise` is owned by Perry's runtime arena from
//!   construction onwards. Once resolved or rejected, the
//!   underlying `Promise` is consumed by the awaiter.
//! - The "bits" passed to [`JsPromise::resolve_string`] /
//!   [`JsPromise::reject_string`] are NaN-boxed `JSValue`
//!   representations. The safe wrappers in this module produce
//!   the right bit pattern so wrapper authors don't need to know
//!   the tag values.

use std::ffi::c_void;

use crate::{alloc_string, Promise, StringHeader};

extern "C" {
    fn perry_ffi_promise_new() -> *mut Promise;
    fn perry_ffi_promise_resolve_bits(promise: *mut Promise, bits: u64);
    fn perry_ffi_promise_reject_bits(promise: *mut Promise, bits: u64);
    fn perry_ffi_spawn_blocking(ctx: *mut c_void, invoke: extern "C" fn(*mut c_void));
    fn perry_ffi_spawn_blocking_with_reactor(ctx: *mut c_void, invoke: extern "C" fn(*mut c_void));
}

// NaN-box tags. These values are part of perry-runtime's stable
// `JSValue` representation — they're documented in
// `perry-runtime/src/value.rs` and have not changed since v0.5.0.
// Keeping them duplicated here (vs. importing) is deliberate: the
// perry-ffi semver promise is that `JsPromise::resolve_string`
// produces a string the runtime will read correctly, regardless of
// what perry-runtime renumbers internally. If they ever do
// change, both this module and the perry-stdlib shim move in
// lockstep and the major bumps.
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;

/// Opaque handle to a JS Promise allocated in Perry's arena.
///
/// Constructed via [`JsPromise::new`]. Consumed by exactly one of
/// `resolve_*` / `reject_*` — the underlying Promise is delivered
/// to the awaiter at that point. Returning a `JsPromise` from your
/// FFI function (via [`JsPromise::as_raw`]) is the typical pattern
/// when async work has been spawned and the resolution will
/// happen later.
#[repr(transparent)]
pub struct JsPromise(*mut Promise);

// SAFETY: pointer crosses thread boundaries via the spawn helper
// below; the underlying Promise object is reference-counted by the
// runtime and synchronizes its own state.
unsafe impl Send for JsPromise {}

impl JsPromise {
    /// Allocate a fresh, unresolved Promise.
    pub fn new() -> Self {
        Self(unsafe { perry_ffi_promise_new() })
    }

    /// Wrap a raw `*mut Promise` (e.g. one returned from another
    /// `extern "C"` function). Consumed by exactly one resolution.
    ///
    /// # Safety
    ///
    /// `ptr` must point to a runtime-allocated `Promise` that has
    /// not yet been resolved or rejected.
    pub unsafe fn from_raw(ptr: *mut Promise) -> Self {
        Self(ptr)
    }

    /// Forward the underlying pointer. Use when returning the
    /// promise to the runtime through a `*mut perry_ffi::Promise`
    /// signature.
    pub fn as_raw(&self) -> *mut Promise {
        self.0
    }

    /// Resolve with a string. Allocates a runtime string, NaN-
    /// boxes it as `STRING_TAG`, queues the resolution.
    pub fn resolve_string(self, s: &str) {
        let str_handle = alloc_string(s);
        unsafe { perry_ffi_promise_resolve_bits(self.0, nanbox_string_bits(str_handle.as_raw())) };
    }

    /// Resolve with a boolean. Encoded as `1.0` / `0.0` (Perry's
    /// FFI ABI represents booleans as f64 in async resolution
    /// flows — this matches what perry-stdlib's bcrypt has been
    /// doing since v0.5.0).
    pub fn resolve_bool(self, b: bool) {
        let bits = if b {
            1.0f64.to_bits()
        } else {
            0.0f64.to_bits()
        };
        unsafe { perry_ffi_promise_resolve_bits(self.0, bits) };
    }

    /// Resolve with a number.
    pub fn resolve_number(self, n: f64) {
        unsafe { perry_ffi_promise_resolve_bits(self.0, n.to_bits()) };
    }

    /// Resolve with `undefined`.
    pub fn resolve_undefined(self) {
        unsafe { perry_ffi_promise_resolve_bits(self.0, TAG_UNDEFINED) };
    }

    /// Resolve with `null`.
    pub fn resolve_null(self) {
        unsafe { perry_ffi_promise_resolve_bits(self.0, TAG_NULL) };
    }

    /// Resolve with an arbitrary [`crate::JsValue`]. Used for
    /// resolutions that don't fit the string/bool/number shortcuts
    /// — e.g. async wrappers returning binary data via
    /// [`crate::alloc_bytes`] + [`crate::JsValue::from_string_ptr`],
    /// or returning objects / arrays.
    pub fn resolve(self, value: crate::JsValue) {
        unsafe { perry_ffi_promise_resolve_bits(self.0, value.bits()) };
    }

    /// Reject with an arbitrary [`crate::JsValue`]. Mirror of
    /// [`Self::resolve`].
    pub fn reject(self, value: crate::JsValue) {
        unsafe { perry_ffi_promise_reject_bits(self.0, value.bits()) };
    }

    /// Reject with an error message string. The wrapper layer
    /// produces an Error-shaped JSValue downstream; here we just
    /// pass the raw message bits.
    pub fn reject_string(self, message: &str) {
        let str_handle = alloc_string(message);
        unsafe { perry_ffi_promise_reject_bits(self.0, nanbox_string_bits(str_handle.as_raw())) };
    }
}

/// NaN-box a `*mut StringHeader` as `STRING_TAG`. Public so the
/// occasional wrapper that needs to encode a string into a custom
/// resolution path (e.g. resolving with an array of strings — not
/// covered by `JsPromise::resolve_string`) can construct the bits
/// without re-deriving the constants.
pub fn nanbox_string_bits(ptr: *mut StringHeader) -> u64 {
    STRING_TAG | (ptr as u64 & POINTER_MASK)
}

/// Spawn `f` on Perry's shared tokio runtime (the blocking pool).
///
/// `f` typically does CPU-bound work (hashing, compression, …) and
/// resolves a `JsPromise` from inside. The closure runs on a
/// blocking-pool thread, so it must NOT touch perry-runtime's
/// thread-local arena directly — string allocation through
/// [`alloc_string`] is safe (it round-trips through the runtime),
/// but constructing JSValues by hand on the blocking thread will
/// trigger UB. perry-stdlib's existing wrappers follow the same
/// rule and rely on `JsPromise::resolve_*` to do the allocation
/// at resolution time.
///
/// The future itself doesn't need to be async — `f: FnOnce() ->
/// () + Send + 'static` covers the common "do work, resolve"
/// shape. For tasks that need actual `await`, run the
/// `tokio::runtime::Handle::current().block_on(async { … })`
/// pattern inside the closure.
pub fn spawn_blocking<F>(f: F)
where
    F: FnOnce() + Send + 'static,
{
    // FnOnce is `Sized` only when monomorphized — we need a thin
    // pointer to cross the FFI boundary. Box twice: the inner
    // `Box<dyn FnOnce>` is a fat pointer, the outer `Box<Box<…>>`
    // is thin and can be passed as `*mut c_void`.
    let boxed: Box<dyn FnOnce() + Send> = Box::new(f);
    let thin: Box<Box<dyn FnOnce() + Send>> = Box::new(boxed);
    let ctx = Box::into_raw(thin) as *mut c_void;

    extern "C" fn invoke(ctx: *mut c_void) {
        // SAFETY: `ctx` came from `Box::into_raw` above. We re-box,
        // unwrap, and call once. The closure's `FnOnce` contract
        // means it's safe to consume.
        let thin: Box<Box<dyn FnOnce() + Send>> =
            unsafe { Box::from_raw(ctx as *mut Box<dyn FnOnce() + Send>) };
        let f: Box<dyn FnOnce() + Send> = *thin;
        f();
    }

    unsafe { perry_ffi_spawn_blocking(ctx, invoke) };
}

/// Like [`spawn_blocking`] but the dispatched task carries the
/// runtime's I/O reactor context — required for any closure that
/// drives `TcpStream` / `TcpListener` / WebSocket / hyper / similar
/// async I/O via `tokio::runtime::Handle::current().block_on(fut)`
/// from inside.
///
/// **Why two variants:** the plain `spawn_blocking` puts the closure
/// on a tokio blocking-pool thread. From there, `Handle::current()
/// .block_on(fut)` spins up a fresh current_thread runtime that
/// has no I/O reactor — so any async I/O inside the future panics
/// with "there is no reactor running, must be called from the
/// context of a Tokio 1.x runtime". Pure-CPU work (bcrypt / argon2
/// hashing, SQL serialization, JSON parsing) doesn't notice; pure-
/// async-I/O work (TcpStream::connect, hyper request, WebSocket
/// handshake) hits this hard.
///
/// This variant routes through `RUNTIME.spawn(async {
/// spawn_blocking(closure).await })` so the blocking task inherits
/// the runtime's reactor + handle. Use this when your closure does
/// `Handle::current().block_on(async { ... I/O work ... })`.
///
/// Like the plain variant, this detaches — the caller does not
/// observe completion.
pub fn spawn_blocking_with_reactor<F>(f: F)
where
    F: FnOnce() + Send + 'static,
{
    let boxed: Box<dyn FnOnce() + Send> = Box::new(f);
    let thin: Box<Box<dyn FnOnce() + Send>> = Box::new(boxed);
    let ctx = Box::into_raw(thin) as *mut c_void;

    extern "C" fn invoke(ctx: *mut c_void) {
        let thin: Box<Box<dyn FnOnce() + Send>> =
            unsafe { Box::from_raw(ctx as *mut Box<dyn FnOnce() + Send>) };
        let f: Box<dyn FnOnce() + Send> = *thin;
        f();
    }

    unsafe { perry_ffi_spawn_blocking_with_reactor(ctx, invoke) };
}

#[cfg(test)]
mod tests {
    // The async surface depends on perry-stdlib symbols at link
    // time, which aren't in the perry-ffi unit-test binary. Real
    // integration testing happens in the wrapper crates that
    // exercise the surface end-to-end (perry-ext-bcrypt,
    // perry-ext-argon2). This stub guards against accidental
    // module deletion + makes `cargo test -p perry-ffi` a no-op
    // here rather than a link error.
    #[test]
    fn module_compiles() {}
}
