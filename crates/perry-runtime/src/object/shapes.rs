//! Agent-local authoritative object-shape descriptors (#8067).
//!
//! A shared `keys_array` already IS a shape (same pointer ⟹ same ordered
//! key list, because mutation always forks a private clone). This module
//! promotes that identity into an explicit per-shape key→slot table,
//! replacing two per-consumer tables that re-derived the same map:
//!
//! * `KEYS_INDEX` — keyed per OBJECT, so 10k same-shape siblings built 10k
//!   private indexes;
//! * `WIDE_KEY_INDEX` — keys-keyed but capped at a 4-entry LRU, so any
//!   working set past 4 wide shapes thrashed.
//!
//! The pointer-keyed key→slot index remains an accelerator: every hit still
//! re-validates the key bytes. Separately, every published `ShapeId` resolves
//! in this agent's `RuntimeState` to a descriptor containing the
//! ordered-keys edge plus the exact logical-key and live-inline-slot bounds.
//! The descriptor table is agent-local while ids are process-global. A live
//! object's ShapeId is authoritative for its ordered keys, logical-key count,
//! live inline-slot bound, and semantic generation. The one deliberately
//! mutable state is an owned ordinary object's #9064 stable-tombstone epoch:
//! per-slot `TAG_HOLE` validation lets its private keys array grow between
//! amortized squeezes without retiring unrelated cached slots.
//!
//! #8113 removed `ObjectHeader::field_count`, so the descriptor's
//! `live_inline_slot_count` is no longer a mirror of a header word — it is the
//! ONLY record of the bound. Every publication below is therefore
//! MINT-THEN-STAMP: the successor descriptor is fully installed while the
//! predecessor stamp is still readable, and the `parent_class_id` store is the
//! single, allocation-free publication point. A stamp-cleared window would be a
//! window in which the collector sees a live bound of 0 (#7154/#7164).
//! #8047 removed `ObjectHeader::keys_array`; consumers derive the edge from
//! this descriptor and no compatibility mirror remains.

use crate::array::ArrayHeader;
use std::cell::RefCell;

#[path = "shapes_slot_list.rs"]
mod shapes_slot_list;
#[path = "shapes_store.rs"]
mod shapes_store;
#[cfg(test)]
pub(crate) use shapes_slot_list::shape_descriptor_keys_slot;
pub(crate) use shapes_slot_list::shape_id_owns_keys_slot;
pub(crate) use shapes_slot_list::{
    object_shape_hole_count, publish_object_shape_holes,
    rekey_stable_tombstone_shape_after_squeeze, retire_owned_shape_history,
    shape_index_migrate_after_delete, shape_index_shift_in_place,
    try_update_stable_tombstone_shape, try_update_stable_tombstone_shape_cached, SlotList,
};
use shapes_store::{
    IdList, ShapeRecord, ShapeSlab, RECORD_FLAG_CACHE_CARRIER, RECORD_FLAG_CARRIED_SEEN,
    RECORD_FLAG_EXTERNAL_CARRIER, RECORD_FLAG_FACTS_INDEXED, RECORD_FLAG_OLD_CARRIER,
    RECORD_FLAG_OLD_CARRIER_SEEN,
};

#[derive(Clone)]
pub(crate) struct ShapeIndex {
    /// Key count covered by `slots`. Longer live array ⟹ catch up
    /// incrementally (append-only while shared); shorter ⟹ a delete
    /// compacted it — drop and rebuild on next lookup_ways.
    indexed_len: u32,
    /// FNV-1a content hash of key bytes → candidate slots (collisions
    /// resolved by the per-hit content validation).
    ///
    /// Keyed with [`crate::fast_hash::PtrHasher`], not the std default: the
    /// key is ALREADY a well-distributed FNV-1a hash, so running SipHash over
    /// it again buys no distribution and costs real time. On
    /// `bench_populated_delete.ts` — perry's worst object-model gap against
    /// node — `hash_one::<&usize>` plus `sip::Hasher::write` were **14.7% of
    /// self time**, second only to the lookup that performs them.
    slots: crate::fast_hash::PtrHashMap<u64, SlotList>,
}

/// Immutable facts named by one ShapeId, copied out of the table.
///
/// #8112: the table record's `keys` is the AUTHORITATIVE ordered-keys edge —
/// the collector marks it and rewrites it in place. Before #8112 the header
/// word was the sole strong edge and this field a weak copy that a post-visit
/// callback repaired. The inversion is what #8047 needs, because deleting the
/// header word must not unroot anything.
///
/// The table stores a packed [`ShapeRecord`] in a chunked slab whose record
/// addresses never move (#9706, `shapes_store.rs`); the incremental collector
/// retains enumerated slot addresses across budgeted resumptions, so that
/// stability is load-bearing. This value is the UNPACKED copy the rest of the
/// runtime consumes, and `record` carries the address of the slab record it
/// was lifted from so a traced receiver can hand the collector a rewritable
/// `keys` location without a second table probe (#8122's one-probe rule).
#[derive(Clone, Copy, Debug)]
pub(crate) struct ShapeDescriptor {
    /// Raw ArrayHeader address in Perry's fixed-width heap-word ABI. Keeping
    /// this u64 preserves identical representation on ILP32/LP64.
    pub(crate) keys: u64,
    /// Address of the slab record this value was lifted from, or 0 for a
    /// descriptor built outside the table (equality comparisons, tests).
    /// Never part of shape IDENTITY — see the hand-written `PartialEq` below.
    pub(crate) record: usize,
    /// Is this shape carried by at least one OLD-generation object?
    ///
    /// #8112's liveness gate. A minor never enumerates old objects, so the
    /// per-receiver edge cannot express "an old object still carries this
    /// shape" — and the record is SHARED, so no per-parent remembered-set
    /// entry can either (one sibling's rewrite creates an old→young edge for a
    /// parent the minor never visits). This flag is what the shape table roots
    /// on. It is sticky within an epoch and recomputed by every full trace, so
    /// it over-approximates by at most one full collection: exactly the
    /// generational contract, and never unconditional rooting.
    ///
    /// The record also keeps the notes accumulated since the last full trace
    /// (`RECORD_FLAG_OLD_CARRIER_SEEN`), adopted into this bit by
    /// [`rotate_old_carrier_epoch_after_full_trace`]; the copy carries only
    /// the adopted gate.
    pub(crate) old_carrier: bool,
    /// A runtime optimization cache can reinstall this historical shape even
    /// while no live object currently carries it. Such a cache is an explicit
    /// strong metadata owner, so collection must root and rewrite `keys` before
    /// weak descriptor pruning.
    pub(crate) cache_carrier: bool,
    pub(crate) logical_key_count: u32,
    pub(crate) live_inline_slot_count: u32,
    /// Zero for ordinary structural shapes. Descriptor/prototype mutations
    /// mint a process-unique nonzero generation so two semantically different
    /// layouts can never compare equal merely because their keys/counts do.
    pub(crate) semantic_generation: u64,
    /// Semantic receiver kind carried by this exact ShapeId. This is kept in
    /// the authoritative descriptor rather than `GcHeader::_reserved`, whose
    /// bits belong to the GC layout/age protocol and object feature flags.
    pub(crate) object_kind: ShapeObjectKind,
    /// Tombstoned key slots (`TAG_HOLE`) left by O(1) deletes; the live key
    /// count is `logical_key_count - hole_count`. Ordinarily immutable per id;
    /// an owned ordinary receiver carrying `OBJ_FLAG_STABLE_TOMBSTONES`
    /// updates this count in place while per-slot IC validation protects the
    /// stable id (#9064).
    pub(crate) hole_count: u32,
}

/// Shape identity is the FACTS, never the storage address. A descriptor value
/// lifted out of the table compares equal to the record it came from.
impl ShapeDescriptor {
    /// The one `keys` word the collector rewrites for this shape, or `None`
    /// for a descriptor value that was never lifted out of the table.
    ///
    /// `keys` is the first field of the `#[repr(C)]` slab record, so the
    /// record address IS the slot address.
    #[inline]
    pub(crate) fn keys_slot(&self) -> Option<*mut u64> {
        if self.record == 0 {
            return None;
        }
        Some(self.record as *mut u64)
    }
}

impl PartialEq for ShapeDescriptor {
    fn eq(&self, other: &Self) -> bool {
        self.keys == other.keys
            && self.logical_key_count == other.logical_key_count
            && self.live_inline_slot_count == other.live_inline_slot_count
            && self.semantic_generation == other.semantic_generation
            && self.object_kind == other.object_kind
            && self.hole_count == other.hole_count
    }
}

impl Eq for ShapeDescriptor {}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum ShapeObjectKind {
    Ordinary,
    Class,
}

/// Per-agent direct cache for the immutable `object_kind` half of a ShapeId.
/// A collision only falls back to the descriptor table. Entries contain no
/// managed address, and descriptor retirement clears a matching id before it
/// can be observed without the authoritative table record.
pub(crate) const SHAPE_KIND_CACHE_SIZE: usize = 16_384;
const SHAPE_KIND_CACHE_MASK: usize = SHAPE_KIND_CACHE_SIZE - 1;
const SHAPE_KIND_ORDINARY: u64 = 1;
const SHAPE_KIND_CLASS: u64 = 2;

#[inline(always)]
fn shape_kind_cache_slot(shape_id: u32) -> usize {
    let mixed = u64::from(shape_id).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    (mixed ^ (mixed >> 32)) as usize & SHAPE_KIND_CACHE_MASK
}

#[inline]
fn cached_shape_object_kind(shape_id: u32) -> Option<ShapeObjectKind> {
    let cache = unsafe { &mut *crate::state::state().object_hot.shape_kind_cache.get() };
    let packed = cache[shape_kind_cache_slot(shape_id)];
    if (packed >> 32) as u32 != shape_id {
        return None;
    }
    match packed & 0xFFFF_FFFF {
        SHAPE_KIND_ORDINARY => Some(ShapeObjectKind::Ordinary),
        SHAPE_KIND_CLASS => Some(ShapeObjectKind::Class),
        _ => None,
    }
}

#[inline]
fn publish_shape_object_kind(shape_id: u32, kind: ShapeObjectKind) {
    let cache = unsafe { &mut *crate::state::state().object_hot.shape_kind_cache.get() };
    let tag = match kind {
        ShapeObjectKind::Ordinary => SHAPE_KIND_ORDINARY,
        ShapeObjectKind::Class => SHAPE_KIND_CLASS,
    };
    cache[shape_kind_cache_slot(shape_id)] = (u64::from(shape_id) << 32) | tag;
}

#[inline]
fn retire_cached_shape_object_kind(shape_id: u32) {
    let cache = unsafe { &mut *crate::state::state().object_hot.shape_kind_cache.get() };
    let entry = &mut cache[shape_kind_cache_slot(shape_id)];
    if (*entry >> 32) as u32 == shape_id {
        *entry = 0;
    }
}

#[cfg(test)]
#[inline]
fn clear_shape_object_kind_cache() {
    let cache = unsafe { &mut *crate::state::state().object_hot.shape_kind_cache.get() };
    cache.fill(0);
}

struct ShapeTableInner {
    indices: crate::fast_hash::PtrHashMap<usize, ShapeIndex>,
    /// Exact-facts accelerator (#9706): the 64-bit fold of a descriptor's six
    /// identity facts (`shapes_store::facts_key`) -> the ids carrying those
    /// facts. Almost always one id; more than one is legal when a worker
    /// minted a local descriptor before a process-global module id arrived,
    /// or on a 64-bit collision — every hit re-validates the slab record, so
    /// a collision costs a second record read, never a wrong answer. This
    /// replaces the `ShapeFacts`-keyed map whose 32-byte key and 24-byte
    /// `Vec` value made it the largest of the old reverse indices.
    ///
    /// The key is a fold of internal shape state (never program input), so
    /// `PtrHasher` (#8125) is the right hasher: the word is already mixed.
    by_facts: crate::fast_hash::PtrHashMap<u64, IdList>,
    /// Keys-array address -> every descriptor id currently indexed under it.
    /// Same-address retirement, squeeze rekeying and GC relocation all work
    /// per family instead of per descriptor, and the family is what the
    /// metadata scan probes ONCE per keys array.
    ///
    /// A family is small by construction for a SHARED keys array, which is
    /// immutable (mutation forks a private clone): its descriptors differ only
    /// in the birth bound, a semantic generation, the class kind, or a
    /// tombstone count. An OWNED array grows in place, and every same-address
    /// publish retires the predecessor it just superseded
    /// (`retire_owned_shape_siblings`), so its family holds the current
    /// version plus at most the cache-carried ones. Without that retirement a
    /// dictionary built by ten thousand appends kept ten thousand prefix
    /// descriptors alive until the array died.
    ///
    /// Single-word key, so `PtrHasher` (#8125).
    families: crate::fast_hash::PtrHashMap<u64, IdList>,
    /// #9754: keys-array addresses a minor can act on — the families and
    /// slot indices whose keys array is not (yet) old. A minor-scoped
    /// `scan_shape_table_rekey_mut` visits only these; see `gc/young_log.rs`.
    young_keys: crate::gc::young_log::YoungLog<u64>,
}

