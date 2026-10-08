//! Function-body facts, dispatch-strategy resolution, and rest-arg / arity-pad
//! helpers.
//!
//! Every fact a caller needs about a function object's BODY — its parameter
//! count, rest kind, `.length`, arrow / strict / async / generator /
//! built-in bits, and the compiler-private direct-call clones — lives in the
//! body's one static [`JsFunctionInfo`] (perry-abi), which the function object
//! points to from its header. Nothing is looked up by code address: a fact is
//! one load from the object's info.

use super::*;

pub use crate::codegen_abi::{
    JsFunctionInfo, FN_ARROW, FN_ASYNC, FN_ASYNC_GENERATOR, FN_BUILTIN, FN_GENERATOR,
    FN_HAS_DECLARED, FN_HAS_LENGTH, FN_NON_CONSTRUCTOR, FN_REST_MASK, FN_REST_NATIVE_ARGS,
    FN_REST_SYNTHETIC_ARGUMENTS, FN_REST_USER, FN_REST_USER_AND_ARGUMENTS, FN_STRICT,
};

/// A compiler-private direct-call clone of an arrow body, from its info.
#[derive(Clone, Copy)]
pub(crate) struct TrustedDirectTarget {
    pub func_ptr: *const u8,
    pub capture_count: u32,
    pub boxed_capture_mask: u64,
}

/// The info of the function object `closure`, if it is a live, valid one
/// (the bound-method / bound-function sentinel infos included).
#[inline(always)]
pub fn closure_info(closure: *const ClosureHeader) -> Option<&'static JsFunctionInfo> {
    let info = get_valid_info(closure);
    // SAFETY: a validated header's info word points at a static info.
    (!info.is_null()).then(|| unsafe { &*info })
}

/// The dispatch strategy `info` implies: pure bit tests on the body's own
/// record.
#[inline(always)]
pub fn resolve_strategy(info: &JsFunctionInfo) -> DispatchStrategy {
    let kind = if info.code == BOUND_METHOD_FUNC_PTR {
        DispatchKind::BoundMethod
    } else if info.code == BOUND_FUNCTION_FUNC_PTR {
        DispatchKind::BoundFunction
    } else if let Some((fixed, kind)) = info_rest(info) {
        DispatchKind::Rest(fixed, kind)
    } else {
        DispatchKind::Arity(u32::from(info.params))
    };
    DispatchStrategy { kind }
}

/// `info`'s rest kind and its fixed parameter count, if it has one.
#[inline(always)]
pub fn info_rest(info: &JsFunctionInfo) -> Option<(u32, RestDispatchKind)> {
    // One mask test answers every body without a rest kind, the case each
    // dynamic call takes.
    if info.flags & FN_REST_MASK == 0 {
        return None;
    }
    let kind = if info.flags & FN_REST_USER != 0 {
        RestDispatchKind::UserRest
    } else if info.flags & FN_REST_SYNTHETIC_ARGUMENTS != 0 {
        RestDispatchKind::SyntheticArguments
    } else if info.flags & FN_REST_USER_AND_ARGUMENTS != 0 {
        RestDispatchKind::UserRestAndArguments
    } else if info.flags & FN_REST_NATIVE_ARGS != 0 {
        RestDispatchKind::NativeArgs
    } else {
        return None;
    };
    Some((u32::from(info.rest_fixed), kind))
}

/// `info`'s ECMAScript `.length`, if it records one.
#[inline(always)]
pub fn info_length(info: &JsFunctionInfo) -> Option<u32> {
    (info.flags & FN_HAS_LENGTH != 0).then_some(info.length)
}

/// Whether `info` has every bit of `flags`.
#[inline(always)]
pub fn info_has(info: &JsFunctionInfo, flags: u32) -> bool {
    info.flags & flags == flags
}

/// The trusted direct-call clone of `info`'s arrow body, if it has one.
#[inline(always)]
pub(crate) fn info_trusted_direct(info: &JsFunctionInfo) -> Option<TrustedDirectTarget> {
    (!info.trusted_code.is_null()).then(|| TrustedDirectTarget {
        func_ptr: info.trusted_code,
        capture_count: info.trusted_captures,
        boxed_capture_mask: info.trusted_boxed_mask,
    })
}

/// The versioned-loop callback clone of `info`'s body, if it has one.
#[inline(always)]
pub(crate) fn info_versioned_loop_direct(info: &JsFunctionInfo) -> Option<TrustedDirectTarget> {
    (!info.versioned_code.is_null()).then(|| TrustedDirectTarget {
        func_ptr: info.versioned_code,
        capture_count: info.versioned_captures,
        boxed_capture_mask: info.versioned_boxed_mask,
    })
}

