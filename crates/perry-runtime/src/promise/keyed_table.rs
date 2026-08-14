//! `PromiseKeyedTable` — the shared backing store for the three
//! promise-pointer-keyed reaction side tables (`PROMISE_ALL_STATES`,
//! `PROMISE_SETTLE_LISTENERS`, `PROMISE_OVERFLOW_REACTIONS`).
//!
//! # Why this exists (#6084 item 2)
//!
//! All three were `RefCell<Vec<(usize, T)>>` and every promise settlement ran a
//! FULL LINEAR SCAN of each table to collect the entries keyed by the settling
//! promise — `js_promise_resolve`/`reject` call
//! `promise_take_settle_listeners` + `promise_all_take_all_handlers` +
//! `promise_take_overflow_reactions` unconditionally. With N promises parked in
//! a table (the `Promise.all([...N])` shape), settling all N is O(N²). Measured
//! settle time for `Promise.all` over N pending promises: 5k → 14ms, 10k → 26ms,
//! 20k → 329ms, 40k → 595ms (node: 1/0/2/4ms). The per-sweep death hook
//! (`remove_*_for_dead_promise`) had the same shape: O(table) per dead promise.
//!
//! # Why it is not just a `HashMap`
//!
//! These tables are **GC root side tables**, and two GC mechanisms depend on the
//! backing `Vec`'s layout:
//!
//! 1. **The incremental root scanner resumes by position.**
//!    `PromiseRootScanState { index, slot }` (`scanners.rs`) stops mid-table
//!    when its step budget runs out and resumes at `entries[index]` on the next
//!    increment. A `HashMap` has no stable positional cursor.
//! 2. **Evacuation rewrites the key in place.** The scanners call
//!    `visitor.visit_metadata_usize_slot(&mut entry.key)`: when a moving GC
//!    relocates a promise, the GC *edits the key of a live entry*. Doing that
//!    inside a `HashMap` leaves the entry in the bucket for its OLD hash —
//!    silently unfindable.
//!
//! So the dense `Vec<Entry<T>>` is kept EXACTLY as the traversal/rewrite surface
//! (the scanners' access pattern and the mutation discipline — append + swap
//! remove — are unchanged from the old raw `Vec`), and an O(1) key → positions
//! index is layered on top as *derived* state:
//!
//! * `index` is a plain `HashMap<usize, Slots>` of positions into `entries`.
//! * `Entry::slot` is the *reverse* of that: an entry's own offset within its
//!   key's slot list. Draining key A `swap_remove`s A's entries, and each
//!   removal displaces one FOREIGN entry (the table's last) into the vacated
//!   slot, whose key's slot list must then be repointed. Searching that list
//!   for the old position is Θ(M) for a key with M parked entries — so
//!   `a.then(f)×M; b.then(f)×M; resolveA()` drains A in Θ(M²), the very defect
//!   this table exists to remove, just moved from the table to a slot list.
//!   With `slot` the displaced entry names its own offset and the repoint is a
//!   single indexed store.
//! * Any GC action that can invalidate them — a key rewritten by evacuation
//!   (`note_key_rewritten`), or the copied-minor cleanup dropping/rekeying
//!   entries (`retain_mut`) — just sets `index_dirty`. Nothing tries to patch
//!   the index (or `slot`) incrementally from inside a GC path.
//! * The next lookup rebuilds both from `entries` alone (`ensure_index`). That
//!   is O(n log n) but happens at most once per GC cycle that actually moved a
//!   promise, and such a cycle already pays O(n) walking this table.
//!
//! This is the same "keys vector stays the GC traversal surface, map gives O(1)"
//! split that `PromiseContextStore` (#6267) uses for the 1:1 `PROMISE_CONTEXTS`
//! table; `PromiseKeyedTable` is its 1:N (multimap) analogue — a promise can be
//! an input to several `Promise.all` calls, and can carry several settle
//! listeners / overflow reactions.
//!
//! # Ordering
//!
//! Per-key registration (FIFO) order is observable: overflow reactions
//! (`p.then(a); p.then(b)`) must replay in registration order. The dense `Vec`
//! no longer preserves it (`swap_remove` reorders), so each entry carries a
//! monotonic `seq` and `take_all` sorts the drained entries by it. `seq` also
//! makes the index fully reconstructible from `entries` alone, which is what
//! lets the GC paths get away with a blunt "mark dirty".

