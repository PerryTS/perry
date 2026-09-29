//! The copying minor's moved-survivor list (#11549): a chunked list that never
//! reallocates.
//!
//! # Why not `Vec`
//!
//! `moved_headers` holds every object a copying minor moves or promotes in
//! place — one entry per survivor — for `clear_marks`, the promoted
//! remembered-set rebuild and the survivor count. As a `Vec` it grew by
//! doubling from a guessed capacity, and a doubling `realloc` of a large
//! vector copies into fresh pages while the allocator keeps the source pages
//! resident. The guess is the previous minor's survivor count, so the first
//! heavy minor of a program — typically a startup structure promoted whole —
//! paid the whole doubling ladder. On binary-trees at n = 3 (a 131 k-object,
//! 5 MB live tree promoted in place by one minor) the `worklist` and this list
//! took 1,557 page faults, about 6 MB, for 2 MB of pointers.
//!
//! The worklist stays a `Vec`: the drain walks it by index while pushing to
//! it, and the plain indexed loop is the cheapest drain there is (a chunked
//! drain measured +2 instructions per survivor, +0.35% on a survivor-heavy
//! loop). This list is only ever appended to during the cycle and read after
//! it, so chunking costs it nothing.
//!
//! # What this is
//!
//! [`HeaderList`] stores entries in fixed-size chunks that are never
//! reallocated, so an entry is written once and the resident size is the
//! number of entries rounded up to one chunk. Each chunk carries `PAD` zeroed
//! words after its last entry slot, so `clear_marks`'s prefetch look-ahead of
//! `PREFETCH_DISTANCE` reads initialised memory inside the allocation — a real
//! entry or zero (which `prefetch_read` skips) — and needs no bounds test. The
//! list is dropped with the collector at the end of the cycle.

use super::GcHeader;

const CHUNK_SHIFT: usize = 9;
/// Entries per chunk: 512 pointers, 4 KiB. Small enough to come from the
/// allocator's small-object size classes, whose pages a minor's alloc/free of
/// its first chunk keeps reusing; large enough that the per-chunk work is
/// noise.
const CHUNK_ENTRIES: usize = 1 << CHUNK_SHIFT;
const CHUNK_MASK: usize = CHUNK_ENTRIES - 1;

/// Zeroed words after each chunk's last entry slot; see the module docs.
const PAD: usize = super::prefetch::PREFETCH_DISTANCE;

pub(super) struct HeaderList {
    /// Fixed-size chunks of `CHUNK_ENTRIES + PAD` words, never reallocated.
    chunks: Vec<Box<[usize]>>,
    /// First entry slot of the last chunk. Entry `len` is written at
    /// `last + (len & CHUNK_MASK)` while `len & CHUNK_MASK != 0` (otherwise the
    /// next push starts a chunk), so a push stores the entry and `len` only.
    last: *mut usize,
    len: usize,
}

impl Default for HeaderList {
    fn default() -> Self {
        Self {
            chunks: Vec::new(),
            last: std::ptr::null_mut(),
            len: 0,
        }
    }
}

impl HeaderList {
    #[inline]
    pub(super) fn push(&mut self, header: *mut GcHeader) {
        let len = self.len;
        if len & CHUNK_MASK == 0 {
            self.start_chunk();
        }
        // SAFETY: `last` is the last chunk, and `len & CHUNK_MASK` is inside
        // its entry area: a chunk is started exactly when `len` reaches a
        // multiple of CHUNK_ENTRIES.
        unsafe { self.last.add(len & CHUNK_MASK).write(header as usize) };
        self.len = len + 1;
    }

    #[cold]
    #[inline(never)]
    fn start_chunk(&mut self) {
        let mut chunk = vec![0usize; CHUNK_ENTRIES + PAD].into_boxed_slice();
        self.last = chunk.as_mut_ptr();
        self.chunks.push(chunk);
    }

    #[inline]
    pub(super) fn len(&self) -> usize {
        self.len
    }

