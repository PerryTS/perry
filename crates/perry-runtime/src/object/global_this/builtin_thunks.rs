use super::*;

pub(crate) extern "C" fn global_this_array_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let rest_value = crate::value::JSValue::from_bits(rest.to_bits());
    let args_arr = if rest_value.is_pointer() {
        rest_value.as_pointer::<crate::array::ArrayHeader>()
    } else {
        std::ptr::null()
    };
    let argc = crate::array::js_array_length(args_arr);
    if argc == 1 {
        let first = crate::array::js_array_get_f64(args_arr, 0);
        let arr = crate::array::js_array_constructor_single(first);
        return crate::value::js_nanbox_pointer(arr as i64);
    }
    let arr = crate::array::js_array_alloc(argc);
    unsafe {
        (*arr).length = argc;
        for i in 0..argc {
            let value = crate::array::js_array_get_f64(args_arr, i);
            crate::array::js_array_set_f64(arr, i, value);
        }
    }
    crate::value::js_nanbox_pointer(arr as i64)
}

pub(crate) extern "C" fn global_this_string_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let string_ptr = crate::builtins::js_string_coerce(value);
    crate::value::js_nanbox_string(string_ptr as i64)
}

pub(crate) extern "C" fn global_this_object_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::object::js_object_coerce(value)
}

pub(crate) extern "C" fn global_this_structured_clone_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
    _options: f64,
) -> f64 {
    crate::builtins::js_structured_clone(value)
}

pub(crate) extern "C" fn global_this_atob_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let decoded = crate::string::js_atob(value);
    crate::value::js_nanbox_string(decoded as i64)
}

pub(crate) extern "C" fn global_this_btoa_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let encoded = crate::string::js_btoa(value);
    crate::value::js_nanbox_string(encoded as i64)
}

pub(crate) extern "C" fn math_f16round_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::math::js_math_f16round(value)
}

pub(crate) extern "C" fn math_random_thunk(_closure: *const crate::closure::ClosureHeader) -> f64 {
    crate::math::js_math_random()
}

fn math_number_arg(value: f64) -> f64 {
    crate::math::js_math_to_number(value)
}

fn math_to_int32(value: f64) -> i32 {
    let n = math_number_arg(value);
    if !n.is_finite() || n == 0.0 {
        return 0;
    }
    const TWO_32: f64 = 4_294_967_296.0;
    (n.trunc().rem_euclid(TWO_32) as u32) as i32
}

fn math_to_uint32(value: f64) -> u32 {
    math_to_int32(value) as u32
}

macro_rules! math_unary_thunk {
    ($name:ident, $body:expr) => {
        pub(crate) extern "C" fn $name(
            _closure: *const crate::closure::ClosureHeader,
            value: f64,
        ) -> f64 {
            let x = math_number_arg(value);
            ($body)(x)
        }
    };
}

math_unary_thunk!(math_abs_thunk, |x: f64| x.abs());
math_unary_thunk!(math_acos_thunk, |x: f64| crate::math::js_math_acos(x));
math_unary_thunk!(math_acosh_thunk, |x: f64| crate::math::js_math_acosh(x));
math_unary_thunk!(math_asin_thunk, |x: f64| crate::math::js_math_asin(x));
math_unary_thunk!(math_asinh_thunk, |x: f64| crate::math::js_math_asinh(x));
math_unary_thunk!(math_atan_thunk, |x: f64| crate::math::js_math_atan(x));
math_unary_thunk!(math_atanh_thunk, |x: f64| crate::math::js_math_atanh(x));
math_unary_thunk!(math_cbrt_thunk, |x: f64| crate::math::js_math_cbrt(x));
math_unary_thunk!(math_ceil_thunk, |x: f64| x.ceil());
math_unary_thunk!(math_cos_thunk, |x: f64| crate::math::js_math_cos(x));
math_unary_thunk!(math_cosh_thunk, |x: f64| crate::math::js_math_cosh(x));
math_unary_thunk!(math_exp_thunk, |x: f64| x.exp());
math_unary_thunk!(math_expm1_thunk, |x: f64| crate::math::js_math_expm1(x));
math_unary_thunk!(math_floor_thunk, |x: f64| x.floor());
math_unary_thunk!(math_fround_thunk, |x: f64| crate::math::js_math_fround(x));
math_unary_thunk!(math_log_thunk, |x: f64| crate::math::js_math_log(x));
math_unary_thunk!(math_log10_thunk, |x: f64| crate::math::js_math_log10(x));
math_unary_thunk!(math_log1p_thunk, |x: f64| crate::math::js_math_log1p(x));
math_unary_thunk!(math_log2_thunk, |x: f64| crate::math::js_math_log2(x));
math_unary_thunk!(math_sin_thunk, |x: f64| crate::math::js_math_sin(x));
math_unary_thunk!(math_sinh_thunk, |x: f64| crate::math::js_math_sinh(x));
math_unary_thunk!(math_sqrt_thunk, |x: f64| x.sqrt());
math_unary_thunk!(math_tan_thunk, |x: f64| crate::math::js_math_tan(x));
math_unary_thunk!(math_tanh_thunk, |x: f64| crate::math::js_math_tanh(x));
math_unary_thunk!(math_trunc_thunk, |x: f64| x.trunc());

