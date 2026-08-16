//! #8149: which registered buffers carry integer-indexed own properties.
//!
//! Perry backs FOUR distinct JS types with the same `BufferHeader` + the same
//! `BUFFER_REGISTRY` entry: a node `Buffer`, a `Uint8Array`, an
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
//! Both backings are covered by construction. `is_registered_buffer` is a
//! side-table membership test, not an address-range or GC-header probe, so an
//! EXTERNAL buffer (no `GcHeader` at all — see `array/header.rs`'s
//! `array_receiver_gc_tag` doc, #8142) is classified by exactly the same
//! lookups as an arena-backed one.

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
    super::is_registered_buffer(addr) && !is_non_indexed_buffer_view(addr)
}

/// `true` for the registered buffers with no integer-indexed own properties:
/// `ArrayBuffer`, `SharedArrayBuffer`, `DataView`.
///
/// Does NOT itself check registration — both underlying sets are address-keyed
/// and only ever populated for registered buffers, and both are latch-guarded
/// so a program that never constructs one pays two relaxed atomic loads. Call
/// it inside an arm that has already established `is_registered_buffer`, or use
/// [`is_byte_indexed_buffer`] for the combined question.
#[inline]
pub fn is_non_indexed_buffer_view(addr: usize) -> bool {
    super::is_any_array_buffer(addr) || super::is_data_view(addr)
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
