//! The per-parent weak-holder fact the copying minor's slot visit reads, the
//! slot visit itself, and its single decode of the visited word. Split out of
//! `gc/copying.rs` for the 2000-line lint.

use super::*;

/// Is this parent one of the weak-holder classes whose weak slots the copying
/// minor must not evacuate through?
///
/// The question is a property of the PARENT's class, but the collector asked
/// the per-SLOT question (`weakref::is_weak_target_trace_slot`) for every slot
/// of every object — an out-of-line call that re-reads `obj_type` and
/// `class_id` and then rejects on class. #10182 gave the full mark the
/// per-object read (`gc/trace.rs`); the copying minor never got it.
///
/// Read LAZILY by the callers: once per object is right only for objects that
/// actually have a slot to visit. See `scan_object_fields`.
///
/// Null-safe: `is_weak_holder_header` answers false for a null header, which
/// is the same answer the per-slot question gave.
#[inline]
pub(super) unsafe fn weak_holder_fact(header: *mut GcHeader) -> bool {
    #[cfg(test)]
    if copy_hoist_sabotage::forgetting_weak() {
        return false;
    }
    crate::weakref::is_weak_holder_header(header)
}

/// Test-only sabotage for [`weak_holder_fact`]: forgetting the per-object fact
/// must change what the collector does, or the hoist is documentation
/// (CLAUDE.md, a gate that cannot fail). Its witness is
/// `gc::tests::copy_slot_hoists`.
#[cfg(test)]
pub(crate) mod copy_hoist_sabotage {
    use std::cell::Cell;

    thread_local! {
        static FORGET_WEAK: Cell<bool> = const { Cell::new(false) };
    }

    #[inline]
    pub(crate) fn forgetting_weak() -> bool {
        FORGET_WEAK.with(Cell::get)
    }

    pub(crate) struct WeakGuard(bool);

    impl WeakGuard {
        pub(crate) fn arm() -> Self {
            Self(FORGET_WEAK.with(|s| s.replace(true)))
        }
    }

    impl Drop for WeakGuard {
        fn drop(&mut self) {
            FORGET_WEAK.with(|s| s.set(self.0));
        }
    }
}

/// Test-only sabotage for the single decode per slot visit
/// (`visit_value_bits_child`). Witness: `gc::tests::copy_slot_decode`.
#[cfg(test)]
pub(crate) mod copy_decode_sabotage {
    use std::cell::Cell;

    /// The validated raw word is dropped instead of marked.
    pub(crate) const RAW_MARK: u8 = 1;
    /// The remembering arm loses the child the visit decoded.
    pub(crate) const CHILD: u8 = 2;

    thread_local! {
        static FORGET: Cell<u8> = const { Cell::new(0) };
    }

    pub(crate) fn forgetting(what: u8) -> bool {
        FORGET.with(|f| f.get() & what != 0)
    }

    pub(crate) struct Guard(u8);

    impl Guard {
        pub(crate) fn arm(what: u8) -> Self {
            Self(FORGET.with(|f| f.replace(f.get() | what)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            FORGET.with(|f| f.set(self.0));
        }
    }
}

impl CopyingNurseryCollector {
    /// The root visitors' form of [`Self::visit_value_bits_child`]. Always
    /// inlined too: out of line it added a call frame per root word.
    #[inline(always)]
    pub(super) fn visit_value_bits(&mut self, bits: u64) -> Option<u64> {
        self.visit_value_bits_child(bits)?.1
    }

    /// Decode, classify and mark one value word ONCE: the child's address as
    /// the word reads after this visit, the word's new bits if the child
    /// moved, and whether the word was raw. `None` is exactly
    /// `CopyingPointerSet::decode_bits`'s `None`: not a heap reference.
    ///
    /// A raw word used to be classified twice — by `decode_bits`, only to
    /// validate it, and again by `mark_addr`. Every traced shaped object visits
    /// its shape record's `keys` word, a raw address, so that was a second
    /// page-table probe and header read per traced object. The validating
    /// classification is now the one the mark uses; the memo is still
    /// consulted after it, as before.
    ///
    /// Always inlined: out of line, the extra frame and the by-memory return
    /// of the triple cost as much as the classification it saves.
    #[inline(always)]
    pub(super) fn visit_value_bits_child(
        &mut self,
        bits: u64,
    ) -> Option<(usize, Option<u64>, bool)> {
        let tag = bits & TAG_MASK;
        if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
            let addr = (bits & POINTER_MASK) as usize;
            if addr == 0 {
                return None;
            }
            // An unclassifiable NaN-boxed word is still a reference to the
            // remembering arm, which never classified NaN-boxed words.
            return Some(match self.mark_addr(addr) {
                Some(new_addr) if new_addr != addr => {
                    let new_bits = tag | (new_addr as u64 & POINTER_MASK);
                    ((new_bits & POINTER_MASK) as usize, Some(new_bits), false)
                }
                _ => (addr, None, false),
            });
        }
        if tag >= 0x7FF8_0000_0000_0000 || !CopyingPointerSet::raw_pointer_candidate(bits) {
            return None;
        }
        let addr = bits as usize;
        let ptr = self.ptrs.classify(addr)?;
        #[cfg(test)]
        if copy_decode_sabotage::forgetting(copy_decode_sabotage::RAW_MARK) {
            return None;
        }
        let new_addr = self.mark_classified_addr(addr, ptr);
        Some((
            new_addr,
            (new_addr != addr).then_some(new_addr as u64),
            true,
        ))
    }

