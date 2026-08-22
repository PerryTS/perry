//! Per-arity `js_closure_callN` FFI entry points (0..=16) and the shared
//! `dispatch_registered_call` / `dispatch_rest_or_declared_arity` routing
//! helpers.
//!
//! The hot-loop counterpart -- resolve a closure ONCE and call it directly for
//! the rest of the loop -- lives in the sibling `direct` module (#8180). It
//! subsumes the `resolve_call2_direct` helper that used to sit here and had
//! exactly one consumer.

use super::*;

/// Call a closure with 0 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call0(closure: *const ClosureHeader) -> f64 {
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
        DispatchKind::Arity(declared) if declared > 0 => unsafe {
            dispatch_with_arity(closure, func_ptr, &[], declared)
        },
        _ => {
            let func: extern "C" fn(*const ClosureHeader) -> f64 =
                unsafe { std::mem::transmute(func_ptr) };
            func(closure)
        }
    }
}

/// Call a closure with 1 argument, returning f64
#[no_mangle]
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
pub extern "C" fn js_closure_call1(closure: *const ClosureHeader, arg0: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return dispatch_proxy_callee_or_throw(closure, &[arg0]);
    }
    dispatch_call1_resolved(closure, func_ptr, arg0, resolve_strategy(func_ptr))
}

#[inline(always)]
fn dispatch_call1_resolved(
    closure: *const ClosureHeader,
    func_ptr: *const u8,
    arg0: f64,
    strategy: DispatchStrategy,
) -> f64 {
    match strategy.kind() {
        DispatchKind::BoundMethod => unsafe { dispatch_bound_method(closure, &[arg0]) },
        DispatchKind::BoundFunction => unsafe { dispatch_bound_function(closure, &[arg0]) },
        DispatchKind::Rest(fixed_arity, synth) => unsafe {
            dispatch_rest_bundled(closure, func_ptr, &[arg0], fixed_arity, synth)
        },
        DispatchKind::Arity(declared) if declared > 1 => unsafe {
            dispatch_with_arity(closure, func_ptr, &[arg0], declared)
        },
        _ => {
            let func: extern "C" fn(*const ClosureHeader, f64) -> f64 =
                unsafe { std::mem::transmute(func_ptr) };
            func(closure, arg0)
        }
    }
}

/// Receiverless one-argument call. Arrow functions have lexical `this`, so
/// resetting the dynamic `IMPLICIT_THIS` cell around them is unobservable and
/// needlessly expensive in callback pipelines. Arrow-ness is already folded
/// into the unified dispatch cache; non-arrows retain OrdinaryCallBindThis and
/// root the displaced receiver across arbitrary generated code.
#[no_mangle]
// Same unwind contract as `js_closure_call1` above: keep `extern "C"`, not
// `extern "C-unwind"`, so raw JS exceptions can traverse this bridge.
pub extern "C" fn js_closure_call1_receiverless(closure: *const ClosureHeader, arg0: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    let strategy = (!func_ptr.is_null()).then(|| resolve_strategy(func_ptr));
    if let Some(arrow_strategy) = strategy.filter(|strategy| strategy.is_arrow()) {
        return dispatch_call1_resolved(closure, func_ptr, arg0, arrow_strategy);
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    let previous_this = crate::object::js_implicit_this_set(undefined);
    let previous_this_handle = scope.root_nanbox_f64(previous_this);
    let result = match strategy {
        Some(strategy) => dispatch_call1_resolved(closure, func_ptr, arg0, strategy),
        None => dispatch_proxy_callee_or_throw(closure, &[arg0]),
    };
    crate::object::js_implicit_this_set(previous_this_handle.get_nanbox_f64());
    result
}

/// Call a closure with 2 arguments, returning f64
#[no_mangle]
// A dynamically-dispatched closure can throw into a generated caller's catch
// landing pad; this bridge is on Next's loadManifest/readFileSync path.
pub extern "C" fn js_closure_call2(closure: *const ClosureHeader, arg0: f64, arg1: f64) -> f64 {
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
        DispatchKind::Arity(declared) if declared > 2 => unsafe {
            dispatch_with_arity(closure, func_ptr, &[arg0, arg1], declared)
        },
        _ => {
            let func: extern "C" fn(*const ClosureHeader, f64, f64) -> f64 =
                unsafe { std::mem::transmute(func_ptr) };
            func(closure, arg0, arg1)
        }
    }
}

