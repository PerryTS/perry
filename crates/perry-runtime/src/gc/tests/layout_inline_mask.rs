//! The inline-mask walk that replaced the per-slot `HeapChildSlotIterator::next`
//! call in `visit_gc_layout_slot_descriptors` must enumerate EXACTLY what the
//! iterator enumerates, for every mask shape it claims. That is a property, so
//! it is tested as one — over every edge case of the mask word and of the live
//! slot count, and over a deterministic pseudo-random sample — and each
//! property has a sabotaged twin that must fail.

use super::super::layout::{
    HeapChildSlot, HeapChildSlotIterator, HeapPayloadSlotSelection, HeapSlotRange, LayoutSlotMask,
};
use super::super::layout_slot_visit::inline_mask_sabotage;
use super::super::*;
use super::support::*;

/// Every mask word worth a case: empty, one bit at each end, full width, both
/// alternations, a few hand-picked sparse shapes, and 64 pseudo-random words
/// from a fixed LCG so the sample is the same on every run.
fn mask_words() -> Vec<u64> {
    let mut words = vec![
        0,
        1,
        0b10,
        0b1011,
        1 << 31,
        1 << 62,
        1 << 63,
        (1 << 63) | 1,
        u64::MAX,
        u64::MAX >> 1,
        0xAAAA_AAAA_AAAA_AAAA,
        0x5555_5555_5555_5555,
        0xFFFF_FFFF,
        0xFFFF_FFFF_0000_0000,
    ];
    let mut state: u64 = 0x2545_F491_4F6C_DD1D;
    for _ in 0..64 {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        words.push(state);
    }
    words
}

/// Live payload slot counts: zero, the small shapes, both sides of every word
/// boundary, the maximum inline width (64) and past it.
fn slot_counts() -> Vec<usize> {
    vec![0, 1, 2, 3, 7, 8, 31, 32, 33, 63, 64, 65, 96, 128]
}

fn iterator_for(bits: u64, slots: &mut [u64]) -> HeapChildSlotIterator {
    HeapChildSlotIterator {
        prefix_slot: None,
        meta_slot: None,
        meta_slot2: None,
        payload: HeapSlotRange::new(slots.as_mut_ptr(), slots.len()),
        selection: HeapPayloadSlotSelection::Masked {
            mask: LayoutSlotMask::Inline(bits),
            cursor: 0,
            raw_numeric_object_slots: 0,
            raw_numeric_recorded: true,
        },
        object_shape: None,
    }
}

/// What the iterator yields: the slot INDEX of every `Child`, in order.
fn iterated_indices(bits: u64, slots: &mut [u64]) -> Vec<usize> {
    let base = slots.as_mut_ptr();
    let mut out = Vec::new();
    for child in &mut iterator_for(bits, slots) {
        match child {
            HeapChildSlot::Child(slot, kind) => {
                assert_eq!(kind, HeapChildSlotReadKind::Masked, "masked payload slot");
                out.push((slot as usize - base as usize) / std::mem::size_of::<u64>());
            }
            other => panic!("a masked payload yields only children, got {other:?}"),
        }
    }
    out
}

/// What the walk yields: the set bits of the word it hands the visitor, in the
/// order `trailing_zeros` + `word &= word - 1` produces.
fn walked_indices(bits: u64, slots: &mut [u64]) -> Vec<usize> {
    let mut word = iterator_for(bits, slots)
        .take_inline_mask_word()
        .expect("an inline mask must take the walk");
    let mut out = Vec::new();
    while word != 0 {
        out.push(word.trailing_zeros() as usize);
        word &= word - 1;
    }
    out
}

fn disagreements() -> Vec<(u64, usize)> {
    let mut bad = Vec::new();
    for bits in mask_words() {
        for count in slot_counts() {
            let mut slots = vec![0u64; count];
            if iterated_indices(bits, &mut slots) != walked_indices(bits, &mut slots) {
                bad.push((bits, count));
            }
        }
    }
    bad
}

