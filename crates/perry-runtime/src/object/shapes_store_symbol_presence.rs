//! Symbol presence derived from the authoritative immutable shape-key prefix.
use super::{ShapeObjectKind, ShapeRecord, RECORD_KEYS_NO_SYMBOLS};

impl ShapeRecord {
    /// An exact absence proof for a shape-owned immutable prefix. A symbol
    /// append forks the prefix; deleting/rebuilding publishes its new facts.
    #[inline]
    pub(crate) fn proves_no_symbols(&self) -> bool {
        self.flags_and_kind & RECORD_KEYS_NO_SYMBOLS != 0
    }

    pub(super) fn refresh_symbol_presence(&mut self) {
        let absent = self.object_kind() == ShapeObjectKind::Ordinary
            && self.proto_id != super::super::PROTO_ID_PER_OBJECT
            && ((self.logical_key_count == 0 && self.keys == 0)
                || unsafe {
                    let keys = self.keys as usize as *const crate::ArrayHeader;
                    if !super::super::keys_prefix_is_immutable(keys) {
                        false
                    } else {
                        let (slots, available) = crate::object::keys_array_dense_slots(keys);
                        available >= self.logical_key_count as usize
                            && (0..self.logical_key_count as usize).all(|i| {
                                !crate::JSValue::from_bits((*slots.add(i)).to_bits()).is_pointer()
                            })
                    }
                });
        self.flags_and_kind = (self.flags_and_kind & !RECORD_KEYS_NO_SYMBOLS)
            | if absent { RECORD_KEYS_NO_SYMBOLS } else { 0 };
    }
}

impl ShapeRecord {
    pub(super) fn refresh_element_kind(&mut self) {
        self.element_kind = if self.summary() == 0 {
            0
        } else if self.keys == 0 {
            2
        } else {
            unsafe {
                let keys = self.keys as usize as *const crate::ArrayHeader;
                if !super::super::keys_prefix_is_immutable(keys) {
                    2
                } else if crate::array::named_props::keys_hold_element_descriptors(
                    keys,
                    self.logical_key_count,
                ) {
                    1
                } else {
                    0
                }
            }
        };
    }
}
