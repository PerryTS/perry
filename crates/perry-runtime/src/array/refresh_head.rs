//! Repair a compiled local after array forwarding.
use super::{clean_arr_ptr, ArrayHeader};

/// Repsel 4a.2 (#6904): follow the growth/GC forwarding chain of a
/// POINTER-tagged array head and return the re-boxed LIVE head; every other
/// input (non-pointer tags, handle-band ids, non-arrays, already-live heads)
/// is returned unchanged.
///
/// A lazy JSON array is returned unchanged: it materializes in place (same
/// address) when its own read policy decides, so there is nothing to repair,
/// and forcing it here would throw away the laziness of a loop that stops
/// early.
///
/// Consumed by the inline guard tiers' COLD arms: a caller-held stale head
/// (canonically: a Phase 2 specialized-ABI callee grew an array the CALLER
/// allocated — the callee's growth write-backs update the callee's own param
/// slot, so the caller's binding keeps the pre-growth stub forever) fails
/// every structural guard by design, which pinned such receivers to the
/// boxed fallback on EVERY access. The cold arm calls this once, stores the
/// repaired head back into the receiver's local slot, and every later
/// iteration re-loads the live head and takes the inline tier. Semantics are
/// unchanged — forwarding is transparent, this only re-points the binding at
/// the same JS object's live storage.
#[no_mangle]
pub extern "C" fn js_array_refresh_local_head(value: f64) -> f64 {
    let bits = value.to_bits();
    if bits & crate::value::TAG_MASK != crate::value::POINTER_TAG {
        return value;
    }
    let raw = (bits & crate::value::POINTER_MASK) as usize;
    // Handle-band ids and implausible addresses pass through untouched.
    if !crate::value::addr_class::is_plausible_heap_addr(raw) {
        return value;
    }
    if let Some(header) = unsafe { crate::value::addr_class::try_read_gc_header(raw) } {
        if header.obj_type == crate::gc::GC_TYPE_LAZY_ARRAY {
            return value;
        }
    }
    let cleaned = clean_arr_ptr(raw as *const ArrayHeader);
    if cleaned.is_null() || cleaned as usize == raw {
        return value;
    }
    f64::from_bits(crate::value::POINTER_TAG | (cleaned as u64 & crate::value::POINTER_MASK))
}
