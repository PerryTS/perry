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
    /// An object instance of a user class extending a typed-array constructor,
    /// whose element kind the class registry does not record.
    TypedArraySubclass,
    /// A `DataView`, or an instance of a user class extending it.
    DataView,
}

impl ViewBrand {
    /// The element kind, when known. A Node `Buffer` is a `Uint8Array`.
    #[inline]
    pub(crate) fn typed_array_kind(self) -> Option<u8> {
        match self {
            Self::NodeBuffer => Some(crate::typedarray::KIND_UINT8),
            Self::TypedArray(kind) => Some(kind),
            Self::TypedArraySubclass | Self::DataView => None,
        }
    }

    /// `util.types.isTypedArray`: every view except a `DataView`.
    #[inline]
    pub(crate) fn is_typed_array(self) -> bool {
        !matches!(self, Self::DataView)
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
            BufferBrand::Uint8Array => Some(ViewBrand::TypedArray(crate::typedarray::KIND_UINT8)),
            BufferBrand::DataView => Some(ViewBrand::DataView),
            BufferBrand::ArrayBuffer | BufferBrand::KeyMaterial => None,
        };
    }
    if let Some(kind) = crate::typedarray::lookup_typed_array_kind(addr) {
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
        if (*gc_header.as_ptr()).obj_type != crate::gc::GC_TYPE_OBJECT {
            return None;
        }
        (*(ptr as *const super::ObjectHeader)).class_id
    };
    if class_id == 0 {
        None
    } else if super::extends_builtin_data_view(class_id) {
        Some(ViewBrand::DataView)
    } else if super::extends_builtin_typed_array(class_id) {
        Some(ViewBrand::TypedArraySubclass)
    } else {
        None
    }
}

#[cfg(test)]
#[path = "view_brand_tests.rs"]
mod tests;