pub(crate) extern "C" fn math_round_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let x = math_number_arg(value);
    js_math_round_value(x)
}

/// ECMA-262 21.3.2.28 `Math.round`. The naive `floor(x + 0.5)` is wrong for
/// two families the spec calls out (test262 Math/round/S15.8.2.15_A7):
///   * `x = 0.5 - ε/4` (just under a half): `x + 0.5` rounds *up* to exactly
///     `1.0` in f64, so `floor` yields 1 — but the true value is < 0.5 and must
///     round to +0.
///   * large odd integers where `x + 0.5 == x` loses the `.5`.
/// Rounding via `r = floor(x)` then comparing the *exact* fractional part
/// `x - r` against 0.5 avoids the pre-rounding of the `+ 0.5` add. Half rounds
/// toward +∞ (the larger integer), and the `[-0.5, -0)` band returns -0.
pub(crate) fn js_math_round_value(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() || x.is_infinite() {
        return x;
    }
    let r = x.floor();
    let rounded = if x - r >= 0.5 { r + 1.0 } else { r };
    if rounded == 0.0 && x.is_sign_negative() {
        -0.0
    } else {
        rounded
    }
}

pub(crate) extern "C" fn math_sign_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::math::js_math_sign(value)
}

pub(crate) extern "C" fn math_clz32_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    math_to_uint32(value).leading_zeros() as f64
}

pub(crate) extern "C" fn math_atan2_thunk(
    _closure: *const crate::closure::ClosureHeader,
    y: f64,
    x: f64,
) -> f64 {
    crate::math::js_math_atan2(math_number_arg(y), math_number_arg(x))
}

pub(crate) extern "C" fn math_imul_thunk(
    _closure: *const crate::closure::ClosureHeader,
    a: f64,
    b: f64,
) -> f64 {
    crate::math::js_math_imul(a, b)
}

pub(crate) extern "C" fn math_pow_thunk(
    _closure: *const crate::closure::ClosureHeader,
    base: f64,
    exp: f64,
) -> f64 {
    crate::math::js_math_pow(math_number_arg(base), math_number_arg(exp))
}

pub(crate) extern "C" fn math_min_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let values = global_this_rest_array_values(rest);
    if values.is_empty() {
        return f64::INFINITY;
    }
    let mut result = f64::INFINITY;
    let mut saw_nan = false;
    for value in values {
        let n = math_number_arg(value);
        if n.is_nan() {
            saw_nan = true;
        } else if n < result || (n == 0.0 && result == 0.0 && n.is_sign_negative()) {
            result = n;
        }
    }
    if saw_nan {
        f64::NAN
    } else {
        result
    }
}

pub(crate) extern "C" fn math_max_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let values = global_this_rest_array_values(rest);
    if values.is_empty() {
        return f64::NEG_INFINITY;
    }
    let mut result = f64::NEG_INFINITY;
    let mut saw_nan = false;
    for value in values {
        let n = math_number_arg(value);
        if n.is_nan() {
            saw_nan = true;
        } else if n > result || (n == 0.0 && result == 0.0 && n.is_sign_positive()) {
            result = n;
        }
    }
    if saw_nan {
        f64::NAN
    } else {
        result
    }
}

pub(crate) extern "C" fn math_hypot_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let mut result = 0.0;
    for value in global_this_rest_array_values(rest) {
        result = crate::math::js_math_hypot(result, math_number_arg(value).abs());
    }
    result
}

// #2905: thunks for the standard global helper functions. Each coerces its
// arguments the same way the bare-call HIR lowering does and forwards to the
// shared runtime helper so a rebound / property-read reference matches Node.

pub(crate) extern "C" fn global_this_parse_int_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
    radix: f64,
) -> f64 {
    let s = crate::builtins::js_string_coerce(value);
    crate::builtins::js_parse_int(s, radix)
}

pub(crate) extern "C" fn global_this_parse_float_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let s = crate::builtins::js_string_coerce(value);
    crate::builtins::js_parse_float(s)
}

pub(crate) extern "C" fn global_this_is_nan_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::builtins::js_is_nan(value)
}

