//! Owner-local Set index. Buckets contain only a hash and a raw element index;
//! equality always reads the authoritative (GC-rewritten) elements buffer.
//! Strings are never rehashed merely because they move. Pointer keys get a
//! lazy weak identity token, shared across all indexed Sets in this thread.

use super::*;
use std::cell::Cell;
use std::hash::{BuildHasher, Hash, Hasher};

crate::perry_thread_local! {
    static SET_KEY_IDENTITIES: RefCell<crate::fast_hash::PtrHashMap<usize, u64>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
    static NEXT_SET_KEY_IDENTITY: Cell<u64> = const { Cell::new(1) };
}

pub(crate) fn scan_identity_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    SET_KEY_IDENTITIES.with(|table| {
        let mut table = table.borrow_mut();
        let mut moved = Vec::new();
        table.retain(|&old, &mut id| {
            let mut new = old;
            if visitor.visit_metadata_usize_slot(&mut new) {
                moved.push((new, id));
                false
            } else {
                true
            }
        });
        table.extend(moved);
    });
}

pub(crate) fn prune_dead_identity_owners(is_dead_owner: &dyn Fn(usize) -> bool) {
    SET_KEY_IDENTITIES.with(|table| table.borrow_mut().retain(|&addr, _| !is_dead_owner(addr)));
}

fn value_hash(value: f64) -> u32 {
    #[cfg(test)]
    TEST_HASH_CALLS.with(|count| count.set(count.get() + 1));
    let bits = value.to_bits();
    let mut hasher = crate::fast_hash::PtrHasher.build_hasher();
    if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
        bits.hash(&mut hasher);
        return hasher.finish() as u32;
    }
    if is_string_like(bits) {
        let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
        if let Some((data, len)) = string_view_from_bits(bits, &mut scratch) {
            unsafe {
                std::slice::from_raw_parts(data, len as usize).hash(&mut hasher);
            }
            return hasher.finish() as u32;
        }
    }
    let tag = bits >> 48;
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    if matches!(tag, 0 | 0x7FFD | 0x7FFA) {
        if unsafe { crate::value::addr_class::try_read_gc_header(addr) }
            .is_some_and(|h| crate::gc::gc_type_is_movable(h.obj_type))
        {
            let id = SET_KEY_IDENTITIES.with(|table| {
                *table.borrow_mut().entry(addr).or_insert_with(|| {
                    NEXT_SET_KEY_IDENTITY.with(|next| {
                        let id = next.get();
                        next.set(id.checked_add(1).expect("Set key identity exhausted"));
                        id
                    })
                })
            });
            (tag, id).hash(&mut hasher);
            return hasher.finish() as u32;
        }
    }
    bits.hash(&mut hasher);
    hasher.finish() as u32
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
    Some(
        index
            .find(set, value, value_hash(value))
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
    let hash = value_hash(value);
    let mask = index.buckets.len() - 1;
    let mut pos = hash as usize & mask;
    while index.buckets[pos].index != EMPTY {
        if index.buckets[pos].index == raw {
            index.buckets[pos].index = DELETED;
            index.live -= 1;
            break;
        }
        pos = (pos + 1) & mask;
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