use std::collections::HashMap;

use crate::fast_hash::{PtrHashMap, PtrHasher};

// Unit tests substitute a transparent key wrapper that counts the actual hash
// and equality probes performed inside `HashMap`. Production keeps the exact
// `usize` key type: the instrumentation therefore cannot change shipped code or
// make a slow lookup look cheap by counting only the call to `HashMap::remove`.
#[cfg(not(test))]
type IndexKey = usize;

#[cfg(test)]
mod operation_probe {
    use std::cell::Cell;
    use std::hash::{Hash, Hasher};

    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    pub(super) struct Counts {
        pub(super) hashes: usize,
        pub(super) index_equalities: usize,
        pub(super) slot_equalities: usize,
    }

    thread_local! {
        static COUNTS: Cell<Counts> = const { Cell::new(Counts {
            hashes: 0,
            index_equalities: 0,
            slot_equalities: 0,
        }) };
    }

    #[repr(transparent)]
    #[derive(Clone, Copy, Debug, Eq)]
    pub(super) struct Key(pub(super) usize);

    impl PartialEq for Key {
        fn eq(&self, other: &Self) -> bool {
            COUNTS.with(|counts| {
                let mut next = counts.get();
                next.index_equalities += 1;
                counts.set(next);
            });
            self.0 == other.0
        }
    }

    #[repr(transparent)]
    #[derive(Clone, Copy, Debug, Eq, Ord, PartialOrd)]
    pub(super) struct SlotPosition(pub(super) u32);

    impl PartialEq for SlotPosition {
        fn eq(&self, other: &Self) -> bool {
            COUNTS.with(|counts| {
                let mut next = counts.get();
                next.slot_equalities += 1;
                counts.set(next);
            });
            self.0 == other.0
        }
    }

    impl Hash for Key {
        fn hash<H: Hasher>(&self, state: &mut H) {
            COUNTS.with(|counts| {
                let mut next = counts.get();
                next.hashes += 1;
                counts.set(next);
            });
            // Identical to `usize::hash`: keep `PtrHasher`'s production hash
            // and bucket distribution, merely count that it was requested.
            state.write_usize(self.0);
        }
    }

    pub(super) fn reset() {
        COUNTS.with(|counts| counts.set(Counts::default()));
    }

    pub(super) fn counts() -> Counts {
        COUNTS.with(Cell::get)
    }
}

#[cfg(test)]
type IndexKey = operation_probe::Key;

#[cfg(not(test))]
type SlotPosition = u32;

#[cfg(test)]
type SlotPosition = operation_probe::SlotPosition;

#[cfg(not(test))]
#[inline]
fn index_key(key: usize) -> IndexKey {
    key
}

#[cfg(test)]
#[inline]
fn index_key(key: usize) -> IndexKey {
    operation_probe::Key(key)
}

#[cfg(not(test))]
#[inline]
fn slot_position(position: u32) -> SlotPosition {
    position
}

#[cfg(test)]
#[inline]
fn slot_position(position: u32) -> SlotPosition {
    operation_probe::SlotPosition(position)
}

#[cfg(not(test))]
#[inline]
fn raw_slot_position(position: SlotPosition) -> u32 {
    position
}

#[cfg(test)]
#[inline]
fn raw_slot_position(position: SlotPosition) -> u32 {
    position.0
}

