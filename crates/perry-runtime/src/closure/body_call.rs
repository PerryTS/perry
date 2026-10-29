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

/// A `&'static JsFunctionInfo` for the native JS body `body`, which declares
/// `n` JS parameters — checked: the cast to the `JsBody{n}` type fails to
/// compile for any other signature. Facts beyond the parameter count follow
/// as `JsFunctionInfo` builder calls:
///
/// ```ignore
/// js_closure_alloc(fn_info!(my_thunk, 2), 0);
/// js_closure_alloc(fn_info!(my_rest_thunk, 1; with_rest(0)), 0);
/// js_closure_alloc(fn_info!(my_method, 1; with_length(0), with_flags(FN_BUILTIN)), 0);
/// ```
/// Each expansion is its own `static`: define one per body where the same
/// body is allocated from several places. A body declared `extern
/// "C-unwind"` takes `fn_info!(unwind body, n; ...)`; one declared C-unwind
/// only in unwinding (test) builds and plain C under `panic = "abort"` takes
/// `fn_info!(unwind_in_tests body, n; ...)`. A runtime-native body taking
/// its arguments in place (`JsNativeArgsBody`) takes
/// `fn_info!(native_args body, declared; ...)`.
#[macro_export]
macro_rules! fn_info {
    (@ty 0) => { $crate::codegen_abi::JsBody0<$crate::closure::ClosureHeader> };
    (@ty 1) => { $crate::codegen_abi::JsBody1<$crate::closure::ClosureHeader> };
    (@ty 2) => { $crate::codegen_abi::JsBody2<$crate::closure::ClosureHeader> };
    (@ty 3) => { $crate::codegen_abi::JsBody3<$crate::closure::ClosureHeader> };
    (@ty 4) => { $crate::codegen_abi::JsBody4<$crate::closure::ClosureHeader> };
    (@ty 5) => { $crate::codegen_abi::JsBody5<$crate::closure::ClosureHeader> };
    (@ty 6) => { $crate::codegen_abi::JsBody6<$crate::closure::ClosureHeader> };
    (@ty 7) => { $crate::codegen_abi::JsBody7<$crate::closure::ClosureHeader> };
    (@ty 8) => { $crate::codegen_abi::JsBody8<$crate::closure::ClosureHeader> };
    (@ty 9) => { $crate::codegen_abi::JsBody9<$crate::closure::ClosureHeader> };
    (@ty 10) => { $crate::codegen_abi::JsBody10<$crate::closure::ClosureHeader> };
    (@ty 11) => { $crate::codegen_abi::JsBody11<$crate::closure::ClosureHeader> };
    (@ty 12) => { $crate::codegen_abi::JsBody12<$crate::closure::ClosureHeader> };
    (@ty 13) => { $crate::codegen_abi::JsBody13<$crate::closure::ClosureHeader> };
    (@ty 14) => { $crate::codegen_abi::JsBody14<$crate::closure::ClosureHeader> };
    (@ty 15) => { $crate::codegen_abi::JsBody15<$crate::closure::ClosureHeader> };
    (@ty 16) => { $crate::codegen_abi::JsBody16<$crate::closure::ClosureHeader> };
    (@ty 17) => { $crate::codegen_abi::JsBody17<$crate::closure::ClosureHeader> };
    (@ty 18) => { $crate::codegen_abi::JsBody18<$crate::closure::ClosureHeader> };
    (@ty 19) => { $crate::codegen_abi::JsBody19<$crate::closure::ClosureHeader> };
    (@ty 20) => { $crate::codegen_abi::JsBody20<$crate::closure::ClosureHeader> };
    (@ty 21) => { $crate::codegen_abi::JsBody21<$crate::closure::ClosureHeader> };
    (@ty 22) => { $crate::codegen_abi::JsBody22<$crate::closure::ClosureHeader> };
    (@ty 23) => { $crate::codegen_abi::JsBody23<$crate::closure::ClosureHeader> };
    (@ty 24) => { $crate::codegen_abi::JsBody24<$crate::closure::ClosureHeader> };
    (@ty 25) => { $crate::codegen_abi::JsBody25<$crate::closure::ClosureHeader> };
    (@ty 26) => { $crate::codegen_abi::JsBody26<$crate::closure::ClosureHeader> };
    (@ty 27) => { $crate::codegen_abi::JsBody27<$crate::closure::ClosureHeader> };
    (@ty 28) => { $crate::codegen_abi::JsBody28<$crate::closure::ClosureHeader> };
    (@ty 29) => { $crate::codegen_abi::JsBody29<$crate::closure::ClosureHeader> };
    (@ty 30) => { $crate::codegen_abi::JsBody30<$crate::closure::ClosureHeader> };
    (@ty 31) => { $crate::codegen_abi::JsBody31<$crate::closure::ClosureHeader> };
    (@ty 32) => { $crate::codegen_abi::JsBody32<$crate::closure::ClosureHeader> };
    (@uty 0) => { $crate::codegen_abi::JsBodyUnwind0<$crate::closure::ClosureHeader> };
    (@uty 1) => { $crate::codegen_abi::JsBodyUnwind1<$crate::closure::ClosureHeader> };
    (@uty 2) => { $crate::codegen_abi::JsBodyUnwind2<$crate::closure::ClosureHeader> };
    (@uty 3) => { $crate::codegen_abi::JsBodyUnwind3<$crate::closure::ClosureHeader> };
    (@uty 4) => { $crate::codegen_abi::JsBodyUnwind4<$crate::closure::ClosureHeader> };
    (@uty 5) => { $crate::codegen_abi::JsBodyUnwind5<$crate::closure::ClosureHeader> };
    (@uty 6) => { $crate::codegen_abi::JsBodyUnwind6<$crate::closure::ClosureHeader> };
    (@uty 7) => { $crate::codegen_abi::JsBodyUnwind7<$crate::closure::ClosureHeader> };
    (@uty 8) => { $crate::codegen_abi::JsBodyUnwind8<$crate::closure::ClosureHeader> };
    (@uty 9) => { $crate::codegen_abi::JsBodyUnwind9<$crate::closure::ClosureHeader> };
    (@uty 10) => { $crate::codegen_abi::JsBodyUnwind10<$crate::closure::ClosureHeader> };
    (@uty 11) => { $crate::codegen_abi::JsBodyUnwind11<$crate::closure::ClosureHeader> };
    (@uty 12) => { $crate::codegen_abi::JsBodyUnwind12<$crate::closure::ClosureHeader> };
    (@uty 13) => { $crate::codegen_abi::JsBodyUnwind13<$crate::closure::ClosureHeader> };
    (@uty 14) => { $crate::codegen_abi::JsBodyUnwind14<$crate::closure::ClosureHeader> };
    (@uty 15) => { $crate::codegen_abi::JsBodyUnwind15<$crate::closure::ClosureHeader> };
    (@uty 16) => { $crate::codegen_abi::JsBodyUnwind16<$crate::closure::ClosureHeader> };
    ($(#[realm($($facts:tt)*)])? native_args $body:path, $declared:tt $(; $($m:ident($($a:expr),*)),* $(,)?)?) => {{
        static OWNER: $crate::codegen_abi::FunctionMetadataOwner = $crate::codegen_abi::FunctionMetadataOwner::native();
        static INFO: $crate::codegen_abi::NativeFunctionInfo = $crate::codegen_abi::NativeFunctionInfo::new(
            $crate::closure::JsFunctionInfo::of_native_args(
                $body as $crate::codegen_abi::JsNativeArgsBody<$crate::closure::ClosureHeader>,
                $declared,
            )
                $($(.$m($($a),*))*)?, &OWNER);
        &INFO.info as *const $crate::closure::JsFunctionInfo
    }};
    ($(#[realm($($facts:tt)*)])? unwind_in_tests $body:path, $n:tt $(; $($m:ident($($a:expr),*)),* $(,)?)?) => {{
        #[cfg(panic = "abort")]
        let info = $crate::fn_info!($body, $n $(; $($m($($a),*)),*)?);
        #[cfg(not(panic = "abort"))]
        let info = $crate::fn_info!(unwind $body, $n $(; $($m($($a),*)),*)?);
        info
    }};
    ($(#[realm($($facts:tt)*)])? unwind $body:path, $n:tt $(; $($m:ident($($a:expr),*)),* $(,)?)?) => {{
        static OWNER: $crate::codegen_abi::FunctionMetadataOwner = $crate::codegen_abi::FunctionMetadataOwner::native();
        static INFO: $crate::codegen_abi::NativeFunctionInfo = $crate::codegen_abi::NativeFunctionInfo::new(
            $crate::closure::JsFunctionInfo::of($body as $crate::fn_info!(@uty $n))
                $($(.$m($($a),*))*)?, &OWNER);
        &INFO.info as *const $crate::closure::JsFunctionInfo
    }};
    ($(#[realm($($facts:tt)*)])? $body:path, $n:tt $(; $($m:ident($($a:expr),*)),* $(,)?)?) => {{
        static OWNER: $crate::codegen_abi::FunctionMetadataOwner = $crate::codegen_abi::FunctionMetadataOwner::native();
        static INFO: $crate::codegen_abi::NativeFunctionInfo = $crate::codegen_abi::NativeFunctionInfo::new(
            $crate::closure::JsFunctionInfo::of($body as $crate::fn_info!(@ty $n))
                $($(.$m($($a),*))*)?, &OWNER);
        &INFO.info as *const $crate::closure::JsFunctionInfo
    }};
    (#[realm($($facts:tt)*)] $body:path, $n:tt; $($m:ident($($a:expr),*)),*; bind $binding:ident, $metadata:ident) => { $crate::realm_fn_info_bind!($binding, $metadata, $crate::fn_info!($body, $n; $($m($($a),*)),*); $($facts)*); };
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

/// [`js_body_fn!`] as an `extern "C-unwind"` callable, for an entry that is
/// itself `extern "C-unwind"` in unwinding (test) builds and hands its call
/// to the body with `become` (the two signatures must be identical).
///
/// # Safety
/// As [`js_body_fn!`].
#[cfg(not(panic = "abort"))]
macro_rules! js_body_fn_unwind {
    (@f64 $x:tt) => { f64 };
    ($code:expr; $($x:tt),* $(,)?) => {
        ::std::mem::transmute::<
            *const u8,
            unsafe extern "C-unwind" fn(
                *const $crate::closure::ClosureHeader,
                $crate::closure::body_call::JsThis
                $(, $crate::closure::body_call::js_body_fn_unwind!(@f64 $x))*
            ) -> f64,
        >($code)
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

/// [`js_bare_body_fn!`] through an unwind-capable pointer type in builds that
/// unwind Rust panics (test builds), for a caller that must let a JS exception
/// travel through it there. Production (`panic = "abort"`) is identical to
/// [`js_bare_body_fn!`].
///
/// # Safety
/// As [`js_bare_body_fn!`].
macro_rules! js_bare_body_fn_unwind {
    (@f64 $x:tt) => { f64 };
    ($code:expr; $($x:tt),* $(,)?) => {{
        #[cfg(panic = "abort")]
        let f = $crate::closure::body_call::js_bare_body_fn!($code; $($x),*);
        #[cfg(not(panic = "abort"))]
        let f = ::std::mem::transmute::<
            *const u8,
            unsafe extern "C-unwind" fn($($crate::closure::body_call::js_bare_body_fn_unwind!(@f64 $x)),*) -> f64,
        >($code);
        f
    }};
}

#[cfg(not(panic = "abort"))]
pub(crate) use js_body_fn_unwind;
pub(crate) use {
    js_bare_body_fn, js_bare_body_fn_unwind, js_body_call, js_body_call_unwind, js_body_fn,
    js_body_fn_ty, js_method_body_call, js_method_body_fn,
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

/// Enter the function's generic callee-owned argument adapter. Cells, unlike
/// a borrowed f64 slice, explicitly permit the collector to update the caller's
/// actual argument storage. No argument-value copy is required in the callee.
#[inline(always)]
pub(crate) unsafe fn call_entry_cells(
    callee: *const crate::closure::ClosureHeader,
    entry: *const u8,
    this: JsThis,
    args: &[std::cell::UnsafeCell<f64>],
) -> f64 {
    call_entry_raw(callee, entry, this, args.as_ptr(), args.len())
}

/// A compiled plain entry loads its input values before calling its operation.
/// Before that call it does no materialization and never rewrites the vector,
/// so a borrowed slice needs no mutable staging. The immutable record must
/// prove the entry has no rest/arguments bundle before taking this path.
#[inline(always)]
pub(crate) unsafe fn call_entry_plain_slice(
    callee: *const crate::closure::ClosureHeader,
    entry: *const u8,
    this: JsThis,
    args: &[f64],
) -> f64 {
    call_entry_raw(callee, entry, this, args.as_ptr().cast(), args.len())
}

#[inline(always)]
unsafe fn call_entry_raw(
    callee: *const crate::closure::ClosureHeader,
    entry: *const u8,
    this: JsThis,
    args: *const std::cell::UnsafeCell<f64>,
    count: usize,
) -> f64 {
    #[cfg(panic = "abort")]
    let body = std::mem::transmute::<
        *const u8,
        crate::codegen_abi::JsCallEntry<crate::closure::ClosureHeader>,
    >(entry);
    #[cfg(not(panic = "abort"))]
    let body = std::mem::transmute::<
        *const u8,
        unsafe extern "C-unwind" fn(
            *const crate::closure::ClosureHeader,
            JsThis,
            *const std::cell::UnsafeCell<f64>,
            usize,
        ) -> f64,
    >(entry);
    body(callee, this, args, count)
}

/// Stage a borrowed runtime argument slice in caller-owned mutable cells.
/// The ordinary closure-call register width fits on the stack; wider calls
/// use storage sized to the same argument count. Both enter the same ABI.
#[inline]
pub(crate) unsafe fn call_entry_slice(
    callee: *const crate::closure::ClosureHeader,
    entry: *const u8,
    this: JsThis,
    args: &[f64],
) -> f64 {
    use std::cell::UnsafeCell;
    use std::mem::MaybeUninit;
    const WIDTH: usize = crate::codegen_abi::JS_CLOSURE_CALL_MAX_ARGS;
    if args.len() <= WIDTH {
        let mut cells: [MaybeUninit<UnsafeCell<f64>>; WIDTH] =
            [const { MaybeUninit::uninit() }; WIDTH];
        for (cell, value) in cells.iter_mut().zip(args) {
            cell.write(UnsafeCell::new(*value));
        }
        // Only the initialized prefix is borrowed, and UnsafeCell permits
        // the collector to rewrite it for this activation.
        let cells = std::slice::from_raw_parts(cells.as_ptr().cast(), args.len());
        call_entry_cells(callee, entry, this, cells)
    } else {
        let cells: Vec<_> = args.iter().copied().map(UnsafeCell::new).collect();
        call_entry_cells(callee, entry, this, &cells)
    }
}

/// A native closure body with no observable action. Shared by builtin families.
pub(crate) extern "C" fn native_noop0(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Source-owned realm facts; an owned body-info item passes through unchanged.
#[macro_export]
macro_rules! realm_decl {
    (@native_compare $property:expr; $key:literal) => { $property == $key };
    (@native_compare $property:expr; $first:literal | $($key:literal)|+) => { matches!($property, $first | $($key)|+) };
    (native_nonconstructors($module:expr, $property:expr); $($($owner:literal)|+ => [$($key:literal)|+]),* $(,)?) => { match $module { $($($owner)|+ => $crate::realm_decl!(@native_compare $property; $($key)|+),)* _ => false, } }; (info $info:item realm { $($facts:tt)* }) => { $info }; (info_expr $info:expr; realm { $($facts:tt)* }) => { $info }; (native_key $key:literal; realm { $($facts:tt)* }) => { $key }; (module_provider $provider:item realm { $($facts:tt)* }) => { $provider }; (record $builder:ident, $layout:ident; realm { $($facts:tt)* }) => { $crate::object::runtime_layouts::runtime_record!($builder, $layout); }; (birth $birth:expr; realm { $($facts:tt)* }) => { $birth }; (object $name:literal, $object:ident; $($members:tt)*) => { $crate::realm_native_object!($object; $($members)*); }; ($($facts:tt)*) => {};
}

/// Bind ABI metadata and a local macro projecting public spec overrides at their
/// original use sites. The macro stores no runtime record or lookup table.
#[doc(hidden)]
#[macro_export]
macro_rules! realm_fn_info_bind {
    ($binding:ident, $metadata:ident, $info:expr; $path:literal, spec = { name = $name:expr, length = $length:expr }, emit = metadata $(, $($facts:tt)*)?) => {
        macro_rules! $metadata {
            (name) => {
                $name
            };
            (length) => {
                $length
            };
        }
        let $binding = $info;
    };
    ($binding:ident, $metadata:ident, $info:expr; $($path:ident).+, spec = { name = $name:expr, length = $length:expr }, emit = metadata $(, $($facts:tt)*)?) => {
        macro_rules! $metadata {
            (name) => {
                $name
            };
            (length) => {
                $length
            };
        }
        let $binding = $info;
    };
    ($binding:ident, $metadata:ident, $info:expr; $path:literal, spec = { name = $value:expr }, emit = metadata $(, $($facts:tt)*)?) => {
        macro_rules! $metadata {
            (name) => {
                $value
            };
        }
        let $binding = $info;
    };
    ($binding:ident, $metadata:ident, $info:expr; $path:literal, spec = { length = $value:expr }, emit = metadata $(, $($facts:tt)*)?) => {
        macro_rules! $metadata {
            (length) => {
                $value
            };
        }
        let $binding = $info;
    };
    ($binding:ident, $metadata:ident, $info:expr; $($path:ident).+, spec = { name = $value:expr }, emit = metadata $(, $($facts:tt)*)?) => {
        macro_rules! $metadata {
            (name) => {
                $value
            };
        }
        let $binding = $info;
    };
    ($binding:ident, $metadata:ident, $info:expr; $($path:ident).+, spec = { length = $value:expr }, emit = metadata $(, $($facts:tt)*)?) => {
        macro_rules! $metadata {
            (length) => {
                $value
            };
        }
        let $binding = $info;
    };
}

/// Literal native object births declared once for runtime and realm production.
#[doc(hidden)]
#[macro_export]
macro_rules! realm_native_object {
    ($object:ident; $($key:literal => $kind:ident($value:literal);)*) => {
        let $object = $crate::object::js_object_alloc(0, 0);
        $(
            $crate::object::native_module::callable_exports::native_set_field(
                $object, $key, $crate::realm_native_object!(@value $kind $value),
            );
        )*
    };
    (@value string $value:literal) => {
        $crate::object::native_module::callable_exports::native_string_value($value)
    };
    (@value boolean $value:literal) => {
        $crate::object::native_module::callable_exports::native_bool_value($value)
    };
    (@value number $value:literal) => { $value };
}
