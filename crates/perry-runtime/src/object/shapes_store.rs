//! Storage for the agent-local shape descriptor table (#9706).
//!
//! Two structures, both owned by [`super::ShapeTable`]:
//!
//! * [`ShapeSlab`] — the by-id store. A ShapeId is a process-global monotonic
//!   counter (`SHAPE_ID_BASE + n`), so `n` indexes a chunked slab directly: no
//!   hash, no per-record heap allocation, and a record address that never
//!   moves for the record's lifetime — the property the collector relies on
//!   when it enumerates a descriptor's `keys` word as a rewritable slot
//!   (#8112) and retains that address across budgeted resumptions. Chunks
//!   (32 records) hang off a two-level page directory, are allocated lazily
//!   (a worker's ids interleave with the main thread's), and an all-dead chunk
//!   is released by [`ShapeSlab::release_empty_chunks`] at the same cadence as
//!   the reverse-index shrink (once per major collection).
//!
//! * [`IdList`] — the value of the per-keys-address family index
//!   (`ShapeTableInner::families`). One entry per keys array names every
//!   descriptor id currently indexed under that address. Exact-facts interning
//!   walks the family and compares the remaining facts against the slab
//!   record, which is what lets the table drop the second, facts-keyed reverse
//!   map it used to carry: a family is small by construction — a SHARED keys
//!   array is immutable, so its descriptors differ only in the birth bound or
//!   a semantic generation, and an OWNED array retires its growth history
//!   eagerly (`retire_owned_shape_siblings`).
//!
//! Measured on the compiled claude-code TUI at idle (`PERRY_GC_CENSUS`), the
//! previous layout — a `PtrHashMap<u32, Box<ShapeDescriptor>>` beside two
//! `Vec<u32>`-valued reverse maps — cost ~330 bytes per live descriptor:
//! a 56-byte record in a 64-byte allocator bin, a 16-byte map entry at 25%
//! load after `shrink_to(2 * len)`, a 57-byte facts-map bucket, and a 33-byte
//! keys-map bucket, plus a 16-byte `Vec` buffer per reverse entry. A packed
//! 32-byte slab record with one 24-byte family bucket per keys array is the
//! same information at a fraction of the bytes.

use super::{ShapeDescriptor, ShapeObjectKind, SHAPE_ID_BASE};
use std::cell::UnsafeCell;

pub(super) const RECORD_FLAG_PRESENT: u8 = 1 << 0;
pub(super) const RECORD_FLAG_FACTS_INDEXED: u8 = 1 << 1;
pub(super) const RECORD_FLAG_OLD_CARRIER: u8 = 1 << 2;
pub(super) const RECORD_FLAG_OLD_CARRIER_SEEN: u8 = 1 << 3;
pub(super) const RECORD_FLAG_CACHE_CARRIER: u8 = 1 << 4;
pub(super) const RECORD_FLAG_KIND_CLASS: u8 = 1 << 5;
pub(super) const RECORD_FLAG_CARRIED_SEEN: u8 = 1 << 6;
pub(super) const RECORD_FLAG_EXTERNAL_CARRIER: u8 = 1 << 7;

/// The table-owned record of one ShapeId. `keys` is first and 8-aligned: it
/// is the word the collector marks through and rewrites in place.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ShapeRecord {
    /// Raw ArrayHeader address in Perry's fixed-width heap-word ABI (0 for a
    /// keyless shape).
    pub(super) keys: u64,
    pub(super) semantic_generation: u64,
    pub(super) logical_key_count: u32,
    pub(super) live_inline_slot_count: u32,
    pub(super) hole_count: u32,
    pub(super) flags: u8,
    _pad: [u8; 3],
}

const _: () = assert!(std::mem::size_of::<ShapeRecord>() == 32);
const _: () = assert!(std::mem::align_of::<ShapeRecord>() == 8);

impl ShapeRecord {
    const EMPTY: ShapeRecord = ShapeRecord {
        keys: 0,
        semantic_generation: 0,
        logical_key_count: 0,
        live_inline_slot_count: 0,
        hole_count: 0,
        flags: 0,
        _pad: [0; 3],
    };

    #[inline]
    pub(super) fn present(&self) -> bool {
        self.flags & RECORD_FLAG_PRESENT != 0
    }

    #[inline]
    pub(super) fn has(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }

    #[inline]
    pub(super) fn set(&mut self, flag: u8, on: bool) {
        if on {
            self.flags |= flag;
        } else {
            self.flags &= !flag;
        }
    }

    /// A runtime table or process-lifetime generated-code global may reinstall
    /// this id even while no object currently carries it.
    #[inline]
    pub(super) fn cache_carrier(&self) -> bool {
        self.has(RECORD_FLAG_CACHE_CARRIER | RECORD_FLAG_EXTERNAL_CARRIER)
    }

    #[inline]
    pub(super) fn object_kind(&self) -> ShapeObjectKind {
        if self.has(RECORD_FLAG_KIND_CLASS) {
            ShapeObjectKind::Class
        } else {
            ShapeObjectKind::Ordinary
        }
    }

    /// A fresh, facts-indexed record with every liveness bit clear.
    pub(super) fn new(
        keys: u64,
        logical_key_count: u32,
        live_inline_slot_count: u32,
        semantic_generation: u64,
        object_kind: ShapeObjectKind,
        hole_count: u32,
    ) -> ShapeRecord {
        let mut flags = RECORD_FLAG_PRESENT | RECORD_FLAG_FACTS_INDEXED;
        if object_kind == ShapeObjectKind::Class {
            flags |= RECORD_FLAG_KIND_CLASS;
        }
        ShapeRecord {
            keys,
            semantic_generation,
            logical_key_count,
            live_inline_slot_count,
            hole_count,
            flags,
            _pad: [0; 3],
        }
    }

