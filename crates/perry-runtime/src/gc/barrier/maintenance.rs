//! Remembered-set inspection, drain and clear helpers — split from
//! `barrier/mod.rs` for the 2000-line file-size gate (the #7830 recipe:
//! extract a cohesive function group into a sibling file, re-export
//! explicitly). No logic change.

use super::*;

pub fn remembered_set_size() -> usize {
    remembered_dirty_page_count() + REMEMBERED_SET.with(|s| s.borrow().len())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::gc) struct MaintenanceClearStep {
    pub(in crate::gc) done: bool,
    pub(in crate::gc) work_units: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RememberedSetClearSubphase {
    DirtyOldPages,
    ExternalDirtySlots,
    FallbackHeaders,
    Done,
}

pub(in crate::gc) struct RememberedSetClearState {
    subphase: RememberedSetClearSubphase,
}

impl RememberedSetClearState {
    pub(in crate::gc) fn new() -> Self {
        Self {
            subphase: RememberedSetClearSubphase::DirtyOldPages,
        }
    }

    pub(in crate::gc) fn step(&mut self, budget: usize) -> bool {
        self.step_counted(budget).done
    }

    pub(in crate::gc) fn step_counted(&mut self, budget: usize) -> MaintenanceClearStep {
        let mut work_units = 0usize;
        loop {
            match self.subphase {
                RememberedSetClearSubphase::DirtyOldPages => {
                    if dirty_old_pages_empty() {
                        self.subphase = RememberedSetClearSubphase::ExternalDirtySlots;
                        continue;
                    }
                    if work_units == budget {
                        break;
                    }
                    if clear_one_dirty_old_page() {
                        work_units = work_units.saturating_add(1);
                    }
                }
                RememberedSetClearSubphase::ExternalDirtySlots => {
                    if external_dirty_slot_headers_empty() {
                        self.subphase = RememberedSetClearSubphase::FallbackHeaders;
                        continue;
                    }
                    if work_units == budget {
                        break;
                    }
                    if clear_one_external_dirty_slot_header() {
                        work_units = work_units.saturating_add(1);
                    }
                }
                RememberedSetClearSubphase::FallbackHeaders => {
                    if fallback_remembered_set_empty() {
                        self.subphase = RememberedSetClearSubphase::Done;
                        continue;
                    }
                    if work_units == budget {
                        break;
                    }
                    if clear_one_fallback_remembered_header() {
                        work_units = work_units.saturating_add(1);
                    }
                }
                RememberedSetClearSubphase::Done => {
                    return MaintenanceClearStep {
                        done: true,
                        work_units,
                    };
                }
            }
        }
        MaintenanceClearStep {
            done: self.subphase == RememberedSetClearSubphase::Done,
            work_units,
        }
    }
}

fn dirty_old_pages_empty() -> bool {
    DIRTY_OLD_PAGES.with(|s| s.borrow().is_empty())
}

/// The **sole** path that removes a page from `DIRTY_OLD_PAGES`. Every other
/// touch of that set is an insert, a read, or the snapshot — which is why
/// #7187 Phase B's cache needs exactly one invalidation point on this side.
fn clear_one_dirty_old_page() -> bool {
    DIRTY_OLD_PAGES.with(|s| {
        let mut pages = s.borrow_mut();
        let Some(page) = pages.iter().next().copied() else {
            return false;
        };
        crate::arena::old_page_clear_dirty(page);
        pages.remove(&page);
        // DELIBERATELY redundant with `old_page_clear_dirty`, which invalidates
        // too (#7187 Phase B rule 2). The cache's invariant has two halves and
        // this line owns the modbuf one: an edit that stops the arena side from
        // invalidating — or a page whose metadata entry no longer exists, so
        // `old_page_clear_dirty` finds nothing to clear — must not silently
        // leave the cache asserting a page this function just removed. The cost
        // is one thread-local store on the cold clear path.
        super::dirty_page_cache::invalidate();
        true
    })
}

fn external_dirty_slot_headers_empty() -> bool {
    EXTERNAL_DIRTY_SLOT_PAGES.with(|s| s.borrow().is_empty())
}

fn clear_one_external_dirty_slot_header() -> bool {
    invalidate_external_dirty_slot_cache();
    EXTERNAL_DIRTY_SLOT_PAGES.with(|s| {
        let mut pages = s.borrow_mut();
        let Some(page) = pages.keys().next().copied() else {
            return false;
        };
        let remove_page = match pages.get_mut(&page) {
            Some(headers) => {
                headers.pop();
                headers.is_empty()
            }
            None => false,
        };
        if remove_page {
            pages.remove(&page);
        }
        true
    })
}

fn fallback_remembered_set_empty() -> bool {
    REMEMBERED_SET.with(|s| s.borrow().is_empty())
}

fn clear_one_fallback_remembered_header() -> bool {
    REMEMBERED_SET.with(|s| {
        let mut headers = s.borrow_mut();
        let Some(header) = headers.iter().next().copied() else {
            return false;
        };
        headers.remove(&header);
        true
    })
}

pub(in crate::gc) struct ConservativePinClearState {
    done: bool,
}

impl ConservativePinClearState {
    pub(in crate::gc) fn new() -> Self {
        Self { done: false }
    }

    pub(in crate::gc) fn step_counted(&mut self, budget: usize) -> MaintenanceClearStep {
        if self.done {
            return MaintenanceClearStep {
                done: true,
                work_units: 0,
            };
        }

        let mut work_units = 0usize;
        while work_units < budget {
            if clear_one_conservative_pin() {
                work_units = work_units.saturating_add(1);
            } else {
                self.done = true;
                break;
            }
        }

        if !self.done && conservative_pins_empty() {
            self.done = true;
        }

        MaintenanceClearStep {
            done: self.done,
            work_units,
        }
    }
}

fn conservative_pins_empty() -> bool {
    CONS_PINNED.with(|s| s.borrow().is_empty())
}

fn clear_one_conservative_pin() -> bool {
    CONS_PINNED.with(|s| {
        let mut pinned = s.borrow_mut();
        let Some(header) = pinned.iter().next().copied() else {
            return false;
        };
        pinned.remove(&header);
        true
    })
}

/// Gen-GC Phase C: clear the remembered set. Will be called by
/// minor GC after the rs-scan completes (Phase C3). Test-only
/// for now to enable test isolation.
pub fn remembered_set_clear() {
    let mut state = RememberedSetClearState::new();
    while !state.step(usize::MAX) {}
}

/// #7035: is `addr`'s old page currently in the remembered set?
pub(in crate::gc) fn dirty_now_for_addr(addr: usize) -> bool {
    DIRTY_OLD_PAGES.with(|s| {
        s.borrow()
            .contains(&crate::arena::generation_page_for_addr(addr))
    })
}

/// #7035: was `addr`'s old page EVER dirtied? Distinguishes "barrier never ran"
/// from "edge was recorded then lost".
pub(in crate::gc) fn ever_dirty_for_addr(addr: usize) -> bool {
    ever_dirty_old_page(crate::arena::generation_page_for_addr(addr))
}
