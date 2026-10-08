use super::*;
use crate::JSValue;
use std::sync::atomic::{AtomicU64, Ordering};

// ============================================================================
// Class-method calls through the vtable's function pointers (constructors,
// private and symbol-keyed members, the method-value trampoline), and the
// registration generation the store-plan cache keys on.
//
// A by-name call of a class instance's string-keyed method is answered by its
// prototype chain's shapes (`native_call_method::class_holder`), not here: the
// per-(class, name) caches this module used to keep for it are gone.
// ============================================================================

pub(crate) static VTABLE_GEN: AtomicU64 = AtomicU64::new(1);

/// Current vtable generation — consumed by caches (the store-plan cache in
/// `object::prop_plan`) that must invalidate on any class
/// registration/mutation.
#[inline]
pub(crate) fn vtable_generation() -> u64 {
    VTABLE_GEN.load(Ordering::Relaxed)
}

#[cfg(test)]
pub(crate) fn test_bump_vtable_generation() {
    VTABLE_GEN.fetch_add(1, Ordering::Release);
}

/// Generation counter for the class-registry lookup surfaces that
/// [`VTABLE_GEN`] deliberately does NOT cover.
///
/// `VTABLE_GEN` tracks method/getter/setter REGISTRATION. Four other writes
/// change what a class-chain walk would ANSWER without touching a vtable, and
/// none of them may bump `VTABLE_GEN`: materializing a declared class's
/// prototype OBJECT is explicitly documented as a dispatch deoptimization to
/// avoid (`class_registry/state.rs`, #7769 — "384,000 of 384,000 shape-guard
/// probes failed here"). They are:
///
/// * `class_prototype_object_root_store` — NULL to a real
///   `CLASS_PROTOTYPE_OBJECTS` entry (a reflective `F.prototype` read,
///   `Object.create`, the lazy builtin prototype installers);
/// * `class_decl_prototype_object_root_store` — NULL to a real
///   `CLASS_DECL_PROTOTYPE_OBJECTS` entry (any `C.prototype`, `instanceof`,
///   `Object.getPrototypeOf(instance)`, a `super` chain);
/// * `js_register_class_generic_origin` — redirects BOTH prototype-object
///   readers and the ordinary prototype read's chain hop to another class id;
///
/// Bumped INSIDE those three writers, after the store, so a new call site
/// cannot forget it — the same enforced-funnel rule `prop_plan_epoch_bump`
/// follows. Kept separate from `VTABLE_GEN` precisely so that a consumer of
/// this counter does not impose the dispatch-speculation cost that bumping
/// `VTABLE_GEN` in those writers would.
///
/// Garbage collection is NOT an input: the class side-table scanners
/// (`object/class_gc_roots.rs`, `class_registry/gc_roots.rs`) only rewrite
/// EXISTING slots, so no collection can add a registry key, and the
/// dead-owner prune only removes entries. Keying a hot cache on a GC-bumped
/// counter is a measured performance CLIFF, not merely waste — see
/// `object::prop_plan`'s module docs (#7910).
///
/// First consumer: the per-`class_id` `toJSON` verdict memo in
/// `json::stringify_tojson_probe` (#10696).
pub(crate) static CLASS_LOOKUP_SURFACE_GEN: AtomicU64 = AtomicU64::new(1);

/// Current class lookup-surface generation — see [`CLASS_LOOKUP_SURFACE_GEN`].
#[inline]
pub(crate) fn class_lookup_surface_generation() -> u64 {
    CLASS_LOOKUP_SURFACE_GEN.load(Ordering::Relaxed)
}

/// Invalidate every cache keyed on [`CLASS_LOOKUP_SURFACE_GEN`]. One relaxed
/// add; every caller is a one-shot-per-class materializer or a `delete`
/// recovery path.
#[inline]
pub(crate) fn class_lookup_surface_gen_bump() {
    CLASS_LOOKUP_SURFACE_GEN.fetch_add(1, Ordering::Release);
    // An inherited-read entry whose chain was resolved through
    // `class_prototype_object` names the object that registry held AT PRIME
    // TIME. Replacing the registration leaves the receiver's class id, ShapeId
    // and recorded prototype bits all unchanged and the old prototype object
    // unmutated, so nothing else in that entry's guard can see it — and the
    // entry would then answer with a different object than the chain walk
    // beside it. All three callers are registry stores on cold paths.
    crate::object::proto_validity::bump_proto_validity();
}

