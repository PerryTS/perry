//! Checked unboxing of dynamic-call callees.
//!
//! Split out of `dispatch.rs` to keep that module under the 2000-line cap.

use super::dispatch::throw_not_callable;

/// Unbox a dynamic-call callee to its closure pointer, throwing
/// `TypeError: value is not a function` when the value is not a heap
/// pointer.
///
/// Issue #5504: the call-emission path used to mask the callee's low 48
/// bits unconditionally and hand them to `js_closure_callN` as a
/// `*const ClosureHeader`. For a non-callable NUMBER whose mantissa's low
/// 48 bits happen to form an in-range address (e.g. `1e-8` →
/// `0x798E_E230_8C3A`), `get_valid_func_ptr`'s range check passed and the
/// subsequent `read_volatile(closure + 12)` dereferenced a wild pointer →
/// SIGSEGV. A range check alone cannot distinguish a real closure pointer
/// from a mantissa-derived address; only a tag check on the full
/// NaN-boxed value can, and it must happen BEFORE the low-48 mask. This
/// runs at the codegen unbox site where the original `f64` is still
/// available. A callable value (closure, bound method/function, native
/// handle) is always `POINTER_TAG`; numbers, strings, bigints, booleans,
/// null and undefined are not, and throw here.
#[no_mangle]
pub extern "C" fn js_closure_unbox_callee_checked(callee: f64) -> i64 {
    let bits = callee.to_bits();
    if bits & crate::value::TAG_MASK != crate::value::POINTER_TAG {
        throw_not_callable();
    }
    (bits & crate::value::POINTER_MASK) as i64
}
