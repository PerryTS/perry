use super::{new_extras, ConstFnSlotInfo, ShapeRecord};
use crate::codegen_abi::LayoutWord;

/// Optional, record-owned extension. It has no keyed lookup: a method-site
/// hit reads only the receiver's shape and current closure slot. The learned
/// masks are not shape identity, so record copies/rekeys transfer this box.
///
/// `brands` is the sorted list of private brands (#11791) every receiver of
/// the shape carries: one scalar per class evaluation whose private methods
/// or fields were installed on it (`private_brand_id`). An identity fact like
/// `constfn_infos`. Bit 62 instead names the birth evaluation holder's
/// stable serial; the holder edge is emitted through the existing identity
/// word pages (shapes_evaluation_brand), never through this scalar list.
#[derive(Debug)]
pub(in crate::object) struct ShapeExtras {
    pub(in crate::object) constfn_infos: Box<[ConstFnSlotInfo]>,
    pub(in crate::object) brands: Box<[u64]>,
    pub(in crate::object) to_nopointer: std::sync::atomic::AtomicU32,
    pub(in crate::object) to_any: std::sync::atomic::AtomicU32,
    /// Learned `to_any` words for ConstFn lanes at slot >= 32: word k holds
    /// slots 32 * (k + 1) ..; one word per 32 slots the body list reaches.
    /// Empty for every shape with no body past slot 31. The lane itself is
    /// not stored: it is the body-list entry (`lane_is_constfn`).
    pub(in crate::object) tail_to_any: Box<[std::sync::atomic::AtomicU32]>,
    /// Weak reverse key-add edge. Never shape identity or a GC carrier.
    pub(in crate::object) rollback_parent: std::sync::atomic::AtomicU32,
    /// Weak keyless receiver-birth ShapeId of this prototype's shape.
    /// Revalidated at use; no identity, width or GC edge.
    pub(in crate::object) created_birth_shape: std::sync::atomic::AtomicU32,
    /// Weak forward DefineField edge. The target owns the key and slot;
    /// this scalar owns no GC pointer and is revalidated before use.
    pub(in crate::object) field_define_target: u32,
    /// Weak integrity successors, validated against live shape facts at use.
    /// Scalar ShapeIds do not root targets and are never reused.
    pub(in crate::object) integrity_targets: [u32; 3],
    /// Exact immutable integrity identity, reached only behind its packed bit.
    pub(in crate::object) integrity_flags: u16,
}

impl ShapeRecord {
    /// Native allocations owned by this record, observed only by the heap
    /// census: `(records, bytes, store facts, store fact bytes)`. These are
    /// outside the managed heap's live-byte count. Records own no store
    /// facts (object model step 3 deleted the chain-verdict cache).
    pub(crate) fn extension_census(&self) -> (usize, usize, usize, usize) {
        if self.extras == 0 || self.extras & 1 != 0 {
            return (0, 0, 0, 0);
        }
        let extras = unsafe { &*(self.extras as usize as *const ShapeExtras) };
        let bytes = std::mem::size_of::<ShapeExtras>()
            + std::mem::size_of_val(extras.constfn_infos.as_ref())
            + std::mem::size_of_val(extras.brands.as_ref())
            + std::mem::size_of_val(extras.tail_to_any.as_ref());
        (1, bytes, 0, 0)
    }

    #[inline]
    pub(in crate::object) fn created_birth_shape(&self) -> LayoutWord {
        if !self.has_boxed_extras() {
            return LayoutWord(0);
        }
        // SAFETY: this live record owns its stable Rust extension.
        LayoutWord(
            unsafe { &*(self.extras as usize as *const ShapeExtras) }
                .created_birth_shape
                .load(std::sync::atomic::Ordering::Relaxed),
        )
    }

    /// Record a weak shape relationship on the prototype's shape. An
    /// existing extension has room for it; a plain record acquires the same
    /// extension, preserving its scalar rollback edge.
    #[cold]
    #[inline(never)]
    pub(in crate::object) fn note_created_birth_shape(&mut self, birth: LayoutWord) {
        self.note_handle_carrier(birth);
        if !self.has_boxed_extras() {
            let rollback = self.rollback_parent();
            self.extras = new_extras(&[], &[]);
            self.note_rollback_parent(rollback);
        }
        // SAFETY: the present record owns this Rust extension, not GC storage.
        unsafe { &*(self.extras as usize as *const ShapeExtras) }
            .created_birth_shape
            .store(birth.0, std::sync::atomic::Ordering::Relaxed);
    }

