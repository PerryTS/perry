//! GC integration for the error side tables (2026-07-02 audit, GC deep set).
//! Split out of `diagnostics.rs` (2000-line lint gate); the diagnostic record
//! stays there.

use super::diagnostics::ERROR_DIAGNOSTICS;

// ---------------------------------------------------------------------------
// GC integration for the error side tables (2026-07-02 audit, GC deep set).
//
// Errors are MOVABLE arena objects (`GC_TYPE_ERROR`, `movable: true`). Node's
// code/syscall/errno/path/dest/hostname fields share one record keyed by the
// ErrorHeader address; user-assigned props moved onto `ObjectMeta.expando` in
// #8891 and need no bespoke table hook.

/// Move an error's address-keyed diagnostic record to its new address.
/// `GcMoveHookKind::ErrorSideTables`, fired by
/// `gc_type_after_payload_move` on evacuation/copy.
pub(crate) fn error_side_tables_owner_moved(old_user: usize, new_user: usize) {
    if old_user == new_user || old_user == 0 {
        return;
    }
    ERROR_DIAGNOSTICS.with(|m| {
        let mut m = m.borrow_mut();
        if let Some(diagnostics) = m.remove(&old_user) {
            m.insert(new_user, diagnostics);
        }
    });
}

/// Drop a dead error's diagnostic record so a fresh error allocated at the
/// recycled address doesn't inherit it.
/// `GcFinalizeHookKind::ErrorSideTables` (old-gen sweep) and the
/// copied-minor from-space finalize both land here.
pub(crate) fn error_side_tables_clear_dead(user_ptr: usize) {
    ERROR_DIAGNOSTICS.with(|m| {
        m.borrow_mut().remove(&user_ptr);
    });
    // 2026-07-09 GC audit wave 2: the DOMException brand set is address-
    // keyed with zero removals — clean it up with the rest of the error
    // side tables (latch-gated no-op unless a DOMException was ever made).
    crate::event_target::dom_exception_error_clear_dead(user_ptr);
}

/// Copied-minor counterpart of the finalize hook (the fast path sweeps
/// from-space wholesale without per-object finalize): drop entries whose
/// key is a dead from-space error — unmarked, unforwarded, nursery-space,
/// still typed `GC_TYPE_ERROR`. Mirrors
/// `finalize_dead_copied_minor_from_space_maps`.
pub(crate) fn finalize_dead_copied_minor_from_space_errors() {
    fn is_dead_from_space_error(addr: usize) -> bool {
        let space = crate::arena::classify_heap_space(addr);
        if !matches!(space, crate::arena::HeapSpace::NurseryEden)
            && space != crate::arena::active_survivor_space()
        {
            return false;
        }
        unsafe {
            let Some(header) = crate::value::addr_class::try_read_gc_header(addr) else {
                return false;
            };
            if header.obj_type != crate::gc::GC_TYPE_ERROR {
                return false;
            }
            let flags = header.gc_flags;
            flags & crate::gc::GC_FLAG_ARENA != 0
                && flags & (crate::gc::GC_FLAG_MARKED | crate::gc::GC_FLAG_FORWARDED) == 0
        }
    }
    let dead: Vec<usize> = ERROR_DIAGNOSTICS.with(|m| {
        m.borrow()
            .keys()
            .copied()
            .filter(|addr| is_dead_from_space_error(*addr))
            .collect()
    });
    for addr in dead {
        error_side_tables_clear_dead(addr);
    }
}