const SHAPE_YOUNG_LOG_NAME: &str = "shapes.families+indices";

impl ShapeTableInner {
    /// Rule 1 of `gc/young_log.rs`: log a keys address BEFORE a family or a
    /// slot index is published under it, when the keys array is not old.
    /// Every family insert funnels through `family_push_back` /
    /// `family_push_front`; the slot-index inserts call this themselves.
    #[inline]
    fn note_young_keys(&mut self, keys: u64) {
        if crate::gc::young_log::addr_is_minor_relevant(keys as usize) {
            self.young_keys.note(keys);
        }
    }

    #[inline]
    fn family_push_back(&mut self, keys: u64, id: u32) {
        self.note_young_keys(keys);
        self.families.entry(keys).or_default().push_back(id);
    }

    /// Append a FRESHLY allocated id (see [`IdList::append_unchecked`]): the
    /// id came from `alloc_shape_id`, which never reuses a value, so the
    /// membership scan `family_push_back` would run is dead work that is
    /// linear in the number of descriptors this keys array has ever had.
    #[inline]
    fn family_append_fresh(&mut self, keys: u64, id: u32) {
        self.families.entry(keys).or_default().append_unchecked(id);
    }

    #[inline]
    fn family_push_front(&mut self, keys: u64, id: u32) {
        self.note_young_keys(keys);
        self.families.entry(keys).or_default().push_front(id);
    }

    /// Drop `id` from the family under `keys`, removing an emptied family.
    #[inline]
    fn family_remove(&mut self, keys: u64, id: u32) -> bool {
        let Some(ids) = self.families.get_mut(&keys) else {
            return false;
        };
        let removed = ids.remove(id);
        if ids.is_empty() {
            self.families.remove(&keys);
        }
        removed
    }

    #[inline]
    fn facts_push_back(&mut self, facts: u64, id: u32) {
        self.by_facts.entry(facts).or_default().push_back(id);
    }

    /// Fresh-id twin of [`ShapeTableInner::facts_push_back`]; same argument.
    #[inline]
    fn facts_append_fresh(&mut self, facts: u64, id: u32) {
        self.by_facts.entry(facts).or_default().append_unchecked(id);
    }

    #[inline]
    fn facts_push_front(&mut self, facts: u64, id: u32) {
        self.by_facts.entry(facts).or_default().push_front(id);
    }

    /// Drop `id` from the accelerator bucket `facts`, removing it if emptied.
    #[inline]
    fn facts_remove(&mut self, facts: u64, id: u32) -> bool {
        let Some(ids) = self.by_facts.get_mut(&facts) else {
            return false;
        };
        let removed = ids.remove(id);
        if ids.is_empty() {
            self.by_facts.remove(&facts);
        }
        removed
    }
}

pub(crate) struct ShapeTable {
    /// The by-id store, outside the `RefCell` on purpose: `shape_descriptor_by_id`
    /// is on the hot property path (profiling a dynamic-property loop put it
    /// and `shape_descriptor_ensure_with_generation` at ~13% of main-thread
    /// samples between them), and the collector reads and writes records
    /// through raw pointers from inside walks that hold `inner` borrowed.
    /// Records are cells; every access goes through a short-lived pointer.
    slab: std::cell::UnsafeCell<ShapeSlab>,
    inner: RefCell<ShapeTableInner>,
}

impl ShapeTable {
    pub(crate) fn new() -> Self {
        ShapeTable {
            slab: std::cell::UnsafeCell::new(ShapeSlab::new()),
            inner: RefCell::new(ShapeTableInner {
                indices: crate::fast_hash::new_ptr_hash_map(),
                by_facts: crate::fast_hash::new_ptr_hash_map(),
                families: crate::fast_hash::new_ptr_hash_map(),
                young_keys: crate::gc::young_log::YoungLog::new(),
            }),
        }
    }

    /// Shared view of the slab. Sound under the single-threaded agent
    /// discipline the whole table relies on; mutation happens only through
    /// [`Self::slab_mut`] in code that holds no other slab reference.
    #[inline]
    fn slab(&self) -> &ShapeSlab {
        // SAFETY: see the field docs — one agent, one thread, no reference
        // held across a call that can insert or remove.
        unsafe { &*self.slab.get() }
    }

    /// # Safety
    ///
    /// The caller holds no other reference into the slab for the duration.
    #[inline]
    #[allow(clippy::mut_from_ref)]
    unsafe fn slab_mut(&self) -> &mut ShapeSlab {
        &mut *self.slab.get()
    }
}

/// #6759 C3c: ShapeIds live in their own u32 range, disjoint from every
/// real class id (user counter tops out far below; builtin reserved
/// ranges sit at `0x7FFF_FF00..=0x7FFF_FFFF` and `0xFFFF_0000..`), so a
/// stamp in a plain object's `parent_class_id` can never be mistaken for
/// inheritance data — and vice versa.
pub(crate) const SHAPE_ID_BASE: u32 = 0x8000_0000;
/// Exclusive end of the ShapeId range (2^30 ids ≈ one per shape BIRTH,
/// unreachable in practice).
pub(crate) const SHAPE_ID_END: u32 = 0xC000_0000;

/// #6759 C3c: PROCESS-GLOBAL allocator (supersedes the per-thread counter
/// C3a landed with). Global uniqueness matters because the worker
/// serializer replays `parent_class_id` verbatim: a deep-copied object's
/// stamp arriving on another thread must never alias an id that thread
/// allocated for a different shape. Monotonic — ids are NEVER reused, so
/// a stale stamp or cache entry can only miss, not falsely hit.
static SHAPE_ID_NEXT: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(SHAPE_ID_BASE);

static SHAPE_SEMANTIC_NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

#[inline]
pub(crate) fn is_shape_id(v: u32) -> bool {
    (SHAPE_ID_BASE..SHAPE_ID_END).contains(&v)
}

/// #6804: classify a WIDENED shape token (`object_shape()`'s usize). Ids
/// stored as usize carry no high bits, so the full-width range test never
/// misclassifies a real heap address whose LOW 32 bits merely fall in the
/// id range (`is_shape_id(v as u32)` would).
#[inline]
pub(crate) fn is_shape_id_token(v: usize) -> bool {
    v >= SHAPE_ID_BASE as usize && v < SHAPE_ID_END as usize
}

/// Lifts a ShapeId into the per-site PIC token space. MUST match the literal
/// the PIC IR emits in
/// `perry-codegen/src/expr/property_get/generic_dispatch.rs`
/// (4611686018427387904 = 1 << 62).
pub(crate) const PIC_ID_TOKEN_BIT: u64 = 1 << 62;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ShapeIdExhausted;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ShapeDescriptorError {
    IdExhausted,
    InvalidFacts,
}

fn alloc_shape_id_from(next: &std::sync::atomic::AtomicU32) -> Result<u32, ShapeIdExhausted> {
    use std::sync::atomic::Ordering;
    loop {
        let id = next.load(Ordering::Relaxed);
        if id >= SHAPE_ID_END {
            // Park at the exclusive end. In particular, never fetch_add at
            // END: wrapping to zero could eventually alias a live ShapeId.
            next.store(SHAPE_ID_END, Ordering::Relaxed);
            return Err(ShapeIdExhausted);
        }
        if next
            .compare_exchange_weak(id, id + 1, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            return Ok(id);
        }
    }
}

fn alloc_shape_id() -> Result<u32, ShapeIdExhausted> {
    alloc_shape_id_from(&SHAPE_ID_NEXT)
}

/// Get or create the exact structural descriptor. The public allocation and
/// mutation paths turn exhaustion into a fail-stop before publishing an
/// untracked layout; the `Result` stays explicit so the allocator boundary and
/// its exhaustion tests remain reviewable.
fn shape_descriptor_ensure_with_generation(
    keys: *const ArrayHeader,
    logical_key_count: u32,
    live_inline_slot_count: u32,
    semantic_generation: u64,
    object_kind: ShapeObjectKind,
) -> Result<u32, ShapeDescriptorError> {
    shape_descriptor_ensure_with_holes(
        keys,
        logical_key_count,
        live_inline_slot_count,
        semantic_generation,
        object_kind,
        0,
    )
}

/// [`shape_descriptor_ensure_with_generation`] with an explicit tombstone
/// count — the publish half of an O(1) hole-delete, which must mint a shape
/// identity distinct from every hole state of the same array. Also the mint
/// for #9019's reserved-floor seed (`object/reserved_floor.rs`), whose keys
/// array is BORN with `floor` leading holes.
pub(crate) fn shape_descriptor_ensure_with_holes(
    keys: *const ArrayHeader,
    logical_key_count: u32,
    live_inline_slot_count: u32,
    semantic_generation: u64,
    object_kind: ShapeObjectKind,
    hole_count: u32,
) -> Result<u32, ShapeDescriptorError> {
    let keys_id = keys as usize as u64;
    if keys_id == 0 && logical_key_count != 0 {
        return Err(ShapeDescriptorError::InvalidFacts);
    }
    let facts = shapes_store::facts_key(
        keys_id,
        logical_key_count,
        live_inline_slot_count,
        semantic_generation,
        object_kind,
        hole_count,
    );
    let table = &crate::state::state().shapes;
    let mut inner = table.inner.borrow_mut();
    if let Some(ids) = inner.by_facts.get(&facts) {
        let slab = table.slab();
        for &id in ids.as_slice() {
            let Some(record) = slab.record_ptr(id) else {
                continue;
            };
            // SAFETY: live slab record, read immediately.
            let record = unsafe { *record };
            // The bucket is a 64-bit fold: validate the facts on every hit.
            if record.has(RECORD_FLAG_FACTS_INDEXED)
                && record.facts_match(
                    keys_id,
                    logical_key_count,
                    live_inline_slot_count,
                    semantic_generation,
                    object_kind,
                    hole_count,
                )
            {
                return Ok(id);
            }
        }
    }
    let id = alloc_shape_id().map_err(|_| ShapeDescriptorError::IdExhausted)?;
    let record = ShapeRecord::new(
        keys_id,
        logical_key_count,
        live_inline_slot_count,
        semantic_generation,
        object_kind,
        hole_count,
    );
    // Publish by-id first, then the reverse accelerators. An ObjectHeader is
    // stamped only after this function returns, so a visible id always has a
    // complete descriptor.
    // SAFETY: no slab reference is held; `slab()` above went out of scope.
    unsafe { table.slab_mut().insert(id, record) };
    // `id` was just handed out by `alloc_shape_id`, which never reuses a
    // value, so neither accelerator can already hold it: append without the
    // membership scan, whose cost is linear in this keys array's descriptor
    // history (see `IdList::append_unchecked`).
    inner.facts_append_fresh(facts, id);
    inner.family_append_fresh(keys_id, id);
    Ok(id)
}

pub(crate) fn shape_descriptor_ensure(
    keys: *const ArrayHeader,
    logical_key_count: u32,
    live_inline_slot_count: u32,
) -> Result<u32, ShapeDescriptorError> {
    shape_descriptor_ensure_with_generation(
        keys,
        logical_key_count,
        live_inline_slot_count,
        0,
        ShapeObjectKind::Ordinary,
    )
}

#[cold]
#[inline(never)]
fn shape_id_exhausted_abort() -> ! {
    eprintln!("Perry ShapeId space exhausted; refusing to publish an untracked object shape");
    std::process::abort()
}

