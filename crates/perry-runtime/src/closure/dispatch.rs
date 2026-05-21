//! Closure dispatch: per-arity `js_closure_callN` entry points,
//! validation (`get_valid_func_ptr`), the not-callable error path,
//! `js_native_call_value`, and the V8 trampoline bridges
//! `js_closure_call_array` / `js_closure_call_apply_with_spread`.

use super::*;

/// Dispatch a bound method call with the given arguments.
/// Extracts the namespace object and method name from the closure captures,
/// then calls js_native_call_method with the packed arguments.
#[inline]
pub unsafe fn dispatch_bound_method(closure: *const ClosureHeader, args: &[f64]) -> f64 {
    let namespace_obj = js_closure_get_capture_f64(closure, 0);
    let method_name_ptr = js_closure_get_capture_ptr(closure, 1) as *const i8;
    let method_name_len = js_closure_get_capture_ptr(closure, 2) as usize;
    crate::object::js_native_call_method(
        namespace_obj,
        method_name_ptr,
        method_name_len,
        args.as_ptr(),
        args.len(),
    )
}

/// Issue #648: calling a value that isn't a function (most commonly the
/// result of a property lookup that returned undefined, e.g.
/// `obj.missingFn()`) must throw a TypeError that user code can catch via
/// `try { ... } catch`. Pre-fix every `js_closure_callN` (and the `_array`
/// / `_apply_with_spread` dispatch entry points) silently returned
/// TAG_UNDEFINED when `func_ptr` failed validation, which let
/// `obj.missingFn(1, 2)` quietly evaluate to `undefined` and continue —
/// the single biggest leverage source of cascading parity-test failures
/// (`test_parity_timers` hung forever waiting on `timers.setTimeout` which
/// silently no-op'd; `test_parity_os`/`tls`/`perf_hooks`/`http2`
/// truncated mid-script when an unimplemented binding silently no-op'd).
/// Now we throw via the existing `js_throw_type_error_not_a_function`
/// machinery, which routes through Perry's exception system so a
/// surrounding `try`/`catch` catches it (per #596).
// Issue #922 circuit breaker. Track consecutive `throw_not_callable`
// invocations on the current thread; abort if the count crosses the
// runaway bound. Mirrors the `record_warn_null_ptr` pattern in
// `object.rs` — production gscmaster-api Fastify route handlers
// (#921/#922) entered a 5.7M-iteration loop where every async-step
// catch arm re-fired the same TypeError, and the per-step-closure
// reentry guard at `promise.rs::ASYNC_STEP_GUARD` missed it because
// the loop alternated between two step closures. With this fixed
// upper bound the loop terminates in milliseconds with a single
// useful stderr line, instead of 5.7M `TypeError: value is not a
// function at <anonymous>` lines that drown out the diagnostic.
const THROW_NOT_CALLABLE_ABORT_LIMIT: u64 = 100_000;

thread_local! {
    static THROW_NOT_CALLABLE_COUNT: std::cell::Cell<u64>
        = const { std::cell::Cell::new(0) };
}

#[cold]
#[inline(never)]
pub fn throw_not_callable() -> ! {
    let count = THROW_NOT_CALLABLE_COUNT.with(|c| {
        let n = c.get().saturating_add(1);
        c.set(n);
        n
    });
    if count >= THROW_NOT_CALLABLE_ABORT_LIMIT {
        eprintln!(
            "[PERRY ABORT] throw_not_callable: detected runaway TypeError loop ({}+ consecutive 'value is not a function' throws -- issue #922 circuit breaker). Common cause: an async function throws across an await boundary inside try/catch where the catch arm re-enters the same await. Convert to a result-tag pattern (see issue #921 workaround). To find the offending callsite: recompile with --debug-symbols and run under a debugger -- set a breakpoint on js_throw_type_error_not_a_function.",
            THROW_NOT_CALLABLE_ABORT_LIMIT
        );
        std::process::abort();
    }
    crate::error::js_throw_type_error_not_a_function(std::ptr::null(), 0, b"value".as_ptr(), 5)
}

