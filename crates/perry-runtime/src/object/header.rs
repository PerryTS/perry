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
use crate::gc::{GcHeader, GC_HEADER_SIZE, GC_TYPE_OBJECT};

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
    if crate::object_has_meta_word!(t) {
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
pub(crate) struct MetaObj(pub(crate) *mut ObjectHeader);

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

    /// An object in the arm of a dispatch that tested `crate::object_has_meta_word!(t)`.
    ///
    /// # Safety
    /// `t` is `object_type(obj)`, and the caller has tested it.
    #[inline(always)]
    pub(crate) unsafe fn from_type(obj: *mut ObjectHeader, t: u8) -> Self {
        debug_assert!(crate::object_has_meta_word!(t));
        debug_assert_eq!(object_type(obj), t);
        MetaObj(obj)
    }

    /// An object reached because a key entry says its storage is a spill or an
    /// overflow table; only a meta-form object has either (P3).
    ///
    /// # Safety
    /// `entry` is the key entry (or position) that led here and names spill or
    /// overflow storage; `obj` is a live ordinary object.
    #[inline(always)]
    pub(crate) unsafe fn from_storage<E>(obj: *mut ObjectHeader, _entry: E) -> Self {
        debug_assert!(crate::object_has_meta_word!(object_type(obj)));
        MetaObj(obj)
    }

    /// An object whose layout word names a kind other than PLAIN/CLASS (P10),
    /// which is always meta-form.
    ///
    /// # Safety
    /// `word` is the layout word of `obj`, read in this scope.
    #[inline(always)]
    pub(crate) unsafe fn from_kind<W>(obj: *mut ObjectHeader, _word: W) -> Self {
        debug_assert!(crate::object_has_meta_word!(object_type(obj)));
        MetaObj(obj)
    }

    /// An object whose record is a runtime-minted shape (DESIGN §3.2): such
    /// objects exist only in meta form.
    ///
    /// # Safety
    /// `shape` is the runtime-minted shape record of `obj`.
    #[inline(always)]
    pub(crate) unsafe fn of_runtime_shape<S>(obj: *mut ObjectHeader, _shape: S) -> Self {
        debug_assert!(crate::object_has_meta_word!(object_type(obj)));
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

/// The allocation size, in bytes, of a runtime birth with `slots` inline
/// slots. Runtime births are meta-form (P10), so the size is the meta-form
/// header plus the slots.
#[inline(always)]
pub const fn meta_form_alloc_bytes(slots: usize) -> usize {
    std::mem::size_of::<ObjectHeader>() + slots * std::mem::size_of::<u64>()
}

/// The byte offset of inline slot `slot` of a meta-form object: what a
/// per-site memo stores at fill, once, so every reader adds it to the object
/// ([`object_slot_at`]) and none composes a slot base.
#[inline(always)]
pub const fn meta_form_slot_off(slot: u32) -> u32 {
    meta_form_alloc_bytes(slot as usize) as u32
}

// The constructors as macros: the struct literal is built in the caller, with
// the form check under `cfg(debug_assertions)`, so a migrated read is the
// tokens it replaced and no release code depends on an inlining decision.

/// [`MetaObj::runtime_born`].
#[macro_export]
macro_rules! meta_obj_runtime_born {
    ($obj:expr $(,)?) => {{
        let obj: *mut $crate::object::ObjectHeader = $obj;
        #[cfg(debug_assertions)]
        {
            // The macro expands both inside and outside `unsafe` blocks.
            #[allow(unused_unsafe)]
            let ty = unsafe { $crate::object::header::object_type(obj) };
            debug_assert_eq!(ty, $crate::gc::GC_TYPE_OBJECT);
        }
        $crate::object::header::MetaObj(obj)
    }};
}

/// [`MetaObj::from_type`].
#[macro_export]
macro_rules! meta_obj_from_type {
    ($obj:expr, $t:expr $(,)?) => {{
        let obj: *mut $crate::object::ObjectHeader = $obj;
        #[cfg(debug_assertions)]
        {
            let t: u8 = $t;
            debug_assert!($crate::object_has_meta_word!(t));
            // The macro expands both inside and outside `unsafe` blocks.
            #[allow(unused_unsafe)]
            let ty = unsafe { $crate::object::header::object_type(obj) };
            debug_assert_eq!(ty, t);
        }
        $crate::object::header::MetaObj(obj)
    }};
}

/// [`MetaObj::from_storage`].
#[macro_export]
macro_rules! meta_obj_from_storage {
    ($obj:expr, $entry:expr $(,)?) => {{
        let obj: *mut $crate::object::ObjectHeader = $obj;
        let _ = &$entry;
        #[cfg(debug_assertions)]
        {
            // The macro expands both inside and outside `unsafe` blocks.
            #[allow(unused_unsafe)]
            let ty = unsafe { $crate::object::header::object_type(obj) };
            debug_assert!($crate::object_has_meta_word!(ty));
        }
        $crate::object::header::MetaObj(obj)
    }};
}

/// [`MetaObj::from_kind`].
#[macro_export]
macro_rules! meta_obj_from_kind {
    ($obj:expr, $word:expr $(,)?) => {{
        let obj: *mut $crate::object::ObjectHeader = $obj;
        let _ = &$word;
        #[cfg(debug_assertions)]
        {
            // The macro expands both inside and outside `unsafe` blocks.
            #[allow(unused_unsafe)]
            let ty = unsafe { $crate::object::header::object_type(obj) };
            debug_assert!($crate::object_has_meta_word!(ty));
        }
        $crate::object::header::MetaObj(obj)
    }};
}

/// The meta word of a proven object: `(*obj).meta`, no call.
#[macro_export]
macro_rules! meta_read {
    ($m:expr) => {{
        let m: $crate::object::header::MetaObj = $m;
        (*m.0).meta
    }};
}

/// The address of the meta word of a proven object, no call.
#[macro_export]
macro_rules! meta_slot_of {
    ($m:expr) => {{
        let m: $crate::object::header::MetaObj = $m;
        ::core::ptr::addr_of_mut!((*m.0).meta)
    }};
}

/// The first inline slot of a proven meta-form object, no call.
#[macro_export]
macro_rules! meta_slots_of {
    ($m:expr) => {{
        let m: $crate::object::header::MetaObj = $m;
        m.0.add(1).cast::<u64>()
    }};
}
