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

#[derive(Default)]
pub(super) struct PendingOldUnregister {
    dead: Vec<(usize, usize)>,
    scratch: Vec<(usize, usize, usize)>,
}

impl PendingOldUnregister {
    /// Invalidate a dead old header now and queue its page-index removal.
    ///
    /// # Safety
    ///
    /// `header` must be a dead old-gen arena header of `total_size` bytes.
    pub(super) unsafe fn defer(&mut self, header: *mut GcHeader, total_size: usize) {
        (*header).obj_type = 0;
        (*header).gc_flags = 0;
        (*header)._reserved = 0;
        self.dead.push((header as usize, total_size));
        if self.dead.len() >= FLUSH_AT {
            self.flush();
        }
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
