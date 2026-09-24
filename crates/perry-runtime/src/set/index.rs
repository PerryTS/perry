//! Owner-local Set index. Buckets contain only a hash and a raw element index;
//! equality always reads the authoritative (GC-rewritten) elements buffer.
//! Strings are never rehashed merely because they move. Pointer keys get a
//! lazy weak identity token, shared across all indexed Sets in this thread.
//! Only inserting into an index allocates a token; a probe for a key without
//! one is a definite miss.

use super::*;
use std::cell::Cell;
use std::hash::{BuildHasher, Hash, Hasher};

type IdentityMap = crate::fast_hash::PtrHashMap<usize, u64>;

/// The weak address -> identity-token table, split by the KEY's generation
/// so a minor never walks old keys (#11169 follow-up). The token is a scalar,
/// so only the key's generation matters: `young` holds keys a minor can move
/// or reclaim (`addr_is_minor_collectible`), `old` the rest. A key only ever
/// goes young -> old; a young walk migrates promoted keys. A lookup probes
/// both halves, because an in-place promotion retags a key without a walk.
struct SetKeyIdentities {
    young: IdentityMap,
    old: IdentityMap,
    /// Reused rekey buffer: `(new_addr, id, still_young)`.
    moved: Vec<(usize, u64, bool)>,
}

impl SetKeyIdentities {
    fn get(&self, addr: usize) -> Option<u64> {
        self.young
            .get(&addr)
            .or_else(|| self.old.get(&addr))
            .copied()
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.young.len() + self.old.len()
    }
}

crate::perry_thread_local! {
    static SET_KEY_IDENTITIES: RefCell<SetKeyIdentities> = RefCell::new(SetKeyIdentities {
        young: crate::fast_hash::new_ptr_hash_map(),
        old: crate::fast_hash::new_ptr_hash_map(),
        moved: Vec::new(),
    });
    static NEXT_SET_KEY_IDENTITY: Cell<u64> = const { Cell::new(1) };
}

const IDENTITY_WALK_NAME: &str = "set.key_identities";

/// Rekeys `SET_KEY_IDENTITIES` after a relocation, never marking the keys
/// (the table is weak; see `prune_dead_identity_owners`). A minor-scoped pass
/// walks only the young half; a full pass walks both. Either way an entry
/// whose (new) key is no longer minor-collectible ends up in the old half.
pub(crate) fn scan_identity_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    use crate::gc::young_log::{addr_is_minor_collectible, note_walk, YoungLogWalk};
    let young_scope = visitor.young_scope();
    let (visited, kept, table_len) = SET_KEY_IDENTITIES.with(|table| {
        let mut table = table.borrow_mut();
        let SetKeyIdentities { young, old, moved } = &mut *table;
        #[cfg(any(debug_assertions, test))]
        if young_scope {
            debug_assert_old_identity_keys_are_old(old);
        }
        moved.clear();
        let mut visited = young.len();
        young.retain(|&addr, &mut id| {
            let mut new = addr;
            let rekeyed = visitor.visit_metadata_usize_slot(&mut new);
            let still_young = addr_is_minor_collectible(new);
            if rekeyed || !still_young {
                moved.push((new, id, still_young));
                false
            } else {
                true
            }
        });
        if !young_scope {
            visited += old.len();
            old.retain(|&addr, &mut id| {
                let mut new = addr;
                if visitor.visit_metadata_usize_slot(&mut new) {
                    moved.push((new, id, addr_is_minor_collectible(new)));
                    false
                } else {
                    true
                }
            });
        }
        for (addr, id, still_young) in moved.drain(..) {
            if still_young {
                young.insert(addr, id);
            } else {
                old.insert(addr, id);
            }
        }
        let table_len = young.len() + old.len();
        (visited as u64, young.len() as u64, table_len as u64)
    });
    note_walk(
        IDENTITY_WALK_NAME,
        YoungLogWalk {
            partial: young_scope,
            logged: visited,
            visited,
            kept,
            table_len,
        },
    );
}

