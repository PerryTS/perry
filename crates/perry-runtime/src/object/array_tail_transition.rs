//! Exact bidirectional transitions for dense numeric object tails.
//!
//! Perry represents `class X extends Array` instances as shaped objects. A
//! generic tail delete therefore clones and compacts the complete ordered-key
//! array even when the runtime has already observed the inverse append. This
//! cache retains that learned `(predecessor, numeric key, successor)` edge in
//! both directions. It does not authorize mutation by itself: the Array
//! subclass fast path separately proves the receiver brand, dense layout,
//! descriptors, prototype state, live length, and physical value slot.

use crate::object::shapes;

pub(crate) const ARRAY_TAIL_TRANSITION_CACHE_SIZE: usize = 8192;
const ARRAY_TAIL_TRANSITION_CACHE_MASK: usize = ARRAY_TAIL_TRANSITION_CACHE_SIZE - 1;
const ARRAY_TAIL_DIRECT_INDEX_MISS: u16 = u16::MAX;

/// Compact exact-shape accelerator into the authoritative rooted transition
/// tables. One ShapeId can simultaneously be the successor of one numeric
/// append and the predecessor of the next, hence the two independent indices.
/// A collision only evicts this accelerator entry; the open-addressed tables
/// remain complete and are the semantics-preserving fallback.
#[derive(Clone, Copy)]
pub(crate) struct ArrayTailDirectIndex {
    shape_id: u32,
    forward: u16,
    reverse: u16,
}

impl ArrayTailDirectIndex {
    pub(crate) const EMPTY: Self = Self {
        shape_id: 0,
        forward: ARRAY_TAIL_DIRECT_INDEX_MISS,
        reverse: ARRAY_TAIL_DIRECT_INDEX_MISS,
    };
}

#[derive(Clone, Copy)]
pub(crate) struct ArrayTailTransitionEntry {
    pub(crate) predecessor_keys: usize,
    pub(crate) successor_keys: usize,
    pub(crate) predecessor_shape_id: u32,
    pub(crate) successor_shape_id: u32,
    pub(crate) slot: u32,
    pub(crate) array_index: u32,
    pub(crate) predecessor_live_inline_slots: u32,
    pub(crate) successor_live_inline_slots: u32,
}

impl ArrayTailTransitionEntry {
    pub(crate) const EMPTY: Self = Self {
        predecessor_keys: 0,
        successor_keys: 0,
        predecessor_shape_id: 0,
        successor_shape_id: 0,
        slot: 0,
        array_index: 0,
        predecessor_live_inline_slots: 0,
        successor_live_inline_slots: 0,
    };

    /// Deleted entries cannot become `EMPTY` without breaking a later entry's
    /// open-addressing probe chain. ShapeIds are nonzero, so this pointer-only
    /// marker is unambiguous and is never visited as a GC root.
    const TOMBSTONE: Self = Self {
        predecessor_keys: usize::MAX,
        successor_keys: 0,
        predecessor_shape_id: 0,
        successor_shape_id: 0,
        slot: 0,
        array_index: 0,
        predecessor_live_inline_slots: 0,
        successor_live_inline_slots: 0,
    };

    #[inline(always)]
    fn is_empty(self) -> bool {
        self.successor_shape_id == 0 && self.predecessor_keys == 0
    }

    #[inline(always)]
    fn is_tombstone(self) -> bool {
        self.successor_shape_id == 0 && self.predecessor_keys == usize::MAX
    }
}

#[inline(always)]
fn forward_slot(shape_id: u32, index: u32) -> usize {
    let mixed = u64::from(shape_id).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ u64::from(index).wrapping_mul(0xC6BC_2796_92B5_C323);
    mixed as usize & ARRAY_TAIL_TRANSITION_CACHE_MASK
}

#[inline(always)]
fn reverse_slot(shape_id: u32) -> usize {
    let mixed = u64::from(shape_id).wrapping_mul(0xD6E8_FEB8_6659_FD93);
    (mixed ^ (mixed >> 32)) as usize & ARRAY_TAIL_TRANSITION_CACHE_MASK
}

