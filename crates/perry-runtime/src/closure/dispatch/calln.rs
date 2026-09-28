//! Per-arity `js_closure_callN(closure, this, a0..)` FFI entry points
//! (0..=16) and the shared `dispatch_registered_call` /
//! `dispatch_rest_or_declared_arity` routing helpers.
//!
//! Stage 1 of this-as-a-parameter: bodies still read the implicit-`this`
//! cell, and the routing below passes the cell's value as every body's
//! receiver parameter. An entry therefore makes the cell hold the `this` it
//! is handed for the duration of the call ([`with_receiver`]); a caller that
//! passes the cell's current value (every runtime plain call,
//! `plain_call_receiver()`) binds nothing.
//!
//! The hot-loop counterpart -- resolve a closure ONCE and call it directly for
//! the rest of the loop -- lives in the sibling `direct` module (#8180). It
//! subsumes the `resolve_call2_direct` helper that used to sit here and had
//! exactly one consumer.

use super::*;
use crate::closure::JsThis;

/// Run `call` with the implicit-`this` cell holding `this`. When the cell
/// already holds it — the common case — nothing is bound. Otherwise the cell
/// is bound for the call and the displaced value restored after it, rooted
/// across the call (an `ImplicitThisScope`), except for an arrow callee, whose
/// `this` is lexical and reads neither the cell nor the parameter.
#[inline(always)]
pub(crate) fn with_receiver(
    closure: *const ClosureHeader,
    this: JsThis,
    call: impl FnOnce() -> f64,
) -> f64 {
    if this == crate::closure::body_call::current_this()
        || crate::closure::closure_is_arrow(closure)
    {
        return call();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let _bound = crate::object::ImplicitThisScope::bind(&scope, this.as_f64());
    call()
}

/// Call a closure with receiver `this` and no arguments.
#[cfg(panic = "abort")]
#[no_mangle]
pub extern "C" fn js_closure_call0(closure: *const ClosureHeader, this: JsThis) -> f64 {
    with_receiver(closure, this, || js_closure_call0_impl(closure))
}

/// Test/debug builds use Rust unwinding for JS exceptions. Keep this entry
/// point unwind-capable there so an interpreted throw can reach a generated
/// caller's catch landing pad. Production builds use the plain-C definition
/// above because their raw Itanium exceptions must cross it without Rust's
/// abort-on-unwind guard (#8479).
#[cfg(not(panic = "abort"))]
#[no_mangle]
pub extern "C-unwind" fn js_closure_call0(closure: *const ClosureHeader, this: JsThis) -> f64 {
    with_receiver(closure, this, || js_closure_call0_impl(closure))
}

// #8479: NOT `C-unwind` (below). The runtime is built `panic=abort` and JS
// throws travel as a raw Itanium `_Unwind_Exception` that must step THROUGH
// these frames untouched (see `crate::eh`).
macro_rules! closure_call_entry {
    ($entry:ident, $cell:ident, $n:literal; $($a:ident),+) => {
        #[doc = concat!("Call a closure with receiver `this` and ", stringify!($n), " argument(s).")]
        #[no_mangle]
        pub extern "C" fn $entry(closure: *const ClosureHeader, this: JsThis, $($a: f64),+) -> f64 {
            with_receiver(closure, this, || $cell(closure, $($a),+))
        }
    };
}

closure_call_entry!(js_closure_call1, closure_call1_cell, 1; a0);
closure_call_entry!(js_closure_call2, closure_call2_cell, 2; a0, a1);
closure_call_entry!(js_closure_call3, closure_call3_cell, 3; a0, a1, a2);
closure_call_entry!(js_closure_call4, closure_call4_cell, 4; a0, a1, a2, a3);
closure_call_entry!(js_closure_call5, closure_call5_cell, 5; a0, a1, a2, a3, a4);
closure_call_entry!(js_closure_call6, closure_call6_cell, 6; a0, a1, a2, a3, a4, a5);
closure_call_entry!(js_closure_call7, closure_call7_cell, 7; a0, a1, a2, a3, a4, a5, a6);
closure_call_entry!(js_closure_call8, closure_call8_cell, 8; a0, a1, a2, a3, a4, a5, a6, a7);
closure_call_entry!(js_closure_call9, closure_call9_cell, 9; a0, a1, a2, a3, a4, a5, a6, a7, a8);
closure_call_entry!(js_closure_call10, closure_call10_cell, 10; a0, a1, a2, a3, a4, a5, a6, a7, a8, a9);
closure_call_entry!(js_closure_call11, closure_call11_cell, 11; a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10);
closure_call_entry!(js_closure_call12, closure_call12_cell, 12; a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11);
closure_call_entry!(js_closure_call13, closure_call13_cell, 13; a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12);
closure_call_entry!(js_closure_call14, closure_call14_cell, 14; a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13);
closure_call_entry!(js_closure_call15, closure_call15_cell, 15; a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14);
closure_call_entry!(js_closure_call16, closure_call16_cell, 16; a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15);

#[inline(always)]
fn js_closure_call0_impl(closure: *const ClosureHeader) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return dispatch_proxy_callee_or_throw(closure, &[]);
    }
    match resolve_strategy(func_ptr).kind() {
        DispatchKind::BoundMethod => unsafe { dispatch_bound_method(closure, &[]) },
        DispatchKind::BoundFunction => unsafe { dispatch_bound_function(closure, &[]) },
        DispatchKind::Rest(fixed_arity, synth) => unsafe {
            dispatch_rest_bundled(closure, func_ptr, &[], fixed_arity, synth)
        },
        DispatchKind::Arity(declared) if arity_needs_dispatch(declared, 0) => unsafe {
            dispatch_with_arity(closure, func_ptr, &[], declared)
        },
        _ => unsafe {
            crate::closure::body_call::js_body_call!(
                func_ptr,
                closure,
                crate::closure::body_call::current_this()
            )
        },
    }
}

