//! `SHAPE_LAYOUTS`' one-entry pointer-mask memo (`gc/layout/shape_layout_table.rs`)
//! must never answer differently from the map. Every write reaches the map
//! through `DerefMut`, which forgets the memo; the sabotaged twin keeps it and
//! must be caught returning the answer from before the write.

use crate::gc::layout::shape_layout_table::{sabotage, test_descriptor};
use crate::gc::layout::{LayoutSlotMask, ShapeLayoutTable};

const SHAPE: u32 = 7;

/// Memoize SHAPE's mask, then run each kind of write and ask again. Returns
/// the answers after each write.
fn answers_across_writes(sabotaged: bool) -> Vec<Option<LayoutSlotMask>> {
    let _sabotage = sabotaged.then(sabotage::KeepMemo::arm);
    let mut table = ShapeLayoutTable::new();
    table.insert(SHAPE, Some(test_descriptor(3, 0b101)));
    let mut answers = Vec::new();
    // Populate the memo, and read it back once from the memo.
    answers.push(table.shared_pointer_mask(SHAPE, 3));
    answers.push(table.shared_pointer_mask(SHAPE, 3));
    // A field-count mismatch is answered from the memo too.
    answers.push(table.shared_pointer_mask(SHAPE, 2));
    // Poison: the shape became ambiguous.
    table.insert(SHAPE, None);
    answers.push(table.shared_pointer_mask(SHAPE, 3));
    // Re-learned with a different layout through `entry`.
    table.remove(&SHAPE);
    let _ = table.shared_pointer_mask(SHAPE, 3);
    table
        .entry(SHAPE)
        .or_insert(Some(test_descriptor(3, 0b011)));
    answers.push(table.shared_pointer_mask(SHAPE, 3));
    answers
}

fn expected() -> Vec<Option<LayoutSlotMask>> {
    vec![
        Some(LayoutSlotMask::Inline(0b101)),
        Some(LayoutSlotMask::Inline(0b101)),
        None,
        None,
        Some(LayoutSlotMask::Inline(0b011)),
    ]
}

#[test]
fn the_shape_mask_memo_always_answers_what_the_map_answers() {
    assert!(answers_across_writes(false) == expected());
}

#[test]
fn a_shape_mask_memo_kept_across_a_write_answers_stale() {
    assert!(
        answers_across_writes(true) != expected(),
        "a memo that survives a write must be caught answering from before it"
    );
}
