//! #8149: which registered buffers carry integer-indexed own properties.
//!
//! Perry backs FOUR distinct JS types with the same `BufferHeader` storage
//! (a buffer-family GC type each, #10694): a node `Buffer`, a `Uint8Array`, an
//! `ArrayBuffer`/`SharedArrayBuffer`, and a `DataView`. Only the first two are
//! *integer-indexed exotic objects*. The other two have **no** integer-indexed
//! own properties at all:
//!
//! ```js
//! const dv = new DataView(new ArrayBuffer(8));
//! dv[0]        // undefined  — not the byte
//! dv.length    // undefined  — DataView has byteLength, not length
//! 0 in dv      // false
//! dv[0] = 7    // an ORDINARY own property "0"; the byte stays 0
//! Object.keys(dv)  // ["0"] — the expando, never the bytes
//! ```
//!
//! Every consumer that triaged a receiver as "registered buffer ⇒ byte
//! indexable" served all four, so a `DataView` and a raw `ArrayBuffer` answered
//! bytes for `[i]`, `in`, `.length` and `hasOwnProperty`, and an index STORE
//! overwrote a byte where node creates a property.
//!
//! The discriminating question is asked ABOVE the byte arm, never below it:
//! the byte arm answers unconditionally, so a re-check placed after it is dead
//! code. This is the same ordering rule #8090 / #8109 / #8119 / #8120 / #8124 /
//! #8140 / #8141 / #8148 / #8173 each had to restore.
//!
//! Every flavor is answered from ONE read of the cell's GC type byte: arena
//! buffers, foreign-backed wrappers and process-global SharedArrayBuffers all
//! carry a real `GcHeader` whose type is their brand.

/// `true` when `addr` is a registered buffer whose integer indices really are
/// byte slots — a node `Buffer`, a `Uint8Array`, or another buffer-backed typed
/// array.
///
/// `false` both for a non-buffer and for the three registered buffers that are
/// NOT integer-indexed exotic objects: `ArrayBuffer`, `SharedArrayBuffer` and
/// `DataView`. Use [`is_non_indexed_buffer_view`] to tell those two `false`
/// cases apart — a non-buffer must keep falling through to the generic object
/// walk, while a `DataView` must answer `undefined`/`false` right here.
///
/// The KeyObject / CryptoKey buffers are byte-indexed today and stay that way:
/// this predicate is about the `ArrayBuffer`/`DataView` split, and silently
/// changing a crypto receiver's indexing would be an unrelated behaviour
/// change. `object::typed_array_proto_thunks::is_typed_array_buffer` is the
/// stricter sibling that also declines those, because it selects a
/// `%TypedArray%.prototype` METHOD population rather than an element read.
#[inline]
pub fn is_byte_indexed_buffer(addr: usize) -> bool {
    super::header::buffer_family_type(addr).is_some_and(|t| !is_non_indexed_type(t))
}

#[inline(always)]
fn is_non_indexed_type(obj_type: u8) -> bool {
    matches!(
        obj_type,
        GC_TYPE_BUFFER_ARRAY_BUFFER | GC_TYPE_BUFFER_SHARED_ARRAY_BUFFER | GC_TYPE_BUFFER_DATA_VIEW
    )
}

use crate::gc::{
    GC_TYPE_BUFFER, GC_TYPE_BUFFER_ARRAY_BUFFER, GC_TYPE_BUFFER_CRYPTO_KEY,
    GC_TYPE_BUFFER_DATA_VIEW, GC_TYPE_BUFFER_SECRET_KEY, GC_TYPE_BUFFER_SHARED_ARRAY_BUFFER,
    GC_TYPE_BUFFER_UINT8ARRAY,
};

/// `true` for the registered buffers with no integer-indexed own properties:
/// `ArrayBuffer`, `SharedArrayBuffer`, `DataView`.
///
/// One header read; `false` for anything that is not a buffer-family cell.
#[inline]
pub fn is_non_indexed_buffer_view(addr: usize) -> bool {
    super::header::buffer_family_type(addr).is_some_and(is_non_indexed_type)
}

