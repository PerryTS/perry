//! Math's ordered birth declaration, carried only by its feature hook.
use super::builtin_namespace::{self, Member};
use super::builtin_thunks::*;
use crate::object::ObjectHeader;

// Preserve the existing own-key order, including f16round after the constants.
const MEMBERS: &[Member] = &[
    Member::method(
        "abs",
        crate::fn_info!(math_abs_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "acos",
        crate::fn_info!(math_acos_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "acosh",
        crate::fn_info!(math_acosh_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "asin",
        crate::fn_info!(math_asin_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "asinh",
        crate::fn_info!(math_asinh_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "atan",
        crate::fn_info!(math_atan_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "atanh",
        crate::fn_info!(math_atanh_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "atan2",
        crate::fn_info!(math_atan2_thunk, 2; with_declared(2), with_flags(crate::closure::FN_BUILTIN)),
        2,
    ),
    Member::method(
        "ceil",
        crate::fn_info!(math_ceil_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "cbrt",
        crate::fn_info!(math_cbrt_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "expm1",
        crate::fn_info!(math_expm1_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "clz32",
        crate::fn_info!(math_clz32_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "cos",
        crate::fn_info!(math_cos_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "cosh",
        crate::fn_info!(math_cosh_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "exp",
        crate::fn_info!(math_exp_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "floor",
        crate::fn_info!(math_floor_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "fround",
        crate::fn_info!(math_fround_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "hypot",
        crate::fn_info!(math_hypot_thunk, 1; with_rest(0), with_flags(crate::closure::FN_BUILTIN)),
        2,
    ),
    Member::method(
        "imul",
        crate::fn_info!(math_imul_thunk, 2; with_declared(2), with_flags(crate::closure::FN_BUILTIN)),
        2,
    ),
    Member::method(
        "log",
        crate::fn_info!(math_log_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "log1p",
        crate::fn_info!(math_log1p_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "log2",
        crate::fn_info!(math_log2_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "log10",
        crate::fn_info!(math_log10_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "max",
        crate::fn_info!(math_max_thunk, 1; with_rest(0), with_flags(crate::closure::FN_BUILTIN)),
        2,
    ),
    Member::method(
        "min",
        crate::fn_info!(math_min_thunk, 1; with_rest(0), with_flags(crate::closure::FN_BUILTIN)),
        2,
    ),
    Member::method(
        "pow",
        crate::fn_info!(math_pow_thunk, 2; with_declared(2), with_flags(crate::closure::FN_BUILTIN)),
        2,
    ),
    Member::method(
        "random",
        crate::fn_info!(math_random_thunk, 0; with_declared(0), with_flags(crate::closure::FN_BUILTIN)),
        0,
    ),
    Member::method(
        "round",
        crate::fn_info!(math_round_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "sign",
        crate::fn_info!(math_sign_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "sin",
        crate::fn_info!(math_sin_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "sinh",
        crate::fn_info!(math_sinh_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "sqrt",
        crate::fn_info!(math_sqrt_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "tan",
        crate::fn_info!(math_tan_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "tanh",
        crate::fn_info!(math_tanh_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "trunc",
        crate::fn_info!(math_trunc_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::constant("E", std::f64::consts::E),
    Member::constant("LN10", std::f64::consts::LN_10),
    Member::constant("LN2", std::f64::consts::LN_2),
    Member::constant("LOG10E", std::f64::consts::LOG10_E),
    Member::constant("LOG2E", std::f64::consts::LOG2_E),
    Member::constant("PI", std::f64::consts::PI),
    Member::constant("SQRT1_2", std::f64::consts::FRAC_1_SQRT_2),
    Member::constant("SQRT2", std::f64::consts::SQRT_2),
    Member::method(
        "f16round",
        crate::fn_info!(math_f16round_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::tag("Math"),
];

#[cold]
#[inline(never)]
pub(crate) fn create_math_namespace() -> *mut ObjectHeader {
    builtin_namespace::create(MEMBERS)
}

// Realm facts owned here; source-only until image emission.
crate::realm_decl!("math_namespace.realm.json#objects");