/// One parked reaction. `key` is the pending promise's address — a GC-visible
/// metadata slot, rewritten in place by evacuation.
pub(super) struct Entry<T> {
    pub(super) key: usize,
    /// Monotonic registration counter — restores FIFO order after `swap_remove`
    /// shuffles `entries`, and is the sort key when the index is rebuilt.
    seq: u64,
    /// Reverse position index: this entry's own offset within `index[key]`'s
    /// slot list, i.e. `index[key].as_slice()[slot]` is this entry's position in
    /// `entries`. Lets a displaced entry repoint its key's slot list with one
    /// indexed store instead of scanning it (see `take_all`).
    ///
    /// Derived state, exactly like `index` itself: meaningful only while
    /// `!index_dirty`, and rebuilt wholesale by `ensure_index`. No GC path
    /// maintains it.
    slot: u32,
    pub(super) value: T,
}

/// Positions of one key's entries, in registration order. Inline for the
/// overwhelmingly common single-entry case so a `Promise.all` over N distinct
/// promises does not allocate N side `Vec`s.
enum Slots {
    One(SlotPosition),
    Many(Vec<SlotPosition>),
}

impl Slots {
    /// Append `position` and return the offset it landed at — the caller stores
    /// that in `Entry::slot` so the entry can find itself again in O(1).
    #[inline]
    fn push(&mut self, position: u32) -> u32 {
        match self {
            Slots::One(first) => {
                *self = Slots::Many(vec![*first, slot_position(position)]);
                1
            }
            Slots::Many(positions) => {
                positions.push(slot_position(position));
                (positions.len() - 1) as u32
            }
        }
    }

    #[inline]
    fn as_slice(&self) -> &[SlotPosition] {
        match self {
            Slots::One(first) => std::slice::from_ref(first),
            Slots::Many(positions) => positions,
        }
    }

    /// Repoint the slot at `offset` — the displaced entry's own `Entry::slot` —
    /// at its new home in `entries`. O(1): no scan of the slot list, which is
    /// what made draining one heavily-populated key ahead of another Θ(M²).
    #[inline]
    fn relocate(&mut self, offset: u32, new_position: u32) {
        match self {
            Slots::One(first) => {
                debug_assert_eq!(offset, 0, "a One slot list has only offset 0");
                *first = slot_position(new_position);
            }
            Slots::Many(positions) => {
                debug_assert!((offset as usize) < positions.len(), "slot offset in range");
                if let Some(position) = positions.get_mut(offset as usize) {
                    *position = slot_position(new_position);
                }
            }
        }
    }
}

pub(super) struct PromiseKeyedTable<T> {
    /// Dense storage. This is the GC scanners' traversal surface: they walk it
    /// positionally and rewrite `Entry::key` in place on evacuation.
    entries: Vec<Entry<T>>,
    /// Derived: promise address → positions in `entries`. Rebuilt wholesale
    /// whenever a GC path touches `entries` (see `index_dirty`). Keys are raw
    /// promise pointers, so `PtrHasher` (one multiply) rather than SipHash.
    index: PtrHashMap<IndexKey, Slots>,
    /// Set by every GC path that can invalidate `index` (key rewritten by
    /// evacuation, entries dropped/rekeyed by the copied-minor cleanup). The
    /// next lookup rebuilds the index from `entries`.
    index_dirty: bool,
    next_seq: u64,
}

impl<T> Default for PromiseKeyedTable<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> PromiseKeyedTable<T> {
    pub(super) const fn new() -> Self {
        Self {
            entries: Vec::new(),
            // `HashMap::with_hasher` is const; `HashMap::new` (RandomState) is
            // not — and these thread_locals want const init.
            index: HashMap::with_hasher(PtrHasher),
            index_dirty: false,
            next_seq: 0,
        }
    }

