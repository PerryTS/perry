//! CopyDataProperties admission from the existing shape authority.
use super::{ShapeObjectKind, ShapeRecordRef};

impl ShapeRecordRef {
    /// An immutable all-default ordinary key list answers identity without
    /// key conversion or a second descriptor/layout probe. Content misses
    /// still use the common own-position lookup.
    #[inline]
    pub(crate) unsafe fn plain_own_identity_slot(self, key: u64) -> Option<u32> {
        let r = &*self.0.as_ptr();
        if !r.proves_no_symbols() || r.hole_count != 0 || r.logical_key_count == 0 {
            return None;
        }
        let slots = crate::array::array_elements_ptr(r.keys as usize as *const crate::ArrayHeader);
        (0..r.logical_key_count.min(r.live_inline_slot_count))
            .find(|&i| *slots.add(i as usize) == key)
            .map(|position| {
                crate::object::key_attrs::key_storage_slot(
                    r.keys as usize as *const crate::ArrayHeader,
                    position,
                )
            })
    }

    /// These slots are exactly the receiver's own enumerable data properties.
    /// Symbol absence also proves an immutable shape-owned key prefix. A
    /// dictionary, native view, proxy or accessor can never borrow this proof.
    #[inline]
    pub(crate) fn spread_data(self) -> bool {
        let r = unsafe { &*self.0.as_ptr() };
        r.kind_is(ShapeObjectKind::Ordinary)
            && r.hole_count == 0
            && r.summary()
                & (crate::object::key_attrs::SUMMARY_ACCESSOR
                    | crate::object::key_attrs::SUMMARY_NON_ENUMERABLE
                    | crate::object::key_attrs::SUMMARY_PRIVATE
                    | crate::object::key_attrs::SUMMARY_HOLES)
                == 0
            && r.proves_no_symbols()
    }

    /// E4 same-layout spread source: an ordinary default-prototype record
    /// with no holes, no attribute summary, no symbols, no integrity level (a
    /// fresh copy is extensible) and no runtime-internal key name (the birth
    /// facts cover exactly the record's logical keys,
    /// the keys the copy carries on this layout word), so a result born on
    /// this very layout word owns exactly the keys the source's enumeration
    /// lists.
    #[inline]
    pub(crate) fn spread_same_layout(self) -> bool {
        let r = unsafe { &*self.0.as_ptr() };
        r.kind_is(ShapeObjectKind::Ordinary)
            && r.hole_count == 0
            && r.summary() == 0
            && r.proves_no_symbols()
            && r.proto_id == super::PROTO_ID_DEFAULT
            && r.integrity_flags() == 0
            && self.enum_bits() & crate::object::key_attrs::ENUM_INTERNAL_KEY == 0
    }

    /// E4 table-born spread source: ordinary, any prototype, holes allowed
    /// (skipped per slot), otherwise only plain enumerable string data keys
    /// with no index or internal key (layout order is then key order).
    #[inline]
    pub(crate) fn spread_plain_slots(self) -> bool {
        let r = unsafe { &*self.0.as_ptr() };
        r.kind_is(ShapeObjectKind::Ordinary)
            && r.summary() & !crate::object::key_attrs::SUMMARY_HOLES == 0
            && r.proves_no_symbols()
            && self.enum_bits()
                & (crate::object::key_attrs::ENUM_INDEX_KEY
                    | crate::object::key_attrs::ENUM_INTERNAL_KEY)
                == 0
    }
}
