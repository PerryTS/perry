//! `JsValue` + object/array helpers for native binding wrappers
//! that need to construct JavaScript values at runtime (added in
//! v0.5.x of the perry-ffi v0.5 surface — non-breaking; pure
//! additions).
//!
//! Database drivers (mysql2, pg, sqlite, ioredis, mongodb), HTTP
//! clients (fetch, axios), and most non-trivial wrappers need to
//! return rows-as-objects, result sets as arrays, parse config
//! objects, and so on. This module exposes the minimum surface
//! that closes those use cases while keeping perry-runtime's
//! NaN-boxing tags hidden behind type-safe constructors.
//!
//! # NaN-boxing
//!
//! Perry encodes every JS value into 64 bits using NaN-boxing.
//! Numbers are real `f64`s; everything else lives in the high
//! 16 bits' tag space:
//!
//! ```text
//! TAG_UNDEFINED = 0x7FFC_0000_0000_0001
//! TAG_NULL      = 0x7FFC_0000_0000_0002
//! TAG_FALSE     = 0x7FFC_0000_0000_0003
//! TAG_TRUE      = 0x7FFC_0000_0000_0004
//! SHORT_STRING_TAG = 0x7FF9  (SSO — inline string ≤5 bytes; len in
//!                             bits 40..=47, data in bits 0..=39)
//! BIGINT_TAG    = 0x7FFA  (lower 48 = ptr)
//! POINTER_TAG   = 0x7FFD  (lower 48 = ptr to ObjectHeader / ArrayHeader / etc.)
//! INT32_TAG     = 0x7FFE  (lower 32 = i32)
//! STRING_TAG    = 0x7FFF  (lower 48 = ptr to StringHeader)
//! ```
//!
//! These tag values are part of perry-ffi's stable API — a
//! perry-runtime renumbering bumps perry-ffi major.

use crate::{ArrayHeader, ObjectHeader, StringHeader};

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const SHORT_STRING_TAG: u64 = 0x7FF9_0000_0000_0000;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const INT32_TAG: u64 = 0x7FFE_0000_0000_0000;
const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// A NaN-boxed JavaScript value, as it crosses the FFI boundary.
///
/// `JsValue` is `#[repr(transparent)]` over `u64`, so functions
/// declared `extern "C" fn(JsValue) -> JsValue` use the same C
/// ABI as `extern "C" fn(u64) -> u64` — pass-by-value in a single
/// register on every platform Perry targets.
#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct JsValue(pub u64);

impl JsValue {
    /// `undefined`.
    pub const UNDEFINED: Self = Self(TAG_UNDEFINED);
    /// `null`.
    pub const NULL: Self = Self(TAG_NULL);
    /// `true`.
    pub const TRUE: Self = Self(TAG_TRUE);
    /// `false`.
    pub const FALSE: Self = Self(TAG_FALSE);

    /// Construct from a Rust `bool`.
    #[inline]
    pub const fn from_bool(b: bool) -> Self {
        Self(if b { TAG_TRUE } else { TAG_FALSE })
    }

    /// Construct from a Rust `f64`. NaN inputs are passed through;
    /// callers that need a canonical NaN should convert to
    /// `Self::UNDEFINED` themselves.
    #[inline]
    pub fn from_number(n: f64) -> Self {
        Self(n.to_bits())
    }

    /// Construct from a 32-bit integer. Encoded as `INT32_TAG` —
    /// faster than `from_number(n as f64)` for integer-heavy paths
    /// (db rowids, indexes, …) and avoids any precision loss.
    #[inline]
    pub const fn from_int32(n: i32) -> Self {
        Self(INT32_TAG | (n as u32 as u64))
    }

    /// Wrap a `*mut StringHeader` returned from
    /// [`crate::alloc_string`] (or another allocation primitive).
    #[inline]
    pub fn from_string_ptr(p: *mut StringHeader) -> Self {
        Self(STRING_TAG | (p as u64 & POINTER_MASK))
    }

    /// Wrap an `*mut ObjectHeader`, `*mut ArrayHeader`, or any
    /// other runtime-allocated heap pointer that the JS side sees
    /// as an object reference.
    #[inline]
    pub fn from_object_ptr<T>(p: *mut T) -> Self {
        Self(POINTER_TAG | (p as u64 & POINTER_MASK))
    }

    /// The raw NaN-boxed bits. Used when round-tripping through a
    /// promise resolution or any other extern function that takes
    /// `u64`.
    #[inline]
    pub const fn bits(self) -> u64 {
        self.0
    }

    /// Reconstruct a `JsValue` from raw bits. Inverse of [`bits`].
    #[inline]
    pub const fn from_bits(b: u64) -> Self {
        Self(b)
    }

    // ── type predicates ────────────────────────────────────────

