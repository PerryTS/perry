use super::*;
use crate::JSValue;
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU64, Ordering};

// ============================================================================
// Per-callsite-keyed inline cache for vtable method dispatch.
//
// `js_native_call_method` is the hot dispatch tower for cross-module class
// instance method calls (e.g. `archetype.set(...)` from CommandBuffer.execute
// in the ECS workloads). Per profile, ~12% of perf-comprehensive samples land
// in `core::hash::BuildHasher` from the per-call `HashMap.get(method_name)`
// SipHash on the vtable lookup.
//
// Cache key: `(class_id, method_name_ptr)` where `method_name_ptr` is the
// rodata byte-pointer perry-codegen passes for the interned method name. The
// pointer is stable across calls within a module, so its address acts as a
// faster identity than re-hashing the bytes. Different modules may produce
// different rodata copies of the same name — the cache simply gets one entry
// per (class_id, name_pointer) pair, no correctness impact.
//
// Invalidation: a global `VTABLE_GEN` atomic is bumped on every
// `js_register_class_method` / `js_register_class_getter`. Each cache entry
// records the gen at populate time; lookups skip stale entries. Registration
// is one-shot at init in practice, so steady-state lookups never miss on
// gen.
// ============================================================================

pub(crate) static VTABLE_GEN: AtomicU64 = AtomicU64::new(1);

/// Current vtable generation — consumed by caches (method IC below, the
/// store-plan cache in `object::prop_plan`) that must invalidate on any
/// class registration/mutation.
#[inline]
pub(crate) fn vtable_generation() -> u64 {
    VTABLE_GEN.load(Ordering::Relaxed)
}

#[cfg(test)]
pub(crate) fn test_bump_vtable_generation() {
    VTABLE_GEN.fetch_add(1, Ordering::Release);
}

const VTABLE_IC_SIZE: usize = 4096;
const VTABLE_IC_MASK: usize = VTABLE_IC_SIZE - 1;

#[repr(C)]
#[derive(Copy, Clone)]
struct VTableICEntry {
    gen: u64,
    class_id: u32,
    _pad: u32,
    method_name_ptr: usize,
    func_ptr: usize,
    param_count: u32,
    has_synthetic_arguments: u32,
    has_rest: u32,
}

const EMPTY_VTABLE_IC_ENTRY: VTableICEntry = VTableICEntry {
    gen: 0,
    class_id: 0,
    _pad: 0,
    method_name_ptr: 0,
    func_ptr: 0,
    param_count: 0,
    has_synthetic_arguments: 0,
    has_rest: 0,
};

thread_local! {
    // arm64_32 fix: HEAP-allocate (Box) this ~160KB cache instead of inline TLS.
    // Oversized `#[thread_local]` storage overflows the ILP32 TLS layout and its
    // writes corrupt adjacent thread-locals. Boxing keeps only a pointer in TLS.
    static VTABLE_IC: UnsafeCell<Box<[VTableICEntry]>> =
        UnsafeCell::new(vec![EMPTY_VTABLE_IC_ENTRY; VTABLE_IC_SIZE].into_boxed_slice());
}

#[inline(always)]
fn vtable_ic_slot(class_id: u32, method_name_ptr: usize) -> usize {
    // Mix class_id into the upper bits of the pointer to spread (class, name)
    // pairs across slots. method_name_ptr is at least 1-byte aligned but
    // typically 8+ for rodata strings, so shift by 3 to drop the alignment
    // zeros before masking.
    let key = method_name_ptr
        .rotate_left(13)
        .wrapping_add((class_id as usize).wrapping_mul(0x9E37_79B9));
    (key >> 3) & VTABLE_IC_MASK
}

#[inline(always)]
pub(crate) unsafe fn vtable_ic_lookup(
    class_id: u32,
    method_name_ptr: usize,
) -> Option<(usize, u32, bool, bool)> {
    if method_name_ptr == 0 {
        return None;
    }
    let cur_gen = VTABLE_GEN.load(Ordering::Relaxed);
    let slot = vtable_ic_slot(class_id, method_name_ptr);
    VTABLE_IC.with(|cell| {
        let cache = &**cell.get();
        let entry = &cache[slot];
        if entry.gen == cur_gen
            && entry.class_id == class_id
            && entry.method_name_ptr == method_name_ptr
        {
            Some((
                entry.func_ptr,
                entry.param_count,
                entry.has_synthetic_arguments != 0,
                entry.has_rest != 0,
            ))
        } else {
            None
        }
    })
}

