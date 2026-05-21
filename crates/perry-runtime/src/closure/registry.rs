//! Closure registries (rest/arity), dispatch-strategy resolution, and
//! rest-arg / arity-pad helpers.

use super::*;
use std::cell::RefCell;

// Side-table mapping closure body `func_ptr` -> fixed_arity (number of fixed
// params declared BEFORE the rest param). Populated at module init by
// `js_register_closure_rest` for every closure body whose HIR signature ends
// in `...rest`. Looked up by `js_closure_callN` so that calls through dynamic
// dispatch (e.g. `obj.cb(a, b, c)` where `cb` is a class field holding an
// arrow) bundle trailing args into the rest array — the previous behavior
// passed unbundled args, leaving the rest param bound to the first trailing
// arg as a scalar (issue #493 / #421-rest fix). Static call sites (named
// functions, `Expr::FuncRef`, local closure-bound `let`) keep their existing
// bundling at the call site, which is faster — the registry is consulted only
// when needed.
//
// Stored as a thread-local rather than a global RwLock because closures are
// thread-local in perry's runtime model (each thread has its own arena +
// GC), so a per-thread copy avoids the per-call lock acquisition. Module
// init runs on the main thread and populates one entry per
// rest-param-bearing closure body in the program; worker threads (issue
// #29 `perry/thread`) currently don't see the table because they aren't
// supposed to invoke arbitrary user closures across the boundary anyway.
thread_local! {
    /// (fixed_arity, synthetic_arguments) — synthetic_arguments=true means
    /// the rest param is the synthesized `arguments` array (HIR-injected
    /// when the body reads `arguments` without a user-declared rest), so
    /// the runtime must bundle ALL passed args into the rest slot (not
    /// just trailing ones after `fixed_arity`). Refs #915 (gap 1 from
    /// #899 — Effect's `dual(arity, body)` arity detection).
    static CLOSURE_REST_REGISTRY: RefCell<crate::fast_hash::PtrHashMap<usize, (u32, bool)>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
    /// Side-table mapping closure body `func_ptr` -> declared param count
    /// (for closures WITHOUT a rest param — those use CLOSURE_REST_REGISTRY).
    /// Populated at module init by `js_register_closure_arity`. Looked up by
    /// `js_native_call_value` (the dynamic dispatch path used when a closure is
    /// stored as a class field and called method-style on an any-typed
    /// receiver) so the runtime can pad missing args with TAG_UNDEFINED to
    /// match the closure body's declared arity. Without this, calling a
    /// 3-param arrow with 1 arg through `js_native_call_method` →
    /// `js_native_call_value` → `js_closure_call1` transmutes the func_ptr to
    /// a 1-arg signature and the closure body reads garbage for params 2 and
    /// 3 (refs #421 — hono's `c.text(text)` / `setDefaultContentType` chain
    /// hit exactly this; uninit `headers` slot evaluated to a small denormal
    /// float, slow-path runs, `setDefaultContentType` returns a header object,
    /// `responseHeaders.set(k, v)` then fails with a #510-class TypeError).
    static CLOSURE_ARITY_REGISTRY: RefCell<crate::fast_hash::PtrHashMap<usize, u32>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());

    /// Unified dispatch lookup, populated lazily on first call to a func_ptr.
    /// Cuts the per-call cost from TWO RefCell::borrow + HashMap::get
    /// (one each for rest and arity) down to ONE — material on hot paths
    /// like `array.sort` (25M comparisons) or `Promise.all` of N async
    /// chains (150k microtasks for the 1k-batch x 50-promise x 3-await
    /// shape). The fast path on a cache hit is one borrow + one
    /// HashMap::get + a small-enum branch.
    static DISPATCH_CACHE: RefCell<crate::fast_hash::PtrHashMap<usize, DispatchStrategy>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

/// Magic value stored in ClosureHeader._reserved to identify closures at runtime.
/// Used by js_value_typeof to return "function" instead of "object" for closures.
pub const CLOSURE_MAGIC: u32 = 0x434C_4F53; // "CLOS" in ASCII

/// Per-call dispatch strategy for a closure body. Decided once at first
/// call, cached in `DISPATCH_CACHE` thereafter.
#[derive(Clone, Copy)]
pub enum DispatchStrategy {
    /// Bound-method receiver pretending to be a closure (BOUND_METHOD_FUNC_PTR
    /// sentinel). Dispatch via `dispatch_bound_method`.
    BoundMethod,
    /// Closure body has a rest param at the given fixed_arity index.
    /// Dispatch via `dispatch_rest_bundled`. The bool flag is true when
    /// the rest param is the synthesized `arguments` array (HIR-injected
    /// when the body reads `arguments`); in that case all passed args
    /// are bundled into the rest slot (not just the trailing tail).
    Rest(u32, bool),
    /// Closure body declares an arity higher than the call sites use;
    /// dispatch must pad with TAG_UNDEFINED via `dispatch_with_arity`.
    Arity(u32),
    /// Direct callable: just transmute func_ptr to typed fn pointer
    /// (declared arity == call site arity, no rest, no bound method).
    /// The hot path for the vast majority of closure call sites.
    Direct,
}

