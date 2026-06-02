//! JavaScript closure invocation across the FFI boundary (added
//! in v0.5.x of the perry-ffi v0.5 surface — non-breaking; pure
//! additions).
//!
//! Many wrappers need to call back into TypeScript-side
//! functions:
//!
//! - `db.transaction(fn)` (better-sqlite3) — wrap user code in
//!   BEGIN/COMMIT;
//! - `events.on('change', listener)` (events) — invoke listeners
//!   with an event object;
//! - `commander.action(fn)` (CLI) — fire the user's command
//!   handler;
//! - `ws.on('message', cb)` (websockets) — push payloads up;
//! - `cron.schedule(expr, fn)` — invoke the cron handler;
//! - `backOff(fn, options)` (exponential-backoff) — retry the
//!   user's async call.
//!
//! All of these consume a `*const ClosureHeader` (the runtime's
//! closure layout) and call it via `js_closure_call0` /
//! `js_closure_call1` / etc. — perry-runtime exports those as
//! `extern "C"` already, so perry-ffi just declares them and
//! exposes a typed [`JsClosure`] wrapper.
//!
//! # Argument / return ABI
//!
//! Closures cross the FFI boundary as raw f64 values — Perry's
//! NaN-boxing means a single 64-bit register can carry any JS
//! value. Wrapper authors construct arguments via [`crate::JsValue`]
//! and decode return values the same way.
//!
//! # Capture access
//!
//! When wrappers need to construct a *new* closure that captures
//! state (e.g. db.transaction's BEGIN/COMMIT wrapper), they use
//! [`alloc_closure_with_captures`] + the per-slot setters. See
//! the better-sqlite3 wrapper's transaction support for a
//! reference example (added under #466 Phase 5 followup).

use crate::ClosureHeader;

pub use crate::ClosureHeader as RawClosureHeader;

extern "C" {
    fn js_closure_call0(closure: *const ClosureHeader) -> f64;
    fn js_closure_call1(closure: *const ClosureHeader, arg0: f64) -> f64;
    fn js_closure_call2(closure: *const ClosureHeader, arg0: f64, arg1: f64) -> f64;
    fn js_closure_call3(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64) -> f64;
    fn js_closure_call4(
        closure: *const ClosureHeader,
        arg0: f64,
        arg1: f64,
        arg2: f64,
        arg3: f64,
    ) -> f64;
}

/// Opaque handle to a JS closure (a `*const ClosureHeader`).
///
/// Wrapper authors receive a `*const ClosureHeader` from their
/// FFI parameter list, convert it via [`JsClosure::from_raw`],
/// then call zero through four args via the `call_*` methods.
/// Beyond 4 args, drop down to the raw extern fns —
/// perry-runtime exports `js_closure_call5..8` for completeness;
/// they're not yet wrapped here because no wrapper needs them.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct JsClosure(*const ClosureHeader);

// SAFETY: the underlying ClosureHeader is reference-counted by
// the runtime; passing the pointer across thread boundaries is
// fine as long as the runtime guarantees the header survives.
unsafe impl Send for JsClosure {}

impl JsClosure {
    /// Wrap a raw `*const ClosureHeader` from an FFI parameter.
    ///
    /// # Safety
    ///
    /// `ptr` must be null or point to a valid runtime-allocated
    /// `ClosureHeader`. Callers can pass null to indicate "no
    /// callback" — `is_null` lets you check before invoking.
    pub unsafe fn from_raw(ptr: *const ClosureHeader) -> Self {
        Self(ptr)
    }

    /// True if the closure handle is null. Wrappers should check
    /// before calling — invoking a null closure is undefined.
    pub fn is_null(self) -> bool {
        self.0.is_null()
    }

    /// Forward the underlying pointer. Used when a wrapper
    /// re-exports a closure to TypeScript without invoking it.
    pub fn as_raw(self) -> *const ClosureHeader {
        self.0
    }

    /// Invoke the closure with no arguments. Returns the result
    /// as a NaN-boxed f64 (the runtime's standard return ABI for
    /// dynamic JS calls).
    ///
    /// # Safety
    ///
    /// `self.0` must point to a live closure that has not been
    /// freed or retired. The closure's body may call back into
    /// the runtime / arena, so callers must not hold any
    /// references that would alias with allocations the closure
    /// may make.
    pub unsafe fn call0(self) -> f64 {
        js_closure_call0(self.0)
    }

    /// Invoke with one argument. See [`Self::call0`] safety.
    pub unsafe fn call1(self, arg0: f64) -> f64 {
        js_closure_call1(self.0, arg0)
    }

    /// Invoke with two arguments. See [`Self::call0`] safety.
    pub unsafe fn call2(self, arg0: f64, arg1: f64) -> f64 {
        js_closure_call2(self.0, arg0, arg1)
    }

    /// Invoke with three arguments. See [`Self::call0`] safety.
    pub unsafe fn call3(self, arg0: f64, arg1: f64, arg2: f64) -> f64 {
        js_closure_call3(self.0, arg0, arg1, arg2)
    }

    /// Invoke with four arguments. See [`Self::call0`] safety.
    pub unsafe fn call4(self, arg0: f64, arg1: f64, arg2: f64, arg3: f64) -> f64 {
        js_closure_call4(self.0, arg0, arg1, arg2, arg3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_closure_predicates() {
        let null = unsafe { JsClosure::from_raw(std::ptr::null()) };
        assert!(null.is_null());
        assert!(null.as_raw().is_null());
    }
}