    /// True if the value is `undefined`.
    #[inline]
    pub const fn is_undefined(self) -> bool {
        self.0 == TAG_UNDEFINED
    }

    /// True if the value is `null`.
    #[inline]
    pub const fn is_null(self) -> bool {
        self.0 == TAG_NULL
    }

    /// True if the value is `true` or `false`.
    #[inline]
    pub const fn is_bool(self) -> bool {
        self.0 == TAG_TRUE || self.0 == TAG_FALSE
    }

    /// True if the value is a heap JS string (`STRING_TAG`) — i.e.
    /// a real `*mut StringHeader`. This is the STRICT check: it does
    /// NOT match inline SSO short strings (`SHORT_STRING_TAG`).
    ///
    /// ⚠ #1781 footgun — do NOT branch
    /// `if v.is_string() { read ptr } else { reject / treat as pointer }`
    /// on a value that could be *any* string. An inline SSO short
    /// string (len 0..=5, `SHORT_STRING_TAG = 0x7FF9`) fails this
    /// strict check and silently falls into the else-branch — so
    /// `socket.write("hi")` and short HTTP bodies got dropped. When a
    /// value may be any string representation, gate on
    /// [`is_any_string`](Self::is_any_string) and materialize it with
    /// `js_get_string_pointer_unified`, which turns either repr into a
    /// real heap `*StringHeader`.
    #[inline]
    pub const fn is_string(self) -> bool {
        (self.0 & TAG_MASK) == STRING_TAG
    }

    /// True for any string representation — a heap `STRING_TAG`
    /// pointer OR an inline `SHORT_STRING_TAG` SSO value. Use this
    /// (not [`is_string`](Self::is_string)) for representation-agnostic
    /// "is this a string?" checks, then materialize via
    /// `js_get_string_pointer_unified` to read the bytes.
    #[inline]
    pub const fn is_any_string(self) -> bool {
        let tag = self.0 & TAG_MASK;
        tag == STRING_TAG || tag == SHORT_STRING_TAG
    }

    /// True iff the value is specifically an inline SSO short string
    /// (`SHORT_STRING_TAG` — string of length 0..=5 stored in the
    /// NaN-box payload, no heap allocation).
    #[inline]
    pub const fn is_short_string(self) -> bool {
        (self.0 & TAG_MASK) == SHORT_STRING_TAG
    }

    /// True if the value is a heap object pointer (`POINTER_TAG` —
    /// covers ObjectHeader, ArrayHeader, ClosureHeader, etc).
    #[inline]
    pub const fn is_pointer(self) -> bool {
        (self.0 & TAG_MASK) == POINTER_TAG
    }

    /// True if the value is a 32-bit integer.
    #[inline]
    pub const fn is_int32(self) -> bool {
        (self.0 & TAG_MASK) == INT32_TAG
    }

    /// True if the value is a number (real f64 OR int32). Returns
    /// false for NaN-tagged values like undefined / null / strings.
    #[inline]
    pub fn is_number(self) -> bool {
        let bits = self.0;
        // f64 values have either both top bits zero (positive
        // doubles) or top bit one with the rest unrestricted. NaN
        // sentinels live in 0x7FF8 / 0x7FFC..7FFF — anything in
        // those tag bands is NOT a number.
        let high = bits & TAG_MASK;
        let nan_band = (0x7FF8_0000_0000_0000..=0x7FFF_0000_0000_0000).contains(&high);
        !nan_band || self.is_int32()
    }

    // ── accessors ──────────────────────────────────────────────

    /// Decode as a Rust `bool`. Returns `false` for any non-bool
    /// value — callers should `is_bool()` first if the input might
    /// be undefined/null.
    #[inline]
    pub fn to_bool(self) -> bool {
        self.0 == TAG_TRUE
    }

    /// Decode as a Rust `f64`. For `int32`-tagged values this
    /// converts via the i32; for real numbers this reads the bits
    /// directly. For non-numeric values returns NaN.
    #[inline]
    pub fn to_number(self) -> f64 {
        if self.is_int32() {
            (self.0 as u32 as i32) as f64
        } else if self.is_number() {
            f64::from_bits(self.0)
        } else {
            f64::NAN
        }
    }

    /// Extract the i32 from an `INT32_TAG`-tagged value. Returns
    /// 0 for non-int32 values.
    #[inline]
    pub fn to_int32(self) -> i32 {
        if self.is_int32() {
            self.0 as u32 as i32
        } else {
            0
        }
    }

    /// Extract the `*mut StringHeader` for a `STRING_TAG`-tagged
    /// value. Returns null for non-strings.
    #[inline]
    pub fn as_string_ptr(self) -> *mut StringHeader {
        if self.is_string() {
            (self.0 & POINTER_MASK) as *mut StringHeader
        } else {
            std::ptr::null_mut()
        }
    }

