//! Mark and remember strong slots through one object descriptor visit.

use super::*;

pub(in crate::gc) unsafe fn trace_heap_rewrite_slots(
    header: *mut GcHeader,
    valid_ptrs: &ValidPointerSet,
    worklist: &mut Vec<*mut GcHeader>,
) {
    trace_heap_rewrite_slots_impl::<false>(header, valid_ptrs, worklist, None);
}

/// Only full-cycle drains pass a buffer. The parent's generation and its
/// descriptor remain the authority for which slots enter the remembered set.
/// Specialize at the object boundary so minors and young parents keep their
/// existing mark loop, without a remembering branch at every slot.
pub(in crate::gc) unsafe fn trace_heap_rewrite_slots_remembering(
    header: *mut GcHeader,
    valid_ptrs: &ValidPointerSet,
    worklist: &mut Vec<*mut GcHeader>,
    sticky: Option<&mut StickyRememberedSet>,
) {
    if let Some(sticky) = sticky {
        let user = (header as *mut u8).add(GC_HEADER_SIZE) as usize;
        if barrier_parent_needs_remembering(user, true) {
            trace_heap_rewrite_slots_impl::<true>(header, valid_ptrs, worklist, Some(sticky));
            return;
        }
    }
    trace_heap_rewrite_slots(header, valid_ptrs, worklist);
}

unsafe fn trace_heap_rewrite_slots_impl<const REMEMBER: bool>(
    header: *mut GcHeader,
    valid_ptrs: &ValidPointerSet,
    worklist: &mut Vec<*mut GcHeader>,
    mut sticky: Option<&mut StickyRememberedSet>,
) {
    #[cfg(test)]
    if (*header).obj_type == GC_TYPE_NATIVE_HANDLE
        && crate::native_payload::callback_sabotage("mark")
    {
        return;
    }
    // #10182: two per-object facts read once instead of once per slot —
    // whether the proxy registry observes this trace (it changes only when a
    // proxy is created, and none is created inside one object's visit), and
    // whether the object is one of the weak-holder classes whose weak slots
    // the trace skips (its class cannot change while it is traced). Range
    // descriptors are walked here directly rather than through a per-slot
    // dynamic callback.
    if (*header).obj_type == GC_TYPE_WEAK_STORAGE {
        crate::gc::ephemeron::discover(header);
        return; // Neither word is an unconditional strong edge.
    }
    let proxy_trace_active = super::full_trace::handle_trace_active();
    #[cfg(not(test))]
    let weak_holder = crate::weakref::is_weak_holder_header(header);
    #[cfg(test)]
    let weak_holder =
        crate::weakref::is_weak_holder_header(header) && !mark_hoist_sabotage::forgetting_weak();
    visit_gc_rewrite_slot_descriptors(header, |descriptor| unsafe {
        let mut visit_slot = |slot: GcMutableSlot| {
            if weak_holder && crate::weakref::is_weak_target_trace_slot(header, slot.slot) {
                return;
            }
            if let Some(kind) = slot.layout_kind {
                record_layout_child_slot_read(kind);
                record_trace_slot_read();
            }
            mark_field_into_worklist(slot.read(), valid_ptrs, worklist, proxy_trace_active);
            if REMEMBER {
                #[cfg(test)]
                {
                    let child = decode_heap_addr(slot.read());
                    if child != 0
                        && crate::gc::barrier::remembered_child_needs_tracking(child)
                        && remembered_mark_sabotage::drop_next_entry()
                    {
                        return;
                    }
                }
                remember_mutable_old_to_young_slot(sticky.as_deref_mut().unwrap(), header, slot);
            }
        };
        match descriptor {
            GcMutableSlotDescriptor::PointerFreeRange(range) => {
                if proxy_trace_active {
                    for i in 0..range.slot_count() {
                        super::full_trace::observe_handle(*range.slot(i), valid_ptrs);
                    }
                }
            }
            GcMutableSlotDescriptor::Slot(slot) => visit_slot(slot),
            GcMutableSlotDescriptor::Range { range, layout_kind } => {
                // Start the header reads of the range's pointer children
                // before marking any of them: each is a cold DRAM read the
                // mark would otherwise take one at a time. A prefetch cannot
                // fault, so the candidate need not be proven a pointer yet.
                for i in 0..range.slot_count() {
                    let bits = *range.slot(i);
                    let tag = bits & TAG_MASK;
                    if tag == POINTER_TAG || tag == STRING_TAG {
                        super::prefetch::prefetch_read(
                            ((bits & POINTER_MASK) as usize).wrapping_sub(GC_HEADER_SIZE),
                        );
                    }
                }
                for i in 0..range.slot_count() {
                    visit_slot(GcMutableSlot::new(range.slot(i), layout_kind));
                }
            }
        }
    });
}

/// Sabotage switch for the mark-hoist test: the per-object weak-holder fact
/// reads false, so a weak holder's weak slots are traced strongly. Test builds
/// only.
#[cfg(test)]
pub(crate) mod mark_hoist_sabotage {
    use std::cell::Cell;

    thread_local! {
        static FORGET_WEAK: Cell<bool> = const { Cell::new(false) };
    }

    #[inline]
    pub(crate) fn forgetting_weak() -> bool {
        FORGET_WEAK.with(Cell::get)
    }

    pub(crate) struct Guard(bool);

    impl Guard {
        pub(crate) fn arm() -> Self {
            Self(FORGET_WEAK.with(|s| s.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            FORGET_WEAK.with(|s| s.set(self.0));
        }
    }
}

/// Drop one strong slot's remembered entry while still marking its child.
/// Test-only: proves the fold's coverage assertions can fail independently
/// of liveness during this full collection.
#[cfg(test)]
pub(crate) mod remembered_mark_sabotage {
    use std::cell::Cell;
    thread_local! {
        static DROP_NEXT: Cell<bool> = const { Cell::new(false) };
    }
    pub(crate) fn drop_next_entry() -> bool {
        DROP_NEXT.with(|s| s.replace(false))
    }
    pub(crate) struct Guard(bool);
    impl Guard {
        pub(crate) fn arm() -> Self {
            Self(DROP_NEXT.with(|s| s.replace(true)))
        }
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            DROP_NEXT.with(|s| s.set(self.0));
        }
    }
}