#[inline]
unsafe fn publish_entry(target: &mut ArrayTailTransitionEntry, entry: ArrayTailTransitionEntry) {
    target.predecessor_shape_id = entry.predecessor_shape_id;
    target.successor_shape_id = entry.successor_shape_id;
    target.slot = entry.slot;
    target.array_index = entry.array_index;
    target.predecessor_live_inline_slots = entry.predecessor_live_inline_slots;
    target.successor_live_inline_slots = entry.successor_live_inline_slots;
    crate::gc::runtime_store_root_usize_slot(&mut target.predecessor_keys, entry.predecessor_keys);
    crate::gc::runtime_store_root_usize_slot(&mut target.successor_keys, entry.successor_keys);
}

unsafe fn insert_forward(
    table: *mut [ArrayTailTransitionEntry; ARRAY_TAIL_TRANSITION_CACHE_SIZE],
    entry: ArrayTailTransitionEntry,
) -> Option<usize> {
    let start = forward_slot(entry.predecessor_shape_id, entry.array_index);
    let mut tombstone = None;
    for offset in 0..ARRAY_TAIL_TRANSITION_CACHE_SIZE {
        let index = (start + offset) & ARRAY_TAIL_TRANSITION_CACHE_MASK;
        let candidate = (*table)[index];
        if candidate.predecessor_shape_id == entry.predecessor_shape_id
            && candidate.array_index == entry.array_index
            && candidate.successor_shape_id != 0
        {
            publish_entry(&mut (*table)[index], entry);
            return Some(index);
        }
        if candidate.is_tombstone() && tombstone.is_none() {
            tombstone = Some(index);
        } else if candidate.is_empty() {
            let index = tombstone.unwrap_or(index);
            publish_entry(&mut (*table)[index], entry);
            return Some(index);
        }
    }
    if let Some(index) = tombstone {
        publish_entry(&mut (*table)[index], entry);
        return Some(index);
    }
    None
}

unsafe fn insert_reverse(
    table: *mut [ArrayTailTransitionEntry; ARRAY_TAIL_TRANSITION_CACHE_SIZE],
    entry: ArrayTailTransitionEntry,
) -> Option<usize> {
    let start = reverse_slot(entry.successor_shape_id);
    let mut tombstone = None;
    for offset in 0..ARRAY_TAIL_TRANSITION_CACHE_SIZE {
        let index = (start + offset) & ARRAY_TAIL_TRANSITION_CACHE_MASK;
        let candidate = (*table)[index];
        if candidate.successor_shape_id == entry.successor_shape_id {
            publish_entry(&mut (*table)[index], entry);
            return Some(index);
        }
        if candidate.is_tombstone() && tombstone.is_none() {
            tombstone = Some(index);
        } else if candidate.is_empty() {
            let index = tombstone.unwrap_or(index);
            publish_entry(&mut (*table)[index], entry);
            return Some(index);
        }
    }
    if let Some(index) = tombstone {
        publish_entry(&mut (*table)[index], entry);
        return Some(index);
    }
    None
}

#[inline]
fn with_forward<R>(
    f: impl FnOnce(*mut [ArrayTailTransitionEntry; ARRAY_TAIL_TRANSITION_CACHE_SIZE]) -> R,
) -> R {
    unsafe {
        let boxed = &mut *crate::state::state().object_hot.array_tail_forward.get();
        f(boxed.as_mut_ptr() as *mut [ArrayTailTransitionEntry; ARRAY_TAIL_TRANSITION_CACHE_SIZE])
    }
}

#[inline]
fn with_reverse<R>(
    f: impl FnOnce(*mut [ArrayTailTransitionEntry; ARRAY_TAIL_TRANSITION_CACHE_SIZE]) -> R,
) -> R {
    unsafe {
        let boxed = &mut *crate::state::state().object_hot.array_tail_reverse.get();
        f(boxed.as_mut_ptr() as *mut [ArrayTailTransitionEntry; ARRAY_TAIL_TRANSITION_CACHE_SIZE])
    }
}

/// Resolve the transition tables through an Array-subclass receiver after its
/// first learned edge. `RuntimeState` is heap allocated and stable until this
/// thread exits, and ObjectHeaders never cross agents (worker inputs are
/// deep-copied), so the native pointer can move with ObjectMeta without GC
/// tracing or rewriting. The cached pointer is still validated against the
/// CURRENT thread's tables on every use: a pump thread acting on an agent's
/// behalf (Android's UI-thread timer pump) must never reach another thread's
/// mutable tables through a cache the owning thread filled.
#[inline(always)]
fn object_hot_for_owner(
    owner: *const crate::object::ObjectHeader,
) -> &'static crate::object::ObjectHotTables {
    unsafe {
        if !owner.is_null() {
            let meta = (*owner).meta;
            if !meta.is_null() {
                let hot = &crate::state::state().object_hot;
                let cached =
                    (*meta).array_tail_object_hot as usize as *const crate::object::ObjectHotTables;
                if std::ptr::eq(cached, hot) {
                    return &*cached;
                }
                // GC_STORE_AUDIT(NATIVE_POINTER): RuntimeState storage, not a
                // managed heap edge; ObjectMeta's GC descriptors intentionally
                // visit only prototype, spill, and private brand.
                (*meta).array_tail_object_hot = hot as *const _ as usize as u64;
                return hot;
            }
        }
    }
    &crate::state::state().object_hot
}

