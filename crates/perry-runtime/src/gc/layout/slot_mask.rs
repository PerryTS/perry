//! The per-object slot-mask representation used by the GC layout tables.
//!
//! Split out of `gc/layout.rs` for the 2000-line file cap (#7809/#7812 took it
//! to 2023). Pure code move — no logic change.

#[derive(Clone, PartialEq, Eq)]
pub(in crate::gc) enum LayoutSlotMask {
    Inline(u64),
    Heap(Vec<u64>),
    /// Every currently-live slot is pointer-bearing. This is useful for
    /// runtime-produced arrays such as `String.prototype.split` results: the
    /// array grows its visible length only after each string has been stored,
    /// so the collector can visit `0..length` directly without allocating or
    /// updating a side-table bit for every element.
    AllPointers,
}

impl LayoutSlotMask {
    pub(in crate::gc) fn from_words(words: &[u64]) -> Self {
        let mut trimmed = words.len();
        while trimmed > 0 && words[trimmed - 1] == 0 {
            trimmed -= 1;
        }
        match trimmed {
            0 => LayoutSlotMask::Inline(0),
            1 => LayoutSlotMask::Inline(words[0]),
            _ => LayoutSlotMask::Heap(words[..trimmed].to_vec()),
        }
    }

    #[inline]
    pub(in crate::gc) fn set_slot(&mut self, slot_index: usize) {
        match self {
            LayoutSlotMask::Inline(bits) if slot_index < 64 => {
                *bits |= 1u64 << slot_index;
            }
            LayoutSlotMask::Inline(bits) => {
                let mut words = vec![0; slot_index / 64 + 1];
                words[0] = *bits;
                words[slot_index / 64] |= 1u64 << (slot_index % 64);
                *self = LayoutSlotMask::Heap(words);
            }
            LayoutSlotMask::Heap(words) => {
                let word = slot_index / 64;
                if words.len() <= word {
                    words.resize(word + 1, 0);
                }
                words[word] |= 1u64 << (slot_index % 64);
            }
            LayoutSlotMask::AllPointers => {}
        }
    }

    #[inline]
    pub(in crate::gc) fn clear_slot(&mut self, slot_index: usize) {
        match self {
            LayoutSlotMask::Inline(bits) if slot_index < 64 => {
                *bits &= !(1u64 << slot_index);
            }
            LayoutSlotMask::Inline(_) => {}
            LayoutSlotMask::Heap(words) => {
                let word = slot_index / 64;
                if word < words.len() {
                    words[word] &= !(1u64 << (slot_index % 64));
                    while words.last().copied() == Some(0) {
                        words.pop();
                    }
                    if words.len() == 1 {
                        *self = LayoutSlotMask::Inline(words[0]);
                    }
                }
            }
            // `layout_note_slot` must downgrade an all-pointer layout before
            // clearing a slot, because this variant intentionally stores no
            // per-slot bitmap from which to reconstruct the remaining set.
            LayoutSlotMask::AllPointers => {
                unreachable!("all-pointer layouts must be downgraded before clearing a slot")
            }
        }
    }

    #[inline]
    pub(in crate::gc) fn is_empty(&self) -> bool {
        match self {
            LayoutSlotMask::Inline(bits) => *bits == 0,
            LayoutSlotMask::Heap(words) => words.iter().all(|&w| w == 0),
            LayoutSlotMask::AllPointers => false,
        }
    }

    pub(in crate::gc) fn visit_slots<F: FnMut(usize)>(&self, slot_count: usize, mut visit: F) {
        match self {
            LayoutSlotMask::Inline(bits) => {
                let limit = slot_count.min(64);
                let mask = if limit == 64 {
                    u64::MAX
                } else if limit == 0 {
                    0
                } else {
                    (1u64 << limit) - 1
                };
                let mut word = *bits & mask;
                while word != 0 {
                    let bit = word.trailing_zeros() as usize;
                    visit(bit);
                    word &= word - 1;
                }
            }
            LayoutSlotMask::Heap(words) => {
                let word_count = slot_count.div_ceil(64);
                for (word_index, &raw_word) in words.iter().take(word_count).enumerate() {
                    let remaining = slot_count.saturating_sub(word_index * 64);
                    let limit = remaining.min(64);
                    let mask = if limit == 64 {
                        u64::MAX
                    } else if limit == 0 {
                        0
                    } else {
                        (1u64 << limit) - 1
                    };
                    let mut word = raw_word & mask;
                    while word != 0 {
                        let bit = word.trailing_zeros() as usize;
                        visit(word_index * 64 + bit);
                        word &= word - 1;
                    }
                }
            }
            LayoutSlotMask::AllPointers => {
                for slot in 0..slot_count {
                    visit(slot);
                }
            }
        }
    }

    pub(in crate::gc) fn count_slots(&self, slot_count: usize) -> usize {
        let mut count = 0usize;
        self.visit_slots(slot_count, |_| {
            count += 1;
        });
        count
    }

    /// Reference implementation only. The construction path asks this question
    /// of the raw mask words (`shape_install::words_intersect`) rather than of
    /// two built masks — see that module's "mask words" section — and
    /// `shape_install::tests::mask_word_helpers_agree_with_layout_slot_mask`
    /// pins the two together, which is what this now exists for.
    #[cfg(test)]
    pub(in crate::gc) fn intersects(&self, other: &Self, slot_count: usize) -> bool {
        let mut found = false;
        self.visit_slots(slot_count, |slot| {
            if other.contains_slot(slot) {
                found = true;
            }
        });
        found
    }

    #[inline]
    pub(in crate::gc) fn contains_slot(&self, slot_index: usize) -> bool {
        match self {
            LayoutSlotMask::Inline(bits) if slot_index < 64 => (*bits & (1u64 << slot_index)) != 0,
            LayoutSlotMask::Inline(_) => false,
            LayoutSlotMask::Heap(words) => {
                let word = slot_index / 64;
                word < words.len() && (words[word] & (1u64 << (slot_index % 64))) != 0
            }
            LayoutSlotMask::AllPointers => true,
        }
    }

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
            LayoutSlotMask::Heap(words) => {
                let mut word_index = cursor / 64;
                let word_count = slot_count.div_ceil(64);
                while word_index < word_count && word_index < words.len() {
                    let word_start = word_index * 64;
                    let remaining = slot_count.saturating_sub(word_start);
                    let limit = remaining.min(64);
                    let limit_mask = if limit == 64 {
                        u64::MAX
                    } else if limit == 0 {
                        0
                    } else {
                        (1u64 << limit) - 1
                    };
                    let cursor_mask = if word_index == cursor / 64 {
                        u64::MAX << (cursor % 64)
                    } else {
                        u64::MAX
                    };
                    let word = words[word_index] & limit_mask & cursor_mask;
                    if word != 0 {
                        return Some(word_start + word.trailing_zeros() as usize);
                    }
                    word_index += 1;
                }
                None
            }
            LayoutSlotMask::AllPointers => (cursor < slot_count).then_some(cursor),
        }
    }
}