    /// Extract the heap pointer for a `POINTER_TAG`-tagged value.
    /// Generic `T` lets callers cast to whichever header they
    /// expect (`ObjectHeader`, `ArrayHeader`, etc.). Returns null
    /// for non-pointers.
    #[inline]
    pub fn as_pointer<T>(self) -> *mut T {
        if self.is_pointer() {
            (self.0 & POINTER_MASK) as *mut T
        } else {
            std::ptr::null_mut()
        }
    }
}

impl std::fmt::Debug for JsValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_undefined() {
            write!(f, "JsValue::UNDEFINED")
        } else if self.is_null() {
            write!(f, "JsValue::NULL")
        } else if self.is_bool() {
            write!(f, "JsValue::from_bool({})", self.to_bool())
        } else if self.is_int32() {
            write!(f, "JsValue::from_int32({})", self.to_int32())
        } else if self.is_string() {
            write!(f, "JsValue::from_string_ptr({:p})", self.as_string_ptr())
        } else if self.is_pointer() {
            write!(f, "JsValue::from_object_ptr({:p})", self.as_pointer::<()>())
        } else {
            write!(f, "JsValue::from_number({})", self.to_number())
        }
    }
}

// ── object / array allocation primitives ─────────────────────────

extern "C" {
    /// Allocate an empty JS array with the given initial capacity.
    /// Capacity is a hint for the runtime; the array can grow.
    pub fn js_array_alloc(capacity: u32) -> *mut ArrayHeader;

    /// Push a value onto the array. Returns the (possibly
    /// reallocated) array header pointer — always reassign:
    /// `arr = js_array_push(arr, value);`
    pub fn js_array_push(arr: *mut ArrayHeader, value: JsValue) -> *mut ArrayHeader;

    /// Read the element at `index`. Returns `JsValue::UNDEFINED`
    /// for out-of-bounds.
    pub fn js_array_get(arr: *const ArrayHeader, index: u32) -> JsValue;

    /// Number of elements in the array.
    pub fn js_array_length(arr: *const ArrayHeader) -> u32;

    /// Write to `index` in-place. No bounds check — caller's
    /// responsibility.
    pub fn js_array_set(arr: *mut ArrayHeader, index: u32, value: JsValue);

    /// Allocate an object with the given shape. `packed_keys` is
    /// a null-byte-separated UTF-8 string ("foo\0bar\0baz" for an
    /// object with keys foo/bar/baz). `shape_id` is a hash that
    /// the runtime uses to dedupe shape metadata across allocs;
    /// see [`build_object_shape`] for the recommended derivation.
    pub fn js_object_alloc_with_shape(
        shape_id: u32,
        field_count: u32,
        packed_keys: *const u8,
        packed_keys_len: u32,
    ) -> *mut ObjectHeader;

    /// Read the field at `field_index` (0-based, in the order the
    /// shape declared them).
    pub fn js_object_get_field(obj: *const ObjectHeader, field_index: u32) -> JsValue;

    /// Write the field at `field_index`.
    pub fn js_object_set_field(obj: *mut ObjectHeader, field_index: u32, value: JsValue);
}

