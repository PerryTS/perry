//! Closure-pointer validation: GC-forwarding resolution (`clean_closure_ptr`)
//! and the speculation-safe `get_valid_info` / `get_valid_func_ptr` gate.

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
        // #5976: never probe a small-handle id's header (see
        // `get_valid_func_ptr` below and `value::addr_class` for the band map).
        if crate::value::addr_class::is_handle_band(addr as usize) {
            return closure;
        }
        let header = unsafe {
            (closure as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader
        };
        unsafe {
            // The kind is the GC type byte, which evacuation leaves intact
            // (it overwrites the first PAYLOAD word with the forwarding
            // address), so a from-space stub still reads as a closure here.
            if std::ptr::read_volatile(&(*header).obj_type) != crate::gc::GC_TYPE_CLOSURE
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

/// Validate a closure pointer and return its body's info if the closure is
/// valid (the bound-method / bound-function sentinel infos included).
///
/// Uses `read_volatile` for the GC type byte + `compiler_fence` to GUARANTEE
/// that the CLOSURE type byte is checked BEFORE the info word is ever read,
/// and that the optimizer cannot hoist that load above the check.
///
/// Background: `#[inline(never)]` on `is_valid_closure_ptr` was
/// insufficient — LLVM still speculatively hoisted the header-word load
/// before the kind check in the caller, so non-closure heap objects
/// (Box<JSValue>, BigInt structs) bypassed validation and their data ran as
/// code via `br x1` → SIGBUS.
///
/// Returns null if invalid (address out of range, not a live closure cell,
/// null info).
#[inline(always)]
pub fn get_valid_info(closure: *const ClosureHeader) -> *const crate::closure::JsFunctionInfo {
    let addr = closure as u64;
    if !(0x1000..0x0001_0000_0000_0000).contains(&addr) {
        return std::ptr::null();
    }
    // #5976: reject the small-handle band BEFORE the header kind probe.
    // Revocable-proxy ids, Web-Fetch/zlib/net handles and the generic stdlib
    // registry ids are all NaN-boxed `POINTER_TAG | <small id>` values, not
    // heap pointers — a real closure is always a GC allocation above the band
    // (`value::addr_class`). The 0x1000 floor above let every one of them
    // through, so this probe dereferenced unmapped low memory.
    if crate::value::addr_class::is_handle_band(addr as usize) {
        return std::ptr::null();
    }
    // The kind is the GC header's type byte (no payload magic): a live,
    // un-evacuated closure cell. Volatile + fence keep the info load below
    // from being hoisted above this check (the SIGBUS history above).
    let header = (addr as usize - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    let (obj_type, gc_flags) = unsafe {
        (
            std::ptr::read_volatile(&(*header).obj_type),
            std::ptr::read_volatile(&(*header).gc_flags),
        )
    };
    if obj_type != crate::gc::GC_TYPE_CLOSURE || gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
        return std::ptr::null();
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    unsafe { std::ptr::read_volatile(std::ptr::addr_of!((*closure).info)) }
}

/// Validate a closure pointer and return its body's code address — the
/// bound-method / bound-function sentinels included, which the dispatchers
/// route by value (#628, #2840) — or null.
#[inline(always)]
pub fn get_valid_func_ptr(closure: *const ClosureHeader) -> *const u8 {
    let info = get_valid_info(closure);
    if info.is_null() {
        return std::ptr::null();
    }
    unsafe { (*info).code }
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
/// dynamically-typed slot (`const g = obj.m` / `arr[0]` / `map.get(k)`)
/// reaches this generic path with
/// no static hint, and `get_valid_func_ptr` correctly refuses to dereference the
/// id (#5976/#6321) — which turned the SIGSEGV into a spurious TypeError. Node
/// calls the proxy. Re-boxing the id and asking the proxy registry costs nothing
/// on the hot path: this runs only where the code used to throw unconditionally.
pub fn dispatch_proxy_callee_or_throw(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    args: &[f64],
) -> f64 {
    let boxed =
        f64::from_bits(crate::value::POINTER_TAG | (closure as u64 & crate::value::POINTER_MASK));
    if crate::proxy::js_proxy_is_proxy(boxed) == 1 {
        return crate::proxy::call_proxy_value_with_this(boxed, this.as_f64(), args);
    }
    throw_not_callable()
}