    #[inline]
    #[allow(dead_code)] // used only by #[cfg(test)] assertions in this module
    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[inline]
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Park `value` against `key`. O(1).
    pub(super) fn push(&mut self, key: usize, value: T) {
        let position = self.entries.len() as u32;
        let seq = self.next_seq;
        self.next_seq += 1;
        // While the index is dirty it is not maintained at all, and neither is
        // `slot`: `ensure_index` rebuilds both together on the next lookup.
        let slot = if self.index_dirty {
            0
        } else {
            self.index_slot_for(key, position)
        };
        self.entries.push(Entry {
            key,
            seq,
            slot,
            value,
        });
    }

    /// Record `position` under `key` in the index and return the entry's offset
    /// within that key's slot list.
    #[inline]
    fn index_slot_for(&mut self, key: usize, position: u32) -> u32 {
        match self.index.get_mut(&index_key(key)) {
            Some(slots) => slots.push(position),
            None => {
                self.index
                    .insert(index_key(key), Slots::One(slot_position(position)));
                0
            }
        }
    }

    /// Positional accessor for the incremental GC scanners.
    #[inline]
    pub(super) fn entry_at_mut(&mut self, index: usize) -> Option<&mut Entry<T>> {
        self.entries.get_mut(index)
    }

    /// Iterate every entry — for the non-incremental (`_mut`) root scanners.
    #[inline]
    pub(super) fn iter_mut(&mut self) -> impl Iterator<Item = &mut Entry<T>> {
        self.entries.iter_mut()
    }

    /// A GC pass rewrote at least one `Entry::key` (evacuation moved a promise),
    /// so every recorded position is still valid but the key → position mapping
    /// is not. Rebuild lazily rather than patching a HashMap whose hashes just
    /// changed underneath it.
    #[inline]
    pub(super) fn note_key_rewritten(&mut self) {
        self.index_dirty = true;
    }

    /// Drain every entry parked against `key`, in registration (FIFO) order.
    /// O(k + k·log k) in the number of entries for THAT key — no longer O(table).
    pub(super) fn take_all(&mut self, key: usize) -> Vec<T> {
        if self.entries.is_empty() {
            return Vec::new();
        }
        self.ensure_index();
        let Some(slots) = self.index.remove(&index_key(key)) else {
            return Vec::new();
        };

        let mut positions: Vec<SlotPosition> = slots.as_slice().to_vec();
        // Remove from the highest position down. `swap_remove(p)` backfills `p`
        // with the element that was last; taking the largest of OUR positions
        // first guarantees that element is never another entry we still have to
        // remove (all of ours that remain are at strictly lower positions), so
        // each removal either pops the tail or relocates exactly one FOREIGN
        // entry, which we fix up through its own key's slot list.
        positions.sort_unstable_by(|a, b| b.cmp(a));

        let mut drained: Vec<Entry<T>> = Vec::with_capacity(positions.len());
        for position in positions {
            let position = raw_slot_position(position);
            let entry = self.entries.swap_remove(position as usize);
            debug_assert_eq!(entry.key, key);
            let vacated = position as usize;
            if vacated < self.entries.len() {
                // `swap_remove` moved what was the LAST entry into `vacated`.
                // Per the argument above it is always a FOREIGN entry, so its
                // key still has a slot list in the index (ours was removed) and
                // that list has to point at `position` now. The entry knows its
                // own offset within that list, so this is one indexed store —
                // scanning the list instead is Θ(M) per removal and turns
                // draining one heavily-populated key that sits ahead of another
                // into Θ(M²).
                let moved = &self.entries[vacated];
                debug_assert_ne!(moved.key, key, "displaced entry must be foreign");
                let moved_key = moved.key;
                let moved_slot = moved.slot;
                if let Some(slots) = self.index.get_mut(&index_key(moved_key)) {
                    slots.relocate(moved_slot, position);
                }
            }
            drained.push(entry);
        }

        // `entries` is not in registration order (swap_remove reorders it), so
        // restore FIFO from the monotonic seq. Observable for overflow
        // reactions: `p.then(a); p.then(b)` must run a before b.
        drained.sort_unstable_by_key(|entry| entry.seq);
        drained.into_iter().map(|entry| entry.value).collect()
    }