/// Whether a call of `info`'s body receives a primitive receiver unboxed
/// (strict code or a built-in, ECMA-262 §10.2.1.2 / §10.3.1).
#[inline(always)]
pub(crate) fn info_receives_primitive_this(info: &JsFunctionInfo) -> bool {
    info.flags & (FN_STRICT | FN_BUILTIN) != 0
}

/// Whether `closure` is an arrow function.
pub fn closure_is_arrow(closure: *const ClosureHeader) -> bool {
    closure_info(closure).is_some_and(|info| info.flags & FN_ARROW != 0)
}

/// Whether `closure` is a bound-method value.
pub fn closure_is_bound_method(closure: *const ClosureHeader) -> bool {
    get_valid_func_ptr(closure) == BOUND_METHOD_FUNC_PTR
}

/// Whether `closure`'s body is a built-in kind without `[[Construct]]`.
pub(crate) fn closure_body_is_non_constructor(closure: *const ClosureHeader) -> bool {
    closure_info(closure).is_some_and(|info| info.flags & FN_NON_CONSTRUCTOR != 0)
}

/// `closure`'s JS-visible declared parameter count: the fixed part of a rest
/// body's, else the body's recorded declared count.
pub fn closure_arity(closure: *const ClosureHeader) -> Option<u32> {
    info_arity(closure_info(closure)?)
}

/// [`closure_arity`] of a body.
#[inline(always)]
pub fn info_arity(info: &JsFunctionInfo) -> Option<u32> {
    if let Some((fixed, _)) = info_rest(info) {
        return Some(fixed);
    }
    (info.flags & FN_HAS_DECLARED != 0).then_some(u32::from(info.declared))
}

/// `closure`'s ECMAScript-visible `.length`: a per-closure override
/// (`builtin_closure_length`), else its body's `.length`, else its declared
/// parameter count ([`closure_arity`]).
pub fn closure_length(closure: *const ClosureHeader) -> Option<u32> {
    if let Some(length) = crate::object::builtin_closure_length(closure as usize) {
        return Some(length);
    }
    let info = closure_info(closure)?;
    info_length(info).or_else(|| info_arity(info))
}

/// Per-call dispatch strategy for a closure body, derived from its info.
#[derive(Clone, Copy)]
pub struct DispatchStrategy {
    kind: DispatchKind,
}

impl DispatchStrategy {
    #[inline(always)]
    pub(crate) fn kind(self) -> DispatchKind {
        self.kind
    }
}

#[derive(Clone, Copy)]
pub(crate) enum DispatchKind {
    /// Bound-method receiver pretending to be a closure (BOUND_METHOD_FUNC_PTR
    /// sentinel). Dispatch via `dispatch_bound_method`.
    BoundMethod,
    /// `Function.prototype.bind` result (BOUND_FUNCTION_FUNC_PTR sentinel).
    /// Dispatch via `dispatch_bound_function`.
    BoundFunction,
    /// Closure body has a rest-like runtime bundling requirement.
    Rest(u32, RestDispatchKind),
    /// A body declaring this many parameters: a call passing at least as
    /// many calls it directly (over-application is safe); one passing fewer
    /// pads with TAG_UNDEFINED via `dispatch_with_arity`.
    Arity(u32),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RestDispatchKind {
    UserRest,
    SyntheticArguments,
    UserRestAndArguments,
    /// A runtime-native body taking the arguments in place
    /// (`FN_REST_NATIVE_ARGS`): nothing is bundled.
    NativeArgs,
}

/// Build a JS array from a slice of NaN-boxed f64 values and return it
/// NaN-boxed as a pointer. Used by the rest-bundling helper below.
#[inline(always)]
pub unsafe fn build_rest_array(values: &[f64], arguments_object: bool) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handles = scope.root_nanbox_f64_slice(values);
    build_rest_array_rooted(&value_handles, arguments_object)
}