/// Reset the throw_not_callable counter — called by the async-step
/// driver whenever a non-error `is_error=false` step dispatches, which
/// signals progress (the catch arm advanced past the bad await). Lives
/// here so the thread-local is private to this module.
///
/// This exists as a `pub fn` (not `extern "C"`) — it's an internal
/// runtime-side reset called from `promise.rs::js_promise_run_microtasks`.
pub(crate) fn reset_throw_not_callable_counter() {
    THROW_NOT_CALLABLE_COUNT.with(|c| c.set(0));
}

/// Validate a closure pointer and return its func_ptr if the closure is valid.
///
/// Uses `read_volatile` for type_tag + `compiler_fence` to GUARANTEE that:
/// 1. CLOSURE_MAGIC is checked BEFORE func_ptr is ever read
/// 2. The optimizer cannot hoist the func_ptr read before the type_tag check
///
/// Background: `#[inline(never)]` on `is_valid_closure_ptr` is insufficient — LLVM
/// still speculatively hoists the non-volatile func_ptr load before the CLOSURE_MAGIC
/// check in the caller. This produces code that only checks CLOSURE_MAGIC when func_ptr==0,
/// allowing non-closure heap objects (Box<JSValue>, BigInt structs) to bypass validation
/// and execute their data as code via `br x1` → SIGBUS.
///
/// Returns null pointer if invalid (address out of range, wrong CLOSURE_MAGIC, bad func_ptr).
#[inline(always)]
pub fn get_valid_func_ptr(closure: *const ClosureHeader) -> *const u8 {
    let addr = closure as u64;
    if !(0x1000..0x0001_0000_0000_0000).contains(&addr) {
        return std::ptr::null();
    }
    let type_tag = unsafe { std::ptr::read_volatile((closure as *const u8).add(12) as *const u32) };
    if type_tag != CLOSURE_MAGIC {
        return std::ptr::null();
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    let func_ptr = unsafe { std::ptr::read_volatile(closure as *const *const u8) };
    let func_ptr_addr = func_ptr as usize;
    if func_ptr_addr == 0 {
        return std::ptr::null();
    }
    // Issue #628: BOUND_METHOD_FUNC_PTR (0xBADD_DEAD) is an intentional
    // sentinel — not a real code address. The js_closure_callN dispatch
    // handlers check for it explicitly and route to dispatch_bound_method
    // instead of transmuting func_ptr to a fn pointer. Pre-fix the macOS
    // code-range check below rejected the sentinel because 0xBADD_DEAD
    // (~3.1 GiB) sits below the 0x1_0000_0000 (4 GiB) lower bound, so
    // get_valid_func_ptr returned null and the closure-call returned
    // TAG_UNDEFINED before reaching the BOUND_METHOD_FUNC_PTR arm. Pass
    // the sentinel through here; the call sites handle it correctly.
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return func_ptr;
    }
    // Validate func_ptr is in a reasonable code address range.
    // macOS ARM64: .text starts at 0x100000000, typically < 0x400000000
    // Windows x86_64: typically 0x7FF7_xxxx_xxxx (ASLR), so we allow up to 0x8000_0000_0000
    // Linux x86_64 PIE: .text is typically in 0x55xxxxxxxxxx range
    // Skip this check on Linux since PIE addresses vary widely and CLOSURE_MAGIC
    // already provides strong validation.
    #[cfg(target_os = "macos")]
    if !(0x100000000..=0x400000000).contains(&func_ptr_addr) {
        return std::ptr::null();
    }
    #[cfg(target_os = "windows")]
    if func_ptr_addr < 0x10000 || func_ptr_addr > 0x800000000000 {
        return std::ptr::null();
    }
    func_ptr
}

