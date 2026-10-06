//! Register-sized shape selection and the header all-pointer selection.
//!
//! Contains no storage keyed by an owner or an address.

#[derive(Clone, PartialEq, Eq)]
pub(in crate::gc) enum LayoutSlotMask {
    Inline(u64),
    /// Every currently-live slot is pointer-bearing. This is useful for
    /// runtime-produced arrays such as `String.prototype.split` results: the
    /// array grows its visible length only after each string has been stored,
    /// so the collector can visit `0..length` directly without allocating or
    /// updating a side-table bit for every element.
    AllPointers,
}

impl LayoutSlotMask {
    pub(in crate::gc) fn next_slot_at_or_after(
        &self,
        cursor: usize,
        slot_count: usize,
    ) -> Option<usize> {
        if cursor >= slot_count {
            return None;
        }
        match self {
            LayoutSlotMask::Inline(bits) => {
                if cursor >= 64 {
                    return None;
                }
                let limit = slot_count.min(64);
                let limit_mask = if limit == 64 {
                    u64::MAX
                } else if limit == 0 {
                    0
                } else {
                    (1u64 << limit) - 1
                };
                let cursor_mask = u64::MAX << cursor;
                let word = *bits & limit_mask & cursor_mask;
                (word != 0).then(|| word.trailing_zeros() as usize)
            }
            LayoutSlotMask::AllPointers => (cursor < slot_count).then_some(cursor),
        }
    }
}
