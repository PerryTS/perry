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
/// A body's [`JsFunctionInfo`] is built from one of these
/// ([`js_function_info!`](crate::js_function_info)), so a body with any
/// other signature — including a bare `*const u8` — does not compile:
///
/// ```ignore
/// extern "C" fn body(c: *const RawClosureHeader, this: JsThis, a0: f64) -> f64 { .. }
/// let closure = perry_ffi::alloc_closure(perry_ffi::js_function_info!(body, 1), 0);
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
    fn js_closure_alloc(info: *const JsFunctionInfo, capture_count: u32) -> *mut ClosureHeader;
    fn js_closure_get_capture_f64(closure: *const ClosureHeader, index: u32) -> f64;
    fn js_closure_set_capture_f64(closure: *mut ClosureHeader, index: u32, value: f64);
}

/// Everything the runtime knows about a native body, in one static record
/// the function object points to (perry-abi): its code address, parameter
/// count, and facts such as a rest parameter or `.length`. Build one with
/// [`js_function_info!`](crate::js_function_info) — the only safe
/// constructor takes the body's typed pointer.
pub use perry_abi::{JsFunctionInfo, FN_ARROW, FN_BUILTIN, FN_NON_CONSTRUCTOR, FN_STRICT};

/// A `&'static JsFunctionInfo` for the native body `body` declaring `n` JS
/// parameters — checked: the cast to [`JsBody0`]..[`JsBody16`] fails to
/// compile for any other signature. Facts beyond the parameter count follow
/// as [`JsFunctionInfo`] builder calls:
///
/// ```ignore
/// alloc_closure(js_function_info!(listener, 2), 1);
/// alloc_closure(js_function_info!(variadic, 1; with_rest(0)), 0);
/// alloc_closure(js_function_info!(method, 1; with_length(0)), 0);
/// ```
///
/// Each expansion is its own `static`: define one per body where the same
/// body is allocated from several places.
#[macro_export]
macro_rules! js_function_info {
    (@ty 0) => { $crate::JsBody0 };
    (@ty 1) => { $crate::JsBody1 };
    (@ty 2) => { $crate::JsBody2 };
    (@ty 3) => { $crate::JsBody3 };
    (@ty 4) => { $crate::JsBody4 };
    (@ty 5) => { $crate::JsBody5 };
    (@ty 6) => { $crate::JsBody6 };
    (@ty 7) => { $crate::JsBody7 };
    (@ty 8) => { $crate::JsBody8 };
    (@ty 9) => { $crate::JsBody9 };
    (@ty 10) => { $crate::JsBody10 };
    (@ty 11) => { $crate::JsBody11 };
    (@ty 12) => { $crate::JsBody12 };
    (@ty 13) => { $crate::JsBody13 };
    (@ty 14) => { $crate::JsBody14 };
    (@ty 15) => { $crate::JsBody15 };
    (@ty 16) => { $crate::JsBody16 };
    ($body:path, $n:tt $(; $($m:ident($($a:expr),*)),* $(,)?)?) => {{
        static INFO: $crate::JsFunctionInfo =
            $crate::JsFunctionInfo::of($body as $crate::js_function_info!(@ty $n))
                $($(.$m($($a),*))*)?;
        &INFO
    }};
}

/// Allocate a native closure running the body `info` describes, with
/// `capture_count` f64 capture slots.
///
/// `info` comes from [`js_function_info!`](crate::js_function_info), which
/// takes the body's typed pointer; a body of any other signature does not
/// compile. A body without the receiver:
///
/// ```compile_fail,E0605
/// // NOT-A-JS-BODY: the example of a body missing its receiver.
/// extern "C" fn no_this(_: *const perry_ffi::RawClosureHeader, a: f64) -> f64 { a }
/// let _ = perry_ffi::alloc_closure(perry_ffi::js_function_info!(no_this, 1), 0);
/// ```
///
/// an erased pointer:
///
/// ```compile_fail,E0277
/// extern "C" fn body(_: *const perry_ffi::RawClosureHeader, _: perry_ffi::JsThis) -> f64 { 0.0 }
/// static INFO: perry_ffi::JsFunctionInfo = perry_ffi::JsFunctionInfo::of(body as *const u8);
/// ```
///
/// a JS argument of the wrong type:
///
/// ```compile_fail,E0605
/// extern "C" fn body(_: *const perry_ffi::RawClosureHeader, _: perry_ffi::JsThis, a: i64) -> f64 { 0.0 }
/// let _ = perry_ffi::alloc_closure(perry_ffi::js_function_info!(body, 1), 0);
/// ```
///
/// or an info assembled by hand:
///
/// ```compile_fail
/// let _ = perry_ffi::JsFunctionInfo { code: std::ptr::null(), params: 0, rest_fixed: 0,
///     flags: 0, length: 0, trusted_captures: 0, trusted_code: std::ptr::null(),
///     trusted_boxed_mask: 0, versioned_code: std::ptr::null(), versioned_captures: 0,
///     reserved: 0, versioned_boxed_mask: 0 };
/// ```
pub fn alloc_closure(info: &'static JsFunctionInfo, capture_count: u32) -> *mut ClosureHeader {
    unsafe { js_closure_alloc(info, capture_count) }
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
        let closure = alloc_closure(crate::js_function_info!(callback, 0), 1);
        unsafe { set_closure_capture_f64(closure, 0, 42.0) };
        assert_eq!(unsafe { closure_capture_f64(closure, 0) }, 42.0);
    }
}
