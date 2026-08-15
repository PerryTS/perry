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
//! live inline-slot bound, and semantic generation. The legacy
//! `ObjectHeader::{keys_array,field_count}` words remain ABI mirrors until
//! #8047 removes them; guards and GC must not use their values as shape facts.

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

/// Immutable facts named by one ShapeId. The raw keys pointer is a weak mirror
/// of the authoritative `ObjectHeader::keys_array` edge: live-object scans
/// rekey it immediately after visiting that header slot, and the metadata pass
/// repairs deferred forwarding. The table itself never exposes a GC slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ShapeDescriptor {
    /// Raw ArrayHeader address in Perry's fixed-width heap-word ABI. Keeping
    /// this weak mirror u64 preserves identical representation on ILP32/LP64.
    pub(crate) keys: u64,
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

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum ShapeObjectKind {
    Ordinary,
    Class,
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
    descriptors: HashMap<u32, ShapeDescriptor>,
    /// Exact-facts reverse index. More than one id is legal when a worker
    /// minted a local descriptor before a process-global module id arrived.
    ids_by_facts: HashMap<ShapeFacts, Vec<u32>>,
    /// Keys-array address -> every descriptor id that currently names it.
    /// Same-address key-count retirement uses this index instead of scanning
    /// every shape ever observed by the agent.
    ids_by_keys: HashMap<u64, Vec<u32>>,
}

pub(crate) struct ShapeTable {
    inner: RefCell<ShapeTableInner>,
}