/// Call a closure with 3 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call3(
    closure: *const ClosureHeader,
    arg0: f64,
    arg1: f64,
    arg2: f64,
) -> f64 {
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
        DispatchKind::Arity(declared) if declared > 3 => unsafe {
            dispatch_with_arity(closure, func_ptr, &[arg0, arg1, arg2], declared)
        },
        _ => {
            let func: extern "C" fn(*const ClosureHeader, f64, f64, f64) -> f64 =
                unsafe { std::mem::transmute(func_ptr) };
            func(closure, arg0, arg1, arg2)
        }
    }
}

/// Call a closure with 4 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call4(
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
        DispatchKind::Arity(declared) if declared > 4 => unsafe {
            dispatch_with_arity(closure, func_ptr, &[arg0, arg1, arg2, arg3], declared)
        },
        _ => {
            let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64) -> f64 =
                unsafe { std::mem::transmute(func_ptr) };
            func(closure, arg0, arg1, arg2, arg3)
        }
    }
}

/// Call a closure with 5 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call5(
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
    if let Some(declared) = lookup_closure_arity(func_ptr) {
        if declared > 5 {
            return unsafe {
                dispatch_with_arity(closure, func_ptr, &[arg0, arg1, arg2, arg3, arg4], declared)
            };
        }
    }
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64) -> f64 =
        unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3, arg4)
}

/// Call a closure with 6 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call6(
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
    if let Some(declared) = lookup_closure_arity(func_ptr) {
        if declared > 6 {
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
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64) -> f64 =
        unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3, arg4, arg5)
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
    if let Some(declared) = lookup_closure_arity(func_ptr) {
        if declared > provided {
            return Some(unsafe { dispatch_with_arity(closure, func_ptr, args, declared) });
        }
    }
    None
}

/// Call a closure with 7 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call7(
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
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64, f64) -> f64 =
        unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6)
}

/// Call a closure with 8 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call8(
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
    let func: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 =
        unsafe { std::mem::transmute(func_ptr) };
    func(closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7)
}

/// Call a closure with 9 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call9(
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
    let func: extern "C" fn(
        *const ClosureHeader,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
    ) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(
        closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8,
    )
}

/// Call a closure with 10 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call10(
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
    let func: extern "C" fn(
        *const ClosureHeader,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
    ) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(
        closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9,
    )
}

/// Call a closure with 11 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call11(
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
    let func: extern "C" fn(
        *const ClosureHeader,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
    ) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(
        closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10,
    )
}

/// Call a closure with 12 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call12(
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
    let func: extern "C" fn(
        *const ClosureHeader,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
    ) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(
        closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11,
    )
}

/// Call a closure with 13 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call13(
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
    let func: extern "C" fn(
        *const ClosureHeader,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
    ) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(
        closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12,
    )
}

/// Call a closure with 14 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call14(
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
    let func: extern "C" fn(
        *const ClosureHeader,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
    ) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(
        closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12,
        arg13,
    )
}

/// Call a closure with 15 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call15(
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
    let func: extern "C" fn(
        *const ClosureHeader,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
    ) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(
        closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12,
        arg13, arg14,
    )
}

/// Call a closure with 16 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call16(
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
    let func: extern "C" fn(
        *const ClosureHeader,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
    ) -> f64 = unsafe { std::mem::transmute(func_ptr) };
    func(
        closure, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12,
        arg13, arg14, arg15,
    )
}

#[cfg(test)]
mod receiverless_tests {
    use super::*;

    extern "C" fn observe_dynamic_this(_: *const ClosureHeader, _: f64) -> f64 {
        crate::object::js_implicit_this_get()
    }

    #[test]
    fn one_arg_receiverless_dispatch_only_resets_this_for_non_arrows() {
        let body = observe_dynamic_this as *const u8;
        let closure = crate::closure::js_closure_alloc(body, 0);
        crate::closure::js_register_closure_arity(body, 1);

        let sentinel = 42.0;
        let original = crate::object::js_implicit_this_set(sentinel);
        let regular_result = js_closure_call1_receiverless(closure, 0.0);
        assert_eq!(regular_result.to_bits(), crate::value::TAG_UNDEFINED);
        assert_eq!(crate::object::js_implicit_this_get(), sentinel);

        crate::closure::js_register_closure_arrow_function(body);
        let arrow_result = js_closure_call1_receiverless(closure, 0.0);
        assert_eq!(arrow_result, sentinel);
        assert_eq!(crate::object::js_implicit_this_get(), sentinel);
        crate::object::js_implicit_this_set(original);
    }
}
