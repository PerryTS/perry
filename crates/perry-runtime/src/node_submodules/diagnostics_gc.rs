//! GC integration for the error side tables (2026-07-02 audit, GC deep set).
//! Split out of `diagnostics.rs` (2000-line lint gate); the tables and
//! `ErrUserProp` stay there.

use super::diagnostics::{
    ErrUserProp, ERROR_MESSAGE_CODES, ERROR_MESSAGE_DESTS, ERROR_MESSAGE_ERRNOS,
    ERROR_MESSAGE_HOSTNAMES, ERROR_MESSAGE_PATHS, ERROR_MESSAGE_SYSCALLS, ERROR_USER_PROPS,
};
use std::cell::RefCell;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// GC integration for the error side tables (2026-07-02 audit, GC deep set).
//
// Errors are MOVABLE arena objects (`GC_TYPE_ERROR`, `movable: true`), and
// every table above — ERROR_MESSAGE_{CODES,SYSCALLS,ERRNOS,PATHS,DESTS,
// HOSTNAMES} and ERROR_USER_PROPS — keys by the ErrorHeader address. Before
// these hooks existed: (1) a moved error's entries were stranded at the old
// address, so `err.code` / user-assigned props VANISHED after an evacuating
// cycle; (2) a swept error's entries persisted, so a FRESH error allocated
// at the recycled address INHERITED the dead error's codes/props; (3) an
// object-valued user prop (`err.cause = {...}`) was stored as raw bits
// invisible to GC — collectable while still reachable through the error.
// The in-code "stale entries are harmless" comments were wrong on all
// three counts.

/// Move an error's entries in every address-keyed side table to its new
/// address. `GcMoveHookKind::ErrorSideTables`, fired by
/// `gc_type_after_payload_move` on evacuation/copy.
pub(crate) fn error_side_tables_owner_moved(old_user: usize, new_user: usize) {
    if old_user == new_user || old_user == 0 {
        return;
    }
    fn rekey<V>(m: &RefCell<HashMap<usize, V>>, old: usize, new: usize) {
        let mut m = m.borrow_mut();
        if let Some(v) = m.remove(&old) {
            m.insert(new, v);
        }
    }
    ERROR_MESSAGE_CODES.with(|m| rekey(m, old_user, new_user));
    ERROR_MESSAGE_SYSCALLS.with(|m| rekey(m, old_user, new_user));
    ERROR_MESSAGE_ERRNOS.with(|m| rekey(m, old_user, new_user));
    ERROR_MESSAGE_PATHS.with(|m| rekey(m, old_user, new_user));
    ERROR_MESSAGE_DESTS.with(|m| rekey(m, old_user, new_user));
    ERROR_MESSAGE_HOSTNAMES.with(|m| rekey(m, old_user, new_user));
    ERROR_USER_PROPS.with(|m| rekey(m, old_user, new_user));
}

/// Drop a dead error's entries from every side table so a fresh error
/// allocated at the recycled address doesn't inherit them.
/// `GcFinalizeHookKind::ErrorSideTables` (old-gen sweep) and the
/// copied-minor from-space finalize both land here.
pub(crate) fn error_side_tables_clear_dead(user_ptr: usize) {
    ERROR_MESSAGE_CODES.with(|m| {
        m.borrow_mut().remove(&user_ptr);
    });
    ERROR_MESSAGE_SYSCALLS.with(|m| {
        m.borrow_mut().remove(&user_ptr);
    });
    ERROR_MESSAGE_ERRNOS.with(|m| {
        m.borrow_mut().remove(&user_ptr);
    });
    ERROR_MESSAGE_PATHS.with(|m| {
        m.borrow_mut().remove(&user_ptr);
    });
    ERROR_MESSAGE_DESTS.with(|m| {
        m.borrow_mut().remove(&user_ptr);
    });
    ERROR_MESSAGE_HOSTNAMES.with(|m| {
        m.borrow_mut().remove(&user_ptr);
    });
    ERROR_USER_PROPS.with(|m| {
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
    let mut dead: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut collect = |keys: Vec<usize>| {
        for addr in keys {
            if is_dead_from_space_error(addr) {
                dead.insert(addr);
            }
        }
    };
    collect(ERROR_MESSAGE_CODES.with(|m| m.borrow().keys().copied().collect()));
    collect(ERROR_MESSAGE_SYSCALLS.with(|m| m.borrow().keys().copied().collect()));
    collect(ERROR_MESSAGE_ERRNOS.with(|m| m.borrow().keys().copied().collect()));
    collect(ERROR_MESSAGE_PATHS.with(|m| m.borrow().keys().copied().collect()));
    collect(ERROR_MESSAGE_DESTS.with(|m| m.borrow().keys().copied().collect()));
    collect(ERROR_MESSAGE_HOSTNAMES.with(|m| m.borrow().keys().copied().collect()));
    collect(ERROR_USER_PROPS.with(|m| m.borrow().keys().copied().collect()));
    for addr in dead {
        error_side_tables_clear_dead(addr);
    }
}

/// Registered mutable-root scanner: object/string-valued user props
/// (`ErrUserProp::Bits` holding a heap-tagged value) were INVISIBLE to GC —
/// an `err.cause = {...}` object was collectable while still reachable
/// through the error. Visit each as a mutable root so the referent stays
/// live and a moved referent's address is rewritten in place.
/// (`visit_nanbox_u64_slot` is tag-aware: numeric/boolean bits are left
/// untouched.)
pub(crate) fn scan_error_user_props_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    ERROR_USER_PROPS.with(|m| {
        for props in m.borrow_mut().values_mut() {
            for v in props.values_mut() {
                if let ErrUserProp::Bits(bits) = v {
                    visitor.visit_nanbox_u64_slot(bits);
                }
            }
        }
    });
}