#[cold]
#[inline(never)]
fn invalid_shape_facts_abort() -> ! {
    eprintln!("Perry internal error: refusing to publish invalid object shape facts");
    std::process::abort()
}

#[inline]
fn shape_descriptor_error_abort(error: ShapeDescriptorError) -> ! {
    match error {
        ShapeDescriptorError::IdExhausted => shape_id_exhausted_abort(),
        ShapeDescriptorError::InvalidFacts => invalid_shape_facts_abort(),
    }
}

#[inline]
pub(crate) fn publish_shape_result(result: Result<u32, ShapeDescriptorError>) -> u32 {
    match result {
        Ok(id) => id,
        Err(error) => shape_descriptor_error_abort(error),
    }
}

/// Compatibility mint for canonical shapes whose key and live-slot counts are
/// identical. New object-aware paths use [`shape_descriptor_ensure`] directly.
pub(crate) fn shape_id_for_keys_ensure(keys: *const ArrayHeader, key_count: u32) -> u32 {
    publish_shape_result(shape_descriptor_ensure(keys, key_count, key_count))
}

/// One FIELD of a shape's descriptor, without lifting the whole record.
///
/// [`shape_descriptor_by_id`] returns `ShapeDescriptor` **by value**, so every
/// caller that wants a single `u32` still copies the entire record out of the
/// table. That is most of them: `object_live_slot_count` — the slot bound
/// consulted on essentially every property read and write — throws away all
/// of it but `live_inline_slot_count`.
#[inline]
fn shape_descriptor_field_by_id<T>(shape_id: u32, read: impl Fn(&ShapeRecord) -> T) -> Option<T> {
    let record = crate::state::state().shapes.slab().record_ptr(shape_id)?;
    // SAFETY: `record_ptr` only returns a live slab record.
    Some(read(unsafe { &*record }))
}

/// The live inline-slot bound for `shape_id`, without copying its descriptor.
pub(crate) fn shape_live_inline_slot_count_by_id(shape_id: u32) -> Option<u32> {
    shape_descriptor_field_by_id(shape_id, |d| d.live_inline_slot_count)
}

/// The descriptor named by `shape_id`, or `None` when the id names no
/// descriptor in this agent.
///
/// #9706: a slab probe — range check, chunk index, record — with no hash,
/// no `RefCell` borrow and no invalidation epoch. The direct-mapped way cache
/// that used to front the hash map is gone because the slab IS that cache:
/// a hit was "mask, compare, deref" and a probe is "shift, index, deref".
#[inline]
pub(crate) fn shape_descriptor_by_id(shape_id: u32) -> Option<ShapeDescriptor> {
    crate::state::state().shapes.slab().lift(shape_id)
}

/// Immutable ordinary-vs-class fact with a pointer-free, per-agent direct
/// cache. The first observation remains the authoritative descriptor lookup_ways;
/// subsequent observations avoid the hot ShapeId HashMap borrow.
#[inline]
pub(crate) fn shape_object_kind_by_id(shape_id: u32) -> Option<ShapeObjectKind> {
    if let Some(kind) = cached_shape_object_kind(shape_id) {
        return Some(kind);
    }
    let kind = shape_descriptor_by_id(shape_id)?.object_kind;
    publish_shape_object_kind(shape_id, kind);
    Some(kind)
}

/// Record that a shape is carried by an OLD-generation receiver.
///
/// Called from the collector's slot visitor, which resolved the descriptor for
/// this receiver already, so the note costs a generation range check and a
/// byte store — no second shape-table probe (#8122's one-probe rule). The
/// store goes straight through the boxed record's address rather than
/// re-borrowing `ShapeTableInner`: the visitor runs inside walks that already
/// hold that borrow.
///
/// # Safety
///
/// `descriptor.record`, when non-zero, is the address of a live slab record
/// owned by this agent's shape table. Records are retired only by the table's
/// own retirement paths, and their chunk is released by
/// `shrink_shape_tables` at the end of a major collection — after every
/// enumeration of the cycle that produced this descriptor.
#[inline]
pub(crate) unsafe fn note_old_generation_carrier(descriptor: Option<ShapeDescriptor>) {
    let Some(descriptor) = descriptor else {
        return;
    };
    if descriptor.record == 0 {
        return;
    }
    let record = descriptor.record as *mut ShapeRecord;
    // GC_STORE_AUDIT(POINTER_FREE): liveness bookkeeping bits, never a heap reference.
    (*record).set(RECORD_FLAG_OLD_CARRIER | RECORD_FLAG_OLD_CARRIER_SEEN, true);
}

/// Note that a complete full trace visited a receiver carrying this shape.
/// Unlike the old-generation gate, this answers receiver liveness regardless
/// of generation and is consumed by post-trace descriptor retirement.
#[inline]
pub(crate) unsafe fn note_full_trace_carrier(descriptor: Option<ShapeDescriptor>) {
    let Some(descriptor) = descriptor else {
        return;
    };
    if descriptor.record != 0 {
        (*(descriptor.record as *mut ShapeRecord)).set(RECORD_FLAG_CARRIED_SEEN, true);
    }
}

#[inline]
pub(crate) unsafe fn note_external_shape_carrier(descriptor: Option<ShapeDescriptor>) {
    let Some(descriptor) = descriptor else {
        return;
    };
    if descriptor.record != 0 {
        (*(descriptor.record as *mut ShapeRecord)).set(RECORD_FLAG_EXTERNAL_CARRIER, true);
    }
}

/// Retain a descriptor while an agent-local optimization cache can reinstall
/// its ShapeId. Cache tables live with `RuntimeState`; the bit is recomputed
/// from live table occupancy after every full trace
/// (`array_tail_transition::recompute_cache_carriers_after_full_trace`), so a
/// descriptor whose last entry was evicted stops being rooted at the next full
/// trace — the same cadence as `old_carrier`.
#[inline]
pub(crate) unsafe fn note_cache_carrier(descriptor: Option<ShapeDescriptor>) {
    let Some(descriptor) = descriptor else {
        return;
    };
    if descriptor.record == 0 {
        return;
    }
    let record = descriptor.record as *mut ShapeRecord;
    // GC_STORE_AUDIT(POINTER_FREE): liveness bookkeeping bit, never a heap reference.
    (*record).set(RECORD_FLAG_CACHE_CARRIER, true);
}

/// The post-birth publication point for a ShapeId into a receiver's header
/// word: stamp, then register the carrier duty the stamp just created.
///
/// #9200: a receiver a MINOR WILL NOT ENUMERATE (old-gen, `gc_malloc`'d
/// large, immortal bootstrap) can be stamped with a descriptor whose keys
/// array is nursery-young. The receiver is invisible to the next minor, so
/// the descriptor's record is the ONLY path that can keep that keys array
/// alive — and an unarmed record is walked metadata-only by
/// `scan_shape_table_rekey_mut`. The keys array is then swept while live,
/// `prune_dead_shape_keys` drops the descriptor as dead, and the receiver
/// comes back shapeless: `Object.keys()` empty, every fixed-slot read
/// `undefined`. The tombstone-delete publish hit exactly this: it minted a
/// fresh unarmed descriptor for an already-promoted receiver and then
/// retired the armed predecessor in its keys-address sweep.
///
/// Arming here — in the same breath as the header store — makes the
/// old-carrier gate hold BY CONSTRUCTION for every publish routed through
/// this funnel, instead of relying on each publish site to remember the
/// note. The nursery test mirrors `visit_gc_layout_slot_descriptors`'
/// carrier note: "not in the nursery", never "in old-gen".
///
/// Over-approximation is the designed cost model: the gate is sticky within
/// an epoch and recomputed by every full trace, so arming a receiver that
/// dies young roots one record for at most one full collection — exactly
/// the generational contract (#8112).
#[inline]
pub(crate) unsafe fn stamp_object_shape_id_with_carrier_note(
    obj: *mut crate::object::ObjectHeader,
    id: u32,
) {
    (*obj).parent_class_id = id;
    if !crate::arena::pointer_in_nursery(obj as usize) {
        note_old_generation_carrier(shape_descriptor_by_id(id));
    }
}

/// Clear every `cache_carrier` bit ahead of the post-full-trace recompute.
pub(crate) fn clear_all_cache_carriers() {
    crate::state::state().shapes.slab().for_each(|_, record| {
        // SAFETY: live slab record, single-threaded agent.
        unsafe { (*record).set(RECORD_FLAG_CACHE_CARRIER, false) };
    });
}

/// Recompute the old-carrier gate from the trace that just finished.
///
/// A FULL trace enumerates every live object, so the notes it accumulated are
/// exactly the shapes old objects still carry; adopt them and clear both the
/// old-carrier accumulator and the all-generation carried note. The latter is
/// consumed by synchronous-full descriptor retirement immediately before this
/// rotation. Budgeted full cycles clear it without retiring because their
/// sliced trace is not a complete carrier census.
pub(crate) fn rotate_old_carrier_epoch_after_full_trace() {
    crate::state::state().shapes.slab().for_each(|_, record| {
        // SAFETY: live slab record, single-threaded agent.
        unsafe {
            let seen = (*record).has(RECORD_FLAG_OLD_CARRIER_SEEN);
            (*record).set(RECORD_FLAG_OLD_CARRIER, seen);
            (*record).set(RECORD_FLAG_OLD_CARRIER_SEEN, false);
            (*record).set(RECORD_FLAG_CARRIED_SEEN, false);
        }
    });
}

/// Mint (or retrieve) the ShapeId paired with canonical keys and equal
/// key/live-slot counts.
///
/// Codegen calls this once per class during module initialization and stores
/// the result beside `@perry_class_keys_*`. It deliberately takes a raw u64
/// rather than `*const ArrayHeader`: Perry's textual LLVM ABI represents the
/// rooted keys global as an integer heap word on every target.
#[no_mangle]
pub extern "C" fn js_object_shape_id_for_keys(keys: u64, key_count: u32) -> u32 {
    let id = shape_id_for_keys_ensure(keys as usize as *const ArrayHeader, key_count);
    // SAFETY: `id` was resolved from this agent's live slab record above.
    unsafe { note_external_shape_carrier(shape_descriptor_by_id(id)) };
    id
}

/// Mint a process-global ShapeId for a codegen-registered typed layout and
/// install its structural descriptor in the current agent. Unlike
/// [`shape_id_for_keys_ensure`], this deliberately does not canonicalise by
/// keys alone: two objects with identical property names but different raw
/// slot representations must never share a pre-baked GC descriptor.
pub(crate) fn mint_registered_typed_shape_id(keys: *const ArrayHeader, key_count: u32) -> u32 {
    let id = alloc_shape_id().unwrap_or_else(|_| shape_id_exhausted_abort());
    if !shapes_slot_list::install_external_shape_id(id, keys, key_count, key_count) {
        invalid_shape_facts_abort();
    }
    id
}

/// Install an already-minted process-global typed ShapeId in this agent (for
/// another module or worker that reuses the same compiled class identity).
pub(crate) fn install_registered_typed_shape_id(
    id: u32,
    keys: *const ArrayHeader,
    key_count: u32,
) -> bool {
    shapes_slot_list::install_external_shape_id(id, keys, key_count, key_count)
}

// ---------------------------------------------------------------------------
// #8067 — THE SHAPE WORD IS UNIFORM AND AUTHORITATIVE.
//
// `ObjectHeader.parent_class_id` is the shape word. Every shaped object is
// birth-stamped; inheritance lives in the class-id-keyed registry instead.
//
// The gate is gone. The rule is now, for every receiver kind:
//
//     the word is a ShapeId  <=>  is_shape_id(word)
//
// which is exactly what emitted PICs test: the ShapeId range and value, never a
// moving keys address or an ObjectHeader compatibility mirror.
// ---------------------------------------------------------------------------

/// True when `obj` really is an `ObjectHeader` whose word 2 may be written.
///
/// RegExp now has a distinct GC kind, so ShapeId publication never needs to
/// inspect an ObjectHeader payload word to distinguish it.
#[inline]
pub(crate) unsafe fn shape_word_is_writable(obj: *const crate::object::ObjectHeader) -> bool {
    crate::object::object_is_shaped(obj)
}

/// The receiver's ShapeId, or 0 when it is not a shaped object.
#[inline]
pub(crate) unsafe fn object_shape_stamp(obj: *const crate::object::ObjectHeader) -> u32 {
    let word = (*obj).parent_class_id;
    if is_shape_id(word) {
        word
    } else {
        0
    }
}

