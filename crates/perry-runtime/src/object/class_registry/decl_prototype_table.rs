//! The declared-class `.prototype` registry, with its reverse index built in.
//!
//! # Why this is a type and not a `HashMap`
//!
//! `class_id_for_decl_prototype_object` answers "which declared class's
//! `.prototype` is this heap object?". Its callers
//! (`descriptor_state::disable_inline_guards_for_descriptor_target`,
//! `Object.getOwnPropertyDescriptor`, `delete`, the proxy and
//! `Reflect.metadata` paths) ask it about ARBITRARY objects, so the common
//! answer is "none". Until #9180 that answer cost a linear scan of every
//! materialized declared-class prototype, and on a large application — where
//! esbuild-style `__export(exports, { … })` runs `Object.defineProperty`
//! thousands of times during module init — the scan was 3.10% of `cc --help`.
//!
//! The obvious repair is a pointer-keyed reverse cache invalidated at the
//! writers. That was tried and it was wrong: the table has six mutation sites
//! (the store, the two GC root scanners, the per-slot GC step, the test reset
//! and the test seeds), a missed one leaves the cache stale, and a stale
//! reverse lookup does not crash — it silently reports "not a prototype", so
//! `getOwnPropertyDescriptor(C.prototype, "g")` starts returning `undefined`
//! where node returns an accessor descriptor.
//!
//! So the invalidation here is not maintained by diligence. Three properties
//! carry it, in decreasing order of how much they are asked to do.
//!
//! ## 1. Privacy — a seventh writer cannot forget
//!
//! `forward` and `reverse` are private to THIS FILE. No other module can name
//! them, so no other module can insert, remove, or rewrite an address; the
//! only ways in are the handful of methods below, and each updates both
//! directions in one statement sequence. Adding a mutation site now means
//! editing this file.
//!
//! ## 2. The fallback is the code that was already correct
//!
//! A targeted `reverse` update is an exact inverse only while `forward` is
//! *injective* — one prototype address per class id, never re-pointed. That
//! is how the registry is really built (every entry is a fresh
//! `js_object_alloc`, stored once, and afterwards only ever relocated by the
//! collector), but it is a precondition, not a theorem, and a precondition
//! that quietly stopped holding is exactly the failure mode above: with two
//! class ids on one address, retiring one entry drops the other's reverse
//! key and its lookups start missing.
//!
//! `insert` is the only place either half of that precondition can break, so it
//! checks both while inserting (with one extra reverse lookup), and when a
//! check trips it gives the index up for good. In that mode `class_id_for`
//! falls back to `scan_class_id_for`, the same authoritative forward-table
//! scan used before #9180.
//!
//! ## 3. The index is checked against its own ground truth
//!
//! In a `debug_assertions` build — i.e. throughout the runtime test suite —
//! every `class_id_for` compares the index against that same linear scan, and
//! every mutation re-checks the whole invariant. A mutation added here that
//! forgets `reverse` fails a test rather than shipping a wrong descriptor.
//!
//! # Why the GC cannot invalidate it either
//!
//! Evacuation rewrites the stored addresses through `visit_usize_slot`. Both
//! visit helpers below capture the slot's value, hand the slot to the
//! visitor, and re-key `reverse` from what the visitor left behind — so a
//! move updates both maps or neither.

use crate::fast_hash::PtrHashMap;

/// `class_id → *mut ObjectHeader` for materialized declared-class
/// prototypes, plus the pointer-keyed inverse.
#[derive(Default)]
pub struct DeclPrototypeTable {
    /// The authoritative direction. Values are raw addresses (`usize` so the
    /// table stays `Send + Sync`); they are GC roots, visited and rewritten
    /// by the scanners below.
    forward: PtrHashMap<u32, usize>,
    /// Exact inverse of `forward` while `reverse_index_abandoned` is false.
    /// Never consulted for anything `forward` does not already say.
    reverse: PtrHashMap<usize, u32>,
    /// Sticky: set when [`Self::insert`] sees something that would make
    /// `reverse` less than an exact inverse (a second class id claiming an
    /// address, or a class id re-pointed at a different object). From then on
    /// the reverse lookup falls back to the linear scan, which is always
    /// right. Never observed in practice — see the module docs.
    reverse_index_abandoned: bool,
}

impl DeclPrototypeTable {
    /// Register `class_id`'s prototype object.
    pub(crate) fn insert(&mut self, class_id: u32, ptr: usize) {
        // Both halves of the injectivity precondition, checked in O(1)
        // BEFORE the forward map changes.
        let address_already_owned =
            matches!(self.reverse.get(&ptr), Some(&owner) if owner != class_id);
        let previous = self.forward.insert(class_id, ptr);
        let class_was_re_pointed = previous.is_some_and(|previous| previous != ptr);

        if address_already_owned || class_was_re_pointed {
            self.abandon_reverse_index();
            return;
        }
        if !self.reverse_index_abandoned {
            self.reverse.insert(ptr, class_id);
        }
        self.debug_assert_consistent();
    }

