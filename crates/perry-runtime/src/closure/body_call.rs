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
/// (`perry_abi::JS_BODY_THIS_PARAM`): the NaN-boxed `this` bits, passed in an
/// integer register (`repr(transparent)` over `u64`, so the ABI is exactly
/// that of a `u64`).
///
/// Stage 1 of this-as-a-parameter: every caller passes the receiver the
/// implicit-`this` cell holds for the call (a runtime caller reads it with
/// [`JsThis::current`]), and bodies still read the cell. A native body
/// declares the parameter and must not assume anything about it yet.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct JsThis(pub u64);

impl JsThis {
    /// `undefined` as a receiver.
    pub const UNDEFINED: JsThis = JsThis(crate::value::TAG_UNDEFINED);

    /// The receiver bits.
    #[inline(always)]
    pub const fn bits(self) -> u64 {
        self.0
    }

    /// The receiver as a NaN-boxed `f64`.
    #[inline(always)]
    pub fn as_f64(self) -> f64 {
        f64::from_bits(self.0)
    }

    /// A NaN-boxed `f64` receiver.
    #[inline(always)]
    pub fn from_f64(value: f64) -> Self {
        JsThis(value.to_bits())
    }

    /// The receiver the implicit-`this` cell holds right now: what a body
    /// called at this point would read from the cell. Read through the
    /// per-agent pointer block (`agent_ptrs`), the cheapest path to the cell
    /// on every target but Apple aarch64, where `HotTls` is the fast path.
    #[inline(always)]
    pub fn current() -> Self {
        #[cfg(not(all(
            target_vendor = "apple",
            target_arch = "aarch64",
            target_pointer_width = "64"
        )))]
        return JsThis(crate::agent_ptrs::implicit_this_bits());
        #[cfg(all(
            target_vendor = "apple",
            target_arch = "aarch64",
            target_pointer_width = "64"
        ))]
        return JsThis(crate::object::implicit_this_bits());
    }
}

/// The native type of a JS body taking the receiver and one `f64` per
/// token: `js_body_fn_ty!(a, b)` is
/// `extern "C" fn(*const ClosureHeader, JsThis, f64, f64) -> f64`.
macro_rules! js_body_fn_ty {
    (@f64 $x:tt) => { f64 };
    ($($x:tt),* $(,)?) => {
        extern "C" fn(
            *const $crate::closure::ClosureHeader,
            $crate::closure::body_call::JsThis
            $(, $crate::closure::body_call::js_body_fn_ty!(@f64 $x))*
        ) -> f64
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
