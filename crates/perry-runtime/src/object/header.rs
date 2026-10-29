//! Object-header access API (A1, slice S0a).
//!
//! Every access to an object's meta word or slot region goes through this
//! module (and `meta_accessors`), so the header form is decided in one place.
//! Today there is one form: a 16-byte header with the meta word, slots at +16.
//! A site never derives a slot base from the type byte: positions come from a
//! layout record (`PerryLayoutKey.off`, a memo, a descriptor range),
//! which hand out absolute byte offsets.

// The readers and slot sites migrate to this API in S0b-S0e; until then part of it has no caller.
#![allow(dead_code)]

use super::{ObjectHeader, ObjectMeta};
use crate::gc::{object_has_meta_word, GcHeader, GC_HEADER_SIZE, GC_TYPE_OBJECT};

/// The `obj_type` byte of an ordinary object's `GcHeader` (the byte at `obj - 8`).
///
/// # Safety
/// `obj` is the user pointer of a live GC allocation.
#[inline(always)]
pub(crate) unsafe fn object_type(obj: *const ObjectHeader) -> u8 {
    (*(obj.cast::<u8>().sub(GC_HEADER_SIZE).cast::<GcHeader>())).obj_type
}

/// The address of the slot at byte offset `off` from the object, nothing else.
/// `off` comes from a key entry, a memo or a descriptor range.
///
/// # Safety
/// `off` lies within the object's slot region.
#[inline(always)]
pub(crate) unsafe fn object_slot_at(obj: *mut ObjectHeader, off: u32) -> *mut u64 {
    obj.cast::<u8>().add(off as usize).cast::<u64>()
}

/// The meta record of an object whose type byte `t` the caller already holds,
/// or null when that type carries none.
///
/// # Safety
/// `t` is `object_type(obj)`, read in this scope.
#[inline(always)]
pub(crate) unsafe fn object_meta_if_present(obj: *mut ObjectHeader, t: u8) -> *mut ObjectMeta {
    if object_has_meta_word(t) {
        (*obj).meta
    } else {
        std::ptr::null_mut()
    }
}

/// An object proven to carry the meta word: built by the runtime allocation
/// funnels, or in the `t`-tested arm of a caller's dispatch. Field access has
/// no check of its own.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub(crate) struct MetaObj(*mut ObjectHeader);

impl MetaObj {
    /// An object the runtime allocated itself as `GC_TYPE_OBJECT`
    /// (prototypes, class objects, bags, handles, JSON construction).
    ///
    /// # Safety
    /// `obj` was born from a runtime `arena_alloc_gc(.., GC_TYPE_OBJECT)`.
    #[inline(always)]
    pub(crate) unsafe fn runtime_born(obj: *mut ObjectHeader) -> Self {
        debug_assert_eq!(object_type(obj), GC_TYPE_OBJECT);
        MetaObj(obj)
    }

    /// An object in the arm of a dispatch that tested `object_has_meta_word(t)`.
    ///
    /// # Safety
    /// `t` is `object_type(obj)`, and the caller has tested it.
    #[inline(always)]
    pub(crate) unsafe fn from_type(obj: *mut ObjectHeader, t: u8) -> Self {
        debug_assert!(object_has_meta_word(t));
        debug_assert_eq!(object_type(obj), t);
        MetaObj(obj)
    }

    #[inline(always)]
    pub(crate) fn as_ptr(self) -> *mut ObjectHeader {
        self.0
    }

    #[inline(always)]
    pub(crate) unsafe fn meta(self) -> *mut ObjectMeta {
        (*self.0).meta
    }

    #[inline(always)]
    pub(crate) unsafe fn meta_slot(self) -> *mut *mut ObjectMeta {
        &mut (*self.0).meta
    }

    /// The first inline slot, for a proven meta-form object.
    #[inline(always)]
    pub(crate) unsafe fn slots(self) -> *mut u64 {
        self.0.add(1).cast::<u64>()
    }
}