/// Call a closure with 1 argument, returning f64
// The one-argument value-call path can run arbitrary generated code and must
// let a JS exception unwind to the generated caller's catch landing pad.
// #8479: NOT `C-unwind`. The runtime is built `panic=abort` and JS throws
// travel as a raw Itanium `_Unwind_Exception` that must step THROUGH these
// frames untouched (see `crate::eh` and the panic=abort rationale in the
// workspace Cargo.toml). Marking a frame `extern "C-unwind"` in a
// panic=abort crate does not enable that — it makes rustc wrap the call in
// an abort-on-unwind landing pad, which is exactly the RFC-2945 guard a JS
// throw trips ("panic in a function that cannot unwind"). #8416 introduced
// the first two such guards here; #8464 added ~40 more and measurably
// regressed main (+20 gap crashes, gc-stress) before being reverted.
#[inline(always)]
fn closure_call1_cell(closure: *const ClosureHeader, arg0: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return dispatch_proxy_callee_or_throw(closure, &[arg0]);
    }
    dispatch_call1_resolved(
        closure,
        func_ptr,
        arg0,
        resolve_strategy(func_ptr),
        crate::closure::body_call::current_this(),
    )
}

/// `this` is the receiver the implicit-`this` cell holds for the call — the
/// caller knows it, so the direct arm does not re-read the cell.
#[inline(always)]
fn dispatch_call1_resolved(
    closure: *const ClosureHeader,
    func_ptr: *const u8,
    arg0: f64,
    strategy: DispatchStrategy,
    this: crate::closure::JsThis,
) -> f64 {
    match strategy.kind() {
        DispatchKind::BoundMethod => unsafe { dispatch_bound_method(closure, &[arg0]) },
        DispatchKind::BoundFunction => unsafe { dispatch_bound_function(closure, &[arg0]) },
        DispatchKind::Rest(fixed_arity, synth) => unsafe {
            dispatch_rest_bundled(closure, func_ptr, &[arg0], fixed_arity, synth)
        },
        DispatchKind::Arity(declared) if arity_needs_dispatch(declared, 1) => unsafe {
            dispatch_with_arity(closure, func_ptr, &[arg0], declared)
        },
        _ => unsafe { crate::closure::body_call::js_body_call!(func_ptr, closure, this, arg0) },
    }
}

