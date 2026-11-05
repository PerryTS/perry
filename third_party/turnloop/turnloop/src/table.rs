//! Paged generational storage. Exhausted generations retire their slot.
//!
//! The capacity passed to [`Table::new`] is a ceiling, not a preallocation: the
//! table builds page zero with itself and another page whenever the free list
//! empties and the ceiling still allows it, so an idle table costs one page
//! whatever its ceiling.
//! Slots are named by index and pages never move, so growth leaves every live
//! entry where it was and invalidates no key. See [`crate::slots`] for the
//! addressing properties this relies on.
use crate::slots::PAGE;
struct Slot<T> {
    generation: u32,
    value: Option<T>,
}
pub(crate) struct Table<T> {
    /// Materialised pages, in index order. Page `p` covers `p * PAGE..(p + 1) * PAGE`.
    pages: Vec<Box<[Slot<T>]>>,
    /// Reusable indices, most recently retired last.
    free: Vec<usize>,
    /// Largest number of slots this table may ever hold.
    ceiling: usize,
    /// Occupied slots, so remaining capacity does not need a scan.
    live: usize,
}
impl<T> Table<T> {
    pub fn new(capacity: usize) -> Self {
        let mut table = Self {
            pages: Vec::new(),
            free: Vec::new(),
            ceiling: capacity,
            live: 0,
        };
        // Page zero is built with the table: a loop that exists will use its
        // first slots, and the allocation gates require that use to be free.
        table.grow();
        table
    }
    /// Slots neither occupied nor retired: what `insert` can still hand out.
    ///
    /// Counts capacity the table has not built yet, because a page is built on
    /// demand. Retired slots (a saturated generation) are excluded, since they
    /// are counted live and never return to the free list.
    pub fn remaining(&self) -> usize {
        self.ceiling - self.live
    }
    /// Build the next page and offer its indices, lowest first. Refused at the ceiling.
    fn grow(&mut self) -> bool {
        let base = self.pages.len() * PAGE;
        if base >= self.ceiling {
            return false;
        }
        let len = PAGE.min(self.ceiling - base);
        self.pages.push(
            (0..len)
                .map(|_| Slot {
                    generation: 1,
                    value: None,
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        // `pop` takes the last element, so push descending to hand out ascending.
        self.free.extend((base..base + len).rev());
        true
    }
    fn slot(&self, i: usize) -> Option<&Slot<T>> {
        self.pages.get(i / PAGE)?.get(i % PAGE)
    }
    fn slot_mut(&mut self, i: usize) -> Option<&mut Slot<T>> {
        self.pages.get_mut(i / PAGE)?.get_mut(i % PAGE)
    }
    pub fn insert(&mut self, value: T) -> Option<u64> {
        if self.free.is_empty() && !self.grow() {
            return None;
        }
        let i = self.free.pop()?;
        self.live += 1;
        let slot = self.slot_mut(i).expect("free index names a built slot");
        slot.value = Some(value);
        Some((u64::from(slot.generation) << 32) | i as u64)
    }
    pub fn get(&self, key: u64) -> Option<&T> {
        let s = self.slot(key as u32 as usize)?;
        (s.generation == (key >> 32) as u32)
            .then_some(s.value.as_ref())
            .flatten()
    }
    pub fn get_mut(&mut self, key: u64) -> Option<&mut T> {
        let s = self.slot_mut(key as u32 as usize)?;
        (s.generation == (key >> 32) as u32)
            .then_some(s.value.as_mut())
            .flatten()
    }
    pub fn remove(&mut self, key: u64) -> Option<T> {
        let i = key as u32 as usize;
        let s = self.slot_mut(i)?;
        if s.generation != (key >> 32) as u32 {
            return None;
        }
        let value = s.value.take()?;
        if let Some(generation) = s.generation.checked_add(1) {
            s.generation = generation;
            self.free.push(i);
            self.live -= 1;
        }
        Some(value)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_generation_never_reaches_reused_slot() {
        let mut t = Table::new(1);
        let a = t.insert(12).expect("capacity");
        assert_eq!(t.remove(a), Some(12));
        let b = t.insert(34).expect("capacity");
        assert_ne!(a, b);
        assert_eq!(t.get(a), None);
        assert_eq!(t.remove(a), None);
        assert_eq!(t.get(b), Some(&34));
    }
    #[test]
    fn a_key_taken_before_growth_still_names_its_value_after_it() {
        let mut t = Table::new(4096);
        // Keys from the first page, taken before any later page exists.
        let early: Vec<u64> = (0..PAGE).map(|i| t.insert(i).expect("capacity")).collect();
        assert_eq!(t.remaining(), 4096 - PAGE);
        // Force seven more pages, which is where a flat vector would reallocate.
        let late: Vec<u64> = (PAGE..PAGE * 8)
            .map(|i| t.insert(i).expect("capacity"))
            .collect();
        for (i, key) in early.iter().enumerate() {
            assert_eq!(
                t.get(*key),
                Some(&i),
                "key {key} lost its value across growth"
            );
            assert_eq!(*key as u32 as usize, i, "key {key} changed slot");
        }
        for (n, key) in late.iter().enumerate() {
            assert_eq!(t.get(*key), Some(&(PAGE + n)));
        }
        // And the earlier keys still remove exactly their own value.
        for (i, key) in early.iter().enumerate() {
            assert_eq!(t.remove(*key), Some(i));
        }
    }
    #[test]
    fn the_ceiling_is_a_ceiling_and_a_partial_page_does_not_exceed_it() {
        // Not a multiple of PAGE: the last page must stop at the ceiling.
        let mut t: Table<usize> = Table::new(PAGE + 3);
        let keys: Vec<u64> = (0..PAGE + 3)
            .map(|i| t.insert(i).expect("within the ceiling"))
            .collect();
        assert_eq!(t.remaining(), 0);
        assert!(t.insert(0).is_none(), "the ceiling refuses one more");
        assert!(
            keys.iter().all(|k| (*k as u32 as usize) < PAGE + 3),
            "no key addresses a slot past the ceiling"
        );
        t.remove(keys[0]).expect("live");
        assert_eq!(t.remaining(), 1);
        assert!(t.insert(99).is_some(), "a freed slot is offered again");
    }
    #[test]
    fn growth_builds_one_page_at_a_time() {
        let mut t: Table<usize> = Table::new(1_000_000);
        assert_eq!(t.pages.len(), 1, "page zero comes with the table");
        t.insert(0).expect("capacity");
        assert_eq!(t.pages.len(), 1, "one page serves the first insert");
        for i in 1..PAGE {
            t.insert(i).expect("capacity");
        }
        assert_eq!(
            t.pages.len(),
            1,
            "the page is filled before the next is built"
        );
        t.insert(PAGE).expect("capacity");
        assert_eq!(t.pages.len(), 2);
    }
    #[test]
    fn a_retired_slot_is_not_counted_as_remaining_capacity() {
        let mut t: Table<usize> = Table::new(2);
        let key = t.insert(0).expect("capacity");
        assert_eq!(t.remaining(), 1);
        // Drive that slot's generation to saturation: it retires instead of
        // returning to the free list, so the ceiling permanently loses a slot.
        let i = key as u32 as usize;
        t.slot_mut(i).expect("built").generation = u32::MAX;
        let saturated = (u64::from(u32::MAX) << 32) | i as u64;
        assert_eq!(t.remove(saturated), Some(0));
        assert_eq!(t.remaining(), 1, "the retired slot stays spent");
        assert!(t.insert(1).is_some(), "the other slot is still available");
        assert_eq!(t.remaining(), 0);
        assert!(
            t.insert(2).is_none(),
            "a retired slot is never offered again"
        );
    }
}