thread_local! {
    /// Last-resolved (func_ptr, strategy) tuple — single-slot direct cache.
    /// Avoids the per-call HashMap::get + RefCell::borrow when the same
    /// closure body is invoked back-to-back, which is the steady-state
    /// shape of:
    ///   - microtask drain (same then_v_arrow / __step body each iter)
    ///   - tight `array.sort` callbacks (same comparator every comparison)
    ///   - hot `array.map` / `array.forEach` loops
    /// Cache key is the func_ptr (usize) and is checked with a single
    /// load + cmp.
    static DISPATCH_LAST: std::cell::Cell<(usize, DispatchStrategy)> =
        const { std::cell::Cell::new((0, DispatchStrategy::Direct)) };
}

#[inline(always)]
pub fn resolve_strategy(func_ptr: *const u8) -> DispatchStrategy {
    let key = func_ptr as usize;
    // Inline single-slot cache: 90%+ of microtask-drain hot paths
    // dispatch the same func_ptr back-to-back. One Cell::get + one cmp
    // beats the RefCell::borrow + HashMap::get of DISPATCH_CACHE.
    let last = DISPATCH_LAST.with(|c| c.get());
    if last.0 == key {
        return last.1;
    }
    let strategy = resolve_strategy_slow(func_ptr);
    DISPATCH_LAST.with(|c| c.set((key, strategy)));
    strategy
}

#[inline(never)]
fn resolve_strategy_slow(func_ptr: *const u8) -> DispatchStrategy {
    let key = func_ptr as usize;
    // Fast path: read existing cache entry.
    if let Some(s) = DISPATCH_CACHE.with(|c| c.borrow().get(&key).copied()) {
        return s;
    }
    // First call for this func_ptr: compute the strategy and cache it.
    if func_ptr == BOUND_METHOD_FUNC_PTR {
        DISPATCH_CACHE.with(|c| {
            c.borrow_mut().insert(key, DispatchStrategy::BoundMethod);
        });
        return DispatchStrategy::BoundMethod;
    }
    let strategy = if let Some((fixed_arity, synthetic)) = lookup_closure_rest_full(func_ptr) {
        DispatchStrategy::Rest(fixed_arity, synthetic)
    } else if let Some(declared) = lookup_closure_arity(func_ptr) {
        DispatchStrategy::Arity(declared)
    } else {
        DispatchStrategy::Direct
    };
    DISPATCH_CACHE.with(|c| {
        c.borrow_mut().insert(key, strategy);
    });
    strategy
}

/// Register that the closure body at `func_ptr` has a rest parameter at index
/// `fixed_arity` (i.e., the closure has `fixed_arity` fixed params before the
/// rest param, and its declared LLVM arity is `fixed_arity + 1` — the +1 is
/// the rest array). Called once per closure literal at module init time.
#[no_mangle]
pub extern "C" fn js_register_closure_rest(func_ptr: *const u8, fixed_arity: u32) {
    if func_ptr.is_null() {
        return;
    }
    CLOSURE_REST_REGISTRY.with(|r| {
        r.borrow_mut()
            .insert(func_ptr as usize, (fixed_arity, false));
    });
}

/// Like `js_register_closure_rest`, but flags the rest param as the
/// synthesized `arguments` array. The HIR's `append_synthetic_arguments_param`
/// helper appends an `arguments` rest param whenever a function body reads
/// `arguments` and the user hasn't already declared a rest of their own.
/// JS spec semantics: `arguments.length` counts ALL passed args (data-first
/// AND any trailing). Without this flag, `dispatch_rest_bundled` was binding
/// fixed params first and then bundling only the post-`fixed_arity` tail
/// into the rest, so `function(a, b) { return arguments.length }` called as
/// `f(10, 20)` saw `arguments.length === 0`. Refs #915 (gap 1 from #899).
#[no_mangle]
pub extern "C" fn js_register_closure_synthetic_arguments(func_ptr: *const u8, fixed_arity: u32) {
    if func_ptr.is_null() {
        return;
    }
    CLOSURE_REST_REGISTRY.with(|r| {
        r.borrow_mut()
            .insert(func_ptr as usize, (fixed_arity, true));
    });
}

