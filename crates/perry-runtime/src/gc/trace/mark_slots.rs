//! Mark and remember strong slots through one object descriptor visit.

use super::*;

pub(in crate::gc) unsafe fn trace_heap_rewrite_slots(
    header: *mut GcHeader,
    valid_ptrs: &ValidPointerSet,
    worklist: &mut Vec<*mut GcHeader>,
) {
    trace_heap_rewrite_slots_impl::<false, false>(header, valid_ptrs, worklist, None);
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
        match crate::gc::barrier::barrier_remembering_parent(user) {
            Some(crate::gc::barrier::RememberingParent::Old) => {
                trace_heap_rewrite_slots_impl::<true, true>(
                    header,
                    valid_ptrs,
                    worklist,
                    Some(sticky),
                );
                return;
            }
            Some(crate::gc::barrier::RememberingParent::Malloc) => {
                // Malloc parents are uncommon and cannot use bare old pages.
                // Reuse the ordinary mark loop, then record their descriptor's
                // post-observer words through one shared cold helper. This
                // avoids emitting a third copy of the complete mark loop.
                trace_heap_rewrite_slots(header, valid_ptrs, worklist);
                remember_malloc_mark_slots(header, sticky);
                return;
            }
            // A young large container this full marks is promoted at its
            // address when the sweep ends, after the remembered-set clear
            // would have dropped entries made then: remember its young and
            // malloc children now, as an old parent's.
            None if crate::arena::is_young_large_owner(user) => {
                trace_heap_rewrite_slots_impl::<true, true>(
                    header,
                    valid_ptrs,
                    worklist,
                    Some(sticky),
                );
                return;
            }
            // No collector traces an image cell (marking stops at the image
            // boundary); its edges are roots in `gc::image_cells`.
            Some(crate::gc::barrier::RememberingParent::Image) | None => {}
        }
    }
    trace_heap_rewrite_slots(header, valid_ptrs, worklist);
}

#[cold]
#[inline(never)]
unsafe fn remember_malloc_mark_slots(header: *mut GcHeader, sticky: &mut StickyRememberedSet) {
    // Match the mark loop's early exits. Ephemeron words are not unconditional
    // strong edges, and a missing native callback must remain detectable.
    if (*header).obj_type == GC_TYPE_WEAK_STORAGE {
        return;
    }
    #[cfg(test)]
    if (*header).obj_type == GC_TYPE_NATIVE_HANDLE
        && crate::native_payload::callback_sabotage("mark")
    {
        return;
    }
    let weak_holder = crate::weakref::is_weak_holder_header(header);
    visit_gc_rewrite_slots(header, |slot| {
        if weak_holder && crate::weakref::is_weak_target_trace_slot(header, slot.slot) {
            return;
        }
        if let Some(word) = FieldWord::decode(slot.read()) {
            remember_decoded_full_mark_slot(sticky, header, slot, word, false);
        }
    });
}

unsafe fn trace_heap_rewrite_slots_impl<const REMEMBER: bool, const OLD_PARENT: bool>(
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
    let handles_active = super::full_trace::handle_trace_active();
    let shared_key_words =
        handles_active && !super::full_trace::object_words_may_name_handles(header);
    let proxy_trace_active = handles_active && !shared_key_words;
    // Reuse the physical generation the barrier gate already established.
    // A malloc owner's slots remain external even inside its own payload.
    #[cfg(not(test))]
    let weak_holder = crate::weakref::is_weak_holder_header(header);
    #[cfg(test)]
    let weak_holder =
        crate::weakref::is_weak_holder_header(header) && !mark_hoist_sabotage::forgetting_weak();
    // The per-slot visit, expanded at each of its three call sites rather than
    // called: the plan walk hands it two slots per object, and the call
    // (spilled captures, a six-register prologue) cost as much as the visit.
    macro_rules! visit_slot {
        ($slot:expr, $sticky:expr) => {{
            let slot: GcMutableSlot = $slot;
            let sticky: &mut Option<&mut StickyRememberedSet> = $sticky;
            'visit: {
                if weak_holder && crate::weakref::is_weak_target_trace_slot(header, slot.slot) {
                    break 'visit;
                }
                if let Some(kind) = slot.layout_kind {
                    record_layout_child_slot_read(kind);
                    record_trace_slot_read();
                }
                let bits = slot.read();
                // Keys have an exact raw-array descriptor, unlike an unknown
                // payload word. A marked shared array has already discharged
                // its trace, including any active handle observer.
                if slot.shared_keys_already_marked(bits) {
                    break 'visit;
                }
                if !handles_active {
                    // Preserve the collector's observer-free specialization: the
                    // child descriptor can omit its handle gate too.
                    trace_decoded_slot::<REMEMBER, false>(
                        header, slot, bits, valid_ptrs, worklist, sticky, OLD_PARENT,
                    );
                    break 'visit;
                }
                if !proxy_trace_active || (REMEMBER && bits & TAG_MASK == STRING_TAG) {
                    // A source string/shared word cannot invoke an observer, but
                    // its child's descriptor still has the globally active gate.
                    trace_decoded_slot::<REMEMBER, true>(
                        header, slot, bits, valid_ptrs, worklist, sticky, OLD_PARENT,
                    );
                    break 'visit;
                }
                // Foreign proxy/Fetch/pool observers retain the original ordering,
                // post-mark reread and barrier decode; no purity is assumed.
                #[cfg(test)]
                let observer_snapshot =
                    if REMEMBER && full_mark_decode_sabotage::stale_observer_word() {
                        FieldWord::decode(slot.read())
                    } else {
                        None
                    };
                mark_field_into_worklist(slot.read(), valid_ptrs, worklist, proxy_trace_active);
                #[cfg(test)]
                if let Some(word) = observer_snapshot {
                    remember_decoded_full_mark_slot(
                        sticky.as_deref_mut().unwrap(),
                        header,
                        slot,
                        word,
                        OLD_PARENT,
                    );
                    break 'visit;
                }
                if REMEMBER {
                    #[cfg(test)]
                    {
                        let child = decode_heap_addr(slot.read());
                        if child != 0
                            && crate::gc::barrier::remembered_child_needs_tracking(child)
                            && remembered_mark_sabotage::drop_next_entry()
                        {
                            break 'visit;
                        }
                    }
                    remember_mutable_old_to_young_slot(
                        sticky.as_deref_mut().unwrap(),
                        header,
                        slot,
                    );
                }
            }
        }};
    }
    // An object on a family layout is walked straight from its descriptor's
    // static plan (`static_scan`); every other object takes the generic walk.
    if super::super::static_scan::static_scan_admissible(header) && !layout_scan_trace_active() {
        if let Some(scan) =
            super::super::static_scan::static_scan(header, Some(full_trace_active()))
        {
            super::super::static_scan::for_each_mark_slot(&scan, |slot| {
                visit_slot!(slot, &mut sticky);
            });
            return;
        }
    }
    visit_gc_rewrite_slot_descriptors(header, |descriptor| unsafe {
        match descriptor {
            GcMutableSlotDescriptor::PointerFreeRange(range) => {
                if proxy_trace_active {
                    for i in 0..range.slot_count() {
                        super::full_trace::observe_handle(*range.slot(i), valid_ptrs);
                    }
                }
            }
            GcMutableSlotDescriptor::Slot(slot) => {
                visit_slot!(slot, &mut sticky);
            }
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
                    visit_slot!(GcMutableSlot::new(range.slot(i), layout_kind), &mut sticky);
                }
            }
        }
    });
}

