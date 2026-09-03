//! Event-loop keepalive and GC integration for live child processes.

use super::*;

/// Whether any live child is keeping the event loop alive — OR'd into
/// `js_stdlib_has_active_handles`.
pub(crate) fn cp_reactor_has_live() -> bool {
    CP_REFED_COUNT.load(Ordering::Relaxed) > 0
}

/// Toggle one live child's event-loop keepalive bit. Returns `false` after the
/// child has already left the registry.
pub(crate) fn cp_live_set_refed(handle: u64, refed: bool) -> bool {
    {
        let mut guard = cp_live_lock();
        let Some(child) = guard.as_mut().and_then(|map| map.get_mut(&handle)) else {
            return false;
        };
        if child.closed {
            return false;
        }
        if child.refed == refed {
            return true;
        }
        child.refed = refed;
    }
    if refed {
        CP_REFED_COUNT.fetch_add(1, Ordering::SeqCst);
        crate::event_pump::js_notify_main_thread();
    } else {
        CP_REFED_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
    true
}

/// Keep every live ChildProcess (and its reachable stdio sub-objects and
/// listener arrays) alive across collections, rewriting stored pointers after
/// evacuation.
pub(crate) fn cp_reactor_scan_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    if CP_LIVE_COUNT.load(Ordering::Relaxed) == 0 {
        return;
    }
    if let Some(map) = cp_live_lock().as_mut() {
        for child in map.values_mut() {
            visitor.visit_nanbox_u64_slot(&mut child.cp_bits);
            if child.abort_signal_bits != 0 {
                visitor.visit_nanbox_u64_slot(&mut child.abort_signal_bits);
            }
            if child.abort_listener_bits != 0 {
                visitor.visit_nanbox_u64_slot(&mut child.abort_listener_bits);
            }
            if let Some(exec) = child.exec.as_mut() {
                visitor.visit_nanbox_u64_slot(&mut exec.cb_bits);
            }
            if let Some(stdin) = child.stdin.as_mut() {
                for callback in &mut stdin.callbacks {
                    visitor.visit_nanbox_u64_slot(callback);
                }
            }
        }
    }
}