#[inline(always)]
pub fn lookup_closure_rest(func_ptr: *const u8) -> Option<u32> {
    CLOSURE_REST_REGISTRY.with(|r| {
        r.borrow()
            .get(&(func_ptr as usize))
            .map(|(arity, _)| *arity)
    })
}

#[inline(always)]
pub fn lookup_closure_rest_full(func_ptr: *const u8) -> Option<(u32, bool)> {
    CLOSURE_REST_REGISTRY.with(|r| r.borrow().get(&(func_ptr as usize)).copied())
}

/// Register a closure body's declared param count (for closures WITHOUT a rest
/// param). Called once per non-rest closure literal at module init time.
/// See `CLOSURE_ARITY_REGISTRY` doc for rationale.
#[no_mangle]
pub extern "C" fn js_register_closure_arity(func_ptr: *const u8, arity: u32) {
    if func_ptr.is_null() {
        return;
    }
    CLOSURE_ARITY_REGISTRY.with(|r| {
        r.borrow_mut().insert(func_ptr as usize, arity);
    });
}

#[inline(always)]
pub fn lookup_closure_arity(func_ptr: *const u8) -> Option<u32> {
    CLOSURE_ARITY_REGISTRY.with(|r| r.borrow().get(&(func_ptr as usize)).copied())
}

/// Public helper: given a `*const ClosureHeader` pointer, return the
/// closure's declared arity if known. Falls back to the rest-registry
/// fixed-arity entry for closures declared with `...rest`. Returns
/// `None` if the pointer isn't a valid closure or no arity was
/// registered.
///
/// Used by the `.length` property accessor on closure values so
/// `fn.length` returns the spec-compliant declared-param count
/// (e.g. ramda's `converge` / `juxt` chain that builds a curry arity
/// from `pluck('length', fns)` — without `.length` returning a real
/// number, the chain feeds `NaN` to `_arity` and throws
/// `First argument to _arity must be a non-negative integer no greater
/// than ten`).
pub fn closure_arity(closure: *const ClosureHeader) -> Option<u32> {
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return None;
    }
    // Closures declared with `...rest` register through a separate
    // registry path; prefer the fixed-arity portion of that entry when
    // present so `length` matches the user-visible declared params.
    if let Some((arity, _synth)) = lookup_closure_rest_full(func_ptr) {
        return Some(arity);
    }
    lookup_closure_arity(func_ptr)
}

