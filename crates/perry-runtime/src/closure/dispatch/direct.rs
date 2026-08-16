//! Hoisted per-arity closure dispatch for callback loops (#8180).
//!
//! `js_closure_callN` re-derives, on EVERY call, three things that cannot
//! change while one closure is being called in a loop:
//!
//!   1. `get_valid_func_ptr` — two address-band checks plus a volatile
//!      `CLOSURE_MAGIC` probe through `*(closure + 12)` and a volatile load of
//!      `closure->func_ptr` (`dispatch/validate.rs`);
//!   2. `resolve_strategy` — a `perry_thread_local!` single-slot cache, which
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
//!   mutated, and a `ClosureHeader` is non-movable, so `get_valid_func_ptr`
//!   answers the same address every time. (A moving collection cannot change
//!   it either — `direct` is a static CODE address; that is the same argument
//!   `ComparatorCall::compare_at` documents.)
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
        if declared > arity {
            return None;
        }
    }
    Some(func_ptr)
}

macro_rules! define_direct_call_site {
    (
        $(#[$meta:meta])*
        $site:ident, $arity:literal, $slow:ident, $($arg:ident),+
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy)]
        pub struct $site(Option<extern "C" fn(*const ClosureHeader, $(define_direct_call_site!(@f64 $arg)),+) -> f64>);

        impl $site {
            /// Resolve `closure` once, before the loop.
            #[inline]
            pub fn resolve(closure: *const ClosureHeader) -> Self {
                $site(resolve_direct_func_ptr(closure, $arity).map(|func_ptr| unsafe {
                    std::mem::transmute::<
                        *const u8,
                        extern "C" fn(*const ClosureHeader, $(define_direct_call_site!(@f64 $arg)),+) -> f64,
                    >(func_ptr)
                }))
            }

            /// Invoke with the CURRENT closure address (see the module docs on
            /// rooting). Falls back to the full dispatcher when the closure
            /// did not resolve.
            #[inline]
            pub fn call(&self, closure: *const ClosureHeader, $($arg: f64),+) -> f64 {
                match self.0 {
                    Some(func) => func(closure, $($arg),+),
                    None => $slow(closure, $($arg),+),
                }
            }

            /// Whether the direct target was resolved. Test-only: a "fast
            /// path" nobody can prove ran is not a fast path.
            #[cfg(test)]
            #[allow(dead_code)]
            pub(crate) fn is_direct(&self) -> bool {
                self.0.is_some()
            }
        }
    };
    (@f64 $arg:ident) => { f64 };
}

define_direct_call_site!(
    /// A 1-argument callback resolved once for a whole loop.
    DirectCall1,
    1,
    js_closure_call1,
    arg0
);

define_direct_call_site!(
    /// A 2-argument callback (comparators, `Map`/`Set` visitors) resolved once
    /// for a whole loop.
    DirectCall2,
    2,
    js_closure_call2,
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
    extern "C" fn add3(_c: *const ClosureHeader, a: f64, b: f64, c: f64) -> f64 {
        a * 100.0 + b * 10.0 + c
    }

    extern "C" fn sum2(_c: *const ClosureHeader, a: f64, b: f64) -> f64 {
        a + b
    }

    fn closure_for(body: *const u8) -> *const ClosureHeader {
        crate::closure::js_closure_alloc(body, 0)
    }

    #[test]
    fn a_plain_callback_resolves_and_answers_identically_to_the_slow_path() {
        let c = closure_for(add3 as *const u8);
        let site = DirectCall3::resolve(c);
        // ASSERT THE SUBJECT IS LIVE. Without this the test passes just as
        // happily when `resolve` always answers `None` and every call falls
        // back — a "fast path" nobody can prove ran (CLAUDE.md, the fourth way
        // a gate cannot fail).
        assert!(
            site.is_direct(),
            "an unregistered, capture-less, non-bound callback must resolve to \
             a direct target -- otherwise this whole module is inert"
        );
        assert_eq!(site.call(c, 1.0, 2.0, 3.0), 123.0);
        assert_eq!(
            site.call(c, 1.0, 2.0, 3.0),
            js_closure_call3(c, 1.0, 2.0, 3.0)
        );
    }

    #[test]
    fn a_declared_arity_above_the_call_arity_is_declined() {
        let c = closure_for(add3 as *const u8);
        // Same body, asked for at a LOWER arity than it declares: the call
        // must keep going through `js_closure_call2`, which pads with
        // undefined via `dispatch_with_arity`.
        crate::closure::js_register_closure_arity(add3 as *const u8, 3);
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
        let c = closure_for(sum2 as *const u8);
        assert!(
            DirectCall2::resolve(c).is_direct(),
            "precondition: it resolves before being registered as rest"
        );
        crate::closure::js_register_closure_rest(sum2 as *const u8, 1);
        assert!(
            !DirectCall2::resolve(c).is_direct(),
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