impl ShapeTable {
    pub(crate) fn new() -> Self {
        ShapeTable {
            inner: RefCell::new(ShapeTableInner {
                indices: crate::fast_hash::new_ptr_hash_map(),
                descriptors: HashMap::new(),
                ids_by_facts: HashMap::new(),
                ids_by_keys: HashMap::new(),
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

fn remove_id_from_keys_index(inner: &mut ShapeTableInner, keys: u64, id: u32) {
    let remove_entry = if let Some(ids) = inner.ids_by_keys.get_mut(&keys) {
        ids.retain(|&candidate| candidate != id);
        ids.is_empty()
    } else {
        false
    };
    if remove_entry {
        inner.ids_by_keys.remove(&keys);
    }
}

fn remove_id_from_facts_index(inner: &mut ShapeTableInner, facts: ShapeFacts, id: u32) {
    let remove_entry = if let Some(ids) = inner.ids_by_facts.get_mut(&facts) {
        ids.retain(|&candidate| candidate != id);
        ids.is_empty()
    } else {
        false
    };
    if remove_entry {
        inner.ids_by_facts.remove(&facts);
    }
}

fn rebuild_descriptor_reverse_indices(inner: &mut ShapeTableInner) {
    let mut ids_by_facts: HashMap<ShapeFacts, Vec<u32>> =
        HashMap::with_capacity(inner.descriptors.len());
    let mut ids_by_keys: HashMap<u64, Vec<u32>> = HashMap::new();
    for (&id, &descriptor) in &inner.descriptors {
        ids_by_facts
            .entry(descriptor_facts(descriptor))
            .or_default()
            .push(id);
        ids_by_keys.entry(descriptor.keys).or_default().push(id);
    }
    inner.ids_by_facts = ids_by_facts;
    inner.ids_by_keys = ids_by_keys;
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
        logical_key_count,
        live_inline_slot_count,
        semantic_generation,
        object_kind,
    };
    // Publish by-id first, then the reverse accelerator. An ObjectHeader is
    // stamped only after this function returns, so a visible id always has a
    // complete descriptor.
    inner.descriptors.insert(id, descriptor);
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
        .copied()
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
        logical_key_count,
        live_inline_slot_count,
        semantic_generation: 0,
        object_kind: ShapeObjectKind::Ordinary,
    };
    let facts = descriptor_facts(descriptor);
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    if let Some(existing) = inner.descriptors.get(&id) {
        return *existing == descriptor;
    }
    // A worker can have minted an equivalent local descriptor before module
    // initialization installs the process-global codegen id. Keep both id
    // descriptors valid for already-published objects and make the external
    // id canonical for subsequent births in this agent.
    inner.descriptors.insert(id, descriptor);
    // An equivalent local descriptor can predate module initialization. Keep
    // both reverse-index entries and prefer the external id for subsequent
    // births in this agent; already-published local ids remain resolvable.
    inner.ids_by_facts.entry(facts).or_default().insert(0, id);
    inner
        .ids_by_keys
        .entry(descriptor.keys)
        .or_default()
        .push(id);
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

/// Stamp `obj` with the exact ShapeId of `keys`, minting the descriptor on
/// first touch. Returns 0 only when the receiver is not a shaped object.
/// Exhaustion fails stop: no live object may depend on the
/// compatibility pointer/count mirrors for its shape.
#[inline]
pub(crate) unsafe fn stamp_object_shape(
    obj: *mut crate::object::ObjectHeader,
    keys: *const ArrayHeader,
    key_count: u32,
) -> u32 {
    if !shape_word_is_writable(obj) {
        return 0;
    }
    let Some(lineage) = object_shape_descriptor(obj) else {
        let id = shape_descriptor_ensure(keys, key_count, (*obj).field_count)
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
/// No `shape_word_is_writable` check: the callers have just written
/// `object_type`/`class_id` into a header they allocated, so the receiver is a
/// genuine `ObjectHeader` and never the `RegExpHeader` alias.
#[inline]
pub(crate) unsafe fn birth_stamp_object_shape(
    obj: *mut crate::object::ObjectHeader,
    runtime_shape_id: u32,
) {
    if obj.is_null() || !shape_word_is_writable(obj) {
        return;
    }
    let current = object_shape_descriptor(obj).unwrap_or_else(|| {
        synchronize_object_shape_descriptor(obj);
        object_shape_descriptor(obj).expect("shape synchronization must publish a descriptor")
    });
    let keys = current.keys as usize as *mut ArrayHeader;
    let key_count = current.logical_key_count;
    let supplied_id_is_local = descriptor_matches_object(runtime_shape_id, obj)
        || install_external_shape_id(runtime_shape_id, keys, key_count, (*obj).field_count);
    if supplied_id_is_local {
        (*obj).parent_class_id = runtime_shape_id;
        debug_assert_object_shape_parity(obj);
    } else {
        synchronize_object_shape_descriptor(obj);
    }
}

/// Install the exact descriptor for the object's current authoritative header
/// facts. This is the only structural shape publication operation used by
/// mutations. Keyless objects receive a descriptor too.
pub(crate) unsafe fn synchronize_object_shape_descriptor(
    obj: *mut crate::object::ObjectHeader,
) -> u32 {
    let predecessor = object_shape_descriptor(obj);
    synchronize_object_shape_descriptor_from(obj, predecessor)
}

/// Structural synchronization after a caller has temporarily cleared the
/// stamp. `predecessor` carries semantic lineage (including class kind) across
/// the pointer/count mutation without exposing stale structural facts.
pub(crate) unsafe fn synchronize_object_shape_descriptor_from(
    obj: *mut crate::object::ObjectHeader,
    predecessor: Option<ShapeDescriptor>,
) -> u32 {
    if obj.is_null() || !shape_word_is_writable(obj) {
        return 0;
    }
    let keys = (*obj).keys_array;
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
            let Some(gc) = crate::value::addr_class::try_read_tracked_gc_header(keys as usize)
            else {
                clear_object_shape_stamp(obj);
                return 0;
            };
            if (*gc.as_ptr()).obj_type != crate::gc::GC_TYPE_ARRAY {
                clear_object_shape_stamp(obj);
                return 0;
            }
            let shared = (*gc.as_ptr()).gc_flags & crate::gc::GC_FLAG_SHAPE_SHARED != 0;
            debug_assert!(
                !shared,
                "shared keys array mutated in place under an immutable ShapeId"
            );
            if shared {
                clear_object_shape_stamp(obj);
                return 0;
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
        (*obj).field_count,
        semantic_generation,
        object_kind,
    ));
    (*obj).parent_class_id = id;
    debug_assert_object_shape_parity(obj);
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
        let Some(descriptor) = inner.descriptors.get(&id).copied() else {
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

fn descriptor_matches_object(shape_id: u32, obj: *const crate::object::ObjectHeader) -> bool {
    let Some(d) = shape_descriptor_by_id(shape_id) else {
        return false;
    };
    unsafe {
        let keys = (*obj).keys_array;
        let key_count = if keys.is_null() {
            0
        } else {
            crate::array::keys_array_len_capped_to_capacity(keys) as u32
        };
        d.keys == keys as u64
            && d.logical_key_count == key_count
            && d.live_inline_slot_count == (*obj).field_count
    }
}

#[inline]
pub(crate) unsafe fn debug_assert_object_shape_parity(obj: *const crate::object::ObjectHeader) {
    let id = object_shape_stamp(obj);
    if id != 0 {
        debug_assert!(
            descriptor_matches_object(id, obj),
            "published ShapeId disagrees with authoritative ObjectHeader facts"
        );
    }
}

/// Validate and immediately mirror a live object's authoritative keys edge
/// after that header slot has been visited. No address inside the descriptor
/// HashMap is ever handed to a generic visitor: remembered-set enumeration can
/// save slot pointers across budgeted mutator resumptions, while descriptor
/// insertion/pruning may reallocate the table in between.
///
/// An immediate copying visitor has already rewritten `new_header_keys`; a
/// deferred dirty-work visitor leaves it equal to `old_header_keys`, and the
/// registered metadata forwarding pass repairs the weak mirror after copying.
/// Exact release-mode facts prevent a stale or foreign id from rekeying an
/// unrelated descriptor. Returns whether the descriptor facts validated.
pub(crate) unsafe fn synchronize_live_object_shape_descriptor_after_header_visit(
    obj: *const crate::object::ObjectHeader,
    old_header_keys: u64,
    new_header_keys: u64,
    logical_key_count: u32,
    live_inline_slot_count: u32,
) -> bool {
    let shape_id = object_shape_stamp(obj);
    if shape_id == 0 {
        return false;
    }

    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    let (old_facts, new_facts) = {
        let Some(descriptor) = inner.descriptors.get_mut(&shape_id) else {
            // A foreign-agent/stale id fails closed; the authoritative header
            // edge is still traced by the caller.
            return false;
        };
        // Release-mode fail-closed gate. An id hit is insufficient: a foreign
        // or stale id must never cause an unrelated descriptor pointer to be
        // rekeyed. `new_header_keys` may differ after evacuation; a sibling
        // may also have rewritten the shared descriptor before this object.
        if descriptor.logical_key_count != logical_key_count
            || descriptor.live_inline_slot_count != live_inline_slot_count
            || (descriptor.keys != old_header_keys && descriptor.keys != new_header_keys)
        {
            return false;
        }
        let old_facts = descriptor_facts(*descriptor);
        if descriptor.keys == old_header_keys && new_header_keys != old_header_keys {
            descriptor.keys = new_header_keys;
        }
        (old_facts, descriptor_facts(*descriptor))
    };
    if new_facts != old_facts {
        remove_id_from_facts_index(&mut inner, old_facts, shape_id);
        inner
            .ids_by_facts
            .entry(new_facts)
            .or_default()
            .push(shape_id);
        remove_id_from_keys_index(&mut inner, old_facts.keys, shape_id);
        inner
            .ids_by_keys
            .entry(new_facts.keys)
            .or_default()
            .push(shape_id);
    }
    true
}

/// Drop the stamp iff the word currently holds one, leaving a real
/// `parent_class_id` untouched. Returns true when a stamp was cleared.
///
/// Ids are never reused, so clearing makes every stale id-keyed cache entry a
/// permanent miss; the next resolve re-stamps from whatever record the live
/// keys array has then.
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

/// Post-trace weak-table prune: drop slot indices and by-id descriptors whose
/// keys array is dead. A live object has already traced its authoritative
/// header edge and synchronized the descriptor named by its ShapeId, so a
/// descriptor removed here cannot be named by a live object. Correctness fails
/// closed on a missing lookup, independently of pruning.
pub(crate) fn prune_dead_shape_keys(is_dead_owner: &dyn Fn(usize) -> bool) {
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    if !inner.indices.is_empty() {
        inner.indices.retain(|keys_id, _| !is_dead_owner(*keys_id));
    }
    let stale: Vec<u32> = inner
        .descriptors
        .iter()
        .filter_map(|(&id, descriptor)| is_dead_owner(descriptor.keys as usize).then_some(id))
        .collect();
    if !stale.is_empty() {
        for id in stale {
            inner.descriptors.remove(&id);
        }
        // Rebuild rather than removing by current facts one-at-a-time. A
        // deferred live-object rewrite may have changed the by-id pointer
        // before the metadata scanner repaired the reverse accelerators; a
        // rebuild cannot retain either an old-facts or old-keys entry.
        rebuild_descriptor_reverse_indices(&mut inner);
    }
}

/// Metadata-only forwarding repair for the weak descriptor table and
/// pointer-keyed slot indices. Mark/copy mode does not root anything; live
/// object scans provide descriptor reachability, and post-copy rewrite follows
/// only forwarding records those live edges already created.
pub(crate) fn scan_shape_table_rekey_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    let mut descriptor_moved = false;
    for descriptor in inner.descriptors.values_mut() {
        let mut addr = descriptor.keys as usize;
        if visitor.visit_metadata_usize_slot(&mut addr) {
            descriptor.keys = addr as u64;
            descriptor_moved = true;
        }
    }
    // Rebuild throughout rewrite phase even when this pass itself observed no
    // move: an immediate live-object header callback may already have rekeyed
    // a shared descriptor, while a deferred callback relies on this pass.
    if descriptor_moved || visitor.is_metadata_rewrite_phase() {
        rebuild_descriptor_reverse_indices(&mut inner);
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
}

#[cfg(test)]
pub(crate) fn test_drop_shape_descriptors(keys_id: usize) {
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    let stale = inner
        .ids_by_keys
        .remove(&(keys_id as u64))
        .unwrap_or_default();
    for id in stale {
        if let Some(descriptor) = inner.descriptors.remove(&id) {
            remove_id_from_facts_index(&mut inner, descriptor_facts(descriptor), id);
        }
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

#[cfg(test)]
mod c3c_tests {
    use super::*;

    fn key(name: &str) -> *mut crate::StringHeader {
        crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
    }

    /// #6759 C3c: ids come from the dedicated range (disjoint from real and
    /// builtin class ids), are stable per exact descriptor facts, and distinct
    /// across identities.
    #[test]
    fn shape_ids_are_range_disjoint_and_stable() {
        let _lock = crate::gc::global_side_table_test_lock();
        let a: usize = 0xC3C0_0000_0000_1000;
        let b: usize = 0xC3C0_0000_0000_2000;
        let ida = shape_id_for_keys_ensure(a as *const ArrayHeader, 4);
        let idb = shape_id_for_keys_ensure(b as *const ArrayHeader, 4);
        assert!(is_shape_id(ida) && is_shape_id(idb));
        assert_ne!(ida, idb);
        assert_eq!(shape_id_for_keys_ensure(a as *const ArrayHeader, 4), ida);
        // Real class-id space must never classify as a shape id.
        assert!(!is_shape_id(0));
        assert!(!is_shape_id(1));
        assert!(!is_shape_id(0x7FFF_FF30));
        assert!(!is_shape_id(0xFFFF_0005));
        shape_drop(a as *const ArrayHeader);
        shape_drop(b as *const ArrayHeader);
        test_drop_shape_descriptors(a);
        test_drop_shape_descriptors(b);
    }

    /// #6759 C3 rung 2: the codegen-facing allocator receives the id minted
    /// beside its canonical keys global and installs it before the newborn
    /// instance is published to user code. No by-name lookup is allowed in
    /// this fixture: observing a stamp therefore proves it was present at
    /// birth rather than lazily self-healed by rung 1.
    #[test]
    fn compiled_class_allocator_stamps_the_canonical_shape_at_birth() {
        let _lock = crate::gc::global_side_table_test_lock();
        const CID: u32 = 0x0C3C_7902;
        let packed = b"birth_a\0birth_b";
        let keys =
            crate::object::js_build_class_keys_array(CID, 2, packed.as_ptr(), packed.len() as u32);
        let shape_id = js_object_shape_id_for_keys(keys as usize as u64, 2);
        assert!(
            is_shape_id(shape_id),
            "module init must mint a real ShapeId"
        );

        let obj =
            crate::object::js_object_alloc_class_inline_keys_stamped(CID, 0, 2, keys, shape_id);
        let birth_word = unsafe { (*obj).parent_class_id };
        assert_eq!(
            birth_word, shape_id,
            "a fresh compiled class instance waited for a by-name lookup to stamp"
        );
        assert_eq!(
            unsafe { (*obj).keys_array },
            keys,
            "the stamp and canonical keys global must describe the same shape"
        );
    }

    /// #6759 C3c stamp invariant on a REAL object through the real
    /// write/read paths: a read resolution stamps a shape id into the
    /// plain object's `parent_class_id`; after further appends any surviving
    /// stamp resolves to exact current pointer/logical/live facts. This fixture
    /// deliberately reserves eight live inline slots while owning fewer keys,
    /// so the old key-count-only compatibility mint is not the expected id.
    #[test]
    fn plain_object_stamp_lifecycle() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let obj = crate::object::js_object_alloc(0, 8);
            for name in ["c3c_a", "c3c_b", "c3c_c"] {
                crate::object::js_object_set_field_by_name(obj, key(name), 1.0);
            }
            assert_eq!((*obj).class_id, 0, "test premise: plain object");
            let _ = crate::object::js_object_get_field_by_name(obj, key("c3c_b"));
            let stamp = (*obj).parent_class_id;
            assert!(
                is_shape_id(stamp),
                "read resolution must stamp a shape id, got {stamp:#x}"
            );

            crate::object::js_object_set_field_by_name(obj, key("c3c_d"), 2.0);
            crate::object::js_object_set_field_by_name(obj, key("c3c_e"), 3.0);
            let stamp2 = (*obj).parent_class_id;
            if stamp2 != 0 {
                assert!(is_shape_id(stamp2));
                let descriptor = shape_descriptor_by_id(stamp2)
                    .expect("a surviving stamp must resolve in this agent");
                assert_eq!(descriptor.keys, (*obj).keys_array as u64);
                assert_eq!(
                    descriptor.logical_key_count,
                    crate::array::js_array_length((*obj).keys_array)
                );
                assert_eq!(descriptor.live_inline_slot_count, (*obj).field_count);
                debug_assert_object_shape_parity(obj);
            }

            // Reads still resolve correctly through the id-keyed cache.
            let v = crate::object::js_object_get_field_by_name(obj, key("c3c_d"));
            assert_eq!(f64::from_bits(v.bits()), 2.0);
        }
    }
}

#[cfg(test)]
mod c6804_tests {
    use super::*;

    /// #6804: shape-cached literal allocation birth-stamps the runtime
    /// ShapeId, and siblings of one shape share one id.
    #[test]
    fn alloc_with_shape_birth_stamps_shared_id() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let packed = b"m6804_a\0m6804_b\0m6804_c";
            let a = crate::object::js_object_alloc_with_shape(
                0x0C3C_6804,
                3,
                packed.as_ptr(),
                packed.len() as u32,
            );
            let b = crate::object::js_object_alloc_with_shape(
                0x0C3C_6804,
                3,
                packed.as_ptr(),
                packed.len() as u32,
            );
            let stamp_a = (*a).parent_class_id;
            let stamp_b = (*b).parent_class_id;
            assert!(
                is_shape_id(stamp_a),
                "newborn literal must carry a runtime ShapeId, got {stamp_a:#x}"
            );
            assert_eq!(
                stamp_a, stamp_b,
                "siblings of one literal shape must share one id"
            );
            assert_eq!(
                (*a).keys_array,
                (*b).keys_array,
                "test premise: shared keys"
            );
        }
    }

    /// #6804: `object_shape()` self-heals — an unstamped plain object gets
    /// stamped at first observation, and the token equals the id every
    /// sibling already carries (no pre/post-stamp token split).
    #[test]
    fn object_shape_token_self_heals_to_shared_id() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let packed = b"m6804_x\0m6804_y";
            let obj = crate::object::js_object_alloc_with_shape(
                0x0C3C_6805,
                2,
                packed.as_ptr(),
                packed.len() as u32,
            );
            let birth_stamp = (*obj).parent_class_id;
            assert!(is_shape_id(birth_stamp), "test premise: birth-stamped");

            // Simulate a pre-#6804 / cleared-stamp object of the same shape.
            (*obj).parent_class_id = 0;
            let token = crate::typed_feedback::test_object_shape_token(obj as usize);
            assert_eq!(
                token, birth_stamp as usize,
                "self-healed token must equal the shape's canonical id"
            );
            assert_eq!(
                (*obj).parent_class_id,
                birth_stamp,
                "observation must re-stamp the object"
            );
        }
    }

    /// #6804: the first dynamic key on a fresh `{}` births a stamped shape.
    #[test]
    fn fresh_dynamic_shape_birth_stamps() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let obj = crate::object::js_object_alloc(0, 8);
            let key = crate::string::js_string_from_bytes(b"m6804_first".as_ptr(), 11);
            crate::object::js_object_set_field_by_name(obj, key, 42.0);
            let stamp = (*obj).parent_class_id;
            // Either stamped at the null-branch birth, or (for a sibling
            // adopting a cached transition edge) still 0 until first read
            // — but THIS test allocates a unique key, so the null branch
            // ran and must have stamped.
            assert!(
                is_shape_id(stamp),
                "first-key birth must stamp the new shape, got {stamp:#x}"
            );
        }
    }
}

#[cfg(test)]
mod descriptor_tests_8067 {
    use super::*;