/// Build a JS array from a slice of NaN-boxed f64 values and return it
/// NaN-boxed as a pointer. Used by the rest-bundling helper below.
#[inline(always)]
pub unsafe fn build_rest_array(values: &[f64]) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handles: Vec<_> = values
        .iter()
        .map(|value| scope.root_nanbox_f64(*value))
        .collect();
    let arr = crate::array::js_array_alloc(values.len() as u32);
    let mut cur = arr;
    for handle in value_handles.iter() {
        cur = crate::array::js_array_push_f64(cur, handle.get_nanbox_f64());
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
/// Currently supports `fixed_arity` in `0..=15` — the same ceiling as
/// `js_closure_callN`. A program that defines a rest closure with more than
/// 15 fixed params before the rest is unsupported (and would already trip
/// the `Phase D.1: closure call with N args (max 16)` guard in lower_call).
#[inline(never)]
pub unsafe fn dispatch_rest_bundled(
    closure: *const ClosureHeader,
    func_ptr: *const u8,
    args: &[f64],
    fixed_arity: u32,
    synthetic_arguments: bool,
) -> f64 {
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let k = fixed_arity as usize;
    let provided = args.len();
    let arg_scope = crate::gc::RuntimeHandleScope::new();
    let arg_handles: Vec<_> = args
        .iter()
        .map(|value| arg_scope.root_nanbox_f64(*value))
        .collect();

    // Bundle args into the rest array.
    //
    // For a user-declared `...rest`, this is the trailing tail past the
    // fixed params. For the HIR-synthesized `arguments` rest, JS spec
    // semantics require ALL passed args — `arguments.length === args.length`
    // regardless of how many fixed params the function declared. Refs
    // #915 (gap 1 from #899): Effect's `dual(arity, body)` checks
    // `arguments.length` to discriminate data-first vs data-last, and the
    // body is `function (a, b) { … arguments.length … }` — pre-fix only
    // post-`b` args showed up, so `dual(2, body)(x, y)` saw 0.
    let rest_slice: &[f64] = if synthetic_arguments {
        args
    } else if provided > k {
        &args[k..]
    } else {
        &[]
    };
    let rest_double = build_rest_array(rest_slice);

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

    match k {
        0 => {
            let f: extern "C" fn(*const ClosureHeader, f64) -> f64 = std::mem::transmute(func_ptr);
            f(closure, rest_double)
        }
        1 => {
            let f: extern "C" fn(*const ClosureHeader, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(closure, a!(0), rest_double)
        }
        2 => {
            let f: extern "C" fn(*const ClosureHeader, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(closure, a!(0), a!(1), rest_double)
        }
        3 => {
            let f: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(closure, a!(0), a!(1), a!(2), rest_double)
        }
        4 => {
            let f: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(closure, a!(0), a!(1), a!(2), a!(3), rest_double)
        }
        5 => {
            let f: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(closure, a!(0), a!(1), a!(2), a!(3), a!(4), rest_double)
        }
        6 => {
            let f: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(
                closure,
                a!(0),
                a!(1),
                a!(2),
                a!(3),
                a!(4),
                a!(5),
                rest_double,
            )
        }
        7 => {
            let f: extern "C" fn(
                *const ClosureHeader,
                f64,
                f64,
                f64,
                f64,
                f64,
                f64,
                f64,
                f64,
            ) -> f64 = std::mem::transmute(func_ptr);
            f(
                closure,
                a!(0),
                a!(1),
                a!(2),
                a!(3),
                a!(4),
                a!(5),
                a!(6),
                rest_double,
            )
        }
        _ => {
            // Unsupported arity — fall back to undefined so we don't
            // mis-call the body and trigger UB. This mirrors the upper
            // bound that lower_call's static-bundling path enforces.
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
    }
}

/// Dispatch a closure call where the caller supplied fewer args than the
/// closure declared. Pad the missing slots with `undefined` and call the
/// body's actual declared signature so the body's `LocalGet(N)` reads
/// correctly initialised slots instead of stale registers.
///
/// `func_ptr` is already validated and known non-BOUND, non-rest.
/// `declared_arity` is what `CLOSURE_ARITY_REGISTRY` recorded for this body
/// at module init time. Callers reach here only when `args.len() < declared_arity`.
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
    match k {
        0 => {
            let f: extern "C" fn(*const ClosureHeader) -> f64 = std::mem::transmute(func_ptr);
            f(closure)
        }
        1 => {
            let f: extern "C" fn(*const ClosureHeader, f64) -> f64 = std::mem::transmute(func_ptr);
            f(closure, a!(0))
        }
        2 => {
            let f: extern "C" fn(*const ClosureHeader, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(closure, a!(0), a!(1))
        }
        3 => {
            let f: extern "C" fn(*const ClosureHeader, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(closure, a!(0), a!(1), a!(2))
        }
        4 => {
            let f: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(closure, a!(0), a!(1), a!(2), a!(3))
        }
        5 => {
            let f: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(closure, a!(0), a!(1), a!(2), a!(3), a!(4))
        }
        6 => {
            let f: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(closure, a!(0), a!(1), a!(2), a!(3), a!(4), a!(5))
        }
        7 => {
            let f: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(closure, a!(0), a!(1), a!(2), a!(3), a!(4), a!(5), a!(6))
        }
        8 => {
            let f: extern "C" fn(
                *const ClosureHeader,
                f64,
                f64,
                f64,
                f64,
                f64,
                f64,
                f64,
                f64,
            ) -> f64 = std::mem::transmute(func_ptr);
            f(
                closure,
                a!(0),
                a!(1),
                a!(2),
                a!(3),
                a!(4),
                a!(5),
                a!(6),
                a!(7),
            )
        }
        _ => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}

/// Sentinel func_ptr value indicating this closure is a "bound method" on a native module.
/// When js_closure_callN detects this, it extracts captures and dispatches via js_native_call_method.
/// Captures layout: [0] = namespace_obj (f64), [1] = method_name_ptr (i64), [2] = method_name_len (i64)
pub const BOUND_METHOD_FUNC_PTR: *const u8 = 0xBADD_DEAD_u64 as *const u8;

/// Flag stored in the high bit of capture_count to indicate that capture slot 0
/// holds `this` (i.e., this closure is an object literal method that captures `this`).
/// When the closure is detached from the object (assigned to a variable via PropertyGet),
/// `js_closure_unbind_this` clones it and clears slot 0 so `this` becomes undefined.
pub const CAPTURES_THIS_FLAG: u32 = 0x8000_0000;

/// Extract the real capture count (masking out the CAPTURES_THIS_FLAG).
#[inline(always)]
pub fn real_capture_count(capture_count: u32) -> u32 {
    capture_count & !CAPTURES_THIS_FLAG
}
