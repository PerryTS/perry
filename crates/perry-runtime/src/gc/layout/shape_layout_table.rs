//! `SHAPE_LAYOUTS` with a one-entry memo of its last pointer-mask answer
//! (#11549).
//!
//! Every traced class instance asks the table for its shape's shared pointer
//! mask: a `RefCell` borrow, a hash probe and a mask clone, ~50 instructions
//! per object in a copying minor over a binary tree — for the SAME shape, object
//! after object. The memo replays the last answer instead.
//!
//! It is exact, not a cache that can go stale, because it is cleared by
//! construction: the map is reachable mutably ONLY through `DerefMut`, and
//! `deref_mut` empties the memo before handing the map out. Insert, poison
//! (`Some` → `None`), `entry`, `iter_mut`, `clear` — every write path goes
//! through it. Replacing the whole table replaces the memo with it. So a
//! memoized answer is always the answer the map would give now.
//! `gc::tests::shape_layout_table` pins that, with a sabotaged twin that keeps
//! the memo across a write.

use super::{LayoutSlotMask, TypedLayoutDescriptor};
use std::cell::Cell;

type ShapeLayoutMap = crate::fast_hash::PtrHashMap<u32, Option<TypedLayoutDescriptor>>;

/// The empty memo. ShapeId 0 is "unstamped" and never looked up here.
const NO_MEMO: (u32, usize, u64) = (0, 0, 0);

pub(in crate::gc) struct ShapeLayoutTable {
    map: ShapeLayoutMap,
    /// `(shape_id, descriptor slot_count, inline pointer mask)` of the last
    /// `Some` descriptor with a one-word pointer mask that
    /// [`Self::shared_pointer_mask`] read.
    memo: Cell<(u32, usize, u64)>,
}

impl ShapeLayoutTable {
    pub(in crate::gc) fn new() -> Self {
        Self {
            map: crate::fast_hash::new_ptr_hash_map(),
            memo: Cell::new(NO_MEMO),
        }
    }

    /// `shape_id`'s shared pointer mask for a receiver whose live inline bound
    /// is `field_count`: exactly
    /// `map.get(&shape_id)?.as_ref()` filtered to `slot_count == field_count`,
    /// with the mask cloned out.
    #[inline]
    pub(in crate::gc) fn shared_pointer_mask(
        &self,
        shape_id: u32,
        field_count: usize,
    ) -> Option<LayoutSlotMask> {
        let (memo_id, memo_count, memo_bits) = self.memo.get();
        if memo_id == shape_id && shape_id != 0 {
            return (memo_count == field_count).then_some(LayoutSlotMask::Inline(memo_bits));
        }
        let desc = self.map.get(&shape_id)?.as_ref()?;
        if let LayoutSlotMask::Inline(bits) = desc.pointer_mask {
            if shape_id != 0 {
                self.memo.set((shape_id, desc.slot_count, bits));
            }
        }
        if desc.slot_count != field_count {
            return None;
        }
        Some(desc.pointer_mask.clone())
    }
}

impl std::ops::Deref for ShapeLayoutTable {
    type Target = ShapeLayoutMap;

    #[inline]
    fn deref(&self) -> &ShapeLayoutMap {
        &self.map
    }
}

impl std::ops::DerefMut for ShapeLayoutTable {
    /// The ONLY way to the map mutably, and it forgets the memo first.
    #[inline]
    fn deref_mut(&mut self) -> &mut ShapeLayoutMap {
        #[cfg(test)]
        if sabotage::keeping_memo() {
            return &mut self.map;
        }
        self.memo.set(NO_MEMO);
        &mut self.map
    }
}

/// Test-only sabotage: a write that keeps the memo. Witness:
/// `gc::tests::shape_layout_table`.
#[cfg(test)]
pub(crate) mod sabotage {
    use std::cell::Cell;

    thread_local! {
        static KEEP_MEMO: Cell<bool> = const { Cell::new(false) };
    }

    pub(super) fn keeping_memo() -> bool {
        KEEP_MEMO.with(Cell::get)
    }

    pub(crate) struct KeepMemo(bool);

    impl KeepMemo {
        pub(crate) fn arm() -> Self {
            Self(KEEP_MEMO.with(|c| c.replace(true)))
        }
    }

    impl Drop for KeepMemo {
        fn drop(&mut self) {
            KEEP_MEMO.with(|c| c.set(self.0));
        }
    }
}

#[cfg(test)]
pub(crate) fn test_descriptor(slot_count: usize, pointer_bits: u64) -> TypedLayoutDescriptor {
    TypedLayoutDescriptor {
        slot_count,
        raw_f64_mask: LayoutSlotMask::Inline(0),
        pointer_mask: LayoutSlotMask::Inline(pointer_bits),
    }
}