/// `globalThis.gc([force])` — request a garbage collection. This is the value
/// form of the same builtin the bare `gc()` call-intrinsic routes to, so
/// `globalThis.gc()`, `global.gc?.()`, `if (globalThis.gc) gc()`, and
/// `const f = gc; f()` all run a real collection. Perry's collector treats
/// `gc()` as a full collection, so Node's optional `force` argument is accepted
/// but ignored. Returns `undefined`.
pub(crate) extern "C" fn global_this_gc_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _force: f64,
) -> f64 {
    crate::gc::js_gc_collect();
    f64::from_bits(crate::value::JSValue::undefined().bits())
}

pub(crate) extern "C" fn global_this_is_finite_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::builtins::js_is_finite(value)
}

pub(crate) extern "C" fn global_this_encode_uri_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::value::js_nanbox_string(crate::builtins::js_encode_uri(value))
}

pub(crate) extern "C" fn global_this_decode_uri_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::value::js_nanbox_string(crate::builtins::js_decode_uri(value))
}

pub(crate) extern "C" fn global_this_encode_uri_component_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::value::js_nanbox_string(crate::builtins::js_encode_uri_component(value))
}

pub(crate) extern "C" fn global_this_decode_uri_component_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::value::js_nanbox_string(crate::builtins::js_decode_uri_component(value))
}

// #4511: legacy `escape()` / `unescape()` (ES Annex B). Used in the wild by
// `qs` for `%uXXXX` decoding, so any app pulling in `qs` (e.g. via `stripe`)
// needs them as real callable globalThis function values.
pub(crate) extern "C" fn global_this_escape_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::value::js_nanbox_string(crate::builtins::js_escape(value))
}

pub(crate) extern "C" fn global_this_unescape_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::value::js_nanbox_string(crate::builtins::js_unescape(value))
}

// #2889: call-form thunks for `Number`/`Boolean` global constructor values.
// `Object`/`String` already have dedicated thunks above; these mirror the
// bare-call HIR lowering (`Expr::NumberCoerce` / `Expr::BooleanCoerce`) so
// `const N = Number; N("42")` and `const B = Boolean; B(0)` match Node.
pub(crate) extern "C" fn global_this_number_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let jsv = crate::value::JSValue::from_bits(value.to_bits());
    if jsv.is_undefined() {
        // `Number()` with no args returns 0; an explicit `undefined` arg → NaN.
        // The closure-call path zero-fills missing args with TAG_UNDEFINED, so
        // we can't distinguish — match the common `Number()` → 0 case.
        return f64::from_bits(crate::value::JSValue::number(0.0).bits());
    }
    crate::builtins::js_number_coerce(value)
}

pub(crate) extern "C" fn global_this_boolean_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let b = crate::value::js_is_truthy(value) != 0;
    f64::from_bits(crate::value::JSValue::bool(b).bits())
}

pub(crate) extern "C" fn global_this_error_capture_stack_trace_thunk(
    _closure: *const crate::closure::ClosureHeader,
    target: f64,
    constructor_opt: f64,
) -> f64 {
    crate::error::js_error_capture_stack_trace(target, constructor_opt)
}

/// #2904: `Error.isError(value)` thunk — delegates to the runtime duck-check.
pub(crate) extern "C" fn global_this_error_is_error_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    crate::error::js_error_is_error(value)
}

