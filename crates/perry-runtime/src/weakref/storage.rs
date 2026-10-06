//! Object-owned identity storage. Buckets contain offsets, never key copies.
//! The collector treats each occupied pair as an ephemeron and rebuilds the
//! buckets after repairing keys. No mutator cache survives outside this cell.
use super::*;

const EMPTY: u32 = u32::MAX;
const DELETED: u32 = u32::MAX - 1;

#[repr(C)]
pub(crate) struct WeakStorage {
    pub(crate) capacity: u32,
    pub(crate) len: u32,
    live: u32,
    free: u32,
}

#[repr(C)]
pub(crate) struct Entry {
    pub(crate) key: u64,
    pub(crate) value: u64,
}

pub(super) enum Probe {
    Hit(u32, *mut Entry),
    Vacant(*mut u32),
}

impl WeakStorage {
    #[inline]
    pub(crate) unsafe fn entries(&self) -> *mut Entry {
        (self as *const Self as *mut Self).add(1).cast()
    }

    #[inline]
    unsafe fn buckets(&self) -> *mut u32 {
        self.entries().add(self.capacity as usize).cast()
    }

    // Occupied entries remember their bucket offset, never another key word.
    // Weak clearing and delete therefore unlink in constant work after lookup.
    #[inline]
    unsafe fn entry_buckets(&self) -> *mut u32 {
        self.buckets().add(self.capacity as usize * 2)
    }

    #[inline]
    fn hash(key: u64) -> usize {
        let mixed = key.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        (mixed ^ (mixed >> 32)) as usize
    }

    #[inline]
    pub(crate) unsafe fn find(&self, key: u64) -> Option<(u32, *mut Entry)> {
        match self.probe::<false>(key) {
            Probe::Hit(slot, entry) => Some((slot, entry)),
            Probe::Vacant(_) => None,
        }
    }

    #[inline]
    pub(super) unsafe fn insertion_probe(&self, key: u64) -> Probe {
        self.probe::<true>(key)
    }

    /// Return the validated hit or insertion bucket from this single walk.
    /// Reads do not pay for remembering a vacant bucket.
    #[inline]
    unsafe fn probe<const INSERT: bool>(&self, key: u64) -> Probe {
        if !INSERT && self.live == 0 {
            return Probe::Vacant(std::ptr::null_mut());
        }
        let mask = self.capacity as usize * 2 - 1;
        let mut bucket = Self::hash(key) & mask;
        let mut vacant = std::ptr::null_mut::<u32>();
        for _ in 0..=mask {
            let dest = self.buckets().add(bucket);
            let slot = *dest;
            if slot == EMPTY {
                return Probe::Vacant(if INSERT && vacant.is_null() {
                    dest
                } else {
                    vacant
                });
            }
            if INSERT && slot == DELETED && vacant.is_null() {
                vacant = dest;
            }
            if slot < self.len {
                let entry = self.entries().add(slot as usize);
                #[cfg(test)]
                test_support::note_weak_entry_visit();
                if (*entry).key == key {
                    return Probe::Hit(slot, entry);
                }
            }
            bucket = (bucket + 1) & mask;
        }
        // At most half the buckets are occupied, even after deletion churn.
        Probe::Vacant(vacant)
    }

    unsafe fn publish_bucket(&mut self, slot: u32) {
        let key = (*self.entries().add(slot as usize)).key;
        let mask = self.capacity as usize * 2 - 1;
        let mut bucket = Self::hash(key) & mask;
        loop {
            let dest = self.buckets().add(bucket);
            if *dest >= DELETED {
                *dest = slot;
                *self.entry_buckets().add(slot as usize) = bucket as u32;
                return;
            }
            bucket = (bucket + 1) & mask;
        }
    }

    pub(crate) unsafe fn insert(&mut self, key: u64, value: u64) {
        match self.insertion_probe(key) {
            Probe::Vacant(bucket) => self.insert_at(key, value, bucket),
            Probe::Hit(_, entry) => {
                (*entry).value = value;
                self.remember(entry);
            }
        }
    }

    pub(super) unsafe fn insert_at(&mut self, key: u64, value: u64, bucket: *mut u32) {
        debug_assert!(!bucket.is_null());
        let slot = if self.free != EMPTY {
            let slot = self.free;
            self.free = (*self.entries().add(slot as usize)).value as u32;
            slot
        } else {
            let slot = self.len;
            self.len += 1;
            slot
        };
        let entry = self.entries().add(slot as usize);
        (*entry).key = key;
        (*entry).value = value;
        self.live += 1;
        *bucket = slot;
        *self.entry_buckets().add(slot as usize) = bucket.offset_from(self.buckets()) as u32;
        self.remember(entry);
    }