/// Maximum positional arity `call_vtable_method` can invoke directly. The
/// dispatch builds a fixed-arity `extern "C"` fn signature for each arity up to
/// this cap (see `vtable_call_dispatch!`). Synthesized capture-stashing
/// constructors (`synthesize_class_captures`) append one `__perry_cap_*` param
/// per captured outer local; a giant minified bundle module (Next.js
/// app-route-turbo's `rJ` route-module class) can capture 130+ IIFE-scope
/// locals, so the cap must comfortably exceed that. Before #5437 the dispatch
/// topped out at 64 and silently transmuted a 135-param ctor to a 64-arg
/// signature in release builds (the `debug_assert!` was compiled out) — every
/// param past the 64th received register/stack garbage, so a captured function
/// (`r_`/`rQ`) arrived as a non-callable and `this.methods = r_(e)` threw
/// "value is not a function", aborting Next route-module init → HTTP 500.
pub(crate) const MAX_VTABLE_DISPATCH_ARITY: usize = 512;

/// Call a `double(double this, double, …, double)` function pointer with `this`
/// plus `nargs` f64 arguments read from `args` (missing slots → `undefined`),
/// for an arbitrary `nargs` (bounded by [`MAX_VTABLE_DISPATCH_ARITY`]).
///
/// The dynamic vtable path can't form an arbitrary-arity Rust `fn` type at
/// runtime, and hand-writing a `match` arm per arity caps out (the pre-#5437
/// 64-arm cap silently mis-called 130+-param synthesized capture ctors). This
/// uses a tiny architecture-specific trampoline: f64 args go in the FP argument
/// registers (first 8) with the remainder spilled to the stack per the platform
/// C ABI, exactly as a native call of that arity would. All Perry-generated
/// method/ctor params are `f64`, so an all-f64 calling convention is faithful.
///
/// #7769: the argument vector is built ONCE, in a stack buffer for the arities
/// that actually occur. This used to be two `Vec<f64>` allocations per dynamic
/// method call (`positional` in [`call_vtable_method`], then `all` here) —
/// i.e. two `malloc`/`free` round-trips for a zero-argument virtual call such
/// as `shape.area()`. On `gc-handoff/apps/shapes.ts` (360 k virtual calls)
/// that was pure overhead against a call that does one multiply.
const INLINE_DISPATCH_ARGS: usize = 24;

/// Invoke `func_ptr` with `this_f64` followed by `param_count` positional
/// arguments read from `args` (missing trailing slots → `undefined`).
#[inline]
unsafe fn call_fn_with_this_and_args(
    func_ptr: usize,
    this_f64: f64,
    args_ptr: *const f64,
    args_len: usize,
    param_count: usize,
) -> f64 {
    debug_assert!(param_count <= MAX_VTABLE_DISPATCH_ARITY);
    let total = param_count + 1;
    let mut inline_buf = [0.0f64; INLINE_DISPATCH_ARGS + 1];
    let mut heap_buf: Vec<f64>;
    let all: &[f64] = if total <= INLINE_DISPATCH_ARGS + 1 {
        inline_buf[0] = this_f64;
        for (i, slot) in inline_buf[1..total].iter_mut().enumerate() {
            *slot = arg_or_undefined(args_ptr, args_len, i);
        }
        &inline_buf[..total]
    } else {
        heap_buf = Vec::with_capacity(total);
        heap_buf.push(this_f64);
        for i in 0..param_count {
            heap_buf.push(arg_or_undefined(args_ptr, args_len, i));
        }
        &heap_buf[..]
    };
    crate::abi_trampoline::call_all_f64(func_ptr, all)
}