    /// Exact-facts identity test (#8067): keys edge, both counts, generation,
    /// kind, tombstones. Liveness bits and the facts-indexed bit are storage
    /// state, never identity.
    #[inline]
    pub(super) fn facts_match(
        &self,
        keys: u64,
        logical_key_count: u32,
        live_inline_slot_count: u32,
        semantic_generation: u64,
        object_kind: ShapeObjectKind,
        hole_count: u32,
    ) -> bool {
        self.keys == keys
            && self.logical_key_count == logical_key_count
            && self.live_inline_slot_count == live_inline_slot_count
            && self.semantic_generation == semantic_generation
            && self.hole_count == hole_count
            && self.object_kind() == object_kind
    }

    /// The 64-bit fold of the six identity facts, with `keys` supplied by
    /// the caller: the collector rewrites a record's `keys` in place, so the
    /// address the record was INDEXED under (its family key) is what the
    /// exact-facts accelerator must be probed with until the metadata scan
    /// re-indexes it.
    #[inline]
    pub(super) fn facts_key_with_keys(&self, keys: u64) -> u64 {
        facts_key(
            keys,
            self.logical_key_count,
            self.live_inline_slot_count,
            self.semantic_generation,
            self.object_kind(),
            self.hole_count,
        )
    }

    /// Copy the record out as the by-value [`ShapeDescriptor`] the rest of the
    /// runtime consumes. `record` is the slab address of THIS record, which is
    /// what `keys_slot()` and the tombstone fast paths hand back to the table.
    #[inline]
    pub(super) fn lift(&self, record: *mut ShapeRecord) -> ShapeDescriptor {
        ShapeDescriptor {
            keys: self.keys,
            record: record as usize,
            old_carrier: self.has(RECORD_FLAG_OLD_CARRIER),
            cache_carrier: self.cache_carrier(),
            logical_key_count: self.logical_key_count,
            live_inline_slot_count: self.live_inline_slot_count,
            semantic_generation: self.semantic_generation,
            object_kind: self.object_kind(),
            hole_count: self.hole_count,
        }
    }
}

/// FNV-1a fold of the six identity facts into the single word the
/// exact-facts accelerator is keyed by. Every field reaches the accumulator
/// (fold, never overwrite — the property `PtrHasher` lacks and the reason the
/// old `ShapeFacts` map could not use it); a 64-bit collision between two
/// live shapes is resolved by the per-hit `facts_match` on the record, so a
/// collision only costs a second record read, never a wrong answer.
#[inline]
pub(super) fn facts_key(
    keys: u64,
    logical_key_count: u32,
    live_inline_slot_count: u32,
    semantic_generation: u64,
    object_kind: ShapeObjectKind,
    hole_count: u32,
) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let fold = |acc: u64, word: u64| (acc ^ word).wrapping_mul(FNV_PRIME);
    let mut h = fold(FNV_OFFSET_BASIS, keys);
    h = fold(h, u64::from(logical_key_count));
    h = fold(h, u64::from(live_inline_slot_count));
    h = fold(h, semantic_generation);
    h = fold(h, u64::from(hole_count));
    h = fold(h, u64::from(object_kind == ShapeObjectKind::Class));
    // Final avalanche: FNV keeps most of its entropy in the high bits and
    // hashbrown's probe sequence starts from the LOW bits.
    h ^ (h >> 32)
}

/// Records per chunk. Ids are minted far faster than they survive — the
/// compiled claude-code TUI mints ~1.05 M ShapeIds during startup and keeps
/// ~44 k, scattered over the whole range — so a chunk is deliberately SMALL
/// (32 records, 1 KB): an all-dead chunk is released whole, and the smaller
/// the chunk the less of a survivor's neighbourhood it drags along. Measured
/// on that TUI, 256-record chunks held 7.15 MB for those 44 k records and
/// 32-record chunks 4.0 MB.
const CHUNK_SHIFT: usize = 5;
const CHUNK_LEN: usize = 1 << CHUNK_SHIFT;
const CHUNK_MASK: usize = CHUNK_LEN - 1;

/// Chunk pointers per directory page. The directory is two-level so its
/// size follows the LIVE id range, not the minted one: a long-running server
/// minting a billion ids over its life would otherwise carry a flat
/// `Vec<Option<Chunk>>` of 250 MB at 32 records per chunk. A page is 8 KB and
/// covers 32 K ids; a page whose chunks have all been released is dropped.
const PAGE_SHIFT: usize = 10;
const PAGE_LEN: usize = 1 << PAGE_SHIFT;
const PAGE_MASK: usize = PAGE_LEN - 1;

/// One lazily allocated run of `CHUNK_LEN` consecutive ids. The cells give
/// the table interior mutability through a shared slab reference: the
/// collector writes liveness bits and the `keys` word through raw record
/// pointers while other code holds only copies (`ShapeDescriptor`).
type Chunk = Box<[UnsafeCell<ShapeRecord>; CHUNK_LEN]>;

/// One directory page: `PAGE_LEN` chunk slots.
type Page = Box<[Option<Chunk>; PAGE_LEN]>;

fn new_chunk() -> Chunk {
    let mut v: Vec<UnsafeCell<ShapeRecord>> = Vec::with_capacity(CHUNK_LEN);
    v.resize_with(CHUNK_LEN, || UnsafeCell::new(ShapeRecord::EMPTY));
    // Exact length by construction; the conversion moves the allocation.
    v.into_boxed_slice()
        .try_into()
        .unwrap_or_else(|_| unreachable!("chunk vector has CHUNK_LEN cells"))
}

