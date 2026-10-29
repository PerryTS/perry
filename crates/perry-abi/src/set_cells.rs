//! The owner-local Set index has the same scalar representation in native
//! storage and readonly realm data. Buckets never contain managed pointers.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SetIndexBucket {
    pub hash: u32,
    pub index: u32,
}

pub const SET_INDEX_EMPTY: u32 = u32::MAX;
pub const SET_INDEX_DELETED: u32 = u32::MAX - 1;
pub const SET_INDEX_EMPTY_BUCKET: SetIndexBucket = SetIndexBucket {
    hash: 0,
    index: SET_INDEX_EMPTY,
};

/// Native instances own this bucket slice. An image instance is immutable;
/// its owning SetStore has no native Box until its first index mutation.
#[repr(C)]
pub struct SetIndexData {
    pub buckets: *mut SetIndexBucket,
    pub capacity: usize,
    pub live: usize,
    pub occupied: usize,
}

pub const fn set_string_index_hash(atom_hash: u64) -> u32 {
    (atom_hash ^ (atom_hash >> 32)) as u32
}

const _: () = {
    assert!(core::mem::size_of::<SetIndexBucket>() == 8);
    assert!(core::mem::size_of::<SetIndexData>() == 4 * core::mem::size_of::<usize>());
    assert!(core::mem::offset_of!(SetIndexData, capacity) == core::mem::size_of::<usize>());
};

/// Inline element storage. Runtime instantiates I/C with Box owners, whose
/// absent values are guaranteed null pointers; image stores own neither.
#[repr(C)]
pub struct SetStoreHeader<I, C> {
    pub _receiver_word: u64,
    pub elements: *mut f64,
    pub capacity: usize,
    pub used: usize,
    pub index: Option<I>,
    pub compaction: Option<C>,
}

use alloc::{boxed::Box, vec};
use SetIndexBucket as Bucket;
const EMPTY: u32 = SET_INDEX_EMPTY;
const DELETED: u32 = SET_INDEX_DELETED;
const EMPTY_BUCKET: Bucket = SET_INDEX_EMPTY_BUCKET;
/// The scalar index builder shared by heap births and compiler emission.
#[repr(transparent)]
pub struct OwnedSetIndex {
    data: SetIndexData,
}

impl OwnedSetIndex {
    fn with_capacity(capacity: usize) -> Self {
        Self::from_buckets(vec![EMPTY_BUCKET; capacity].into_boxed_slice(), 0, 0)
    }

    fn from_buckets(buckets: Box<[Bucket]>, live: usize, occupied: usize) -> Self {
        let capacity = buckets.len();
        Self {
            data: SetIndexData {
                buckets: Box::into_raw(buckets).cast::<Bucket>(),
                capacity,
                live,
                occupied,
            },
        }
    }

    pub fn new(size: usize) -> Self {
        Self::with_capacity((size * 2).max(16).next_power_of_two())
    }

    pub fn buckets(&self) -> &[Bucket] {
        // Native instances own a Box slice; image instances point at a
        // program-lifetime readonly slice with this same declared extent.
        unsafe { core::slice::from_raw_parts(self.data.buckets, self.data.capacity) }
    }

    fn buckets_mut(&mut self) -> &mut [Bucket] {
        // Mutable access is only obtained through an owned SetStore.index.
        unsafe { core::slice::from_raw_parts_mut(self.data.buckets, self.data.capacity) }
    }

    pub fn clone_owned(&self) -> Self {
        Self::from_buckets(self.buckets().into(), self.data.live, self.data.occupied)
    }

    pub fn byte_len(&self) -> usize {
        core::mem::size_of::<Self>() + self.data.capacity * core::mem::size_of::<Bucket>()
    }

    pub fn insert_hash(&mut self, hash: u32, index: u32) {
        if (self.data.occupied + 1) * 4 >= self.data.capacity * 3 {
            let capacity = if (self.data.live + 1) * 4 >= self.data.capacity * 3 {
                self.data.capacity * 2
            } else {
                self.data.capacity
            };
            let old = core::mem::replace(self, Self::with_capacity(capacity));
            for bucket in old.buckets() {
                if bucket.index < DELETED {
                    self.insert_hash(bucket.hash, bucket.index);
                }
            }
        }
        let mask = self.data.capacity - 1;
        let mut pos = hash as usize & mask;
        while self.buckets()[pos].index < DELETED {
            pos = (pos + 1) & mask;
        }
        if self.buckets()[pos].index == EMPTY {
            self.data.occupied += 1;
        }
        self.buckets_mut()[pos] = Bucket { hash, index };
        self.data.live += 1;
    }

    pub fn live_len(&self) -> usize {
        self.data.live
    }
    pub fn occupied_len(&self) -> usize {
        self.data.occupied
    }
    pub fn remove_bucket(&mut self, position: usize) {
        self.buckets_mut()[position].index = DELETED;
        self.data.live -= 1;
    }
}

impl Drop for OwnedSetIndex {
    fn drop(&mut self) {
        // Only owned native instances are placed in a Box/stack value and
        // dropped. Readonly image indexes are borrowed through native_state.
        unsafe {
            drop(Box::from_raw(core::ptr::slice_from_raw_parts_mut(
                self.data.buckets,
                self.data.capacity,
            )))
        }
    }
}

pub const SMALL_SET_SCAN_MAX: u32 = 8;