#[cfg(test)]
thread_local! {
    static TEST_CACHED_TRANSITION_WATCH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static TEST_CACHED_TRANSITION_STAMPS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Install a transition cache's exact successor without re-hashing descriptor
/// facts. The predecessor ShapeId is the complete semantic guard: it includes
/// the ordered keys edge, live-slot bound, semantic generation, and object
/// kind. A mismatch declines to the ordinary mint-or-find publication path.
///
/// The cache strongly roots `target_keys`, so copying GC rewrites both that
/// edge and the descriptor named by `target_shape_id` while the stable IDs stay
/// unchanged. Entries are learned only after the slow path has published the
/// successor, and ShapeIds are never reused.
#[inline]
pub(crate) unsafe fn install_cached_object_shape_transition(
    obj: *mut crate::object::ObjectHeader,
    expected_predecessor_shape_id: u32,
    target_shape_id: u32,
    _target_keys: *mut ArrayHeader,
) -> bool {
    let target_key_count = if _target_keys.is_null() {
        0
    } else {
        crate::array::keys_array_len_capped_to_capacity(_target_keys) as u32
    };
    install_cached_object_shape_version(
        obj,
        expected_predecessor_shape_id,
        target_shape_id,
        _target_keys,
        target_key_count,
    )
}

/// Install an exact historical shape version whose authoritative keys array
/// may have grown in place since the descriptor was minted. Reflection and
/// field tracing use the descriptor's logical bound, not the backing array's
/// later physical length.
#[inline]
pub(crate) unsafe fn install_cached_object_shape_version(
    obj: *mut crate::object::ObjectHeader,
    expected_predecessor_shape_id: u32,
    target_shape_id: u32,
    _target_keys: *mut ArrayHeader,
    _target_key_count: u32,
) -> bool {
    install_cached_object_shape_version_impl(
        obj,
        expected_predecessor_shape_id,
        target_shape_id,
        _target_keys,
        _target_key_count,
        false,
    )
}

/// Install a historical shape held by an optimization cache that permanently
/// owns the target descriptor and roots its keys array.
///
/// Unlike the general cached-shape entry, this does not need to probe the
/// shape table merely to note an old-generation carrier: `cache_carrier`
/// already keeps the descriptor and keys live for the lifetime of the cache,
/// which is strictly stronger than the epoch-scoped old-carrier note. The
/// Array-subclass tail cache establishes that ownership before publishing an
/// edge and never returns an unowned entry.
#[inline]
pub(crate) unsafe fn install_cache_carried_object_shape_version(
    obj: *mut crate::object::ObjectHeader,
    expected_predecessor_shape_id: u32,
    target_shape_id: u32,
    _target_keys: *mut ArrayHeader,
    _target_key_count: u32,
) -> bool {
    install_cached_object_shape_version_impl(
        obj,
        expected_predecessor_shape_id,
        target_shape_id,
        _target_keys,
        _target_key_count,
        true,
    )
}

#[inline]
unsafe fn install_cached_object_shape_version_impl(
    obj: *mut crate::object::ObjectHeader,
    expected_predecessor_shape_id: u32,
    target_shape_id: u32,
    _target_keys: *mut ArrayHeader,
    _target_key_count: u32,
    target_is_cache_carried: bool,
) -> bool {
    if obj.is_null()
        || !shape_word_is_writable(obj)
        || object_shape_stamp(obj) != expected_predecessor_shape_id
        || !is_shape_id(target_shape_id)
    {
        return false;
    }

    // Debug/test builds verify the cache-to-table invariant before trusting
    // the constant-time release publication. This lookup_ways is compiled out of
    // optimized release builds, where full-GC pruning validates both ShapeIds
    // and the cache's rooted target edge keeps its descriptor live.
    #[cfg(debug_assertions)]
    {
        if !shape_descriptor_by_id(target_shape_id).is_some_and(|descriptor| {
            descriptor.keys == _target_keys as u64
                && descriptor.logical_key_count == _target_key_count
                && (!target_is_cache_carried || descriptor.cache_carrier)
        }) {
            return false;
        }
    }

    // Match `set_object_keys_array_with_live`: representation feedback must be
    // invalidated while the predecessor stamp is still authoritative.
    super::mark_object_dynamic_shape_unknown(obj);
    if target_is_cache_carried {
        // `cache_carrier` already roots the target descriptor for the
        // cache's lifetime — strictly stronger than the epoch-scoped
        // old-carrier note, so this stamp deliberately skips the funnel's
        // descriptor probe (see the function doc above).
        (*obj).parent_class_id = target_shape_id;
    } else {
        stamp_object_shape_id_with_carrier_note(obj, target_shape_id);
    }

    #[cfg(debug_assertions)]
    if !_target_keys.is_null()
        && crate::array::keys_array_len_capped_to_capacity(_target_keys) as u32 == _target_key_count
    {
        debug_assert_object_shape_parity_for_keys(obj, _target_keys);
    }
    #[cfg(test)]
    TEST_CACHED_TRANSITION_WATCH.with(|watch| {
        if watch.get() == obj as usize {
            TEST_CACHED_TRANSITION_STAMPS.with(|hits| hits.set(hits.get() + 1));
        }
    });
    true
}

#[cfg(test)]
pub(crate) fn test_reset_cached_transition_stamps() {
    TEST_CACHED_TRANSITION_WATCH.with(|watch| watch.set(0));
    TEST_CACHED_TRANSITION_STAMPS.with(|hits| hits.set(0));
}

#[cfg(test)]
pub(crate) fn test_watch_cached_transition_stamps(obj: usize) {
    TEST_CACHED_TRANSITION_WATCH.with(|watch| watch.set(obj));
    TEST_CACHED_TRANSITION_STAMPS.with(|hits| hits.set(0));
}

#[cfg(test)]
pub(crate) fn test_cached_transition_stamps() -> u64 {
    TEST_CACHED_TRANSITION_STAMPS.with(std::cell::Cell::get)
}

/// Stamp `obj` with the exact ShapeId of `keys`, minting the descriptor on
/// first touch. Returns 0 only when the receiver is not a shaped object.
/// Exhaustion fails stop: no live object may depend on the
/// compatibility pointer/count mirrors for its shape.
#[inline]
pub(crate) unsafe fn stamp_object_shape(
    obj: *mut crate::object::ObjectHeader,
    keys: *const ArrayHeader,
    key_count: u32,
    live_inline_slot_count: u32,
) -> u32 {
    if !shape_word_is_writable(obj) {
        return 0;
    }
    let Some(lineage) = object_shape_descriptor(obj) else {
        crate::array::clear_array_subclass_named_prefix_token(obj);
        let id = shape_descriptor_ensure(keys, key_count, live_inline_slot_count)
            .unwrap_or_else(|error| shape_descriptor_error_abort(error));
        stamp_object_shape_id_with_carrier_note(obj, id);
        debug_assert_object_shape_parity(obj);
        return id;
    };
    let id = publish_shape_result(shape_descriptor_ensure_with_holes(
        keys,
        key_count,
        lineage.live_inline_slot_count,
        lineage.semantic_generation,
        lineage.object_kind,
        // Same-array restamp: physical holes persist, so must the count
        // (see the lineage publish below for the churn-growth rationale).
        lineage.hole_count,
    ));
    if id != (*obj).parent_class_id {
        // Read-side lookup_ways also calls `stamp_object_shape` to populate its
        // field cache. Preserve a proved Array-subclass prefix when that call
        // merely republishes the exact current descriptor; retire it only for
        // an actual structural identity change.
        crate::array::clear_array_subclass_named_prefix_token(obj);
    }
    stamp_object_shape_id_with_carrier_note(obj, id);
    debug_assert_object_shape_parity(obj);
    id
}

/// Birth-stamp a NEWBORN receiver with an already-minted ShapeId after checking
/// its descriptor against the completed header. A missing, foreign, or
/// count-mismatched id is replaced with an exact local descriptor. A valid
/// process-global id absent from this worker is installed with the worker's
/// local moving keys pointer before it is stamped.
///
/// Every allocator that installs a shape-cached keys array on a fresh
/// `ObjectHeader` must call this so all runtime and emitted guards observe the
/// same descriptor identity from birth.
///
/// `live_inline_slot_count` is the birth bound the allocator sized the object
/// with. #8113: it is a parameter rather than a `(*obj).field_count` read
/// because the header no longer carries the word — the descriptor this
/// publishes is the only record of it.
///
/// No `shape_word_is_writable` check beyond the null test: the callers have just
/// written `class_id` into a header they allocated, so the receiver is a genuine
/// `ObjectHeader` and never the `RegExpHeader` alias.
#[inline]
pub(crate) unsafe fn birth_stamp_object_shape(
    obj: *mut crate::object::ObjectHeader,
    runtime_shape_id: u32,
    live_inline_slot_count: u32,
) {
    if obj.is_null() || !shape_word_is_writable(obj) {
        return;
    }
    let current = object_shape_descriptor(obj).unwrap_or_else(|| {
        birth_publish_object_shape(obj, live_inline_slot_count);
        object_shape_descriptor(obj).expect("shape synchronization must publish a descriptor")
    });
    let keys = current.keys as usize as *mut ArrayHeader;
    let key_count = current.logical_key_count;
    let supplied_id_is_local =
        descriptor_matches_object(runtime_shape_id, obj, live_inline_slot_count)
            || shapes_slot_list::install_external_shape_id(
                runtime_shape_id,
                keys,
                key_count,
                live_inline_slot_count,
            );
    if supplied_id_is_local {
        stamp_object_shape_id_with_carrier_note(obj, runtime_shape_id);
        debug_assert_object_shape_parity(obj);
    } else {
        // `current` was just published from the newborn's explicit keys edge
        // and allocation bound, so it is already the exact descriptor.  The
        // cached id can legitimately disagree when an object reserves hidden
        // inline slots that have no public key (fs.Stats has 21 keys and four
        // hidden Date slots).  Before #8047 this fallback rebuilt the same
        // facts from the header's `keys_array` mirror.  With that mirror gone,
        // rebuilding through `birth_publish_object_shape` would instead use a
        // null edge and overwrite the exact 21/25 descriptor with a keyless
        // 0/25 one.  Keep the exact descriptor already stamped by
        // `set_object_keys_array_with_live`.
        debug_assert_object_shape_parity(obj);
    }
}

/// Stamp a newborn compiled-class allocation from the ShapeId installed at
/// module initialization, without re-canonicalizing the same shape facts.
///
/// A hit proves the immutable ordered-keys edge, logical key count, and live
/// inline-slot bound directly from the agent-local descriptor. The id and keys
/// pointer arrive through separate module globals, so every structural fact is
/// checked before the single stamp store. Missing worker-local ids, key-count
/// drift, and learned-width mismatches return `false` for the existing
/// mint-and-validate path to handle.
///
/// # Safety
///
/// `obj` must be a freshly allocated, unpublished `ObjectHeader` and `keys`
/// must be the module-init canonical keys pointer paired with
/// `runtime_shape_id`. No allocation or collection may occur between this
/// function returning `true` and initialization of the newborn's fields.
#[inline]
pub(crate) unsafe fn try_birth_stamp_preinstalled_shape(
    obj: *mut crate::object::ObjectHeader,
    runtime_shape_id: u32,
    keys: *mut ArrayHeader,
    live_inline_slot_count: u32,
) -> bool {
    if obj.is_null() {
        return false;
    }
    let Some(descriptor) = shape_descriptor_by_id(runtime_shape_id) else {
        return false;
    };
    let logical_key_count = if keys.is_null() {
        0
    } else {
        crate::array::keys_array_len_capped_to_capacity(keys) as u32
    };
    if descriptor.keys != keys as u64
        || descriptor.logical_key_count != logical_key_count
        || descriptor.live_inline_slot_count != live_inline_slot_count
        || descriptor.semantic_generation != 0
        || descriptor.object_kind != ShapeObjectKind::Ordinary
    {
        return false;
    }
    (*obj).parent_class_id = runtime_shape_id;
    if !crate::arena::pointer_in_nursery(obj as usize) {
        note_old_generation_carrier(Some(descriptor));
    }
    debug_assert_object_shape_parity(obj);
    true
}

/// Publish the exact descriptor for a FRESHLY ALLOCATED header. #8113: the
/// birth live-slot bound must be supplied because no header word carries it.
///
/// Mint-then-stamp: `shape_descriptor_ensure_with_generation` can collect, and
/// at that point the object is still unstamped, which is sound only because it
/// is also still unpublished — the allocator has not returned it and no live
/// edge reaches it. Every LATER bound change goes through
/// [`publish_object_live_slot_count`], which keeps a valid predecessor stamp
/// across the mint.
#[inline]
pub(crate) unsafe fn birth_publish_object_shape(
    obj: *mut crate::object::ObjectHeader,
    live_inline_slot_count: u32,
) -> u32 {
    synchronize_object_shape_descriptor_from(obj, None, live_inline_slot_count)
}

/// Publish a new live inline-slot bound for an ALREADY PUBLISHED object.
///
/// This is the #8113 replacement for `(*obj).field_count = n`. The successor
/// descriptor is minted while the predecessor stamp is still installed, so a
/// collection inside the mint observes the OLD bound — correct, because the
/// slot the caller is about to expose has not been written yet — and the new
/// bound becomes visible at the single `parent_class_id` store, which cannot
/// allocate and therefore cannot collect.
pub(crate) unsafe fn publish_object_live_slot_count(
    obj: *mut crate::object::ObjectHeader,
    live_inline_slot_count: u32,
) -> u32 {
    if obj.is_null() || !shape_word_is_writable(obj) {
        return 0;
    }
    let predecessor = object_shape_descriptor(obj);
    if let Some(current) = predecessor {
        if current.live_inline_slot_count == live_inline_slot_count {
            debug_assert_object_shape_parity(obj);
            return object_shape_stamp(obj);
        }
    }
    synchronize_object_shape_descriptor_from(obj, predecessor, live_inline_slot_count)
}

/// Install the exact descriptor for the object's current authoritative keys
/// edge, preserving the live inline-slot bound the receiver already carries.
/// This is the only structural shape publication operation used by mutations.
/// Keyless objects receive a descriptor too.
///
/// #8113: an UNSTAMPED receiver has no recorded bound anywhere, so this
/// publishes 0 for it rather than inventing one. Callers that know the bound
/// (allocators, the by-name append path) must use
/// [`birth_publish_object_shape`] / [`publish_object_live_slot_count`].
pub(crate) unsafe fn synchronize_object_shape_descriptor(
    obj: *mut crate::object::ObjectHeader,
) -> u32 {
    let predecessor = object_shape_descriptor(obj);
    let live = predecessor
        .map(|descriptor| descriptor.live_inline_slot_count)
        .unwrap_or(0);
    synchronize_object_shape_descriptor_from(obj, predecessor, live)
}

/// Structural synchronization across a keys-edge or slot-bound mutation.
/// `predecessor` carries semantic lineage (including class kind) across the
/// mutation without exposing stale structural facts.
///
/// MINT-THEN-STAMP (#8113): every allocation below happens with the
/// predecessor stamp still installed; the receiver's published shape changes at
/// the final `parent_class_id` store and nowhere else.
pub(crate) unsafe fn synchronize_object_shape_descriptor_from(
    obj: *mut crate::object::ObjectHeader,
    predecessor: Option<ShapeDescriptor>,
    live_inline_slot_count: u32,
) -> u32 {
    if obj.is_null() {
        return 0;
    }
    let keys = predecessor
        .map(|descriptor| descriptor.keys as usize as *mut ArrayHeader)
        .unwrap_or(std::ptr::null_mut());
    publish_object_shape_from(obj, predecessor, keys, live_inline_slot_count)
}

/// Publish the exact descriptor for an EXPLICIT keys edge — which may not be
/// the one the header currently holds.
///
/// This is what makes the keys-edge mutation mint-then-stamp (#8113). The
/// caller stamps the successor here, with the predecessor still describing the
/// current edge throughout every allocation inside. The final ShapeId store is
/// the atomic publication point for the new descriptor and its rooted edge.
pub(crate) unsafe fn publish_object_shape_from(
    obj: *mut crate::object::ObjectHeader,
    predecessor: Option<ShapeDescriptor>,
    keys: *mut ArrayHeader,
    live_inline_slot_count: u32,
) -> u32 {
    if obj.is_null() || !shape_word_is_writable(obj) {
        return 0;
    }
    // Generic structural publication may add/delete/reorder a named field.
    // The learned exact numeric-tail installer has its own entry point and
    // intentionally preserves this Array-subclass family proof.
    crate::array::clear_array_subclass_named_prefix_token(obj);
    let key_count = if keys.is_null() {
        0
    } else {
        crate::array::keys_array_len_capped_to_capacity(keys) as u32
    };

    // A same-address length change is legal only for an owned keys array. A
    // shared array must have cloned before push; otherwise siblings already
    // observe mutated bytes and no descriptor can make that state sound.
    let old_id = object_shape_stamp(obj);
    let mut retire_owned_history = false;
    if let Some(old) = shape_descriptor_by_id(old_id) {
        // #9064: an owned ordinary receiver that already entered stable-
        // tombstone mode keeps its id across same-allocation tail appends and
        // live-bound growth. Cached slots validate `TAG_HOLE`, so the deleted
        // slot stays a miss while every surviving slot remains valid. A keys
        // reallocation declines inside the helper and takes the ordinary
        // mint-then-stamp path below.
        if let Some(id) = try_update_stable_tombstone_shape(
            obj,
            keys,
            key_count,
            live_inline_slot_count,
            old.hole_count,
        ) {
            return id;
        }
        if old.keys == keys as u64 && old.logical_key_count != key_count {
            // #8113: these three arms are unreachable-by-construction defenses
            // (`debug_assert!` below). They deliberately leave the receiver
            // STAMPED with its predecessor rather than clearing: an unstamped
            // object now has no live-slot bound at all, so clearing would turn
            // a shape-identity fault into heap-payload loss.
            let Some(gc) = crate::value::addr_class::try_read_tracked_gc_header(keys as usize)
            else {
                return old_id;
            };
            if (*gc.as_ptr()).obj_type != crate::gc::GC_TYPE_ARRAY {
                return old_id;
            }
            let shared = (*gc.as_ptr()).gc_flags & crate::gc::GC_FLAG_SHAPE_SHARED != 0;
            debug_assert!(
                !shared,
                "shared keys array mutated in place under an immutable ShapeId"
            );
            if shared {
                return old_id;
            }
            // An Array-subclass receiver is the one owner whose history IS
            // reinstalled: `array_tail_transition` learns the (predecessor,
            // successor) pair right after this publish returns and its
            // reverse edge stamps the predecessor back on `pop`. That cache
            // takes ownership through `cache_carrier`, but only once the
            // learner has run, so the gate here is the receiver kind the
            // learner is scoped to (`record_array_tail` in the append tail).
            retire_owned_history = !crate::array::is_array_subclass_class_id((*obj).class_id);
        }
    }

    // A caller-supplied predecessor was captured before it temporarily
    // cleared the stamp to mutate structural facts, so it is the semantic
    // authority for this transition. A re-entrant observer can defensively
    // self-heal the zero stamp in that window; never let that interim
    // descriptor replace the saved class/semantic lineage.
    let lineage = predecessor.or_else(|| shape_descriptor_by_id(old_id));
    let semantic_generation = lineage
        .map(|descriptor| descriptor.semantic_generation)
        .unwrap_or(0);
    let object_kind = lineage
        .map(|descriptor| descriptor.object_kind)
        .unwrap_or(ShapeObjectKind::Ordinary);
    // Tombstones (#9029): an append or grow-realloc keeps every hole slot
    // physically in the array, so the successor must inherit the count — a
    // reset would let delete/re-add churn dodge the squeeze threshold
    // forever and grow the array unbounded. Only the squeeze itself (which
    // physically removes the holes) publishes 0, explicitly.
    let hole_count = lineage.map(|descriptor| descriptor.hole_count).unwrap_or(0);
    let id = publish_shape_result(shape_descriptor_ensure_with_holes(
        keys,
        key_count,
        live_inline_slot_count,
        semantic_generation,
        object_kind,
        hole_count,
    ));
    stamp_object_shape_id_with_carrier_note(obj, id);
    if retire_owned_history {
        // #9706: the array is OWNED, so this receiver was the only carrier of
        // every earlier same-address version, and the stamp above just
        // superseded the last of them. Retire the growth history now rather
        // than leaving one prefix descriptor per append alive until the
        // array itself dies: on the compiled claude-code TUI that history was
        // most of the descriptor table. Ordered after the stamp for the same
        // reason as the tombstone publish (#9200) — the successor must be
        // armed before the armed predecessor goes.
        retire_owned_shape_siblings(keys as u64, id);
    }
    debug_assert_object_shape_parity_for_keys(obj, keys);
    id
}

/// Retire every descriptor of an OWNED keys array other than `keep`.
///
/// Sound because `GC_FLAG_SHAPE_SHARED` is sticky: an array without it has
/// had exactly one owner for its whole life, and that owner now carries
/// `keep`. A stale IC token already misses on the stamp compare and
/// `shape_descriptor_by_id` of a retired id is `None`, so nothing can observe
/// the retired versions — with one exception: a descriptor an optimization
/// cache permanently owns (`cache_carrier`) may be reinstalled by that cache
/// while no live object carries it, so it stays.
fn retire_owned_shape_siblings(keys: u64, keep: u32) {
    let table = &crate::state::state().shapes;
    let mut inner = table.inner.borrow_mut();
    let stale: Vec<u32> = inner
        .families
        .get(&keys)
        .map(|ids| {
            ids.as_slice()
                .iter()
                .copied()
                .filter(|&id| {
                    id != keep
                        && table.slab().get(id).is_some_and(|record| {
                            !record.has(RECORD_FLAG_CACHE_CARRIER | RECORD_FLAG_EXTERNAL_CARRIER)
                        })
                })
                .collect()
        })
        .unwrap_or_default();
    for id in stale {
        remove_descriptor_and_reverse_indices(&mut inner, id);
    }
}

/// Mint an exact successor for a descriptor/prototype semantic transition.
/// The structural facts remain unchanged, but the process-unique generation
/// prevents a cache trained before the transition from comparing equal after
/// it. Shared siblings retain their immutable predecessor descriptor.
pub(crate) unsafe fn transition_object_shape_semantics(
    obj: *mut crate::object::ObjectHeader,
) -> u32 {
    if obj.is_null() || !shape_word_is_writable(obj) {
        return 0;
    }
    crate::array::clear_array_subclass_named_prefix_token(obj);
    let current = object_shape_descriptor(obj).unwrap_or_else(|| {
        synchronize_object_shape_descriptor(obj);
        object_shape_descriptor(obj).expect("shape synchronization must publish a descriptor")
    });
    let keys = current.keys as usize as *mut ArrayHeader;
    let key_count = current.logical_key_count;
    let generation = SHAPE_SEMANTIC_NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if generation == 0 {
        shape_id_exhausted_abort();
    }
    let id = publish_shape_result(shape_descriptor_ensure_with_generation(
        keys,
        key_count,
        current.live_inline_slot_count,
        generation,
        current.object_kind,
    ));
    stamp_object_shape_id_with_carrier_note(obj, id);
    debug_assert_object_shape_parity(obj);
    id
}

/// Publish the successor shape for an O(1) hole-delete on `obj`'s CURRENT
/// keys array: same address, same surviving slots, one more tombstone.
///
/// Modeled on [`transition_object_shape_semantics`]: the structural facts are
/// unchanged except `hole_count`, and the fresh process-unique generation is
/// what retires every cached `(token, key)` pair for this receiver — a
/// deleted key must stop hitting even though the array address and every
/// surviving slot are byte-identical, or a stale IC hit would return the
/// cleared slot instead of walking the prototype chain.
///
/// Returns the successor id, or 0 when the object is not stamped/shaped —
/// the caller falls back to the compacting delete.

/// Turn a class-expression object into a class receiver. The kind is part of
/// the exact immutable descriptor, so it cannot alias GC layout bits and every
/// pre-mark ShapeId guard permanently misses afterward.
///
/// Unlike a general semantic transition, changing `object_kind` already makes
/// the descriptor facts distinct. Preserve the predecessor generation so
/// repeated evaluations of the same class expression reuse one class-shaped
/// descriptor. Minting a fresh generation here retained one descriptor per
/// evaluation as long as their shared keys array stayed live (one million
/// evaluations consumed hundreds of MB).
pub(crate) unsafe fn transition_object_shape_to_class(
    obj: *mut crate::object::ObjectHeader,
) -> u32 {
    if obj.is_null() || !shape_word_is_writable(obj) {
        return 0;
    }
    crate::array::clear_array_subclass_named_prefix_token(obj);
    let current = object_shape_descriptor(obj).unwrap_or_else(|| {
        synchronize_object_shape_descriptor(obj);
        object_shape_descriptor(obj).expect("shape synchronization must publish a descriptor")
    });
    if current.object_kind == ShapeObjectKind::Class {
        return object_shape_stamp(obj);
    }
    let id = publish_shape_result(shape_descriptor_ensure_with_generation(
        current.keys as usize as *const ArrayHeader,
        current.logical_key_count,
        current.live_inline_slot_count,
        current.semantic_generation,
        ShapeObjectKind::Class,
    ));
    stamp_object_shape_id_with_carrier_note(obj, id);
    debug_assert_object_shape_parity(obj);
    id
}

/// Authoritative descriptor for a genuine shaped object.
#[inline]
pub(crate) unsafe fn object_shape_descriptor(
    obj: *const crate::object::ObjectHeader,
) -> Option<ShapeDescriptor> {
    shape_descriptor_by_id(object_shape_stamp(obj))
}

#[inline]
pub(crate) unsafe fn object_shape_id(obj: *const crate::object::ObjectHeader) -> u32 {
    object_shape_descriptor(obj)
        .map(|_| object_shape_stamp(obj))
        .unwrap_or(0)
}

/// Retire `id` from the by-id store and the family index (#9706).
///
/// The family index is keyed by the address the descriptor was indexed under.
/// Between a live receiver rewriting the record's `keys` word and the
/// metadata scan moving the family, the two can name different addresses; a
/// removal in that window leaves the id in the stale family, where every
/// walk skips it (`record_ptr` is `None`) and the next scan drops it.
fn remove_descriptor_and_reverse_indices(inner: &mut ShapeTableInner, id: u32) {
    let table = &crate::state::state().shapes;
    let Some(indexed) = table.slab().get(id).map(|record| record.keys) else {
        return;
    };
    remove_descriptor_indexed_under(inner, id, indexed);
}

/// [`remove_descriptor_and_reverse_indices`] for a caller that knows the
/// address the id is indexed under — the metadata scan, which retires a
/// family whose keys address was recycled while a live edge may already have
/// rewritten the records to the forwarded address.
fn remove_descriptor_indexed_under(inner: &mut ShapeTableInner, id: u32, indexed: u64) {
    let table = &crate::state::state().shapes;
    // SAFETY: no slab reference is held by the caller across this call.
    let Some(record) = (unsafe { table.slab_mut().remove(id) }) else {
        return;
    };
    retire_cached_shape_object_kind(id);
    if record.has(RECORD_FLAG_FACTS_INDEXED) {
        inner.facts_remove(record.facts_key_with_keys(indexed), id);
    }
    inner.family_remove(indexed, id);
}

/// Exact-facts test for a candidate id against the receiver's authoritative
/// header facts. #8113: the live bound is a PARAMETER — the header no longer
/// mirrors it, so the caller supplies the bound it is claiming.
fn descriptor_matches_object(
    shape_id: u32,
    obj: *const crate::object::ObjectHeader,
    live_inline_slot_count: u32,
) -> bool {
    let Some(d) = shape_descriptor_by_id(shape_id) else {
        return false;
    };
    unsafe {
        d.keys == crate::object::object_keys_array(obj) as u64
            && d.logical_key_count == object_header_key_count(obj)
            && d.live_inline_slot_count == live_inline_slot_count
    }
}

#[inline]
unsafe fn object_header_key_count(obj: *const crate::object::ObjectHeader) -> u32 {
    let keys = crate::object::object_keys_array(obj);
    if keys.is_null() {
        0
    } else {
        crate::array::keys_array_len_capped_to_capacity(keys) as u32
    }
}

/// #8113: the live-slot bound is no longer independently observable, so parity
/// is now exactly "the stamp resolves, and its structural keys facts match the
/// keys edge the receiver is about to carry". The bound cannot disagree with
/// itself.
#[inline]
pub(crate) unsafe fn debug_assert_object_shape_parity(obj: *const crate::object::ObjectHeader) {
    debug_assert_object_shape_parity_for_keys(obj, crate::object::object_keys_array(obj));
}

/// Parity against an EXPLICIT keys edge.
///
/// `publish_object_shape_from` stamps the successor before the header store
/// (that is what makes the keys mutation mint-then-stamp), so for that one
/// window the authoritative edge is the caller's argument, not the header word.
#[inline]
pub(crate) unsafe fn debug_assert_object_shape_parity_for_keys(
    obj: *const crate::object::ObjectHeader,
    keys: *mut ArrayHeader,
) {
    let id = object_shape_stamp(obj);
    if id != 0 {
        let key_count = if keys.is_null() {
            0
        } else {
            crate::array::keys_array_len_capped_to_capacity(keys) as u32
        };
        debug_assert!(
            shape_descriptor_by_id(id)
                .is_some_and(|d| { d.keys == keys as u64 && d.logical_key_count == key_count }),
            "published ShapeId disagrees with authoritative ObjectHeader facts"
        );
    }
}

/// Drop the stamp iff the word currently holds one, leaving a real
/// `parent_class_id` untouched. Returns true when a stamp was cleared.
///
/// # TEST-ONLY since #8113
///
/// Production code must never clear a stamp. The descriptor is now the sole
/// record of the live inline-slot bound, so an unstamped receiver reports a
/// bound of ZERO — its payload stops being traced, rewritten, and writable.
/// Every mutation that used to clear-then-re-mint is mint-then-stamp instead
/// (`publish_object_live_slot_count`, `publish_object_shape_from`), which has no
/// window at all. This survives only so tests can MANUFACTURE the unstamped
/// state and assert what the runtime does with it.
#[cfg(test)]
#[inline]
pub(crate) unsafe fn clear_object_shape_stamp(obj: *mut crate::object::ObjectHeader) -> bool {
    if is_shape_id((*obj).parent_class_id) {
        (*obj).parent_class_id = 0;
        true
    } else {
        false
    }
}

/// Build (or extend) the slot map for `keys` covering `key_count` keys.
unsafe fn index_range(shape: &mut ShapeIndex, keys: *const ArrayHeader, key_count: u32) {
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let (slots, slot_len) = super::keys_array_dense_slots(keys);
    for i in shape.indexed_len..key_count.min(slot_len as u32) {
        let v = crate::JSValue::from_bits((*slots.add(i as usize)).to_bits());
        if let Some(b) = crate::string::js_string_key_bytes(v, &mut sso) {
            let h = super::key_bytes_hash(b.as_ptr(), b.len());
            match shape.slots.entry(h) {
                std::collections::hash_map::Entry::Occupied(mut e) => e.get_mut().push(i),
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert(SlotList::One(i));
                }
            }
        }
    }
    shape.indexed_len = key_count;
}