    fn key(name: &str) -> *mut crate::StringHeader {
        crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
    }

    #[test]
    fn every_keyless_runtime_allocator_publishes_a_shape_id() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            for obj in [
                crate::object::js_object_alloc(0, 0),
                crate::object::js_object_alloc_fast(0, 0),
                crate::object::js_object_alloc_with_parent(0x8067_0101, 0, 0),
                crate::object::js_object_alloc_fast_with_parent(0x8067_0102, 0, 0),
            ] {
                let id = object_shape_id(obj);
                assert!(is_shape_id(id), "newborn keyless object has no ShapeId");
                let facts = object_shape_descriptor(obj).expect("keyless descriptor");
                assert_eq!(facts.keys, 0);
                assert_eq!(facts.logical_key_count, 0);
                assert_eq!(facts.live_inline_slot_count, 0);
            }
        }
    }

    #[test]
    fn descriptor_and_prototype_changes_mint_semantic_successors() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let obj = crate::object::js_object_alloc(0, 1);
            crate::object::js_object_set_field_by_name(obj, key("semantic8067"), 1.0);
            let structural = object_shape_id(obj);

            crate::object::descriptor_state::set_property_attrs(
                obj as usize,
                "semantic8067".to_string(),
                crate::object::descriptor_state::PropertyAttrs::new(false, true, true),
            );
            let described = object_shape_id(obj);
            assert_ne!(described, structural);
            let described_facts = object_shape_descriptor(obj).unwrap();
            assert_ne!(described_facts.semantic_generation, 0);

            crate::object::prototype_chain::object_set_static_prototype(
                obj as usize,
                crate::value::TAG_NULL,
            );
            let reparented = object_shape_id(obj);
            assert_ne!(reparented, described);
            assert_eq!(
                object_shape_descriptor(obj).unwrap().keys,
                described_facts.keys,
                "semantic transitions must preserve the rooted ordered keys edge"
            );
        }
    }

    #[test]
    fn absent_descriptor_clears_do_not_mint_semantic_successors() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let obj = crate::object::js_object_alloc(0, 1);
            let addr = obj as usize;
            let initial = object_shape_id(obj);

            crate::object::descriptor_state::clear_property_attrs(addr, "missing8067");
            crate::object::descriptor_state::clear_accessor_descriptor(addr, "missing8067");
            assert_eq!(object_shape_id(obj), initial);

            crate::object::descriptor_state::set_property_attrs(
                addr,
                "attrs8067".to_string(),
                crate::object::descriptor_state::PropertyAttrs::new(false, true, true),
            );
            crate::object::descriptor_state::clear_property_attrs(addr, "attrs8067");
            let after_real_attr_clear = object_shape_id(obj);
            crate::object::descriptor_state::clear_property_attrs(addr, "attrs8067");
            assert_eq!(object_shape_id(obj), after_real_attr_clear);

            crate::object::descriptor_state::set_accessor_descriptor(
                addr,
                "accessor8067".to_string(),
                crate::object::descriptor_state::AccessorDescriptor::default(),
            );
            crate::object::descriptor_state::clear_accessor_descriptor(addr, "accessor8067");
            let after_real_accessor_clear = object_shape_id(obj);
            crate::object::descriptor_state::clear_accessor_descriptor(addr, "accessor8067");
            assert_eq!(object_shape_id(obj), after_real_accessor_clear);
        }
    }

    #[test]
    fn delete_compaction_never_compares_equal_to_the_predelete_layout() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let obj = crate::object::js_object_alloc(0, 3);
            let a = key("delete8067_a");
            let b = key("delete8067_b");
            let c = key("delete8067_c");
            crate::object::js_object_set_field_by_name(obj, a, 1.0);
            crate::object::js_object_set_field_by_name(obj, b, 2.0);
            crate::object::js_object_set_field_by_name(obj, c, 3.0);
            let before = object_shape_id(obj);
            assert_eq!(crate::object::js_object_delete_field(obj, a), 1);
            let after = object_shape_id(obj);
            assert_ne!(after, before);
            let facts = object_shape_descriptor(obj).unwrap();
            assert_eq!(facts.logical_key_count, 2);
            assert_eq!(facts.live_inline_slot_count, 2);
            assert_eq!(
                crate::object::js_object_get_field_by_name_f64(obj, b),
                2.0,
                "middle-field lookup used a stale pre-delete slot mapping"
            );
        }
    }

    #[test]
    fn exhaustion_parks_without_reuse_or_alias() {
        let next = std::sync::atomic::AtomicU32::new(SHAPE_ID_END - 1);
        assert_eq!(alloc_shape_id_from(&next), Ok(SHAPE_ID_END - 1));
        assert_eq!(alloc_shape_id_from(&next), Err(ShapeIdExhausted));
        assert_eq!(alloc_shape_id_from(&next), Err(ShapeIdExhausted));
        assert_eq!(
            next.load(std::sync::atomic::Ordering::Relaxed),
            SHAPE_ID_END,
            "exhaustion must park instead of wrapping into an alias"
        );
    }

    #[test]
    fn inconsistent_facts_are_not_reported_as_id_exhaustion() {
        assert_eq!(
            shape_descriptor_ensure(std::ptr::null(), 1, 1),
            Err(ShapeDescriptorError::InvalidFacts)
        );
    }

    #[test]
    fn equivalent_local_and_external_ids_remain_resolvable() {
        let _lock = crate::gc::global_side_table_test_lock();
        let keys = 0x8067_0000_0000_1700usize;
        let local = shape_descriptor_ensure(keys as *const ArrayHeader, 1, 1)
            .expect("shape range unexpectedly exhausted");
        let external = alloc_shape_id().expect("shape range unexpectedly exhausted");
        assert!(install_external_shape_id(
            external,
            keys as *const ArrayHeader,
            1,
            1,
        ));

        assert_eq!(
            shape_descriptor_ensure(keys as *const ArrayHeader, 1, 1).unwrap(),
            external,
            "the process-global id should be preferred for later births"
        );
        retain_key_count_versions(keys as u64);
        assert!(shape_descriptor_by_id(local).is_some());
        assert!(shape_descriptor_by_id(external).is_some());

        test_drop_shape_descriptors(keys);
    }

    #[test]
    fn a_foreign_agent_id_misses_instead_of_aliasing_same_address() {
        let _lock = crate::gc::global_side_table_test_lock();
        let fake_keys = 0x8067_0000_0000_1000usize;
        let local = shape_descriptor_ensure(fake_keys as *const ArrayHeader, 2, 2)
            .expect("shape range unexpectedly exhausted");
        let foreign = std::thread::spawn(move || {
            assert_eq!(
                shape_descriptor_by_id(local),
                None,
                "another RuntimeState resolved a foreign agent's ShapeId"
            );
            shape_descriptor_ensure(fake_keys as *const ArrayHeader, 2, 2)
                .expect("shape range unexpectedly exhausted")
        })
        .join()
        .expect("agent-isolation thread panicked");
        assert_ne!(
            local, foreign,
            "process-global ids must not alias by address"
        );
        shape_drop(fake_keys as *const ArrayHeader);
        test_drop_shape_descriptors(fake_keys);
    }

    #[test]
    fn process_global_module_shape_id_installs_with_agent_local_keys() {
        let _lock = crate::gc::global_side_table_test_lock();
        let module_keys = 0x8067_0000_0000_1800usize;
        let module_id = shape_descriptor_ensure(module_keys as *const ArrayHeader, 2, 2)
            .expect("shape range unexpectedly exhausted");
        let worker_keys = 0x8067_0000_0000_1900usize;
        std::thread::spawn(move || {
            assert!(install_external_shape_id(
                module_id,
                worker_keys as *const ArrayHeader,
                2,
                2,
            ));
            assert_eq!(
                shape_descriptor_by_id(module_id).unwrap().keys,
                worker_keys as u64,
                "worker resolved a module ShapeId to another agent's keys pointer"
            );
        })
        .join()
        .expect("worker shape installation panicked");
        test_drop_shape_descriptors(module_keys);
    }

    #[test]
    fn gc_descriptor_mirror_requires_exact_release_facts() {
        let _lock = crate::gc::global_side_table_test_lock();
        let keys = 0x8067_0000_0000_2000usize;
        let id = shape_descriptor_ensure(keys as *const ArrayHeader, 3, 2)
            .expect("shape range unexpectedly exhausted");
        let obj = crate::object::ObjectHeader {
            object_type: 1,
            class_id: 0,
            parent_class_id: id,
            field_count: 2,
            keys_array: keys as *mut ArrayHeader,
            meta: std::ptr::null_mut(),
        };

        unsafe {
            assert!(
                !synchronize_live_object_shape_descriptor_after_header_visit(
                    &obj,
                    keys as u64 + 0x1000,
                    keys as u64 + 0x2000,
                    3,
                    2,
                )
            );
            assert!(
                !synchronize_live_object_shape_descriptor_after_header_visit(
                    &obj,
                    keys as u64,
                    keys as u64,
                    4,
                    2,
                )
            );
        }
        assert_eq!(shape_descriptor_by_id(id).unwrap().keys, keys as u64);

        let moved_keys = keys as u64 + 0x3000;
        assert!(unsafe {
            synchronize_live_object_shape_descriptor_after_header_visit(
                &obj,
                keys as u64,
                moved_keys,
                3,
                2,
            )
        });
        assert_eq!(shape_descriptor_by_id(id).unwrap().keys, moved_keys);
        test_drop_shape_descriptors(moved_keys as usize);
        assert_eq!(
            shape_descriptor_by_id(id),
            None,
            "descriptor rekey did not update the keys-address index"
        );
    }

    #[test]
    fn key_count_versions_remain_resolvable_until_the_keys_die() {
        let _lock = crate::gc::global_side_table_test_lock();
        let keys = 0x8067_0000_0000_2100usize;
        let unrelated_keys = 0x8067_0000_0000_2200usize;
        let stale_a = shape_descriptor_ensure(keys as *const ArrayHeader, 1, 1)
            .expect("shape range unexpectedly exhausted");
        let stale_b = shape_descriptor_ensure(keys as *const ArrayHeader, 1, 2)
            .expect("shape range unexpectedly exhausted");
        let current = shape_descriptor_ensure(keys as *const ArrayHeader, 2, 2)
            .expect("shape range unexpectedly exhausted");
        let unrelated = shape_descriptor_ensure(unrelated_keys as *const ArrayHeader, 1, 1)
            .expect("shape range unexpectedly exhausted");

        retain_key_count_versions(keys as u64);

        assert!(shape_descriptor_by_id(stale_a).is_some());
        assert!(shape_descriptor_by_id(stale_b).is_some());
        assert!(shape_descriptor_by_id(current).is_some());
        assert!(shape_descriptor_by_id(unrelated).is_some());
        let inner = crate::state::state().shapes.inner.borrow();
        let current_ids = inner
            .ids_by_keys
            .get(&(keys as u64))
            .expect("keys identity disappeared from descriptor index");
        assert_eq!(current_ids.as_slice(), &[stale_a, stale_b, current]);
        drop(inner);

        test_drop_shape_descriptors(keys);
        test_drop_shape_descriptors(unrelated_keys);
    }

    #[test]
    fn shape_drop_does_not_delete_a_potential_siblings_descriptor() {
        let _lock = crate::gc::global_side_table_test_lock();
        let keys = 0x8067_0000_0000_3000usize;
        let id = shape_descriptor_ensure(keys as *const ArrayHeader, 1, 1)
            .expect("shape range unexpectedly exhausted");

        shape_drop(keys as *const ArrayHeader);

        assert_eq!(
            shape_descriptor_by_id(id).map(|descriptor| descriptor.keys),
            Some(keys as u64),
            "shape_drop eagerly invalidated a descriptor a sibling may still name"
        );
        test_drop_shape_descriptors(keys);
    }

    #[test]
    fn live_slot_growth_versions_descriptor_before_value_publication() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let packed = b"slot8067_a";
            let obj = crate::object::js_object_alloc_with_shape(
                0x8067_1001,
                1,
                packed.as_ptr(),
                packed.len() as u32,
            );
            let keys = (*obj).keys_array as usize;
            let before = (*obj).parent_class_id;
            let before_descriptor = shape_descriptor_by_id(before).expect("birth descriptor");
            assert_eq!(before_descriptor.live_inline_slot_count, 1);

            crate::object::js_object_set_field(obj, 1, crate::JSValue::string_ptr(key("value")));
            let after = (*obj).parent_class_id;
            assert_ne!(before, after);
            let after_descriptor = shape_descriptor_by_id(after).expect("grown descriptor");
            assert_eq!(after_descriptor.keys, keys as u64);
            assert_eq!(after_descriptor.logical_key_count, 1);
            assert_eq!(after_descriptor.live_inline_slot_count, 2);
            debug_assert_object_shape_parity(obj);
        }
    }

    #[test]
    fn shared_sibling_append_clones_before_descriptor_version_changes() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let packed = b"sib8067_a";
            let a = crate::object::js_object_alloc_with_shape(
                0x8067_1002,
                1,
                packed.as_ptr(),
                packed.len() as u32,
            );
            let b = crate::object::js_object_alloc_with_shape(
                0x8067_1002,
                1,
                packed.as_ptr(),
                packed.len() as u32,
            );
            let shared_keys = (*a).keys_array;
            let shared_id = (*a).parent_class_id;
            assert_eq!(shared_keys, (*b).keys_array);
            assert_eq!(shared_id, (*b).parent_class_id);

            crate::object::js_object_set_field_by_name(a, key("sib8067_b"), 2.0);

            assert_ne!((*a).keys_array, shared_keys);
            assert_eq!((*b).keys_array, shared_keys);
            assert_eq!((*b).parent_class_id, shared_id);
            assert_ne!((*a).parent_class_id, shared_id);
            assert_eq!(
                shape_descriptor_by_id(shared_id)
                    .expect("untouched sibling descriptor")
                    .logical_key_count,
                1
            );
            let transitioned =
                shape_descriptor_by_id((*a).parent_class_id).expect("transitioned descriptor");
            assert_eq!(transitioned.keys, (*a).keys_array as u64);
            assert_eq!(transitioned.logical_key_count, 2);
            assert_eq!(transitioned.live_inline_slot_count, 2);
        }
    }
}
