//! `SlotList` — the shape key index's per-hash slot list, in a sibling file.
//!
//! Extracted from `shapes.rs` to keep it under the repo's 2000-line cap.
//! Also carries `record_shape_scan_outcome`, the shape scanner's
//! per-descriptor bookkeeping, to keep `shapes.rs` under that cap.

/// Slots sharing one content hash.
///
/// Almost always exactly one: the key is an FNV-1a hash of distinct property
/// names, so a bucket with two entries is a genuine hash collision. Storing
/// that common case inline removes a heap allocation PER KEY from every index
/// build — and the index is rebuilt on every populated delete, so a 500-key
/// object was making ~500 `Vec` allocations per `delete`. Allocator and page
/// churn is the dominant cost on that benchmark (`clear_page_erms` 5.6%,
/// `mi_free` 4.2%, `RawVecInner::finish_grow` 2.9%), well above the lookup
/// work itself.
#[derive(Clone, Debug)]
pub(crate) enum SlotList {
    One(u32),
    Many(Vec<u32>),
}

impl SlotList {
    #[inline]
    pub(crate) fn push(&mut self, slot: u32) {
        match self {
            SlotList::One(existing) => {
                *self = SlotList::Many(vec![*existing, slot]);
            }
            SlotList::Many(v) => v.push(slot),
        }
    }

    #[inline]
    pub(crate) fn iter(&self) -> impl Iterator<Item = &u32> {
        match self {
            SlotList::One(slot) => std::slice::from_ref(slot).iter(),
            SlotList::Many(v) => v.iter(),
        }
    }
}

use super::{shape_keys_address_is_recycled, ShapeDescriptor};

/// Per-descriptor bookkeeping after its keys address has been probed.
///
/// Lifted out of `scan_shape_table_rekey_mut`'s loop so the memoised path and
/// the probing path cannot drift apart — the probe is what is deduplicated,
/// never the bookkeeping, which still runs once per descriptor.
#[inline]
pub(crate) fn record_shape_scan_outcome(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    id: &u32,
    descriptor: &mut ShapeDescriptor,
    addr: usize,
    moved: bool,
    dead_descriptor_ids: &mut Vec<u32>,
    descriptor_rekeys: &mut Vec<u32>,
) {
    // Validate the POST-visit address. A stale shape key can follow the
    // forwarding record of the non-array tenant that recycled its address;
    // checking only an unmoved old address misses that case.
    if visitor.is_metadata_rewrite_phase() && shape_keys_address_is_recycled(addr) {
        dead_descriptor_ids.push(*id);
    } else if moved {
        descriptor.keys = addr as u64;
    }
    // A live-object edge can rewrite the boxed `keys` slot before this metadata
    // pass. Comparing against the address represented in the reverse maps
    // catches both that ordering and a move observed here.
    if descriptor.keys != descriptor.indexed_keys {
        descriptor_rekeys.push(*id);
    }
}
