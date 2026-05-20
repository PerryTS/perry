//! Closure runtime support for Perry
//!
//! A closure is a function pointer plus captured environment.
//! Layout:
//!   - ClosureHeader at the start
//!   - Followed by captured values (as f64 or i64 pointers)

use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// Singleton cache keyed by `func_ptr` for non-capturing closures.
    /// See `js_closure_alloc_singleton` and `scan_singleton_closure_roots_mut`.
    /// Pointer-keyed; uses `PtrHasher` (Fibonacci-multiplicative) to
    /// skip SipHash's per-byte cost — the function-pointer keys never
    /// come from external input and are already ~uniformly distributed.
    static SINGLETON_CLOSURES: RefCell<crate::fast_hash::PtrHashMap<usize, *mut ClosureHeader>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());

    /// Per-`func_ptr` single-slot cache for closures with captures.
    /// Each value is `(last_captures, last_closure)` — when the same
    /// closure literal is created again with the SAME capture bits,
    /// we return the cached closure; otherwise we allocate a fresh
    /// one and replace the slot.
    ///
    /// One entry per closure literal (bounded by the number of
    /// `Expr::Closure` sites in the program), not per
    /// `(func_ptr, capture-tuple)` pair — this prevents a closure
    /// whose captures vary per call (e.g.
    /// `getOrCompute(map, key, () => new Foo(sortedTypes))` capturing
    /// a fresh array per call) from filling the cache and crowding
    /// out closures with stable captures.
    /// Per-`func_ptr` small-LRU cache. Each entry holds up to
    /// `MAX_CAPTURED_CLOSURE_SLOTS` (captures-bits, ClosureHeader)
    /// pairs. Multiple slots are critical for the parallel-instance
    /// async-await pattern (e.g. `Promise.all` of N async closures
    /// each capturing its own boxed `__async_step`), where a single-
    /// slot cache evicts every cycle and effectively never hits.
    /// `PtrHasher`-keyed for the same reason as the other registries
    /// here — on `promise_all_chains` this is hit on every closure
    /// alloc (150 k/run).
    static SINGLETON_CAPTURED_CLOSURES: RefCell<crate::fast_hash::PtrHashMap<usize, Vec<(Vec<u64>, *mut ClosureHeader)>>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

/// Magic value stored in ClosureHeader._reserved to identify closures at runtime.
/// Used by js_value_typeof to return "function" instead of "object" for closures.
pub const CLOSURE_MAGIC: u32 = 0x434C_4F53; // "CLOS" in ASCII

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