/// Look up `key_bytes` in the shape of `keys`. Returns a slot whose stored
/// key has been re-validated against `key_bytes`; `None` means "not found
/// via the shape" (caller falls back to its linear scan / append path).
///
/// `build` gates first-time index construction (callers keep their
/// historical thresholds: write path ≥ `KEYS_INDEX_THRESHOLD`, read path
/// ≥ `WIDE_KEY_INDEX_MIN_KEYS`) — but an entry that already exists is
/// consulted regardless, so a read may reuse the index a write built.
/// A key-index consultation's answer, distinguishing "this COMPLETE index
/// proves the key absent" from "the index cannot answer".
pub(crate) enum KeysIndexVerdict {
    Found(u32),
    /// The index covers every slot of the array (`indexed_len == key_count`)
    /// and holds no entry for this key: the key is not present, and the
    /// caller may skip its linear backstop scan. Trusting absence is what
    /// makes tombstone-delete churn O(1) — the re-add's find-before-append
    /// otherwise pays a full scan per delete, measured at 60.4% of the
    /// flag-on `bench_populated_delete` profile.
    Absent,
    /// No index, a partial build, or a declined consult — scan.
    Unindexed,
}

pub(crate) unsafe fn shape_slot_lookup(
    keys: *const ArrayHeader,
    key_bytes: &[u8],
    key_hash: u64,
    key_count: u32,
    build: bool,
) -> Option<u32> {
    match shape_slot_lookup_verdict(keys, key_bytes, key_hash, key_count, build) {
        KeysIndexVerdict::Found(slot) => Some(slot),
        _ => None,
    }
}