/// The analogue of `gc/young_log.rs` rule 2: a minor skips the old half, so
/// the old half must hold no key a minor can act on.
#[cfg(any(debug_assertions, test))]
fn debug_assert_old_identity_keys_are_old(old: &IdentityMap) {
    for &addr in old.keys() {
        assert!(
            !crate::gc::young_log::addr_is_minor_collectible(addr),
            "SET_KEY_IDENTITIES: old half holds minor-collectible key {addr:#x}"
        );
    }
}

pub(crate) fn prune_dead_identity_owners(is_dead_owner: &dyn Fn(usize) -> bool) {
    SET_KEY_IDENTITIES.with(|table| {
        let mut table = table.borrow_mut();
        #[cfg(test)]
        TEST_PRUNE_VISITS.with(|count| count.set(count.get() + table.len()));
        table.young.retain(|&addr, _| !is_dead_owner(addr));
        table.old.retain(|&addr, _| !is_dead_owner(addr));
    });
}

/// [`prune_dead_identity_owners`] for a MINOR: only a young key can be dead,
/// and every key a minor can reclaim is in the young half.
pub(crate) fn prune_dead_identity_owners_young(is_dead_owner: &dyn Fn(usize) -> bool) {
    SET_KEY_IDENTITIES.with(|table| {
        let mut table = table.borrow_mut();
        #[cfg(test)]
        TEST_PRUNE_VISITS.with(|count| count.set(count.get() + table.young.len()));
        table.young.retain(|&addr, _| !is_dead_owner(addr));
    });
}

/// Hash for a value being PUT INTO an index: allocates an identity for a
/// movable key that has none yet.
fn value_hash(value: f64) -> u32 {
    hash_value(value, true).expect("inserting hash always resolves")
}

/// Hash for a PROBE (`has`/`delete`/`remove`): never allocates an identity.
/// `None` means the key is a movable object with no identity, which is a
/// definite miss — every element put into an index went through
/// [`value_hash`], and an identity is dropped only when its object dies.
fn existing_hash(value: f64) -> Option<u32> {
    hash_value(value, false)
}

fn hash_value(value: f64, insert: bool) -> Option<u32> {
    #[cfg(test)]
    TEST_HASH_CALLS.with(|count| count.set(count.get() + 1));
    let bits = value.to_bits();
    let mut hasher = crate::fast_hash::PtrHasher.build_hasher();
    if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
        bits.hash(&mut hasher);
        return Some(hasher.finish() as u32);
    }
    if is_string_like(bits) {
        let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
        if let Some((data, len)) = string_view_from_bits(bits, &mut scratch) {
            unsafe {
                std::slice::from_raw_parts(data, len as usize).hash(&mut hasher);
            }
            return Some(hasher.finish() as u32);
        }
    }
    let tag = bits >> 48;
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    if matches!(tag, 0 | 0x7FFD | 0x7FFA)
        && unsafe { crate::value::addr_class::try_read_gc_header(addr) }
            .is_some_and(|h| crate::gc::gc_type_is_movable(h.obj_type))
    {
        let id = SET_KEY_IDENTITIES.with(|table| {
            let mut table = table.borrow_mut();
            if let Some(id) = table.get(addr) {
                return Some(id);
            }
            if !insert {
                return None;
            }
            let id = NEXT_SET_KEY_IDENTITY.with(|next| {
                let id = next.get();
                next.set(id.checked_add(1).expect("Set key identity exhausted"));
                id
            });
            if crate::gc::young_log::addr_is_minor_collectible(addr) {
                table.young.insert(addr, id);
            } else {
                table.old.insert(addr, id);
            }
            Some(id)
        })?;
        (tag, id).hash(&mut hasher);
        return Some(hasher.finish() as u32);
    }
    bits.hash(&mut hasher);
    Some(hasher.finish() as u32)
}