/// Call a closure with 0 arguments, returning f64
#[no_mangle]
pub extern "C" fn js_closure_call0(closure: *const ClosureHeader) -> f64 {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        throw_not_callable();
    }
    match resolve_strategy(func_ptr) {
        DispatchStrategy::BoundMethod => unsafe { dispatch_bound_method(closure, &[]) },
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
        throw_not_callable();
    }
    match resolve_strategy(func_ptr) {
        DispatchStrategy::BoundMethod => unsafe { dispatch_bound_method(closure, &[arg0]) },
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
    if func_ptr.is_null() || func_ptr == BOUND_METHOD_FUNC_PTR {
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
        throw_not_callable();
    }
    match resolve_strategy(func_ptr) {
        DispatchStrategy::BoundMethod => unsafe { dispatch_bound_method(closure, &[arg0, arg1]) },
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
        throw_not_callable();
    }
    match resolve_strategy(func_ptr) {
        DispatchStrategy::BoundMethod => unsafe {
            dispatch_bound_method(closure, &[arg0, arg1, arg2])
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
        throw_not_callable();
    }
    match resolve_strategy(func_ptr) {
        DispatchStrategy::BoundMethod => unsafe {
            dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3])
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
        throw_not_callable();
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4]) };
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
        throw_not_callable();
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe { dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4, arg5]) };
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
        throw_not_callable();
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe {
            dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4, arg5, arg6])
        };
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
        throw_not_callable();
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe {
            dispatch_bound_method(closure, &[arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7])
        };
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
        throw_not_callable();
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe {
            dispatch_bound_method(
                closure,
                &[arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8],
            )
        };
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
        throw_not_callable();
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe {
            dispatch_bound_method(
                closure,
                &[arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9],
            )
        };
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
        throw_not_callable();
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe {
            dispatch_bound_method(
                closure,
                &[
                    arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10,
                ],
            )
        };
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
        throw_not_callable();
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe {
            dispatch_bound_method(
                closure,
                &[
                    arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11,
                ],
            )
        };
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
        throw_not_callable();
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe {
            dispatch_bound_method(
                closure,
                &[
                    arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11, arg12,
                ],
            )
        };
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
        throw_not_callable();
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe {
            dispatch_bound_method(
                closure,
                &[
                    arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11,
                    arg12, arg13,
                ],
            )
        };
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
        throw_not_callable();
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe {
            dispatch_bound_method(
                closure,
                &[
                    arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11,
                    arg12, arg13, arg14,
                ],
            )
        };
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
        throw_not_callable();
    }
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        return unsafe {
            dispatch_bound_method(
                closure,
                &[
                    arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11,
                    arg12, arg13, arg14, arg15,
                ],
            )
        };
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