/// Compute `(packed_keys_bytes, shape_id)` for use with
/// [`js_object_alloc_with_shape`].
///
/// The `shape_id` is a stable hash of the keys; the runtime uses
/// it to share shape metadata across allocations of the same
/// object literal. The exact hash isn't part of the JS-visible
/// contract — the runtime treats `shape_id` as a hint and falls
/// back to packed-key comparison on collisions — but using
/// [`build_object_shape`] gives every wrapper the same hash for
/// the same key list, which improves shape sharing across
/// crates. `0x4646_0000` ("FF" prefix) namespaces perry-ffi-built
/// shapes from perry-stdlib's hand-rolled ones.
pub fn build_object_shape(keys: &[&str]) -> (Vec<u8>, u32) {
    let mut packed: Vec<u8> = Vec::new();
    let mut shape_id: u32 = 0x4646_0000;
    for (i, name) in keys.iter().enumerate() {
        if i > 0 {
            packed.push(0u8);
        }
        packed.extend_from_slice(name.as_bytes());
        for &b in name.as_bytes() {
            shape_id = shape_id.wrapping_mul(31).wrapping_add(b as u32);
        }
    }
    shape_id = shape_id.wrapping_add(keys.len() as u32);
    (packed, shape_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_round_trip() {
        assert!(JsValue::UNDEFINED.is_undefined());
        assert!(JsValue::NULL.is_null());
        assert!(JsValue::TRUE.is_bool());
        assert_eq!(JsValue::TRUE.to_bool(), true);
        assert_eq!(JsValue::FALSE.to_bool(), false);
    }

    #[test]
    fn number_round_trips() {
        for n in [0.0, 1.0, -1.5, std::f64::consts::PI, 1e300] {
            let v = JsValue::from_number(n);
            assert!(v.is_number(), "{} should be number, got {:?}", n, v);
            assert_eq!(v.to_number(), n);
        }
    }

    #[test]
    fn int32_round_trips() {
        for n in [0, 1, -1, i32::MIN, i32::MAX] {
            let v = JsValue::from_int32(n);
            assert!(v.is_int32(), "{} should be int32, got {:?}", n, v);
            assert_eq!(v.to_int32(), n);
            // is_number also covers int32:
            assert!(v.is_number());
            assert_eq!(v.to_number(), n as f64);
        }
    }

    #[test]
    fn type_predicates_disjoint() {
        let cases = [
            (JsValue::UNDEFINED, "undefined"),
            (JsValue::NULL, "null"),
            (JsValue::TRUE, "true"),
            (JsValue::FALSE, "false"),
            (JsValue::from_number(3.14), "number"),
            (JsValue::from_int32(42), "int32"),
        ];
        for (val, kind) in cases {
            // Each value should match at most one main predicate
            // (excluding is_number, which overlaps with int32).
            let mut count = 0;
            if val.is_undefined() {
                count += 1;
            }
            if val.is_null() {
                count += 1;
            }
            if val.is_bool() {
                count += 1;
            }
            if val.is_string() {
                count += 1;
            }
            if val.is_pointer() {
                count += 1;
            }
            if val.is_int32() {
                count += 1;
            }
            // Numbers (non-int32) match only `is_number`, not the
            // others — count stays at 0.
            assert!(count <= 1, "{kind} matched multiple main predicates");
        }
    }

    /// A JS string has TWO NaN-box representations: a heap
    /// `STRING_TAG` (real `*StringHeader`) and an inline `SHORT_STRING_TAG`
    /// SSO value (string ≤5 bytes packed into the payload). `is_any_string`
    /// must match BOTH; the strict `is_string` matches only the heap repr.
    /// The footgun: branching on `is_string()` silently drops SSO short
    /// strings, so `socket.write("hi")` / `res.end("hi")` emit nothing.
    /// `is_short_string` must distinguish the SSO repr specifically.
    #[test]
    fn is_any_string_matches_both_string_reprs() {
        // An inline SSO value, built the way the runtime packs one: tag
        // 0x7FF9, length in bits 40..=47, data little-endian in bits 0..=39.
        // "hi" → len 2, payload 'h' | ('i' << 8).
        let sso_bits = SHORT_STRING_TAG | (2u64 << 40) | (b'h' as u64) | ((b'i' as u64) << 8);
        let sso = JsValue::from_bits(sso_bits);
        assert!(sso.is_any_string(), "SSO short string is a string");
        assert!(sso.is_short_string(), "SSO short string is the short repr");
        assert!(
            !sso.is_string(),
            "strict is_string() must NOT match SSO — this is the footgun"
        );
        assert!(!sso.is_pointer(), "SSO is not a heap pointer");

        // A heap STRING_TAG value (the payload pointer is never dereferenced
        // by these predicates, so a sentinel address is fine).
        let heap = JsValue::from_bits(STRING_TAG | 0x1234);
        assert!(heap.is_any_string(), "heap string is a string");
        assert!(heap.is_string(), "heap string matches strict is_string()");
        assert!(
            !heap.is_short_string(),
            "a heap STRING_TAG value is not the SSO repr"
        );

        // Non-strings: neither predicate fires.
        for (val, kind) in [
            (JsValue::UNDEFINED, "undefined"),
            (JsValue::NULL, "null"),
            (JsValue::from_number(3.14), "number"),
            (JsValue::TRUE, "bool"),
            (JsValue::from_int32(42), "int32"),
            (JsValue::from_bits(POINTER_TAG | 0x5678), "pointer"),
        ] {
            assert!(!val.is_any_string(), "{kind} is not any string");
            assert!(!val.is_short_string(), "{kind} is not a short string");
        }
    }

    #[test]
    fn shape_hash_is_deterministic() {
        let (k1, s1) = build_object_shape(&["foo", "bar", "baz"]);
        let (k2, s2) = build_object_shape(&["foo", "bar", "baz"]);
        assert_eq!(k1, k2);
        assert_eq!(s1, s2);
        // Different key set → different hash (probabilistically).
        let (_, s3) = build_object_shape(&["foo", "qux", "baz"]);
        assert_ne!(s1, s3);
    }

    #[test]
    fn packed_keys_format() {
        let (packed, _) = build_object_shape(&["a", "bb", "ccc"]);
        assert_eq!(packed, b"a\0bb\0ccc");
    }
}