    /// Forward lookup: the prototype object registered for `class_id`.
    #[inline]
    pub(crate) fn get(&self, class_id: u32) -> Option<usize> {
        self.forward.get(&class_id).copied()
    }

    /// Reverse lookup: the declared class whose `.prototype` is `ptr`.
    /// O(1) — this is the whole point of the type.
    #[inline]
    pub(crate) fn class_id_for(&self, ptr: usize) -> Option<u32> {
        if self.reverse_index_abandoned {
            return self.scan_class_id_for(ptr);
        }
        let answer = self.reverse.get(&ptr).copied();
        debug_assert_eq!(
            answer,
            self.scan_class_id_for(ptr),
            "decl-prototype reverse index disagrees with the forward table"
        );
        answer
    }

    /// Every registered class id. Read-only by construction (`u32` copies),
    /// so a caller cannot reach the addresses through it.
    pub(crate) fn class_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.forward.keys().copied()
    }

    /// Drop `class_id`'s registration. Test-support only today; kept next to
    /// `insert` so the pair is read together.
    #[cfg(test)]
    pub(crate) fn remove(&mut self, class_id: u32) {
        if let Some(previous) = self.forward.remove(&class_id) {
            if self.reverse.get(&previous) == Some(&class_id) {
                self.reverse.remove(&previous);
            }
        }
        self.debug_assert_consistent();
    }

    /// Hand every root slot to a GC visitor, then re-key `reverse` for any
    /// address the visitor rewrote (evacuation).
    pub(crate) fn visit_root_slots(&mut self, mut visit: impl FnMut(&mut usize)) {
        let mut moved = false;
        for slot in self.forward.values_mut() {
            let before = *slot;
            visit(slot);
            moved |= *slot != before;
        }
        if moved && !self.reverse_index_abandoned {
            self.rebuild_reverse();
        }
        self.debug_assert_consistent();
    }

    /// Single-slot twin of [`Self::visit_root_slots`], for the step-wise
    /// (cycle-based) root machine that visits one recorded class id at a time.
    pub(crate) fn visit_root_slot_for(&mut self, class_id: u32, mut visit: impl FnMut(&mut usize)) {
        let Some(slot) = self.forward.get_mut(&class_id) else {
            return;
        };
        let before = *slot;
        visit(slot);
        let after = *slot;
        if after != before && !self.reverse_index_abandoned {
            // Injective `forward` (see the module docs) means `before` was
            // this class id's address and nobody else's, so the retarget is
            // exact; if it were not, `reverse_index_abandoned` would already
            // be set and this arm unreachable.
            if self.reverse.get(&before) == Some(&class_id) {
                self.reverse.remove(&before);
            }
            self.reverse.insert(after, class_id);
        }
        self.debug_assert_consistent();
    }

    /// Give up the O(1) index for the life of the table and answer every
    /// future reverse lookup with the linear scan instead.
    fn abandon_reverse_index(&mut self) {
        self.reverse_index_abandoned = true;
        self.reverse.clear();
        self.reverse.shrink_to_fit();
    }

    fn rebuild_reverse(&mut self) {
        self.reverse.clear();
        for (&class_id, &ptr) in self.forward.iter() {
            self.reverse.insert(ptr, class_id);
        }
    }

    /// Ground truth: what the pre-#9180 linear scan answered, and what this
    /// table still answers once the index is abandoned. Also the oracle the
    /// `debug_assert_eq!` in [`Self::class_id_for`] checks the index against.
    /// The assertion is compiled out of release builds, so it costs nothing
    /// there.
    fn scan_class_id_for(&self, ptr: usize) -> Option<u32> {
        self.forward
            .iter()
            .find(|(_, &candidate)| candidate == ptr)
            .map(|(&class_id, _)| class_id)
    }

    #[cfg(debug_assertions)]
    fn debug_assert_consistent(&self) {
        if self.reverse_index_abandoned {
            debug_assert!(
                self.reverse.is_empty(),
                "abandoned decl-prototype index must not be consulted"
            );
            return;
        }
        debug_assert_eq!(
            self.forward.len(),
            self.reverse.len(),
            "decl-prototype reverse index lost or gained an entry"
        );
        for (&class_id, &ptr) in self.forward.iter() {
            debug_assert_eq!(
                self.reverse.get(&ptr).copied(),
                Some(class_id),
                "decl-prototype reverse index is missing {ptr:#x} → {class_id}"
            );
        }
    }

    #[cfg(not(debug_assertions))]
    #[inline(always)]
    fn debug_assert_consistent(&self) {}

    #[cfg(test)]
    fn is_abandoned(&self) -> bool {
        self.reverse_index_abandoned
    }
}

#[cfg(test)]
mod tests {
    use super::DeclPrototypeTable;