    /// The written entries, one slice per chunk, in push order. Each slice is
    /// followed in memory by at least `PAD` initialised words.
    pub(super) fn chunk_entries(&self) -> impl Iterator<Item = &[usize]> + '_ {
        let full = self.len >> CHUNK_SHIFT;
        let last = self.len & CHUNK_MASK;
        self.chunks.iter().enumerate().map(move |(i, chunk)| {
            let n = if i < full { CHUNK_ENTRIES } else { last };
            &chunk[..n]
        })
    }

    /// Clear `GC_FLAG_MARKED` on every entry, prefetching ahead.
    ///
    /// # Safety
    /// Every entry must still be a valid, writable GC header.
    pub(super) unsafe fn clear_marks(&self) {
        for chunk in self.chunk_entries() {
            let base = chunk.as_ptr();
            for i in 0..chunk.len() {
                // In bounds and initialised: the chunk has PAD words past its
                // last entry slot.
                super::prefetch::prefetch_read(*base.add(i + PAD));
                let header = *base.add(i) as *mut GcHeader;
                (*header).gc_flags &= !super::GC_FLAG_MARKED;
            }
        }
    }

    /// Every entry, in push order.
    #[cfg(test)]
    pub(super) fn iter(&self) -> impl Iterator<Item = *mut GcHeader> + '_ {
        self.chunk_entries()
            .flat_map(|chunk| chunk.iter())
            .map(|&word| word as *mut GcHeader)
    }

    /// Chunks currently held. Tests use it to prove the footprint follows the
    /// entries.
    #[cfg(test)]
    pub(super) fn chunk_count(&self) -> usize {
        self.chunks.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::GC_FLAG_MARKED;

    fn h(i: usize) -> *mut GcHeader {
        ((i + 1) * 8) as *mut GcHeader
    }

    #[test]
    fn entries_come_back_in_push_order_across_chunks() {
        let mut list = HeaderList::default();
        let n = CHUNK_ENTRIES * 3 + 5;
        for i in 0..n {
            list.push(h(i));
        }
        assert_eq!(list.len(), n);
        assert_eq!(list.chunk_count(), 4, "chunks are allocated on demand");
        assert_eq!(
            list.iter().collect::<Vec<_>>(),
            (0..n).map(h).collect::<Vec<_>>()
        );
        let sizes: Vec<_> = list.chunk_entries().map(<[usize]>::len).collect();
        assert_eq!(sizes, vec![CHUNK_ENTRIES, CHUNK_ENTRIES, CHUNK_ENTRIES, 5]);
    }

    #[test]
    fn a_chunk_boundary_starts_exactly_one_chunk() {
        let mut list = HeaderList::default();
        for i in 0..CHUNK_ENTRIES {
            list.push(h(i));
        }
        assert_eq!(list.chunk_count(), 1);
        list.push(h(CHUNK_ENTRIES));
        assert_eq!(list.chunk_count(), 2);
    }

    #[test]
    fn the_look_ahead_reads_padding_or_unwritten_zeroes() {
        let mut list = HeaderList::default();
        for i in 0..CHUNK_ENTRIES + 3 {
            list.push(h(i));
        }
        // A full chunk's last entry looks PAD words ahead into zero padding;
        // the last chunk's last entry into slots nothing has written.
        for chunk in list.chunk_entries() {
            let ahead = unsafe { *chunk.as_ptr().add(chunk.len() - 1 + PAD) };
            assert_eq!(ahead, 0);
        }
    }

    #[test]
    fn clear_marks_clears_every_entry() {
        let mut headers: Vec<GcHeader> = (0..CHUNK_ENTRIES + 7)
            .map(|_| GcHeader {
                obj_type: 0,
                gc_flags: GC_FLAG_MARKED,
                _reserved: 0,
                size: 0,
            })
            .collect();
        let mut list = HeaderList::default();
        for header in headers.iter_mut() {
            list.push(header as *mut GcHeader);
        }
        unsafe { list.clear_marks() };
        assert!(headers
            .iter()
            .all(|header| header.gc_flags & GC_FLAG_MARKED == 0));
    }

    #[test]
    fn an_empty_list_allocates_nothing() {
        let list = HeaderList::default();
        assert_eq!(list.chunk_count(), 0);
        assert_eq!(list.iter().count(), 0);
    }
}
