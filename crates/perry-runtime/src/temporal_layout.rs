//! Shared Temporal cell identity checks.
use crate::temporal::{TemporalCell, TemporalKind, TemporalValue};
use crate::value::JSValue;
const NANBOX_PTR_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// True if `addr` (a cleaned heap address, NOT NaN-boxed bits) points at a
/// `TemporalCell`. Mirrors [`crate::date::is_date_cell_addr`]: reject the
/// small-handle band first (registry ids are pointer-tagged but not real heap),
/// then read the `GcHeader.obj_type`.
#[inline]
pub fn is_temporal_cell_addr(addr: usize) -> bool {
    // #5625: small typed arrays and `Buffer`s are raw-`alloc`'d off the GC heap
    // with NO `GcHeader` prefix, so the word at `addr - GC_HEADER_SIZE` is
    // unrelated heap memory that can coincidentally hold the `GC_TYPE_TEMPORAL`
    // tag (observed: a 4-element typed array whose stale back-read obj_type was
    // 18 → `<TA>.slice()` mis-routed through Temporal dispatch and deref'd a
    // garbage cell). They are never `TemporalCell`s; reject them via the side
    // tables first, exactly as `crate::date::is_date_cell_addr` does.
    if crate::typedarray_hook_slots::is_offheap_sidetable_alloc(addr) {
        return false;
    }
    match unsafe { crate::value::addr_class::try_read_gc_header(addr) } {
        // #11558: a Box-leaked symbol has no `GcHeader`; the word before it
        // can be the previous symbol's `id`, and id 18 reads as
        // `GC_TYPE_TEMPORAL`. Same screen as `crate::date::is_date_cell_addr`.
        Some(header) => {
            header.obj_type == crate::gc::GC_TYPE_TEMPORAL
                && !unsafe {
                    crate::symbol::may_be_symbol_header(addr as *const u8)
                        && crate::symbol::is_registered_symbol(addr)
                }
        }
        None => false,
    }
}

/// True if `value` is any Temporal value — a NaN-boxed pointer to a
/// `TemporalCell`.
#[inline]
pub fn is_temporal_value(value: f64) -> bool {
    let bits = value.to_bits();
    if !JSValue::from_bits(bits).is_pointer() {
        return false;
    }
    is_temporal_cell_addr((bits & NANBOX_PTR_MASK) as usize)
}

/// Borrow the `TemporalValue` a Temporal pointer refers to, or `None` if
/// `value` is not a Temporal cell. The borrow is valid as long as the cell is
/// live (which the caller's stack reference guarantees).
#[inline]
#[allow(clippy::needless_lifetimes)]
pub fn temporal_value_ref<'a>(value: f64) -> Option<&'a TemporalValue> {
    let bits = value.to_bits();
    if !JSValue::from_bits(bits).is_pointer() {
        return None;
    }
    let addr = (bits & NANBOX_PTR_MASK) as usize;
    if !is_temporal_cell_addr(addr) {
        return None;
    }
    // `&*box` derefs the `Box<TemporalValue>` to `&TemporalValue`.
    unsafe { Some(&*(*(addr as *const TemporalCell)).value) }
}

/// The brand sub-kind of a Temporal value, or `None` if not a Temporal cell.
#[inline]
pub fn temporal_kind(value: f64) -> Option<TemporalKind> {
    temporal_value_ref(value).map(TemporalValue::kind)
}

/// True if `value` is a Temporal cell whose brand kind matches the reserved
/// Temporal `class_id` (`CLASS_ID_TEMPORAL_FIRST..=CLASS_ID_TEMPORAL_LAST`).
/// Returns `false` for non-Temporal values or a mismatched kind.
#[inline]
pub fn temporal_value_matches_class_id(value: f64, class_id: u32) -> bool {
    match temporal_kind(value) {
        Some(k) => k.class_id() == class_id,
        None => false,
    }
}