/// Call a JavaScript function value with variable arguments
/// This is the native implementation for dynamic function dispatch.
/// func_value: NaN-boxed f64 containing a closure pointer
/// args_ptr: pointer to array of f64 arguments
/// args_len: number of arguments
/// Returns the result as f64
///
/// NOTE: This function is named js_native_call_value to avoid symbol collision
/// with js_call_value in perry-jsruntime which handles V8 JavaScript values.
#[no_mangle]
pub unsafe extern "C" fn js_native_call_value(
    func_value: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    use crate::value::JSValue;

    let jsval = JSValue::from_bits(func_value.to_bits());

    // Get the closure pointer from the value
    // For native compilation, function values are stored as NaN-boxed pointers
    let closure: *const ClosureHeader = if jsval.is_pointer() {
        jsval.as_pointer()
    } else if jsval.is_undefined() || jsval.is_null() || func_value.is_nan() {
        // TAG_UNDEFINED, TAG_NULL, or other NaN values are not callable
        return f64::from_bits(JSValue::undefined().bits());
    } else {
        // Try treating the value directly as a pointer (for i64 representation)
        func_value.to_bits() as *const ClosureHeader
    };

    if closure.is_null() {
        // Return undefined for null/invalid closures
        return f64::from_bits(JSValue::undefined().bits());
    }

    // Refs #421: when the closure body declares more params than the call site
    // provides, pad with TAG_UNDEFINED before dispatch. Without this, the
    // dispatch transmutes func_ptr to a lower-arity signature and the closure
    // body reads garbage for the missing slots — `c.text('hi')` (1 arg)
    // dispatching to a `(text, arg, headers)` arrow read the `headers` slot
    // from random stack memory, which evaluated truthy and fell into the
    // slow-path `#newResponse` chain that ended in `(number).set is not a
    // function`. Closures with rest params (`(a, ...rest) => …`) have their
    // own registry path via `lookup_closure_rest` which already pads, so we
    // skip the arity lookup when the rest registry has an entry.
    let func_ptr = get_valid_func_ptr(closure);
    let dispatch_args_len = if !func_ptr.is_null() && lookup_closure_rest(func_ptr).is_none() {
        match lookup_closure_arity(func_ptr) {
            Some(declared) if (declared as usize) > args_len => declared as usize,
            _ => args_len,
        }
    } else {
        args_len
    };

    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let arg_at = |i: usize| -> f64 {
        if i < args_len && !args_ptr.is_null() {
            unsafe { *args_ptr.add(i) }
        } else {
            undef
        }
    };

    // Call with the appropriate arity
    match dispatch_args_len {
        0 => js_closure_call0(closure),
        1 => js_closure_call1(closure, arg_at(0)),
        2 => js_closure_call2(closure, arg_at(0), arg_at(1)),
        3 => js_closure_call3(closure, arg_at(0), arg_at(1), arg_at(2)),
        4 => js_closure_call4(closure, arg_at(0), arg_at(1), arg_at(2), arg_at(3)),
        5 => js_closure_call5(
            closure,
            arg_at(0),
            arg_at(1),
            arg_at(2),
            arg_at(3),
            arg_at(4),
        ),
        6 => js_closure_call6(
            closure,
            arg_at(0),
            arg_at(1),
            arg_at(2),
            arg_at(3),
            arg_at(4),
            arg_at(5),
        ),
        7 => js_closure_call7(
            closure,
            arg_at(0),
            arg_at(1),
            arg_at(2),
            arg_at(3),
            arg_at(4),
            arg_at(5),
            arg_at(6),
        ),
        _ => js_closure_call8(
            closure,
            arg_at(0),
            arg_at(1),
            arg_at(2),
            arg_at(3),
            arg_at(4),
            arg_at(5),
            arg_at(6),
            arg_at(7),
        ),
    }
}