#[inline(always)]
fn with_forward_for_owner<R>(
    owner: *const crate::object::ObjectHeader,
    f: impl FnOnce(*mut [ArrayTailTransitionEntry; ARRAY_TAIL_TRANSITION_CACHE_SIZE]) -> R,
) -> R {
    unsafe {
        let boxed = &mut *object_hot_for_owner(owner).array_tail_forward.get();
        f(boxed.as_mut_ptr() as *mut [ArrayTailTransitionEntry; ARRAY_TAIL_TRANSITION_CACHE_SIZE])
    }
}

#[inline(always)]
fn with_reverse_for_owner<R>(
    owner: *const crate::object::ObjectHeader,
    f: impl FnOnce(*mut [ArrayTailTransitionEntry; ARRAY_TAIL_TRANSITION_CACHE_SIZE]) -> R,
) -> R {
    unsafe {
        let boxed = &mut *object_hot_for_owner(owner).array_tail_reverse.get();
        f(boxed.as_mut_ptr() as *mut [ArrayTailTransitionEntry; ARRAY_TAIL_TRANSITION_CACHE_SIZE])
    }
}

#[inline(always)]
fn direct_slot(shape_id: u32) -> usize {
    shape_id as usize & ARRAY_TAIL_TRANSITION_CACHE_MASK
}

#[inline]
fn publish_direct_index(
    owner: *const crate::object::ObjectHeader,
    shape_id: u32,
    forward: Option<usize>,
    reverse: Option<usize>,
) {
    let hot = object_hot_for_owner(owner);
    let direct = unsafe { &mut *hot.array_tail_direct.get() };
    let entry = &mut direct[direct_slot(shape_id)];
    if entry.shape_id != shape_id {
        *entry = ArrayTailDirectIndex {
            shape_id,
            ..ArrayTailDirectIndex::EMPTY
        };
    }
    if let Some(index) = forward {
        debug_assert!(index < ARRAY_TAIL_TRANSITION_CACHE_SIZE);
        entry.forward = index as u16;
    }
    if let Some(index) = reverse {
        debug_assert!(index < ARRAY_TAIL_TRANSITION_CACHE_SIZE);
        entry.reverse = index as u16;
    }
}

#[inline]
fn canonical_index(key: *const crate::StringHeader) -> Option<u32> {
    unsafe {
        crate::object::has_own_helpers::str_from_string_header(key)
            .and_then(crate::object::canonical_array_index)
    }
}

#[inline]
fn descriptor_pair_is_exact(entry: ArrayTailTransitionEntry) -> bool {
    let Some(predecessor) = shapes::shape_descriptor_by_id(entry.predecessor_shape_id) else {
        return false;
    };
    let Some(successor) = shapes::shape_descriptor_by_id(entry.successor_shape_id) else {
        return false;
    };
    predecessor.keys as usize == entry.predecessor_keys
        && successor.keys as usize == entry.successor_keys
        && predecessor.object_kind == successor.object_kind
        && predecessor.logical_key_count == entry.slot
        && successor.logical_key_count == entry.slot.saturating_add(1)
        && entry.predecessor_keys >= crate::gc::GC_HEADER_SIZE
        && entry.successor_keys >= crate::gc::GC_HEADER_SIZE
        && unsafe {
            let predecessor_header = (entry.predecessor_keys as *const u8)
                .sub(crate::gc::GC_HEADER_SIZE)
                .cast::<crate::gc::GcHeader>();
            let successor_header = (entry.successor_keys as *const u8)
                .sub(crate::gc::GC_HEADER_SIZE)
                .cast::<crate::gc::GcHeader>();
            (*predecessor_header).obj_type == crate::gc::GC_TYPE_ARRAY
                && (*successor_header).obj_type == crate::gc::GC_TYPE_ARRAY
        }
}