fn new_page() -> Page {
    let mut v: Vec<Option<Chunk>> = Vec::with_capacity(PAGE_LEN);
    v.resize_with(PAGE_LEN, || None);
    v.into_boxed_slice()
        .try_into()
        .unwrap_or_else(|_| unreachable!("page vector has PAGE_LEN slots"))
}

/// The by-id descriptor store. See the module docs.
pub(crate) struct ShapeSlab {
    pages: Vec<Option<Page>>,
    /// Present records.
    len: usize,
}

impl ShapeSlab {
    pub(super) fn new() -> Self {
        ShapeSlab {
            pages: Vec::new(),
            len: 0,
        }
    }

    #[inline]
    fn index_of(id: u32) -> Option<usize> {
        super::is_shape_id(id).then(|| (id - SHAPE_ID_BASE) as usize)
    }

    #[inline]
    fn id_of(index: usize) -> u32 {
        SHAPE_ID_BASE + index as u32
    }

    /// `(page, chunk within page, record within chunk)` of a slab index.
    #[inline]
    fn split(index: usize) -> (usize, usize, usize) {
        (
            index >> (CHUNK_SHIFT + PAGE_SHIFT),
            (index >> CHUNK_SHIFT) & PAGE_MASK,
            index & CHUNK_MASK,
        )
    }

    /// Present records.
    #[inline]
    pub(super) fn len(&self) -> usize {
        self.len
    }

    /// The record for `id`, or `None` when the id names no descriptor in this
    /// agent. The pointer stays valid until the record is removed; a removal
    /// only ever happens through the table's own retirement paths.
    #[inline]
    pub(super) fn record_ptr(&self, id: u32) -> Option<*mut ShapeRecord> {
        let index = Self::index_of(id)?;
        let (page, chunk, slot) = Self::split(index);
        let chunk = self.pages.get(page)?.as_ref()?[chunk].as_ref()?;
        let cell = chunk[slot].get();
        // SAFETY: the cell belongs to a live chunk owned by this slab; reads
        // and writes are serialized by the single-threaded agent discipline
        // every other shape-table access already relies on.
        if unsafe { (*cell).present() } {
            Some(cell)
        } else {
            None
        }
    }

    /// A copy of the record for `id`.
    #[inline]
    pub(super) fn get(&self, id: u32) -> Option<ShapeRecord> {
        // SAFETY: `record_ptr` only returns a cell of a live chunk.
        self.record_ptr(id).map(|p| unsafe { *p })
    }

    /// Lift `id` to the by-value descriptor.
    #[inline]
    pub(super) fn lift(&self, id: u32) -> Option<ShapeDescriptor> {
        // SAFETY: as in `get`.
        self.record_ptr(id).map(|p| unsafe { (*p).lift(p) })
    }

    /// Install `record` under `id`, allocating the page and chunk on first
    /// touch. Returns the record it replaced, if the id was already present.
    pub(super) fn insert(&mut self, id: u32, mut record: ShapeRecord) -> Option<ShapeRecord> {
        let index = Self::index_of(id).expect("ShapeSlab::insert: id outside the ShapeId range");
        record.flags |= RECORD_FLAG_PRESENT;
        let (page, chunk, slot) = Self::split(index);
        if page >= self.pages.len() {
            self.pages.resize_with(page + 1, || None);
        }
        let page = self.pages[page].get_or_insert_with(new_page);
        let chunk = page[chunk].get_or_insert_with(new_chunk);
        let cell = chunk[slot].get_mut();
        let previous = cell.present().then_some(*cell);
        *cell = record;
        if previous.is_none() {
            self.len += 1;
        }
        previous
    }

    /// Clear the record under `id`, returning it if it was present.
    pub(super) fn remove(&mut self, id: u32) -> Option<ShapeRecord> {
        let index = Self::index_of(id)?;
        let (page, chunk, slot) = Self::split(index);
        let chunk = self.pages.get_mut(page)?.as_mut()?[chunk].as_mut()?;
        let cell = chunk[slot].get_mut();
        if !cell.present() {
            return None;
        }
        let previous = *cell;
        *cell = ShapeRecord::EMPTY;
        self.len -= 1;
        Some(previous)
    }

    /// Visit every present record in id order. The callback may write
    /// through the record pointer; it must not insert or remove.
    pub(super) fn for_each(&self, mut f: impl FnMut(u32, *mut ShapeRecord)) {
        for (page_index, page) in self.pages.iter().enumerate() {
            let Some(page) = page else {
                continue;
            };
            for (chunk_index, chunk) in page.iter().enumerate() {
                let Some(chunk) = chunk else {
                    continue;
                };
                let base = ((page_index << PAGE_SHIFT) | chunk_index) << CHUNK_SHIFT;
                for (slot, cell) in chunk.iter().enumerate() {
                    let p = cell.get();
                    // SAFETY: live chunk, single-threaded agent.
                    if unsafe { (*p).present() } {
                        f(Self::id_of(base | slot), p);
                    }
                }
            }
        }
    }

    /// Every present id, in id order.
    #[cfg(test)]
    pub(super) fn ids(&self) -> Vec<u32> {
        let mut ids = Vec::with_capacity(self.len);
        self.for_each(|id, _| ids.push(id));
        ids
    }

    /// Free chunks that hold no present record, and pages that hold no
    /// chunk. Called once per major collection, after dead-key pruning:
    /// retirement is monotonic in id order for the common workload, so the
    /// oldest chunks empty first.
    pub(super) fn release_empty_chunks(&mut self) {
        for page in self.pages.iter_mut() {
            let Some(chunks) = page.as_mut() else {
                continue;
            };
            let mut live_chunks = 0usize;
            for chunk in chunks.iter_mut() {
                let empty = chunk
                    .as_ref()
                    .is_some_and(|c| c.iter().all(|cell| !unsafe { (*cell.get()).present() }));
                if empty {
                    *chunk = None;
                }
                if chunk.is_some() {
                    live_chunks += 1;
                }
            }
            if live_chunks == 0 {
                *page = None;
            }
        }
        while self.pages.last().is_some_and(Option::is_none) {
            self.pages.pop();
        }
        self.pages.shrink_to_fit();
    }