/// [`build_rest_array`] for a caller that already holds its values in handles.
/// The array allocation and every push can collect, so the values have to be
/// read out of the handles anyway — a caller that has them keeps one rooting
/// pass instead of two.
pub unsafe fn build_rest_array_rooted(
    values: &[crate::gc::RuntimeHandle<'_>],
    arguments_object: bool,
) -> f64 {
    let arr = crate::array::js_array_alloc(values.len() as u32);
    let mut cur = arr;
    for handle in values.iter() {
        cur = crate::array::js_array_push_f64(cur, handle.get_nanbox_f64());
    }
    if arguments_object {
        crate::array::mark_array_as_arguments_object(cur as *const crate::array::ArrayHeader);
    }
    f64::from_bits(crate::value::JSValue::pointer(cur as *mut u8).bits())
}

/// Dispatch a closure with `args` to its body using a rest-bundled call.
/// `func_ptr` is already validated and known non-BOUND. `fixed_arity` is the
/// closure body's declared arity minus 1 (the +1 being the rest array).
///
/// Behavior matches the static-call-site bundling path in `lower_call.rs`:
/// the first `fixed_arity` args are forwarded as-is (padded with `undefined`
/// when the caller passed fewer than expected); everything from index
/// `fixed_arity` onwards is bundled into a fresh JS Array passed as the
/// last arg. The body is then invoked with exactly `fixed_arity + 1` doubles.
///
/// `fixed_arity` 0..=15 have exact arms; wider bodies go through the padded
/// ladder in `wide_call` (#10420).
#[inline(never)]
pub unsafe fn dispatch_rest_bundled(
    closure: *const ClosureHeader,
    func_ptr: *const u8,
    this: crate::closure::JsThis,
    args: &[f64],
    fixed_arity: u32,
    kind: RestDispatchKind,
) -> f64 {
    if kind == RestDispatchKind::NativeArgs {
        return call_native_args_body(closure, func_ptr, this, args);
    }
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let k = fixed_arity as usize;
    let provided = args.len();
    let arg_scope = crate::gc::RuntimeHandleScope::new();
    let arg_handles: Vec<_> = args
        .iter()
        .map(|value| arg_scope.root_nanbox_f64(*value))
        .collect();

    // Both arrays are built from the arguments this scope already roots, so
    // they read current values however many times the builder collects — and
    // the second array's allocation can move the first, which the body is
    // about to receive, so the first takes a handle too (#10532 review).
    let rest_handles: &[crate::gc::RuntimeHandle<'_>] =
        if kind == RestDispatchKind::SyntheticArguments {
            &arg_handles
        } else if provided > k {
            &arg_handles[k..]
        } else {
            &[]
        };
    let rest_handle = arg_scope.root_nanbox_f64(build_rest_array_rooted(
        rest_handles,
        kind == RestDispatchKind::SyntheticArguments,
    ));
    crate::gc::collection_point("closure.rest_bundle.between_arrays");
    let all_arguments_handle = if kind == RestDispatchKind::UserRestAndArguments {
        Some(arg_scope.root_nanbox_f64(build_rest_array_rooted(&arg_handles, true)))
    } else {
        None
    };
    let rest_double = rest_handle.get_nanbox_f64();
    let all_arguments_double = all_arguments_handle.map(|handle| handle.get_nanbox_f64());

    // Read fixed args, padding with undefined when caller under-supplied.
    macro_rules! a {
        ($i:expr) => {
            if $i < provided {
                arg_handles[$i].get_nanbox_f64()
            } else {
                undef
            }
        };
    }

    // Use a macro for higher rest arities so this stays in sync with the
    // documented 0..=15 support and the generated closure-call ceiling.
    // One arm per fixed arity: the body takes `(callee, fixed..., rest
    // [, arguments])`. Every arm goes through the body-call funnel.
    macro_rules! rest_arm {
        ($($i:tt),* $(,)?) => {{
            if let Some(arguments_double) = all_arguments_double {
                crate::closure::body_call::js_body_call_unwind!(
                    func_ptr, closure, this $(, a!($i))*, rest_double, arguments_double
                )
            } else {
                crate::closure::body_call::js_body_call_unwind!(
                    func_ptr, closure, this $(, a!($i))*, rest_double
                )
            }
        }};
    }

    match k {
        0 => rest_arm!(),
        1 => rest_arm!(0),
        2 => rest_arm!(0, 1),
        3 => rest_arm!(0, 1, 2),
        4 => rest_arm!(0, 1, 2, 3),
        5 => rest_arm!(0, 1, 2, 3, 4),
        6 => rest_arm!(0, 1, 2, 3, 4, 5),
        7 => rest_arm!(0, 1, 2, 3, 4, 5, 6),
        8 => rest_arm!(0, 1, 2, 3, 4, 5, 6, 7),
        9 => rest_arm!(0, 1, 2, 3, 4, 5, 6, 7, 8),
        10 => rest_arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9),
        11 => rest_arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10),
        12 => rest_arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11),
        13 => rest_arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12),
        14 => rest_arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13),
        15 => rest_arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14),
        _ => {
            // #10420: 16+ fixed params used to return `undefined` without
            // calling the body. Lay out the full ABI — fixed params, rest
            // array, then the `arguments` object when the body has one — and
            // call through the padded ladder.
            let mut slots: Vec<f64> = Vec::with_capacity(k + 2);
            slots.extend((0..k).map(|i| a!(i)));
            slots.push(rest_double);
            if let Some(arguments_double) = all_arguments_double {
                slots.push(arguments_double);
            }
            let width = slots.len();
            super::dispatch_wide_abi(closure, func_ptr, this, &slots, width)
        }
    }
}

