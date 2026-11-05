//! Object-owned identity storage. Buckets contain offsets, never key copies.
//! The collector treats each occupied pair as an ephemeron and rebuilds the
//! buckets after repairing keys. No mutator cache survives outside this cell.
use super::*;

const EMPTY: u32 = u32::MAX;
const DELETED: u32 = u32::MAX - 1;
/// Slot hash of a key hashed by its address (token hashes are odd).
const NO_STABLE_HASH: u32 = 0;

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

    /// Per-slot stable hash (#13792): `NO_STABLE_HASH` for a key hashed by
    /// its address, else the token hash of an array key. Growth gives an
    /// array a second address, so its slot must not be found by address; the
    /// token survives growth and collection, and a rebuild reads it here, so
    /// the collector never reaches into the key's metadata.
    #[inline]
    unsafe fn slot_hashes(&self) -> *mut u32 {
        self.entry_buckets().add(self.capacity as usize)
    }

    #[inline]
    fn hash(key: u64) -> usize {
        let mixed = key.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        (mixed ^ (mixed >> 32)) as usize
    }

    /// The token hash of an array key, never `NO_STABLE_HASH`.
    #[inline]
    fn token_hash(token: u64) -> u32 {
        (Self::hash(token) as u32) | 1
    }

    /// How `key` is hashed: by address (`Some(NO_STABLE_HASH)`), or, for an
    /// array, by its identity token (`create` on insertion). `None`: an
    /// array probe with no token, which no table can hold.
    #[inline]
    unsafe fn stable_hash(key: u64, create: bool) -> Option<u32> {
        if !crate::array::is_array_value_bits(key) {
            return Some(NO_STABLE_HASH);
        }
        crate::array::array_identity_token(key, create).map(Self::token_hash)
    }

    #[inline]
    unsafe fn bucket_hash(&self, slot: u32) -> usize {
        let stable = *self.slot_hashes().add(slot as usize);
        if stable == NO_STABLE_HASH {
            Self::hash((*self.entries().add(slot as usize)).key)
        } else {
            stable as usize
        }
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

    /// Return the validated hit or insertion bucket.
    ///
    /// The walk is by address and hits on equal bits: every key that is not
    /// an array lives there, so it is the whole probe of an object key, inline
    /// as it was before arrays had a second hash. Only a pointer-key miss goes
    /// out of line to [`Self::probe_array_chain`]. Reads do not pay for
    /// remembering a vacant bucket.
    #[inline(always)]
    unsafe fn probe<const INSERT: bool>(&self, key: u64) -> Probe {
        if !INSERT && self.live == 0 {
            return Probe::Vacant(std::ptr::null_mut());
        }
        let mask = self.capacity as usize * 2 - 1;
        match self.walk::<INSERT, false>(Self::hash(key) & mask, mask, key, NO_STABLE_HASH) {
            Probe::Hit(slot, entry) => Probe::Hit(slot, entry),
            Probe::Vacant(vacant)
                if key & !crate::value::POINTER_MASK != crate::value::POINTER_TAG =>
            {
                Probe::Vacant(vacant)
            }
            Probe::Vacant(vacant) => self.probe_array_chain::<INSERT>(key, mask, vacant),
        }
    }

    /// An array key lives on its identity-token chain, where a slot holding
    /// the array under its other address (a growth stub, #13792) also
    /// matches. Any other key's address miss is final.
    #[cold]
    #[inline(never)]
    unsafe fn probe_array_chain<const INSERT: bool>(
        &self,
        key: u64,
        mask: usize,
        vacant: *mut u32,
    ) -> Probe {
        if !crate::array::is_array_value_bits(key) {
            return Probe::Vacant(vacant);
        }
        // An array never inserted anywhere has no token: a definite miss.
        let Some(stable) = crate::array::array_identity_token(key, INSERT).map(Self::token_hash)
        else {
            return Probe::Vacant(std::ptr::null_mut());
        };
        self.walk::<INSERT, true>(stable as usize & mask, mask, key, stable)
    }

    /// One linear-probe chain from `bucket`. `stable` is the array token hash
    /// the chain belongs to, or `NO_STABLE_HASH` for the address chain.
    #[inline(always)]
    unsafe fn walk<const INSERT: bool, const TOKEN: bool>(
        &self,
        mut bucket: usize,
        mask: usize,
        key: u64,
        stable: u32,
    ) -> Probe {
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
                if (*entry).key == key
                    || (TOKEN
                        && *self.slot_hashes().add(slot as usize) == stable
                        && crate::array::arrays_share_identity((*entry).key, key))
                {
                    return Probe::Hit(slot, entry);
                }
            }
            bucket = (bucket + 1) & mask;
        }
        // At most half the buckets are occupied, even after deletion churn.
        Probe::Vacant(vacant)
    }

    unsafe fn publish_bucket(&mut self, slot: u32) {
        let mask = self.capacity as usize * 2 - 1;
        let mut bucket = self.bucket_hash(slot) & mask;
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
        // The insertion probe created the token of an array key.
        *self.slot_hashes().add(slot as usize) =
            Self::stable_hash(key, false).unwrap_or(NO_STABLE_HASH);
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

/// Per capacity unit: one 16-byte entry, two u32 buckets, the entry's bucket
/// offset and its stable hash.
const BYTES_PER_SLOT: usize = 32;

unsafe fn allocate(capacity: u32) -> *mut WeakStorage {
    let size = std::mem::size_of::<WeakStorage>() + capacity as usize * BYTES_PER_SLOT;
    let storage =
        crate::arena::arena_alloc_gc(size, 8, crate::gc::GC_TYPE_WEAK_STORAGE) as *mut WeakStorage;
    // GC_STORE_AUDIT(POINTER_FREE): WeakStorage header initializer writes only four u32 values capacity,len,live,free=EMPTY, before bucket rebuild. Managed key/value entries are not part of this write.
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
#[inline]
pub(crate) unsafe fn collection_brand(obj: *const ObjectHeader) -> Option<u32> {
    crate::object::shapes::object_shape_record(obj)?.weak_collection_brand()
}

#[inline]
pub(crate) unsafe fn owned_storage(obj: *const ObjectHeader) -> *mut WeakStorage {
    // A1_UNPROVEN: record-first: WeakRef holder layout word (kind-A/B question for S0f).
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
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
    crate::object::object_meta_ensure(
        crate::meta_obj_runtime_born!(owner),
        crate::object::MetaReason::WeakStorage,
    );
    let old = owned_storage(js_nanbox_get_pointer(map.get_nanbox_f64()) as *mut ObjectHeader);
    let capacity = if old.is_null() {
        4
    } else {
        (*old)
            .capacity
            .checked_mul(2)
            .filter(|n| *n <= (u32::MAX - 32) / BYTES_PER_SLOT as u32)
            .unwrap_or_else(|| {
                let message = b"Weak collection capacity limit exceeded";
                let string =
                    crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
                let error = crate::error::CoreError::range_error(string);
                crate::exception::throw_core(error)
            })
    };
    let fresh = allocate(capacity);
    let owner = js_nanbox_get_pointer(map.get_nanbox_f64()) as *mut ObjectHeader;
    let old = owned_storage(owner);
    if !old.is_null() {
        (*fresh).len = (*old).len;
        std::ptr::copy_nonoverlapping((*old).entries(), (*fresh).entries(), (*old).len as usize);
        std::ptr::copy_nonoverlapping(
            (*old).slot_hashes(),
            (*fresh).slot_hashes(),
            (*old).len as usize,
        );
        (*fresh).rebuild();
        for i in 0..(*fresh).len {
            let entry = (*fresh).entries().add(i as usize);
            if (*entry).key != TAG_UNDEFINED {
                (*fresh).remember(entry);
            }
        }
    }
    // A1_UNPROVEN: record-first: WeakRef holder layout word (kind-A/B question for S0f).
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(owner as *mut _));
    let bits = JSValue::pointer(fresh.cast()).bits();
    (*meta).native_state = bits;
    crate::gc::runtime_write_barrier_slot(
        meta as usize,
        &mut (*meta).native_state as *mut u64 as usize,
        bits,
    );
    fresh
}
