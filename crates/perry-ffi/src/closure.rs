//! JavaScript closure invocation across the FFI boundary.
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
//! `js_closure_call1` / etc., passing the receiver (`this`) after the
//! closure — perry-runtime exports those as `extern "C"`, so perry-ffi
//! declares them and exposes a typed [`JsClosure`] wrapper.
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

/// The receiver every native closure body takes as its SECOND parameter,
/// and every call into a JS function passes: the NaN-boxed `this` bits, in
/// an integer register ([`JsThis::UNDEFINED`] for a plain call). Defined
/// once in perry-abi and shared with the runtime.
pub use perry_abi::JsThis;

/// The JS body types (`perry_abi::js_body_fn_ty!`) over perry-ffi's closure
/// header: `JsBody1` is
/// `unsafe extern "C" fn(*const RawClosureHeader, JsThis, f64) -> f64`.
/// [`alloc_closure`] and [`register_closure_arity`] take one of these, so a
/// body with any other signature — including a bare `*const u8` — does not
/// compile:
///
/// ```ignore
/// extern "C" fn body(c: *const RawClosureHeader, this: JsThis, a0: f64) -> f64 { .. }
/// let closure = perry_ffi::alloc_closure(body as perry_ffi::JsBody1, 0);
/// ```
///
/// A JS body declaring 0 JS parameters.
pub type JsBody0 = perry_abi::JsBody0<ClosureHeader>;
/// A JS body declaring 1 JS parameters.
pub type JsBody1 = perry_abi::JsBody1<ClosureHeader>;
/// A JS body declaring 2 JS parameters.
pub type JsBody2 = perry_abi::JsBody2<ClosureHeader>;
/// A JS body declaring 3 JS parameters.
pub type JsBody3 = perry_abi::JsBody3<ClosureHeader>;
/// A JS body declaring 4 JS parameters.
pub type JsBody4 = perry_abi::JsBody4<ClosureHeader>;
/// A JS body declaring 5 JS parameters.
pub type JsBody5 = perry_abi::JsBody5<ClosureHeader>;
/// A JS body declaring 6 JS parameters.
pub type JsBody6 = perry_abi::JsBody6<ClosureHeader>;
/// A JS body declaring 7 JS parameters.
pub type JsBody7 = perry_abi::JsBody7<ClosureHeader>;
/// A JS body declaring 8 JS parameters.
pub type JsBody8 = perry_abi::JsBody8<ClosureHeader>;
/// A JS body declaring 9 JS parameters.
pub type JsBody9 = perry_abi::JsBody9<ClosureHeader>;
/// A JS body declaring 10 JS parameters.
pub type JsBody10 = perry_abi::JsBody10<ClosureHeader>;
/// A JS body declaring 11 JS parameters.
pub type JsBody11 = perry_abi::JsBody11<ClosureHeader>;
/// A JS body declaring 12 JS parameters.
pub type JsBody12 = perry_abi::JsBody12<ClosureHeader>;
/// A JS body declaring 13 JS parameters.
pub type JsBody13 = perry_abi::JsBody13<ClosureHeader>;
/// A JS body declaring 14 JS parameters.
pub type JsBody14 = perry_abi::JsBody14<ClosureHeader>;
/// A JS body declaring 15 JS parameters.
pub type JsBody15 = perry_abi::JsBody15<ClosureHeader>;
/// A JS body declaring 16 JS parameters.
pub type JsBody16 = perry_abi::JsBody16<ClosureHeader>;
/// A JS body of any arity over perry-ffi's closure header.
pub use perry_abi::JsBody;

extern "C" {
    fn js_closure_call0(closure: *const ClosureHeader, this: JsThis) -> f64;
    fn js_closure_call1(closure: *const ClosureHeader, this: JsThis, arg0: f64) -> f64;
    fn js_closure_call2(closure: *const ClosureHeader, this: JsThis, arg0: f64, arg1: f64) -> f64;
    fn js_closure_call3(
        closure: *const ClosureHeader,
        this: JsThis,
        arg0: f64,
        arg1: f64,
        arg2: f64,
    ) -> f64;
    fn js_closure_call4(
        closure: *const ClosureHeader,
        this: JsThis,
        arg0: f64,
        arg1: f64,
        arg2: f64,
        arg3: f64,
    ) -> f64;
    fn js_closure_call_array(
        closure: *const ClosureHeader,
        this: JsThis,
        args: *const f64,
        args_len: i64,
    ) -> f64;
    fn js_native_call_value(
        func_value: f64,
        this: JsThis,
        args: *const f64,
        args_len: usize,
    ) -> f64;
    fn js_closure_alloc(func_ptr: *const u8, capture_count: u32) -> *mut ClosureHeader;
    fn js_register_closure_arity(func_ptr: *const u8, arity: u32);
    fn js_register_closure_rest(func_ptr: *const u8, fixed_arity: u32);
    fn js_closure_get_capture_f64(closure: *const ClosureHeader, index: u32) -> f64;
    fn js_closure_set_capture_f64(closure: *mut ClosureHeader, index: u32, value: f64);
}

/// Register the arity the runtime uses when dispatching a native closure
/// body (a rest-bundling body registers its fixed arity).
pub fn register_closure_arity<F: JsBody<ClosureHeader>>(func: F, arity: u32) {
    unsafe { js_register_closure_arity(func.code(), arity) }
}