#[inline(always)]
pub(crate) unsafe fn vtable_ic_insert(
    class_id: u32,
    method_name_ptr: usize,
    func_ptr: usize,
    param_count: u32,
    has_synthetic_arguments: bool,
    has_rest: bool,
) {
    if method_name_ptr == 0 {
        return;
    }
    let cur_gen = VTABLE_GEN.load(Ordering::Relaxed);
    let slot = vtable_ic_slot(class_id, method_name_ptr);
    VTABLE_IC.with(|cell| {
        let cache = &mut **cell.get();
        cache[slot] = VTableICEntry {
            gen: cur_gen,
            class_id,
            _pad: 0,
            method_name_ptr,
            func_ptr,
            param_count,
            has_synthetic_arguments: if has_synthetic_arguments { 1 } else { 0 },
            has_rest: if has_rest { 1 } else { 0 },
        };
    });
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
#[inline]
unsafe fn call_fn_with_f64_args(func_ptr: usize, this_f64: f64, args: &[f64]) -> f64 {
    debug_assert!(args.len() <= MAX_VTABLE_DISPATCH_ARITY);
    // Build the full argument vector: `this` followed by the positional args.
    let mut all: Vec<f64> = Vec::with_capacity(args.len() + 1);
    all.push(this_f64);
    all.extend_from_slice(args);
    crate::abi_trampoline::call_all_f64(func_ptr, &all)
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
    // A missing trailing argument is `undefined` per spec (NOT NaN): default
    // parameters lower to a `param === undefined ? <default> : param` check in
    // the method prologue, so padding a hole with NaN left the default
    // un-applied (`async method(a, b, c = 99)` called via the dynamic vtable
    // path — e.g. a detached `C.prototype.method` value — saw `c = NaN`). Pad
    // with TAG_UNDEFINED so the prologue's default-check fires.
    #[inline(always)]
    unsafe fn arg_or_undefined(args_ptr: *const f64, args_len: usize, idx: usize) -> f64 {
        if idx < args_len {
            *args_ptr.add(idx)
        } else {
            // A missing argument is `undefined` per spec, not a bare IEEE NaN.
            // This vtable path is reached without call-site padding when a
            // method is invoked as a value (`const f = obj.m; f()`, or a bound
            // method from a getter), so NaN here defeated the callee's
            // default-param / destructuring prologue (`if (p === undefined)`).
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
    }

    // LLVM-generated methods have signature `double(double this, double arg0, ...)`.
    // `this` is NaN-boxed as f64, so we must pass it as f64 — not i64 — to match
    // the calling convention. On ARM64 i64 and f64 share registers, so passing i64
    // works by accident; on Windows x64 ABI they use *different* registers (rcx vs
    // xmm0), causing segfaults when the method reads `this` from the wrong register.
    //
    // Issue #519: all call sites pass `this` as a RAW POINTER (the bottom-48-bit
    // address from `jsval.as_pointer()`). Bit-casting raw pointer bits to f64
    // produces a subnormal float (no NaN-box tag), which the method body
    // interprets as a number — every nested method call inside the body sees
    // `(number).<method>` and either returns garbage or throws TypeError via
    // the issue #510 catch-all (e.g. RegExpRouter.match → `this.buildAllMatchers()`
    // → "(number).buildAllMatchers is not a function" inside SmartRouter's
    // dispatch chain). NaN-box with POINTER_TAG before passing so the body
    // sees a real instance pointer.
    let this_f64: f64 = {
        let bits = this as u64;
        const PTR_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
        if bits != 0 && bits <= PTR_MASK {
            // Raw pointer (no NaN-box tag) — wrap with POINTER_TAG so the
            // method body's `this` arrives as a real instance pointer.
            f64::from_bits(JSValue::pointer(bits as *mut u8).bits())
        } else {
            // Already NaN-boxed (top bits set) or null — pass through.
            f64::from_bits(bits)
        }
    };

    // A trailing param that is either the synthesized `arguments` object or a
    // user rest param (`method(a, ...rest)`) needs the call-site args bundled
    // into a JS array for that slot. Without this, an apply/dynamic dispatch
    // (`recv.method(...spread)` via `js_native_call_method_apply`) passes the
    // raw individual args and the callee reads `rest = args[0]` as a scalar —
    // marked's `new Marked()` -> `this.use(...e)` hit exactly this, throwing
    // `(number).forEach is not a function`. The synthesized-`arguments` slot
    // holds ALL passed args; a user rest slot holds only args from the rest
    // position onward (so `method(a, ...rest)` keeps `a` positional).
    let mut adjusted_args_storage: Option<Vec<f64>> = None;
    let (call_args_ptr, call_args_len) = if has_synthetic_arguments || has_rest {
        let visible_params = (param_count as usize).saturating_sub(1);
        let pack_start = if has_synthetic_arguments {
            0
        } else {
            visible_params.min(args_len)
        };
        let packed_len = args_len.saturating_sub(pack_start);
        let raw_args = crate::array::js_array_alloc_with_length(packed_len as u32);
        for (slot, i) in (pack_start..args_len).enumerate() {
            crate::array::js_array_set_f64(
                raw_args,
                slot as u32,
                arg_or_undefined(args_ptr, args_len, i),
            );
        }
        let raw_args_value = crate::value::js_nanbox_pointer(raw_args as i64);
        let mut args = Vec::with_capacity(param_count as usize);
        for i in 0..visible_params {
            args.push(arg_or_undefined(args_ptr, args_len, i));
        }
        args.push(raw_args_value);
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
    let mut positional: Vec<f64> = Vec::with_capacity(param_count as usize);
    for i in 0..(param_count as usize) {
        positional.push(arg_or_undefined(call_args_ptr, call_args_len, i));
    }
    call_fn_with_f64_args(func_ptr, this_f64, &positional)
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
