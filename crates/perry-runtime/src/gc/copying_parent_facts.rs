//! The per-parent weak-holder fact the copying minor's slot visit reads, and
//! the slot visit itself. Split out of `gc/copying.rs` for the 2000-line lint.

use super::*;

/// Is this parent one of the weak-holder classes whose weak slots the copying
/// minor must not evacuate through?
///
/// The question is a property of the PARENT's class, but the collector asked
/// the per-SLOT question (`weakref::is_weak_target_trace_slot`) for every slot
/// of every object — an out-of-line call that re-reads `obj_type` and
/// `class_id` and then rejects on class. #10182 gave the full mark the
/// per-object read (`gc/trace.rs`); the copying minor never got it.
///
/// Read LAZILY by the callers: once per object is right only for objects that
/// actually have a slot to visit. See `scan_object_fields`.
///
/// Null-safe: `is_weak_holder_header` answers false for a null header, which
/// is the same answer the per-slot question gave.
#[inline]
pub(super) unsafe fn weak_holder_fact(header: *mut GcHeader) -> bool {
    #[cfg(test)]
    if copy_hoist_sabotage::forgetting_weak() {
        return false;
    }
    crate::weakref::is_weak_holder_header(header)
}

/// Test-only sabotage for [`weak_holder_fact`]: forgetting the per-object fact
/// must change what the collector does, or the hoist is documentation
/// (CLAUDE.md, a gate that cannot fail). Its witness is
/// `gc::tests::copy_slot_hoists`.
#[cfg(test)]
pub(crate) mod copy_hoist_sabotage {
    use std::cell::Cell;

    thread_local! {
        static FORGET_WEAK: Cell<bool> = const { Cell::new(false) };
    }

    #[inline]
    pub(crate) fn forgetting_weak() -> bool {
        FORGET_WEAK.with(Cell::get)
    }

    pub(crate) struct WeakGuard(bool);

    impl WeakGuard {
        pub(crate) fn arm() -> Self {
            Self(FORGET_WEAK.with(|s| s.replace(true)))
        }
    }

    impl Drop for WeakGuard {
        fn drop(&mut self) {
            FORGET_WEAK.with(|s| s.set(self.0));
        }
    }
}

impl CopyingNurseryCollector {
    pub(super) unsafe fn visit_slot_with_parent(
        &mut self,
        slot: *mut u64,
        parent_header: *mut GcHeader,
        external: bool,
    ) {
        let weak_holder = weak_holder_fact(parent_header);
        self.visit_slot_with_weak_fact(slot, parent_header, weak_holder, external);
    }

    /// [`visit_slot_with_parent`](Self::visit_slot_with_parent) with the
    /// parent's weak-holder fact supplied by the caller, so a whole object's
    /// slots pay for it once. See [`weak_holder_fact`].
    pub(super) unsafe fn visit_slot_with_weak_fact(
        &mut self,
        slot: *mut u64,
        parent_header: *mut GcHeader,
        weak_holder: bool,
        external: bool,
    ) {
        if slot.is_null() {
            return;
        }
        // Weak target edge (WeakRef referent / weak entry key / finreg
        // record target): never evacuate through it — the mark/barrier
        // paths skip these (`is_weak_target_trace_slot`), and copying
        // through them strengthened the reference, so WeakMap entries
        // never tombstoned and FinalizationRegistry never fired while
        // copied-minor was the operative cycle. Repair an already-moved
        // target's address now and queue the slot so `repair_weak_slots`
        // fixes targets evacuated after this visit; the registry pass then
        // tombstones dead ones.
        // No remembered-set entry either — the write barrier skips weak
        // slots the same way.
        if weak_holder && crate::weakref::is_weak_target_trace_slot(parent_header, slot) {
            if let Some(new_bits) = self.rewrite_value_bits(*slot) {
                *slot = new_bits;
            }
            self.weak_slots.push(slot);
            return;
        }
        let bits = *slot;
        if let Some(new_bits) = self.visit_value_bits(bits) {
            *slot = new_bits;
        }
        if !parent_header.is_null() && !self.skip_remembering {
            let parent_user = (parent_header as *mut u8).add(GC_HEADER_SIZE) as usize;
            if barrier_parent_needs_remembering(parent_user, external) {
                if let Some((child_addr, _, _)) = self.ptrs.decode_bits(*slot) {
                    // Keep old→malloc pages dirty alongside old→nursery:
                    // the malloc child is spared by this cycle's mark
                    // (mark_addr handles CopyingPointerKind::Malloc) but
                    // the NEXT minor's malloc sweep needs the edge again.
                    if crate::gc::barrier::remembered_child_needs_tracking(child_addr) {
                        self.sticky.remember_slot(parent_header, slot, external);
                    }
                }
            }
        }
    }
}