/// A missing trailing argument is `undefined` per spec (NOT NaN): default
/// parameters lower to a `param === undefined ? <default> : param` check in
/// the method prologue, so padding a hole with NaN left the default
/// un-applied (`async method(a, b, c = 99)` called via the dynamic vtable
/// path — e.g. a detached `C.prototype.method` value — saw `c = NaN`). Pad
/// with TAG_UNDEFINED so the prologue's default-check fires.
#[inline(always)]
unsafe fn arg_or_undefined(args_ptr: *const f64, args_len: usize, idx: usize) -> f64 {
    if idx < args_len && !args_ptr.is_null() {
        *args_ptr.add(idx)
    } else {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    }
}

/// Call a vtable method with the correct arity.
/// All method params are f64, `this` is i64.
pub(crate) unsafe fn call_vtable_method(
    func_ptr: usize,
    this: i64,
    args_ptr: *const f64,
    args_len: usize,
    param_count: u32,
    has_synthetic_arguments: bool,
    has_rest: bool,
) -> f64 {
    call_vtable_method_value(
        func_ptr,
        legacy_method_receiver(this),
        args_ptr,
        args_len,
        param_count,
        has_synthetic_arguments,
        has_rest,
        None,
    )
}

pub(crate) unsafe fn call_vtable_method_with_private_brand(
    func_ptr: usize,
    this: i64,
    args_ptr: *const f64,
    args_len: usize,
    param_count: u32,
    has_synthetic_arguments: bool,
    has_rest: bool,
    private_brand: f64,
) -> f64 {
    call_vtable_method_value(
        func_ptr,
        legacy_method_receiver(this),
        args_ptr,
        args_len,
        param_count,
        has_synthetic_arguments,
        has_rest,
        Some(private_brand),
    )
}

/// Convert the legacy dispatch ABI, which accepts raw object pointers or tagged values.
fn legacy_method_receiver(this: i64) -> f64 {
    let bits = this as u64;
    if bits != 0 && bits <= crate::value::POINTER_MASK {
        f64::from_bits(JSValue::pointer(bits as *mut u8).bits())
    } else {
        f64::from_bits(bits)
    }
}