    pub(crate) unsafe fn remember(&self, entry: *mut Entry) {
        crate::gc::weak_collection_store_barrier(self as *const Self as *mut Self, entry);
    }

    pub(crate) unsafe fn remove(&mut self, slot: u32) {
        let entry = self.entries().add(slot as usize);
        let bucket = *self.entry_buckets().add(slot as usize) as usize;
        debug_assert_eq!(*self.buckets().add(bucket), slot);
        *self.buckets().add(bucket) = DELETED;
        (*entry).key = TAG_UNDEFINED;
        // Empty entries have no JS edges. Their value word is a free offset.
        (*entry).value = u64::from(self.free);
        self.free = slot;
        self.live -= 1;
    }

    /// Repair has completed; reconstruct from authoritative keys in place.
    /// Also collects tombstones created by a weak pass into the free chain.
    pub(crate) unsafe fn rebuild(&mut self) {
        for i in 0..self.capacity as usize * 2 {
            *self.buckets().add(i) = EMPTY;
        }
        self.free = EMPTY;
        self.live = 0;
        for i in (0..self.len).rev() {
            let entry = self.entries().add(i as usize);
            if (*entry).key == TAG_UNDEFINED {
                (*entry).value = u64::from(self.free);
                self.free = i;
            } else {
                self.live += 1;
                self.publish_bucket(i);
            }
        }
    }

    pub(crate) fn full(&self) -> bool {
        self.live == self.capacity
    }
}

unsafe fn allocate(capacity: u32) -> *mut WeakStorage {
    let size = std::mem::size_of::<WeakStorage>() + capacity as usize * 28;
    let storage =
        crate::arena::arena_alloc_gc(size, 8, crate::gc::GC_TYPE_WEAK_STORAGE) as *mut WeakStorage;
    storage.write(WeakStorage {
        capacity,
        len: 0,
        live: 0,
        free: EMPTY,
    });
    (*storage).rebuild();
    storage
}

/// Internal brands use the reserved builtin class-id namespace, disjoint
/// from generated class ids and the high-bit private-evaluation namespace.
pub(crate) unsafe fn collection_brand(obj: *const ObjectHeader) -> Option<u32> {
    crate::object::shapes::object_shape_record(obj)?.weak_collection_brand()
}

#[inline]
pub(crate) unsafe fn owned_storage(obj: *const ObjectHeader) -> *mut WeakStorage {
    let meta = (*obj).meta;
    if meta.is_null() {
        return std::ptr::null_mut();
    }
    ((*meta).native_state & POINTER_MASK) as *mut WeakStorage
}

pub(super) fn initialize(this: f64, class: u32) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this);
    unsafe {
        let obj = js_nanbox_get_pointer(this.get_nanbox_f64()) as *mut ObjectHeader;
        crate::object::shapes::transition_object_shape_add_brand(obj, u64::from(class));
    }
    this.get_nanbox_f64()
}

/// Rooted slow path only. Allocation may move the owner and its old cell.
pub(super) unsafe fn grow(map: crate::gc::RuntimeHandle<'_>) -> *mut WeakStorage {
    let owner = js_nanbox_get_pointer(map.get_nanbox_f64()) as *mut ObjectHeader;
    crate::object::object_meta_ensure(owner);
    let old = owned_storage(js_nanbox_get_pointer(map.get_nanbox_f64()) as *mut ObjectHeader);
    let capacity = if old.is_null() {
        4
    } else {
        (*old)
            .capacity
            .checked_mul(2)
            .filter(|n| *n <= (u32::MAX - 32) / 28)
            .unwrap_or_else(|| {
                let message = b"Weak collection capacity limit exceeded";
                let string =
                    crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
                let error = crate::error::js_rangeerror_new(string);
                crate::exception::js_throw(f64::from_bits(JSValue::pointer(error.cast()).bits()))
            })
    };
    let fresh = allocate(capacity);
    let owner = js_nanbox_get_pointer(map.get_nanbox_f64()) as *mut ObjectHeader;
    let old = owned_storage(owner);
    if !old.is_null() {
        (*fresh).len = (*old).len;
        std::ptr::copy_nonoverlapping((*old).entries(), (*fresh).entries(), (*old).len as usize);
        (*fresh).rebuild();
        for i in 0..(*fresh).len {
            let entry = (*fresh).entries().add(i as usize);
            if (*entry).key != TAG_UNDEFINED {
                (*fresh).remember(entry);
            }
        }
    }
    let meta = (*owner).meta;
    let bits = JSValue::pointer(fresh.cast()).bits();
    (*meta).native_state = bits;
    crate::gc::runtime_write_barrier_slot(
        meta as usize,
        &mut (*meta).native_state as *mut u64 as usize,
        bits,
    );
    fresh
}
