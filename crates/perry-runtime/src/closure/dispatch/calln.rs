//! Per-arity `js_closure_callN` FFI entry points (0..=16), the `resolve_call2_direct`
//! hot-loop helper, and the shared `dispatch_registered_call` /
//! `dispatch_rest_or_declared_arity` routing helpers.

use super::super::*;
use super::*;

/// Call a closure with 0 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call0(closure: *const ClosureHeader) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return dispatch_proxy_callee_or_throw(closure, &[]);
    }
    match resolve_strategy(func_ptr) {
        DispatchStrategy::BoundMethod => unsafe { dispatch_bound_method(closure, &[]) },
        DispatchStrategy::BoundFunction => unsafe { dispatch_bound_function(closure, &[]) },
        DispatchStrategy::Rest(fixed_arity, synth) => unsafe {
            dispatch_rest_bundled(closure, func_ptr, &[], fixed_arity, synth)
        },
        DispatchStrategy::Arity(declared) if declared > 0 => unsafe {
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
pub extern "C" fn js_closure_call1(closure: *const ClosureHeader, arg0: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return dispatch_proxy_callee_or_throw(closure, &[arg0]);
    }
    match resolve_strategy(func_ptr) {
        DispatchStrategy::BoundMethod => unsafe { dispatch_bound_method(closure, &[arg0]) },
        DispatchStrategy::BoundFunction => unsafe { dispatch_bound_function(closure, &[arg0]) },
        DispatchStrategy::Rest(fixed_arity, synth) => unsafe {
            dispatch_rest_bundled(closure, func_ptr, &[arg0], fixed_arity, synth)
        },
        DispatchStrategy::Arity(declared) if declared > 1 => unsafe {
            dispatch_with_arity(closure, func_ptr, &[arg0], declared)
        },
        _ => {
            let func: extern "C" fn(*const ClosureHeader, f64) -> f64 =
                unsafe { std::mem::transmute(func_ptr) };
            func(closure, arg0)
        }
    }
}

/// Resolve a 2-arg closure call once: returns Some(typed_fn_ptr) when
/// the closure can be invoked via a direct call without per-call
/// dispatch adjustments (no rest-bundling, no arity-padding, no
/// bound-method routing). Returns None when the call must go through
/// the slow `js_closure_call2` path. Hot loops that call the same
/// closure many times (e.g. `array.sort((a,b) => a-b)`) can hoist
/// this resolution out of the loop and skip ~50M HashMap lookups
/// over a 1.25M-element sort.
#[inline]
pub(crate) fn resolve_call2_direct(
    closure: *const ClosureHeader,
) -> Option<extern "C" fn(*const ClosureHeader, f64, f64) -> f64> {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null()
        || func_ptr == BOUND_METHOD_FUNC_PTR
        || func_ptr == BOUND_FUNCTION_FUNC_PTR
    {
        return None;
    }
    if lookup_closure_rest(func_ptr).is_some() {
        return None;
    }
    if let Some(declared) = lookup_closure_arity(func_ptr) {
        if declared > 2 {
            return None;
        }
    }
    Some(unsafe { std::mem::transmute(func_ptr) })
}

/// Call a closure with 2 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call2(closure: *const ClosureHeader, arg0: f64, arg1: f64) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return dispatch_proxy_callee_or_throw(closure, &[arg0, arg1]);
    }
    match resolve_strategy(func_ptr) {
        DispatchStrategy::BoundMethod => unsafe { dispatch_bound_method(closure, &[arg0, arg1]) },
        DispatchStrategy::BoundFunction => unsafe {
            dispatch_bound_function(closure, &[arg0, arg1])
        },
        DispatchStrategy::Rest(fixed_arity, synth) => unsafe {
            dispatch_rest_bundled(closure, func_ptr, &[arg0, arg1], fixed_arity, synth)
        },
        DispatchStrategy::Arity(declared) if declared > 2 => unsafe {
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
    match resolve_strategy(func_ptr) {
        DispatchStrategy::BoundMethod => unsafe {
            dispatch_bound_method(closure, &[arg0, arg1, arg2])
        },
        DispatchStrategy::BoundFunction => unsafe {
            dispatch_bound_function(closure, &[arg0, arg1, arg2])
        },
        DispatchStrategy::Rest(fixed_arity, synth) => unsafe {
            dispatch_rest_bundled(closure, func_ptr, &[arg0, arg1, arg2], fixed_arity, synth)
        },
        DispatchStrategy::Arity(declared) if declared > 3 => unsafe {
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
    match resolve_strategy(func_ptr) {
        DispatchStrategy::BoundMethod => unsafe {
            dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3])
        },
        DispatchStrategy::BoundFunction => unsafe {
            dispatch_bound_function(closure, &[arg0, arg1, arg2, arg3])
        },
        DispatchStrategy::Rest(fixed_arity, synth) => unsafe {
            dispatch_rest_bundled(
                closure,
                func_ptr,
                &[arg0, arg1, arg2, arg3],
                fixed_arity,
                synth,
            )
        },
        DispatchStrategy::Arity(declared) if declared > 4 => unsafe {
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
