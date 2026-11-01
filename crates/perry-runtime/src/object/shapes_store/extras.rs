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
            + std::mem::size_of_val(extras.brands.as_ref());
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
    #[optimize(size)]
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
}