/// Register `func` as a rest-parameter body: the runtime bundles every JS
/// argument from index `fixed_arity` on into one array, passed as the body's
/// last parameter (so a body with `fixed_arity` fixed parameters is a
/// `JsBody{fixed_arity + 1}`).
pub fn register_closure_rest<F: JsBody<ClosureHeader>>(func: F, fixed_arity: u32) {
    debug_assert_eq!(
        F::ARITY,
        fixed_arity + 1,
        "a rest body takes its fixed parameters and the rest array"
    );
    unsafe { js_register_closure_rest(func.code(), fixed_arity) }
}

/// Allocate a native closure running `func` with `capture_count` f64
/// capture slots.
///
/// `func` must be a JS body pointer type; anything else does not compile.
/// A body without the receiver:
///
/// ```compile_fail,E0605
/// // NOT-A-JS-BODY: the example of a body missing its receiver.
/// extern "C" fn no_this(_: *const perry_ffi::RawClosureHeader, a: f64) -> f64 { a }
/// let _ = perry_ffi::alloc_closure(no_this as perry_ffi::JsBody1, 0);
/// ```
///
/// an erased pointer:
///
/// ```compile_fail,E0277
/// extern "C" fn body(_: *const perry_ffi::RawClosureHeader, _: perry_ffi::JsThis) -> f64 { 0.0 }
/// let _ = perry_ffi::alloc_closure(body as *const u8, 0);
/// ```
///
/// a JS argument of the wrong type:
///
/// ```compile_fail,E0605
/// extern "C" fn body(_: *const perry_ffi::RawClosureHeader, _: perry_ffi::JsThis, a: i64) -> f64 { 0.0 }
/// let _ = perry_ffi::alloc_closure(body as perry_ffi::JsBody1, 0);
/// ```
pub fn alloc_closure<F: JsBody<ClosureHeader>>(func: F, capture_count: u32) -> *mut ClosureHeader {
    unsafe { js_closure_alloc(func.code(), capture_count) }
}

/// Read an f64 capture slot from a native closure.
///
/// # Safety
/// `closure` must point to a live closure with an allocated `index` slot.
pub unsafe fn closure_capture_f64(closure: *const ClosureHeader, index: u32) -> f64 {
    js_closure_get_capture_f64(closure, index)
}

/// Write an f64 capture slot in a native closure.
///
/// # Safety
/// `closure` must point to a live closure with an allocated `index` slot.
pub unsafe fn set_closure_capture_f64(closure: *mut ClosureHeader, index: u32, value: f64) {
    js_closure_set_capture_f64(closure, index, value)
}

/// Call any JS value as a function with receiver `this`
/// ([`JsThis::UNDEFINED`] for a plain call): what `func.call(this, ...args)`
/// does. A value that is not callable throws a JS `TypeError`.
///
/// # Safety
/// As [`JsClosure::call0`]; `args` must stay valid for the call.
pub unsafe fn call_value(func: f64, this: JsThis, args: &[f64]) -> f64 {
    js_native_call_value(func, this, args.as_ptr(), args.len())
}

/// Opaque handle to a JS closure (a `*const ClosureHeader`).
///
/// Wrapper authors receive a `*const ClosureHeader` from their
/// FFI parameter list, convert it via [`JsClosure::from_raw`],
/// then call it through the `call*` methods, each taking the receiver
/// first ([`JsThis::UNDEFINED`] for a plain call).
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

    /// Invoke the closure with no arguments and receiver `this`
    /// ([`JsThis::UNDEFINED`] for a plain call; a sloppy body sees
    /// globalThis). Returns the result as a NaN-boxed f64 (the runtime's
    /// standard return ABI for dynamic JS calls).
    ///
    /// # Safety
    ///
    /// `self.0` must point to a live closure that has not been
    /// freed or retired. The closure's body may call back into
    /// the runtime / arena, so callers must not hold any
    /// references that would alias with allocations the closure
    /// may make.
    pub unsafe fn call0(self, this: JsThis) -> f64 {
        js_closure_call0(self.0, this)
    }

    /// Invoke with one argument. See [`Self::call0`].
    pub unsafe fn call1(self, this: JsThis, arg0: f64) -> f64 {
        js_closure_call1(self.0, this, arg0)
    }

    /// Invoke with two arguments. See [`Self::call0`].
    pub unsafe fn call2(self, this: JsThis, arg0: f64, arg1: f64) -> f64 {
        js_closure_call2(self.0, this, arg0, arg1)
    }

    /// Invoke with three arguments. See [`Self::call0`].
    pub unsafe fn call3(self, this: JsThis, arg0: f64, arg1: f64, arg2: f64) -> f64 {
        js_closure_call3(self.0, this, arg0, arg1, arg2)
    }

    /// Invoke with four arguments. See [`Self::call0`].
    pub unsafe fn call4(self, this: JsThis, arg0: f64, arg1: f64, arg2: f64, arg3: f64) -> f64 {
        js_closure_call4(self.0, this, arg0, arg1, arg2, arg3)
    }

    /// Invoke with any number of arguments. See [`Self::call0`].
    pub unsafe fn call_slice(self, this: JsThis, args: &[f64]) -> f64 {
        js_closure_call_array(self.0, this, args.as_ptr(), args.len() as i64)
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

    #[cfg(feature = "runtime-link")]
    #[test]
    fn native_closure_retains_capture() {
        unsafe extern "C" fn callback(_: *const ClosureHeader, _this: crate::JsThis) -> f64 {
            0.0
        }
        register_closure_arity(callback as JsBody0, 0);
        let closure = alloc_closure(callback as JsBody0, 1);
        unsafe { set_closure_capture_f64(closure, 0, 42.0) };
        assert_eq!(unsafe { closure_capture_f64(closure, 0) }, 42.0);
    }
}
