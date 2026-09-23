//! [`ObjectKeys`]: the one way to read a receiver's key list.

use super::keys_array_dense_slots;
use crate::array::ArrayHeader;
use crate::JSValue;

/// A receiver's ordered key list: the keys array, and how many of its entries
/// belong to this receiver.
///
/// The COUNT comes from the shape, which is the authority. The array can be a
/// canonical BACKING shared along a growth chain — `[a]`, `[a,b]` and
/// `[a,b,c]` all name one backing whose header `length` is the longest
/// list's — so an array's header length is never a receiver's key count.
/// Every consumer reads an object's keys through this view, and the view has
/// no accessor that would let it read past `count`.
///
/// For an array that is exclusively owned (a dictionary receiver's private
/// list, a builder's array before publication) the two agree, and
/// [`ObjectKeys::owned`] says so explicitly.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct ObjectKeys {
    arr: *mut ArrayHeader,
    count: u32,
}

impl ObjectKeys {
    /// No keys.
    pub(crate) const NONE: ObjectKeys = ObjectKeys {
        arr: std::ptr::null_mut(),
        count: 0,
    };

    /// The first `count` entries of `arr`.
    #[inline]
    pub(crate) fn new(arr: *mut ArrayHeader, count: u32) -> Self {
        if arr.is_null() {
            return Self::NONE;
        }
        ObjectKeys { arr, count }
    }

    /// An exclusively owned array, whose header length IS its key count.
    ///
    /// # Safety
    /// `arr` is null or a live array that no other key list shares.
    #[inline]
    pub(crate) unsafe fn owned(arr: *mut ArrayHeader) -> Self {
        if arr.is_null() {
            return Self::NONE;
        }
        ObjectKeys {
            arr,
            count: crate::array::keys_array_len_capped_to_capacity(arr) as u32,
        }
    }

    /// The keys array, for identity (a descriptor's `keys` word, a cache
    /// key) and for element reads below [`Self::count`].
    #[inline]
    pub(crate) fn arr(self) -> *mut ArrayHeader {
        self.arr
    }

    /// How many keys this receiver has — the shape's count.
    #[inline]
    pub(crate) fn count(self) -> u32 {
        self.count
    }

    #[inline]
    pub(crate) fn is_null(self) -> bool {
        self.arr.is_null()
    }

    /// The receiver's key slots: a raw element base and `min(count, physical)`.
    ///
    /// # Safety
    /// The array is live, and nothing allocates while the slots are read.
    #[inline]
    pub(crate) unsafe fn dense_slots(self) -> (*const f64, usize) {
        if self.arr.is_null() {
            return (std::ptr::null(), 0);
        }
        let (slots, len) = keys_array_dense_slots(self.arr);
        (slots, len.min(self.count as usize))
    }

    /// Key `index`, which must be below [`Self::count`].
    ///
    /// # Safety
    /// The array is live.
    #[inline]
    pub(crate) unsafe fn get(self, index: u32) -> JSValue {
        debug_assert!(index < self.count, "key {index} of {}", self.count);
        crate::array::js_array_get(self.arr, index)
    }
}