    /// Image run: this record as one entry of the mint record: the facts the runtime's mint
    /// takes and the state attached to the record.
    pub(in crate::object) fn snap_image_mint(&self) -> crate::snap_image::format::MintRec {
        use std::sync::atomic::Ordering::Relaxed;
        let mut edges = Vec::new();
        if self.has_boxed_extras() {
            // SAFETY: a boxed extension is an owned, aligned `ShapeExtras`.
            let x = unsafe { &*(self.extras as usize as *const super::ShapeExtras) };
            edges.push(x.to_nopointer.load(Relaxed));
            edges.push(x.to_any.load(Relaxed));
            edges.extend(x.tail_to_any.iter().map(|w| w.load(Relaxed)));
        }
        crate::snap_image::format::MintRec {
            id: self.shape_id.0,
            word: self.layout_word.0,
            keys: self.keys,
            logical: self.logical_key_count,
            live: self.live_inline_slot_count,
            holes: self.hole_count,
            generation: self.semantic_generation,
            kind: ((self.flags_and_kind & super::RECORD_KIND_MASK) >> super::RECORD_KIND_SHIFT)
                as u8,
            summary: self.summary(),
            integrity: self.integrity_flags(),
            proto_id: self.proto_id,
            proto_cell: self.proto_cell,
            rep: self.rep,
            brands: self.brands().to_vec(),
            constfn: self
                .constfn_infos()
                .iter()
                .map(|i| (i.slot, i.info))
                .collect(),
            handles: self.owned_handles().map(|w| w.0),
            family_desc: self.family_desc_index_plus_one,
            edges,
            route: crate::object::shapes::snap_image_mint_route(
                self.shape_id.0,
                self.layout_word.0,
            ),
            node: self.keys_node(),
            keys_ref: None,
            proto_ref: None,
            desc_off: None,
            fam_serial: 0,
        }
    }

    /// Image run: this record's shape state as bytes, for a begin-to-cut diff. The words
    /// the collector and the lazy probes own (carrier epochs, the facts-indexed bit, the
    /// keys word, the key probe and colour masks) and the address of a boxed extension
    /// are left out; the extension contributes its contents.
    pub(in crate::object) fn snap_image_state(&self) -> Vec<u8> {
        use std::sync::atomic::Ordering::Relaxed;
        let mut r = *self;
        r.keys = 0;
        r.probe = 0;
        r.colour_mask = [0; 2];
        r.colour_store_mask = [0; 2];
        r.flags_and_kind &= !u32::from(
            super::RECORD_FLAG_FACTS_INDEXED
                | super::RECORD_FLAG_OLD_CARRIER
                | super::RECORD_FLAG_OLD_CARRIER_SEEN
                | super::RECORD_FLAG_CARRIED_SEEN,
        );
        let boxed = self.has_boxed_extras();
        if boxed {
            r.extras = 1 << 63;
        }
        // SAFETY: a plain-data record read as bytes.
        let mut out = unsafe {
            std::slice::from_raw_parts(
                (&r as *const ShapeRecord).cast::<u8>(),
                std::mem::size_of::<ShapeRecord>(),
            )
        }
        .to_vec();
        if boxed {
            // SAFETY: a boxed extension is an owned, aligned `ShapeExtras`.
            let x = unsafe { &*(self.extras as usize as *const super::ShapeExtras) };
            let mut put = |v: u64| out.extend_from_slice(&v.to_le_bytes());
            put(u64::from(x.to_nopointer.load(Relaxed)));
            put(u64::from(x.to_any.load(Relaxed)));
            put(u64::from(x.rollback_parent.load(Relaxed)));
            put(u64::from(x.created_birth_shape.load(Relaxed)));
            put(u64::from(x.field_define_target));
            for t in x.integrity_targets {
                put(u64::from(t));
            }
            put(u64::from(x.integrity_flags));
            put(x.tail_to_any.len() as u64);
            for w in x.tail_to_any.iter() {
                put(u64::from(w.load(Relaxed)));
            }
            put(x.constfn_infos.len() as u64);
            for i in x.constfn_infos.iter() {
                put(u64::from(i.slot));
                put(i.info);
            }
            put(x.brands.len() as u64);
            for b in x.brands.iter() {
                put(*b);
            }
        }
        out
    }
}

#[cfg(test)]
mod snap_image_state_tests {
    use super::super::*;

    fn record() -> ShapeRecord {
        ShapeRecord::new(0, 0, 0, 0, ShapeObjectKind::Ordinary, 0)
    }

    #[test]
    fn snap_image_state_sees_a_change_inside_a_boxed_extension() {
        let mut r = record();
        r.extras = new_extras(&[], &[]);
        let before = r.snap_image_state();
        // SAFETY: the extension was allocated above; only its own field changes, so the
        // record words stay equal and only the extension contents can tell the states apart.
        unsafe { &mut *(r.extras as usize as *mut ShapeExtras) }.integrity_flags = 3;
        assert_ne!(before, r.snap_image_state());
        // SAFETY: the extension was allocated above and is freed once.
        drop(unsafe { Box::from_raw(r.extras as usize as *mut ShapeExtras) });
    }

    #[test]
    fn snap_image_state_ignores_the_collector_owned_bits() {
        let mut r = record();
        let before = r.snap_image_state();
        r.set(RECORD_FLAG_CARRIED_SEEN, true);
        r.set(RECORD_FLAG_OLD_CARRIER, true);
        r.probe = 5;
        assert_eq!(before, r.snap_image_state());
        r.logical_key_count += 1;
        assert_ne!(before, r.snap_image_state());
    }
}
