//! One entry format for keyed, runtime and method holder proofs.
//! Fixed-name sites leave `key` zero. Each entry owns its hop block; no
//! receiver/key index or second root registry is involved.
use super::*;

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct SharedEntry {
    // These first four fields preserve the emitted method-chain ABI.
    pub(crate) hops: *mut Hop,
    pub(crate) depth: usize,
    pub(crate) holder: usize,
    pub(crate) slot: u32,
    pub(crate) holder_shape: u32,
    pub(crate) token: i64,
    // A keyed site stores its NaN-boxed key; a fixed-name site stores its
    // stable metadata identity-word address. That untagged address is not
    // a GC edge, so the same NaN-box visitor safely leaves it unchanged.
    pub(crate) identity_key: u64,
    pub(crate) class_id: u32,
    pub(crate) absent: bool,
    pub(crate) pinned_hops: bool,
    pub(crate) forward_absent: bool,
    // Method proofs include full object and class identity words. Their
    // owned block uses ShapeHop rather than the shape-only read Hop.
    pub(crate) exact_chain: bool,
    // `Obj(S, memo)`: the proof epoch (`method_site::memo_proof_epoch`) its
    // intermediate hops were recorded at; 0 never matches. Unused by method
    // chains (`exact_chain`) and by entries of depth 0 or 1, which name no
    // intermediate hop.
    pub(crate) epoch: u64,
}

impl SharedEntry {
    pub(crate) const EMPTY: Self = Self {
        hops: std::ptr::null_mut(),
        depth: 0,
        holder: 0,
        slot: 0,
        holder_shape: 0,
        token: 0,
        identity_key: 0,
        class_id: 0,
        absent: false,
        pinned_hops: false,
        forward_absent: false,
        exact_chain: false,
        epoch: 0,
    };

    pub(crate) unsafe fn method(
        links: crate::object::shape_chain::ShapeChain,
        holder: usize,
        slot: u32,
    ) -> Self {
        let (hops, depth) = links.into_raw_parts();
        Self {
            hops: hops.cast(),
            depth,
            holder,
            slot,
            exact_chain: true,
            ..Self::EMPTY
        }
    }

    #[inline]
    unsafe fn exact_proof(&self) -> std::mem::ManuallyDrop<crate::object::shape_chain::ShapeChain> {
        std::mem::ManuallyDrop::new(crate::object::shape_chain::ShapeChain::from_raw_parts(
            self.hops.cast(),
            self.depth,
        ))
    }

    #[inline]
    pub(crate) unsafe fn method_value(&self) -> Option<u64> {
        debug_assert!(self.exact_chain);
        if !self.exact_proof().valid() {
            return None;
        }
        if self.slot & HOLDER_SLOT_SPILL == 0 {
            Some(slot_bits(self.holder, self.slot))
        } else {
            crate::object::spill::spill_get_present(
                self.holder,
                (self.slot & !HOLDER_SLOT_SPILL) as usize,
            )
        }
    }

    pub(crate) unsafe fn drop_method_proof(&self) {
        debug_assert!(self.exact_chain);
        drop(crate::object::shape_chain::ShapeChain::from_raw_parts(
            self.hops.cast(),
            self.depth,
        ));
    }

    /// `Obj(S, memo)`, once the receiver's ShapeId matched the entry's token:
    /// do the intermediate hops still hold? An entry of depth 0 or 1 names
    /// none; a deeper one holds while the proof epoch it was recorded at is
    /// current (`method_site::memo_proof_epoch`). No hop ShapeId is read.
    #[inline(always)]
    pub(crate) fn hops_hold(&self) -> bool {
        self.depth <= 1 || self.epoch == super::super::memo_epoch()
    }

    /// The holder fact: the holder (the terminal object of an absent entry)
    /// still has the ShapeId the entry recorded. True for an entry naming no
    /// holder (an own key, or a null-link absence).
    #[inline(always)]
    pub(crate) unsafe fn holder_holds(&self) -> bool {
        self.holder == 0 || shape_word(self.holder) == self.holder_shape
    }

    /// The complete shape proof publication established: every hop's
    /// ShapeId and the holder's. Contract mode re-runs it in each typed body
    /// (`contracts`); the collecting re-proof uses it before it records a
    /// fresh epoch.
    #[inline]
    pub(super) unsafe fn valid_shapes(&self) -> bool {
        if self.depth == 0 {
            return true;
        }
        if self.depth > 1 {
            for &(addr, shape) in std::slice::from_raw_parts(self.hops, self.depth - 1) {
                if addr == 0 || shape_word(addr) != shape {
                    return false;
                }
            }
        }
        self.holder != 0 && shape_word(self.holder) == self.holder_shape
    }