    #[test]
    fn reverse_answers_what_the_linear_scan_answered() {
        let mut table = DeclPrototypeTable::default();
        table.insert(7, 0x1000);
        table.insert(9, 0x2000);
        assert!(!table.is_abandoned());
        assert_eq!(table.class_id_for(0x1000), Some(7));
        assert_eq!(table.class_id_for(0x2000), Some(9));
        assert_eq!(table.class_id_for(0x3000), None);
        assert_eq!(table.class_id_for(0), None);
        assert_eq!(table.get(7), Some(0x1000));
        assert_eq!(table.get(11), None);
    }

    #[test]
    fn re_registering_the_same_address_is_not_a_retarget() {
        let mut table = DeclPrototypeTable::default();
        table.insert(7, 0x1000);
        table.insert(7, 0x1000);
        assert!(!table.is_abandoned());
        assert_eq!(table.class_id_for(0x1000), Some(7));
    }

    #[test]
    fn removing_a_class_retires_both_directions() {
        let mut table = DeclPrototypeTable::default();
        table.insert(7, 0x1000);
        table.insert(9, 0x2000);
        table.remove(7);
        assert_eq!(table.class_id_for(0x1000), None);
        assert_eq!(table.get(7), None);
        assert_eq!(table.class_id_for(0x2000), Some(9));
    }

    /// The evacuation case the naive pointer-keyed cache got wrong.
    #[test]
    fn evacuation_rekeys_the_reverse_index() {
        let mut table = DeclPrototypeTable::default();
        table.insert(7, 0x1000);
        table.insert(9, 0x2000);
        table.visit_root_slots(|slot| *slot += 0x10_0000);
        assert_eq!(table.class_id_for(0x1000), None);
        assert_eq!(table.class_id_for(0x2000), None);
        assert_eq!(table.class_id_for(0x10_1000), Some(7));
        assert_eq!(table.class_id_for(0x10_2000), Some(9));
        assert_eq!(table.get(7), Some(0x10_1000));
    }

    #[test]
    fn stepwise_evacuation_rekeys_only_the_visited_class() {
        let mut table = DeclPrototypeTable::default();
        table.insert(7, 0x1000);
        table.insert(9, 0x2000);
        table.visit_root_slot_for(7, |slot| *slot = 0x9000);
        assert_eq!(table.class_id_for(0x1000), None);
        assert_eq!(table.class_id_for(0x9000), Some(7));
        assert_eq!(table.class_id_for(0x2000), Some(9));
        table.visit_root_slot_for(1234, |_| unreachable!("unregistered class visited"));
    }

    #[test]
    fn a_visit_that_moves_nothing_leaves_the_index_alone() {
        let mut table = DeclPrototypeTable::default();
        table.insert(7, 0x1000);
        table.visit_root_slots(|_| {});
        table.visit_root_slot_for(7, |_| {});
        assert_eq!(table.class_id_for(0x1000), Some(7));
        let mut ids: Vec<_> = table.class_ids().collect();
        ids.sort_unstable();
        assert_eq!(ids, vec![7]);
    }

    /// Two class ids on one address would make a targeted reverse update
    /// drop the other one's key. The table gives the index up instead and
    /// answers exactly as the pre-#9180 linear scan did — including through
    /// a later evacuation, where the abandoned index must stay out of the way.
    #[test]
    fn a_shared_prototype_address_falls_back_to_the_scan() {
        let mut table = DeclPrototypeTable::default();
        table.insert(7, 0x1000);
        table.insert(9, 0x1000);
        assert!(table.is_abandoned());
        assert!(matches!(table.class_id_for(0x1000), Some(7) | Some(9)));
        assert_eq!(table.class_id_for(0x2000), None);
        assert_eq!(table.get(7), Some(0x1000));
        assert_eq!(table.get(9), Some(0x1000));

        table.insert(11, 0x3000);
        assert_eq!(table.class_id_for(0x3000), Some(11));
        table.visit_root_slots(|slot| *slot += 0x10_0000);
        assert_eq!(table.class_id_for(0x3000), None);
        assert_eq!(table.class_id_for(0x10_3000), Some(11));
        assert!(matches!(table.class_id_for(0x10_1000), Some(7) | Some(9)));
        table.visit_root_slot_for(11, |slot| *slot = 0x4000);
        assert_eq!(table.class_id_for(0x4000), Some(11));
        assert_eq!(table.class_id_for(0x10_3000), None);
    }

    /// Re-pointing one class at a different object is the other half of the
    /// precondition; it degrades the same way rather than silently stranding
    /// the old address in `reverse`.
    #[test]
    fn re_pointing_a_class_falls_back_to_the_scan() {
        let mut table = DeclPrototypeTable::default();
        table.insert(7, 0x1000);
        table.insert(7, 0x4000);
        assert!(table.is_abandoned());
        assert_eq!(table.class_id_for(0x1000), None);
        assert_eq!(table.class_id_for(0x4000), Some(7));
        assert_eq!(table.get(7), Some(0x4000));
    }
}
