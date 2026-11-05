//! Index-addressed storage that allocates in pages, so a capacity ceiling costs
//! nothing until it is used.
//!
//! A loop's capacities (`Config::max_handles`, `Config::max_operations`) name the
//! largest number of slots it may ever hold. Building every slot at construction
//! makes that number a preallocation instead of a ceiling, which forces hosts to
//! choose between refusing work and paying for a loop they mostly do not use.
//!
//! [`Slots`] keeps the same addressing — a slot is named by its index, exactly as
//! in the `Vec<Option<T>>` it replaces — and materialises pages on demand. Two
//! properties make that safe to substitute:
//!
//! * **A page never moves.** Each page is a separately allocated boxed slice, so
//!   growth appends to the page directory and leaves every existing slot at its
//!   address. Backends that hand a slot's address to the kernel (the IOCP
//!   `OVERLAPPED` slab) depend on this; a `Vec` that reallocates on growth does
//!   not have it.
//! * **A page boundary is invisible to a key.** An index means the same slot
//!   before and after growth, so generations, handles and operation ids stay
//!   valid across it.
//!
//! Pages are a dense prefix: page `n` exists only if pages `0..n` do. Slot
//! indices are handed out lowest-free-first, so the materialised prefix tracks
//! the loop's high-water mark rather than its ceiling.
//!
//! Page zero is built at construction. A loop that exists will use its first
//! slots, and building them with it keeps the reserve the allocation gates
//! check: a loop at its high-water mark allocates nothing per turn, and only
//! passing a new high-water mark builds a page.
use std::ops::{Index, IndexMut};

/// Slots per page. A power of two so the index split is a shift and a mask.
///
/// The page is the unit of growth, so this trades the per-loop floor (one page
/// of every paged structure) against how often a growing loop allocates. At 64,
/// the largest paged element in the crate keeps a page under 16 KiB.
pub(crate) const PAGE: usize = 64;
const SHIFT: u32 = PAGE.trailing_zeros();
const MASK: usize = PAGE - 1;

/// One page's worth of a ceiling.
///
/// Queues bounded by a capacity reserved that whole capacity, which made the
/// ceiling a preallocation for the same reason the slot arrays did. They reserve
/// this instead and grow from it, so the first page of work still costs no
/// allocation and the reserve no longer scales with the ceiling.
pub(crate) fn page_reserve(ceiling: usize) -> usize {
    PAGE.min(ceiling)
}

/// Paged, index-addressed storage for at most `ceiling` values.
///
/// Substitutable for `Vec<Option<T>>` at the call site: [`Index`], [`IndexMut`],
/// [`get`](Self::get), [`get_mut`](Self::get_mut) and [`len`](Self::len) all
/// report what the flat vector reported. Indexing for write materialises the
/// page; indexing for read never allocates.
pub(crate) struct Slots<T> {
    /// Materialised pages, in index order. Page `p` covers `p * PAGE..(p + 1) * PAGE`.
    pages: Vec<Box<[Option<T>]>>,
    /// The configured ceiling. Indices at or above it are out of bounds.
    ceiling: usize,
}

impl<T> Slots<T> {
    /// A vacant slot to hand out for reads of an unmaterialised index. Borrowing
    /// a `None` const promotes to `'static`, so this costs no storage and no
    /// allocation.
    const VACANT: Option<T> = None;

    /// Storage for at most `ceiling` slots, with page zero built.
    pub fn new(ceiling: usize) -> Self {
        let mut slots = Self {
            pages: Vec::new(),
            ceiling,
        };
        slots.grow();
        slots
    }