/// Call a closure with 2 arguments, returning f64
// A dynamically-dispatched closure can throw into a generated caller's catch
// landing pad; this bridge is on Next's loadManifest/readFileSync path.
#[inline(always)]
fn closure_call2_cell(closure: *const ClosureHeader, arg0: f64, arg1: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return dispatch_proxy_callee_or_throw(closure, &[arg0, arg1]);
    }
    match resolve_strategy(func_ptr).kind() {
        DispatchKind::BoundMethod => unsafe { dispatch_bound_method(closure, &[arg0, arg1]) },
        DispatchKind::BoundFunction => unsafe { dispatch_bound_function(closure, &[arg0, arg1]) },
        DispatchKind::Rest(fixed_arity, synth) => unsafe {
            dispatch_rest_bundled(closure, func_ptr, &[arg0, arg1], fixed_arity, synth)
        },
        DispatchKind::Arity(declared) if arity_needs_dispatch(declared, 2) => unsafe {
            dispatch_with_arity(closure, func_ptr, &[arg0, arg1], declared)
        },
        _ => unsafe {
            crate::closure::body_call::js_body_call!(
                func_ptr,
                closure,
                crate::closure::body_call::current_this(),
                arg0,
                arg1
            )
        },
    }
}

/// Call a closure with 3 arguments, returning f64
#[inline(always)]
fn closure_call3_cell(closure: *const ClosureHeader, arg0: f64, arg1: f64, arg2: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return dispatch_proxy_callee_or_throw(closure, &[arg0, arg1, arg2]);
    }
    match resolve_strategy(func_ptr).kind() {
        DispatchKind::BoundMethod => unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2]) },
        DispatchKind::BoundFunction => unsafe {
            dispatch_bound_function(closure, &[arg0, arg1, arg2])
        },
        DispatchKind::Rest(fixed_arity, synth) => unsafe {
            dispatch_rest_bundled(closure, func_ptr, &[arg0, arg1, arg2], fixed_arity, synth)
        },
        DispatchKind::Arity(declared) if arity_needs_dispatch(declared, 3) => unsafe {
            dispatch_with_arity(closure, func_ptr, &[arg0, arg1, arg2], declared)
        },
        _ => unsafe {
            crate::closure::body_call::js_body_call!(
                func_ptr,
                closure,
                crate::closure::body_call::current_this(),
                arg0,
                arg1,
                arg2
            )
        },
    }
}

/// Call a closure with 4 arguments, returning f64
#[inline(always)]
fn closure_call4_cell(
    closure: *const ClosureHeader,
    arg0: f64,
    arg1: f64,
    arg2: f64,
    arg3: f64,
) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return dispatch_proxy_callee_or_throw(closure, &[arg0, arg1, arg2, arg3]);
    }
    match resolve_strategy(func_ptr).kind() {
        DispatchKind::BoundMethod => unsafe {
            dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3])
        },
        DispatchKind::BoundFunction => unsafe {
            dispatch_bound_function(closure, &[arg0, arg1, arg2, arg3])
        },
        DispatchKind::Rest(fixed_arity, synth) => unsafe {
            dispatch_rest_bundled(
                closure,
                func_ptr,
                &[arg0, arg1, arg2, arg3],
                fixed_arity,
                synth,
            )
        },
        DispatchKind::Arity(declared) if arity_needs_dispatch(declared, 4) => unsafe {
            dispatch_with_arity(closure, func_ptr, &[arg0, arg1, arg2, arg3], declared)
        },
        _ => unsafe {
            crate::closure::body_call::js_body_call!(
                func_ptr,
                closure,
                crate::closure::body_call::current_this(),
                arg0,
                arg1,
                arg2,
                arg3
            )
        },
    }
}

