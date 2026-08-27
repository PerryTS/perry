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
//! in this agent's `RuntimeState` to an immutable descriptor containing the
//! ordered-keys edge plus the exact logical-key and live-inline-slot bounds.
//! The descriptor table is agent-local while ids are process-global. A live
//! object's ShapeId is authoritative for its ordered keys, logical-key count,
//! live inline-slot bound, and semantic generation.
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
use std::collections::HashMap;

pub(crate) struct ShapeIndex {
    /// Key count covered by `slots`. Longer live array ⟹ catch up
    /// incrementally (append-only while shared); shorter ⟹ a delete
    /// compacted it — drop and rebuild on next lookup.
    indexed_len: u32,
    /// FNV-1a content hash of key bytes → candidate slots (collisions
    /// resolved by the per-hit content validation).
    slots: HashMap<u64, Vec<u32>>,
}

/// Immutable facts named by one ShapeId.
///
/// #8112: `keys` is the AUTHORITATIVE ordered-keys edge — the collector marks
/// it and rewrites it in place. Before #8112 the header word was the sole
/// strong edge and this field a weak copy that a post-visit callback repaired.
/// The inversion is what #8047 needs, because deleting the header word must
/// not unroot anything.
///
/// The table is a rehashing `PtrHashMap`, so the bucket address is NOT stable
/// across descriptor insertion — and the incremental collector retains
/// enumerated slot addresses across budgeted resumptions. Descriptors are
/// therefore BOXED (`ShapeTableInner::descriptors`), which makes each record's
/// address fixed for its lifetime, and `record` carries the address of THIS
/// boxed descriptor so a traced receiver can hand the collector a rewritable
/// `keys` location without a second table probe (#8122's one-probe rule).
#[derive(Clone, Copy, Debug)]
pub(crate) struct ShapeDescriptor {
    /// Raw ArrayHeader address in Perry's fixed-width heap-word ABI. Keeping
    /// this u64 preserves identical representation on ILP32/LP64.
    pub(crate) keys: u64,
    /// The keys address currently represented in `ids_by_facts` and
    /// `ids_by_keys`. The collector may rewrite `keys` directly through a raw
    /// slot before the metadata scanner runs; retaining the indexed address
    /// lets that scanner repair exactly this descriptor instead of rebuilding
    /// and sorting both reverse maps for every shape in the agent.
    /// Never part of shape identity.
    indexed_keys: u64,
    /// Address of the BOXED record this value was lifted from, or 0 for a
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
    pub(crate) old_carrier: bool,
    /// Notes accumulated since the last full trace; adopted into `old_carrier`
    /// by [`rotate_old_carrier_epoch_after_full_trace`].
    pub(crate) old_carrier_seen: bool,
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
}

/// Shape identity is the FACTS, never the storage address. A descriptor value
/// lifted out of the table compares equal to the boxed record it came from.
impl ShapeDescriptor {
    /// The one `keys` word the collector rewrites for this shape, or `None`
    /// for a descriptor value that was never lifted out of the table.
    #[inline]
    pub(crate) fn keys_slot(&self) -> Option<*mut u64> {
        if self.record == 0 {
            return None;
        }
        Some(unsafe { std::ptr::addr_of_mut!((*(self.record as *mut ShapeDescriptor)).keys) })
    }
}

