//! Per-arity closure-call FFI entry points (0..=16):
//! `js_closure_call{N}(closure, this, a0..)`, where `this` is the call's
//! receiver (`JsThis::UNDEFINED` for a plain call), and the shared routing
//! helpers.
//!
//! A call whose body is plain for its argument count
//! (`JsFunctionInfo::plain_params`) jumps straight to the body. Every other
//! call funnels into one per-arity dispatcher that routes bound
//! methods/functions, rest bundling, arity padding and the direct body call,
//! threading the receiver to the body as its `this` parameter
//! (`perry_abi::JS_BODY_THIS_PARAM`).
//!
//! The hot-loop counterpart -- resolve a closure ONCE and call it directly for
//! the rest of the loop -- lives in the sibling `direct` module (#8180).

use super::*;
use crate::closure::JsThis;

macro_rules! closure_call_dispatch {
    ($dispatch:ident, $n:literal; $($a:ident),*) => {
        /// Route one closure call with a known receiver: a plain body takes
        /// the call as it is, anything else goes down the ladder.
        #[inline(always)]
        pub(crate) fn $dispatch(closure: *const ClosureHeader, this: JsThis $(, $a: f64)*) -> f64 {
            let Some(info) = crate::closure::closure_info(closure) else {
                return dispatch_proxy_callee_or_throw(closure, this, &[$($a),*]);
            };
            let func_ptr = info.code;
            if plain_admits(info, $n) {
                return unsafe {
                    crate::closure::body_call::js_body_call!(func_ptr, closure, this $(, $a)*)
                };
            }
            match resolve_strategy(info).kind() {
                DispatchKind::BoundMethod => unsafe {
                    dispatch_bound_method(closure, this, &[$($a),*])
                },
                DispatchKind::BoundFunction => unsafe {
                    dispatch_bound_function(closure, &[$($a),*])
                },
                DispatchKind::Rest(fixed_arity, synth) => unsafe {
                    dispatch_rest_bundled(closure, func_ptr, this, &[$($a),*], fixed_arity, synth)
                },
                DispatchKind::Arity(declared) if arity_needs_dispatch(declared, $n) => unsafe {
                    dispatch_with_arity(closure, func_ptr, this, &[$($a),*], declared)
                },
                _ => unsafe {
                    crate::closure::body_call::js_body_call!(func_ptr, closure, this $(, $a)*)
                },
            }
        }
    };
}

/// The body a call of `closure` passing `argc` arguments jumps to with its
/// registers untouched, if there is one: `closure` is a live function object
/// (the same proof [`get_valid_info`] gives every dispatch) whose body is
/// plain for `argc` ([`plain_admits`]).
#[inline(always)]
fn plain_body(closure: *const ClosureHeader, argc: u32) -> Option<*const u8> {
    let info = crate::closure::closure_info(closure)?;
    plain_admits(info, argc).then_some(info.code)
}

