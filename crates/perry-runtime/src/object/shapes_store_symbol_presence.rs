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
        let absent = self.kind_is(ShapeObjectKind::Ordinary)
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
    /// See [`RECORD_KEYS_NO_FUNCTION_METHODS`].
    #[inline(always)]
    pub(crate) fn proves_no_function_method_keys(&self) -> bool {
        self.keys_node_and_element_kind & RECORD_KEYS_NO_FUNCTION_METHODS != 0
    }

    pub(super) fn refresh_function_method_absence(&mut self) {
        let absent = (self.logical_key_count == 0 && self.keys == 0)
            || unsafe {
                let keys = self.keys as usize as *const crate::ArrayHeader;
                if self.keys == 0 || !super::super::keys_prefix_is_immutable(keys) {
                    false
                } else {
                    let (slots, available) = crate::object::keys_array_dense_slots(keys);
                    available >= self.logical_key_count as usize
                        && (0..self.logical_key_count as usize).all(|i| {
                            let v = crate::JSValue::from_bits((*slots.add(i)).to_bits());
                            if !v.is_any_string() {
                                return true;
                            }
                            let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                            match crate::string::js_string_key_bytes(v, &mut scratch) {
                                Some(b) => !matches!(b, b"call" | b"apply" | b"bind"),
                                None => false,
                            }
                        })
                }
            };
        self.keys_node_and_element_kind = (self.keys_node_and_element_kind
            & !RECORD_KEYS_NO_FUNCTION_METHODS)
            | if absent {
                RECORD_KEYS_NO_FUNCTION_METHODS
            } else {
                0
            };
    }

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
            (self.keys_node_and_element_kind & RECORD_KEY_FACTS_MASK) | stored;
    }

    /// This record carrying `node` as its key-list node.
    #[inline]
    pub(crate) fn with_keys_node(mut self, node: u32) -> ShapeRecord {
        self.set_keys_node(node);
        self
    }
}

/// Bit 7 of the packed field's low byte: an exact absence proof that this
/// record's immutable key prefix names none of `%Function.prototype%`'s
/// `call`, `apply` and `bind`. Derived once at slab publication from the
/// key list, as `RECORD_KEYS_NO_SYMBOLS`; a key append, delete or rebuild
/// publishes a new record with its own fact. Clear = not proven (an owned
/// mutable list, a key whose bytes are not read here).
const RECORD_KEYS_NO_FUNCTION_METHODS: u32 = 0x80;
const RECORD_ELEMENT_KIND_MASK: u32 = 0x03;
/// The key-prefix facts byte: the element kind (bits 0-1) and the E2
/// enumeration bits (bits 2-4).
const RECORD_KEY_FACTS_MASK: u32 = 0xFF;
const RECORD_ENUM_SHIFT: u32 = 2;
const RECORD_ENUM_MASK: u32 = 0x7 << RECORD_ENUM_SHIFT;

impl ShapeRecord {
    /// E2: the enumeration bits (`key_attrs::ENUM_*`) of this record's key
    /// prefix, derived once when the record is published, like its symbol
    /// presence. A list that is not an immutable prefix proves nothing.
    pub(super) fn refresh_enum_facts(&mut self) {
        use crate::object::key_attrs as ka;
        let bits = if self.logical_key_count == 0 {
            0
        } else if self.keys == 0 {
            ka::ENUM_BITS
        } else {
            unsafe {
                let keys = self.keys as usize as *const crate::ArrayHeader;
                if !super::super::keys_prefix_is_immutable(keys) {
                    ka::ENUM_BITS
                } else {
                    let (slots, available) = crate::object::keys_array_dense_slots(keys);
                    if available < self.logical_key_count as usize {
                        ka::ENUM_BITS
                    } else {
                        let attrs = self.summary()
                            & (ka::SUMMARY_NON_ENUMERABLE | ka::SUMMARY_PRIVATE)
                            != 0;
                        let mut b = 0;
                        for i in 0..self.logical_key_count {
                            let entry = if attrs { ka::keys_entry(keys, i) } else { 0 };
                            b |= ka::key_enum_bits((*slots.add(i as usize)).to_bits(), entry);
                        }
                        b
                    }
                }
            }
        };
        self.keys_node_and_element_kind = (self.keys_node_and_element_kind & !RECORD_ENUM_MASK)
            | ((bits >> 8) << RECORD_ENUM_SHIFT);
    }

    /// The E2 enumeration bits, in `key_attrs::ENUM_*` positions.
    #[inline]
    pub(crate) fn enum_bits(&self) -> u32 {
        ((self.keys_node_and_element_kind & RECORD_ENUM_MASK) >> RECORD_ENUM_SHIFT) << 8
    }
}
const RECORD_KEYS_NODE_SHIFT: u32 = 8;
const _: () = assert!(
    crate::object::canonical_keys::NODE_ID_LIMIT as u64
        == (1u64 << (32 - RECORD_KEYS_NODE_SHIFT)) - 1
);
