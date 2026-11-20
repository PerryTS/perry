//! #11239: the one classifier behind every `ArrayBuffer`-view predicate.
//!
//! `ArrayBuffer.isView` (direct call and extracted value), every
//! `util.types.is*Array` / `isArrayBufferView` / `isDataView`, and
//! `instanceof Uint8Array` / `instanceof Buffer` all answer from
//! [`view_brand`]. They used to spell their own subsets of the buffer
//! side-table probes and disagreed with each other: a Node `Buffer` was
//! `instanceof Uint8Array` yet not `ArrayBuffer.isView` (which broke bson's
//! `UUID` constructor, and with it mongodb's `listCollections`), while a
//! `DataView` or an `ArrayBuffer` was `instanceof Buffer`.
//!
//! In Node a `Buffer` is a `FastBuffer`, a `Uint8Array` subclass, so every
//! Uint8Array predicate is true for it.

use crate::buffer::BufferBrand;
use crate::value::JSValue;

/// What kind of `ArrayBuffer` view a value is, if any.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ViewBrand {
    /// A Node `Buffer`: a `Uint8Array` whose prototype is `Buffer.prototype`.
    NodeBuffer,
    /// A `%TypedArray%` instance of this element kind (`typedarray::KIND_*`),
    /// including a plain `Uint8Array`.
    TypedArray(u8),
    /// A plain object (`GC_TYPE_OBJECT`) of a user class extending a
    /// typed-array constructor: classified as a view, but it carries no view
    /// storage (a subclass instance built by the base constructor is a
    /// [`Self::TypedArray`] cell), and the class registry records no kind.
    TypedArraySubclass,
    /// A `DataView` cell, including a subclass instance built by the base
    /// constructor.
    DataView,
    /// A plain object of a user class extending `DataView`, without view
    /// storage (as [`Self::TypedArraySubclass`]).
    DataViewSubclass,
}

impl ViewBrand {
    /// The element kind, when known. A Node `Buffer` is a `Uint8Array`.
    #[inline]
    pub(crate) fn typed_array_kind(self) -> Option<u8> {
        match self {
            Self::NodeBuffer => Some(crate::typedarray_hook_slots::KIND_UINT8),
            Self::TypedArray(kind) => Some(kind),
            Self::TypedArraySubclass | Self::DataView | Self::DataViewSubclass => None,
        }
    }

    /// `util.types.isTypedArray`: every view except a `DataView`.
    #[inline]
    pub(crate) fn is_typed_array(self) -> bool {
        !matches!(self, Self::DataView | Self::DataViewSubclass)
    }

    /// Whether the value carries the view byte storage
    /// (`buffer::bytes::span` resolves it). Every operation that reads or
    /// writes the view's bytes checks this, so its check and its byte
    /// resolution cannot disagree.
    #[inline]
    pub(crate) fn has_storage(self) -> bool {
        !matches!(self, Self::TypedArraySubclass | Self::DataViewSubclass)
    }

    /// `util.types.isDataView`.
    #[inline]
    pub(crate) fn is_data_view(self) -> bool {
        matches!(self, Self::DataView | Self::DataViewSubclass)
    }
}

/// The [`ViewBrand`] of a JS value (NaN-boxed or legacy raw-pointer form).
/// `None` for every non-view, including `ArrayBuffer` / `SharedArrayBuffer`
/// and crypto key material that shares buffer storage.
pub(crate) fn view_brand(value: f64) -> Option<ViewBrand> {
    let addr = crate::value::addr_class::object_ref_addr(value);
    if addr == 0 {
        return None;
    }
    if let Some(brand) = crate::buffer::buffer_brand(addr) {
        return match brand {
            BufferBrand::NodeBuffer => Some(ViewBrand::NodeBuffer),
            BufferBrand::Uint8Array => Some(ViewBrand::TypedArray(
                crate::typedarray_hook_slots::KIND_UINT8,
            )),
            BufferBrand::DataView => Some(ViewBrand::DataView),
            BufferBrand::ArrayBuffer | BufferBrand::KeyMaterial => None,
        };
    }
    if let Some(kind) = crate::typedarray_hook_slots::lookup_typed_array_kind(addr) {
        return Some(ViewBrand::TypedArray(kind));
    }
    extends_builtin_view(value)
}

/// A user-class instance (`GC_TYPE_OBJECT` with a class id) whose class chain
/// reaches `DataView` or a typed-array constructor.
fn extends_builtin_view(value: f64) -> Option<ViewBrand> {
    let v = JSValue::from_bits(value.to_bits());
    if !v.is_pointer() {
        return None;
    }
    let ptr = v.as_pointer::<u8>();
    let gc_header = unsafe { crate::value::addr_class::try_read_tracked_gc_header(ptr as usize) }?;
    let class_id = unsafe {
        if crate::gc_type_is_not_object!((*gc_header.as_ptr()).obj_type) {
            return None;
        }
        (*(ptr as *const super::ObjectHeader)).class_id
    };
    if class_id == 0 {
        None
    } else if super::extends_builtin_data_view(class_id) {
        Some(ViewBrand::DataViewSubclass)
    } else if super::extends_builtin_typed_array(class_id) {
        Some(ViewBrand::TypedArraySubclass)
    } else {
        None
    }
}

#[cfg(test)]
#[path = "view_brand_tests.rs"]
mod tests;