/// Define the per-arity entry `js_closure_call{N}`.
///
/// Where the target has guaranteed tail calls, a plain body is reached with
/// `become`: one validation, one compare of the body's `plain_params`, then a
/// jump with the caller's argument registers untouched, so the entry leaves
/// no frame behind and the body returns straight to the compiled caller.
/// Anything else jumps to the out-of-line ladder (`$slow`), which routes it
/// exactly as [`closure_call_dispatch!`] does. wasm keeps the call: its
/// `return_call` needs a target feature perry does not enable.
///
/// The entry is `extern "C"`; `unwind` makes it `extern "C-unwind"` in
/// unwinding (test) builds (`js_closure_call0`, #8479) and plain C under
/// `panic = "abort"`.
macro_rules! closure_call_entry {
    (unwind $(#[$doc:meta])* $entry:ident, $slow:ident, $dispatch:ident, $n:literal; $($a:ident),*) => {
        #[cfg(panic = "abort")]
        closure_call_entry!(@define "C", js_body_fn, $(#[$doc])* $entry, $slow, $dispatch, $n; $($a),*);
        #[cfg(not(panic = "abort"))]
        closure_call_entry!(@define "C-unwind", js_body_fn_unwind, $(#[$doc])* $entry, $slow, $dispatch, $n; $($a),*);
    };
    ($(#[$doc:meta])* $entry:ident, $slow:ident, $dispatch:ident, $n:literal; $($a:ident),*) => {
        closure_call_entry!(@define "C", js_body_fn, $(#[$doc])* $entry, $slow, $dispatch, $n; $($a),*);
    };
    (@define $abi:literal, $body_fn:ident, $(#[$doc:meta])* $entry:ident, $slow:ident, $dispatch:ident, $n:literal; $($a:ident),*) => {
        $(#[$doc])*
        #[no_mangle]
        pub extern $abi fn $entry(closure: *const ClosureHeader, this: JsThis $(, $a: f64)*) -> f64 {
            #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
            {
                if let Some(code) = plain_body(closure, $n) {
                    // SAFETY: `code` is the body of the live function object
                    // `closure`, plain for this argument count.
                    let body = unsafe { crate::closure::body_call::$body_fn!(code; $($a),*) };
                    unsafe { become body(closure, this $(, $a)*) }
                }
                become $slow(closure, this $(, $a)*)
            }
            #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
            {
                $dispatch(closure, this $(, $a)*)
            }
        }

        /// The ladder behind the entry's plain test, out of line so the
        /// entry stays a test and a jump.
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        #[inline(never)]
        extern $abi fn $slow(closure: *const ClosureHeader, this: JsThis $(, $a: f64)*) -> f64 {
            $dispatch(closure, this $(, $a)*)
        }
    };
}

// #8479: the entries are NOT `C-unwind` (except `js_closure_call0` in
// unwinding test builds). The runtime is built `panic=abort` and JS throws
// travel as a raw Itanium `_Unwind_Exception` that must step THROUGH these
// frames untouched (see `crate::eh` and the panic=abort rationale in the
// workspace Cargo.toml). Marking a frame `extern "C-unwind"` in a panic=abort
// crate does not enable that — it makes rustc wrap the call in an
// abort-on-unwind landing pad, which is exactly the RFC-2945 guard a JS throw
// trips. A plain body's call leaves no entry frame at all.
closure_call_dispatch!(dispatch_call0, 0;);
closure_call_entry!(
    unwind
    /// Call a closure with receiver `this` and no arguments.
    js_closure_call0, slow_call0, dispatch_call0, 0;
);

closure_call_dispatch!(dispatch_call1, 1; a0);
closure_call_entry!(
    /// Call a closure with receiver `this` and 1 argument.
    js_closure_call1, slow_call1, dispatch_call1, 1; a0
);

closure_call_dispatch!(dispatch_call2, 2; a0, a1);
closure_call_entry!(
    /// Call a closure with receiver `this` and 2 arguments.
    js_closure_call2, slow_call2, dispatch_call2, 2; a0, a1
);

closure_call_dispatch!(dispatch_call3, 3; a0, a1, a2);
closure_call_entry!(
    /// Call a closure with receiver `this` and 3 arguments.
    js_closure_call3, slow_call3, dispatch_call3, 3; a0, a1, a2
);

closure_call_dispatch!(dispatch_call4, 4; a0, a1, a2, a3);
closure_call_entry!(
    /// Call a closure with receiver `this` and 4 arguments.
    js_closure_call4, slow_call4, dispatch_call4, 4; a0, a1, a2, a3
);

closure_call_dispatch!(dispatch_call5, 5; a0, a1, a2, a3, a4);
closure_call_entry!(
    /// Call a closure with receiver `this` and 5 arguments.
    js_closure_call5, slow_call5, dispatch_call5, 5; a0, a1, a2, a3, a4
);

closure_call_dispatch!(dispatch_call6, 6; a0, a1, a2, a3, a4, a5);
closure_call_entry!(
    /// Call a closure with receiver `this` and 6 arguments.
    js_closure_call6, slow_call6, dispatch_call6, 6; a0, a1, a2, a3, a4, a5
);

closure_call_dispatch!(dispatch_call7, 7; a0, a1, a2, a3, a4, a5, a6);
closure_call_entry!(
    /// Call a closure with receiver `this` and 7 arguments.
    js_closure_call7, slow_call7, dispatch_call7, 7; a0, a1, a2, a3, a4, a5, a6
);

closure_call_dispatch!(dispatch_call8, 8; a0, a1, a2, a3, a4, a5, a6, a7);
closure_call_entry!(
    /// Call a closure with receiver `this` and 8 arguments.
    js_closure_call8, slow_call8, dispatch_call8, 8; a0, a1, a2, a3, a4, a5, a6, a7
);

closure_call_dispatch!(dispatch_call9, 9; a0, a1, a2, a3, a4, a5, a6, a7, a8);
closure_call_entry!(
    /// Call a closure with receiver `this` and 9 arguments.
    js_closure_call9, slow_call9, dispatch_call9, 9; a0, a1, a2, a3, a4, a5, a6, a7, a8
);

closure_call_dispatch!(dispatch_call10, 10; a0, a1, a2, a3, a4, a5, a6, a7, a8, a9);
closure_call_entry!(
    /// Call a closure with receiver `this` and 10 arguments.
    js_closure_call10, slow_call10, dispatch_call10, 10; a0, a1, a2, a3, a4, a5, a6, a7, a8, a9
);

closure_call_dispatch!(dispatch_call11, 11; a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10);
closure_call_entry!(
    /// Call a closure with receiver `this` and 11 arguments.
    js_closure_call11, slow_call11, dispatch_call11, 11; a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10
);

closure_call_dispatch!(dispatch_call12, 12; a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11);
closure_call_entry!(
    /// Call a closure with receiver `this` and 12 arguments.
    js_closure_call12, slow_call12, dispatch_call12, 12; a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11
);

closure_call_dispatch!(dispatch_call13, 13; a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12);
closure_call_entry!(
    /// Call a closure with receiver `this` and 13 arguments.
    js_closure_call13, slow_call13, dispatch_call13, 13; a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12
);

closure_call_dispatch!(dispatch_call14, 14; a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13);
closure_call_entry!(
    /// Call a closure with receiver `this` and 14 arguments.
    js_closure_call14, slow_call14, dispatch_call14, 14; a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13
);

closure_call_dispatch!(dispatch_call15, 15; a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14);
closure_call_entry!(
    /// Call a closure with receiver `this` and 15 arguments.
    js_closure_call15, slow_call15, dispatch_call15, 15; a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14
);

closure_call_dispatch!(dispatch_call16, 16; a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15);
closure_call_entry!(
    /// Call a closure with receiver `this` and 16 arguments.
    js_closure_call16, slow_call16, dispatch_call16, 16; a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15
);

/// Dispatch `args` (any count) to `closure` with receiver `this`: the
/// per-arity dispatchers up to 16 arguments, the padded wide ladder past
/// them. The one Rust-side way to call a closure pointer.
pub(crate) fn dispatch_call_slice(
    closure: *const ClosureHeader,
    this: JsThis,
    args: &[f64],
) -> f64 {
    let a = |i: usize| args[i];
    match args.len() {
        0 => dispatch_call0(closure, this),
        1 => dispatch_call1(closure, this, a(0)),
        2 => dispatch_call2(closure, this, a(0), a(1)),
        3 => dispatch_call3(closure, this, a(0), a(1), a(2)),
        4 => dispatch_call4(closure, this, a(0), a(1), a(2), a(3)),
        5 => dispatch_call5(closure, this, a(0), a(1), a(2), a(3), a(4)),
        6 => dispatch_call6(closure, this, a(0), a(1), a(2), a(3), a(4), a(5)),
        7 => dispatch_call7(closure, this, a(0), a(1), a(2), a(3), a(4), a(5), a(6)),
        8 => dispatch_call8(
            closure,
            this,
            a(0),
            a(1),
            a(2),
            a(3),
            a(4),
            a(5),
            a(6),
            a(7),
        ),
        9 => dispatch_call9(
            closure,
            this,
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
        10 => dispatch_call10(
            closure,
            this,
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
        11 => dispatch_call11(
            closure,
            this,
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
        12 => dispatch_call12(
            closure,
            this,
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
        13 => dispatch_call13(
            closure,
            this,
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
        14 => dispatch_call14(
            closure,
            this,
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
        15 => dispatch_call15(
            closure,
            this,
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
        16 => dispatch_call16(
            closure,
            this,
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
        _ => dispatch_call_wide(closure, this, args),
    }
}

/// More than 16 arguments: route as the per-arity entries do, then call the
/// body through the padded ladder (`wide_call`).
fn dispatch_call_wide(closure: *const ClosureHeader, this: JsThis, args: &[f64]) -> f64 {
    let Some(info) = crate::closure::closure_info(closure) else {
        return dispatch_proxy_callee_or_throw(closure, this, args);
    };
    let func_ptr = info.code;
    if plain_admits(info, args.len() as u32) {
        return unsafe {
            super::super::dispatch_wide_abi(closure, func_ptr, this, args, args.len())
        };
    }
    match resolve_strategy(info).kind() {
        DispatchKind::BoundMethod => unsafe { dispatch_bound_method(closure, this, args) },
        DispatchKind::BoundFunction => unsafe { dispatch_bound_function(closure, args) },
        DispatchKind::Rest(fixed_arity, synth) => unsafe {
            dispatch_rest_bundled(closure, func_ptr, this, args, fixed_arity, synth)
        },
        DispatchKind::Arity(declared) if arity_needs_dispatch(declared, args.len() as u32) => unsafe {
            dispatch_with_arity(closure, func_ptr, this, args, declared)
        },
        _ => unsafe { super::super::dispatch_wide_abi(closure, func_ptr, this, args, args.len()) },
    }
}

#[cfg(test)]
mod plain_call_tests {
    use super::*;

    extern "C" fn observe_dynamic_this(
        _: *const ClosureHeader,
        this: crate::closure::JsThis,
        _: f64,
    ) -> f64 {
        this.as_f64()
    }

    #[test]
    fn a_plain_call_passes_undefined_this() {
        let closure = crate::closure::js_closure_alloc(crate::fn_info!(observe_dynamic_this, 1), 0);

        // A receiver passed to an enclosing call must not leak into a plain
        // call: the body sees exactly the `undefined` this entry passes.
        let explicit = js_closure_call1(closure, JsThis::from_f64(42.0), 0.0);
        assert_eq!(explicit, 42.0);
        let regular_result = js_closure_call1(closure, crate::closure::plain_call_receiver(), 0.0);
        assert_eq!(regular_result.to_bits(), crate::value::TAG_UNDEFINED);
    }

    extern "C" fn second_arg(_: *const ClosureHeader, _: JsThis, _: f64, b: f64) -> f64 {
        b
    }

    #[test]
    fn legacy_zero_padding_keeps_under_applied_calls_on_the_dispatcher() {
        let info = crate::fn_info!(second_arg, 2);
        // Legacy providers published zero here. Reproduce those exact bytes,
        // and observe padding through a real under-applied closure call.
        let word = unsafe {
            info.cast::<u8>()
                .add(crate::codegen_abi::JS_FUNCTION_INFO_PLAIN_PARAMS_OFFSET)
                .cast::<u16>()
                .read()
        };
        assert_eq!(word, 0, "the fixture has the legacy padding word");
        let closure = crate::closure::js_closure_alloc(info, 0);
        assert_eq!(
            js_closure_call1(closure, crate::closure::plain_call_receiver(), 1.0).to_bits(),
            crate::value::TAG_UNDEFINED
        );
    }

    /// A plain body takes an exact or over-applied call straight from the
    /// entry; a short call goes down the ladder and reads `undefined` for the
    /// missing parameter.
    #[test]
    fn a_plain_body_takes_every_call_passing_its_parameters() {
        let info = crate::fn_info!(second_arg, 2; plain());
        let closure = crate::closure::js_closure_alloc(info, 0);
        let info = unsafe { &*info };
        assert_eq!(info.plain_params(), 2);
        assert!(!plain_admits(info, 1));
        assert!(plain_admits(info, 2) && plain_admits(info, 3));
        let undefined = crate::closure::plain_call_receiver();
        assert_eq!(js_closure_call2(closure, undefined, 1.0, 2.0), 2.0);
        assert_eq!(js_closure_call3(closure, undefined, 1.0, 5.0, 9.0), 5.0);
        assert_eq!(
            js_closure_call16(
                closure, undefined, 1.0, 6.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                0.0, 0.0, 0.0, 0.0,
            ),
            6.0
        );
        assert_eq!(
            js_closure_call1(closure, undefined, 1.0).to_bits(),
            crate::value::TAG_UNDEFINED
        );
        assert_eq!(dispatch_call_slice(closure, undefined, &[3.0; 20]), 3.0);
    }

    /// Only a compiled body is plain: a bound value, a body with a rest kind
    /// and every info the runtime builds take the dispatcher.
    #[test]
    fn bound_values_rest_bodies_and_runtime_infos_are_never_plain() {
        use crate::codegen_abi::NOT_PLAIN;
        assert_eq!(
            crate::closure::BOUND_FUNCTION_INFO.plain_params(),
            NOT_PLAIN
        );
        assert_eq!(crate::closure::BOUND_METHOD_INFO.plain_params(), NOT_PLAIN);
        let native = unsafe { &*crate::fn_info!(second_arg, 2) };
        assert_eq!(native.plain_params(), NOT_PLAIN);
        let rest = unsafe { &*crate::fn_info!(second_arg, 2; with_rest(1), plain()) };
        assert_eq!(rest.plain_params(), NOT_PLAIN);
        let rest_after_plain = unsafe { &*crate::fn_info!(second_arg, 2; plain(), with_rest(1)) };
        let args_after_plain = unsafe {
            &*crate::fn_info!(second_arg, 2; plain(), with_rest_kind(0, crate::codegen_abi::FN_REST_SYNTHETIC_ARGUMENTS))
        };
        for argc in [0, 1, 2, 16, u32::from(u16::MAX) - 1] {
            assert!(!plain_admits(native, argc) && !plain_admits(rest, argc));
            assert!(!plain_admits(rest_after_plain, argc) && !plain_admits(args_after_plain, argc));
        }
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