/// Call an `FN_REST_NATIVE_ARGS` body with the caller's own argument slice.
/// Nothing allocates between the caller's read of these values and the
/// body's, so the body sees them exactly as the caller held them; a body that
/// allocates before it is done with them roots them itself.
#[inline]
unsafe fn call_native_args_body(
    closure: *const ClosureHeader,
    func_ptr: *const u8,
    this: crate::closure::JsThis,
    args: &[f64],
) -> f64 {
    let (ptr, len) = if args.is_empty() {
        (std::ptr::null(), 0)
    } else {
        (args.as_ptr(), args.len())
    };
    #[cfg(panic = "abort")]
    let f = std::mem::transmute::<*const u8, crate::codegen_abi::JsNativeArgsBody<ClosureHeader>>(
        func_ptr,
    );
    #[cfg(not(panic = "abort"))]
    let f = std::mem::transmute::<
        *const u8,
        unsafe extern "C-unwind" fn(
            *const ClosureHeader,
            crate::closure::JsThis,
            *const f64,
            usize,
        ) -> f64,
    >(func_ptr);
    f(closure, this, ptr, len)
}

/// Dispatch a closure call where the caller supplied fewer args than the
/// closure declared. Pad the missing slots with `undefined` and call the
/// body's actual declared signature so the body's `LocalGet(N)` reads
/// correctly initialised slots instead of stale registers.
///
/// `func_ptr` is already validated and known non-BOUND, non-rest.
/// `declared_arity` is the parameter count the body's `JsFunctionInfo`
/// records. Callers reach here only when `args.len() < declared_arity`.
///
/// Refs #420: drizzle's `pgTable` is `(name, columns, extraConfig) => …`
/// (3 params); user calls it as `pgTable("users", cols)` (2 args). Without
/// this padding, the body's `extraConfig` slot reads garbage and downstream
/// `if (extraConfig)` evaluated truthy on bit patterns that should have been
/// `undefined`. Symptom: `pgTable("users", {})` returned a malformed table
/// object, breaking every downstream property read.
#[inline(never)]
pub unsafe fn dispatch_with_arity(
    closure: *const ClosureHeader,
    func_ptr: *const u8,
    this: crate::closure::JsThis,
    args: &[f64],
    declared_arity: u32,
) -> f64 {
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let k = declared_arity as usize;
    let provided = args.len();
    macro_rules! a {
        ($i:expr) => {
            if $i < provided {
                args[$i]
            } else {
                undef
            }
        };
    }
    // One match arm per declared arity. Each arm calls the body through the
    // body-call funnel with exactly N (padded) JS arguments. Arities up to 32 have exact arms so high-arity closures dispatched
    // dynamically — e.g. qs's recursive `stringify`, which declares 18
    // params and self-calls with 18 args (#3527) — call their body
    // correctly instead of mis-calling and corrupting registers; wider
    // bodies take the padded ladder in `wide_call` (#10420). The
    // `arm!` macro builds the (padded) call args from the arg-index token list.
    macro_rules! arm {
        ($($i:tt),* $(,)?) => {
            crate::closure::body_call::js_body_call!(func_ptr, closure, this $(, a!($i))*)
        };
    }
    match k {
        0 => arm!(),
        1 => arm!(0),
        2 => arm!(0, 1),
        3 => arm!(0, 1, 2),
        4 => arm!(0, 1, 2, 3),
        5 => arm!(0, 1, 2, 3, 4),
        6 => arm!(0, 1, 2, 3, 4, 5),
        7 => arm!(0, 1, 2, 3, 4, 5, 6),
        8 => arm!(0, 1, 2, 3, 4, 5, 6, 7),
        9 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8),
        10 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9),
        11 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10),
        12 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11),
        13 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12),
        14 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13),
        15 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14),
        16 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15),
        17 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16),
        18 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17),
        19 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18),
        20 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19),
        21 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20),
        22 => arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21),
        23 => {
            arm!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22)
        }
        24 => arm!(
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23
        ),
        25 => arm!(
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24
        ),
        26 => arm!(
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25
        ),
        27 => arm!(
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26
        ),
        28 => arm!(
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27
        ),
        29 => arm!(
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28
        ),
        30 => arm!(
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29
        ),
        31 => arm!(
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29, 30
        ),
        32 => arm!(
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29, 30, 31
        ),
        // #10420: more than 32 declared params used to return `undefined`
        // without calling the body.
        _ => super::dispatch_wide_abi(closure, func_ptr, this, args, k),
    }
}