    #[cfg(test)]
    pub(super) fn clear(&mut self) {
        self.pages.clear();
        self.len = 0;
    }

    /// Bytes held: the page directory, every allocated page and every
    /// allocated chunk.
    pub(super) fn estimated_bytes(&self) -> usize {
        let mut pages = 0usize;
        let mut chunks = 0usize;
        for page in self.pages.iter().flatten() {
            pages += 1;
            chunks += page.iter().filter(|c| c.is_some()).count();
        }
        self.pages.capacity() * std::mem::size_of::<Option<Page>>()
            + pages * PAGE_LEN * std::mem::size_of::<Option<Chunk>>()
            + chunks * CHUNK_LEN * std::mem::size_of::<ShapeRecord>()
    }

    /// Allocated chunks (diagnostics).
    #[cfg(test)]
    pub(super) fn chunk_count(&self) -> usize {
        self.pages
            .iter()
            .flatten()
            .map(|page| page.iter().filter(|c| c.is_some()).count())
            .sum()
    }
}

/// MEASUREMENT that this structure is judged on, and the test's instrument.
///
/// Two counters on the id-list mutation path, kept unconditionally because the
/// rig falsifier and the unit guard both read them and a `cfg(test)` counter
/// can only prove the test's own arithmetic. Deliberately a THREE-field struct
/// in one `Cell`: a thread-local `Cell<T>` get/set copies `T` on every
/// operation, and this path runs millions of times per turn, so the width of
/// this type is itself a cost.
#[derive(Default, Clone, Copy)]
pub(crate) struct IdListOpStats {
    /// Removals that found their id.
    pub(crate) removals: u64,
    /// Elements shifted by a removal. Bytes = this x 4. Swap-remove moves
    /// none; `Vec::remove` moves the whole tail past the removed position.
    pub(crate) elems_moved: u64,
    /// Entries touched by a linear membership or position scan. The other
    /// half of the same defect: the index removes this too.
    pub(crate) positions_scanned: u64,
}

thread_local! {
    pub(crate) static ID_LIST_OP_STATS: std::cell::Cell<IdListOpStats> =
        const {
            std::cell::Cell::new(IdListOpStats {
                removals: 0,
                elems_moved: 0,
                positions_scanned: 0,
            })
        };
}

#[inline]
fn note_scan(entries: usize) {
    ID_LIST_OP_STATS.with(|c| {
        let mut st = c.get();
        st.positions_scanned += entries as u64;
        c.set(st);
    });
}

#[inline]
fn note_removal(elems_moved: usize) {
    ID_LIST_OP_STATS.with(|c| {
        let mut st = c.get();
        st.removals += 1;
        st.elems_moved += elems_moved as u64;
        c.set(st);
    });
}

/// One `[gc-idlist]` line per copying minor under `PERRY_GC_DIAG=1`,
/// cumulative. `elems_moved` is the rig falsifier for this change.
pub(crate) fn id_list_report() {
    if !crate::gc::gc_diag_enabled() {
        return;
    }
    let st = ID_LIST_OP_STATS.with(std::cell::Cell::get);
    if st.removals == 0 {
        return;
    }
    eprintln!(
        "[gc-idlist] removals={} elems_moved={} bytes_moved={} positions_scanned={}",
        st.removals,
        st.elems_moved,
        st.elems_moved * 4,
        st.positions_scanned,
    );
}

/// A spilled id list: the ids, plus an `id -> index` map built once the list
/// is large enough for a linear scan to cost more than a hash probe.
///
/// The index is what makes `remove_unordered`, `contains` and `position` O(1)
/// on the lists that actually get long. Below [`SPILL_INDEX_MIN`] it stays
/// empty and every operation is the linear scan it always was, because for a
/// handful of entries the scan is a single cache line and the map is not.
#[derive(Clone, Debug, Default)]
pub(super) struct SpillList {
    ids: Vec<u32>,
    /// Empty while `ids.len() < SPILL_INDEX_MIN`; complete above it.
    ///
    /// `PtrHasher` (#8125) is the right hasher here for the same reason it is
    /// on the maps around it: shape ids come from a monotonic counter, so the
    /// key is a small dense integer and the avalanche step is what keeps every
    /// one of them off bucket 0.
    pos: crate::fast_hash::PtrHashMap<u32, u32>,
}

/// Where the index starts paying. Measured shape of the problem: `families`
/// lists reach 514,030 entries on a claude-code reply while `by_facts` lists
/// are length 1, so anything in the low tens is far below the case that hurts
/// and far above the case where the map would be pure overhead.
const SPILL_INDEX_MIN: usize = 32;

impl SpillList {
    #[inline]
    fn indexed(&self) -> bool {
        !self.pos.is_empty()
    }

    /// Build the index if the list has just crossed the threshold. Called
    /// after every growth, so the map exists from the first entry past it.
    #[inline]
    fn maybe_build_index(&mut self) {
        if self.pos.is_empty() && self.ids.len() >= SPILL_INDEX_MIN {
            self.pos.reserve(self.ids.len());
            for (i, &id) in self.ids.iter().enumerate() {
                self.pos.insert(id, i as u32);
            }
        }
    }