pub(crate) fn record_numeric_tail_transition(
    owner: *const crate::object::ObjectHeader,
    predecessor_shape_id: u32,
    successor_shape_id: u32,
    key: *const crate::StringHeader,
    successor_keys: usize,
    slot: u32,
) {
    let Some(array_index) = canonical_index(key) else {
        return;
    };
    if owner.is_null()
        || !crate::array::array_subclass_tail_descriptors_are_plain(owner, array_index)
    {
        return;
    }
    let Some(predecessor) = shapes::shape_descriptor_by_id(predecessor_shape_id) else {
        return;
    };
    let Some(successor) = shapes::shape_descriptor_by_id(successor_shape_id) else {
        return;
    };
    let entry = ArrayTailTransitionEntry {
        predecessor_keys: predecessor.keys as usize,
        successor_keys,
        predecessor_shape_id,
        successor_shape_id,
        slot,
        array_index,
        predecessor_live_inline_slots: predecessor.live_inline_slot_count,
        successor_live_inline_slots: successor.live_inline_slot_count,
    };
    if successor.keys as usize != successor_keys
        || predecessor.logical_key_count != slot
        || successor.logical_key_count != slot.saturating_add(1)
        || predecessor.object_kind != successor.object_kind
        || !descriptor_pair_is_exact(entry)
    {
        return;
    }
    // A single lost edge poisons the remainder of a shrinking dense shape
    // chain: the generic fallback mints a different predecessor, after which
    // no historical edge can match. Preserve colliding entries with bounded
    // open addressing instead of overwriting one direct-mapped slot.
    let forward = with_forward_for_owner(owner, |table| unsafe { insert_forward(table, entry) });
    let reverse = with_reverse_for_owner(owner, |table| unsafe { insert_reverse(table, entry) });
    if forward.is_none() && reverse.is_none() {
        // Nothing references the pair: it must not claim cache ownership.
        return;
    }
    // Both descriptors are now named by a live entry. The bit is recomputed
    // from table occupancy after every full trace
    // (`recompute_cache_carriers_after_full_trace`), so an entry that is later
    // evicted or tombstoned releases its descriptors at the next full trace.
    unsafe {
        shapes::note_cache_carrier(Some(predecessor));
        shapes::note_cache_carrier(Some(successor));
    }
    if let Some(index) = forward {
        publish_direct_index(owner, predecessor_shape_id, Some(index), None);
    }
    if let Some(index) = reverse {
        publish_direct_index(owner, successor_shape_id, None, Some(index));
    }
}

#[inline]
pub(crate) fn lookup_forward_for_owner(
    owner: *const crate::object::ObjectHeader,
    predecessor_shape_id: u32,
    array_index: u32,
) -> Option<ArrayTailTransitionEntry> {
    let hot = object_hot_for_owner(owner);
    let direct = unsafe { &*hot.array_tail_direct.get() };
    let cached = direct[direct_slot(predecessor_shape_id)];
    let table = unsafe { &mut *hot.array_tail_forward.get() };
    if cached.shape_id == predecessor_shape_id && cached.forward != ARRAY_TAIL_DIRECT_INDEX_MISS {
        let entry = table[cached.forward as usize];
        if entry.predecessor_shape_id == predecessor_shape_id
            && entry.array_index == array_index
            && entry.successor_shape_id != 0
        {
            return Some(entry);
        }
    }
    let start = forward_slot(predecessor_shape_id, array_index);
    for offset in 0..ARRAY_TAIL_TRANSITION_CACHE_SIZE {
        let entry = table[(start + offset) & ARRAY_TAIL_TRANSITION_CACHE_MASK];
        if entry.is_empty() {
            return None;
        }
        if entry.predecessor_shape_id == predecessor_shape_id && entry.array_index == array_index {
            return Some(entry);
        }
    }
    None
}

#[cfg(test)]
#[inline]
pub(crate) fn lookup_reverse(successor_shape_id: u32) -> Option<ArrayTailTransitionEntry> {
    with_reverse(|table| unsafe {
        let start = reverse_slot(successor_shape_id);
        for offset in 0..ARRAY_TAIL_TRANSITION_CACHE_SIZE {
            let entry = (*table)[(start + offset) & ARRAY_TAIL_TRANSITION_CACHE_MASK];
            if entry.is_empty() {
                return None;
            }
            if entry.successor_shape_id == successor_shape_id {
                return Some(entry);
            }
        }
        None
    })
}