/// `new Function(...)` with a RUNTIME-constructed body. Static/const bodies are
/// AOT-compiled in HIR; only dynamic ones reach here. Perry has no JS
/// interpreter, but it CAN recognize the fixed templates a few popular codegen
/// libraries emit and return a real native function. Currently: `depd`'s
/// deprecation wrapper (used eagerly by `send` → Next.js). depd's wrapper just
/// logs a deprecation then forwards to the wrapped fn, so the "wrapper" can
/// simply BE that fn — `new Function(...)(fn,log,deprecate,msg,site)` returns
/// `fn`. Unrecognized templates fall back to a non-callable placeholder object
/// (prior behavior); there is no general eval.
#[no_mangle]
pub extern "C" fn js_function_ctor_from_strings(args_ptr: *const f64, args_len: usize) -> f64 {
    let arg_str = |i: usize| -> String {
        if i >= args_len || args_ptr.is_null() {
            return String::new();
        }
        let v = unsafe { *args_ptr.add(i) };
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        match crate::string::str_bytes_from_jsvalue(v, &mut scratch) {
            Some((p, n)) if !p.is_null() => {
                let bytes = unsafe { std::slice::from_raw_parts(p, n as usize) };
                std::str::from_utf8(bytes).unwrap_or("").to_string()
            }
            _ => String::new(),
        }
    };
    // depd `wrapfunction`: `new Function("fn","log","deprecate","message",
    // "site", '…return function (…) { log.call(deprecate, message, site)\n
    // return fn.apply(this, arguments)\n}')`. The outer, called with
    // (fn,log,deprecate,message,site), returns that wrapper. Match the FULL
    // shape — exactly six args, the five parameter names verbatim, AND the
    // body substrings — so an unrelated dynamic Function body that happens to
    // contain the substrings isn't misclassified as depd's wrapper.
    if args_len == 6
        && arg_str(0) == "fn"
        && arg_str(1) == "log"
        && arg_str(2) == "deprecate"
        && arg_str(3) == "message"
        && arg_str(4) == "site"
    {
        let body = arg_str(5);
        if body.contains("return function (")
            && body.contains("log.call(deprecate, message, site)")
            && body.contains("return fn.apply(this, arguments)")
        {
            let fp = depd_wrapfunction_outer_thunk as *const u8;
            crate::closure::js_register_closure_arity(fp, 5);
            let closure = crate::closure::js_closure_alloc_singleton(fp);
            if !closure.is_null() {
                return crate::value::js_nanbox_pointer(closure as i64);
            }
        }
    }
    // #6559: unrecognized dynamic-Function shape → the scoped interpreter.
    // The generated source is parsed with perry-parser (the compiler's own
    // SWC front end) and evaluated by the tree-walking interpreter in
    // `crate::dyn_eval`; the returned value is a first-class runtime closure.
    // This is what ajv / fast-json-stringify / find-my-way (fastify) need —
    // their codegen has NO non-`Function` fallback. Constructs outside the
    // interpreter subset throw a diagnostic TypeError naming the construct;
    // sources that don't parse throw a SyntaxError (matching Node) — so
    // feature-probing libraries (zod's JIT probe, #6031) still get an honest
    // signal: the probe now SUCCEEDS and their generated code runs
    // interpreted.
    #[cfg(feature = "dyn-eval")]
    {
        let args_vec: Vec<String> = (0..args_len).map(arg_str).collect();
        return crate::dyn_eval::dyn_function_from_strings(&args_vec);
    }
    // Without the `dyn-eval` feature (size-optimized builds that carry no
    // dynamic-eval site), keep the historical clean throw: it lets
    // feature-detecting libraries take their non-`Function` fallback. The
    // eprintln names the offending library for diagnostics.
    #[cfg(not(feature = "dyn-eval"))]
    {
        let body = if args_len > 0 {
            arg_str(args_len - 1)
        } else {
            String::new()
        };
        let preview: String = body.chars().take(160).collect();
        eprintln!(
            "[perry] dynamic Function refused (AOT, dyn-eval feature off) — {} arg(s); body[..160]={:?}",
            args_len, preview
        );
        super::super::object_ops::throw_object_type_error(
            b"Function: dynamic code generation from a runtime string is not supported \
              in an ahead-of-time compiled binary",
        )
    }
}

/// depd `wrapfunction` outer `(fn, log, deprecate, message, site) => wrapper`.
/// The wrapper forwards to `fn` (deprecation logging dropped — a non-essential
/// warning), so return `fn` itself: calling the "deprecated" function calls the
/// real one with identical `this`/arguments.
extern "C" fn depd_wrapfunction_outer_thunk(
    _closure: *const crate::closure::ClosureHeader,
    fn_v: f64,
    _log: f64,
    _deprecate: f64,
    _message: f64,
    _site: f64,
) -> f64 {
    fn_v
}

#[used]
static KEEP_JS_FUNCTION_CTOR_FROM_STRINGS: extern "C" fn(*const f64, usize) -> f64 =
    js_function_ctor_from_strings;

/// #2904: `Error.prepareStackTrace` default — Node leaves a hook here that
/// formats the stack from structured frames. Perry's stack strings are
/// coarse; the installed default returns the existing `error.stack` string
/// (or empty) so `typeof Error.prepareStackTrace === "function"` holds and
/// callers that invoke it get a usable string rather than a crash.
pub(crate) extern "C" fn global_this_error_prepare_stack_trace_thunk(
    _closure: *const crate::closure::ClosureHeader,
    error: f64,
    _structured_stack: f64,
) -> f64 {
    let jsval = crate::value::JSValue::from_bits(error.to_bits());
    if jsval.is_pointer() {
        let ptr = crate::value::js_nanbox_get_pointer(error) as *mut crate::error::ErrorHeader;
        if !ptr.is_null() {
            let stack = crate::error::js_error_get_stack(ptr);
            if !stack.is_null() {
                return crate::value::js_nanbox_string(stack as i64);
            }
        }
    }
    let empty = crate::string::js_string_from_bytes(b"".as_ptr(), 0);
    crate::value::js_nanbox_string(empty as i64)
}

/// `Proxy.revocable(target, handler)` static method thunk. Delegates to the
/// existing `js_proxy_revocable` implementation in `crate::proxy`.
pub(crate) extern "C" fn proxy_revocable_thunk(
    _closure: *const crate::closure::ClosureHeader,
    target: f64,
    handler: f64,
) -> f64 {
    crate::proxy::js_proxy_revocable(target, handler)
}