    /// Position of `id`, O(1) when indexed and a counted linear scan below the
    /// threshold.
    #[inline]
    fn position(&self, id: u32) -> Option<usize> {
        if self.indexed() {
            return self.pos.get(&id).map(|&i| i as usize);
        }
        note_scan(self.ids.len());
        self.ids.iter().position(|&x| x == id)
    }

    #[inline]
    fn push(&mut self, id: u32) {
        let i = self.ids.len();
        self.ids.push(id);
        if self.indexed() {
            self.pos.insert(id, i as u32);
        } else {
            self.maybe_build_index();
        }
    }

    /// ORDER-PRESERVING removal, for a list whose order is load-bearing.
    /// O(n) in the tail by construction — that is what "preserve the order"
    /// costs — and it reindexes the shifted suffix.
    fn remove_ordered(&mut self, id: u32) -> Option<usize> {
        let pos = self.position(id)?;
        let moved = self.ids.len() - 1 - pos;
        note_removal(moved);
        self.ids.remove(pos);
        if self.indexed() {
            self.pos.remove(&id);
            for (i, &other) in self.ids.iter().enumerate().skip(pos) {
                self.pos.insert(other, i as u32);
            }
        }
        Some(pos)
    }

    /// UNORDERED removal: the last element takes the removed one's slot.
    /// Moves ONE element regardless of position, which is the whole point —
    /// the measured removals sit at position ~0.31 of a list up to 514,030
    /// long, so `Vec::remove` was shifting essentially the entire list every
    /// time.
    ///
    /// What this does NOT claim: that the memmove explains the bimodal turn
    /// CPU. On perrymaster one draw moved 335 GB and was as fast as a draw
    /// that moved 16 GB, so bytes moved is necessary but not sufficient for
    /// the slow mode. This removes work that is unambiguously wasted; how much
    /// TIME it removes is the A/B's to say.
    fn remove_unordered(&mut self, id: u32) -> Option<usize> {
        let pos = self.position(id)?;
        note_removal(if pos + 1 == self.ids.len() { 0 } else { 1 });
        let last = self.ids.len() - 1;
        self.ids.swap_remove(pos);
        if self.indexed() {
            self.pos.remove(&id);
            if pos != last {
                // The element that was last now lives at `pos`.
                self.pos.insert(self.ids[pos], pos as u32);
            }
        }
        Some(pos)
    }

    #[inline]
    fn replace(&mut self, old: u32, new: u32) -> bool {
        let Some(pos) = self.position(old) else {
            return false;
        };
        self.ids[pos] = new;
        if self.indexed() {
            self.pos.remove(&old);
            self.pos.insert(new, pos as u32);
        }
        true
    }
}

/// A compact list of descriptor ids: up to three inline, then a spilled
/// `Vec` with an `id -> index` map (see [`SpillList`]). Sized so a family-index
/// bucket is `(u64, IdList)` = 24 bytes.
///
/// # Order
/// Order is meaningful **for `by_facts` only**: [`IdList::push_front`] is how
/// an installed process-global id becomes the canonical answer for exact-facts
/// interning ahead of an equivalent local id (`install_external_shape_id`), and
/// that list is read first-wins. `families` is NOT order-sensitive: its only
/// order-touching reader is the "one descriptor stands for the family" choice
/// in the two rekey walks, which breaks on the first carrier and otherwise
/// takes any present member — and the chosen descriptor feeds exactly one
/// expression, `old_carrier || cache_carrier`, whose value is the same for
/// every carrier and the same for every non-carrier. The outcome is a function
/// of the SET, not of the order.
///
/// That asymmetry is why removal comes in two flavours:
/// [`IdList::remove_ordered`] for `by_facts` and [`IdList::remove_unordered`]
/// for `families`. **The caller declares the contract**, because the caller is
/// the one that knows whether its order is load-bearing; a single `remove` that
/// guessed would be the bug.
#[derive(Clone, Debug)]
pub(super) enum IdList {
    Inline { len: u8, ids: [u32; 3] },
    // The `Box` is the point: an inline `SpillList` is far wider and would make
    // every bucket pay for it; the spill is the rare case, so its extra
    // indirection is cheaper than those bytes on every family.
    Spill(Box<SpillList>),
}

const _: () = assert!(std::mem::size_of::<IdList>() == 16);

impl Default for IdList {
    fn default() -> Self {
        IdList::Inline {
            len: 0,
            ids: [0; 3],
        }
    }
}

impl IdList {
    #[inline]
    pub(super) fn as_slice(&self) -> &[u32] {
        match self {
            IdList::Inline { len, ids } => &ids[..*len as usize],
            IdList::Spill(v) => v.ids.as_slice(),
        }
    }

    #[inline]
    pub(super) fn len(&self) -> usize {
        self.as_slice().len()
    }

    #[inline]
    pub(super) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub(super) fn contains(&self, id: u32) -> bool {
        match self {
            IdList::Inline { len, ids } => ids[..*len as usize].contains(&id),
            IdList::Spill(v) => v.position(id).is_some(),
        }
    }

    fn spill(&mut self) -> &mut SpillList {
        if let IdList::Inline { len, ids } = self {
            let v = SpillList {
                ids: ids[..*len as usize].to_vec(),
                pos: crate::fast_hash::new_ptr_hash_map(),
            };
            *self = IdList::Spill(Box::new(v));
        }
        match self {
            IdList::Spill(v) => v,
            IdList::Inline { .. } => unreachable!(),
        }
    }

    /// Append `id` unless already present.
    pub(super) fn push_back(&mut self, id: u32) {
        if self.contains(id) {
            return;
        }
        self.append_unchecked(id);
    }

