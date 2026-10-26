//! Scalar weak edges owned by the shape: reverse key-add and forward integrity.
//! Neither kind roots a target. Every publisher resolves the live IDs.
use super::*;

impl ShapeRecord {
    #[inline]
    pub(in super::super) fn field_define_target(&self) -> u32 {
        if !self.has_boxed_extras() {
            return 0;
        }
        unsafe { &*(self.extras as usize as *const ShapeExtras) }.field_define_target
    }

    #[cold]
    pub(in super::super) fn note_field_define_target(&mut self, target: u32) {
        self.note_handle_carrier(target);
        if !self.has_boxed_extras() {
            let parent = self.rollback_parent();
            self.extras = new_extras(&[], &[]);
            self.note_rollback_parent(parent);
        }
        unsafe { &mut *(self.extras as usize as *mut ShapeExtras) }.field_define_target = target;
    }

    /// Unrestricted records answer from their existing packed word alone.
    #[inline]
    pub(in super::super) fn integrity_flags(&self) -> u16 {
        if self.flags_and_kind & RECORD_HAS_INTEGRITY == 0 {
            return 0;
        }
        debug_assert!(self.has_boxed_extras());
        // SAFETY: publication sets the bit only after initializing this box.
        unsafe { &*(self.extras as usize as *const ShapeExtras) }.integrity_flags
    }

    /// Cold identity publication preserves the existing extension and weak edges.
    #[cold]
    pub(in super::super) fn set_integrity_flags(&mut self, flags: u16) {
        if flags == 0 {
            self.flags_and_kind &= !RECORD_HAS_INTEGRITY;
            return;
        }
        if !self.has_boxed_extras() {
            let parent = self.rollback_parent();
            self.extras = new_extras(&[], &[]);
            self.note_rollback_parent(parent);
        }
        unsafe { &mut *(self.extras as usize as *mut ShapeExtras) }.integrity_flags = flags;
        self.flags_and_kind |= RECORD_HAS_INTEGRITY;
    }

    #[inline]
    pub(super) fn has_boxed_extras(&self) -> bool {
        self.extras != 0 && self.extras & 1 == 0
    }

    pub(in super::super) fn rollback_parent(&self) -> u32 {
        if self.extras & 1 != 0 {
            return (self.extras >> 1) as u32;
        }
        if self.extras == 0 {
            return 0;
        }
        // SAFETY: this live record owns the extension.
        unsafe { &*(self.extras as usize as *const ShapeExtras) }
            .rollback_parent
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(in super::super) fn note_rollback_parent(&mut self, parent: u32) {
        self.note_handle_carrier(parent);
        if !self.has_boxed_extras() {
            self.extras = (u64::from(parent) << 1) | 1;
            return;
        }
        // SAFETY: this live record owns the extension.
        unsafe { &*(self.extras as usize as *const ShapeExtras) }
            .rollback_parent
            .store(parent, std::sync::atomic::Ordering::Relaxed);
    }
    pub(in super::super) fn integrity_target(&self, level: usize) -> u32 {
        if !self.has_boxed_extras() {
            return 0;
        }
        unsafe { &*(self.extras as usize as *const ShapeExtras) }.integrity_targets[level]
    }

    pub(in super::super) fn note_integrity_target(&mut self, level: usize, target: u32) {
        self.note_handle_carrier(target);
        if !self.has_boxed_extras() {
            let parent = self.rollback_parent();
            self.extras = new_extras(&[], &[]);
            self.note_rollback_parent(parent);
        }
        unsafe { &mut *(self.extras as usize as *mut ShapeExtras) }.integrity_targets[level] =
            target;
    }
}

/// Integrity is exact shape identity. Keep the ordinary hash unchanged.
#[inline]
pub(in super::super) fn integrity_facts_key(facts: u64, flags: u16) -> u64 {
    if flags == 0 {
        facts
    } else {
        (facts ^ (0x494e_5445_4752_0000 | u64::from(flags))).wrapping_mul(0x9e37_79b9_7f4a_7c15)
    }
}

impl ShapeRecord {
    /// A stored local handle is readable for as long as this owner survives.
    pub(in super::super) fn note_handle_carrier(&self, word: u32) {
        if self.layout_word != 0 && super::super::is_layout_word(word) {
            unsafe {
                super::super::note_cache_carrier(super::super::shape_descriptor_for_word(word));
            }
        }
    }
    pub(in super::super) fn owned_handles(&self) -> [u32; 6] {
        [
            self.rollback_parent(),
            self.created_birth_shape(),
            self.field_define_target(),
            self.integrity_target(0),
            self.integrity_target(1),
            self.integrity_target(2),
        ]
    }
}
