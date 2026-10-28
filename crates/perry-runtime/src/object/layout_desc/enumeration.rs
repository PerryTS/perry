//! Checked descriptor indices for discontiguous immutable Wasm image records.
//! Constructor registration fixes the image enumeration before JavaScript runs;
//! both directions use that order, never offsets between unrelated allocations.
use super::{PerryLayoutDesc, LAYOUT_INDEX_LIMIT};

pub(super) fn enumerated_layout_index(
    mut records: impl Iterator<Item = *mut PerryLayoutDesc>,
    desc: *const PerryLayoutDesc,
) -> Option<u32> {
    let index = records.position(|record| std::ptr::eq(record, desc))?;
    u32::try_from(index)
        .ok()
        .filter(|&index| index < LAYOUT_INDEX_LIMIT)
}

pub(super) fn enumerated_layout_at(
    mut records: impl Iterator<Item = *mut PerryLayoutDesc>,
    index: u32,
) -> Option<*const PerryLayoutDesc> {
    if index >= LAYOUT_INDEX_LIMIT {
        return None;
    }
    records
        .nth(index as usize)
        .map(|record| record.cast_const())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::runtime_layouts;

    #[test]
    fn descriptor_lookup_follows_enumeration_and_rejects_unlisted_addresses() {
        let object = std::ptr::addr_of_mut!(runtime_layouts::OBJECT_PROTOTYPE);
        let timeout = std::ptr::addr_of_mut!(runtime_layouts::TIMEOUT);
        let absent = std::ptr::addr_of_mut!(super::super::LAYOUT_SECTION_SENTINEL);
        for records in [[object, timeout], [timeout, object]] {
            for (index, desc) in records.into_iter().enumerate() {
                assert_eq!(
                    enumerated_layout_index(records.into_iter(), desc),
                    Some(index as u32)
                );
                assert_eq!(
                    enumerated_layout_at(records.into_iter(), index as u32),
                    Some(desc.cast_const())
                );
            }
            assert_eq!(enumerated_layout_index(records.into_iter(), absent), None);
            assert_eq!(enumerated_layout_at(records.into_iter(), 2), None);
            assert_eq!(
                enumerated_layout_at(records.into_iter(), LAYOUT_INDEX_LIMIT),
                None
            );
        }
    }
}
