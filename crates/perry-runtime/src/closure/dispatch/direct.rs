//! Hoisted per-arity closure dispatch for callback loops (#8180).
//!
//! `js_closure_callN` re-derives, on EVERY call, three things that cannot
//! change while one closure is being called in a loop:
//!
//!   1. `get_valid_func_ptr` — two address-band checks plus a volatile
//!      `CLOSURE_MAGIC` probe through `*(closure + 12)` and a volatile load of
//!      `closure->func_ptr` (`dispatch/validate.rs`);
//!   2. `resolve_strategy` — a `perry_thread_local!` polymorphic cache, which
//!      on Darwin is a `tlv_get_addr` CALL plus a load and a compare even when
//!      it hits (`closure/registry.rs`);
//!   3. the `match` over `DispatchStrategy` before the indirect jump.
//!
//! Over `array.forEach(cb)` on a million elements that is a million repeats of
//! one answer. `array/sort.rs`'s `ComparatorCall` already hoists it for the
//! 2-argument comparator — introduced to "skip ~50M HashMap lookups over a
//! 1.25M-element sort" — but that was the only consumer. This module
//! generalises the same shape to the arities the array-callback helpers use
//! and gives it one place to live.
//!
//! # Why hoisting is sound
//!
//! Each input is invariant for a FIXED closure:
//!
//! * `closure->func_ptr` is written once by `js_closure_alloc` and never
//!   mutated. Collection can move the ClosureHeader, but its function field
//!   still holds the same static code address. Callers must re-read the
//!   closure argument from a current root before calling that code, as
//!   `ComparatorCall::less_equal_at` documents.
//! * `lookup_closure_rest` / `lookup_closure_arity` are keyed by that
//!   func_ptr, and both registries are insert-only per key — the registration
//!   happens at closure creation, before the closure can be passed anywhere.
//! * `BOUND_METHOD_FUNC_PTR` / `BOUND_FUNCTION_FUNC_PTR` are process
//!   constants.
//!
//! So the only way a loop could observe a different dispatch strategy
//! mid-iteration is by calling a DIFFERENT closure, and an array method calls
//! exactly one. A site that can retarget its callee (a dynamic property read
//! per element, say) must not use these types.
//!
//! # Interaction with rooting
//!
//! `call` takes the closure pointer as a parameter rather than caching it, so
//! a caller that roots its callback in a `RuntimeHandleScope` (#8179, gh
//! #6206) passes the CURRENT address after every user-code window. The
//! resolved target stays valid regardless: relocation does not move code.
//!
//! # Fallback
//!
//! `resolve` answers `None` for a bound method/function, a rest parameter, a
//! declared arity above the call arity, and an invalid closure pointer. Those
//! calls go through `js_closure_callN` unchanged, which keeps the
//! proxy-callee/throw path, the rest bundling and the undefined-padding in
//! exactly one place.

use super::*;

/// Resolve a closure ONCE for a fixed call arity: `Some(func_ptr)` when every
/// call at `arity` can jump straight to the compiled body (no bound-method
/// routing, no rest bundling, no undefined-padding). See the module docs for
/// why the answer is invariant.
#[inline]
pub(crate) fn resolve_direct_func_ptr(
    closure: *const ClosureHeader,
    arity: u32,
) -> Option<*const u8> {
    resolve_direct_info(closure, arity).map(|info| info.code)
}

/// [`resolve_direct_func_ptr`]'s info: `Some` when every call at `arity` can
/// jump straight to the body's code.
#[inline]
pub(crate) fn resolve_direct_info(
    closure: *const ClosureHeader,
    arity: u32,
) -> Option<&'static crate::closure::JsFunctionInfo> {
    let info = crate::closure::closure_info(closure)?;
    match resolve_strategy(info).kind() {
        DispatchKind::Arity(declared) if !super::arity_needs_dispatch(declared, arity) => {
            Some(info)
        }
        _ => None,
    }
}

/// Resolve one callback for PLAIN calls (`f(x)`) at a method boundary: the
/// body the caller may call directly, passing `undefined` as its receiver —
/// exactly what `js_closure_callN` passes. An arrow ignores the receiver; an
/// ordinary function binds it (a sloppy body coerces it in its own
/// prologue). Bound/rest/padded calls retain their full dispatcher semantics.
#[no_mangle]
pub extern "C" fn js_closure_resolve_plain_direct_call(
    closure: *const ClosureHeader,
    arity: u32,
) -> *const u8 {
    let Some(info) = resolve_direct_info(closure, arity) else {
        return std::ptr::null();
    };
    let func_ptr = info.code;
    let Some(trusted) = crate::closure::info_trusted_direct(info) else {
        return func_ptr;
    };
    let actual_capture_count = unsafe { real_capture_count((*closure).capture_count) };
    if actual_capture_count != trusted.capture_count {
        return func_ptr;
    }
    let mut mask = trusted.boxed_capture_mask;
    while mask != 0 {
        let index = mask.trailing_zeros();
        let box_ptr = crate::closure::js_closure_get_capture_bits(closure, index);
        if !crate::r#box::scope::is_capture_cell_ptr(box_ptr) {
            return func_ptr;
        }
        mask &= mask - 1;
    }
    trusted.func_ptr
}

