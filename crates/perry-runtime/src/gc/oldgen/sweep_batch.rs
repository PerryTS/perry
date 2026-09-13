//! Batched page-index removal for the dead old objects a sweep step frees.
//!
//! `invalidate_dead_old_arena_header` unregisters each dead old object from the
//! page index on its own, and that removal is quadratic in objects per page (see
//! `arena::unregister_old_objects_batch`). A full collection frees whole pages
//! of small objects, so the sweep queues them here and removes a step's worth in
//! one pass. The header fields are still invalidated immediately, exactly as
//! before, so no walker can read a dead header as a live object in between.

use super::super::*;

/// Flush once this many dead headers are queued, so a single unbudgeted sweep
/// of a large heap never stages an unbounded buffer.
const FLUSH_AT: usize = 4096;

pub(super) struct PendingOldUnregister {
    dead: Vec<(usize, usize)>,
    scratch: Vec<(usize, usize, usize)>,
    /// `(first page, last page)` of the last dead header whose described
    /// promoted runs were expanded: consecutive dead objects on one page expand
    /// it once.
    expanded_pages: (usize, usize),
}

impl Default for PendingOldUnregister {
    fn default() -> Self {
        Self {
            dead: Vec::new(),
            scratch: Vec::new(),
            expanded_pages: (usize::MAX, usize::MAX),
        }
    }
}

impl PendingOldUnregister {
    /// Invalidate a dead old header now and queue its page-index removal.
    ///
    /// # Safety
    ///
    /// `header` must be a dead old-gen arena header of `total_size` bytes.
    pub(super) unsafe fn defer(&mut self, header: *mut GcHeader, total_size: usize) {
        // #10182: a described promoted page is re-parsed by header type when it
        // is expanded, so it must be expanded BEFORE this header stops parsing
        // as an object. Expanded after, the list would silently lack the dead
        // object, the batched removal below would not find it, and the page's
        // `allocated_bytes`/`object_count` would keep counting it.
        #[cfg(test)]
        let expand = super::super::trace::block_skip::sabotage::get()
            & super::super::trace::block_skip::sabotage::FORGET_RUN_EXPANSION
            == 0;
        #[cfg(not(test))]
        let expand = true;
        if expand {
            let pages = (
                crate::arena::generation_page_for_addr(header as usize),
                crate::arena::generation_page_for_addr(header as usize + total_size - 1),
            );
            if pages != self.expanded_pages {
                crate::arena::materialize_promoted_page_runs_for_object(
                    header as usize,
                    total_size,
                );
                self.expanded_pages = pages;
            }
        }
        (*header).obj_type = 0;
        (*header).gc_flags = 0;
        (*header)._reserved = 0;
        self.dead.push((header as usize, total_size));
        if self.dead.len() >= FLUSH_AT {
            self.flush();
        }
    }

    /// Will the next `defer` flush the queue (and so zero the sweep accounting
    /// of every page whose last object it removes)?
    #[inline]
    pub(super) fn flushes_on_next_defer(&self) -> bool {
        self.dead.len() + 1 >= FLUSH_AT
    }

    /// Remove every queued header from the page index.
    pub(super) fn flush(&mut self) {
        if self.dead.is_empty() {
            return;
        }
        crate::arena::unregister_old_objects_batch(&self.dead, &mut self.scratch);
        self.dead.clear();
    }
}
