//! Minor-cycle alloc-flag restore, split from `cycle.rs` for the
//! 2000-line file cap (#9644 added the idle-compaction path).

use super::*;

pub(crate) fn restore_minor_in_alloc(prev_in_alloc: u8) {
    GC_FLAGS.with(|f| {
        let cur = f.get();
        if prev_in_alloc != 0 {
            f.set(cur | GC_FLAG_IN_ALLOC);
        } else {
            f.set(cur & !GC_FLAG_IN_ALLOC);
        }
    });
}