/// Call a closure with 5 arguments, returning f64
#[inline(always)]
fn closure_call5_cell(
    closure: *const ClosureHeader,
    arg0: f64,
    arg1: f64,
    arg2: f64,
    arg3: f64,
    arg4: f64,
) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return dispatch_proxy_callee_or_throw(closure, &[arg0, arg1, arg2, arg3, arg4]);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4]) };
    }
    if func_ptr == BOUND_FUNCTION_FUNC_PTR {
        return unsafe { dispatch_bound_function(closure, &[arg0, arg1, arg2, arg3, arg4]) };
    }
    if let Some((fixed_arity, synth)) = lookup_closure_rest_full(func_ptr) {
        return unsafe {
            dispatch_rest_bundled(
                closure,
                func_ptr,
                &[arg0, arg1, arg2, arg3, arg4],
                fixed_arity,
                synth,
            )
        };
    }
    if let Some(declared) = dispatch_arity(func_ptr) {
        if arity_needs_dispatch(declared, 5) {
            return unsafe {
                dispatch_with_arity(closure, func_ptr, &[arg0, arg1, arg2, arg3, arg4], declared)
            };
        }
    }
    unsafe {
        crate::closure::body_call::js_body_call!(
            func_ptr,
            closure,
            crate::closure::body_call::current_this(),
            arg0,
            arg1,
            arg2,
            arg3,
            arg4
        )
    }
}

/// Call a closure with 6 arguments, returning f64
#[inline(always)]
fn closure_call6_cell(
    closure: *const ClosureHeader,
    arg0: f64,
    arg1: f64,
    arg2: f64,
    arg3: f64,
    arg4: f64,
    arg5: f64,
) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return dispatch_proxy_callee_or_throw(closure, &[arg0, arg1, arg2, arg3, arg4, arg5]);
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4, arg5]) };
    }
    if func_ptr == BOUND_FUNCTION_FUNC_PTR {
        return unsafe { dispatch_bound_function(closure, &[arg0, arg1, arg2, arg3, arg4, arg5]) };
    }
    if let Some((fixed_arity, synth)) = lookup_closure_rest_full(func_ptr) {
        return unsafe {
            dispatch_rest_bundled(
                closure,
                func_ptr,
                &[arg0, arg1, arg2, arg3, arg4, arg5],
                fixed_arity,
                synth,
            )
        };
    }
    if let Some(declared) = dispatch_arity(func_ptr) {
        if arity_needs_dispatch(declared, 6) {
            return unsafe {
                dispatch_with_arity(
                    closure,
                    func_ptr,
                    &[arg0, arg1, arg2, arg3, arg4, arg5],
                    declared,
                )
            };
        }
    }
    unsafe {
        crate::closure::body_call::js_body_call!(
            func_ptr,
            closure,
            crate::closure::body_call::current_this(),
            arg0,
            arg1,
            arg2,
            arg3,
            arg4,
            arg5
        )
    }
}

#[inline]
pub(crate) fn dispatch_registered_call(
    closure: *const ClosureHeader,
    func_ptr: *const u8,
    args: &[f64],
) -> Option<f64> {
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return Some(unsafe { dispatch_bound_method(closure, args) });
    }
    if func_ptr == BOUND_FUNCTION_FUNC_PTR {
        return Some(unsafe { dispatch_bound_function(closure, args) });
    }
    None
}

#[inline]
pub(crate) fn dispatch_rest_or_declared_arity(
    closure: *const ClosureHeader,
    func_ptr: *const u8,
    args: &[f64],
    provided: u32,
) -> Option<f64> {
    if let Some((fixed_arity, synth)) = lookup_closure_rest_full(func_ptr) {
        return Some(unsafe { dispatch_rest_bundled(closure, func_ptr, args, fixed_arity, synth) });
    }
    if let Some(declared) = dispatch_arity(func_ptr) {
        if arity_needs_dispatch(declared, provided) {
            return Some(unsafe { dispatch_with_arity(closure, func_ptr, args, declared) });
        }
    }
    None
}