/// Dispatch with an already boxed JavaScript receiver. In particular, positive
/// subnormal numbers must not be mistaken for legacy raw object pointers.
pub(crate) unsafe fn call_vtable_method_value(
    func_ptr: usize,
    this_f64: f64,
    args_ptr: *const f64,
    args_len: usize,
    param_count: u32,
    has_synthetic_arguments: bool,
    has_rest: bool,
    explicit_private_brand: Option<f64>,
) -> f64 {
    // A trailing param that is either the synthesized `arguments` object or a
    // user rest param (`method(a, ...rest)`) needs the call-site args bundled
    // into a JS array for that slot. Without this, an apply/dynamic dispatch
    // (`recv.method(...spread)` via `js_native_call_method_apply`) passes the
    // raw individual args and the callee reads `rest = args[0]` as a scalar —
    // marked's `new Marked()` -> `this.use(...e)` hit exactly this, throwing
    // `(number).forEach is not a function`. The synthesized-`arguments` slot
    // holds ALL passed args; a user rest slot holds only args from the rest
    // position onward (so `method(a, ...rest)` keeps `a` positional).
    // Root the receiver and supplied arguments before allocating either
    // trailing array. Handles are re-read after a copying collection.
    let dispatch_scope = crate::gc::RuntimeHandleScope::new();
    let needs_packed_args = has_synthetic_arguments || has_rest;
    let this_handle = needs_packed_args.then(|| dispatch_scope.root_nanbox_f64(this_f64));
    let explicit_private_brand_handle = if needs_packed_args {
        explicit_private_brand.map(|value| dispatch_scope.root_nanbox_f64(value))
    } else {
        None
    };

    // A user rest parameter and the hidden `arguments` parameter are distinct
    // ABI slots. A method containing both lowers as
    // `[fixed..., user_rest, synthetic_arguments]`: the first array contains
    // only the tail after the fixed formals, while the second contains every
    // supplied argument. Treating the flags as mutually exclusive bound the
    // first scalar argument directly to `user_rest`.
    let adjusted_args_storage: Option<Vec<f64>>;
    let (call_args_ptr, call_args_len) = if needs_packed_args {
        let supplied_args: Vec<f64> = (0..args_len)
            .map(|i| arg_or_undefined(args_ptr, args_len, i))
            .collect();
        let supplied_arg_handles = dispatch_scope.root_nanbox_f64_slice(&supplied_args);
        let trailing_slots = usize::from(has_rest) + usize::from(has_synthetic_arguments);
        let fixed_params = (param_count as usize).saturating_sub(trailing_slots);

        let user_rest_handle = if has_rest {
            let refreshed =
                crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&supplied_arg_handles);
            let rest_start = fixed_params.min(refreshed.len());
            Some(
                dispatch_scope.root_nanbox_f64(crate::closure::build_rest_array(
                    &refreshed[rest_start..],
                    false,
                )),
            )
        } else {
            None
        };

        let synthetic_arguments_handle = if has_synthetic_arguments {
            let refreshed =
                crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&supplied_arg_handles);
            Some(dispatch_scope.root_nanbox_f64(crate::closure::build_rest_array(&refreshed, true)))
        } else {
            None
        };

        let mut args = Vec::with_capacity(param_count as usize);
        for i in 0..fixed_params {
            args.push(
                supplied_arg_handles
                    .get(i)
                    .map(|handle| handle.get_nanbox_f64())
                    .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED)),
            );
        }
        if let Some(handle) = user_rest_handle.as_ref() {
            args.push(handle.get_nanbox_f64());
        }
        if let Some(handle) = synthetic_arguments_handle.as_ref() {
            args.push(handle.get_nanbox_f64());
        }
        adjusted_args_storage = Some(args);
        let adjusted_args = adjusted_args_storage.as_ref().unwrap();
        (adjusted_args.as_ptr(), adjusted_args.len())
    } else {
        (args_ptr, args_len)
    };

    // All Perry method/ctor params are `f64`. Build the positional arg list
    // (missing trailing args → `undefined` per spec) and invoke through the
    // arbitrary-arity all-f64 trampoline. A fixed `match`-arm-per-arity dispatch
    // previously capped at 64 and silently mis-called 130+-param synthesized
    // capture constructors (#5437).
    // REAL runtime guard (all builds, not just debug): reject any arity past the
    // dispatch cap BEFORE building the positional vec and invoking the
    // trampoline. A `debug_assert!` alone is compiled out in release — exactly
    // the bug class behind the original 64-cap miscompile (#5437), where an
    // over-cap arity silently mis-called the fn pointer in release builds. Fail
    // closed with a clear panic instead.
    let param_count_usize = param_count as usize;
    assert!(
        param_count_usize <= MAX_VTABLE_DISPATCH_ARITY,
        "call_vtable_method: param_count {} exceeds MAX_VTABLE_DISPATCH_ARITY ({})",
        param_count,
        MAX_VTABLE_DISPATCH_ARITY
    );
    let this_f64 = this_handle
        .as_ref()
        .map(|handle| handle.get_nanbox_f64())
        .unwrap_or(this_f64);
    let private_brand = explicit_private_brand_handle
        .as_ref()
        .map(|handle| handle.get_nanbox_f64())
        .or(explicit_private_brand)
        .or_else(|| crate::object::private_evaluation_brand_value(this_f64))
        .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
    let derived_super_depth = crate::object::derived_super_binding_stack_savepoint();
    crate::object::private_lexical_brand_push(private_brand);
    let result = call_fn_with_this_and_args(
        func_ptr,
        this_f64,
        call_args_ptr,
        call_args_len,
        param_count_usize,
    );
    crate::object::private_lexical_brand_pop();
    crate::object::derived_super_binding_stack_restore(derived_super_depth);
    result
}

/// Walk the class parent chain looking for a recorded fetch-builtin parent
/// (Request = 1, Response = 2). Returns the kind for the first ancestor (incl.
/// `class_id` itself) that directly extends a global Request/Response.
pub(crate) fn fetch_parent_kind_in_chain(class_id: u32) -> Option<u8> {
    let mut cid = class_id;
    let mut depth = 0u32;
    while depth < 32 {
        if let Some(kind) = super::super::fetch_parent_kind(cid) {
            return Some(kind);
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    None
}
