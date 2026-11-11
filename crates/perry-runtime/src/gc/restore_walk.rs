//! Which slots of an owner the post-cycle coverage restore asks about.
//!
//! `restore_surviving_dirty_coverage` re-derives the remembered set from the
//! pre-clear dirty snapshot. For an owner the dirty scan could not report
//! complete it used to visit every slot the owner's descriptor names. An old
//! Map store, Set store or array that spans many pages is such an owner on
//! every minor (a store into its tail dirties one page and leaves the rest
//! clean), so each minor paid for the whole container: on a chess search with
//! a 36 000-entry transposition table that was 7.8 M slot visits over 296
//! minors, 2.5% of the program.
//!
//! # The custody rule this rests on
//!
//! The remembered set is page granular for an old parent's in-body slots: a
//! slot that holds a child a minor must keep valid has its page in the dirty
//! set, or the edge is already lost (the dirty scan never looks at a clean
//! page, and an old object with no dirty page is never visited by the restore
//! at all). `scan_dirty_object_slots` states the same custody: "an old in-body
//! slot is described by its page; every other slot needs the owner entry".
//! So an in-body slot of an old parent on a page the snapshot does not name
//! owes the remembered set nothing, and visiting it can only find nothing.
//!
//! The bounded walk therefore visits, of an **old arena parent**:
//!
//! * every in-body slot whose page the snapshot names (one probe per page for
//!   a range, through the same `dirty_slot_ranges_for` the dirty scan uses);
//! * every slot outside the parent's body, as before: those are owner
//!   qualified, and an owner entry does not say which of its pages are dirty.
//!
//! A malloc parent keeps the whole-owner walk for the same reason: all of its
//! entries are owner qualified (`RememberingParent::Malloc`).
//!
//! # Witness
//!
//! Test and debug builds, and any build under
//! `PERRY_GC_RESTORE_DIRTY_PAGES=verify`, also run the whole-owner walk and
//! require it to add no entry the bounded walk and the cycle's own sticky set
//! did not already restore. `PERRY_GC_RESTORE_DIRTY_PAGES=0` selects the
//! whole-owner walk outright.

use super::barrier::dirty_slot_ranges_for;
use super::*;

/// How the coverage restore walks an old arena parent it cannot skip.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RestoreWalk {
    /// Every slot the owner's descriptor names.
    WholeOwner,
    /// In-body slots only on the snapshot's dirty pages.
    DirtyPages,
    /// `DirtyPages`, with the whole-owner walk as its witness.
    Verify,
}

/// Pure parse of `PERRY_GC_RESTORE_DIRTY_PAGES`, so every state is asserted
/// without touching the process environment.
///
/// Default is the bounded walk. Only an explicit off spelling selects the
/// whole-owner walk, and only `verify` adds the witness: a typo must not
/// change which walk a bisect is measuring.
pub(super) fn parse_restore_walk(raw: Option<&str>) -> RestoreWalk {
    match raw.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("0" | "off" | "false" | "no") => RestoreWalk::WholeOwner,
        Some("verify") => RestoreWalk::Verify,
        _ => RestoreWalk::DirtyPages,
    }
}

#[cfg(test)]
thread_local! {
    static RESTORE_WALK_TEST_OVERRIDE: std::cell::Cell<Option<RestoreWalk>> =
        const { std::cell::Cell::new(None) };
}

/// Pin [`restore_walk`] for this thread only.
#[cfg(test)]
pub(crate) struct RestoreWalkTestGuard(Option<RestoreWalk>);

#[cfg(test)]
impl RestoreWalkTestGuard {
    pub(super) fn pin(walk: RestoreWalk) -> Self {
        Self(RESTORE_WALK_TEST_OVERRIDE.with(|c| c.replace(Some(walk))))
    }
}

#[cfg(test)]
impl Drop for RestoreWalkTestGuard {
    fn drop(&mut self) {
        RESTORE_WALK_TEST_OVERRIDE.with(|c| c.set(self.0));
    }
}