/// Call a closure with 7 arguments, returning f64
#[inline(always)]
fn closure_call7_cell(
    closure: *const ClosureHeader,
    arg0: f64,
    arg1: f64,
    arg2: f64,
    arg3: f64,
    arg4: f64,
    arg5: f64,
    arg6: f64,
) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return dispatch_proxy_callee_or_throw(
            closure,
            &[arg0, arg1, arg2, arg3, arg4, arg5, arg6],
        );
    }
    let args = [arg0, arg1, arg2, arg3, arg4, arg5, arg6];
    if let Some(result) = dispatch_registered_call(closure, func_ptr, &args) {
        return result;
    }
    let args = [arg0, arg1, arg2, arg3, arg4, arg5, arg6];
    if let Some(result) = dispatch_rest_or_declared_arity(closure, func_ptr, &args, 7) {
        return result;
    }
    unsafe {
        crate::closure::body_call::js_body_call!(
            func_ptr,
            closure,
            crate::closure::body_call::current_this(),
            arg0,
            arg1,
            arg2,
            arg3,
            arg4,
            arg5,
            arg6
        )
    }
}

/// Call a closure with 8 arguments, returning f64
#[inline(always)]
fn closure_call8_cell(
    closure: *const ClosureHeader,
    arg0: f64,
    arg1: f64,
    arg2: f64,
    arg3: f64,
    arg4: f64,
    arg5: f64,
    arg6: f64,
    arg7: f64,
) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return dispatch_proxy_callee_or_throw(
            closure,
            &[arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7],
        );
    }
    let args = [arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7];
    if let Some(result) = dispatch_registered_call(closure, func_ptr, &args) {
        return result;
    }
    let args = [arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7];
    if let Some(result) = dispatch_rest_or_declared_arity(closure, func_ptr, &args, 8) {
        return result;
    }
    unsafe {
        crate::closure::body_call::js_body_call!(
            func_ptr,
            closure,
            crate::closure::body_call::current_this(),
            arg0,
            arg1,
            arg2,
            arg3,
            arg4,
            arg5,
            arg6,
            arg7
        )
    }
}

/// Call a closure with 9 arguments, returning f64
#[inline(always)]
fn closure_call9_cell(
    closure: *const ClosureHeader,
    arg0: f64,
    arg1: f64,
    arg2: f64,
    arg3: f64,
    arg4: f64,
    arg5: f64,
    arg6: f64,
    arg7: f64,
    arg8: f64,
) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return dispatch_proxy_callee_or_throw(
            closure,
            &[arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8],
        );
    }
    let args = [arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8];
    if let Some(result) = dispatch_registered_call(closure, func_ptr, &args) {
        return result;
    }
    let args = [arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8];
    if let Some(result) = dispatch_rest_or_declared_arity(closure, func_ptr, &args, 9) {
        return result;
    }
    unsafe {
        crate::closure::body_call::js_body_call!(
            func_ptr,
            closure,
            crate::closure::body_call::current_this(),
            arg0,
            arg1,
            arg2,
            arg3,
            arg4,
            arg5,
            arg6,
            arg7,
            arg8
        )
    }
}

/// Call a closure with 10 arguments, returning f64
#[inline(always)]
fn closure_call10_cell(
    closure: *const ClosureHeader,
    arg0: f64,
    arg1: f64,
    arg2: f64,
    arg3: f64,
    arg4: f64,
    arg5: f64,
    arg6: f64,
    arg7: f64,
    arg8: f64,
    arg9: f64,
) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return dispatch_proxy_callee_or_throw(
            closure,
            &[arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9],
        );
    }
    let args = [arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9];
    if let Some(result) = dispatch_registered_call(closure, func_ptr, &args) {
        return result;
    }
    let args = [arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9];
    if let Some(result) = dispatch_rest_or_declared_arity(closure, func_ptr, &args, 10) {
        return result;
    }
    unsafe {
        crate::closure::body_call::js_body_call!(
            func_ptr,
            closure,
            crate::closure::body_call::current_this(),
            arg0,
            arg1,
            arg2,
            arg3,
            arg4,
            arg5,
            arg6,
            arg7,
            arg8,
            arg9
        )
    }
}