/// Sentinel func_ptr value indicating this closure is a "bound method" on a native module.
/// When js_closure_callN detects this, it extracts captures and dispatches via js_native_call_method.
/// Captures layout: [0] = namespace_obj (f64), [1] = method_name_ptr (i64), [2] = method_name_len (i64)
pub const BOUND_METHOD_FUNC_PTR: *const u8 = 0xBADD_DEAD_u64 as *const u8;

/// Sentinel func_ptr value indicating this closure is a `Function.prototype.bind`
/// result: a bound function with a fixed `this`, prepended partial args, and an
/// adjusted `.name` / `.length`. When `js_closure_callN` (or `js_native_call_value`)
/// detects this sentinel it dispatches via `dispatch_bound_function`, which
/// prepends the bound args and calls the target closure with the bound receiver
/// as its `this`.
///
/// Captures layout (`js_function_bind`, `dispatch/bound.rs`):
///   [0] = target closure value (f64, NaN-boxed)
///   [1] = bound `this` value (f64)
///   [2] = bound-args JS Array pointer (i64; 0 when no partial args)
///   [3] = target name snapshot (for the `bound <name>` name)
///   [4] = `length` snapshot
pub const BOUND_FUNCTION_FUNC_PTR: *const u8 = 0xBADD_B12D_u64 as *const u8;

/// The info every bound-method value points to (its "code" is the
/// [`BOUND_METHOD_FUNC_PTR`] sentinel the dispatchers route by).
// SAFETY: the dispatchers route this sentinel by value; it is never called.
pub static BOUND_METHOD_INFO: JsFunctionInfo =
    unsafe { JsFunctionInfo::from_code(BOUND_METHOD_FUNC_PTR, 0) };

/// The info every `Function.prototype.bind` result points to (its "code" is
/// the [`BOUND_FUNCTION_FUNC_PTR`] sentinel).
// SAFETY: the dispatchers route this sentinel by value; it is never called.
pub static BOUND_FUNCTION_INFO: JsFunctionInfo =
    unsafe { JsFunctionInfo::from_code(BOUND_FUNCTION_FUNC_PTR, 0) };

/// Flag stored in the high bit of capture_count: the closure's LAST capture
/// slot (index `real_capture_count - 1`) holds its `this` — an arrow's lexical
/// receiver, or the object an object-literal method was created on (patched
/// by `lower_object_literal`). `clone_closure_rebind_this` rewrites that slot
/// for `call`/`apply`/borrowed-method dispatch unless `NO_THIS_REBIND_FLAG` is
/// also set.
pub const CAPTURES_THIS_FLAG: u32 = 0x8000_0000;

/// Flag stored in bit 30 of `capture_count` marking a closure whose captured
/// `this` slot is LEXICAL and must never be re-bound by `Function.prototype.call`
/// / method dispatch (`clone_closure_rebind_this`). Set per-closure (on the
/// header itself) by `js_generator_attach_prototype` for the generator
/// state-machine `next`/`return`/`throw` step closures: their `this` is the
/// generator BODY's receiver, fixed at generator-creation time, and the `yield*`
/// delegation desugar (`next.call(iter, v)`) would otherwise clobber it with the
/// iterator object. Capture counts never approach 2^30, so bit 30 is free.
pub const NO_THIS_REBIND_FLAG: u32 = 0x4000_0000;

/// Extract the real capture count (masking out the flag bits stored in the
/// two high bits: `CAPTURES_THIS_FLAG` and `NO_THIS_REBIND_FLAG`).
#[inline(always)]
pub fn real_capture_count(capture_count: u32) -> u32 {
    capture_count & !(CAPTURES_THIS_FLAG | NO_THIS_REBIND_FLAG)
}
