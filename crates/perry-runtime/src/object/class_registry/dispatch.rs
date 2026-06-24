use super::*;
use crate::object::*;
use crate::{ArrayHeader, JSValue};
use std::cell::{Cell, RefCell, UnsafeCell};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicU8, Ordering};
use std::sync::RwLock;

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
    static VTABLE_IC: UnsafeCell<[VTableICEntry; VTABLE_IC_SIZE]> = const {
        UnsafeCell::new([EMPTY_VTABLE_IC_ENTRY; VTABLE_IC_SIZE])
    };
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
        let cache = &*cell.get();
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
        let cache = &mut *cell.get();
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

    match param_count {
        0 => {
            let f: extern "C" fn(f64) -> f64 = std::mem::transmute(func_ptr);
            f(this_f64)
        }
        1 => {
            let f: extern "C" fn(f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(this_f64, arg_or_undefined(call_args_ptr, call_args_len, 0))
        }
        2 => {
            let f: extern "C" fn(f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_undefined(call_args_ptr, call_args_len, 0),
                arg_or_undefined(call_args_ptr, call_args_len, 1),
            )
        }
        3 => {
            let f: extern "C" fn(f64, f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_undefined(call_args_ptr, call_args_len, 0),
                arg_or_undefined(call_args_ptr, call_args_len, 1),
                arg_or_undefined(call_args_ptr, call_args_len, 2),
            )
        }
        4 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_undefined(call_args_ptr, call_args_len, 0),
                arg_or_undefined(call_args_ptr, call_args_len, 1),
                arg_or_undefined(call_args_ptr, call_args_len, 2),
                arg_or_undefined(call_args_ptr, call_args_len, 3),
            )
        }
        5 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_undefined(call_args_ptr, call_args_len, 0),
                arg_or_undefined(call_args_ptr, call_args_len, 1),
                arg_or_undefined(call_args_ptr, call_args_len, 2),
                arg_or_undefined(call_args_ptr, call_args_len, 3),
                arg_or_undefined(call_args_ptr, call_args_len, 4),
            )
        }
        6 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_undefined(call_args_ptr, call_args_len, 0),
                arg_or_undefined(call_args_ptr, call_args_len, 1),
                arg_or_undefined(call_args_ptr, call_args_len, 2),
                arg_or_undefined(call_args_ptr, call_args_len, 3),
                arg_or_undefined(call_args_ptr, call_args_len, 4),
                arg_or_undefined(call_args_ptr, call_args_len, 5),
            )
        }
        7 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_undefined(call_args_ptr, call_args_len, 0),
                arg_or_undefined(call_args_ptr, call_args_len, 1),
                arg_or_undefined(call_args_ptr, call_args_len, 2),
                arg_or_undefined(call_args_ptr, call_args_len, 3),
                arg_or_undefined(call_args_ptr, call_args_len, 4),
                arg_or_undefined(call_args_ptr, call_args_len, 5),
                arg_or_undefined(call_args_ptr, call_args_len, 6),
            )
        }
        8 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_undefined(call_args_ptr, call_args_len, 0),
                arg_or_undefined(call_args_ptr, call_args_len, 1),
                arg_or_undefined(call_args_ptr, call_args_len, 2),
                arg_or_undefined(call_args_ptr, call_args_len, 3),
                arg_or_undefined(call_args_ptr, call_args_len, 4),
                arg_or_undefined(call_args_ptr, call_args_len, 5),
                arg_or_undefined(call_args_ptr, call_args_len, 6),
                arg_or_undefined(call_args_ptr, call_args_len, 7),
            )
        }
        9 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_undefined(call_args_ptr, call_args_len, 0),
                arg_or_undefined(call_args_ptr, call_args_len, 1),
                arg_or_undefined(call_args_ptr, call_args_len, 2),
                arg_or_undefined(call_args_ptr, call_args_len, 3),
                arg_or_undefined(call_args_ptr, call_args_len, 4),
                arg_or_undefined(call_args_ptr, call_args_len, 5),
                arg_or_undefined(call_args_ptr, call_args_len, 6),
                arg_or_undefined(call_args_ptr, call_args_len, 7),
                arg_or_undefined(call_args_ptr, call_args_len, 8),
            )
        }
        // Arities above the explicit arms: the generated method/ctor signature is
        // `double(double this, double×param_count)`. Rust can't form a
        // param_count-arity fn pointer dynamically, so transmute to a generous
        // fixed arity (64) and pass `param_count` real args plus `undefined`
        // padding (`arg_or_undefined` yields undefined past `call_args_len`).
        // Passing MORE args than the callee declares is safe on every target —
        // the arg area is caller-allocated and caller-cleaned, and the callee
        // reads only its declared params. This is the runtime-dispatch counterpart
        // to the codegen direct call, and matters for ctors/methods that take many
        // params — notably a class capturing dozens of module-level `require`s
        // (`__perry_cap_*` params), the wall-45 `Derived extends _mod.default`
        // shape, where the pre-fix 10-arg cap silently dropped captures 10+.
        // (The prior `_` arm called every >9-arity function as if it had 10
        // params.) `debug_assert` flags the rare class that would still exceed
        // the bound so it surfaces in tests rather than as silent corruption.
        _ => {
            debug_assert!(
                param_count as usize <= 64,
                "call_vtable_method: param_count {} exceeds fixed dispatch arity 64",
                param_count
            );
            let f: extern "C" fn(
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
                f64,
            ) -> f64 = std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_undefined(call_args_ptr, call_args_len, 0),
                arg_or_undefined(call_args_ptr, call_args_len, 1),
                arg_or_undefined(call_args_ptr, call_args_len, 2),
                arg_or_undefined(call_args_ptr, call_args_len, 3),
                arg_or_undefined(call_args_ptr, call_args_len, 4),
                arg_or_undefined(call_args_ptr, call_args_len, 5),
                arg_or_undefined(call_args_ptr, call_args_len, 6),
                arg_or_undefined(call_args_ptr, call_args_len, 7),
                arg_or_undefined(call_args_ptr, call_args_len, 8),
                arg_or_undefined(call_args_ptr, call_args_len, 9),
                arg_or_undefined(call_args_ptr, call_args_len, 10),
                arg_or_undefined(call_args_ptr, call_args_len, 11),
                arg_or_undefined(call_args_ptr, call_args_len, 12),
                arg_or_undefined(call_args_ptr, call_args_len, 13),
                arg_or_undefined(call_args_ptr, call_args_len, 14),
                arg_or_undefined(call_args_ptr, call_args_len, 15),
                arg_or_undefined(call_args_ptr, call_args_len, 16),
                arg_or_undefined(call_args_ptr, call_args_len, 17),
                arg_or_undefined(call_args_ptr, call_args_len, 18),
                arg_or_undefined(call_args_ptr, call_args_len, 19),
                arg_or_undefined(call_args_ptr, call_args_len, 20),
                arg_or_undefined(call_args_ptr, call_args_len, 21),
                arg_or_undefined(call_args_ptr, call_args_len, 22),
                arg_or_undefined(call_args_ptr, call_args_len, 23),
                arg_or_undefined(call_args_ptr, call_args_len, 24),
                arg_or_undefined(call_args_ptr, call_args_len, 25),
                arg_or_undefined(call_args_ptr, call_args_len, 26),
                arg_or_undefined(call_args_ptr, call_args_len, 27),
                arg_or_undefined(call_args_ptr, call_args_len, 28),
                arg_or_undefined(call_args_ptr, call_args_len, 29),
                arg_or_undefined(call_args_ptr, call_args_len, 30),
                arg_or_undefined(call_args_ptr, call_args_len, 31),
                arg_or_undefined(call_args_ptr, call_args_len, 32),
                arg_or_undefined(call_args_ptr, call_args_len, 33),
                arg_or_undefined(call_args_ptr, call_args_len, 34),
                arg_or_undefined(call_args_ptr, call_args_len, 35),
                arg_or_undefined(call_args_ptr, call_args_len, 36),
                arg_or_undefined(call_args_ptr, call_args_len, 37),
                arg_or_undefined(call_args_ptr, call_args_len, 38),
                arg_or_undefined(call_args_ptr, call_args_len, 39),
                arg_or_undefined(call_args_ptr, call_args_len, 40),
                arg_or_undefined(call_args_ptr, call_args_len, 41),
                arg_or_undefined(call_args_ptr, call_args_len, 42),
                arg_or_undefined(call_args_ptr, call_args_len, 43),
                arg_or_undefined(call_args_ptr, call_args_len, 44),
                arg_or_undefined(call_args_ptr, call_args_len, 45),
                arg_or_undefined(call_args_ptr, call_args_len, 46),
                arg_or_undefined(call_args_ptr, call_args_len, 47),
                arg_or_undefined(call_args_ptr, call_args_len, 48),
                arg_or_undefined(call_args_ptr, call_args_len, 49),
                arg_or_undefined(call_args_ptr, call_args_len, 50),
                arg_or_undefined(call_args_ptr, call_args_len, 51),
                arg_or_undefined(call_args_ptr, call_args_len, 52),
                arg_or_undefined(call_args_ptr, call_args_len, 53),
                arg_or_undefined(call_args_ptr, call_args_len, 54),
                arg_or_undefined(call_args_ptr, call_args_len, 55),
                arg_or_undefined(call_args_ptr, call_args_len, 56),
                arg_or_undefined(call_args_ptr, call_args_len, 57),
                arg_or_undefined(call_args_ptr, call_args_len, 58),
                arg_or_undefined(call_args_ptr, call_args_len, 59),
                arg_or_undefined(call_args_ptr, call_args_len, 60),
                arg_or_undefined(call_args_ptr, call_args_len, 61),
                arg_or_undefined(call_args_ptr, call_args_len, 62),
                arg_or_undefined(call_args_ptr, call_args_len, 63),
            )
        }
    }
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