/// Call a closure with 11 arguments, returning f64
#[inline(always)]
fn closure_call11_cell(
    closure: *const ClosureHeader,
    arg0: f64,
    arg1: f64,
    arg2: f64,
    arg3: f64,
    arg4: f64,
    arg5: f64,
    arg6: f64,
    arg7: f64,
    arg8: f64,
    arg9: f64,
    arg10: f64,
) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return dispatch_proxy_callee_or_throw(
            closure,
            &[
                arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10,
            ],
        );
    }
    let args = [
        arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10,
    ];
    if let Some(result) = dispatch_registered_call(closure, func_ptr, &args) {
        return result;
    }
    let args = [
        arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10,
    ];
    if let Some(result) = dispatch_rest_or_declared_arity(closure, func_ptr, &args, 11) {
        return result;
    }
    unsafe {
        crate::closure::body_call::js_body_call!(
            func_ptr,
            closure,
            crate::closure::body_call::current_this(),
            arg0,
            arg1,
            arg2,
            arg3,
            arg4,
            arg5,
            arg6,
            arg7,
            arg8,
            arg9,
            arg10
        )
    }
}

/// Call a closure with 12 arguments, returning f64
#[inline(always)]
fn closure_call12_cell(
    closure: *const ClosureHeader,
    arg0: f64,
    arg1: f64,
    arg2: f64,
    arg3: f64,
    arg4: f64,
    arg5: f64,
    arg6: f64,
    arg7: f64,
    arg8: f64,
    arg9: f64,
    arg10: f64,
    arg11: f64,
) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return dispatch_proxy_callee_or_throw(
            closure,
            &[
                arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11,
            ],
        );
    }
    let args = [
        arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11,
    ];
    if let Some(result) = dispatch_registered_call(closure, func_ptr, &args) {
        return result;
    }
    let args = [
        arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11,
    ];
    if let Some(result) = dispatch_rest_or_declared_arity(closure, func_ptr, &args, 12) {
        return result;
    }
    unsafe {
        crate::closure::body_call::js_body_call!(
            func_ptr,
            closure,
            crate::closure::body_call::current_this(),
            arg0,
            arg1,
            arg2,
            arg3,
            arg4,
            arg5,
            arg6,
            arg7,
            arg8,
            arg9,
            arg10,
            arg11
        )
    }
}

/// Call a closure with 13 arguments, returning f64
#[inline(always)]
fn closure_call13_cell(
    closure: *const ClosureHeader,
    arg0: f64,
    arg1: f64,
    arg2: f64,
    arg3: f64,
    arg4: f64,
    arg5: f64,
    arg6: f64,
    arg7: f64,
    arg8: f64,
    arg9: f64,
    arg10: f64,
    arg11: f64,
    arg12: f64,
) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return dispatch_proxy_callee_or_throw(
            closure,
            &[
                arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12,
            ],
        );
    }
    let args = [
        arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12,
    ];
    if let Some(result) = dispatch_registered_call(closure, func_ptr, &args) {
        return result;
    }
    let args = [
        arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12,
    ];
    if let Some(result) = dispatch_rest_or_declared_arity(closure, func_ptr, &args, 13) {
        return result;
    }
    unsafe {
        crate::closure::body_call::js_body_call!(
            func_ptr,
            closure,
            crate::closure::body_call::current_this(),
            arg0,
            arg1,
            arg2,
            arg3,
            arg4,
            arg5,
            arg6,
            arg7,
            arg8,
            arg9,
            arg10,
            arg11,
            arg12
        )
    }
}