    #[inline]
    pub(super) unsafe fn slot_value(&self, recv: *const ObjectHeader) -> Option<u64> {
        if self.absent {
            return Some(crate::value::TAG_UNDEFINED);
        }
        holder_slot_value(
            if self.holder == 0 {
                recv as usize
            } else {
                self.holder
            },
            self.slot & !(1 << 30),
        )
    }

    /// The keyed/runtime typed body (`Obj(S, memo)`), after the caller
    /// matched the receiver's token and the key: hops by the proof epoch, the
    /// holder by its ShapeId, then the slot. Any failed proof declines to
    /// the one generic entry (the site's miss), which re-walks and
    /// re-publishes.
    #[inline]
    pub(super) unsafe fn read_value(&self, recv: *const ObjectHeader) -> Option<u64> {
        if !self.hops_hold() || !self.holder_holds() || super::super::contracts::force_generic() {
            return None;
        }
        super::super::contracts::assert_holds(
            || self.valid_shapes(),
            "read_holder::SharedEntry::read_value",
            "Obj(S, memo)",
            self as *const Self as usize,
            self.token as u64,
        );
        self.slot_value(recv)
            .filter(|&bits| bits != crate::value::TAG_HOLE)
    }

    /// The objects a method proof names: its holder, then each heap hop.
    pub(crate) unsafe fn method_referents(&self) -> Vec<usize> {
        debug_assert!(self.exact_chain);
        std::iter::once(self.holder)
            .chain(self.exact_proof().heap_addrs())
            .collect()
    }
}

#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(std::mem::size_of::<SharedEntry>() == 64);
    assert!(std::mem::offset_of!(SharedEntry, hops) == 0);
    assert!(std::mem::offset_of!(SharedEntry, depth) == crate::codegen_abi::SHAPE_CHAIN_LEN_OFFSET);
    assert!(
        std::mem::offset_of!(SharedEntry, holder) == crate::codegen_abi::METHOD_CHAIN_HOLDER_OFFSET
    );
    assert!(
        std::mem::offset_of!(SharedEntry, slot) == crate::codegen_abi::METHOD_CHAIN_SLOT_OFFSET
    );
    assert!(
        std::mem::offset_of!(SharedEntry, token) == crate::codegen_abi::KEYED_HOLDER_TOKEN_OFFSET
    );
    assert!(
        std::mem::offset_of!(SharedEntry, identity_key)
            == crate::codegen_abi::KEYED_HOLDER_KEY_OFFSET
    );
    assert!(
        std::mem::offset_of!(SharedEntry, absent) == crate::codegen_abi::KEYED_HOLDER_ABSENT_OFFSET
    );
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_method_entry_preserves_undefined_slot_answer() {
        if !super::super::super::run_with_fresh_worker_gate(
            "shared_method_entry_preserves_undefined_slot_answer",
        ) {
            return;
        }
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        crate::object::builtin_prototype_value("Object");
        unsafe {
            let key = crate::string::canonical_key(b"shared_method_undefined");
            let holder = crate::object::js_object_alloc(0, 1);
            // A mixed representation retains its shape when overwritten.
            crate::object::js_object_set_field_by_name(
                holder,
                key,
                f64::from_bits(crate::value::TAG_TRUE),
            );
            crate::object::js_object_set_field_by_name(holder, key, 7.0);
            let recv =
                crate::object::js_object_create(crate::value::js_nanbox_pointer(holder as i64));
            let recv = (recv.to_bits() & crate::value::POINTER_MASK) as *const ObjectHeader;
            let links = crate::object::shape_chain::ShapeChain::capture(recv).unwrap();
            let entry = SharedEntry::method(links, holder as usize, 0);
            assert_eq!(entry.method_value(), Some(7.0f64.to_bits()));
            let shape = object_shape_stamp(holder);
            crate::object::js_object_set_field_by_name(
                holder,
                key,
                f64::from_bits(crate::value::TAG_UNDEFINED),
            );
            assert_eq!(object_shape_stamp(holder), shape, "the proof remains live");
            assert_eq!(entry.method_value(), Some(crate::value::TAG_UNDEFINED));
            entry.drop_method_proof();
        }
    }
}