const EMPTY: u32 = u32::MAX;
const DELETED: u32 = u32::MAX - 1;

#[derive(Clone, Copy)]
struct Bucket {
    hash: u32,
    index: u32,
}

const EMPTY_BUCKET: Bucket = Bucket {
    hash: 0,
    index: EMPTY,
};

pub(super) struct SetIndex {
    buckets: Vec<Bucket>,
    live: usize,
    occupied: usize,
}

impl SetIndex {
    fn new(size: usize) -> Self {
        Self {
            buckets: vec![EMPTY_BUCKET; (size * 2).max(16).next_power_of_two()],
            live: 0,
            occupied: 0,
        }
    }

    pub(super) fn byte_len(&self) -> usize {
        std::mem::size_of::<Self>() + self.buckets.capacity() * std::mem::size_of::<Bucket>()
    }

    fn insert_hash(&mut self, hash: u32, index: u32) {
        if (self.occupied + 1) * 4 >= self.buckets.len() * 3 {
            let capacity = if (self.live + 1) * 4 >= self.buckets.len() * 3 {
                self.buckets.len() * 2
            } else {
                self.buckets.len()
            };
            let old = std::mem::replace(&mut self.buckets, vec![EMPTY_BUCKET; capacity]);
            self.live = 0;
            self.occupied = 0;
            for bucket in old {
                if bucket.index < DELETED {
                    self.insert_hash(bucket.hash, bucket.index);
                }
            }
        }
        let mask = self.buckets.len() - 1;
        let mut pos = hash as usize & mask;
        while self.buckets[pos].index < DELETED {
            pos = (pos + 1) & mask;
        }
        if self.buckets[pos].index == EMPTY {
            self.occupied += 1;
        }
        self.buckets[pos] = Bucket { hash, index };
        self.live += 1;
    }

    unsafe fn find(&self, set: *const SetHeader, value: f64, hash: u32) -> Option<usize> {
        let mask = self.buckets.len() - 1;
        let mut pos = hash as usize & mask;
        loop {
            let bucket = self.buckets[pos];
            if bucket.index == EMPTY {
                return None;
            }
            if bucket.index < (*set).used && bucket.hash == hash {
                let candidate = *(*set).elements.add(bucket.index as usize);
                if candidate.to_bits() != SET_HOLE_VALUE_BITS && jsvalue_eq(candidate, value) {
                    return Some(pos);
                }
            }
            pos = (pos + 1) & mask;
        }
    }
}

unsafe fn index_ptr(set: *const SetHeader) -> *mut SetIndex {
    let meta = (*set).meta;
    if meta.is_null() {
        ptr::null_mut()
    } else {
        (*meta).native_state as *mut SetIndex
    }
}

pub(super) unsafe fn lookup_value(set: *const SetHeader, value: f64) -> Option<i32> {
    let index = index_ptr(set).as_ref()?;
    // `Some(-1)`, not `None`: a probe key with no identity is a definite miss
    // (`None` would send the caller to a linear scan of the elements).
    let Some(hash) = existing_hash(value) else {
        return Some(-1);
    };
    Some(
        index
            .find(set, value, hash)
            .map_or(-1, |pos| index.buckets[pos].index as i32),
    )
}

pub(super) unsafe fn insert_value(set: *mut SetHeader, value: f64, raw: u32) {
    let index = index_ptr(set);
    if index.is_null() {
        if (*set).size > SMALL_SET_SCAN_MAX {
            rebuild_index(set);
        }
    } else {
        let before = (*index).byte_len();
        (*index).insert_hash(value_hash(value), raw);
        let added = (*index).byte_len() - before;
        if added != 0 {
            crate::gc::gc_note_external_side_alloc(added);
        }
    }
}