/// Call a closure with 14 arguments, returning f64
#[inline(always)]
fn closure_call14_cell(
    closure: *const ClosureHeader,
    arg0: f64,
    arg1: f64,
    arg2: f64,
    arg3: f64,
    arg4: f64,
    arg5: f64,
    arg6: f64,
    arg7: f64,
    arg8: f64,
    arg9: f64,
    arg10: f64,
    arg11: f64,
    arg12: f64,
    arg13: f64,
) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return dispatch_proxy_callee_or_throw(
            closure,
            &[
                arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12,
                arg13,
            ],
        );
    }
    let args = [
        arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12, arg13,
    ];
    if let Some(result) = dispatch_registered_call(closure, func_ptr, &args) {
        return result;
    }
    let args = [
        arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12, arg13,
    ];
    if let Some(result) = dispatch_rest_or_declared_arity(closure, func_ptr, &args, 14) {
        return result;
    }
    unsafe {
        crate::closure::body_call::js_body_call!(
            func_ptr,
            closure,
            crate::closure::body_call::current_this(),
            arg0,
            arg1,
            arg2,
            arg3,
            arg4,
            arg5,
            arg6,
            arg7,
            arg8,
            arg9,
            arg10,
            arg11,
            arg12,
            arg13
        )
    }
}

/// Call a closure with 15 arguments, returning f64
#[inline(always)]
fn closure_call15_cell(
    closure: *const ClosureHeader,
    arg0: f64,
    arg1: f64,
    arg2: f64,
    arg3: f64,
    arg4: f64,
    arg5: f64,
    arg6: f64,
    arg7: f64,
    arg8: f64,
    arg9: f64,
    arg10: f64,
    arg11: f64,
    arg12: f64,
    arg13: f64,
    arg14: f64,
) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return dispatch_proxy_callee_or_throw(
            closure,
            &[
                arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12,
                arg13, arg14,
            ],
        );
    }
    let args = [
        arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12, arg13,
        arg14,
    ];
    if let Some(result) = dispatch_registered_call(closure, func_ptr, &args) {
        return result;
    }
    let args = [
        arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12, arg13,
        arg14,
    ];
    if let Some(result) = dispatch_rest_or_declared_arity(closure, func_ptr, &args, 15) {
        return result;
    }
    unsafe {
        crate::closure::body_call::js_body_call!(
            func_ptr,
            closure,
            crate::closure::body_call::current_this(),
            arg0,
            arg1,
            arg2,
            arg3,
            arg4,
            arg5,
            arg6,
            arg7,
            arg8,
            arg9,
            arg10,
            arg11,
            arg12,
            arg13,
            arg14
        )
    }
}

/// Call a closure with 16 arguments, returning f64
#[inline(always)]
fn closure_call16_cell(
    closure: *const ClosureHeader,
    arg0: f64,
    arg1: f64,
    arg2: f64,
    arg3: f64,
    arg4: f64,
    arg5: f64,
    arg6: f64,
    arg7: f64,
    arg8: f64,
    arg9: f64,
    arg10: f64,
    arg11: f64,
    arg12: f64,
    arg13: f64,
    arg14: f64,
    arg15: f64,
) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return dispatch_proxy_callee_or_throw(
            closure,
            &[
                arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12,
                arg13, arg14, arg15,
            ],
        );
    }
    let args = [
        arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12, arg13,
        arg14, arg15,
    ];
    if let Some(result) = dispatch_registered_call(closure, func_ptr, &args) {
        return result;
    }
    let args = [
        arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12, arg13,
        arg14, arg15,
    ];
    if let Some(result) = dispatch_rest_or_declared_arity(closure, func_ptr, &args, 16) {
        return result;
    }
    unsafe {
        crate::closure::body_call::js_body_call!(
            func_ptr,
            closure,
            crate::closure::body_call::current_this(),
            arg0,
            arg1,
            arg2,
            arg3,
            arg4,
            arg5,
            arg6,
            arg7,
            arg8,
            arg9,
            arg10,
            arg11,
            arg12,
            arg13,
            arg14,
            arg15
        )
    }
}

/// `perry_abi::JS_CLOSURE_CALL_ENTRIES` is what codegen DECLARES; these are
/// the functions it links to. Each entry must name the function taking
/// exactly its index's JS argument count (the coercion below fails to compile
/// otherwise), in order.
#[cfg(test)]
mod abi_table_tests {
    use super::*;