/// The no-observer and active-child cases share decoding and custody. The
/// literal observer argument lets the existing leaf test discard its dynamic
/// gate in the no-observer case, as it did before sharing string decoding.
#[inline(always)]
unsafe fn trace_decoded_slot<const REMEMBER: bool, const HANDLES: bool>(
    header: *mut GcHeader,
    slot: GcMutableSlot,
    bits: u64,
    valid_ptrs: &ValidPointerSet,
    worklist: &mut Vec<*mut GcHeader>,
    sticky: &mut Option<&mut StickyRememberedSet>,
    old_parent: bool,
) {
    if let Some(word) = FieldWord::decode(bits) {
        let _marked = mark_decoded_field_into_worklist(word.addr(), valid_ptrs, worklist, HANDLES);
        #[cfg(test)]
        if !_marked && full_mark_decode_sabotage::requiring_new_mark() {
            return;
        }
        if REMEMBER {
            remember_decoded_full_mark_slot(
                sticky.as_deref_mut().unwrap(),
                header,
                slot,
                word,
                old_parent,
            );
        }
    }
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

/// Independent negatives for the shared-word full-mark consumer. Test only.
#[cfg(test)]
pub(crate) mod full_mark_decode_sabotage {
    use std::cell::Cell;
    thread_local! {
        static REQUIRE_NEW_MARK: Cell<bool> = const { Cell::new(false) };
        static GENERATION_ONLY_CUSTODY: Cell<bool> = const { Cell::new(false) };
        static STALE_OBSERVER_WORD: Cell<bool> = const { Cell::new(false) };
    }
    pub(crate) fn requiring_new_mark() -> bool {
        REQUIRE_NEW_MARK.with(Cell::get)
    }
    pub(crate) fn generation_only_custody() -> bool {
        GENERATION_ONLY_CUSTODY.with(Cell::get)
    }
    pub(crate) fn stale_observer_word() -> bool {
        STALE_OBSERVER_WORD.with(Cell::get)
    }
    pub(crate) struct ObserverGuard(bool);
    impl ObserverGuard {
        pub(crate) fn arm() -> Self {
            Self(STALE_OBSERVER_WORD.with(|s| s.replace(true)))
        }
    }
    impl Drop for ObserverGuard {
        fn drop(&mut self) {
            STALE_OBSERVER_WORD.with(|s| s.set(self.0));
        }
    }
    pub(crate) struct Guard(bool, bool);
    impl Guard {
        pub(crate) fn new(require_new_mark: bool, generation_only: bool) -> Self {
            Self(
                REQUIRE_NEW_MARK.with(|s| s.replace(require_new_mark)),
                GENERATION_ONLY_CUSTODY.with(|s| s.replace(generation_only)),
            )
        }
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            REQUIRE_NEW_MARK.with(|s| s.set(self.0));
            GENERATION_ONLY_CUSTODY.with(|s| s.set(self.1));
        }
    }
}