/// The walk this process uses. Read once, at the first restore.
pub(super) fn restore_walk() -> RestoreWalk {
    #[cfg(test)]
    if let Some(walk) = RESTORE_WALK_TEST_OVERRIDE.with(std::cell::Cell::get) {
        return walk;
    }
    use std::sync::OnceLock;
    static CACHED: OnceLock<RestoreWalk> = OnceLock::new();
    let walk = *crate::once_init::get_or_init(&CACHED, || {
        parse_restore_walk(
            std::env::var("PERRY_GC_RESTORE_DIRTY_PAGES")
                .ok()
                .as_deref(),
        )
    });
    // Test and debug builds always carry the witness.
    if cfg!(any(test, debug_assertions)) && walk == RestoreWalk::DirtyPages {
        return RestoreWalk::Verify;
    }
    walk
}

/// Visit the slots of the old arena parent `header` that can owe the
/// remembered set an entry, given the pre-clear dirty pages. Returns the
/// number of in-body slots it did not visit because their page is clean.
///
/// # Safety
/// `header` is a live old-generation arena object.
pub(super) unsafe fn visit_old_parent_owed_slots<const DIAGNOSTICS: bool>(
    header: *mut GcHeader,
    dirty_pages: &crate::fast_hash::PtrHashSet<usize>,
    visit: &mut dyn FnMut(GcMutableSlot),
) -> usize {
    let body_start = header as usize;
    let body_end = body_start.saturating_add((*header).size as usize);
    let in_body = |slot: *mut u64| {
        let addr = slot as usize;
        addr >= body_start && addr < body_end
    };
    let mut clean_slots = 0usize;
    // `dirty_slot_ranges_for` reports through the dirty scan's statistics;
    // the restore keeps its own counts.
    let mut scratch = RememberedSetTraceStats::default();
    visit_gc_rewrite_slot_descriptors_shared::<DIAGNOSTICS>(header, &mut |descriptor| unsafe {
        match descriptor {
            GcMutableSlotDescriptor::Slot(slot) => {
                if in_body(slot.slot)
                    && !super::barrier::dirty_pages_contains_addr(dirty_pages, slot.slot as usize)
                {
                    clean_slots += 1;
                    return;
                }
                visit(slot);
            }
            GcMutableSlotDescriptor::Range { range, layout_kind } => {
                let count = range.slot_count();
                if count == 0 {
                    return;
                }
                if !(in_body(range.slot(0)) && in_body(range.slot(count - 1))) {
                    // Owner-qualified custody: the whole range, as before.
                    for i in 0..count {
                        visit(GcMutableSlot::new(range.slot(i), layout_kind));
                    }
                    return;
                }
                let mut visited = 0usize;
                for (start, end) in dirty_slot_ranges_for(range, dirty_pages, &mut scratch) {
                    visited += end - start;
                    for i in start..end {
                        visit(GcMutableSlot::new(range.slot(i), layout_kind));
                    }
                }
                clean_slots += count - visited;
            }
            GcMutableSlotDescriptor::PointerFreeRange(_) => {}
        }
    });
    clean_slots
}

/// The witness failed: the whole-owner walk found an entry on a page the
/// snapshot did not name. That is an old-to-young edge the write barrier never
/// recorded, which the next minor would have missed for any old object that
/// has no other dirty page; the bounded walk only stops hiding it for the ones
/// that do. Report it instead of collecting on.
#[cold]
#[inline(never)]
pub(super) fn bounded_restore_witness_failed(cycle_label: &str, missing: usize) -> ! {
    panic!(
        "restore_surviving_dirty_coverage ({cycle_label}): the whole-owner walk of {missing} \
         entr{} found a remembered child on a page the pre-clear dirty snapshot does not name. \
         An old parent's in-body slot held a young or malloc child without its page being \
         dirty: a store that skipped the write barrier. PERRY_GC_RESTORE_DIRTY_PAGES=0 restores \
         the whole-owner walk.",
        if missing == 1 { "y" } else { "ies" }
    );
}
