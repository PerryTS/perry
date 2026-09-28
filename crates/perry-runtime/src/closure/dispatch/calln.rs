//! Per-arity closure-call FFI entry points (0..=16):
//! `js_closure_call{N}(closure, this, a0..)`, where `this` is the call's
//! receiver (`JsThis::UNDEFINED` for a plain call), and the shared routing
//! helpers.
//!
//! Each funnels into one per-arity dispatcher that routes bound
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
        /// Route one closure call with a known receiver.
        #[inline(always)]
        pub(crate) fn $dispatch(closure: *const ClosureHeader, this: JsThis $(, $a: f64)*) -> f64 {
            let Some(info) = crate::closure::closure_info(closure) else {
                return dispatch_proxy_callee_or_throw(closure, this, &[$($a),*]);
            };
            let func_ptr = info.code;
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

// `js_closure_call0` is `extern "C-unwind"` in unwinding (test) builds so an
// interpreted throw can reach a generated caller's catch landing pad;
// production keeps plain C (#8479).
closure_call_dispatch!(dispatch_call0, 0;);

/// Call a closure with receiver `this` and no arguments.
#[cfg(panic = "abort")]
#[no_mangle]
pub extern "C" fn js_closure_call0(closure: *const ClosureHeader, this: JsThis) -> f64 {
    dispatch_call0(closure, this)
}

/// See the production definition above.
#[cfg(not(panic = "abort"))]
#[no_mangle]
pub extern "C-unwind" fn js_closure_call0(closure: *const ClosureHeader, this: JsThis) -> f64 {
    dispatch_call0(closure, this)
}

// #8479: NOT `C-unwind` (below). The runtime is built `panic=abort` and JS
// throws travel as a raw Itanium `_Unwind_Exception` that must step THROUGH
// these frames untouched (see `crate::eh` and the panic=abort rationale in the
// workspace Cargo.toml). Marking a frame `extern "C-unwind"` in a panic=abort
// crate does not enable that — it makes rustc wrap the call in an
// abort-on-unwind landing pad, which is exactly the RFC-2945 guard a JS throw
// trips.
closure_call_dispatch!(dispatch_call1, 1; a0);
/// Call a closure with receiver `this` and 1 argument.
#[no_mangle]
pub extern "C" fn js_closure_call1(closure: *const ClosureHeader, this: JsThis, a0: f64) -> f64 {
    dispatch_call1(closure, this, a0)
}

closure_call_dispatch!(dispatch_call2, 2; a0, a1);
/// Call a closure with receiver `this` and 2 arguments.
#[no_mangle]
pub extern "C" fn js_closure_call2(
    closure: *const ClosureHeader,
    this: JsThis,
    a0: f64,
    a1: f64,
) -> f64 {
    dispatch_call2(closure, this, a0, a1)
}

closure_call_dispatch!(dispatch_call3, 3; a0, a1, a2);
/// Call a closure with receiver `this` and 3 arguments.
#[no_mangle]
pub extern "C" fn js_closure_call3(
    closure: *const ClosureHeader,
    this: JsThis,
    a0: f64,
    a1: f64,
    a2: f64,
) -> f64 {
    dispatch_call3(closure, this, a0, a1, a2)
}

closure_call_dispatch!(dispatch_call4, 4; a0, a1, a2, a3);
/// Call a closure with receiver `this` and 4 arguments.
#[no_mangle]
pub extern "C" fn js_closure_call4(
    closure: *const ClosureHeader,
    this: JsThis,
    a0: f64,
    a1: f64,
    a2: f64,
    a3: f64,
) -> f64 {
    dispatch_call4(closure, this, a0, a1, a2, a3)
}

closure_call_dispatch!(dispatch_call5, 5; a0, a1, a2, a3, a4);
/// Call a closure with receiver `this` and 5 arguments.
#[no_mangle]
pub extern "C" fn js_closure_call5(
    closure: *const ClosureHeader,
    this: JsThis,
    a0: f64,
    a1: f64,
    a2: f64,
    a3: f64,
    a4: f64,
) -> f64 {
    dispatch_call5(closure, this, a0, a1, a2, a3, a4)
}

closure_call_dispatch!(dispatch_call6, 6; a0, a1, a2, a3, a4, a5);
/// Call a closure with receiver `this` and 6 arguments.
#[no_mangle]
pub extern "C" fn js_closure_call6(
    closure: *const ClosureHeader,
    this: JsThis,
    a0: f64,
    a1: f64,
    a2: f64,
    a3: f64,
    a4: f64,
    a5: f64,
) -> f64 {
    dispatch_call6(closure, this, a0, a1, a2, a3, a4, a5)
}

closure_call_dispatch!(dispatch_call7, 7; a0, a1, a2, a3, a4, a5, a6);
/// Call a closure with receiver `this` and 7 arguments.
#[no_mangle]
pub extern "C" fn js_closure_call7(
    closure: *const ClosureHeader,
    this: JsThis,
    a0: f64,
    a1: f64,
    a2: f64,
    a3: f64,
    a4: f64,
    a5: f64,
    a6: f64,
) -> f64 {
    dispatch_call7(closure, this, a0, a1, a2, a3, a4, a5, a6)
}

closure_call_dispatch!(dispatch_call8, 8; a0, a1, a2, a3, a4, a5, a6, a7);
/// Call a closure with receiver `this` and 8 arguments.
#[no_mangle]
pub extern "C" fn js_closure_call8(
    closure: *const ClosureHeader,
    this: JsThis,
    a0: f64,
    a1: f64,
    a2: f64,
    a3: f64,
    a4: f64,
    a5: f64,
    a6: f64,
    a7: f64,
) -> f64 {
    dispatch_call8(closure, this, a0, a1, a2, a3, a4, a5, a6, a7)
}

closure_call_dispatch!(dispatch_call9, 9; a0, a1, a2, a3, a4, a5, a6, a7, a8);
/// Call a closure with receiver `this` and 9 arguments.
#[no_mangle]
pub extern "C" fn js_closure_call9(
    closure: *const ClosureHeader,
    this: JsThis,
    a0: f64,
    a1: f64,
    a2: f64,
    a3: f64,
    a4: f64,
    a5: f64,
    a6: f64,
    a7: f64,
    a8: f64,
) -> f64 {
    dispatch_call9(closure, this, a0, a1, a2, a3, a4, a5, a6, a7, a8)
}

closure_call_dispatch!(dispatch_call10, 10; a0, a1, a2, a3, a4, a5, a6, a7, a8, a9);
/// Call a closure with receiver `this` and 10 arguments.
#[no_mangle]
pub extern "C" fn js_closure_call10(
    closure: *const ClosureHeader,
    this: JsThis,
    a0: f64,
    a1: f64,
    a2: f64,
    a3: f64,
    a4: f64,
    a5: f64,
    a6: f64,
    a7: f64,
    a8: f64,
    a9: f64,
) -> f64 {
    dispatch_call10(closure, this, a0, a1, a2, a3, a4, a5, a6, a7, a8, a9)
}

closure_call_dispatch!(dispatch_call11, 11; a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10);
/// Call a closure with receiver `this` and 11 arguments.
#[no_mangle]
pub extern "C" fn js_closure_call11(
    closure: *const ClosureHeader,
    this: JsThis,
    a0: f64,
    a1: f64,
    a2: f64,
    a3: f64,
    a4: f64,
    a5: f64,
    a6: f64,
    a7: f64,
    a8: f64,
    a9: f64,
    a10: f64,
) -> f64 {
    dispatch_call11(closure, this, a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10)
}

closure_call_dispatch!(dispatch_call12, 12; a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11);
/// Call a closure with receiver `this` and 12 arguments.
#[no_mangle]
pub extern "C" fn js_closure_call12(
    closure: *const ClosureHeader,
    this: JsThis,
    a0: f64,
    a1: f64,
    a2: f64,
    a3: f64,
    a4: f64,
    a5: f64,
    a6: f64,
    a7: f64,
    a8: f64,
    a9: f64,
    a10: f64,
    a11: f64,
) -> f64 {
    dispatch_call12(
        closure, this, a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11,
    )
}

closure_call_dispatch!(dispatch_call13, 13; a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12);
/// Call a closure with receiver `this` and 13 arguments.
#[no_mangle]
pub extern "C" fn js_closure_call13(
    closure: *const ClosureHeader,
    this: JsThis,
    a0: f64,
    a1: f64,
    a2: f64,
    a3: f64,
    a4: f64,
    a5: f64,
    a6: f64,
    a7: f64,
    a8: f64,
    a9: f64,
    a10: f64,
    a11: f64,
    a12: f64,
) -> f64 {
    dispatch_call13(
        closure, this, a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12,
    )
}

closure_call_dispatch!(dispatch_call14, 14; a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13);
/// Call a closure with receiver `this` and 14 arguments.
#[no_mangle]
pub extern "C" fn js_closure_call14(
    closure: *const ClosureHeader,
    this: JsThis,
    a0: f64,
    a1: f64,
    a2: f64,
    a3: f64,
    a4: f64,
    a5: f64,
    a6: f64,
    a7: f64,
    a8: f64,
    a9: f64,
    a10: f64,
    a11: f64,
    a12: f64,
    a13: f64,
) -> f64 {
    dispatch_call14(
        closure, this, a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13,
    )
}

closure_call_dispatch!(dispatch_call15, 15; a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14);
/// Call a closure with receiver `this` and 15 arguments.
#[no_mangle]
pub extern "C" fn js_closure_call15(
    closure: *const ClosureHeader,
    this: JsThis,
    a0: f64,
    a1: f64,
    a2: f64,
    a3: f64,
    a4: f64,
    a5: f64,
    a6: f64,
    a7: f64,
    a8: f64,
    a9: f64,
    a10: f64,
    a11: f64,
    a12: f64,
    a13: f64,
    a14: f64,
) -> f64 {
    dispatch_call15(
        closure, this, a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14,
    )
}

closure_call_dispatch!(dispatch_call16, 16; a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15);
/// Call a closure with receiver `this` and 16 arguments.
#[no_mangle]
pub extern "C" fn js_closure_call16(
    closure: *const ClosureHeader,
    this: JsThis,
    a0: f64,
    a1: f64,
    a2: f64,
    a3: f64,
    a4: f64,
    a5: f64,
    a6: f64,
    a7: f64,
    a8: f64,
    a9: f64,
    a10: f64,
    a11: f64,
    a12: f64,
    a13: f64,
    a14: f64,
    a15: f64,
) -> f64 {
    dispatch_call16(
        closure, this, a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15,
    )
}

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