/// Resolve only a compiler-private versioned-loop callback clone. A runtime
/// closure must match the registered capture layout exactly; any other arrow,
/// ordinary function, rest/padded call, or forged capture falls back.
#[no_mangle]
pub extern "C" fn js_closure_resolve_versioned_loop_direct_call(
    closure: *const ClosureHeader,
    arity: u32,
) -> *const u8 {
    let Some(info) = resolve_direct_info(closure, arity) else {
        return std::ptr::null();
    };
    if info.flags & crate::closure::FN_ARROW == 0 {
        return std::ptr::null();
    }
    let Some(target) = crate::closure::info_versioned_loop_direct(info) else {
        return std::ptr::null();
    };
    let actual_capture_count = unsafe { real_capture_count((*closure).capture_count) };
    if actual_capture_count != target.capture_count {
        return std::ptr::null();
    }
    let mut mask = target.boxed_capture_mask;
    while mask != 0 {
        let index = mask.trailing_zeros();
        let box_ptr = crate::closure::js_closure_get_capture_bits(closure, index);
        if !crate::r#box::scope::is_capture_cell_ptr(box_ptr) {
            return std::ptr::null();
        }
        mask &= mask - 1;
    }
    target.func_ptr
}

macro_rules! define_direct_call_site {
    (
        $(#[$meta:meta])*
        $site:ident, $arity:literal, $entry:ident, $slow:ident, $($arg:ident),+
    ) => {
        /// The fallback, a JS body: the full dispatcher with the receiver it
        /// is handed.
        extern "C" fn $slow(
            closure: *const ClosureHeader,
            this: crate::closure::JsThis,
            $($arg: f64),+
        ) -> f64 {
            $entry(closure, this, $($arg),+)
        }

        $(#[$meta])*
        #[derive(Clone, Copy)]
        pub struct $site(crate::closure::body_call::js_body_fn_ty!($($arg),+));

        impl $site {
            /// Resolve `closure` once, before the loop.
            #[inline]
            pub fn resolve(closure: *const ClosureHeader) -> Self {
                $site(resolve_direct_func_ptr(closure, $arity).map_or($slow, |func_ptr| unsafe {
                    crate::closure::body_call::js_body_fn!(func_ptr; $($arg),+)
                }))
            }

            /// Invoke with the CURRENT closure address (see the module docs on
            /// rooting) and receiver `this` (`plain_call_receiver()` for a
            /// plain call, a builtin's `thisArg` otherwise). Falls back to
            /// the full dispatcher when the closure did not resolve.
            #[inline]
            pub fn call(
                &self,
                closure: *const ClosureHeader,
                this: crate::closure::JsThis,
                $($arg: f64),+
            ) -> f64 {
                // SAFETY: `self.0` is the resolved body of `closure` at this
                // arity, or the dispatcher fallback.
                unsafe { (self.0)(closure, this, $($arg),+) }
            }

            /// Whether the direct target was resolved. Test-only: a "fast
            /// path" nobody can prove ran is not a fast path.
            #[cfg(test)]
            #[allow(dead_code)]
            pub(crate) fn is_direct(&self) -> bool {
                !std::ptr::fn_addr_eq(
                    self.0,
                    $slow as crate::closure::body_call::js_body_fn_ty!($($arg),+),
                )
            }
        }
    };
}

define_direct_call_site!(
    /// A 1-argument callback resolved once for a whole loop.
    DirectCall1,
    1,
    js_closure_call1,
    direct_call1_fallback,
    arg0
);

define_direct_call_site!(
    /// A 2-argument callback (comparators, `Map`/`Set` visitors) resolved once
    /// for a whole loop.
    DirectCall2,
    2,
    js_closure_call2,
    direct_call2_fallback,
    arg0,
    arg1
);

define_direct_call_site!(
    /// A 3-argument callback — `(element, index, array)`, the shape every
    /// `Array.prototype` iteration method uses — resolved once for a whole
    /// loop.
    DirectCall3,
    3,
    js_closure_call3,
    direct_call3_fallback,
    arg0,
    arg1,
    arg2
);

define_direct_call_site!(
    /// A 4-argument callback — `(accumulator, element, index, array)`, the
    /// `reduce`/`reduceRight` shape — resolved once for a whole loop.
    DirectCall4,
    4,
    js_closure_call4,
    direct_call4_fallback,
    arg0,
    arg1,
    arg2,
    arg3
);

#[cfg(test)]
mod tests {
    use super::*;

    // A capture-less body behind a real `ClosureHeader`, the same way
    // `array/tests.rs` and `array/typed_array_receiver_tests.rs` build theirs.
    extern "C" fn add3(
        _c: *const ClosureHeader,
        _this: crate::closure::JsThis,
        a: f64,
        b: f64,
        c: f64,
    ) -> f64 {
        a * 100.0 + b * 10.0 + c
    }

    extern "C" fn sum2(
        _c: *const ClosureHeader,
        _this: crate::closure::JsThis,
        a: f64,
        b: f64,
    ) -> f64 {
        a + b
    }

    extern "C" fn ordinary3(
        _c: *const ClosureHeader,
        _this: crate::closure::JsThis,
        a: f64,
        b: f64,
        c: f64,
    ) -> f64 {
        a + b + c
    }

    extern "C" fn rest_body(
        _c: *const ClosureHeader,
        _this: crate::closure::JsThis,
        _a: f64,
        _rest: f64,
    ) -> f64 {
        0.0
    }

    extern "C" fn trusted_add3(
        _c: *const ClosureHeader,
        _this: crate::closure::JsThis,
        a: f64,
        b: f64,
        c: f64,
    ) -> f64 {
        a * 100.0 + b * 10.0 + c
    }

    extern "C" fn boxed1(
        _c: *const ClosureHeader,
        _this: crate::closure::JsThis,
        value: f64,
    ) -> f64 {
        value
    }

    extern "C" fn trusted_boxed1(
        _c: *const ClosureHeader,
        _this: crate::closure::JsThis,
        value: f64,
    ) -> f64 {
        value
    }

    extern "C" fn versioned_source(
        _c: *const ClosureHeader,
        _this: crate::closure::JsThis,
        value: f64,
    ) -> f64 {
        value
    }

    extern "C" fn versioned_boxed1(_c: *const ClosureHeader, value: f64, _deopt: *mut u64) -> f64 {
        value
    }

    use crate::closure::{JsFunctionInfo, FN_ARROW};
    use crate::codegen_abi::{JsBody1, JsBody2, JsBody3};
    type C = ClosureHeader;

    const ADD3: JsFunctionInfo = JsFunctionInfo::of(add3 as JsBody3<C>).with_flags(FN_ARROW);
    static ADD3_ARROW: JsFunctionInfo = ADD3;
    static ADD3_TRUSTED: JsFunctionInfo = ADD3.with_trusted_direct(trusted_add3 as *const u8, 0, 0);
    static ORDINARY3: JsFunctionInfo = JsFunctionInfo::of(ordinary3 as JsBody3<C>);
    static REST_ARROW: JsFunctionInfo = JsFunctionInfo::of(rest_body as JsBody2<C>)
        .with_rest(1)
        .with_flags(FN_ARROW);
    static SUM2: JsFunctionInfo = JsFunctionInfo::of(sum2 as JsBody2<C>);
    static SUM2_REST: JsFunctionInfo = JsFunctionInfo::of(sum2 as JsBody2<C>).with_rest(1);
    static BOXED1: JsFunctionInfo = JsFunctionInfo::of(boxed1 as JsBody1<C>)
        .with_flags(FN_ARROW)
        .with_trusted_direct(trusted_boxed1 as *const u8, 1, 1);
    static VERSIONED: JsFunctionInfo = JsFunctionInfo::of(versioned_source as JsBody1<C>)
        .with_flags(FN_ARROW)
        .with_versioned_loop(versioned_boxed1 as *const u8, 1, 1);

    fn closure_for(info: &'static JsFunctionInfo) -> *const ClosureHeader {
        crate::closure::js_closure_alloc(info, 0)
    }

    #[test]
    fn exported_resolver_admits_directly_callable_bodies() {
        let arrow = closure_for(&ADD3_ARROW);
        assert_eq!(
            js_closure_resolve_plain_direct_call(arrow, 3),
            add3 as *const u8
        );
        let trusted = closure_for(&ADD3_TRUSTED);
        assert_eq!(
            js_closure_resolve_plain_direct_call(trusted, 3),
            trusted_add3 as *const u8
        );
        assert!(js_closure_resolve_plain_direct_call(trusted, 2).is_null());

        // An ordinary function is as directly callable as an arrow: the
        // caller passes the plain-call `undefined` receiver itself.
        let ordinary = closure_for(&ORDINARY3);
        assert_eq!(
            js_closure_resolve_plain_direct_call(ordinary, 3),
            ordinary3 as *const u8
        );

        let rest = closure_for(&REST_ARROW);
        assert!(js_closure_resolve_plain_direct_call(rest, 3).is_null());

        for sentinel in [
            &crate::closure::BOUND_METHOD_INFO,
            &crate::closure::BOUND_FUNCTION_INFO,
        ] {
            let bound = closure_for(sentinel);
            assert!(js_closure_resolve_plain_direct_call(bound, 3).is_null());
        }
    }

    #[test]
    fn trusted_target_requires_the_registered_capture_layout() {
        let wrong_count = closure_for(&BOXED1);
        assert_eq!(
            js_closure_resolve_plain_direct_call(wrong_count, 1),
            boxed1 as *const u8,
            "a wrong capture count must retain the checked public body"
        );

        let non_box = crate::closure::js_closure_alloc(&BOXED1, 1);
        crate::closure::js_closure_set_capture_bits(non_box, 0, crate::value::TAG_UNDEFINED);
        assert_eq!(
            js_closure_resolve_plain_direct_call(non_box, 1),
            boxed1 as *const u8,
            "a non-box payload must retain the checked public body"
        );

        let valid = crate::closure::js_closure_alloc(&BOXED1, 1);
        let cell = crate::r#box::js_box_alloc_bits(crate::value::TAG_UNDEFINED as i64);
        crate::closure::js_closure_set_box_capture_ptr(valid, 0, cell as i64);
        assert_eq!(
            js_closure_resolve_plain_direct_call(valid, 1),
            trusted_boxed1 as *const u8,
            "the exact compiler-installed layout must select the private body"
        );
    }

    #[test]
    fn versioned_target_is_exact_and_fails_closed() {
        let wrong_count = closure_for(&VERSIONED);
        assert!(js_closure_resolve_versioned_loop_direct_call(wrong_count, 1).is_null());

        let non_box = crate::closure::js_closure_alloc(&VERSIONED, 1);
        crate::closure::js_closure_set_capture_bits(non_box, 0, crate::value::TAG_UNDEFINED);
        assert!(js_closure_resolve_versioned_loop_direct_call(non_box, 1).is_null());

        let valid = crate::closure::js_closure_alloc(&VERSIONED, 1);
        let cell = crate::r#box::js_box_alloc_bits(crate::value::TAG_UNDEFINED as i64);
        crate::closure::js_closure_set_box_capture_ptr(valid, 0, cell as i64);
        assert_eq!(
            js_closure_resolve_versioned_loop_direct_call(valid, 1),
            versioned_boxed1 as *const u8
        );
        assert!(js_closure_resolve_versioned_loop_direct_call(valid, 0).is_null());
    }

    #[test]
    fn a_plain_callback_resolves_and_answers_identically_to_the_slow_path() {
        let c = closure_for(&ADD3_ARROW);
        let site = DirectCall3::resolve(c);
        // ASSERT THE SUBJECT IS LIVE. Without this the test passes just as
        // happily when `resolve` always answers `None` and every call falls
        // back — a "fast path" nobody can prove ran.
        assert!(
            site.is_direct(),
            "a capture-less, non-bound callback must resolve to a direct target \
             -- otherwise this whole module is inert"
        );
        assert_eq!(
            site.call(c, crate::closure::plain_call_receiver(), 1.0, 2.0, 3.0),
            123.0
        );
        assert_eq!(
            site.call(c, crate::closure::plain_call_receiver(), 1.0, 2.0, 3.0),
            js_closure_call3(c, crate::closure::plain_call_receiver(), 1.0, 2.0, 3.0)
        );
    }

    #[test]
    fn a_declared_arity_above_the_call_arity_is_declined() {
        let c = closure_for(&ADD3_ARROW);
        // Asked for at a LOWER arity than the body declares: the call must
        // keep going through `js_closure_call2`, which pads with undefined
        // via `dispatch_with_arity`.
        assert!(
            !DirectCall2::resolve(c).is_direct(),
            "declared arity 3 > call arity 2 must decline: a direct 2-arg call \
             would leave the third parameter as whatever was in the register"
        );
        // ...and the same body at its own arity still resolves.
        assert!(DirectCall3::resolve(c).is_direct());
    }

    #[test]
    fn a_rest_closure_is_declined() {
        assert!(
            DirectCall2::resolve(closure_for(&SUM2)).is_direct(),
            "precondition: the same body resolves without a rest parameter"
        );
        assert!(
            !DirectCall2::resolve(closure_for(&SUM2_REST)).is_direct(),
            "a rest parameter needs `dispatch_rest_bundled` to build the rest \
             array; a direct call would hand the body a bare f64"
        );
    }

    #[test]
    fn an_invalid_closure_pointer_is_declined_and_falls_back() {
        // `get_valid_func_ptr` rejects the small-handle band, so this is the
        // shape a NaN-boxed stdlib handle takes if one reaches a callback slot.
        let site = DirectCall1::resolve(0x40 as *const ClosureHeader);
        assert!(!site.is_direct());
    }
}
