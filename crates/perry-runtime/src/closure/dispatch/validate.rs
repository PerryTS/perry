//! Closure-pointer validation: GC-forwarding resolution (`clean_closure_ptr`)
//! and the speculation-safe `get_valid_func_ptr` gate.

use super::*;

/// Resolve a closure pointer through any GC forwarding stubs left behind by
/// copied-minor or evacuation. Generated code may still hold a raw closure
/// local across an explicit `gc()` call; the shadow root is rewritten, but the
/// local alloca is not. Following the stub here keeps dynamic function calls
/// coherent after closures move from the nursery.
#[inline(always)]
pub fn clean_closure_ptr(mut closure: *const ClosureHeader) -> *const ClosureHeader {
    for _ in 0..64 {
        let addr = closure as u64;
        if !(0x1000..0x0001_0000_0000_0000).contains(&addr) {
            return closure;
        }
        // #5976: never probe `*(addr + 12)` on a small-handle id (see
        // `get_valid_func_ptr` below and `value::addr_class` for the band map).
        if crate::value::addr_class::is_handle_band(addr as usize) {
            return closure;
        }
        let type_tag = unsafe {
            std::ptr::read_volatile(
                (closure as *const u8).add(CLOSURE_TYPE_TAG_OFFSET) as *const u32
            )
        };
        if type_tag != CLOSURE_MAGIC {
            return closure;
        }
        if addr < crate::gc::GC_HEADER_SIZE as u64 {
            return closure;
        }
        let header = unsafe {
            (closure as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader
        };
        unsafe {
            if (*header).obj_type != crate::gc::GC_TYPE_CLOSURE
                || (*header).gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
            {
                return closure;
            }
            let next = crate::gc::forwarding_address(header) as *const ClosureHeader;
            if next.is_null() || next == closure {
                return closure;
            }
            closure = next;
        }
    }
    closure
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
pub fn get_valid_func_ptr(closure: *const ClosureHeader) -> *const u8 {
    let addr = closure as u64;
    if !(0x1000..0x0001_0000_0000_0000).contains(&addr) {
        return std::ptr::null();
    }
    // #5976: reject the small-handle band BEFORE the `*(addr + 12)`
    // CLOSURE_MAGIC probe. Revocable-proxy ids, Web-Fetch/zlib/net handles and
    // the generic stdlib registry ids are all NaN-boxed `POINTER_TAG | <small
    // id>` values, not heap pointers — a real closure is always a GC allocation
    // above the band (`value::addr_class`). The 0x1000 floor above let every
    // one of them through, so this probe dereferenced unmapped low memory:
    // `new ProxyOfClass(...)` reaching the dynamic construct path passed the
    // proxy value to `is_bound_function_closure_value` → `get_valid_func_ptr`,
    // which faulted at `PROXY_ID_BAND_START + id + 12` before
    // `js_new_function_construct` ever got to its Proxy branch. `is_closure_ptr`
    // (closure/dynamic_props.rs) has carried this guard since #1843/#4800; the
    // two validators here were the remaining hole.
    if crate::value::addr_class::is_handle_band(addr as usize) {
        return std::ptr::null();
    }
    let type_tag = unsafe {
        std::ptr::read_volatile((closure as *const u8).add(CLOSURE_TYPE_TAG_OFFSET) as *const u32)
    };
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
    // BOUND_FUNCTION_FUNC_PTR (0xBADD_B12D) is the Function.prototype.bind
    // sentinel — like BOUND_METHOD_FUNC_PTR it's not a real code address, so
    // pass it through here and let the call sites route to
    // dispatch_bound_function (#2840).
    if func_ptr == BOUND_FUNCTION_FUNC_PTR {
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

/// Last resort for a callee that [`get_valid_func_ptr`] rejected: it may be a
/// **Proxy of a function**, not a broken pointer. Route it through the Proxy
/// `[[Call]]` (apply trap, else forwarded to the target); otherwise throw
/// `TypeError: value is not a function` exactly as before.
///
/// #6320. The `js_closure_callN` entry points take a RAW `*const ClosureHeader`
/// — codegen has already stripped the NaN-box tag (`js_closure_unbox_callee_
/// checked`) — so a proxy callee arrives here as its bare registry id
/// (`PROXY_ID_BAND_START + id`). The compiler emits a `ProxyApply` node only
/// when it can statically prove the callee is a proxy; a proxy read out of a
/// dynamically-typed slot (`const g = obj.m` / `arr[0]` / `map.get(k)`, or a
/// method detached by `js_closure_unbind_this`) reaches this generic path with
/// no static hint, and `get_valid_func_ptr` correctly refuses to dereference the
/// id (#5976/#6321) — which turned the SIGSEGV into a spurious TypeError. Node
/// calls the proxy. Re-boxing the id and asking the proxy registry costs nothing
/// on the hot path: this runs only where the code used to throw unconditionally.
pub fn dispatch_proxy_callee_or_throw(closure: *const ClosureHeader, args: &[f64]) -> f64 {
    let boxed =
        f64::from_bits(crate::value::POINTER_TAG | (closure as u64 & crate::value::POINTER_MASK));
    if crate::proxy::js_proxy_is_proxy(boxed) == 1 {
        let this_arg = f64::from_bits(crate::value::TAG_UNDEFINED);
        return crate::proxy::call_proxy_value_with_this(boxed, this_arg, args);
    }
    throw_not_callable()
}