    /// Storage for at most `ceiling` slots, with page zero built *and filled*.
    ///
    /// For slabs whose elements were all constructed up front so that later use
    /// could not allocate — a per-operation job slot holding a mutex whose
    /// storage Darwin allocates on first lock. Page zero's elements are built
    /// here; later pages build theirs in
    /// [`get_or_insert_with`](Self::get_or_insert_with) as the ceiling is used.
    // Serves the native pool slabs (`fs::Service`, `backend::files`), which wasm
    // targets do not build.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn filled(ceiling: usize, mut build: impl FnMut() -> T) -> Self {
        let mut slots = Self::new(ceiling);
        for slot in slots.iter_mut() {
            *slot = Some(build());
        }
        slots
    }

    /// Build the next page, clamped to the ceiling. Refused at the ceiling.
    fn grow(&mut self) -> bool {
        let base = self.pages.len() * PAGE;
        if base >= self.ceiling {
            return false;
        }
        let len = PAGE.min(self.ceiling - base);
        self.pages.push(
            (0..len)
                .map(|_| None)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        true
    }

    /// The ceiling, matching the length of the `Vec<Option<T>>` this replaces.
    pub fn len(&self) -> usize {
        self.ceiling
    }

    /// The materialised prefix: every index at or above it is vacant.
    ///
    /// Scans that walked `0..len()` looking for occupied slots want this instead,
    /// so the cost tracks what the loop has used rather than what it may use.
    pub fn materialised(&self) -> usize {
        (self.pages.len() * PAGE).min(self.ceiling)
    }

    /// The vacant slot for an unmaterialised read, or `None` past the ceiling.
    fn read(&self, i: usize) -> Option<&Option<T>> {
        if i >= self.len() {
            return None;
        }
        Some(match self.pages.get(i >> SHIFT) {
            Some(page) => &page[i & MASK],
            None => &Self::VACANT,
        })
    }

    /// Materialise pages through `i`'s, so the slot can be written.
    ///
    /// Pages are a dense prefix, so reaching a high index materialises the pages
    /// below it. Slot indices are handed out lowest-free-first, so that only
    /// happens when a host addresses a slot the loop never allocated.
    fn materialise(&mut self, i: usize) -> &mut Option<T> {
        assert!(
            i < self.len(),
            "slot {i} is beyond the configured ceiling {}",
            self.ceiling
        );
        let page = i >> SHIFT;
        while self.pages.len() <= page {
            assert!(self.grow(), "the ceiling admits index {i}");
        }
        debug_assert!(i < self.materialised(), "pages are a dense prefix");
        &mut self.pages[page][i & MASK]
    }

    /// The slot at `i`, or `None` past the ceiling. Never allocates.
    pub fn get(&self, i: usize) -> Option<&Option<T>> {
        self.read(i)
    }

    /// The slot at `i` for writing, or `None` past the ceiling. Materialises the page.
    pub fn get_mut(&mut self, i: usize) -> Option<&mut Option<T>> {
        (i < self.len()).then(|| self.materialise(i))
    }

    /// The value at `i`, building it on first use.
    ///
    /// For slabs whose slots are always present once reached — a per-operation
    /// job slot, a per-handle object cell — where the flat vector built every
    /// element at construction. The element is built when its index is first
    /// reached rather than when the loop is created.
    // Serves the native pool slabs (`fs::Service`, `backend::files`), which wasm
    // targets do not build.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn get_or_insert_with(&mut self, i: usize, build: impl FnOnce() -> T) -> &mut T {
        self.materialise(i).get_or_insert_with(build)
    }

    /// The materialised prefix, in index order. Every slot beyond it is vacant,
    /// so `flatten`, `any(Option::is_some)` and `position` see the same sequence
    /// the flat vector produced, and `enumerate` yields the same indices.
    ///
    /// Returns a named type rather than `impl Iterator` on purpose: an opaque
    /// return type is assumed to have drop glue, which holds the borrow of the
    /// whole owner to the end of the loop and rejects bodies that a slice
    /// iterator allows.
    pub fn iter(&self) -> Iter<'_, T> {
        self.into_iter()
    }

    /// The materialised prefix for mutation. See [`iter`](Self::iter).
    // Serves the native pool slabs (`fs::Service`, `backend::files`), which wasm
    // targets do not build.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        self.into_iter()
    }
}

/// Borrowed iteration over the materialised prefix. See [`Slots::iter`].
pub(crate) type Iter<'a, T> = std::iter::Flatten<std::slice::Iter<'a, Box<[Option<T>]>>>;
/// Mutable iteration over the materialised prefix. See [`Slots::iter`].
pub(crate) type IterMut<'a, T> = std::iter::Flatten<std::slice::IterMut<'a, Box<[Option<T>]>>>;

impl<'a, T> IntoIterator for &'a Slots<T> {
    type Item = &'a Option<T>;
    type IntoIter = Iter<'a, T>;
    /// The materialised prefix. See [`Slots::iter`].
    fn into_iter(self) -> Self::IntoIter {
        self.pages.iter().flatten()
    }
}

impl<'a, T> IntoIterator for &'a mut Slots<T> {
    type Item = &'a mut Option<T>;
    type IntoIter = IterMut<'a, T>;
    /// The materialised prefix. See [`Slots::iter`].
    fn into_iter(self) -> Self::IntoIter {
        self.pages.iter_mut().flatten()
    }
}

impl<T> IntoIterator for Slots<T> {
    type Item = Option<T>;
    type IntoIter = std::iter::FlatMap<
        std::vec::IntoIter<Box<[Option<T>]>>,
        std::vec::Vec<Option<T>>,
        fn(Box<[Option<T>]>) -> std::vec::Vec<Option<T>>,
    >;
    /// The materialised prefix, by value. See [`Slots::iter`].
    fn into_iter(self) -> Self::IntoIter {
        self.pages.into_iter().flat_map(Vec::from)
    }
}

