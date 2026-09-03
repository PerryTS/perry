//! Write-barrier side-table census rows (#9637 `PERRY_GC_CENSUS`), split
//! from `barrier/mod.rs` for the 2000-line file cap.

use super::*;

/// `PERRY_GC_CENSUS`: remembered-set / dirty-page tables.
pub(crate) fn barrier_tables_census() -> Vec<crate::gc::census::SideTableRow> {
    use crate::gc::census::{map_bytes, set_bytes, vec_bytes};
    let mut rows = Vec::new();
    DIRTY_OLD_PAGES.with(|s| {
        let s = s.borrow();
        rows.push(("gc.dirty_old_pages", s.len(), set_bytes(&s)));
    });
    EXTERNAL_DIRTY_SLOT_PAGES.with(|m| {
        let m = m.borrow();
        let inner: usize = m.values().map(vec_bytes).sum();
        rows.push((
            "gc.external_dirty_slot_pages",
            m.len(),
            map_bytes(&m) + inner,
        ));
    });
    REMEMBERED_SET.with(|s| {
        let s = s.borrow();
        rows.push(("gc.remembered_set", s.len(), set_bytes(&s)));
    });
    EVER_DIRTY_OLD_PAGES.with(|s| {
        let s = s.borrow();
        rows.push(("gc.ever_dirty_old_pages", s.len(), set_bytes(&s)));
    });
    rows
}