#[inline]
pub(crate) fn lookup_reverse_for_owner(
    owner: *const crate::object::ObjectHeader,
    successor_shape_id: u32,
) -> Option<ArrayTailTransitionEntry> {
    let hot = object_hot_for_owner(owner);
    let direct = unsafe { &*hot.array_tail_direct.get() };
    let cached = direct[direct_slot(successor_shape_id)];
    let table = unsafe { &mut *hot.array_tail_reverse.get() };
    if cached.shape_id == successor_shape_id && cached.reverse != ARRAY_TAIL_DIRECT_INDEX_MISS {
        let entry = table[cached.reverse as usize];
        if entry.successor_shape_id == successor_shape_id {
            return Some(entry);
        }
    }
    let start = reverse_slot(successor_shape_id);
    for offset in 0..ARRAY_TAIL_TRANSITION_CACHE_SIZE {
        let entry = table[(start + offset) & ARRAY_TAIL_TRANSITION_CACHE_MASK];
        if entry.is_empty() {
            return None;
        }
        if entry.successor_shape_id == successor_shape_id {
            return Some(entry);
        }
    }
    None
}

unsafe fn scan_table(
    table: *mut [ArrayTailTransitionEntry; ARRAY_TAIL_TRANSITION_CACHE_SIZE],
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
) {
    for index in 0..ARRAY_TAIL_TRANSITION_CACHE_SIZE {
        let entry = &mut (*table)[index];
        if entry.successor_shape_id != 0 {
            visitor.visit_usize_slot(&mut entry.predecessor_keys);
            visitor.visit_usize_slot(&mut entry.successor_keys);
        }
    }
}

pub(crate) fn scan_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    with_forward(|table| unsafe { scan_table(table, visitor) });
    with_reverse(|table| unsafe { scan_table(table, visitor) });
}

unsafe fn prune_table(table: *mut [ArrayTailTransitionEntry; ARRAY_TAIL_TRANSITION_CACHE_SIZE]) {
    for index in 0..ARRAY_TAIL_TRANSITION_CACHE_SIZE {
        let entry = &mut (*table)[index];
        if entry.successor_shape_id != 0 && !descriptor_pair_is_exact(*entry) {
            *entry = ArrayTailTransitionEntry::TOMBSTONE;
        }
    }
}

/// Rebuild the `cache_carrier` gate from live cache occupancy.
///
/// `note_cache_carrier` marks a descriptor when an entry naming it is inserted;
/// eviction and tombstoning do not unmark it, because several entries may name
/// the same descriptor. A full trace is the one point where the exact answer is
/// cheap: clear every bit, then re-note the descriptors of every live entry in
/// both tables. Between full traces the bit set is therefore a superset of the
/// live occupancy — never a subset — which is the direction the rooting gate in
/// `scan_shape_table_rekey_mut` requires. Mirrors
/// `rotate_old_carrier_epoch_after_full_trace` for the other carrier class.
pub(crate) fn recompute_cache_carriers_after_full_trace() {
    shapes::clear_all_cache_carriers();
    with_forward(|table| unsafe { note_live_entries(table) });
    with_reverse(|table| unsafe { note_live_entries(table) });
}

unsafe fn note_live_entries(
    table: *mut [ArrayTailTransitionEntry; ARRAY_TAIL_TRANSITION_CACHE_SIZE],
) {
    for entry in (*table).iter() {
        // `EMPTY` and `TOMBSTONE` both carry a zero successor ShapeId.
        if entry.successor_shape_id == 0 {
            continue;
        }
        shapes::note_cache_carrier(shapes::shape_descriptor_by_id(entry.predecessor_shape_id));
        shapes::note_cache_carrier(shapes::shape_descriptor_by_id(entry.successor_shape_id));
    }
}

#[cold]
pub(crate) fn prune_invalid_entries() {
    with_forward(|table| unsafe { prune_table(table) });
    with_reverse(|table| unsafe { prune_table(table) });
}

#[cfg(test)]
pub(crate) fn test_clear() {
    with_forward(|table| unsafe {
        for entry in (*table).iter_mut() {
            *entry = ArrayTailTransitionEntry::EMPTY;
        }
    });
    with_reverse(|table| unsafe {
        for entry in (*table).iter_mut() {
            *entry = ArrayTailTransitionEntry::EMPTY;
        }
    });
}