/// #11239: which JS type a registered `BufferHeader` IS.
///
/// Five JS types share `BufferHeader` storage and the buffer registry. Every
/// predicate that asks "is this a Buffer / a Uint8Array / a view?" must answer
/// from this one classification — `ArrayBuffer.isView`, `util.types.*`,
/// `instanceof Uint8Array` / `instanceof Buffer` and `Buffer.isBuffer` each
/// used to spell their own subset of the side-table probes below, and they
/// disagreed: a Buffer was `instanceof Uint8Array` but not `isView`, and a
/// `DataView` or `ArrayBuffer` was `instanceof Buffer`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferBrand {
    /// `ArrayBuffer` or `SharedArrayBuffer` — a backing store, not a view.
    ArrayBuffer,
    /// `DataView` — a view, but not a typed array.
    DataView,
    /// KeyObject / CryptoKey key material. Not a JS view of any kind.
    KeyMaterial,
    /// A `Uint8Array` made by the `Uint8Array` constructor (or a
    /// typed-array method that returns one).
    Uint8Array,
    /// A Node `Buffer` — a `FastBuffer`, i.e. a `Uint8Array` whose prototype
    /// is `Buffer.prototype`. Every other byte-indexed registered buffer.
    NodeBuffer,
}

impl BufferBrand {
    /// A `Uint8Array` in the JS sense: a plain `Uint8Array` or a `Buffer`.
    #[inline]
    pub fn is_uint8_array(self) -> bool {
        matches!(self, Self::Uint8Array | Self::NodeBuffer)
    }
}

/// The [`BufferBrand`] of `addr`, or `None` when it is not a buffer-family
/// cell. One header read: the brand IS the cell's GC type (#10694). The one
/// brand that does not live in the type byte is a string-backed asymmetric
/// KeyObject's, whose table a test can also point at a buffer cell, so a byte
/// view still consults it (latch-guarded: free until an asymmetric key exists).
#[inline]
pub fn buffer_brand(addr: usize) -> Option<BufferBrand> {
    Some(match super::header::buffer_family_type(addr)? {
        GC_TYPE_BUFFER_ARRAY_BUFFER | GC_TYPE_BUFFER_SHARED_ARRAY_BUFFER => {
            BufferBrand::ArrayBuffer
        }
        GC_TYPE_BUFFER_DATA_VIEW => BufferBrand::DataView,
        GC_TYPE_BUFFER_SECRET_KEY | GC_TYPE_BUFFER_CRYPTO_KEY => BufferBrand::KeyMaterial,
        _ if super::asymmetric_key_meta(addr).is_some() => BufferBrand::KeyMaterial,
        GC_TYPE_BUFFER_UINT8ARRAY => BufferBrand::Uint8Array,
        GC_TYPE_BUFFER => BufferBrand::NodeBuffer,
        other => unreachable!("buffer_family_type answered non-flavor {other}"),
    })
}

/// `buffer_brand(addr).is_some_and(BufferBrand::is_uint8_array)` without the
/// `Uint8Array`-vs-`Buffer` probe neither answer needs — this sits on
/// element-access paths (`%TypedArray%.prototype` receiver gating).
#[inline]
pub fn is_uint8_view_buffer(addr: usize) -> bool {
    buffer_brand(addr).is_some_and(BufferBrand::is_uint8_array)
}

/// `true` only for a Node `Buffer`, not for another JS type that happens to
/// share Perry's `BufferHeader` storage and registry entry.
///
/// Keep this stricter than [`is_byte_indexed_buffer`]: a plain `Uint8Array` is
/// also byte-indexed, but `Buffer.isBuffer(new Uint8Array(...))` must be false.
/// See [`buffer_brand`].
#[inline]
pub fn is_node_buffer(addr: usize) -> bool {
    buffer_brand(addr) == Some(BufferBrand::NodeBuffer)
}

/// The own-property key a NUMERIC computed key names on a non-indexed buffer
/// view, or `None` when the key is not a number at all.
///
/// `dv[0] = 7` stores under `"0"` and `Object.keys(dv)` must report `"0"` —
/// the ordinary `ToPropertyKey` string, not the raw double. The argument is the
/// NaN-BOXED key as the index paths carry it, so both the `INT32_TAG` form
/// (`dv[0]` with a loop counter) and the plain-double form reach the same
/// answer; a string / symbol / pointer key answers `None` so the caller keeps
/// its existing by-name route.
///
/// Scope: CANONICAL array indices only. A numeric key that is not one (`-1`,
/// `1.5`, `NaN`) is also a legitimate ordinary property key in node
/// (`dv[-1] = 1` then `Object.keys(dv)` is `["-1"]`), but perry drops that
/// store today for a Buffer as well as a DataView, so it is left alone here
/// rather than fixed only for one of the four buffer-backed types. The caller
/// falls through to its existing route, whose answer for those keys is
/// unchanged.
pub fn canonical_index_key(index: f64) -> Option<String> {
    let jv = crate::value::JSValue::from_bits(index.to_bits());
    let n = if jv.is_int32() {
        f64::from(jv.as_int32())
    } else if jv.is_number() {
        index
    } else {
        return None;
    };
    (n >= 0.0 && n.fract() == 0.0 && n <= u32::MAX as f64).then(|| (n as u32).to_string())
}