    /// The remembering arm's one remaining re-decode: a raw word that moved.
    /// Out of line so the rare case does not grow the slot visit it sits in.
    #[inline(never)]
    fn revalidate_moved_raw(&self, bits: u64) -> Option<usize> {
        self.ptrs.decode_bits(bits).map(|(addr, _, _)| addr)
    }

    pub(super) unsafe fn visit_slot_with_parent(
        &mut self,
        slot: *mut u64,
        parent_header: *mut GcHeader,
        external: bool,
    ) {
        let weak_holder = weak_holder_fact(parent_header);
        self.visit_slot_with_weak_fact(slot, parent_header, weak_holder, external);
    }

    /// [`visit_slot_with_parent`](Self::visit_slot_with_parent) with the
    /// parent's weak-holder fact supplied by the caller, so a whole object's
    /// slots pay for it once. See [`weak_holder_fact`].
    pub(super) unsafe fn visit_slot_with_weak_fact(
        &mut self,
        slot: *mut u64,
        parent_header: *mut GcHeader,
        weak_holder: bool,
        external: bool,
    ) {
        if slot.is_null() {
            return;
        }
        // Weak target edge (WeakRef referent / weak entry key / finreg
        // record target): never evacuate through it — the mark/barrier
        // paths skip these (`is_weak_target_trace_slot`), and copying
        // through them strengthened the reference, so WeakMap entries
        // never tombstoned and FinalizationRegistry never fired while
        // copied-minor was the operative cycle. Repair an already-moved
        // target's address now and queue the slot so `repair_weak_slots`
        // fixes targets evacuated after this visit; the registry pass then
        // tombstones dead ones.
        // No remembered-set entry either — the write barrier skips weak
        // slots the same way.
        if weak_holder && crate::weakref::is_weak_target_trace_slot(parent_header, slot) {
            if let Some(new_bits) = self.rewrite_value_bits(*slot) {
                *slot = new_bits;
            }
            self.weak_slots.push(slot);
            return;
        }
        // Asked BEFORE the visit: it reads only the parent and the slot's own
        // address, never the child. Asked after, the optimizer duplicated the
        // call into both decode arms and then stopped inlining it.
        let remembering = !parent_header.is_null()
            && !self.skip_remembering
            && barrier_parent_needs_remembering(
                (parent_header as *mut u8).add(GC_HEADER_SIZE) as usize,
                external,
            );
        let visited = self.visit_value_bits_child(*slot);
        if let Some((_, Some(new_bits), _)) = visited {
            *slot = new_bits;
        }
        if !remembering {
            return;
        }
        // The visit above already decoded this word; re-decoding `*slot`
        // repeated it. Only a raw word that MOVED is validated again, which is
        // all the re-decode could still reject.
        let child = match visited {
            Some((_, Some(new_bits), true)) => self.revalidate_moved_raw(new_bits),
            other => other.map(|(addr, _, _)| addr).filter(|&addr| addr != 0),
        };
        #[cfg(test)]
        let child =
            child.filter(|_| !copy_decode_sabotage::forgetting(copy_decode_sabotage::CHILD));
        if let Some(child_addr) = child {
            // Keep old→malloc pages dirty alongside old→nursery: the malloc
            // child is spared by this cycle's mark (mark_addr handles
            // CopyingPointerKind::Malloc) but the NEXT minor's malloc sweep
            // needs the edge again.
            if crate::gc::barrier::remembered_child_needs_tracking(child_addr) {
                self.sticky.remember_slot(parent_header, slot, external);
            }
        }
    }
}