pub(crate) unsafe fn shape_slot_lookup_verdict(
    keys: *const ArrayHeader,
    key_bytes: &[u8],
    key_hash: u64,
    key_count: u32,
    build: bool,
) -> KeysIndexVerdict {
    let keys_id = keys as usize;
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    let shape = match inner.indices.get_mut(&keys_id) {
        Some(s) => {
            if s.indexed_len > key_count {
                // Shrink (delete/compaction): slots are untrustworthy.
                inner.indices.remove(&keys_id);
                return KeysIndexVerdict::Unindexed;
            }
            s
        }
        None => {
            if !build {
                return KeysIndexVerdict::Unindexed;
            }
            inner.note_young_keys(keys_id as u64);
            inner.indices.entry(keys_id).or_insert(ShapeIndex {
                indexed_len: 0,
                slots: crate::fast_hash::new_ptr_hash_map(),
            })
        }
    };
    if shape.indexed_len < key_count {
        index_range(shape, keys, key_count);
    }
    let complete = shape.indexed_len == key_count;
    let absent = if complete {
        KeysIndexVerdict::Absent
    } else {
        KeysIndexVerdict::Unindexed
    };
    let Some(candidates) = shape.slots.get(&key_hash) else {
        return absent;
    };
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let (slots, slot_len) = super::keys_array_dense_slots(keys);
    for &i in candidates.iter() {
        if (i as usize) >= slot_len || i >= key_count {
            continue;
        }
        let v = crate::JSValue::from_bits((*slots.add(i as usize)).to_bits());
        if let Some(stored) = crate::string::js_string_key_bytes(v, &mut sso) {
            if stored == key_bytes {
                return KeysIndexVerdict::Found(i);
            }
        }
    }
    // Hash-bucket candidates existed but none matched: with a complete index
    // that still proves absence (the bucket held colliding OTHER keys).
    absent
}

/// Record a freshly appended key: `keys` (the POST-append array — a clone
/// or grow-realloc lands under its new identity, or nowhere if no entry
/// exists yet) grew to `new_count` with `key_hash` at `slot`.
pub(crate) fn shape_note_append(
    keys: *const ArrayHeader,
    new_count: u32,
    key_hash: u64,
    slot: u32,
) {
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    if let Some(shape) = inner.indices.get_mut(&(keys as usize)) {
        if shape.indexed_len + 1 == new_count {
            shape.indexed_len = new_count;
            match shape.slots.entry(key_hash) {
                std::collections::hash_map::Entry::Occupied(mut e) => e.get_mut().push(slot),
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert(SlotList::One(slot));
                }
            }
        }
    }
}

/// Back-fill a linear-scan hit (no-op when the shape has no entry — the
/// next lookup_ways builds it wholesale at the caller's threshold).
pub(crate) fn shape_note_hit(keys: *const ArrayHeader, key_hash: u64, slot: u32) {
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    if let Some(shape) = inner.indices.get_mut(&(keys as usize)) {
        match shape.slots.entry(key_hash) {
            std::collections::hash_map::Entry::Occupied(mut e) => e.get_mut().push(slot),
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(SlotList::One(slot));
            }
        }
    }
}

