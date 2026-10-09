//! Immutable special-slot identities and derived brand summaries.
use super::*;

impl ShapeRecord {
    /// Install a SPECIAL rep with its exact ConstFn body identities. A `11`
    /// lane absent from `infos` is reserved for optional NoPointer. The old
    /// `with_rep` keeps rejecting `11`, so no legacy mint silently omits the
    /// identity fact. Called only after the interner misses.
    #[inline(always)]
    pub(in crate::object) fn with_special_facts(
        mut self,
        rep: u64,
        infos: &[ConstFnSlotInfo],
        brands: &[u64],
    ) -> ShapeRecord {
        assert_eq!(self.extras, 0, "special facts replace a fresh record only");
        let mask = constfn_mask(infos).expect("invalid ConstFn slot list");
        assert!(crate::object::field_rep::is_valid_with_special(rep, mask));
        assert_eq!(mask & !crate::object::field_rep::special_lane_slots(rep), 0);
        assert!(
            brands_are_sorted(brands),
            "a brand list is sorted and unique"
        );
        self.rep = rep;
        self.special_constfn_mask = mask;
        // Derive the exact brand once, while publishing immutable facts.
        // Seven bits suffice for learned width (capped at 64), leaving the
        // high bit for WeakSet without growing the 64-byte record.
        if brands
            .binary_search(&u64::from(crate::weakref::CLASS_ID_WEAKMAP))
            .is_ok()
        {
            self.flags_and_kind |= RECORD_WEAK_COLLECTION;
        } else if brands
            .binary_search(&u64::from(crate::weakref::CLASS_ID_WEAKSET))
            .is_ok()
        {
            self.flags_and_kind |= RECORD_WEAK_COLLECTION | RECORD_WEAK_SET;
        }
        if !infos.is_empty() || !brands.is_empty() {
            self.extras = new_extras(infos, brands);
        }
        self
    }

    /// The exact brand derived from the immutable brand list at mint time.
    /// Dispatch reads only the record header, never extras or a brand search.
    #[inline]
    pub(crate) fn weak_collection_brand(&self) -> Option<u32> {
        if self.flags_and_kind & RECORD_WEAK_COLLECTION == 0 {
            return None;
        }
        Some(if self.flags_and_kind & RECORD_WEAK_SET == 0 {
            crate::weakref::CLASS_ID_WEAKMAP
        } else {
            crate::weakref::CLASS_ID_WEAKSET
        })
    }
}
