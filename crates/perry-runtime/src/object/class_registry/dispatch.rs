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

crate::perry_thread_local! {
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

// ============================================================================
// #7769: object-dispatch-tower outcome cache.
//
// `js_native_call_method` resolves `obj.m(...)` by running a ~900-line tower of
// probes (native-module namespace, disposal protocol, TextDecoder handles,
// console, WeakMap/WeakSet, perf entries, own-field scan, prototype-chain
// walk) before finally consulting `CLASS_VTABLE_REGISTRY`. For an ordinary
// user-class instance every one of those probes misses, and the tail alone
// cost a `String` allocation for the method name, a GC-heap `StringHeader`
// allocation for the prototype probe, a `RwLock` read and two SipHash probes —
// per virtual call.
//
// This table records the tower's OUTCOME: an entry exists for
// `(class_id, method name)` only because a previous call for that exact pair
// ran the tower and reached a class-vtable resolution, which is the proof that
// no earlier probe claims this (class, name).
//
// There are two such resolution points and BOTH populate it: the parent-chain
// walk in `native_call_method::handle_methods` (which is where an INHERITED
// method resolves — `class Square extends Rect` calling `Rect`'s `area` — and
// therefore the common case in any real hierarchy), and the tail vtable arm of
// `js_native_call_method` (own-class methods). Populating only the tail left
// every inherited call permanently on the slow path.
//
// Writes go through `native_call_method::note_class_vtable_resolution`, which
// re-checks the receiver-shape predicate before storing, and the per-RECEIVER
// preconditions are re-verified again on every fast-path hit — see
// `native_call_method::class_vtable_fast_guard` — so a cache hit never
// substitutes for an object-specific check.
//
// Deliberately SEPARATE from `VTABLE_IC` above: that one is also written from
// the collection dispatcher, and it is keyed on the name's ADDRESS.
// ============================================================================

// The key is the method-name BYTES, never its address. `VTABLE_IC` above keys
// on the rodata pointer codegen passes, which is stable — but
// `js_native_call_method_str_key` reaches the same tower with a name
// materialised into a CALLER-STACK scratch buffer (`str_bytes_from_jsvalue`
// with a `[u8; SHORT_STRING_MAX_LEN]`), and two different short names can land
// at the same stack address in successive calls. Comparing content makes the
// cache exact for both sources; names too long to store inline are simply not
// cached.
const OBJ_DISPATCH_IC_SIZE: usize = 1024;
const OBJ_DISPATCH_IC_MASK: usize = OBJ_DISPATCH_IC_SIZE - 1;
/// Longest method name the cache stores. Comfortably above every method name
/// in practice; longer names fall through to the tower.
const OBJ_DISPATCH_IC_NAME_MAX: usize = 24;

#[repr(C)]
#[derive(Copy, Clone)]
struct ObjDispatchICEntry {
    gen: u64,
    class_id: u32,
    name_len: u32,
    name: [u8; OBJ_DISPATCH_IC_NAME_MAX],
    func_ptr: usize,
    param_count: u32,
    has_synthetic_arguments: u32,
    has_rest: u32,
    _pad: u32,
}

const EMPTY_OBJ_DISPATCH_IC_ENTRY: ObjDispatchICEntry = ObjDispatchICEntry {
    gen: 0,
    class_id: 0,
    name_len: 0,
    name: [0; OBJ_DISPATCH_IC_NAME_MAX],
    func_ptr: 0,
    param_count: 0,
    has_synthetic_arguments: 0,
    has_rest: 0,
    _pad: 0,
};

crate::perry_thread_local! {
    // Boxed for the same arm64_32 reason as `VTABLE_IC`: oversized inline TLS
    // storage overflows the ILP32 TLS layout. `perry_thread_local!` (#7469)
    // rather than `std::thread_local!` — Darwin has no local-exec TLS, so the
    // std form costs a real `_tlv_get_addr` call, which is exactly the tax the
    // fast path exists to remove.
    static OBJ_DISPATCH_IC: UnsafeCell<Box<[ObjDispatchICEntry]>> =
        UnsafeCell::new(vec![EMPTY_OBJ_DISPATCH_IC_ENTRY; OBJ_DISPATCH_IC_SIZE].into_boxed_slice());
}

/// FNV-1a over the name bytes, mixed with the class id.
#[inline(always)]
fn obj_dispatch_ic_slot(class_id: u32, name: &[u8]) -> usize {
    let mut h: u64 =
        0xcbf2_9ce4_8422_2325 ^ ((class_id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    for &b in name {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    ((h ^ (h >> 29)) as usize) & OBJ_DISPATCH_IC_MASK
}

/// The vtable entry the object-dispatch tower previously resolved for this
/// `(class_id, method name)`, if it is still current.
#[inline]
pub(crate) fn obj_dispatch_ic_lookup(
    class_id: u32,
    name: &[u8],
) -> Option<(usize, u32, bool, bool)> {
    if name.is_empty() || name.len() > OBJ_DISPATCH_IC_NAME_MAX {
        return None;
    }
    let cur_gen = VTABLE_GEN.load(Ordering::Relaxed);
    let slot = obj_dispatch_ic_slot(class_id, name);
    OBJ_DISPATCH_IC.with(|cell| {
        // SAFETY: the cache is thread-local and never handed out by reference
        // across a call that could re-enter this module.
        let entry = unsafe { &(**cell.get())[slot] };
        if entry.gen == cur_gen
            && entry.class_id == class_id
            && entry.name_len as usize == name.len()
            && entry.name[..name.len()] == *name
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

/// Record that the tower resolved `(class_id, name)` to this vtable entry.
/// Only the tower's own vtable arm may call this.
#[inline]
pub(crate) fn obj_dispatch_ic_insert(
    class_id: u32,
    name: &[u8],
    func_ptr: usize,
    param_count: u32,
    has_synthetic_arguments: bool,
    has_rest: bool,
) {
    if name.is_empty() || name.len() > OBJ_DISPATCH_IC_NAME_MAX {
        return;
    }
    let cur_gen = VTABLE_GEN.load(Ordering::Relaxed);
    let slot = obj_dispatch_ic_slot(class_id, name);
    let mut stored = [0u8; OBJ_DISPATCH_IC_NAME_MAX];
    stored[..name.len()].copy_from_slice(name);
    OBJ_DISPATCH_IC.with(|cell| {
        // SAFETY: thread-local, no outstanding borrows (see `_lookup`).
        unsafe {
            (**cell.get())[slot] = ObjDispatchICEntry {
                gen: cur_gen,
                class_id,
                name_len: name.len() as u32,
                name: stored,
                func_ptr,
                param_count,
                has_synthetic_arguments: u32::from(has_synthetic_arguments),
                has_rest: u32::from(has_rest),
                _pad: 0,
            };
        }
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
    call_vtable_method_inner(
        func_ptr,
        this,
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
    call_vtable_method_inner(
        func_ptr,
        this,
        args_ptr,
        args_len,
        param_count,
        has_synthetic_arguments,
        has_rest,
        Some(private_brand),
    )
}

unsafe fn call_vtable_method_inner(
    func_ptr: usize,
    this: i64,
    args_ptr: *const f64,
    args_len: usize,
    param_count: u32,
    has_synthetic_arguments: bool,
    has_rest: bool,
    explicit_private_brand: Option<f64>,
) -> f64 {
    // (`arg_or_undefined` — the spec-correct missing-argument padding — is a
    // module-level helper now, shared with `call_fn_with_this_and_args`.)

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

#[cfg(test)]
mod obj_dispatch_ic_tests {
    use super::*;

    const CID: u32 = 61_001;

    /// Run `body` on a stable vtable generation, retrying if it straddled a bump.
    ///
    /// Entries are keyed on `VTABLE_GEN`, and the whole point of that key is
    /// that ANY class registration anywhere retires the cache. Sibling tests
    /// register classes concurrently — `a_class_registration_invalidates_every_entry`
    /// in THIS module calls `test_bump_vtable_generation()` on purpose — so an
    /// insert/lookup pair can straddle a bump and miss for a reason that has
    /// nothing to do with what is being asserted.
    ///
    /// #7365: the retry existed but could not fire. `body` asserted internally,
    /// so a straddled pair **panicked on the setup assertion** before the loop
    /// got to re-check the generation, and the retry budget was never spent.
    /// That is why the failure rate INVERTED with scope: running just this
    /// module (`--lib obj_dispatch_ic_tests`) put its five tests — including
    /// the deliberate bumper — on threads together and failed 10 of 12 runs,
    /// while the full 2000-test suite spread them out and failed about 1 in 6.
    /// A filtered re-run, the standard triage move, therefore made an
    /// intermittent test look reliably broken.
    ///
    /// So `body` now REPORTS whether its observations were valid instead of
    /// asserting them: `false` means "a bump landed mid-pair, nothing was
    /// learned", which is a retry rather than a failure. The assertions the
    /// tests actually exist for stay assertions.
    fn with_stable_gen(body: &dyn Fn() -> bool) {
        for _ in 0..64 {
            let before = VTABLE_GEN.load(Ordering::Acquire);
            let observed = body();
            if observed && VTABLE_GEN.load(Ordering::Acquire) == before {
                return;
            }
        }
        panic!("vtable generation never stayed still long enough to assert");
    }

    #[test]
    fn a_hit_requires_matching_name_bytes_not_a_matching_address() {
        with_stable_gen(&|| {
            // The hazard this test exists for: `js_native_call_method_str_key`
            // materialises a short method name into a CALLER-STACK scratch buffer,
            // so two different names genuinely do arrive at the same address in
            // successive calls. An address-keyed cache would answer the second
            // call with the first call's method — a silent wrong-method dispatch.
            //
            // Sabotage it deliberately: cache under one name, then look up a
            // different name through the SAME backing storage.
            let mut scratch = *b"area\0\0\0\0";
            obj_dispatch_ic_insert(CID, &scratch[..4], 0xAAAA, 1, false, false);
            // Setup, not the subject: a miss here means a sibling bumped the
            // generation between insert and lookup. Report it and retry.
            if obj_dispatch_ic_lookup(CID, &scratch[..4]) != Some((0xAAAA, 1, false, false)) {
                return false;
            }

            scratch[..4].copy_from_slice(b"perim"[..4].try_into().unwrap());
            assert_eq!(
                obj_dispatch_ic_lookup(CID, &scratch[..4]),
                None,
                "a different name at the same address must MISS"
            );
            true
        });
    }

    #[test]
    fn a_hit_requires_the_matching_class_id() {
        with_stable_gen(&|| {
            obj_dispatch_ic_insert(CID, b"describe", 0xBBBB, 1, false, false);
            if obj_dispatch_ic_lookup(CID, b"describe") != Some((0xBBBB, 1, false, false)) {
                return false;
            }
            assert_eq!(obj_dispatch_ic_lookup(CID + 1, b"describe"), None);
            true
        });
    }

    #[test]
    fn a_class_registration_invalidates_every_entry() {
        obj_dispatch_ic_insert(CID, b"perimeter", 0xCCCC, 1, false, false);
        assert!(obj_dispatch_ic_lookup(CID, b"perimeter").is_some());
        // Registering a method anywhere bumps `VTABLE_GEN`; every cached
        // resolution predates the new vtable shape and must stop being used.
        test_bump_vtable_generation();
        assert_eq!(obj_dispatch_ic_lookup(CID, b"perimeter"), None);
    }

    #[test]
    fn names_too_long_to_store_are_never_cached() {
        with_stable_gen(&|| {
            let long = vec![b'x'; OBJ_DISPATCH_IC_NAME_MAX + 1];
            obj_dispatch_ic_insert(CID, &long, 0xDDDD, 1, false, false);
            assert_eq!(
                obj_dispatch_ic_lookup(CID, &long),
                None,
                "an over-long name must fall through to the tower, not alias a \
             truncated key"
            );
            true
        });
    }

    /// A name one byte shorter than a cached one must not hit it — the stored
    /// length is part of the key, not just the prefix bytes.
    #[test]
    fn a_prefix_of_a_cached_name_misses() {
        with_stable_gen(&|| {
            obj_dispatch_ic_insert(CID, b"describe", 0xEEEE, 1, false, false);
            assert_eq!(obj_dispatch_ic_lookup(CID, b"describ"), None);
            true
        });
    }
}
