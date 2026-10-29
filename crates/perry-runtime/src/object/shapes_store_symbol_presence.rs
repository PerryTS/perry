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
        let kind: u8 = if self.summary() == 0 {
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
        // The kind owns the word's low byte, so this is one byte store and
        // the node above it is untouched.
        self.keys_node_and_element_kind =
            (self.keys_node_and_element_kind & !RECORD_ELEMENT_KIND_MASK) | u32::from(kind);
    }

    /// Element properties of this key prefix (see the packed field).
    #[inline]
    pub(crate) fn element_kind(&self) -> u32 {
        self.keys_node_and_element_kind & RECORD_ELEMENT_KIND_MASK
    }

    /// The canonical trie node of this record's key list, or
    /// `canonical_keys::NO_NODE`.
    #[inline]
    pub(crate) fn keys_node(&self) -> u32 {
        if self.keys == 0 {
            // No keys array is the empty list, whose node is the trie root:
            // the keyless family lives there (CKI S2).
            return crate::object::canonical_keys::ROOT_NODE;
        }
        (self.keys_node_and_element_kind >> RECORD_KEYS_NODE_SHIFT).wrapping_sub(1)
    }

    /// Carry `node` as this record's key-list node. A node past the packed
    /// field's capacity is unreachable: the trie aborts before minting one.
    #[inline]
    pub(crate) fn set_keys_node(&mut self, node: u32) {
        let stored = if node < crate::object::canonical_keys::NODE_ID_LIMIT {
            (node + 1) << RECORD_KEYS_NODE_SHIFT
        } else {
            0
        };
        self.keys_node_and_element_kind =
            (self.keys_node_and_element_kind & RECORD_ELEMENT_KIND_MASK) | stored;
    }

    /// This record carrying `node` as its key-list node.
    #[inline]
    pub(crate) fn with_keys_node(mut self, node: u32) -> ShapeRecord {
        self.set_keys_node(node);
        self
    }
}

const RECORD_ELEMENT_KIND_MASK: u32 = 0xFF;
const RECORD_KEYS_NODE_SHIFT: u32 = 8;
const _: () = assert!(
    crate::object::canonical_keys::NODE_ID_LIMIT as u64 == (1u64 << (32 - RECORD_KEYS_NODE_SHIFT)) - 1
);
