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

    /// The same shape validator serves keyed and runtime read entries.
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

    #[inline]
    pub(super) unsafe fn read_value(&self, recv: *const ObjectHeader) -> Option<u64> {
        if !self.valid_shapes() {
            return None;
        }
        self.slot_value(recv)
            .filter(|&bits| bits != crate::value::TAG_HOLE)
    }

    pub(crate) unsafe fn scan(&mut self, visitor: &mut crate::gc::RuntimeRootVisitor<'_>) -> u64 {
        let mut rewritten = 0;
        // A key carries its NaN-box tag; a metadata address has no tag.
        if self.identity_key & !crate::value::POINTER_MASK != 0 {
            visitor.visit_nanbox_u64_slot(&mut self.identity_key);
        }
        if self.holder != 0
            && visitor.visit_tagged_usize_slot(&mut self.holder, crate::value::POINTER_TAG)
        {
            rewritten += 1;
        }
        if self.exact_chain {
            self.exact_proof().scan(visitor);
        } else if self.depth > 1 {
            for hop in std::slice::from_raw_parts_mut(self.hops, self.depth - 1) {
                if visitor.visit_tagged_usize_slot(&mut hop.0, crate::value::POINTER_TAG) {
                    rewritten += 1;
                }
            }
        }
        rewritten
    }
}

#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(std::mem::size_of::<SharedEntry>() == 56);
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
    fn shared_identity_address_is_metadata_not_a_gc_edge() {
        let identity = crate::value::TAG_NULL;
        let address = &identity as *const u64 as u64;
        assert_eq!(address & !crate::value::POINTER_MASK, 0);
        let mut entry = SharedEntry {
            identity_key: address,
            ..SharedEntry::EMPTY
        };
        let mut seen = Vec::new();
        unsafe {
            entry.scan(&mut crate::gc::RuntimeRootVisitor::for_copy(&mut |value| {
                seen.push(value.to_bits());
            }));
        }
        assert!(seen.is_empty());
        assert_eq!(entry.identity_key, address);
    }

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