    /// Append an id the caller knows is not in this list.
    ///
    /// `alloc_shape_id` hands out a strictly increasing counter that is never
    /// reused (it parks at `SHAPE_ID_END` rather than wrapping), so an id that
    /// was allocated after this list was built cannot be in it, in this family
    /// or in any other. The membership scan in [`push_back`] is therefore dead
    /// work at the two interning sites, and it is not O(1) dead work: a family
    /// holds every descriptor ever created for one keys array, so the scan is
    /// linear in the history of that keys array and interning the *n*-th
    /// descriptor for it costs O(n) — quadratic over a render that keeps
    /// bumping a shape's semantic generation. `IdList::contains` was 6.2 % of
    /// main-thread leaf samples on a claude-code streamed reply, 95 % of it
    /// under `ShapeTableInner::family_push_back`.
    ///
    /// The spill index now removes that scan for the callers that cannot use
    /// this entry point, which is why `contains` is O(1) above
    /// [`SPILL_INDEX_MIN`]. This function stays because skipping the probe
    /// entirely is still cheaper than performing it.
    ///
    /// Callers that re-file an EXISTING id (the metadata rekey when a keys
    /// array moves) must keep using [`push_back`]: those ids can already be in
    /// the destination list.
    pub(super) fn append_unchecked(&mut self, id: u32) {
        match self {
            IdList::Inline { len, ids } if (*len as usize) < ids.len() => {
                ids[*len as usize] = id;
                *len += 1;
            }
            _ => self.spill().push(id),
        }
    }

    /// Prepend `id` unless already present. Order-preserving by definition, so
    /// it stays O(n) on a spilled list; only `by_facts` and the external-id
    /// install use it, and neither is on a hot path.
    pub(super) fn push_front(&mut self, id: u32) {
        if self.contains(id) {
            return;
        }
        match self {
            IdList::Inline { len, ids } if (*len as usize) < ids.len() => {
                ids.copy_within(0..*len as usize, 1);
                ids[0] = id;
                *len += 1;
            }
            _ => {
                let v = self.spill();
                v.ids.insert(0, id);
                if v.indexed() {
                    v.pos.clear();
                }
                v.maybe_build_index();
            }
        }
    }

    /// Drop `id` if present, PRESERVING the order of what remains; returns
    /// whether it was there. For a list whose order is load-bearing —
    /// `by_facts`, where the first entry is the canonical answer.
    pub(super) fn remove_ordered(&mut self, id: u32) -> bool {
        match self {
            IdList::Inline { len, ids } => Self::remove_inline(len, ids, id),
            IdList::Spill(v) => v.remove_ordered(id).is_some(),
        }
    }

    /// Drop `id` if present, WITHOUT preserving order; returns whether it was
    /// there. For `families`, whose readers are set-valued (see the type doc).
    ///
    /// This is the change: on a spilled list it moves ONE element instead of
    /// the whole tail.
    pub(super) fn remove_unordered(&mut self, id: u32) -> bool {
        match self {
            // Three entries: the inline shift is a single register move and
            // there is nothing to gain from disturbing the order.
            IdList::Inline { len, ids } => Self::remove_inline(len, ids, id),
            IdList::Spill(v) => v.remove_unordered(id).is_some(),
        }
    }

    #[inline]
    fn remove_inline(len: &mut u8, ids: &mut [u32; 3], id: u32) -> bool {
        let n = *len as usize;
        note_scan(n);
        let Some(pos) = ids[..n].iter().position(|&x| x == id) else {
            return false;
        };
        note_removal(n - 1 - pos);
        ids.copy_within(pos + 1..n, pos);
        ids[n - 1] = 0;
        *len -= 1;
        true
    }

    /// Replace `old` with `new` in place (keeps its position); returns
    /// whether `old` was present.
    pub(super) fn replace(&mut self, old: u32, new: u32) -> bool {
        match self {
            IdList::Inline { len, ids } => {
                let n = *len as usize;
                note_scan(n);
                match ids[..n].iter().position(|&x| x == old) {
                    Some(pos) => {
                        ids[pos] = new;
                        true
                    }
                    None => false,
                }
            }
            IdList::Spill(v) => v.replace(old, new),
        }
    }