/// Adapter for V8's `native_callback_trampoline` (perry-jsruntime).
///
/// `js_create_callback(func_ptr, closure_env, param_count)` registers a JS
/// callable whose trampoline invokes `func_ptr(closure_env, args_ptr,
/// args_len)`. Perry closure bodies have signature
/// `(closure_ptr, arg0, arg1, ...)` per arity instead, so the codegen
/// arm for `Expr::JsCreateCallback` (issue #248 Phase 2B) passes
/// `js_closure_call_array` as the trampoline `func_ptr` and the raw
/// `*const ClosureHeader` (NaN-boxing stripped) as `closure_env`. The
/// trampoline then ends up calling THIS function, which dispatches to
/// the right `js_closure_callN` per `args_len`.
///
/// Mirrors `js_native_call_value` exactly but takes an i64 closure
/// pointer (already unboxed) instead of an f64 NaN-boxed value, so the
/// SysV-x64 / Win64 first-arg register lands in rdi/rcx (integer)
/// rather than xmm0 — matching the trampoline's `extern "C"` int-arg
/// expectation.
#[no_mangle]
pub unsafe extern "C" fn js_closure_call_array(
    closure_env: i64,
    args_ptr: *const f64,
    args_len: i64,
) -> f64 {
    let closure = closure_env as *const ClosureHeader;
    if closure.is_null() {
        throw_not_callable();
    }
    let n = if args_len < 0 { 0 } else { args_len as usize };

    // Issue #653 followup: route through `dispatch_rest_bundled` directly
    // when the closure body has a registered rest param, before falling
    // through to the per-arity `js_closure_callN` dispatchers. Pre-fix,
    // `js_closure_call7` through `js_closure_call16` skipped the
    // rest-bundling path entirely and trampolined the args list straight
    // through `mem::transmute`. With a wrapper registered for the rest
    // param at `fixed_arity = 2` (e.g. `function h(a, b, ...rest)`),
    // calling with 8 total args matched the call8 arm and called the
    // wrapper with 9 doubles when the wrapper signature is 4 doubles —
    // the receiver's `rest` parameter then read whatever happened to be
    // in the call's overflow registers, which the wrapper passed
    // through to the underlying user function as the rest array. Result:
    // `rest.length` came back as 0 because the actual rest array was
    // never built. Centralizing the dispatch here keeps the `callN`
    // arity-specific paths sound for direct-callee dispatch (which is
    // the dominant case for closure literals stored as locals) while
    // making the spread path correct for arities ≥ 7. The bound-method
    // routing has its own path inside `js_closure_callN` and isn't
    // affected here — we never see BOUND_METHOD_FUNC_PTR through this
    // entry because `js_closure_call_apply_with_spread`'s caller always
    // resolves a real closure pointer first.
    let fp_for_rest = get_valid_func_ptr(closure);
    if let Some((fixed_arity, synth)) = lookup_closure_rest_full(fp_for_rest) {
        let mut tmp: Vec<f64> = Vec::with_capacity(n);
        if !args_ptr.is_null() && n > 0 {
            for i in 0..n {
                let raw = *args_ptr.add(i);
                let bits = raw.to_bits();
                // Same INT32_TAG unboxing the per-arity dispatchers do
                // below — keep the body's `fadd` arithmetic working when
                // the args came from `v8_to_native`.
                let unboxed = if (bits & 0xFFFF_0000_0000_0000) == 0x7FFE_0000_0000_0000 {
                    ((bits & 0xFFFF_FFFF) as i32) as f64
                } else {
                    raw
                };
                tmp.push(unboxed);
            }
        }
        return dispatch_rest_bundled(closure, fp_for_rest, &tmp, fixed_arity, synth);
    }
    // Perry's closure-body arithmetic uses plain `fadd`/`fmul`/etc on
    // f64 inputs and assumes its arguments arrive as plain doubles, not
    // NaN-boxed values. perry-jsruntime's `v8_to_native` (bridge.rs:215)
    // NaN-boxes JS integers with INT32_TAG=0x7FFE. If we passed those
    // bits straight through, the closure body's `fadd` would produce a
    // NaN (whose payload happens to look like one of the operands when
    // re-decoded by `console.log`'s tag-aware unbox — which is why
    // `(a, b) => a + b` with `cb(10, 20)` returned 10 instead of 30
    // pre-fix). Unbox at the dispatch boundary so the body sees a
    // plain `20.0` not the NaN-boxed `0x7FFE_0000_0000_0014`. JS
    // doubles (non-int32) already arrive as plain f64 from
    // `v8_to_native`; only the INT32_TAG case needs unboxing here.
    let a = |i: usize| {
        if args_ptr.is_null() {
            return 0.0;
        }
        let raw = *args_ptr.add(i);
        let bits = raw.to_bits();
        if (bits & 0xFFFF_0000_0000_0000) == 0x7FFE_0000_0000_0000 {
            let int_val = (bits & 0xFFFF_FFFF) as i32;
            return int_val as f64;
        }
        raw
    };
    match n {
        0 => js_closure_call0(closure),
        1 => js_closure_call1(closure, a(0)),
        2 => js_closure_call2(closure, a(0), a(1)),
        3 => js_closure_call3(closure, a(0), a(1), a(2)),
        4 => js_closure_call4(closure, a(0), a(1), a(2), a(3)),
        5 => js_closure_call5(closure, a(0), a(1), a(2), a(3), a(4)),
        6 => js_closure_call6(closure, a(0), a(1), a(2), a(3), a(4), a(5)),
        7 => js_closure_call7(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6)),
        8 => js_closure_call8(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7)),
        9 => js_closure_call9(
            closure,
            a(0),
            a(1),
            a(2),
            a(3),
            a(4),
            a(5),
            a(6),
            a(7),
            a(8),
        ),
        10 => js_closure_call10(
            closure,
            a(0),
            a(1),
            a(2),
            a(3),
            a(4),
            a(5),
            a(6),
            a(7),
            a(8),
            a(9),
        ),
        11 => js_closure_call11(
            closure,
            a(0),
            a(1),
            a(2),
            a(3),
            a(4),
            a(5),
            a(6),
            a(7),
            a(8),
            a(9),
            a(10),
        ),
        12 => js_closure_call12(
            closure,
            a(0),
            a(1),
            a(2),
            a(3),
            a(4),
            a(5),
            a(6),
            a(7),
            a(8),
            a(9),
            a(10),
            a(11),
        ),
        13 => js_closure_call13(
            closure,
            a(0),
            a(1),
            a(2),
            a(3),
            a(4),
            a(5),
            a(6),
            a(7),
            a(8),
            a(9),
            a(10),
            a(11),
            a(12),
        ),
        14 => js_closure_call14(
            closure,
            a(0),
            a(1),
            a(2),
            a(3),
            a(4),
            a(5),
            a(6),
            a(7),
            a(8),
            a(9),
            a(10),
            a(11),
            a(12),
            a(13),
        ),
        15 => js_closure_call15(
            closure,
            a(0),
            a(1),
            a(2),
            a(3),
            a(4),
            a(5),
            a(6),
            a(7),
            a(8),
            a(9),
            a(10),
            a(11),
            a(12),
            a(13),
            a(14),
        ),
        _ => js_closure_call16(
            closure,
            a(0),
            a(1),
            a(2),
            a(3),
            a(4),
            a(5),
            a(6),
            a(7),
            a(8),
            a(9),
            a(10),
            a(11),
            a(12),
            a(13),
            a(14),
            a(15),
        ),
    }
}