/// Per-call dispatch strategy for a closure body. Decided once at first
/// call, cached in `DISPATCH_CACHE` thereafter.
#[derive(Clone, Copy)]
enum DispatchStrategy {
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
fn resolve_strategy(func_ptr: *const u8) -> DispatchStrategy {
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
fn lookup_closure_rest(func_ptr: *const u8) -> Option<u32> {
    CLOSURE_REST_REGISTRY.with(|r| {
        r.borrow()
            .get(&(func_ptr as usize))
            .map(|(arity, _)| *arity)
    })
}

#[inline(always)]
fn lookup_closure_rest_full(func_ptr: *const u8) -> Option<(u32, bool)> {
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
fn lookup_closure_arity(func_ptr: *const u8) -> Option<u32> {
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
unsafe fn build_rest_array(values: &[f64]) -> f64 {
    let arr = crate::array::js_array_alloc(values.len() as u32);
    let mut cur = arr;
    for v in values {
        cur = crate::array::js_array_push_f64(cur, *v);
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
unsafe fn dispatch_rest_bundled(
    closure: *const ClosureHeader,
    func_ptr: *const u8,
    args: &[f64],
    fixed_arity: u32,
    synthetic_arguments: bool,
) -> f64 {
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let k = fixed_arity as usize;
    let provided = args.len();

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
                args[$i]
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
unsafe fn dispatch_with_arity(
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

/// Header for heap-allocated closures
#[repr(C)]
pub struct ClosureHeader {
    /// Function pointer (the actual compiled function)
    pub func_ptr: *const u8,
    /// Number of captured values
    pub capture_count: u32,
    /// Type tag: set to CLOSURE_MAGIC to identify closures at runtime
    pub type_tag: u32,
}

/// Allocate a closure with space for captured values.
/// The high bit of `capture_count` may contain CAPTURES_THIS_FLAG to indicate
/// that slot 0 is reserved for `this`. The flag is preserved in the header
/// for later use by `js_closure_unbind_this`, but the actual allocation size
/// uses only the lower 31 bits.
/// Returns pointer to ClosureHeader
#[no_mangle]
pub extern "C" fn js_closure_alloc(func_ptr: *const u8, capture_count: u32) -> *mut ClosureHeader {
    crate::promise::bump(&CLOSURE_ALLOC_COUNT);
    let actual_count = real_capture_count(capture_count) as usize;
    let captures_size = actual_count * 8; // Each capture is 8 bytes (f64 or i64)
    let total_size = std::mem::size_of::<ClosureHeader>() + captures_size;

    let raw = crate::gc::gc_malloc(total_size, crate::gc::GC_TYPE_CLOSURE);
    let ptr = raw as *mut ClosureHeader;

    unsafe {
        (*ptr).func_ptr = func_ptr;
        (*ptr).capture_count = capture_count; // Preserve flag in high bit
        (*ptr).type_tag = CLOSURE_MAGIC;
        crate::gc::layout_init_pointer_free(ptr as *mut u8);
    }

    ptr
}

pub static CLOSURE_ALLOC_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static CLOSURE_CAP_SINGLETON_HIT: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
pub static CLOSURE_CAP_SINGLETON_MISS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Singleton-cached closure allocation for non-capturing closures and FuncRef
/// wrappers. The same `func_ptr` always yields the SAME ClosureHeader, so a
/// hot loop like `arr.filter(x => x.kind === 'foo')` doesn't allocate (and
/// trigger GC against) a fresh closure on every iteration.
///
/// Per-call cost: one thread-local hashmap lookup + one branch + one load.
/// Roughly 50× faster than `js_closure_alloc` for the no-capture case
/// because the gc_malloc path runs `gc_check_trigger` which can fire a
/// minor collection — a single hot non-capturing closure inside a tight
/// for-loop was the dominant cost in sync-hotpath / perf-comprehensive
/// (sample profile pinned 7/11 samples on `isDontFragmentRelation` →
/// `js_closure_alloc` → `gc_collect_minor`).
///
/// Safety: the cached closure has zero captures, so it has no per-call
/// state — sharing it across all call sites is observationally identical
/// to allocating fresh. The closure is GC-rooted by the singleton table's
/// mutable scanner so it stays live across collections.
#[no_mangle]
pub extern "C" fn js_closure_alloc_singleton(func_ptr: *const u8) -> *mut ClosureHeader {
    // Fast path: already cached. Drop the borrow before any potential
    // alloc so gc_malloc can re-enter SINGLETON_CLOSURES if it ever needs to.
    if let Some(cached) = SINGLETON_CLOSURES.with(|s| s.borrow().get(&(func_ptr as usize)).copied())
    {
        return cached;
    }
    let allocated = js_closure_alloc(func_ptr, 0);
    SINGLETON_CLOSURES.with(|s| {
        s.borrow_mut().insert(func_ptr as usize, allocated);
    });
    allocated
}

/// Mutable GC scanner for singleton closure caches.
///
/// No-capture cache values are raw closure pointers. Captured cache entries
/// additionally keep a bit-exact capture tuple as the cache key; each key word
/// can be a NaN-boxed JSValue or a raw heap pointer, matching closure capture
/// storage. The mutable visitor lets copied-minor rewrite both the closure's
/// heap capture slots and the cache key words after moving young captures.
pub fn scan_singleton_closure_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    SINGLETON_CLOSURES.with(|s| {
        let mut closures = s.borrow_mut();
        for closure in closures.values_mut() {
            visitor.visit_raw_mut_ptr_slot(closure);
        }
    });
    SINGLETON_CAPTURED_CLOSURES.with(|s| {
        let mut captured = s.borrow_mut();
        for slots in captured.values_mut() {
            for (capture_key, closure) in slots.iter_mut() {
                visitor.visit_raw_mut_ptr_slot(closure);
                for word in capture_key.iter_mut() {
                    visitor.visit_heap_word_u64_slot(word);
                }
            }
        }
    });
}

#[cfg(test)]
pub(crate) fn test_clear_singleton_closure_caches() {
    SINGLETON_CLOSURES.with(|s| s.borrow_mut().clear());
    SINGLETON_CAPTURED_CLOSURES.with(|s| s.borrow_mut().clear());
    CAPTURED_MISS_STREAK.with(|s| s.borrow_mut().clear());
}

#[cfg(test)]
pub(crate) fn test_seed_singleton_closure_cache(func_ptr: *const u8, closure: *mut ClosureHeader) {
    SINGLETON_CLOSURES.with(|s| {
        s.borrow_mut().insert(func_ptr as usize, closure);
    });
}

#[cfg(test)]
pub(crate) fn test_seed_captured_singleton_closure_cache(
    func_ptr: *const u8,
    capture_key: Vec<u64>,
    closure: *mut ClosureHeader,
) {
    SINGLETON_CAPTURED_CLOSURES.with(|s| {
        s.borrow_mut()
            .entry(func_ptr as usize)
            .or_insert_with(Vec::new)
            .insert(0, (capture_key, closure));
    });
}

#[cfg(test)]
pub(crate) fn test_singleton_closure_cache_entry(
    func_ptr: *const u8,
) -> Option<*mut ClosureHeader> {
    SINGLETON_CLOSURES.with(|s| s.borrow().get(&(func_ptr as usize)).copied())
}

#[cfg(test)]
pub(crate) fn test_captured_singleton_closure_cache_entries(
    func_ptr: *const u8,
) -> Vec<(Vec<u64>, *mut ClosureHeader)> {
    SINGLETON_CAPTURED_CLOSURES.with(|s| {
        s.borrow()
            .get(&(func_ptr as usize))
            .cloned()
            .unwrap_or_default()
    })
}

/// Maximum number of (captures-tuple, ClosureHeader) entries cached
/// per-`func_ptr` in `SINGLETON_CAPTURED_CLOSURES`. Sized to absorb the
/// parallel-instance async-await pattern (e.g. `Promise.all` of N
/// concurrent unitOfWork calls each capturing their own boxed
/// `__async_step`) without filling the cache when N is large. The
/// LRU eviction inside the slot list keeps the most-recently-seen
/// entries hot. Empirical: capping at 64 keeps memory bounded but
/// covers the per-batch fan-out shape (50 promises) found in
/// `benchmarks/app-patterns/kernels/promise_all_chains.ts`.
const MAX_CAPTURED_CLOSURE_SLOTS: usize = 64;

/// Per-`func_ptr` cache miss-streak counter for the adaptive bypass.
/// Closures whose captures change every call (per-call boxes for
/// `__step` / `__gen_state`, etc.) miss 100% of the time on the
/// captures-tuple cache; after `CAPTURED_MISS_STREAK_DISABLE` consecutive
/// misses we mark the `func_ptr` as "cache-disabled" and route it to a
/// direct `js_closure_alloc + memcpy` with no HashMap touch, no Vec scan,
/// no Vec::to_vec capture-tuple allocation. A future hit (e.g. if the
/// workload changes shape and captures stabilise) resets the counter.
const CAPTURED_MISS_STREAK_DISABLE: u32 = 256;
const CAPTURED_DISABLED_SENTINEL: u32 = u32::MAX;

thread_local! {
    static CAPTURED_MISS_STREAK: RefCell<crate::fast_hash::PtrHashMap<usize, u32>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

/// Per-`func_ptr` single-slot cache for closures with captures. When
/// the same closure literal is created again with the SAME capture
/// bits, we return the cached closure; otherwise we allocate a fresh
/// one and replace the slot.
///
/// `captures_ptr` points at `capture_count` consecutive 8-byte values
/// matching the layout `js_closure_set_capture_f64` writes.
///
/// One entry per closure literal (bounded by program size). Closures
/// whose captures vary per call (e.g. `getOrCompute(map, key, () =>
/// ...)` capturing a fresh array each call) miss every time but only
/// occupy one slot, so they don't crowd out steady-state captures.
#[no_mangle]
pub extern "C" fn js_closure_alloc_with_captures_singleton(
    func_ptr: *const u8,
    capture_count: u32,
    captures_ptr: *const u64,
) -> *mut ClosureHeader {
    let n = real_capture_count(capture_count) as usize;
    let captures_slice: &[u64] = if n == 0 || captures_ptr.is_null() {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(captures_ptr, n) }
    };

    // Adaptive bypass: if this func_ptr has missed the cache N times in
    // a row, skip the cache entirely. Async-step closures (`__step` /
    // `next` / `throw` / `__then_v` / `__then_e`) all capture a fresh
    // box pointer per invocation so they miss 100% of the time; the
    // bypass turns ~150 ns of cache-lookup overhead per call into a
    // ~50 ns direct `gc_malloc + memcpy`.
    let streak =
        CAPTURED_MISS_STREAK.with(|m| m.borrow().get(&(func_ptr as usize)).copied().unwrap_or(0));
    if streak == CAPTURED_DISABLED_SENTINEL {
        crate::promise::bump(&CLOSURE_CAP_SINGLETON_MISS);
        let allocated = js_closure_alloc(func_ptr, capture_count);
        if n > 0 && !captures_ptr.is_null() {
            unsafe {
                let dest =
                    (allocated as *mut u8).add(std::mem::size_of::<ClosureHeader>()) as *mut u64;
                std::ptr::copy_nonoverlapping(captures_ptr, dest, n);
                crate::gc::layout_rebuild_from_slots(allocated as *mut u8, dest as *const u64, n);
            }
        }
        return allocated;
    }

    // Fast path: scan the per-`func_ptr` slot list looking for a
    // matching capture-tuple. We touch only the cached `Vec` (small,
    // bounded by MAX_CAPTURED_CLOSURE_SLOTS). The match check is
    // bit-equality of u64 capture slots — same as a plain primitive
    // value comparison. Move the matched entry to the front to keep
    // recency information for the LRU eviction policy below.
    if let Some(cached) = SINGLETON_CAPTURED_CLOSURES.with(|s| {
        let mut s = s.borrow_mut();
        if let Some(slots) = s.get_mut(&(func_ptr as usize)) {
            for i in 0..slots.len() {
                if slots[i].0.as_slice() == captures_slice {
                    let entry = slots.remove(i);
                    let ptr = entry.1;
                    slots.insert(0, entry);
                    return Some(ptr);
                }
            }
        }
        None
    }) {
        crate::promise::bump(&CLOSURE_CAP_SINGLETON_HIT);
        // Cache hit — reset the streak so a workload that briefly
        // thrashed then settled into stable captures gets caching back.
        CAPTURED_MISS_STREAK.with(|m| {
            m.borrow_mut().insert(func_ptr as usize, 0);
        });
        return cached;
    }
    crate::promise::bump(&CLOSURE_CAP_SINGLETON_MISS);

    // Slow path: allocate, populate captures, insert into cache as
    // the most-recent entry. If the slot list is full, drop the
    // least-recent (back of the Vec).
    let allocated = js_closure_alloc(func_ptr, capture_count);
    if n > 0 && !captures_ptr.is_null() {
        unsafe {
            let dest = (allocated as *mut u8).add(std::mem::size_of::<ClosureHeader>()) as *mut u64;
            std::ptr::copy_nonoverlapping(captures_ptr, dest, n);
            crate::gc::layout_rebuild_from_slots(allocated as *mut u8, dest as *const u64, n);
        }
    }
    SINGLETON_CAPTURED_CLOSURES.with(|s| {
        let mut s = s.borrow_mut();
        let slots = s.entry(func_ptr as usize).or_insert_with(Vec::new);
        slots.insert(0, (captures_slice.to_vec(), allocated));
        if slots.len() > MAX_CAPTURED_CLOSURE_SLOTS {
            slots.truncate(MAX_CAPTURED_CLOSURE_SLOTS);
        }
    });
    // Bump the miss-streak counter; flip to disabled sentinel when we
    // hit the threshold.
    CAPTURED_MISS_STREAK.with(|m| {
        let mut m = m.borrow_mut();
        let entry = m.entry(func_ptr as usize).or_insert(0);
        if *entry < CAPTURED_DISABLED_SENTINEL - 1 {
            *entry += 1;
            if *entry >= CAPTURED_MISS_STREAK_DISABLE {
                *entry = CAPTURED_DISABLED_SENTINEL;
            }
        }
    });
    allocated
}

/// Get the function pointer from a closure
#[no_mangle]
pub extern "C" fn js_closure_get_func(closure: *const ClosureHeader) -> *const u8 {
    unsafe { (*closure).func_ptr }
}

/// Get a captured value (as f64) by index
#[no_mangle]
pub extern "C" fn js_closure_get_capture_f64(closure: *const ClosureHeader, index: u32) -> f64 {
    if closure.is_null() {
        return 0.0;
    }
    unsafe {
        let captures_ptr =
            (closure as *const u8).add(std::mem::size_of::<ClosureHeader>()) as *const f64;
        *captures_ptr.add(index as usize)
    }
}

/// Set a captured value (as f64) by index
#[no_mangle]
pub extern "C" fn js_closure_set_capture_f64(closure: *mut ClosureHeader, index: u32, value: f64) {
    if closure.is_null() {
        return;
    }
    unsafe {
        let captures_ptr =
            (closure as *mut u8).add(std::mem::size_of::<ClosureHeader>()) as *mut f64;
        *captures_ptr.add(index as usize) = value;
        crate::gc::layout_note_slot(closure as usize, index as usize, value.to_bits());
    }
}

/// Get a captured value (as i64 pointer) by index
#[no_mangle]
pub extern "C" fn js_closure_get_capture_ptr(closure: *const ClosureHeader, index: u32) -> i64 {
    if closure.is_null() {
        return 0;
    }
    unsafe {
        let captures_ptr =
            (closure as *const u8).add(std::mem::size_of::<ClosureHeader>()) as *const i64;
        *captures_ptr.add(index as usize)
    }
}

/// Set a captured value (as i64 pointer) by index
#[no_mangle]
pub extern "C" fn js_closure_set_capture_ptr(closure: *mut ClosureHeader, index: u32, value: i64) {
    if closure.is_null() {
        return;
    }
    unsafe {
        let captures_ptr =
            (closure as *mut u8).add(std::mem::size_of::<ClosureHeader>()) as *mut i64;
        *captures_ptr.add(index as usize) = value;
        crate::gc::layout_note_slot(closure as usize, index as usize, value as u64);
    }
}

/// Dispatch a bound method call with the given arguments.
/// Extracts the namespace object and method name from the closure captures,
/// then calls js_native_call_method with the packed arguments.
#[inline]
unsafe fn dispatch_bound_method(closure: *const ClosureHeader, args: &[f64]) -> f64 {
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
fn throw_not_callable() -> ! {
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
fn get_valid_func_ptr(closure: *const ClosureHeader) -> *const u8 {
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

use std::sync::{Mutex, OnceLock};

static CLOSURE_PROPS: OnceLock<Mutex<HashMap<usize, HashMap<String, f64>>>> = OnceLock::new();

fn get_closure_props() -> &'static Mutex<HashMap<usize, HashMap<String, f64>>> {
    CLOSURE_PROPS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Check if a raw pointer points to a ClosureHeader by checking CLOSURE_MAGIC at offset 12.
/// Safe to call with any non-null, sufficiently aligned pointer >= 0x10000.
pub fn is_closure_ptr(ptr: usize) -> bool {
    if ptr < 0x10000 {
        return false;
    }
    unsafe {
        let type_tag = *((ptr as *const u8).add(12) as *const u32);
        type_tag == CLOSURE_MAGIC
    }
}

/// Get a dynamic property stored on a closure.
/// Returns TAG_UNDEFINED if not found.
pub fn closure_get_dynamic_prop(ptr: usize, prop: &str) -> f64 {
    if let Ok(props) = get_closure_props().lock() {
        if let Some(closure_props) = props.get(&ptr) {
            if let Some(&val) = closure_props.get(prop) {
                return val;
            }
        }
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Set a dynamic property on a closure.
pub fn closure_set_dynamic_prop(ptr: usize, prop: &str, value: f64) {
    if let Ok(mut props) = get_closure_props().lock() {
        props
            .entry(ptr)
            .or_insert_with(HashMap::new)
            .insert(prop.to_string(), value);
    }
}

/// Snapshot every dynamic property on a closure as `(name, value)` pairs.
/// Sorted alphabetically for stable output (`HashMap` iteration order is
/// non-deterministic). Used by `format_jsvalue` to emit `[Function: f]
/// { ownProp: value }` for functions with user-attached properties. See
/// #1203.
pub fn closure_dynamic_props_snapshot(ptr: usize) -> Vec<(String, f64)> {
    if let Ok(props) = get_closure_props().lock() {
        if let Some(map) = props.get(&ptr) {
            let mut out: Vec<(String, f64)> = map.iter().map(|(k, v)| (k.clone(), *v)).collect();
            out.sort_by(|a, b| a.0.cmp(&b.0));
            return out;
        }
    }
    Vec::new()
}

/// Unbind `this` from a detached method closure.
///
/// When a method is read from an object via PropertyGet (e.g., `const fn = holder.getX`),
/// this function is called on the result. If the value is a closure whose capture_count
/// has CAPTURES_THIS_FLAG set (indicating slot 0 is `this`), it allocates a new closure
/// with the same func_ptr and captures but slot 0 set to undefined.
///
/// For non-closure values (numbers, strings, objects, arrays), this is a no-op.
#[no_mangle]
pub extern "C" fn js_closure_unbind_this(val: f64) -> f64 {
    let bits = val.to_bits();
    let tag = bits & 0xFFFF_0000_0000_0000;
    // Only process POINTER_TAG values (closures are NaN-boxed with POINTER_TAG)
    if tag != 0x7FFD_0000_0000_0000 {
        return val;
    }
    let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
    if ptr < 0x10000 {
        return val;
    }
    // Check CLOSURE_MAGIC
    unsafe {
        let type_tag = *((ptr as *const u8).add(12) as *const u32);
        if type_tag != CLOSURE_MAGIC {
            return val;
        }
        let header = ptr as *const ClosureHeader;
        let raw_count = (*header).capture_count;
        // Only unbind if the closure has the CAPTURES_THIS_FLAG
        if raw_count & CAPTURES_THIS_FLAG == 0 {
            return val;
        }
        let count = real_capture_count(raw_count) as usize;
        if count == 0 {
            return val;
        }
        // Clone the closure with slot 0 set to undefined
        let new_closure = js_closure_alloc((*header).func_ptr, raw_count);
        let src_captures =
            (ptr as *const u8).add(std::mem::size_of::<ClosureHeader>()) as *const f64;
        let dst_captures =
            (new_closure as *mut u8).add(std::mem::size_of::<ClosureHeader>()) as *mut f64;
        // Set slot 0 to undefined
        *dst_captures = f64::from_bits(crate::value::TAG_UNDEFINED);
        // Copy remaining captures (slots 1..count)
        for i in 1..count {
            *dst_captures.add(i) = *src_captures.add(i);
        }
        crate::gc::layout_rebuild_from_slots(
            new_closure as *mut u8,
            dst_captures as *const u64,
            count,
        );
        // NaN-box the new closure pointer
        let new_ptr = new_closure as u64;
        f64::from_bits(0x7FFD_0000_0000_0000 | (new_ptr & 0x0000_FFFF_FFFF_FFFF))
    }
}

/// Issue #450: clone an accessor closure (from `Object.defineProperty(obj, k, { get, set })`)
/// and patch its reserved `this` slot with `recv_box` (the NaN-boxed target object pointer).
///
/// The user's descriptor object literal's `{ get() {...}, set() {...} }` methods are codegen'd
/// with `captures_this: true` — at object-literal construction the codegen patches their
/// reserved `this` slot to point to the *descriptor* object. But spec says the getter/setter
/// runs with `this === obj` (the property access target, NOT the descriptor). So we clone
/// the closure once at defineProperty time and rebind `this` to `obj`. The original
/// descriptor closure is untouched (in case the user reuses it).
///
/// `closure_bits` is the NaN-boxed closure value (POINTER_TAG | ptr); `recv_box` is the
/// NaN-boxed target receiver (POINTER_TAG | obj). Returns the new closure as NaN-boxed bits,
/// or returns `closure_bits` unchanged if the input isn't a CAPTURES_THIS closure.
///
/// Reserved `this` slot index is `auto_captures.len()` per the codegen convention
/// (`crates/perry-codegen/src/expr.rs::lower_object_literal` and
/// `crates/perry-runtime/src/symbol.rs::js_object_set_symbol_method` — both use the LAST
/// capture slot, i.e. `real_count - 1`, as the `this` slot for `captures_this` closures).
pub(crate) fn clone_closure_rebind_this(closure_bits: u64, recv_box: f64) -> u64 {
    let tag = closure_bits & 0xFFFF_0000_0000_0000;
    if tag != 0x7FFD_0000_0000_0000 {
        return closure_bits;
    }
    let ptr = (closure_bits & 0x0000_FFFF_FFFF_FFFF) as usize;
    if ptr < 0x10000 {
        return closure_bits;
    }
    unsafe {
        let type_tag = std::ptr::read_volatile((ptr as *const u8).add(12) as *const u32);
        if type_tag != CLOSURE_MAGIC {
            return closure_bits;
        }
        let header = ptr as *const ClosureHeader;
        let raw_count = (*header).capture_count;
        // No CAPTURES_THIS_FLAG → the closure body doesn't read `this`, no rebind needed.
        if raw_count & CAPTURES_THIS_FLAG == 0 {
            return closure_bits;
        }
        let count = real_capture_count(raw_count) as usize;
        if count == 0 {
            return closure_bits;
        }
        // Allocate a fresh closure with the same func_ptr + capture_count (preserving the flag).
        let new_closure = js_closure_alloc((*header).func_ptr, raw_count);
        let src_captures =
            (ptr as *const u8).add(std::mem::size_of::<ClosureHeader>()) as *const f64;
        let dst_captures =
            (new_closure as *mut u8).add(std::mem::size_of::<ClosureHeader>()) as *mut f64;
        // Copy every capture verbatim, then overwrite the `this` slot (last) with recv_box.
        for i in 0..count {
            *dst_captures.add(i) = *src_captures.add(i);
        }
        let this_slot = count - 1;
        *dst_captures.add(this_slot) = recv_box;
        crate::gc::layout_rebuild_from_slots(
            new_closure as *mut u8,
            dst_captures as *const u64,
            count,
        );
        let new_ptr = new_closure as u64;
        0x7FFD_0000_0000_0000 | (new_ptr & 0x0000_FFFF_FFFF_FFFF)
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

// V8 interop no-op stubs. Real implementations are in perry-jsruntime/src/interop.rs.
// These stubs ensure symbols are always available even when perry-jsruntime is not linked
// (iOS, Android, standalone builds). When perry-jsruntime IS linked, its strong symbols
// override these stubs via linker symbol resolution order.
//
// Signatures must match `crates/perry-codegen/src/runtime_decls.rs` exactly — the codegen
// declarations determine which register the caller reads the result from (rax/x0 for I64,
// xmm0/d0 for DOUBLE). A signature mismatch reads garbage and silently miscompiles.
//
// Stubs return NaN-boxed `TAG_UNDEFINED` (not 0.0) so when V8 isn't linked, downstream
// `typeof` correctly observes `undefined` instead of `"number"` — making the missing-V8
// case diagnostically distinct from a successful 0-returning JS call.
//
// On macOS (Mach-O) the stubs are emitted as **weak** symbols via `global_asm!` so
// perry-jsruntime's strong impls always win, regardless of linker archive scan order.
// Pre-fix, when user code only referenced FFIs that have stubs (e.g. `js_load_module` +
// `js_call_function`, but NOT `js_call_method`), the linker resolved those symbols against
// closure.o and never pulled `interop.o` from libperry_jsruntime.a — yielding a runtime
// that links V8 nowhere and silently returns undefined for every JS call. The weak
// attribute forces the linker to keep looking past closure.o's defs and pull in interop.o
// when jsruntime.a is on the command line. (Issue #257.)
//
// On other platforms (Linux, iOS, Android, Windows), Rust functions remain — Linux's
// linker handles duplicate-defs via link order (jsruntime is listed first in link.rs);
// iOS/Android/Windows don't link jsruntime at all (see compile.rs:2877), so the stubs
// are the only defs and behave as runtime-only no-ops.

const _UNDEF_BITS: u64 = crate::value::TAG_UNDEFINED;

// On Mach-O arm64, emit weak symbol stubs that return NaN-boxed TAG_UNDEFINED
// (0x7FFC_0000_0000_0001) for f64-returning FFIs, 0 for i64-returning,
// nothing for void. .weak_definition tells ld64 to treat this as a weak
// symbol so a strong def from libperry_jsruntime.a wins regardless of
// archive scan order.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
core::arch::global_asm!(
    // js_load_module(i64, i64) -> i64 ;  return 0 (handle 0 = invalid)
    ".globl _js_load_module",
    ".weak_definition _js_load_module",
    ".p2align 2",
    "_js_load_module:",
    "    mov x0, xzr",
    "    ret",
    // js_call_function(i64, i64, i64, i64, i64) -> f64 ;  return TAG_UNDEFINED
    ".globl _js_call_function",
    ".weak_definition _js_call_function",
    ".p2align 2",
    "_js_call_function:",
    "    mov x0, #1",
    "    movk x0, #0x7FFC, lsl #48",
    "    fmov d0, x0",
    "    ret",
    // js_get_export(i64, i64, i64) -> f64
    ".globl _js_get_export",
    ".weak_definition _js_get_export",
    ".p2align 2",
    "_js_get_export:",
    "    mov x0, #1",
    "    movk x0, #0x7FFC, lsl #48",
    "    fmov d0, x0",
    "    ret",
    // js_set_property(f64, i64, i64, f64) -> void
    ".globl _js_set_property",
    ".weak_definition _js_set_property",
    ".p2align 2",
    "_js_set_property:",
    "    ret",
    // js_runtime_init() -> void
    ".globl _js_runtime_init",
    ".weak_definition _js_runtime_init",
    ".p2align 2",
    "_js_runtime_init:",
    "    ret",
    // js_new_from_handle(f64, i64, i64) -> f64
    ".globl _js_new_from_handle",
    ".weak_definition _js_new_from_handle",
    ".p2align 2",
    "_js_new_from_handle:",
    "    mov x0, #1",
    "    movk x0, #0x7FFC, lsl #48",
    "    fmov d0, x0",
    "    ret",
    // js_new_instance(i64, i64, i64, i64, i64) -> f64
    ".globl _js_new_instance",
    ".weak_definition _js_new_instance",
    ".p2align 2",
    "_js_new_instance:",
    "    mov x0, #1",
    "    movk x0, #0x7FFC, lsl #48",
    "    fmov d0, x0",
    "    ret",
    // js_create_callback(i64, i64, i64) -> f64
    ".globl _js_create_callback",
    ".weak_definition _js_create_callback",
    ".p2align 2",
    "_js_create_callback:",
    "    mov x0, #1",
    "    movk x0, #0x7FFC, lsl #48",
    "    fmov d0, x0",
    "    ret",
    // js_await_js_promise(f64) -> f64
    ".globl _js_await_js_promise",
    ".weak_definition _js_await_js_promise",
    ".p2align 2",
    "_js_await_js_promise:",
    "    mov x0, #1",
    "    movk x0, #0x7FFC, lsl #48",
    "    fmov d0, x0",
    "    ret",
);

// macOS x86_64: same idea, x86_64 SysV ABI returns f64 in xmm0, i64 in rax.
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
core::arch::global_asm!(
    ".globl _js_load_module",
    ".weak_definition _js_load_module",
    "_js_load_module:",
    "    xor eax, eax",
    "    ret",
    ".globl _js_call_function",
    ".weak_definition _js_call_function",
    "_js_call_function:",
    "    movabs rax, 0x7FFC000000000001",
    "    movq xmm0, rax",
    "    ret",
    ".globl _js_get_export",
    ".weak_definition _js_get_export",
    "_js_get_export:",
    "    movabs rax, 0x7FFC000000000001",
    "    movq xmm0, rax",
    "    ret",
    ".globl _js_set_property",
    ".weak_definition _js_set_property",
    "_js_set_property:",
    "    ret",
    ".globl _js_runtime_init",
    ".weak_definition _js_runtime_init",
    "_js_runtime_init:",
    "    ret",
    ".globl _js_new_from_handle",
    ".weak_definition _js_new_from_handle",
    "_js_new_from_handle:",
    "    movabs rax, 0x7FFC000000000001",
    "    movq xmm0, rax",
    "    ret",
    ".globl _js_new_instance",
    ".weak_definition _js_new_instance",
    "_js_new_instance:",
    "    movabs rax, 0x7FFC000000000001",
    "    movq xmm0, rax",
    "    ret",
    ".globl _js_create_callback",
    ".weak_definition _js_create_callback",
    "_js_create_callback:",
    "    movabs rax, 0x7FFC000000000001",
    "    movq xmm0, rax",
    "    ret",
    ".globl _js_await_js_promise",
    ".weak_definition _js_await_js_promise",
    "_js_await_js_promise:",
    "    movabs rax, 0x7FFC000000000001",
    "    movq xmm0, rax",
    "    ret",
);

// Non-macOS platforms: plain Rust stubs. Signatures match codegen declarations
// in `crates/perry-codegen/src/runtime_decls.rs` (caller register
// agreement). Returns TAG_UNDEFINED for f64 returns, 0 for i64 returns.
#[cfg(not(target_os = "macos"))]
#[no_mangle]
pub extern "C" fn js_load_module(_path_ptr: i64, _path_len: i64) -> i64 {
    0
}

#[cfg(not(target_os = "macos"))]
#[no_mangle]
pub extern "C" fn js_call_function(
    _module_handle: i64,
    _name_ptr: i64,
    _name_len: i64,
    _args_ptr: i64,
    _args_len: i64,
) -> f64 {
    f64::from_bits(_UNDEF_BITS)
}

#[cfg(not(target_os = "macos"))]
#[no_mangle]
pub extern "C" fn js_get_export(_module: i64, _name_ptr: i64, _name_len: i64) -> f64 {
    f64::from_bits(_UNDEF_BITS)
}

#[cfg(not(target_os = "macos"))]
#[no_mangle]
pub extern "C" fn js_set_property(_obj: f64, _key_ptr: i64, _key_len: i64, _value: f64) {}

#[cfg(not(target_os = "macos"))]
#[no_mangle]
pub extern "C" fn js_runtime_init() {}

#[cfg(not(target_os = "macos"))]
#[no_mangle]
pub extern "C" fn js_new_from_handle(_constructor: f64, _args_ptr: i64, _args_len: i64) -> f64 {
    f64::from_bits(_UNDEF_BITS)
}

#[cfg(not(target_os = "macos"))]
#[no_mangle]
pub extern "C" fn js_new_instance(
    _class_ptr: i64,
    _name_ptr: i64,
    _name_len: i64,
    _args_ptr: i64,
    _args_len: i64,
) -> f64 {
    f64::from_bits(_UNDEF_BITS)
}

#[cfg(not(target_os = "macos"))]
#[no_mangle]
pub extern "C" fn js_create_callback(_func_ptr: i64, _closure_env: i64, _param_count: i64) -> f64 {
    f64::from_bits(_UNDEF_BITS)
}

#[cfg(not(target_os = "macos"))]
#[no_mangle]
pub extern "C" fn js_await_js_promise(_promise: f64) -> f64 {
    f64::from_bits(_UNDEF_BITS)
}

// =============================================================================
// AOT stubs for unconditionally-declared extern functions
// =============================================================================

#[no_mangle]
pub extern "C" fn js_ratelimit_create() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_ends_with() -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn js_lodash_escape() -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn js_lodash_includes() -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn js_lodash_lower_first() -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn js_lodash_replace() -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn js_lodash_split() -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn js_lodash_start_case() -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn js_lodash_starts_with() -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn js_lodash_unescape() -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn js_lodash_upper_first() -> f64 {
    0.0
}
#[no_mangle]
pub extern "C" fn js_axios_create() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_axios_request() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_argon2_hash_options() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_sharp_negate() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_sharp_quality() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_sharp_to_format() -> i64 {
    0
}
// js_sqlite_transaction / _commit / _rollback stubs removed — the real
// implementations live in perry-stdlib/src/sqlite.rs and would collide at
// link time when both crates are present (e.g. `cargo test --workspace`).
#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn test_closure_func(closure: *const ClosureHeader) -> f64 {
        unsafe {
            let captured = js_closure_get_capture_f64(closure, 0);
            captured * 2.0
        }
    }

    #[test]
    fn test_closure_basic() {
        let closure = js_closure_alloc(test_closure_func as *const u8, 1);
        js_closure_set_capture_f64(closure, 0, 21.0);
        let result = js_closure_call0(closure);
        assert_eq!(result, 42.0);
    }
}