    pub(super) fn heap_bytes(&self) -> usize {
        match self {
            IdList::Inline { .. } => 0,
            IdList::Spill(v) => {
                std::mem::size_of::<SpillList>()
                    + v.ids.capacity() * 4
                    // The index is the structure's memory cost and is reported
                    // rather than hidden: it exists only above SPILL_INDEX_MIN.
                    + v.pos.capacity() * (std::mem::size_of::<(u32, u32)>() + 1)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slab_records_are_addressed_by_id_and_keep_their_address() {
        let mut slab = ShapeSlab::new();
        let id_a = SHAPE_ID_BASE + 5;
        let id_b = SHAPE_ID_BASE + 5 + (CHUNK_LEN * PAGE_LEN) as u32 * 3;
        assert_eq!(slab.get(id_a), None);
        assert_eq!(
            slab.insert(
                id_a,
                ShapeRecord::new(0x1000, 1, 1, 0, ShapeObjectKind::Ordinary, 0)
            )
            .map(|r| r.keys),
            None
        );
        let a_ptr = slab.record_ptr(id_a).expect("present");
        // A later insert into another chunk must not move the first record.
        slab.insert(
            id_b,
            ShapeRecord::new(0x2000, 2, 2, 7, ShapeObjectKind::Class, 1),
        );
        assert_eq!(slab.record_ptr(id_a), Some(a_ptr));
        assert_eq!(slab.len(), 2);
        assert_eq!(slab.chunk_count(), 2);
        let b = slab.get(id_b).unwrap();
        assert_eq!(b.object_kind(), ShapeObjectKind::Class);
        assert_eq!(b.semantic_generation, 7);
        assert_eq!(b.hole_count, 1);
        assert!(b.facts_match(0x2000, 2, 2, 7, ShapeObjectKind::Class, 1));
        assert!(!b.facts_match(0x2000, 2, 2, 7, ShapeObjectKind::Ordinary, 1));
        // Ids outside the range and never-minted ids resolve to nothing.
        assert_eq!(slab.get(0), None);
        assert_eq!(slab.get(SHAPE_ID_BASE + 6), None);
        assert_eq!(slab.get(super::super::SHAPE_ID_END - 1), None);
        assert_eq!(slab.ids(), vec![id_a, id_b]);
        // Removal clears the record and, once a chunk is empty, the chunk.
        assert_eq!(slab.remove(id_a).map(|r| r.keys), Some(0x1000));
        assert_eq!(slab.remove(id_a), None);
        assert_eq!(slab.len(), 1);
        slab.release_empty_chunks();
        assert_eq!(slab.chunk_count(), 1);
        assert_eq!(slab.get(id_b).map(|r| r.keys), Some(0x2000));
        assert_eq!(slab.remove(id_b).map(|r| r.keys), Some(0x2000));
        slab.release_empty_chunks();
        assert_eq!(slab.chunk_count(), 0);
        assert_eq!(slab.estimated_bytes(), 0);
    }

    #[test]
    fn lifted_descriptor_mirrors_the_record_and_names_its_address() {
        let mut slab = ShapeSlab::new();
        let id = SHAPE_ID_BASE + 42;
        let mut record = ShapeRecord::new(0x3000, 4, 6, 9, ShapeObjectKind::Ordinary, 2);
        record.set(RECORD_FLAG_OLD_CARRIER, true);
        record.set(RECORD_FLAG_CACHE_CARRIER, true);
        record.set(RECORD_FLAG_FACTS_INDEXED, false);
        slab.insert(id, record);
        let ptr = slab.record_ptr(id).unwrap();
        let lifted = slab.lift(id).unwrap();
        assert_eq!(lifted.record, ptr as usize);
        assert_eq!(lifted.keys, 0x3000);
        assert_eq!(lifted.logical_key_count, 4);
        assert_eq!(lifted.live_inline_slot_count, 6);
        assert_eq!(lifted.semantic_generation, 9);
        assert_eq!(lifted.hole_count, 2);
        assert!(lifted.old_carrier);
        assert!(lifted.cache_carrier);
        assert!(!slab.get(id).unwrap().has(RECORD_FLAG_FACTS_INDEXED));
        assert_eq!(lifted.keys_slot(), Some(ptr as *mut u64));
        // Writing through the slot is what an evacuating visitor does.
        unsafe { *lifted.keys_slot().unwrap() = 0x4000 };
        assert_eq!(slab.get(id).unwrap().keys, 0x4000);
    }

    /// Varying any ONE fact must change the key: a fold that dropped a field
    /// would send two different shapes to one bucket for every value of it.
    #[test]
    fn facts_key_folds_every_field() {
        let base = facts_key(0x1111_2222_3333_4444, 7, 3, 9, ShapeObjectKind::Ordinary, 0);
        let variants = [
            (
                "keys",
                facts_key(0x5555_6666_7777_8888, 7, 3, 9, ShapeObjectKind::Ordinary, 0),
            ),
            (
                "logical",
                facts_key(0x1111_2222_3333_4444, 8, 3, 9, ShapeObjectKind::Ordinary, 0),
            ),
            (
                "live",
                facts_key(0x1111_2222_3333_4444, 7, 4, 9, ShapeObjectKind::Ordinary, 0),
            ),
            (
                "generation",
                facts_key(
                    0x1111_2222_3333_4444,
                    7,
                    3,
                    10,
                    ShapeObjectKind::Ordinary,
                    0,
                ),
            ),
            (
                "kind",
                facts_key(0x1111_2222_3333_4444, 7, 3, 9, ShapeObjectKind::Class, 0),
            ),
            (
                "holes",
                facts_key(0x1111_2222_3333_4444, 7, 3, 9, ShapeObjectKind::Ordinary, 1),
            ),
        ];
        for (field, key) in variants {
            assert_ne!(
                key, base,
                "changing `{field}` alone must change the facts key"
            );
        }
        let record = ShapeRecord::new(0x1111_2222_3333_4444, 7, 3, 9, ShapeObjectKind::Ordinary, 0);
        assert_eq!(record.facts_key_with_keys(0x1111_2222_3333_4444), base);
        assert_eq!(
            record.facts_key_with_keys(0x5555_6666_7777_8888),
            variants[0].1
        );
    }

    #[test]
    fn id_list_keeps_order_across_the_inline_to_spill_boundary() {
        let mut list = IdList::default();
        assert!(list.is_empty());
        list.push_back(2);
        list.push_back(3);
        list.push_front(1);
        list.push_back(2); // duplicate ignored
        assert_eq!(list.as_slice(), &[1, 2, 3]);
        assert!(matches!(list, IdList::Inline { .. }));
        list.push_back(4);
        assert!(matches!(list, IdList::Spill(_)));
        assert_eq!(list.as_slice(), &[1, 2, 3, 4]);
        list.push_front(0);
        assert_eq!(list.as_slice(), &[0, 1, 2, 3, 4]);
        // The ORDERED removal keeps this list's order, which is what
        // `by_facts` depends on.
        assert!(list.remove_ordered(2));
        assert!(!list.remove_ordered(2));
        assert_eq!(list.as_slice(), &[0, 1, 3, 4]);
        assert!(list.replace(3, 30));
        assert!(!list.replace(3, 300));
        assert_eq!(list.as_slice(), &[0, 1, 30, 4]);
        assert!(list.heap_bytes() >= 4 * 4);

        let mut inline = IdList::default();
        inline.push_back(7);
        inline.push_back(8);
        inline.push_back(9);
        assert!(inline.remove_ordered(8));
        assert_eq!(inline.as_slice(), &[7, 9]);
        assert!(inline.replace(9, 10));
        assert_eq!(inline.as_slice(), &[7, 10]);
        assert!(inline.remove_ordered(7));
        assert!(inline.remove_ordered(10));
        assert!(inline.is_empty());
        assert_eq!(inline.heap_bytes(), 0);
    }

    /// THE GUARD for this change, and it is an asymmetric one: the unordered
    /// removal must move O(1) elements per call, and the ordered one is
    /// allowed to move O(n) because that is what preserving the order costs.
    ///
    /// Front removal is the measured shape of the defect — removals sit at
    /// position ~0.31 of a list up to 514,030 long — so the test removes from
    /// the front, which is the worst case for `Vec::remove` and the best case
    /// for nothing.
    ///
    /// **Sabotage: point `remove_unordered` at `remove_ordered`.** The bound
    /// below is `4 * N`; the O(n) path moves `N * (N - 1) / 2` = 1,999,000
    /// elements for N = 2,000, i.e. 250x the bound, and this fails. A bound
    /// expressed as a MULTIPLE of N rather than an absolute is what makes the
    /// assertion about the complexity class instead of about one N.
    #[test]
    fn unordered_removal_moves_o1_elements_and_scans_o1_entries() {
        const N: u32 = 2_000;

        let baseline = ID_LIST_OP_STATS.with(std::cell::Cell::get);
        let mut list = IdList::default();
        for id in 1..=N {
            // The interning sites' entry point: no membership probe.
            list.append_unchecked(id);
        }
        assert_eq!(list.len(), N as usize);
        assert!(matches!(list, IdList::Spill(_)));

        // Remove every id from the FRONT of the list, in insertion order.
        for id in 1..=N {
            assert!(list.remove_unordered(id), "id {id} was not present");
        }
        assert!(list.is_empty());

        let after = ID_LIST_OP_STATS.with(std::cell::Cell::get);
        let moved = after.elems_moved - baseline.elems_moved;
        let scanned = after.positions_scanned - baseline.positions_scanned;
        let removals = after.removals - baseline.removals;
        assert_eq!(removals, u64::from(N));

        // O(1) per removal, with room for the swap itself.
        assert!(
            moved <= 4 * u64::from(N),
            "unordered removal moved {moved} elements for {N} removals — that \
             is the O(n) tail shift this structure exists to remove \
             (the ordered path would move {})",
            u64::from(N) * (u64::from(N) - 1) / 2
        );
        // The index answers `position`, so no linear scan may be charged for
        // a list this long. Sabotage: raise SPILL_INDEX_MIN above N and this
        // fails with ~N*N/2 scanned entries.
        assert!(
            scanned <= 4 * u64::from(N),
            "unordered removal scanned {scanned} entries for {N} removals — \
             the spill index is not answering `position`"
        );
    }

    /// The index must agree with the vector after every operation, including
    /// the swap that moves a third element nobody named. Checked exhaustively
    /// against a plain `Vec` oracle, because an index that drifts is a wrong
    /// ANSWER (a descriptor that cannot be found, or one found under the wrong
    /// id), not a slow one.
    ///
    /// Sabotage: drop the `self.pos.insert(self.ids[pos], pos as u32)` fixup
    /// in `remove_unordered` — the element the swap relocated keeps a stale
    /// index and the `contains` check below fails.
    #[test]
    fn the_spill_index_agrees_with_the_vector_after_every_operation() {
        let mut list = IdList::default();
        let mut oracle: Vec<u32> = Vec::new();
        for id in 1..=200u32 {
            list.append_unchecked(id);
            oracle.push(id);
        }
        // Remove a scattered third of them, front, middle and back.
        for &id in &[1u32, 2, 3, 100, 101, 199, 200, 50, 150, 7] {
            assert!(list.remove_unordered(id));
            oracle.retain(|&x| x != id);
        }
        // Same SET, whatever the order.
        let mut got = list.as_slice().to_vec();
        got.sort_unstable();
        let mut want = oracle.clone();
        want.sort_unstable();
        assert_eq!(got, want);
        // And every survivor is still findable through the index.
        for &id in &want {
            assert!(list.contains(id), "id {id} lost its index entry");
        }
        for &id in &[1u32, 2, 3, 100, 101, 199, 200, 50, 150, 7] {
            assert!(!list.contains(id), "removed id {id} is still findable");
        }
        // `replace` must keep the index coherent too.
        let survivor = want[0];
        assert!(list.replace(survivor, 9_999));
        assert!(!list.contains(survivor));
        assert!(list.contains(9_999));
    }

    /// A list that never reaches `SPILL_INDEX_MIN` must not allocate an index
    /// — the map is the structure's memory cost and it is only worth paying
    /// where the scan hurts. `by_facts` lists, measured at length 1 on cc,
    /// live entirely in this regime.
    #[test]
    fn a_short_spilled_list_builds_no_index() {
        let mut list = IdList::default();
        for id in 1..=8u32 {
            list.append_unchecked(id);
        }
        assert!(matches!(list, IdList::Spill(_)));
        match &list {
            IdList::Spill(v) => assert!(
                !v.indexed(),
                "a list of 8 built an index; SPILL_INDEX_MIN is {SPILL_INDEX_MIN}"
            ),
            IdList::Inline { .. } => unreachable!(),
        }
        // Still correct without one.
        assert!(list.remove_unordered(4));
        assert!(!list.contains(4));
        assert!(list.contains(8));
    }
}