/// An OWNED (non-`GC_FLAG_SHAPE_SHARED`) keys array was reallocated while
/// `js_array_push` appended a key. Migrate only the validated slot-index
/// accelerator so it survives capacity growth. The weak old descriptor is not
/// eagerly deleted: even if a release-only invariant regression left a sibling
/// naming it, that sibling must continue to resolve. Post-trace dead-key
/// pruning retires it once no live owner reaches the old array.
///
/// Callers must pass the OWNED-grow pair only: a shared array's fork is a
/// genuine transition (the clone starts a NEW identity and the old address
/// still describes the siblings' live shape — migrating it would corrupt
/// them). Safety net: a wrong or stale migration cannot produce wrong
/// results — every hit re-validates key bytes against the live array —
/// it only wastes the rebuild this exists to save.
pub(crate) fn shape_keys_grown(old_keys: usize, new_keys: *const ArrayHeader) {
    let new_id = new_keys as usize;
    if old_keys == 0 || new_id == 0 || old_keys == new_id {
        return;
    }
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    if let Some(shape) = inner.indices.remove(&old_keys) {
        inner.note_young_keys(new_id as u64);
        inner.indices.insert(new_id, shape);
    }
}

/// Drop only the validated slot-index accelerator for a keys array that was
/// compacted/retired (delete path). Descriptors are weak and exact-fact gated,
/// but are not eagerly removed: another live sibling may still name one. The
/// post-trace dead-key fan-out retires them when the array is actually dead.
pub(crate) fn shape_drop(keys: *const ArrayHeader) {
    let keys = keys as usize;
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    inner.indices.remove(&keys);
}

/// True when a shape's keys address is currently occupied by another GC type.
/// A tracked non-array allocation proves that the original keys array died and
/// its address was recycled; unreadable/off-arena addresses remain governed by
/// the collector's ordinary dead-owner predicate.
#[inline]
fn shape_keys_address_is_recycled(addr: usize) -> bool {
    #[cfg(test)]
    if RECYCLED_KEYS_CHECK_SUPPRESSED.with(std::cell::Cell::get) {
        return false;
    }

    unsafe {
        crate::value::addr_class::try_read_tracked_gc_header(addr).is_some_and(|header| {
            let obj_type = (*header.as_ptr()).obj_type;
            obj_type != crate::gc::GC_TYPE_ARRAY && obj_type != crate::gc::GC_TYPE_LAZY_ARRAY
        })
    }
}

/// Retire descriptors no live receiver carried during the just-completed
/// synchronous full trace and no runtime metadata owner can reinstall.
///
/// The caller must run this while the full trace's `CARRIED_SEEN` notes are
/// intact and only after cache-carrier bits have been rebuilt from live table
/// occupancy. Minor and budgeted cycles are deliberately ineligible: neither
/// provides an exact, stop-the-world enumeration of every live receiver.
pub(crate) fn prune_uncarried_shape_descriptors_after_full_trace() {
    let table = &crate::state::state().shapes;
    let mut inner = table.inner.borrow_mut();
    let mut stale = Vec::new();
    table.slab().for_each(|id, record| {
        // SAFETY: live slab record, read immediately under agent ownership.
        let record = unsafe { &*record };
        if !record.has(RECORD_FLAG_CARRIED_SEEN) && !record.cache_carrier() {
            stale.push(id);
        }
    });
    for id in stale {
        remove_descriptor_and_reverse_indices(&mut inner, id);
    }
}

/// Post-trace weak-table prune: drop slot indices and by-id descriptors whose
/// keys array is dead. A live object has already traced its authoritative
/// header edge and synchronized the descriptor named by its ShapeId, so a
/// descriptor removed here cannot be named by a live object. Correctness fails
/// closed on a missing lookup_ways, independently of pruning.
pub(crate) fn prune_dead_shape_keys(is_dead_owner: &dyn Fn(usize) -> bool) {
    let table = &crate::state::state().shapes;
    let mut inner = table.inner.borrow_mut();
    // A shape keys entry is keyed by the address of its keys array — a
    // `GC_TYPE_ARRAY` (or `GC_TYPE_LAZY_ARRAY`). When the keys array dies
    // and the arena recycles its address for a different object type
    // (closure, string, …), the `is_dead_owner` predicate sees the NEW
    // object's flags (MARKED/FORWARDED) and reports the address as alive,
    // leaving a stale entry that makes property lookups on objects whose
    // descriptor still points at the old address read the wrong shape.
    // Guard the retain with a type check: if the object at the key address
    // is not an array/lazy-array, the keys array is dead regardless of what
    // `is_dead_owner` says about the recycled tenant.
    if !inner.indices.is_empty() {
        inner.indices.retain(|keys_id, _| {
            !is_dead_owner(*keys_id) && !shape_keys_address_is_recycled(*keys_id)
        });
    }
    let mut stale: Vec<u32> = Vec::new();
    table.slab().for_each(|id, record| {
        // SAFETY: live slab record, read immediately.
        let descriptor = unsafe { *record };
        let keys = descriptor.keys as usize;
        if is_dead_owner(descriptor.keys as usize) || shape_keys_address_is_recycled(keys) {
            stale.push(id);
        }
    });
    for id in stale {
        remove_descriptor_and_reverse_indices(&mut inner, id);
    }
}

/// [`prune_dead_shape_keys`] for a MINOR (#9754): only a young keys array can
/// die, and a young keys address is always in the young log (noted at
/// insert, re-logged by every minor-scoped walk while it stays young), so the
/// log is the complete candidate set.
pub(crate) fn prune_dead_shape_keys_young(is_dead_owner: &dyn Fn(usize) -> bool) {
    let table = &crate::state::state().shapes;
    let mut inner = table.inner.borrow_mut();
    let candidates = inner.young_keys.take_sorted();
    let mut kept = Vec::with_capacity(candidates.len());
    for keys in candidates {
        let addr = keys as usize;
        if !is_dead_owner(addr) && !shape_keys_address_is_recycled(addr) {
            kept.push(keys);
            continue;
        }
        inner.indices.remove(&addr);
        let ids: Vec<u32> = inner
            .families
            .get(&keys)
            .map(|ids| ids.as_slice().to_vec())
            .unwrap_or_default();
        for id in ids {
            remove_descriptor_indexed_under(&mut inner, id, keys);
        }
    }
    inner.young_keys.extend(kept);
}

/// Metadata-only forwarding repair for the weak descriptor table and
/// pointer-keyed slot indices. Mark/copy mode does not root anything; live
/// object scans provide descriptor reachability, and post-copy rewrite follows
/// only forwarding records those live edges already created.
///
/// #9706: the walk is per keys-array FAMILY, not per descriptor. Every
/// descriptor of a family shares one keys address, so one probe answers for
/// all of them — the per-address memo the descriptor walk used to keep
/// (`PROBE_MEMO`, a persistent map sized to every distinct address in the
/// table) is now simply the family index itself. A family is probed with the
/// MARKING visit when any of its descriptors is a carrier, which is exactly
/// the rooting duty the #8112 gate assigns: the keys array must survive while
/// an old receiver or a cache still names one of its shapes.
pub(crate) fn scan_shape_table_rekey_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let table = &crate::state::state().shapes;
    let mut inner = table.inner.borrow_mut();
    let rewrite_phase = visitor.is_metadata_rewrite_phase();
    // #9754: a minor-scoped pass visits only the young-logged keys addresses;
    // the full walk below rebuilds the log from what it finds.
    if visitor.young_scope() {
        scan_shape_table_young(visitor, table, &mut inner, rewrite_phase);
        return;
    }
    let table_len = (inner.families.len() + inner.indices.len()) as u64;
    let mut moved_families: Vec<(u64, u64)> = Vec::new();
    let mut dead_descriptor_ids: Vec<(u32, u64)> = Vec::new();
    // The shared slab view is scoped to the probe loop: retirement below
    // takes the slab mutably, and nothing after the loop may still hold it.
    let slab = table.slab();
    for (&indexed, ids) in inner.families.iter() {
        if indexed == 0 {
            // Keyless shapes hold no edge.
            continue;
        }
        // #8112 ephemeron gate. A shape with an OLD carrier is rooted here:
        // the minor that has to keep its keys array alive never enumerates the
        // object that carries it. A shape with only young carriers is NOT —
        // those receivers are traced, and each one emits the edge itself, so
        // rooting them from the table would make every keys array ever minted
        // immortal and turn `prune_dead_shape_keys`'s "is the keys array
        // dead?" into a question it asks of itself.
        //
        // One descriptor stands for the family: a carrier if the family has
        // one (its duty is the strongest), else any present member.
        let mut descriptor: Option<ShapeDescriptor> = None;
        for &id in ids.as_slice() {
            if let Some(lifted) = slab.lift(id) {
                if lifted.old_carrier || lifted.cache_carrier {
                    descriptor = Some(lifted);
                    break;
                }
                descriptor.get_or_insert(lifted);
            }
        }
        let Some(descriptor) = descriptor else {
            // Every id retired under a stale address; the family is empty.
            moved_families.push((indexed, 0));
            continue;
        };
        let mut addr = indexed as usize;
        // The census gate (`scripts/shape_descriptor_census.py`) pins this
        // exact two-armed expression so that a sabotage which widens the gate
        // or swaps the arms is red, and its own self-test sabotages this very
        // literal.
        let moved = if descriptor.old_carrier || descriptor.cache_carrier {
            visitor.visit_usize_slot(&mut addr)
        } else {
            visitor.visit_metadata_usize_slot(&mut addr)
        };
        // Validate the POST-visit address. A stale shape key can follow the
        // forwarding record of the non-array tenant that recycled its address;
        // checking only an unmoved old address misses that case.
        if rewrite_phase && shape_keys_address_is_recycled(addr) {
            dead_descriptor_ids.extend(ids.as_slice().iter().map(|&id| (id, indexed)));
            continue;
        }
        if moved {
            for &id in ids.as_slice() {
                if let Some(record) = slab.record_ptr(id) {
                    // SAFETY: live slab record, single-threaded agent. A live
                    // receiver's edge may already have written the same
                    // forwarded address here; the store is idempotent.
                    unsafe { (*record).keys = addr as u64 };
                }
            }
        }
        if addr as u64 != indexed {
            moved_families.push((indexed, addr as u64));
        }
    }
    for (id, indexed) in dead_descriptor_ids {
        remove_descriptor_indexed_under(&mut inner, id, indexed);
    }
    for (old, new) in moved_families {
        move_shape_family(table, &mut inner, old, new);
    }

    if rewrite_phase && !inner.indices.is_empty() {
        let moved: Vec<(usize, usize)> = inner
            .indices
            .keys()
            .filter_map(|&keys_id| {
                let mut addr = keys_id;
                visitor.visit_metadata_usize_slot(&mut addr);
                (addr != keys_id).then_some((keys_id, addr))
            })
            .collect();
        for (old, new) in moved {
            if let Some(shape) = inner.indices.remove(&old) {
                inner.indices.insert(new, shape);
            }
        }
        // Drop indices entries whose keys-array address was recycled: the
        // forwarding record at the old address points to a DIFFERENT object
        // (not a keys array), so `visit_metadata_usize_slot` either rekeyed
        // it to the wrong address (caught above by the type mismatch on the
        // new address) or returned false because the forwarding walk could
        // not classify the address. Either way the keys array is dead; remove
        // the stale entry so property lookups don't resolve the wrong shape.
        let recycled: Vec<usize> = inner
            .indices
            .keys()
            .filter(|&&keys_id| shape_keys_address_is_recycled(keys_id))
            .copied()
            .collect();
        for old in recycled {
            inner.indices.remove(&old);
        }
    }

    // A full walk is authoritative: rebuild the young log from the tables.
    let kept = relevant_shape_keys(&inner);
    let kept_len = kept.len() as u64;
    let _ = inner.young_keys.take_sorted();
    inner.young_keys.extend(kept);
    crate::gc::young_log::note_walk(
        SHAPE_YOUNG_LOG_NAME,
        crate::gc::young_log::YoungLogWalk {
            partial: false,
            logged: table_len,
            visited: table_len,
            kept: kept_len,
            table_len,
        },
    );
}

/// Re-index a family that the collector moved from `old` to `new` (`new ==
/// 0`: every member retired, drop it).
fn move_shape_family(table: &ShapeTable, inner: &mut ShapeTableInner, old: u64, new: u64) {
    let Some(ids) = inner.families.remove(&old) else {
        return;
    };
    if new == 0 {
        return;
    }
    for &id in ids.as_slice() {
        let Some(record) = table.slab().get(id) else {
            continue;
        };
        // The accelerator was keyed with the OLD address; the other five
        // facts never change under the collector.
        if record.has(RECORD_FLAG_FACTS_INDEXED) {
            inner.facts_remove(record.facts_key_with_keys(old), id);
            inner.facts_push_back(record.facts_key_with_keys(new), id);
        }
        inner.family_push_back(new, id);
    }
}