/// Closure call with regular + spread args: `cb(reg0, reg1, ..., ...spread_arr)`.
///
/// Codegen lowers `closure(...args)` (or `closure(a, b, ...rest)`) at the
/// CallSpread arm by collecting regular arg slots into a stack buffer,
/// unboxing the spread source to an array handle, and calling this helper.
/// We concatenate `regular_args[0..regular_count]` with the array's
/// elements into a scratch buffer, then dispatch through
/// `js_closure_call_array`.
///
/// `closure_box` is a NaN-boxed closure value (the same shape that
/// `lower_expr` produces for a closure-typed expression). A null/undefined
/// box returns TAG_UNDEFINED.
#[no_mangle]
pub unsafe extern "C" fn js_closure_call_apply_with_spread(
    closure_box: f64,
    regular_args: *const f64,
    regular_count: i64,
    spread_arr_handle: i64,
) -> f64 {
    use crate::array::ArrayHeader;

    let bits = closure_box.to_bits();
    let closure_ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const ClosureHeader;
    if closure_ptr.is_null() {
        throw_not_callable();
    }

    let reg_n = if regular_count < 0 {
        0
    } else {
        regular_count as usize
    };

    let arr = spread_arr_handle as *const ArrayHeader;
    let (spread_n, spread_data): (usize, *const f64) = if arr.is_null() {
        (0, std::ptr::null())
    } else {
        let len = (*arr).length as usize;
        let data = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        (len, data)
    };

    let total = reg_n + spread_n;

    // Small fast path: stack buffer for up to 16 args (matches js_closure_call16).
    let mut stack_buf: [f64; 16] = [0.0; 16];
    let mut heap_buf: Vec<f64>;
    let buf_ptr: *const f64 = if total <= 16 {
        if !regular_args.is_null() && reg_n > 0 {
            std::ptr::copy_nonoverlapping(regular_args, stack_buf.as_mut_ptr(), reg_n);
        }
        if !spread_data.is_null() && spread_n > 0 {
            std::ptr::copy_nonoverlapping(spread_data, stack_buf.as_mut_ptr().add(reg_n), spread_n);
        }
        stack_buf.as_ptr()
    } else {
        heap_buf = vec![0.0; total];
        if !regular_args.is_null() && reg_n > 0 {
            std::ptr::copy_nonoverlapping(regular_args, heap_buf.as_mut_ptr(), reg_n);
        }
        if !spread_data.is_null() && spread_n > 0 {
            std::ptr::copy_nonoverlapping(spread_data, heap_buf.as_mut_ptr().add(reg_n), spread_n);
        }
        heap_buf.as_ptr()
    };

    js_closure_call_array(closure_ptr as i64, buf_ptr, total as i64)
}