#[test]
fn the_inline_walk_enumerates_exactly_what_the_iterator_enumerates() {
    let bad = disagreements();
    assert!(
        bad.is_empty(),
        "the inline walk and `next` must agree for every mask and live slot \
         count; they disagreed on {} of {} cases, first {:?}",
        bad.len(),
        mask_words().len() * slot_counts().len(),
        bad.first()
    );
}

#[test]
fn every_index_the_walk_yields_is_live_and_set_in_the_mask() {
    for bits in mask_words() {
        for count in slot_counts() {
            let mut slots = vec![0u64; count];
            for index in walked_indices(bits, &mut slots) {
                assert!(
                    index < count,
                    "walked slot {index} is past the live count {count}"
                );
                assert!(
                    bits & (1u64 << index) != 0,
                    "walked slot {index} is not in the mask"
                );
            }
        }
    }
}

#[test]
fn a_sabotaged_walk_is_caught_by_the_property() {
    let _sabotage = inline_mask_sabotage::Guard::arm(inline_mask_sabotage::DROP_TOP);
    let bad = disagreements();
    assert!(
        !bad.is_empty(),
        "with the mask's top slot dropped the walk must disagree with `next`; \
         a property that cannot see that proves nothing"
    );
}

/// The walk is the arm a real collection takes: a young string reachable only
/// through the HIGHEST masked slot of a rooted young array must survive a
/// copying minor, and must not when that slot is dropped.
fn top_masked_slot_child_survives(sabotaged: bool) -> bool {
    std::thread::spawn(move || {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _scan = ConservativeScanDisabledGuard::new();
        let _roots = ShadowAndGlobalRootResetGuard;
        const LEN: usize = 6;
        let top = LEN - 1;
        let arr = crate::array::js_array_alloc_with_length(LEN as u32);
        let child = young_leaf();
        // Numbers everywhere else, so the mask holds exactly the top slot: a
        // walk that loses its top bit loses this child and nothing else.
        for index in 0..top {
            crate::array::js_array_set_f64(arr, index as u32, index as f64);
        }
        crate::array::js_array_set_f64(arr, top as u32, f64::from_bits(string_bits(child)));
        js_shadow_slot_set(0, ptr_bits(arr as usize));

        // Premise: this array's payload really is an inline-masked selection
        // whose only bit is the top slot. Without it the collection below takes
        // the general arm and proves nothing about the walk.
        let word = unsafe {
            let header = header_from_user_ptr(arr as *const u8) as *mut GcHeader;
            crate::gc::layout::gc_child_slots(header).take_inline_mask_word()
        };
        assert_eq!(
            word,
            Some(1u64 << top),
            "premise: the fixture must produce an inline mask holding only slot {top}"
        );

        {
            let _sabotage =
                sabotaged.then(|| inline_mask_sabotage::Guard::arm(inline_mask_sabotage::DROP_TOP));
            let _ = gc_collect_minor();
        }
        let arr_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
        assert_ne!(arr_after, arr as usize, "premise: the rooted array moved");
        let slot = unsafe {
            *crate::array::gc_element_slot_range(arr_after as *mut crate::array::ArrayHeader)
                .expect("the array must still enumerate its elements")
                .slot(top)
        };
        (slot & POINTER_MASK) as usize != child
    })
    .join()
    .expect("inline-mask collection test thread must not panic")
}

#[test]
fn the_top_masked_slots_child_is_evacuated_through_the_walk() {
    assert!(
        top_masked_slot_child_survives(false),
        "the child in the highest masked slot must be evacuated and the slot rewritten"
    );
}

#[test]
fn sabotaging_the_walks_top_slot_strands_its_child() {
    assert!(
        !top_masked_slot_child_survives(true),
        "with the mask's top slot dropped the child is never visited, so the \
         slot still names from-space"
    );
}