/// Every keys address a minor can act on, re-derived from the authoritative
/// tables (families and slot indices whose keys array is not old).
fn relevant_shape_keys(inner: &ShapeTableInner) -> Vec<u64> {
    use crate::gc::young_log::addr_is_minor_relevant;
    let mut relevant: Vec<u64> = inner
        .families
        .keys()
        .copied()
        .filter(|&keys| keys != 0 && addr_is_minor_relevant(keys as usize))
        .collect();
    relevant.extend(
        inner
            .indices
            .keys()
            .copied()
            .filter(|&keys| addr_is_minor_relevant(keys))
            .map(|keys| keys as u64),
    );
    relevant.sort_unstable();
    relevant.dedup();
    relevant
}

/// The minor-scoped walk (#9754): only the young-logged keys addresses, each
/// visited exactly as the full walk visits it — the family's carrier gate,
/// the record rewrite, the recycled-address retirement, the slot-index
/// re-key — and re-logged iff the keys array is still not old afterwards.
fn scan_shape_table_young(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    table: &ShapeTable,
    inner: &mut ShapeTableInner,
    rewrite_phase: bool,
) {
    let table_len = (inner.families.len() + inner.indices.len()) as u64;
    #[cfg(debug_assertions)]
    {
        let relevant = relevant_shape_keys(inner);
        inner
            .young_keys
            .debug_assert_logged(SHAPE_YOUNG_LOG_NAME, &relevant);
    }
    let mut logged = 0u64;
    let mut visited = 0u64;
    let mut kept = Vec::new();
    loop {
        let batch = inner.young_keys.take_sorted();
        if batch.is_empty() {
            break;
        }
        logged += batch.len() as u64;
        for keys in batch {
            if keys == 0 {
                continue;
            }
            visited += 1;
            let (post, relevant) = scan_shape_keys_address(visitor, table, inner, rewrite_phase, keys);
            if relevant {
                kept.push(post);
            }
            // The family moves in the MARK pass (a carrier's `visit_usize_slot`
            // copies the keys array) while the slot index is re-keyed only in
            // the REWRITE pass, so between the two the index still sits under
            // the from-space address: keep that key logged as well.
            if post != keys && inner.indices.contains_key(&(keys as usize)) {
                kept.push(keys);
            }
        }
    }
    let kept_len = kept.len() as u64;
    inner.young_keys.extend(kept);
    crate::gc::young_log::note_walk(
        SHAPE_YOUNG_LOG_NAME,
        crate::gc::young_log::YoungLogWalk {
            partial: true,
            logged,
            visited,
            kept: kept_len,
            table_len,
        },
    );
}

/// Visit one keys address — its family and its slot index — with the same
/// per-entry body as the full walk. Returns the post-visit address and
/// whether the keys array can still matter to a minor.
fn scan_shape_keys_address(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    table: &ShapeTable,
    inner: &mut ShapeTableInner,
    rewrite_phase: bool,
    indexed: u64,
) -> (u64, bool) {
    let mut post = indexed;
    let ids: Vec<u32> = inner
        .families
        .get(&indexed)
        .map(|ids| ids.as_slice().to_vec())
        .unwrap_or_default();
    if !ids.is_empty() {
        let slab = table.slab();
        let mut descriptor: Option<ShapeDescriptor> = None;
        for &id in &ids {
            if let Some(lifted) = slab.lift(id) {
                if lifted.old_carrier || lifted.cache_carrier {
                    descriptor = Some(lifted);
                    break;
                }
                descriptor.get_or_insert(lifted);
            }
        }
        match descriptor {
            None => {
                // Every id retired under a stale address; the family is empty.
                move_shape_family(table, inner, indexed, 0);
            }
            Some(descriptor) => {
                let mut addr = indexed as usize;
                let moved = if descriptor.old_carrier || descriptor.cache_carrier {
                    visitor.visit_usize_slot(&mut addr)
                } else {
                    visitor.visit_metadata_usize_slot(&mut addr)
                };
                if rewrite_phase && shape_keys_address_is_recycled(addr) {
                    for id in ids {
                        remove_descriptor_indexed_under(inner, id, indexed);
                    }
                } else {
                    if moved {
                        for &id in &ids {
                            if let Some(record) = slab.record_ptr(id) {
                                // SAFETY: live slab record, single-threaded agent;
                                // the store is idempotent (see the full walk).
                                unsafe { (*record).keys = addr as u64 };
                            }
                        }
                    }
                    if addr as u64 != indexed {
                        move_shape_family(table, inner, indexed, addr as u64);
                        post = addr as u64;
                    }
                }
            }
        }
    }
    if rewrite_phase && inner.indices.contains_key(&(indexed as usize)) {
        let mut addr = indexed as usize;
        visitor.visit_metadata_usize_slot(&mut addr);
        if addr != indexed as usize {
            if let Some(shape) = inner.indices.remove(&(indexed as usize)) {
                inner.indices.insert(addr, shape);
            }
            post = addr as u64;
        }
        if shape_keys_address_is_recycled(addr) {
            inner.indices.remove(&addr);
        }
    }
    (post, crate::gc::young_log::addr_is_minor_relevant(post as usize))
}

// #8112 sabotage switch. Suppressing the descriptor edge proves the fixture's
// detector distinguishes a rewritten record from a stale one.
//
// Deliberately `#[cfg(test)]` thread-locals and not env knobs: the GC-knob
// kill policy requires every shipped knob's off-state to be exercised by a
// required CI arm, and neither state may be reachable in a shipped binary —
// Only collector-level fixtures may turn it on.
#[cfg(test)]
thread_local! {
    static KEYS_EDGE_SUPPRESSED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static RECYCLED_KEYS_CHECK_SUPPRESSED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Test-only helpers for the shape table, in a sibling file (see the cap note there).
#[cfg(test)]
#[path = "shapes_test_support.rs"]
mod shapes_test_support;

#[cfg(test)]
pub(crate) use shapes_test_support::*;

/// The shape-table unit suites, in a sibling file: `shapes.rs` sits close to
/// the repo's 2000-line-per-file cap and #8112 added the descriptor record's
/// keys slot and old-carrier gate to it. Moved verbatim.
#[cfg(test)]
#[path = "shapes_tests.rs"]
mod shapes_tests;

/// #9612: release the capacity that pruning left behind.
///
/// hashbrown never shrinks on `remove`/`retain`, so the shape tables keep the
/// allocation of their startup PEAK for the life of the process. Measured on
/// the compiled claude-code TUI at idle: `ids_by_facts` held 30.8 MB at 12.3%
/// fill and `descriptors` 13.8 MB at 10.5% fill, i.e. sized for a peak that
/// `prune_dead_shape_keys` had already discarded.
///
/// Called once per MAJOR collection, right after the prune, where a rehash is
/// already amortized against a full heap walk. #9706: the by-id store is a
/// slab now, so this also releases its all-dead chunks; the family and slot
/// index maps are shrunk to `len + len / 4`, one growth step of headroom.
pub(crate) fn shrink_shape_tables() {
    fn worth_shrinking(len: usize, capacity: usize) -> bool {
        // Only when the table is holding real slack and is less than half
        // used; a small or well-packed table is left alone.
        capacity > 4096 && capacity > len.saturating_mul(2)
    }
    let table = &crate::state::state().shapes;
    let mut inner = table.inner.borrow_mut();
    if worth_shrinking(inner.indices.len(), inner.indices.capacity()) {
        let target = inner.indices.len() + inner.indices.len() / 4;
        inner.indices.shrink_to(target);
    }
    if worth_shrinking(inner.by_facts.len(), inner.by_facts.capacity()) {
        let target = inner.by_facts.len() + inner.by_facts.len() / 4;
        inner.by_facts.shrink_to(target);
    }
    if worth_shrinking(inner.families.len(), inner.families.capacity()) {
        let target = inner.families.len() + inner.families.len() / 4;
        inner.families.shrink_to(target);
    }
    // SAFETY: the prune that precedes this call holds no slab reference, and
    // neither does anything else while the major collection owns the agent.
    unsafe { table.slab_mut().release_empty_chunks() };
}

/// `PERRY_GC_CENSUS`: the by-id slab, the per-shape key indices, the
/// exact-facts accelerator and the keys-address family index.
pub(crate) fn shape_table_census() -> Vec<crate::gc::census::SideTableRow> {
    use crate::gc::census::{hash_table_bytes, map_bytes};
    let table = &crate::state::state().shapes;
    let inner = table.inner.borrow();
    let slab = table.slab();
    let mut rows = Vec::new();
    rows.push(("shapes.descriptors", slab.len(), slab.estimated_bytes()));
    let index_inner: usize = inner
        .indices
        .values()
        .map(|ix| hash_table_bytes(ix.slots.capacity(), std::mem::size_of::<(u64, SlotList)>()))
        .sum();
    rows.push((
        "shapes.indices",
        inner.indices.len(),
        map_bytes(&inner.indices) + index_inner,
    ));
    let facts_inner: usize = inner.by_facts.values().map(IdList::heap_bytes).sum();
    rows.push((
        "shapes.by_facts",
        inner.by_facts.len(),
        map_bytes(&inner.by_facts) + facts_inner,
    ));
    let families_inner: usize = inner.families.values().map(IdList::heap_bytes).sum();
    rows.push((
        "shapes.families",
        inner.families.len(),
        map_bytes(&inner.families) + families_inner,
    ));
    // Ids ever minted by this process: the slab is indexed by id, so the gap
    // between this and `shapes.descriptors` is what chunk release reclaims.
    let minted = SHAPE_ID_NEXT.load(std::sync::atomic::Ordering::Relaxed) - SHAPE_ID_BASE;
    rows.push(("shapes.ids_minted(process)", minted as usize, 0));
    rows
}

/// `PERRY_GC_CENSUS`: how the descriptor population relates to the live heap
/// (#9706). `live_ids` is the sorted, deduplicated set of ShapeIds stamped on
/// live shaped objects, collected by the census walk.
///
/// * `shapes.descriptors.carried` — descriptors some live object is stamped
///   with: the population V8's "object shape" bucket corresponds to.
/// * `shapes.descriptors.uncarried` — descriptors no live object carries:
///   transition history a cache may reinstall (`cache_carrier`), versions
///   kept for an old receiver since the last full trace, and shapes whose
///   keys array is still alive on some other descriptor.
/// * `shapes.families.multi` — keys arrays with more than one descriptor,
///   and the descriptors they hold beyond the first: the duplication the
///   family walk pays for.
pub(crate) fn shape_table_liveness_census(
    live_ids: &[u32],
) -> Vec<crate::gc::census::SideTableRow> {
    let table = &crate::state::state().shapes;
    let inner = table.inner.borrow();
    let slab = table.slab();
    let mut carried = 0usize;
    let mut uncarried = 0usize;
    let mut uncarried_cache = 0usize;
    let mut uncarried_old = 0usize;
    slab.for_each(|id, record| {
        if live_ids.binary_search(&id).is_ok() {
            carried += 1;
            return;
        }
        uncarried += 1;
        // SAFETY: live slab record, read immediately.
        let record = unsafe { *record };
        if record.cache_carrier() {
            uncarried_cache += 1;
        } else if record.has(RECORD_FLAG_OLD_CARRIER) {
            uncarried_old += 1;
        }
    });
    let mut multi_families = 0usize;
    let mut multi_extra = 0usize;
    let mut largest = 0usize;
    for ids in inner.families.values() {
        let n = ids.len();
        largest = largest.max(n);
        if n > 1 {
            multi_families += 1;
            multi_extra += n - 1;
        }
    }
    vec![
        ("shapes.descriptors.carried(live objects)", carried, 0),
        ("shapes.descriptors.uncarried", uncarried, 0),
        (
            "shapes.descriptors.uncarried.cache_carrier",
            uncarried_cache,
            0,
        ),
        ("shapes.descriptors.uncarried.old_carrier", uncarried_old, 0),
        (
            "shapes.families.multi(families,extra descriptors)",
            multi_families,
            multi_extra,
        ),
        ("shapes.families.largest", largest, 0),
    ]
}