pub(super) unsafe fn rebuild_index(set: *mut SetHeader) {
    if (*set).size <= SMALL_SET_SCAN_MAX {
        clear_index(set);
        return;
    }
    let mut index = Box::new(SetIndex::new((*set).size as usize));
    for i in 0..(*set).used {
        let value = *(*set).elements.add(i as usize);
        if value.to_bits() != SET_HOLE_VALUE_BITS {
            index.insert_hash(value_hash(value), i);
        }
    }
    // This bounded metadata allocation must not collect while `set` is a raw
    // pointer. No user code runs here; normal allocation pacing resumes below.
    let meta = {
        let _no_gc = crate::gc::GcSuppressScope::new();
        crate::object::object_meta_ensure_for_cell(set as usize).expect("Set has a meta edge")
    };
    let bytes = index.byte_len();
    let index_ptr = &mut *index as *mut SetIndex;
    let old = SET_REGISTRY.with(|registry| {
        registry
            .borrow_mut()
            .get_mut(&(set as usize))
            .expect("registered Set")
            .index
            .replace(index)
    });
    let old_bytes = old.as_ref().map_or(0, |index| index.byte_len());
    drop(old);
    // Native allocation, owned by SET_REGISTRY, not a managed-heap edge.
    (*meta).native_state = index_ptr as u64;
    crate::gc::gc_note_external_side_free(old_bytes);
    crate::gc::gc_note_external_side_alloc(bytes);
}

pub(super) unsafe fn remove_value(set: *mut SetHeader, value: f64, raw: u32) {
    let Some(index) = index_ptr(set).as_mut() else {
        return;
    };
    // The authoritative element was already tombstoned, so match its raw index.
    // An indexed element always has its identity; should one be missing, fall
    // back to a full bucket scan rather than leave a live bucket behind.
    let mask = index.buckets.len() - 1;
    let pos = match existing_hash(value) {
        Some(hash) => {
            let mut pos = hash as usize & mask;
            while index.buckets[pos].index != EMPTY && index.buckets[pos].index != raw {
                pos = (pos + 1) & mask;
            }
            Some(pos).filter(|&pos| index.buckets[pos].index == raw)
        }
        None => index.buckets.iter().position(|bucket| bucket.index == raw),
    };
    if let Some(pos) = pos {
        index.buckets[pos].index = DELETED;
        index.live -= 1;
    }
    if (*set).size <= SMALL_SET_SCAN_MAX {
        clear_index(set);
    }
}

pub(super) unsafe fn clear_index(set: *mut SetHeader) {
    if index_ptr(set).is_null() {
        return;
    }
    (*(*set).meta).native_state = 0;
    let old = SET_REGISTRY.with(|registry| {
        registry
            .borrow_mut()
            .get_mut(&(set as usize))
            .and_then(|allocation| allocation.index.take())
    });
    if let Some(index) = old {
        crate::gc::gc_note_external_side_free(index.byte_len());
    }
}

#[cfg(test)]
crate::perry_thread_local! {
    static TEST_HASH_CALLS: Cell<usize> = const { Cell::new(0) };
    static TEST_PRUNE_VISITS: Cell<usize> = const { Cell::new(0) };
}

/// Identity entries examined by the dead-key prunes so far on this thread.
#[cfg(test)]
pub(crate) fn test_identity_prune_visits() -> usize {
    TEST_PRUNE_VISITS.with(Cell::get)
}