    // An ENTRY takes the callee, the receiver and the JS arguments.
    macro_rules! entry {
        (@f64 $x:tt) => { f64 };
        ($f:ident; $($x:tt),*) => {{
            let f: extern "C" fn(*const ClosureHeader, JsThis $(, entry!(@f64 $x))*) -> f64 = $f;
            (stringify!($f), f as *const u8)
        }};
    }

    #[test]
    fn closure_call_entries_match_the_abi_table() {
        // `js_closure_call0` is `extern "C-unwind"` in unwinding (test) builds.
        let call0 = {
            let f: extern "C-unwind" fn(*const ClosureHeader, JsThis) -> f64 = js_closure_call0;
            ("js_closure_call0", f as *const u8)
        };
        let real = [
            call0,
            entry!(js_closure_call1; a),
            entry!(js_closure_call2; a, a),
            entry!(js_closure_call3; a, a, a),
            entry!(js_closure_call4; a, a, a, a),
            entry!(js_closure_call5; a, a, a, a, a),
            entry!(js_closure_call6; a, a, a, a, a, a),
            entry!(js_closure_call7; a, a, a, a, a, a, a),
            entry!(js_closure_call8; a, a, a, a, a, a, a, a),
            entry!(js_closure_call9; a, a, a, a, a, a, a, a, a),
            entry!(js_closure_call10; a, a, a, a, a, a, a, a, a, a),
            entry!(js_closure_call11; a, a, a, a, a, a, a, a, a, a, a),
            entry!(js_closure_call12; a, a, a, a, a, a, a, a, a, a, a, a),
            entry!(js_closure_call13; a, a, a, a, a, a, a, a, a, a, a, a, a),
            entry!(js_closure_call14; a, a, a, a, a, a, a, a, a, a, a, a, a, a),
            entry!(js_closure_call15; a, a, a, a, a, a, a, a, a, a, a, a, a, a, a),
            entry!(js_closure_call16; a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a),
        ];
        let table = crate::codegen_abi::JS_CLOSURE_CALL_ENTRIES;
        assert_eq!(real.len(), table.len());
        for (argc, ((name, ptr), declared)) in real.iter().zip(table.iter()).enumerate() {
            assert_eq!(name, declared, "JS_CLOSURE_CALL_ENTRIES[{argc}]");
            assert!(!ptr.is_null());
        }
        for name in table {
            assert!(
                crate::codegen_abi::JS_CALL_ENTRIES.contains(&name),
                "{name} is a JS-call entry but not in JS_CALL_ENTRIES"
            );
        }
    }
}

#[cfg(test)]
mod receiver_binding_tests {
    use super::*;

    extern "C" fn observe(_: *const ClosureHeader, this: JsThis, _: f64) -> f64 {
        // Stage 1: the body reads the cell; the parameter must agree with it.
        let cell = crate::object::js_implicit_this_get();
        assert_eq!(cell.to_bits(), this.bits(), "parameter and cell disagree");
        cell
    }

    #[test]
    fn an_entry_binds_the_cell_to_the_receiver_it_is_handed_and_restores_it() {
        let body = observe as *const u8;
        let closure = crate::closure::js_closure_alloc(body, 0);
        crate::closure::js_register_closure_arity(body, 1);

        let sentinel = 42.0;
        let original = crate::object::js_implicit_this_set(sentinel);
        // A plain runtime call passes the cell: the body sees it unchanged.
        let plain = js_closure_call1(closure, crate::closure::plain_call_receiver(), 0.0);
        assert_eq!(plain, sentinel);
        // An explicit receiver is what the body sees, and the cell comes back.
        let explicit = js_closure_call1(closure, JsThis::from_f64(7.0), 0.0);
        assert_eq!(explicit, 7.0);
        assert_eq!(crate::object::js_implicit_this_get(), sentinel);
        let undefined = js_closure_call1(closure, JsThis::UNDEFINED, 0.0);
        assert_eq!(undefined.to_bits(), crate::value::TAG_UNDEFINED);
        assert_eq!(crate::object::js_implicit_this_get(), sentinel);
        crate::object::js_implicit_this_set(original);
    }
}