    /// Drop every entry parked against `key` (GC death hook: the promise was
    /// swept, so nothing parked against it can ever fire). O(k), was O(table).
    pub(super) fn remove_key(&mut self, key: usize) {
        if self.entries.is_empty() {
            return;
        }
        drop(self.take_all(key));
    }

    /// Copied-minor from-space cleanup: `keep` may mutate `Entry::key` (rekey a
    /// promise the scanners missed) and returns false to drop the entry. Both
    /// invalidate the index, so it is rebuilt on the next lookup.
    pub(super) fn retain_mut(&mut self, mut keep: impl FnMut(&mut Entry<T>) -> bool) {
        if self.entries.is_empty() {
            return;
        }
        self.entries.retain_mut(|entry| keep(entry));
        self.index_dirty = true;
    }

    #[cfg(test)]
    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.index.clear();
        self.index_dirty = false;
    }

    #[cfg(test)]
    pub(super) fn count_for_key(&mut self, key: usize) -> usize {
        self.ensure_index();
        self.index
            .get(&index_key(key))
            .map(|slots| slots.as_slice().len())
            .unwrap_or(0)
    }

    /// Rebuild `index` — and every entry's reverse `slot` — from `entries`
    /// alone. Positions are grouped per key in ascending `seq` order so each
    /// key's slot list stays registration-ordered.
    fn ensure_index(&mut self) {
        if !self.index_dirty {
            return;
        }
        self.index.clear();
        let mut order: Vec<u32> = (0..self.entries.len() as u32).collect();
        order.sort_unstable_by_key(|&position| self.entries[position as usize].seq);
        for position in order {
            let key = self.entries[position as usize].key;
            let slot = self.index_slot_for(key, position);
            self.entries[position as usize].slot = slot;
        }
        self.index_dirty = false;
    }

    #[cfg(test)]
    fn reset_operation_probes(&mut self) {
        operation_probe::reset();
    }

    /// Debug-only: the index must describe `entries` exactly, each key's slot
    /// list must be in registration (seq) order, and every entry's reverse
    /// `slot` must point back at the slot that points at it.
    #[cfg(test)]
    fn assert_invariants(&self) {
        if self.index_dirty {
            return;
        }
        let indexed: usize = self.index.values().map(|s| s.as_slice().len()).sum();
        assert_eq!(indexed, self.entries.len(), "index must cover every entry");
        for (key, slots) in self.index.iter() {
            let mut last_seq = None;
            for (offset, &position) in slots.as_slice().iter().enumerate() {
                let position = raw_slot_position(position);
                let entry = &self.entries[position as usize];
                assert_eq!(entry.key, key.0, "indexed position must hold that key");
                assert_eq!(
                    entry.slot as usize, offset,
                    "entry must record its own offset in its key's slot list"
                );
                if let Some(previous) = last_seq {
                    assert!(entry.seq > previous, "slot list must be seq-ordered");
                }
                last_seq = Some(entry.seq);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PromiseKeyedTable;

    #[test]
    fn take_all_returns_registration_order_and_leaves_others_intact() {
        let mut table: PromiseKeyedTable<&str> = PromiseKeyedTable::new();
        table.push(1, "a1");
        table.push(2, "b1");
        table.push(1, "a2");
        table.push(3, "c1");
        table.push(1, "a3");
        table.assert_invariants();

        assert_eq!(table.take_all(1), vec!["a1", "a2", "a3"]);
        table.assert_invariants();
        assert_eq!(table.len(), 2);
        assert_eq!(table.take_all(1), Vec::<&str>::new());
        assert_eq!(table.take_all(2), vec!["b1"]);
        assert_eq!(table.take_all(3), vec!["c1"]);
        assert!(table.is_empty());
    }

    #[test]
    fn take_all_of_absent_key_is_a_no_op() {
        let mut table: PromiseKeyedTable<u32> = PromiseKeyedTable::new();
        table.push(7, 1);
        assert_eq!(table.take_all(9), Vec::<u32>::new());
        assert_eq!(table.len(), 1);
        table.assert_invariants();
    }

    #[test]
    fn rekey_then_lookup_rebuilds_the_index() {
        // Mirrors evacuation: the scanner rewrites Entry::key in place and calls
        // note_key_rewritten(); the entry must be findable under its NEW key and
        // gone from the old one.
        let mut table: PromiseKeyedTable<&str> = PromiseKeyedTable::new();
        table.push(10, "x");
        table.push(20, "y");
        table.push(10, "z");

        for entry in table.iter_mut() {
            if entry.key == 10 {
                entry.key = 99;
            }
        }
        table.note_key_rewritten();

        assert_eq!(table.take_all(10), Vec::<&str>::new());
        assert_eq!(table.take_all(99), vec!["x", "z"]);
        table.assert_invariants();
        assert_eq!(table.take_all(20), vec!["y"]);
    }

    #[test]
    fn retain_mut_drop_and_rekey_rebuilds_the_index() {
        let mut table: PromiseKeyedTable<u32> = PromiseKeyedTable::new();
        for i in 0..6u32 {
            table.push((i % 3) as usize, i);
        }
        // Drop key 1 entirely, rekey key 0 -> key 5.
        table.retain_mut(|entry| {
            if entry.key == 1 {
                return false;
            }
            if entry.key == 0 {
                entry.key = 5;
            }
            true
        });

        assert_eq!(table.take_all(1), Vec::<u32>::new());
        assert_eq!(table.take_all(0), Vec::<u32>::new());
        assert_eq!(table.take_all(5), vec![0, 3]);
        table.assert_invariants();
        assert_eq!(table.take_all(2), vec![2, 5]);
        assert!(table.is_empty());
    }

    #[test]
    fn remove_key_drops_only_that_key() {
        let mut table: PromiseKeyedTable<u32> = PromiseKeyedTable::new();
        table.push(1, 10);
        table.push(2, 20);
        table.push(1, 11);
        table.remove_key(1);
        table.assert_invariants();
        assert_eq!(table.len(), 1);
        assert_eq!(table.take_all(2), vec![20]);
    }

    /// Randomized differential test against the naive `Vec<(usize, T)>` the
    /// three tables used before #6084: same push/take/remove sequence must
    /// produce the same drained values in the same order.
    #[test]
    fn matches_naive_vec_model_under_random_operations() {
        let mut table: PromiseKeyedTable<u64> = PromiseKeyedTable::new();
        let mut model: Vec<(usize, u64)> = Vec::new();

        // Deterministic xorshift — no rand dependency in perry-runtime.
        let mut state: u64 = 0x243f_6a88_85a3_08d3;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };

        for step in 0..20_000u64 {
            let key = (next() % 16) as usize;
            match next() % 4 {
                0 | 1 => {
                    table.push(key, step);
                    model.push((key, step));
                }
                2 => {
                    let drained = table.take_all(key);
                    let expected: Vec<u64> = model
                        .iter()
                        .filter(|(k, _)| *k == key)
                        .map(|(_, v)| *v)
                        .collect();
                    model.retain(|(k, _)| *k != key);
                    assert_eq!(drained, expected, "FIFO drain must match the naive model");
                }
                _ => {
                    table.remove_key(key);
                    model.retain(|(k, _)| *k != key);
                }
            }
            assert_eq!(table.len(), model.len());
            // The reverse `slot` index is only observable through a later
            // relocation, so check it as we go rather than once at the end.
            if step % 128 == 0 {
                table.assert_invariants();
            }
        }
        table.assert_invariants();
    }

    /// The regression this exists to prevent: settling N distinct promises must
    /// not be O(N²). With the old full-table scan this is ~N²/2 comparisons.
    ///
    /// Shape: MANY keys, ONE entry each (`Promise.all([...N])`). This exercises
    /// the *lookup* path but NOT the *relocation* path — see
    /// `draining_a_heavily_populated_key_ahead_of_another_is_not_quadratic`.
    #[test]
    fn settling_many_keys_is_not_quadratic() {
        const N: usize = 20_000;

        let mut table: PromiseKeyedTable<usize> = PromiseKeyedTable::new();
        for i in 0..N {
            table.push(i * 64, i);
        }

        // Measure the algorithm, not how much CPU time libtest happened to get.
        // The test-only transparent wrappers count the actual HashMap and slot
        // comparisons while preserving the production hash and key layout.
        table.reset_operation_probes();
        for i in 0..N {
            assert_eq!(table.take_all(i * 64), vec![i]);
        }
        assert!(table.is_empty());

        let probes = super::operation_probe::counts();
        // N successful removes, plus at most one relocation lookup for each
        // removed entry. A full-table key scan performs no indexed remove and
        // therefore fails the lower bound; a pathological index fails the
        // equality bound.
        assert!(probes.hashes >= N && probes.hashes <= N * 2);
        assert!(
            probes.index_equalities >= probes.hashes
                && probes.index_equalities <= probes.hashes * 2,
            "indexed drain used {} hashes but {} equality probes",
            probes.hashes,
            probes.index_equalities
        );
        assert_eq!(probes.slot_equalities, 0);
    }

    /// The OTHER quadratic — the one the many-keys/one-entry benchmark above
    /// cannot see (CodeRabbit review on #6327).
    ///
    /// Shape: a FEW keys, each with MANY parked entries. In TS:
    /// `for (…M…) a.then(f); for (…M…) b.then(f); resolveA()`. All of A's
    /// entries sit at the FRONT of `entries`, so every `swap_remove` of one
    /// backfills the vacated slot with one of B's entries and has to repoint
    /// B's slot list at the moved entry's new home. Finding the moved entry's
    /// offset in B's slot list by scanning is Θ(M) per removal ⇒ Θ(M²) for the
    /// drain. Each entry therefore records its own offset (`Entry::slot`), so
    /// the relocation is a single indexed store.
    #[test]
    fn draining_a_heavily_populated_key_ahead_of_another_is_not_quadratic() {
        const A: usize = 0x1000;
        const B: usize = 0x2000;
        const M: usize = 20_000;

        let mut table: PromiseKeyedTable<usize> = PromiseKeyedTable::new();
        for i in 0..M {
            table.push(A, i);
        }
        for i in 0..M {
            table.push(B, M + i);
        }

        table.reset_operation_probes();
        let drained = table.take_all(A);
        let probes = super::operation_probe::counts();

        // Correctness, not just work: A drains in registration order and B is
        // left whole. Since A occupied the front half, every one of its M
        // removals displaces a B entry. Each displaced entry must perform one
        // indexed B lookup and one direct `Entry::slot` relocation — the old
        // scan of B's M slots performed ~M² position comparisons instead.
        assert_eq!(drained, (0..M).collect::<Vec<_>>());
        assert_eq!(table.len(), M);
        assert_eq!(probes.hashes, M + 1);
        assert!(
            probes.index_equalities >= probes.hashes
                && probes.index_equalities <= probes.hashes * 2,
            "relocation used {} hashes but {} equality probes",
            probes.hashes,
            probes.index_equalities
        );
        assert_eq!(
            probes.slot_equalities, 0,
            "relocation must use Entry::slot directly, without searching B's slot list"
        );
        table.assert_invariants();

        table.reset_operation_probes();
        assert_eq!(table.take_all(B), (M..2 * M).collect::<Vec<_>>());
        assert!(table.is_empty());
        assert_eq!(
            super::operation_probe::counts(),
            super::operation_probe::Counts {
                hashes: 1,
                index_equalities: 1,
                slot_equalities: 0,
            }
        );
    }
}
