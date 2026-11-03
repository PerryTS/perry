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
///
/// A view of a canonical list also carries the list's trie node (CKI S1),
/// learned from the shape record or the canonical handle it came from, so a
/// consumer that hands it back to the trie needs no address lookup. The node
/// is the list's identity in this agent's trie, not part of the view's
/// value: equality is `(array, count)`, and a view without a node
/// (`canonical_keys::NO_NODE`) names the same list.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ObjectKeys {
    arr: *mut ArrayHeader,
    count: u32,
    node: u32,
}

impl PartialEq for ObjectKeys {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.arr == other.arr && self.count == other.count
    }
}

impl Eq for ObjectKeys {}

impl ObjectKeys {
    /// No keys.
    pub(crate) const NONE: ObjectKeys = ObjectKeys {
        arr: std::ptr::null_mut(),
        count: 0,
        node: super::canonical_keys::NO_NODE,
    };

    /// The first `count` entries of `arr`, with no known trie node.
    #[inline]
    pub(crate) fn new(arr: *mut ArrayHeader, count: u32) -> Self {
        Self::with_node(arr, count, super::canonical_keys::NO_NODE)
    }

    /// The first `count` entries of `arr`, a list whose trie node is `node`
    /// (`NO_NODE` for an owned or unknown list).
    #[inline]
    pub(crate) fn with_node(arr: *mut ArrayHeader, count: u32, node: u32) -> Self {
        if arr.is_null() {
            return Self::NONE;
        }
        ObjectKeys { arr, count, node }
    }

    /// The trie node this view carries, or `canonical_keys::NO_NODE`.
    #[inline]
    pub(crate) fn node(self) -> u32 {
        self.node
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
            node: super::canonical_keys::NO_NODE,
            count: if super::overflow::is_table(arr) {
                (*arr).length
            } else {
                crate::array::keys_array_len_capped_to_capacity(arr) as u32
            },
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
    pub(crate) unsafe fn dense_slots(self) -> (super::keys_lookup::KeySlots, usize) {
        if self.arr.is_null() {
            return (super::keys_lookup::KeySlots::null(), 0);
        }
        let (slots, len) = keys_array_dense_slots(self.arr);
        (slots, len.min(self.count as usize))
    }

    /// Storage slot of a key position. Key iteration continues to use get().
    #[inline(always)]
    pub(crate) unsafe fn storage_slot(self, position: u32) -> u32 {
        super::key_attrs::key_storage_slot(self.arr, position)
    }

    /// Key `index`, which must be below [`Self::count`].
    ///
    /// # Safety
    /// The array is live.
    #[inline]
    pub(crate) unsafe fn get(self, index: u32) -> JSValue {
        debug_assert!(index < self.count, "key {index} of {}", self.count);
        // This list is shape storage, never a JS-facing array. Resolve the
        // backing/front once; no element getter, exotic probe or descriptor
        // lookup can affect an internal dense key slot.
        let (slots, len) = self.dense_slots();
        if index as usize >= len {
            return JSValue::undefined();
        }
        let bits = (*slots.add(index as usize)).to_bits();
        if bits == crate::value::TAG_HOLE {
            JSValue::undefined()
        } else {
            JSValue::from_bits(bits)
        }
    }
}

#[cfg(test)]
#[path = "metadata_12015_tests.rs"]
mod metadata_12015_tests;
