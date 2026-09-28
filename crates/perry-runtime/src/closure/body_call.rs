//! THE funnel: the only Rust code that turns a JS function body's code
//! pointer into something callable.
//!
//! A *JS body* is native code a function object runs: a compiled closure
//! body, a value wrapper, or a native builtin installed as a function
//! object. Its native signature is
//!
//! ```text
//! double body(i64 callee, i64 this, double a0, double a1, ...)
//! ```
//!
//! (`perry_abi::JS_BODY_*` names the parameter positions; the receiver is a
//! [`JsThis`]). Two other body kinds exist until they join the one body ABI,
//! and have their own macros:
//!
//! * METHOD bodies — class instance methods, constructors, instance
//!   accessors: `double m(double this, double a0, ...)`
//!   ([`js_method_body_call!`]);
//! * BARE bodies — static methods and static accessors, and the top-level
//!   functions HIR lifts well-known hooks into (`static [Symbol.hasInstance]`,
//!   `get [Symbol.toStringTag]`, whose receiver is an explicit parameter):
//!   `double f(double a0, ...)` ([`js_bare_body_fn!`]).
//!
//! Every call site states the receiver and the JS arguments it passes and
//! nothing else, so the compiler finds every caller when the body ABI
//! changes. A `mem::transmute` to a body type anywhere else is refused by
//! `scripts/check_js_body_call_funnel.py`, and so is a native body definition
//! that does not declare the receiver.
//!
//! Over-application is safe and relied on: every supported calling
//! convention leaves the stack argument area to the caller, so a body that
//! declares N parameters reads only the first N (`wide_call.rs`).

/// The receiver a JS body takes as its second native parameter
/// (`perry_abi::JS_BODY_THIS_PARAM`), defined once in perry-abi and shared
/// with perry-ffi. It is the ONLY way a body learns its receiver: a
/// method-style caller passes the receiver, a plain call
/// [`JsThis::UNDEFINED`]. There is no ambient `this` state.
pub use crate::codegen_abi::JsThis;

const _: () = assert!(crate::codegen_abi::TAG_UNDEFINED == crate::value::TAG_UNDEFINED);

/// The receiver of a PLAIN call — a call with no receiver: `f(x)` through a
/// function value, a callback a builtin invokes without a `thisArg`:
/// `undefined` (OrdinaryCallBindThis; a sloppy body coerces it to
/// `globalThis` itself).
#[inline(always)]
pub const fn plain_call_receiver() -> JsThis {
    JsThis::UNDEFINED
}

/// The native type of a JS body taking the receiver and one `f64` per
/// token: `js_body_fn_ty!(a, b)` is perry-abi's
/// `unsafe extern "C" fn(*const ClosureHeader, JsThis, f64, f64) -> f64`.
macro_rules! js_body_fn_ty {
    ($($x:tt),* $(,)?) => {
        ::perry_abi::js_body_fn_ty!($crate::closure::ClosureHeader; $($x),*)
    };
}

/// Reinterpret a validated, non-sentinel body code pointer as a callable
/// taking the callee, the receiver and one `f64` per token
/// (`js_body_fn!(code; a, b)`).
///
/// # Safety
/// `code` must be a JS body whose declared JS parameter count is at most the
/// number of tokens (over-application is safe, see the module docs).
macro_rules! js_body_fn {
    ($code:expr; $($x:tt),* $(,)?) => {
        ::std::mem::transmute::<*const u8, $crate::closure::body_call::js_body_fn_ty!($($x),*)>($code)
    };
}

/// Call a JS body: `js_body_call!(code, callee, this, a0, a1, ...)`, where
/// `this` is a [`JsThis`].
///
/// # Safety
/// As [`js_body_fn!`]; `callee` is the function object whose body `code` is.
macro_rules! js_body_call {
    ($code:expr, $callee:expr, $this:expr $(, $a:expr)* $(,)?) => {{
        let f = $crate::closure::body_call::js_body_fn!($code; $($a),*);
        f($callee, $this $(, $a)*)
    }};
}

/// [`js_body_call!`] through an unwind-capable pointer type in builds that
/// unwind Rust panics (test builds), for callers that must let a JS
/// exception travel through them there. Production (`panic = "abort"`) is
/// identical to [`js_body_call!`].
macro_rules! js_body_call_unwind {
    (@f64 $x:tt) => { f64 };
    ($code:expr, $callee:expr, $this:expr $(, $a:expr)* $(,)?) => {{
        #[cfg(panic = "abort")]
        let f = $crate::closure::body_call::js_body_fn!($code; $($a),*);
        #[cfg(not(panic = "abort"))]
        let f = ::std::mem::transmute::<
            *const u8,
            extern "C-unwind" fn(
                *const $crate::closure::ClosureHeader,
                $crate::closure::body_call::JsThis
                $(, $crate::closure::body_call::js_body_call_unwind!(@f64 $a))*
            ) -> f64,
        >($code);
        f($callee, $this $(, $a)*)
    }};
}

/// Reinterpret a METHOD body (`double m(double this, double a0, ...)`) as a
/// callable taking the receiver plus one `f64` per token:
/// `js_method_body_fn!(code; value)` is `extern "C" fn(f64, f64) -> f64`.
///
/// # Safety
/// `code` must be a method body declaring at most the passed argument count.
macro_rules! js_method_body_fn {
    (@f64 $x:tt) => { f64 };
    ($code:expr; $($x:tt),* $(,)?) => {
        ::std::mem::transmute::<
            *const u8,
            extern "C" fn(f64 $(, $crate::closure::body_call::js_method_body_fn!(@f64 $x))*) -> f64,
        >($code)
    };
}

/// Call a METHOD body: `js_method_body_call!(code, this, a0, ...)`.
///
/// # Safety
/// As [`js_method_body_fn!`].
macro_rules! js_method_body_call {
    ($code:expr, $this:expr $(, $a:expr)* $(,)?) => {{
        let f = $crate::closure::body_call::js_method_body_fn!($code; $($a),*);
        f($this $(, $a)*)
    }};
}

/// Reinterpret a BARE body (`double f(double a0, ...)`: no callee, no
/// implicit receiver) as a callable taking one `f64` per token.
///
/// # Safety
/// `code` must be a bare body declaring at most the passed argument count.
macro_rules! js_bare_body_fn {
    (@f64 $x:tt) => { f64 };
    ($code:expr; $($x:tt),* $(,)?) => {
        ::std::mem::transmute::<
            *const u8,
            extern "C" fn($($crate::closure::body_call::js_bare_body_fn!(@f64 $x)),*) -> f64,
        >($code)
    };
}

pub(crate) use {
    js_bare_body_fn, js_body_call, js_body_call_unwind, js_body_fn, js_body_fn_ty,
    js_method_body_call, js_method_body_fn,
};

/// Call function value `func` with receiver `this` and `args`: the Rust-side
/// method-style call (`thisArg` of a builtin, an emitter, a getter's holder).
/// A plain call passes [`plain_call_receiver`].
///
/// # Safety
/// As [`crate::closure::js_native_call_value`].
#[inline]
pub unsafe fn call_value(func: f64, this: JsThis, args: &[f64]) -> f64 {
    let ptr = if args.is_empty() {
        std::ptr::null()
    } else {
        args.as_ptr()
    };
    crate::closure::native_call_value_this(func, this, ptr, args.len())
}