#[cfg(test)]
pub(crate) unsafe fn test_snapshot(set: *const SetHeader) -> (usize, usize, usize) {
    let index = index_ptr(set);
    (
        index as usize,
        index.as_ref().map_or(0, |i| i.byte_len()),
        TEST_HASH_CALLS.with(Cell::get),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_mixed_sets_are_lazy_and_rebuild_after_shrink() {
        let set = js_set_alloc(16);
        let string = crate::string::js_string_from_bytes(b"hello".as_ptr(), 5);
        let values = [
            0.0,
            f64::NAN,
            2.0,
            3.0,
            f64::from_bits(crate::value::TAG_TRUE),
            f64::from_bits(crate::value::TAG_NULL),
            f64::from_bits(crate::value::TAG_UNDEFINED),
            boxed_heap_string_value(string),
        ];
        for value in values {
            js_set_add(set, value);
        }
        unsafe {
            assert!(
                (*set).meta.is_null(),
                "eight mixed values need no metadata or index"
            );
            for value in values {
                assert_eq!(js_set_has(set, value), 1);
            }
            js_set_add(set, 9.0);
            assert!(!index_ptr(set).is_null());
            let equivalent = crate::string::js_string_from_bytes(b"hello".as_ptr(), 5);
            assert_eq!(js_set_has_string(set, equivalent), 1);
            js_set_add_string(set, equivalent);
            assert_eq!((*set).size, 9);
            assert_eq!(js_set_delete(set, 9.0), 1);
            assert!(index_ptr(set).is_null());
            for value in values {
                assert_eq!(js_set_has(set, value), 1);
            }
            js_set_add(set, 10.0);
            assert!(!index_ptr(set).is_null());
            js_set_clear(set);
            assert!(index_ptr(set).is_null());
            js_set_add(set, 11.0);
            assert!(index_ptr(set).is_null());
            finalize_set_side_allocation_for_gc(set);
        }
    }

    #[test]
    fn hash_collisions_use_live_values_and_tombstones_preserve_probe_chains() {
        let set = js_set_alloc(16);
        for i in 0..12 {
            js_set_add(set, i as f64);
        }
        unsafe {
            let mut index = SetIndex::new(12);
            for i in 0..12 {
                index.insert_hash(7, i);
            }
            for i in 0..12 {
                assert!(index.find(set, i as f64, 7).is_some());
            }
            let pos = index.find(set, 3.0, 7).unwrap();
            index.buckets[pos].index = DELETED;
            index.live -= 1;
            assert!(index.find(set, 3.0, 7).is_none());
            assert!(index.find(set, 11.0, 7).is_some());
            assert!(index.find(set, 99.0, 7).is_none());
            index.insert_hash(7, 3);
            assert!(index.find(set, 3.0, 7).is_some());
            finalize_set_side_allocation_for_gc(set);
        }
    }

    #[test]
    fn churn_matches_ordered_membership_across_growth_and_compaction() {
        let set = js_set_alloc(4);
        let mut expected = Vec::new();
        let mut seed = 12345u32;
        for step in 0..6000 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let value = ((seed >> 16) % 128) as f64;
            if step % 97 == 0 {
                js_set_clear(set);
                expected.clear();
            } else if seed & 3 == 0 {
                let present = expected.contains(&value);
                assert_eq!(js_set_delete(set, value), i32::from(present));
                expected.retain(|v| *v != value);
            } else {
                js_set_add(set, value);
                if !expected.contains(&value) {
                    expected.push(value);
                }
            }
            assert_eq!(js_set_size(set) as usize, expected.len());
            for i in 0..128 {
                assert_eq!(
                    js_set_has(set, i as f64),
                    i32::from(expected.contains(&(i as f64)))
                );
            }
            for (i, &v) in expected.iter().enumerate() {
                assert_eq!(js_set_value_at(set, i as u32), v);
            }
        }
        unsafe {
            finalize_set_side_allocation_for_gc(set);
        }
    }
}

#[cfg(test)]
pub(crate) fn test_identity_count() -> usize {
    SET_KEY_IDENTITIES.with(|table| table.borrow().len())
}

/// `(young, old)` sizes of the identity table's two halves.
#[cfg(test)]
pub(crate) fn test_identity_halves() -> (usize, usize) {
    SET_KEY_IDENTITIES.with(|table| {
        let table = table.borrow();
        (table.young.len(), table.old.len())
    })
}