impl<T> Index<usize> for Slots<T> {
    type Output = Option<T>;
    fn index(&self, i: usize) -> &Option<T> {
        self.read(i)
            .unwrap_or_else(|| panic!("slot {i} is beyond the configured ceiling {}", self.ceiling))
    }
}

impl<T> IndexMut<usize> for Slots<T> {
    fn index_mut(&mut self, i: usize) -> &mut Option<T> {
        self.materialise(i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unmaterialised_slot_reads_vacant_without_allocating() {
        let slots: Slots<u32> = Slots::new(1_000_000);
        assert_eq!(
            slots.materialised(),
            PAGE,
            "exactly page zero is built at construction, whatever the ceiling"
        );
        assert_eq!(slots.len(), 1_000_000, "the ceiling is still the length");
        assert!(slots[999_999].is_none());
        assert!(slots.get(999_999).expect("within the ceiling").is_none());
        assert_eq!(slots.materialised(), PAGE, "reading materialises nothing");
        assert!(slots.get(1_000_000).is_none(), "past the ceiling");
    }

    #[test]
    fn growth_leaves_every_earlier_slot_at_its_address() {
        let mut slots: Slots<u32> = Slots::new(4096);
        let mut addresses = Vec::new();
        for i in 0..PAGE {
            slots[i] = Some(i as u32);
            addresses.push(std::ptr::from_ref(&slots[i]));
        }
        assert_eq!(slots.materialised(), PAGE);
        for i in PAGE..PAGE * 8 {
            slots[i] = Some(i as u32);
        }
        assert_eq!(slots.materialised(), PAGE * 8, "seven more pages");
        for (i, address) in addresses.iter().enumerate() {
            assert_eq!(slots[i], Some(i as u32), "value survived growth");
            assert_eq!(
                std::ptr::from_ref(&slots[i]),
                *address,
                "slot {i} moved across a page boundary"
            );
        }
        for i in 0..PAGE * 8 {
            assert_eq!(slots[i], Some(i as u32), "index still names its slot");
        }
    }

    #[test]
    fn iteration_matches_the_flat_vector_over_occupied_slots() {
        let mut slots: Slots<u32> = Slots::new(4096);
        for i in [0, 5, 63, 64, 130] {
            slots[i] = Some(i as u32);
        }
        let seen: Vec<u32> = slots.iter().flatten().copied().collect();
        assert_eq!(seen, vec![0, 5, 63, 64, 130]);
        let indices: Vec<usize> = slots
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| slot.map(|_| i))
            .collect();
        assert_eq!(indices, vec![0, 5, 63, 64, 130], "enumerate keeps indices");
        assert!(slots.iter().any(Option::is_some));
        for slot in slots.iter_mut() {
            *slot = None;
        }
        assert!(slots.iter().all(Option::is_none));
    }

    #[test]
    fn a_page_holds_exactly_its_own_index_range() {
        let mut slots: Slots<u32> = Slots::new(PAGE * 3);
        assert_eq!(slots.materialised(), PAGE);
        slots[PAGE * 3 - 1] = Some(7);
        assert_eq!(slots.materialised(), PAGE * 3);
        assert_eq!(slots[PAGE * 3 - 1], Some(7));
        assert!(
            slots.iter().filter(|s| s.is_some()).count() == 1,
            "materialising a page leaves its other slots vacant"
        );
    }

    #[test]
    #[should_panic(expected = "beyond the configured ceiling")]
    fn writing_past_the_ceiling_panics_like_a_flat_vector() {
        let mut slots: Slots<u32> = Slots::new(8);
        slots[8] = Some(1);
    }

    #[test]
    fn a_ceiling_below_a_page_builds_only_the_slots_it_allows() {
        let mut slots: Slots<u32> = Slots::new(3);
        assert_eq!(slots.materialised(), 3, "page zero stops at the ceiling");
        assert_eq!(slots.iter().count(), 3);
        slots[2] = Some(9);
        assert_eq!(slots[2], Some(9));
    }

    #[test]
    fn filled_builds_page_zeros_elements_and_later_pages_on_demand() {
        let mut built = 0;
        let mut slots: Slots<u32> = Slots::filled(4096, || {
            built += 1;
            Some(built).map(|n| n as u32).expect("counter")
        });
        assert_eq!(built, PAGE, "exactly page zero is filled at construction");
        assert!(slots[0].is_some());
        assert!(slots[PAGE - 1].is_some());
        assert!(slots[PAGE].is_none(), "a later page starts vacant");
        let value = *slots.get_or_insert_with(PAGE, || 4242);
        assert_eq!(value, 4242);
    }

    #[test]
    #[should_panic(expected = "beyond the configured ceiling")]
    fn reading_past_the_ceiling_panics_like_a_flat_vector() {
        let slots: Slots<u32> = Slots::new(8);
        let _ = &slots[8];
    }
}
