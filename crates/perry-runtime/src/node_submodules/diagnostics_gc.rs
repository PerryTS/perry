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
}

/// Copied-minor counterpart of the finalize hook (the fast path sweeps
/// from-space wholesale without per-object finalize): drop entries whose
/// key is a dead from-space error — unmarked, unforwarded, nursery-space,
/// still typed `GC_TYPE_ERROR`. Mirrors
/// `finalize_dead_copied_minor_from_space_maps`.
pub(crate) fn finalize_dead_copied_minor_from_space_errors() -> usize {
    fn is_dead_from_space_error(addr: usize) -> bool {
        crate::gc::owner_is_dead_copied_minor_from_space_of_type(addr, crate::gc::GC_TYPE_ERROR)
    }
    let dead: Vec<usize> = ERROR_DIAGNOSTICS.with(|m| {
        m.borrow()
            .keys()
            .copied()
            .filter(|addr| is_dead_from_space_error(*addr))
            .collect()
    });
    let count = dead.len();
    for addr in dead {
        error_side_tables_clear_dead(addr);
    }
    count
}