impl PartialEq for ShapeDescriptor {
    fn eq(&self, other: &Self) -> bool {
        descriptor_facts(*self) == descriptor_facts(*other)
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

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct ShapeFacts {
    keys: u64,
    logical_key_count: u32,
    live_inline_slot_count: u32,
    semantic_generation: u64,
    object_kind: ShapeObjectKind,
}

struct ShapeTableInner {
    indices: crate::fast_hash::PtrHashMap<usize, ShapeIndex>,
    /// #8125: `PtrHashMap`, not the SipHash default.
    ///
    /// This is the map `shape_descriptor_by_id` probes, and that probe is the
    /// single hottest runtime lookup in the object model: `object_is_regular`
    /// runs it once per array element-shape test (3 M times on the `retain`
    /// bench, 20 M on `churn`) and, since #8113 deleted
    /// `ObjectHeader::field_count`, `object_live_slot_count` runs it on every
    /// indexed field get/set. A symbol profile of the `shapes` bench
    /// (`PERRY_DEBUG_SYMBOLS=1` + `sample`) put `RandomState::hash_one` at the
    /// TOP of self time with `shape_descriptor_by_id` fourth — together ~22% of
    /// the program, nearly all of it SipHash on a bare `u32`.
    ///
    /// The key is a ShapeId minted by this process from a monotonic counter.
    /// No external input reaches it, so hash-flooding resistance buys nothing
    /// here for the same reason it buys nothing on the pointer-keyed
    /// registries `fast_hash` already serves.
    /// BOXED (#8112): the collector enumerates `&mut record.keys` as an
    /// ordinary GC slot, so the record's address must survive every descriptor
    /// insertion that can happen while a budgeted scan holds it. A `Box` keeps
    /// the payload put when the map rehashes; the map only ever moves the
    /// eight-byte owning pointer.
    descriptors: crate::fast_hash::PtrHashMap<u32, Box<ShapeDescriptor>>,
    /// Exact-facts reverse index. More than one id is legal when a worker
    /// minted a local descriptor before a process-global module id arrived.
    ///
    /// Deliberately NOT a `PtrHashMap`: `PtrHasher`'s `write_*` methods
    /// OVERWRITE the accumulator instead of folding it, which is exactly right
    /// for a single-word key and wrong for this five-field one — every
    /// `ShapeFacts` would hash to its last field alone.
    ids_by_facts: HashMap<ShapeFacts, Vec<u32>>,
    /// Keys-array address -> every descriptor id that currently names it.
    /// Same-address key-count retirement uses this index instead of scanning
    /// every shape ever observed by the agent. Single-word key, so `PtrHasher`
    /// (#8125).
    ids_by_keys: crate::fast_hash::PtrHashMap<u64, Vec<u32>>,
}

pub(crate) struct ShapeTable {
    inner: RefCell<ShapeTableInner>,
}

impl ShapeTable {
    pub(crate) fn new() -> Self {
        ShapeTable {
            inner: RefCell::new(ShapeTableInner {
                indices: crate::fast_hash::new_ptr_hash_map(),
                descriptors: crate::fast_hash::new_ptr_hash_map(),
                ids_by_facts: HashMap::new(),
                ids_by_keys: crate::fast_hash::new_ptr_hash_map(),
            }),
        }
    }
}

#[inline]
fn descriptor_facts(descriptor: ShapeDescriptor) -> ShapeFacts {
    ShapeFacts {
        keys: descriptor.keys,
        logical_key_count: descriptor.logical_key_count,
        live_inline_slot_count: descriptor.live_inline_slot_count,
        semantic_generation: descriptor.semantic_generation,
        object_kind: descriptor.object_kind,
    }
}

fn descriptor_facts_with_keys(descriptor: ShapeDescriptor, keys: u64) -> ShapeFacts {
    ShapeFacts {
        keys,
        logical_key_count: descriptor.logical_key_count,
        live_inline_slot_count: descriptor.live_inline_slot_count,
        semantic_generation: descriptor.semantic_generation,
        object_kind: descriptor.object_kind,
    }
}

fn remove_descriptor_id_from_facts_index(inner: &mut ShapeTableInner, facts: ShapeFacts, id: u32) {
    let remove_entry = if let Some(ids) = inner.ids_by_facts.get_mut(&facts) {
        if let Ok(index) = ids.binary_search(&id) {
            ids.remove(index);
        } else {
            ids.retain(|&candidate| candidate != id);
        }
        ids.is_empty()
    } else {
        false
    };
    if remove_entry {
        inner.ids_by_facts.remove(&facts);
    }
}

fn remove_descriptor_id_from_keys_index(inner: &mut ShapeTableInner, keys: u64, id: u32) {
    let remove_entry = if let Some(ids) = inner.ids_by_keys.get_mut(&keys) {
        if let Ok(index) = ids.binary_search(&id) {
            ids.remove(index);
        } else {
            ids.retain(|&candidate| candidate != id);
        }
        ids.is_empty()
    } else {
        false
    };
    if remove_entry {
        inner.ids_by_keys.remove(&keys);
    }
}

#[inline]
fn insert_descriptor_id_sorted(ids: &mut Vec<u32>, id: u32) {
    if let Err(index) = ids.binary_search(&id) {
        ids.insert(index, id);
    }
}

/// Repair one descriptor after its collector-owned `keys` slot moved.
///
/// `indexed_keys` records the address under which the id is still indexed, so
/// this is O(population sharing the old/new facts) rather than O(all shapes).
fn sync_descriptor_reverse_indices(inner: &mut ShapeTableInner, id: u32) {
    let Some(descriptor) = inner.descriptors.get(&id).map(|record| **record) else {
        return;
    };
    if descriptor.indexed_keys == descriptor.keys {
        return;
    }

    let old_facts = descriptor_facts_with_keys(descriptor, descriptor.indexed_keys);
    let new_facts = descriptor_facts(descriptor);
    remove_descriptor_id_from_facts_index(inner, old_facts, id);
    remove_descriptor_id_from_keys_index(inner, descriptor.indexed_keys, id);
    insert_descriptor_id_sorted(inner.ids_by_facts.entry(new_facts).or_default(), id);
    insert_descriptor_id_sorted(inner.ids_by_keys.entry(descriptor.keys).or_default(), id);
    if let Some(record) = inner.descriptors.get_mut(&id) {
        record.indexed_keys = descriptor.keys;
    }
}

fn remove_descriptor_and_reverse_indices(inner: &mut ShapeTableInner, id: u32) {
    let Some(descriptor) = inner.descriptors.remove(&id) else {
        return;
    };
    retire_cached_shape_object_kind(id);
    let facts = descriptor_facts_with_keys(*descriptor, descriptor.indexed_keys);
    remove_descriptor_id_from_facts_index(inner, facts, id);
    remove_descriptor_id_from_keys_index(inner, descriptor.indexed_keys, id);
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
    let keys_id = keys as usize;
    if keys_id == 0 && logical_key_count != 0 {
        return Err(ShapeDescriptorError::InvalidFacts);
    }
    let facts = ShapeFacts {
        keys: keys_id as u64,
        logical_key_count,
        live_inline_slot_count,
        semantic_generation,
        object_kind,
    };
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    if let Some(id) = inner
        .ids_by_facts
        .get(&facts)
        .and_then(|ids| ids.first().copied())
    {
        return Ok(id);
    }
    let id = alloc_shape_id().map_err(|_| ShapeDescriptorError::IdExhausted)?;
    let descriptor = ShapeDescriptor {
        keys: keys_id as u64,
        indexed_keys: keys_id as u64,
        record: 0,
        old_carrier: false,
        old_carrier_seen: false,
        cache_carrier: false,
        logical_key_count,
        live_inline_slot_count,
        semantic_generation,
        object_kind,
    };
    // Publish by-id first, then the reverse accelerator. An ObjectHeader is
    // stamped only after this function returns, so a visible id always has a
    // complete descriptor.
    inner.descriptors.insert(id, box_descriptor(descriptor));
    inner.ids_by_facts.entry(facts).or_default().push(id);
    inner.ids_by_keys.entry(facts.keys).or_default().push(id);
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
fn publish_shape_result(result: Result<u32, ShapeDescriptorError>) -> u32 {
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

pub(crate) fn shape_descriptor_by_id(shape_id: u32) -> Option<ShapeDescriptor> {
    if !is_shape_id(shape_id) {
        return None;
    }
    crate::state::state()
        .shapes
        .inner
        .borrow()
        .descriptors
        .get(&shape_id)
        .map(|record| lift_descriptor(record))
}

/// Immutable ordinary-vs-class fact with a pointer-free, per-agent direct
/// cache. The first observation remains the authoritative descriptor lookup;
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

/// Box a descriptor and stamp the record with its OWN address (#8112).
///
/// Self-referential on purpose. The alternative — deriving the address in
/// `lift_descriptor` from the `&ShapeDescriptor` a shared table borrow yields —
/// would hand the collector a pointer with SHARED provenance and then write
/// through it. Taking it from the box while it is still uniquely owned keeps
/// the write well-formed.
fn box_descriptor(descriptor: ShapeDescriptor) -> Box<ShapeDescriptor> {
    let mut boxed = Box::new(descriptor);
    boxed.record = std::ptr::addr_of_mut!(*boxed) as usize;
    boxed
}

/// Copy a boxed record out of the table (#8112).
///
/// The copy's `keys` is a snapshot; `record` — stamped by [`box_descriptor`] —
/// names the one storage the collector rewrites. A caller that only reads facts
/// uses the snapshot; the GC hands `keys_slot()` to the slot visitor, so a
/// moved keys array lands back in the table with no second probe and no
/// write-back callback.
#[inline]
fn lift_descriptor(record: &ShapeDescriptor) -> ShapeDescriptor {
    *record
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
/// `descriptor.record`, when non-zero, is the address of a live boxed record
/// owned by this agent's shape table. Records are freed only by
/// `prune_dead_shape_keys`, which runs at sweep — after every enumeration of
/// the cycle that produced this descriptor.
#[inline]
pub(crate) unsafe fn note_old_generation_carrier(descriptor: Option<ShapeDescriptor>) {
    let Some(descriptor) = descriptor else {
        return;
    };
    if descriptor.record == 0 {
        return;
    }
    let record = descriptor.record as *mut ShapeDescriptor;
    // GC_STORE_AUDIT(POINTER_FREE): liveness bookkeeping byte, never a heap reference.
    (*record).old_carrier = true;
    (*record).old_carrier_seen = true;
}

/// Permanently retain a descriptor while an agent-local optimization cache can
/// reinstall its ShapeId. Cache tables live with `RuntimeState`, so this bit
/// has the same lifetime and is reclaimed with the whole agent.
#[inline]
pub(crate) unsafe fn note_cache_carrier(descriptor: Option<ShapeDescriptor>) {
    let Some(descriptor) = descriptor else {
        return;
    };
    if descriptor.record == 0 {
        return;
    }
    let record = descriptor.record as *mut ShapeDescriptor;
    // GC_STORE_AUDIT(POINTER_FREE): liveness bookkeeping byte, never a heap reference.
    (*record).cache_carrier = true;
}

/// Recompute the old-carrier gate from the trace that just finished.
///
/// A FULL trace enumerates every live object, so the notes it accumulated are
/// exactly the shapes old objects still carry; adopt them and clear the
/// accumulator. Minors only ever ADD notes, which is why the gate needs a full
/// trace to shed a shape whose last old carrier died — the same rule that
/// governs every other old-generation reclamation.
pub(crate) fn rotate_old_carrier_epoch_after_full_trace() {
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    for record in inner.descriptors.values_mut() {
        record.old_carrier = record.old_carrier_seen;
        record.old_carrier_seen = false;
    }
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
    shape_id_for_keys_ensure(keys as usize as *const ArrayHeader, key_count)
}

/// Mint a process-global ShapeId for a codegen-registered typed layout and
/// install its structural descriptor in the current agent. Unlike
/// [`shape_id_for_keys_ensure`], this deliberately does not canonicalise by
/// keys alone: two objects with identical property names but different raw
/// slot representations must never share a pre-baked GC descriptor.
pub(crate) fn mint_registered_typed_shape_id(keys: *const ArrayHeader, key_count: u32) -> u32 {
    let id = alloc_shape_id().unwrap_or_else(|_| shape_id_exhausted_abort());
    if !install_external_shape_id(id, keys, key_count, key_count) {
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
    install_external_shape_id(id, keys, key_count, key_count)
}

/// Install a process-global id into this agent's local descriptor table.
/// Module globals are initialized once per process, while workers own distinct
/// runtime state and moving keys pointers. Global id uniqueness makes a local
/// first installation unambiguous; an existing different descriptor fails
/// closed and the caller mints a fresh local id instead.
fn install_external_shape_id(
    id: u32,
    keys: *const ArrayHeader,
    logical_key_count: u32,
    live_inline_slot_count: u32,
) -> bool {
    if !is_shape_id(id) || (keys.is_null() && logical_key_count != 0) {
        return false;
    }
    let descriptor = ShapeDescriptor {
        keys: keys as usize as u64,
        indexed_keys: keys as usize as u64,
        record: 0,
        old_carrier: false,
        old_carrier_seen: false,
        cache_carrier: false,
        logical_key_count,
        live_inline_slot_count,
        semantic_generation: 0,
        object_kind: ShapeObjectKind::Ordinary,
    };
    let facts = descriptor_facts(descriptor);
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    if let Some(existing) = inner.descriptors.get(&id) {
        return **existing == descriptor;
    }
    // A worker can have minted an equivalent local descriptor before module
    // initialization installs the process-global codegen id. Keep both id
    // descriptors valid for already-published objects and make the external
    // id canonical for subsequent births in this agent.
    inner.descriptors.insert(id, box_descriptor(descriptor));
    // An equivalent local descriptor can predate module initialization. Keep
    // both reverse-index entries and prefer the external id for subsequent
    // births in this agent; already-published local ids remain resolvable.
    inner.ids_by_facts.entry(facts).or_default().insert(0, id);
    insert_descriptor_id_sorted(inner.ids_by_keys.entry(descriptor.keys).or_default(), id);
    true
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
    // the constant-time release publication. This lookup is compiled out of
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
    (*obj).parent_class_id = target_shape_id;
    if !target_is_cache_carried && !crate::arena::pointer_in_nursery(obj as usize) {
        note_old_generation_carrier(shape_descriptor_by_id(target_shape_id));
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
        (*obj).parent_class_id = id;
        debug_assert_object_shape_parity(obj);
        return id;
    };
    let id = publish_shape_result(shape_descriptor_ensure_with_generation(
        keys,
        key_count,
        lineage.live_inline_slot_count,
        lineage.semantic_generation,
        lineage.object_kind,
    ));
    if id != (*obj).parent_class_id {
        // Read-side lookup also calls `stamp_object_shape` to populate its
        // field cache. Preserve a proved Array-subclass prefix when that call
        // merely republishes the exact current descriptor; retire it only for
        // an actual structural identity change.
        crate::array::clear_array_subclass_named_prefix_token(obj);
    }
    (*obj).parent_class_id = id;
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
            || install_external_shape_id(runtime_shape_id, keys, key_count, live_inline_slot_count);
    if supplied_id_is_local {
        (*obj).parent_class_id = runtime_shape_id;
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
    if let Some(old) = shape_descriptor_by_id(old_id) {
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
            retain_key_count_versions(keys as u64);
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
    let id = publish_shape_result(shape_descriptor_ensure_with_generation(
        keys,
        key_count,
        live_inline_slot_count,
        semantic_generation,
        object_kind,
    ));
    (*obj).parent_class_id = id;
    debug_assert_object_shape_parity_for_keys(obj, keys);
    id
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
    (*obj).parent_class_id = id;
    debug_assert_object_shape_parity(obj);
    id
}

/// Turn a class-expression object into a class receiver. The kind is part of
/// the exact immutable descriptor, so it cannot alias GC layout bits and every
/// pre-mark ShapeId guard permanently misses afterward.
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
    let generation = SHAPE_SEMANTIC_NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if generation == 0 {
        shape_id_exhausted_abort();
    }
    let id = publish_shape_result(shape_descriptor_ensure_with_generation(
        current.keys as usize as *const ArrayHeader,
        current.logical_key_count,
        current.live_inline_slot_count,
        generation,
        ShapeObjectKind::Class,
    ));
    (*obj).parent_class_id = id;
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

fn retain_key_count_versions(keys: u64) {
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    let Some(ids) = inner.ids_by_keys.remove(&keys) else {
        return;
    };
    let mut current_ids = Vec::with_capacity(ids.len());
    for id in ids {
        let Some(descriptor) = inner.descriptors.get(&id).map(|record| **record) else {
            continue;
        };
        debug_assert_eq!(
            descriptor.keys, keys,
            "keys index contains a foreign descriptor"
        );
        if descriptor.keys != keys {
            let correct_ids = inner.ids_by_keys.entry(descriptor.keys).or_default();
            if !correct_ids.contains(&id) {
                correct_ids.push(id);
            }
        } else {
            // Keep immutable historical descriptors addressable by id. An
            // append under an owned keys allocation preserves the old prefix,
            // and a stale cache/object may still carry either a local or an
            // equivalent external id. Dead-key pruning reclaims the whole
            // lineage once no live owner reaches the keys allocation.
            current_ids.push(id);
        }
    }
    if !current_ids.is_empty() {
        inner.ids_by_keys.insert(keys, current_ids);
    }
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

/// The address of the ONE `keys` word the collector rewrites for `shape_id`,
/// or `None` when the id names no descriptor in this agent (#8112).
///
/// This is the seam that replaced the post-visit write-back callback. The
/// callback existed because the header word was the strong edge and the
/// descriptor a weak copy that had to be repaired from it, under exact-facts
/// validation, once per traced receiver whose keys array had moved. With the
/// descriptor holding the edge, the slot visitor writes the record directly
/// and there is nothing left to reconcile.
///
/// The returned address belongs to a BOXED record, so it is stable across
/// descriptor insertion; only `prune_dead_shape_keys` frees one, and that runs
/// at sweep, after every enumeration of the cycle that produced it.
#[cfg(test)]
#[inline]
pub(crate) fn shape_descriptor_keys_slot(shape_id: u32) -> Option<*mut u64> {
    if !is_shape_id(shape_id) {
        return None;
    }
    crate::state::state()
        .shapes
        .inner
        .borrow_mut()
        .descriptors
        .get_mut(&shape_id)
        .map(|record| std::ptr::addr_of_mut!(record.keys))
}

/// Is `slot` the shared `keys` word of `shape_id`'s descriptor record?
///
/// #8112: that word is a TABLE root, not a slot any receiver owns. Every
/// sibling of the shape enumerates it, so a rewrite performed while tracing
/// one receiver silently changes the edge of every other — including old
/// receivers a minor never visits, for which no per-parent remembered-set page
/// could ever be armed. The remembered-set and old→young verification paths
/// therefore skip it and let the shape table's own root scanner cover it.
#[inline]
pub(crate) fn shape_id_owns_keys_slot(shape_id: u32, slot: *mut u64) -> bool {
    if !is_shape_id(shape_id) {
        return false;
    }
    // Immutable borrow on purpose: this runs inside collector walks, and a
    // `borrow_mut` here would make the predicate itself a re-entrancy hazard.
    crate::state::state()
        .shapes
        .inner
        .borrow()
        .descriptors
        .get(&shape_id)
        .is_some_and(|record| std::ptr::addr_of!(record.keys) as *mut u64 == slot)
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
            shape.slots.entry(h).or_default().push(i);
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
pub(crate) unsafe fn shape_slot_lookup(
    keys: *const ArrayHeader,
    key_bytes: &[u8],
    key_hash: u64,
    key_count: u32,
    build: bool,
) -> Option<u32> {
    let keys_id = keys as usize;
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    let shape = match inner.indices.get_mut(&keys_id) {
        Some(s) => {
            if s.indexed_len > key_count {
                // Shrink (delete/compaction): slots are untrustworthy.
                inner.indices.remove(&keys_id);
                return None;
            }
            s
        }
        None => {
            if !build {
                return None;
            }
            inner.indices.entry(keys_id).or_insert(ShapeIndex {
                indexed_len: 0,
                slots: HashMap::with_capacity(key_count as usize),
            })
        }
    };
    if shape.indexed_len < key_count {
        index_range(shape, keys, key_count);
    }
    let candidates = shape.slots.get(&key_hash)?;
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let (slots, slot_len) = super::keys_array_dense_slots(keys);
    for &i in candidates {
        if (i as usize) >= slot_len || i >= key_count {
            continue;
        }
        let v = crate::JSValue::from_bits((*slots.add(i as usize)).to_bits());
        if let Some(stored) = crate::string::js_string_key_bytes(v, &mut sso) {
            if stored == key_bytes {
                return Some(i);
            }
        }
    }
    None
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
            shape.slots.entry(key_hash).or_default().push(slot);
        }
    }
}

/// Back-fill a linear-scan hit (no-op when the shape has no entry — the
/// next lookup builds it wholesale at the caller's threshold).
pub(crate) fn shape_note_hit(keys: *const ArrayHeader, key_hash: u64, slot: u32) {
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    if let Some(shape) = inner.indices.get_mut(&(keys as usize)) {
        shape.slots.entry(key_hash).or_default().push(slot);
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

/// Post-trace weak-table prune: drop slot indices and by-id descriptors whose
/// keys array is dead. A live object has already traced its authoritative
/// header edge and synchronized the descriptor named by its ShapeId, so a
/// descriptor removed here cannot be named by a live object. Correctness fails
/// closed on a missing lookup, independently of pruning.
pub(crate) fn prune_dead_shape_keys(is_dead_owner: &dyn Fn(usize) -> bool) {
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
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
    let stale: Vec<u32> = inner
        .descriptors
        .iter()
        .filter_map(|(&id, descriptor)| {
            let keys = descriptor.keys as usize;
            (is_dead_owner(descriptor.keys as usize) || shape_keys_address_is_recycled(keys))
                .then_some(id)
        })
        .collect();
    if !stale.is_empty() {
        for id in stale {
            remove_descriptor_and_reverse_indices(&mut inner, id);
        }
    }
}

/// Metadata-only forwarding repair for the weak descriptor table and
/// pointer-keyed slot indices. Mark/copy mode does not root anything; live
/// object scans provide descriptor reachability, and post-copy rewrite follows
/// only forwarding records those live edges already created.
pub(crate) fn scan_shape_table_rekey_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    let mut descriptor_rekeys: Vec<u32> = Vec::new();
    let mut dead_descriptor_ids: Vec<u32> = Vec::new();
    for (id, descriptor) in inner.descriptors.iter_mut() {
        let mut addr = descriptor.keys as usize;
        // #8112 ephemeron gate. A shape with an OLD carrier is rooted here:
        // the minor that has to keep its keys array alive never enumerates the
        // object that carries it. A shape with only young carriers is NOT —
        // those receivers are traced, and each one emits the edge itself, so
        // rooting them from the table would make every keys array ever minted
        // immortal and turn `prune_dead_shape_keys`'s "is the keys array
        // dead?" into a question it asks of itself.
        let moved = if descriptor.old_carrier || descriptor.cache_carrier {
            visitor.visit_usize_slot(&mut addr)
        } else {
            visitor.visit_metadata_usize_slot(&mut addr)
        };
        // Validate the POST-visit address. A stale shape key can follow the
        // forwarding record of the non-array tenant that recycled its address;
        // checking only an unmoved old address misses that case.
        if visitor.is_metadata_rewrite_phase() && shape_keys_address_is_recycled(addr) {
            dead_descriptor_ids.push(*id);
        } else if moved {
            descriptor.keys = addr as u64;
        }
        // A live-object edge can rewrite the boxed `keys` slot before this
        // metadata pass. Comparing against the address represented in the
        // reverse maps catches both that ordering and a move observed here.
        if descriptor.keys != descriptor.indexed_keys {
            descriptor_rekeys.push(*id);
        }
    }
    // Remove descriptors whose keys array was recycled.
    if !dead_descriptor_ids.is_empty() {
        for id in &dead_descriptor_ids {
            remove_descriptor_and_reverse_indices(&mut inner, *id);
        }
    }
    for id in descriptor_rekeys {
        sync_descriptor_reverse_indices(&mut inner, id);
    }

    if !visitor.is_metadata_rewrite_phase() || inner.indices.is_empty() {
        return;
    }
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

#[cfg(test)]
#[inline]
pub(crate) fn test_keys_edge_suppressed() -> bool {
    KEYS_EDGE_SUPPRESSED.with(std::cell::Cell::get)
}

/// RAII guard so a panicking fixture cannot leave a suppression on for the
/// next test on this thread.
#[cfg(test)]
pub(crate) struct TestKeysEdgeSuppression {
    edge: bool,
}

#[cfg(test)]
impl TestKeysEdgeSuppression {
    /// Drop the only edge. Nothing roots or rewrites the keys array.
    pub(crate) fn without_descriptor_edge() -> Self {
        Self {
            edge: KEYS_EDGE_SUPPRESSED.with(|c| c.replace(true)),
        }
    }
}

#[cfg(test)]
impl Drop for TestKeysEdgeSuppression {
    fn drop(&mut self) {
        KEYS_EDGE_SUPPRESSED.with(|c| c.set(self.edge));
    }
}

/// Test-only sabotage of the recycled-address type check. Keeping this scoped
/// and unshipped lets the regression fixture prove its detector would fail if
/// both prune and metadata rewrite trusted the replacement tenant.
#[cfg(test)]
pub(crate) struct TestRecycledKeysCheckSuppression {
    previous: bool,
}

#[cfg(test)]
impl TestRecycledKeysCheckSuppression {
    pub(crate) fn new() -> Self {
        Self {
            previous: RECYCLED_KEYS_CHECK_SUPPRESSED.with(|cell| cell.replace(true)),
        }
    }
}

#[cfg(test)]
impl Drop for TestRecycledKeysCheckSuppression {
    fn drop(&mut self) {
        RECYCLED_KEYS_CHECK_SUPPRESSED.with(|cell| cell.set(self.previous));
    }
}

#[cfg(test)]
pub(crate) fn test_shape_entry_exists(keys_id: usize) -> bool {
    crate::state::state()
        .shapes
        .inner
        .borrow()
        .indices
        .get(&keys_id)
        .is_some()
}

#[cfg(test)]
pub(crate) fn test_shape_descriptor_count() -> usize {
    crate::state::state()
        .shapes
        .inner
        .borrow()
        .descriptors
        .len()
}

#[cfg(test)]
pub(crate) fn test_clear_shape_table() {
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    inner.indices.clear();
    inner.descriptors.clear();
    inner.ids_by_facts.clear();
    inner.ids_by_keys.clear();
    drop(inner);
    clear_shape_object_kind_cache();
}

#[cfg(test)]
pub(crate) fn test_drop_shape_descriptors(keys_id: usize) {
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    let stale = inner
        .ids_by_keys
        .remove(&(keys_id as u64))
        .unwrap_or_default();
    for id in stale {
        remove_descriptor_and_reverse_indices(&mut inner, id);
    }
}

#[cfg(test)]
pub(crate) fn test_seed_shape_entry(keys_id: usize) {
    crate::state::state()
        .shapes
        .inner
        .borrow_mut()
        .indices
        .insert(
            keys_id,
            ShapeIndex {
                indexed_len: 0,
                slots: HashMap::new(),
            },
        );
    let _ = shape_descriptor_ensure(keys_id as *const ArrayHeader, 0, 0)
        .expect("test shape id range unexpectedly exhausted");
}

#[cfg(test)]
pub(crate) fn test_shape_id_for_keys(keys_id: usize) -> Option<u32> {
    let inner = crate::state::state().shapes.inner.borrow();
    inner
        .ids_by_keys
        .get(&(keys_id as u64))
        .and_then(|ids| ids.first().copied())
}

/// The shape-table unit suites, in a sibling file: `shapes.rs` sits close to
/// the repo's 2000-line-per-file cap and #8112 added the descriptor record's
/// keys slot and old-carrier gate to it. Moved verbatim.
#[cfg(test)]
#[path = "shapes_tests.rs"]
mod shapes_tests;
