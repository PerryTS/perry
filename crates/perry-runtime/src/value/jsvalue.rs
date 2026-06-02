//! The `JSValue` NaN-boxed value type, its construct/inspect/coerce
//! methods, and the `Debug`/`Default` impls.

use super::*;

/// A JavaScript value using NaN-boxing representation
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct JSValue {
    pub(super) bits: u64,
}

impl JSValue {
    /// Create undefined value
    #[inline]
    pub const fn undefined() -> Self {
        Self {
            bits: TAG_UNDEFINED,
        }
    }

    /// Create null value
    #[inline]
    pub const fn null() -> Self {
        Self { bits: TAG_NULL }
    }

    /// Create a boolean value
    #[inline]
    pub const fn bool(value: bool) -> Self {
        Self {
            bits: if value { TAG_TRUE } else { TAG_FALSE },
        }
    }

    /// Create an f64 number value
    #[inline]
    pub fn number(value: f64) -> Self {
        // Just reinterpret the bits - f64 values are stored directly
        Self {
            bits: value.to_bits(),
        }
    }

    /// Create an i32 value (stored in payload, faster than f64 for integers)
    #[inline]
    pub const fn int32(value: i32) -> Self {
        Self {
            bits: INT32_TAG | ((value as u32) as u64),
        }
    }

    /// Create a pointer value (for heap-allocated objects)
    #[inline]
    pub fn pointer(ptr: *const u8) -> Self {
        debug_assert!(
            (ptr as u64) <= POINTER_MASK,
            "Pointer too large for NaN-boxing"
        );
        Self {
            bits: POINTER_TAG | (ptr as u64 & POINTER_MASK),
        }
    }

    /// Check if this is a number (not a tagged value)
    #[inline]
    pub fn is_number(&self) -> bool {
        // Perry-owned tags occupy the positive qNaN band 0x7FF9..=0x7FFF.
        // Keep IEEE f64 values, including canonical qNaN 0x7FF8 and negative
        // NaN payloads, classified as numbers.
        let tag = self.bits & TAG_MASK;
        !(SHORT_STRING_TAG..=STRING_TAG).contains(&tag)
    }

    /// Check if this is undefined
    #[inline]
    pub fn is_undefined(&self) -> bool {
        self.bits == TAG_UNDEFINED
    }

    /// Check if this is null
    #[inline]
    pub fn is_null(&self) -> bool {
        self.bits == TAG_NULL
    }

    /// Check if this is a boolean
    #[inline]
    pub fn is_bool(&self) -> bool {
        self.bits == TAG_TRUE || self.bits == TAG_FALSE
    }

    /// Check if this is an int32
    #[inline]
    pub fn is_int32(&self) -> bool {
        (self.bits & !INT32_MASK) == INT32_TAG
    }

    /// Check if this is a pointer (object or array)
    #[inline]
    pub fn is_pointer(&self) -> bool {
        (self.bits & !POINTER_MASK) == POINTER_TAG
    }

    /// Check if this is a heap-allocated string pointer
    /// (STRING_TAG only — inline SSO values return false). This is
    /// the legacy predicate that most call sites rely on: they
    /// follow `is_string()` with `as_string_ptr()` assuming a real
    /// `*mut StringHeader`. Keeping this strict avoids a massive
    /// audit during the SSO rollout; use `is_any_string()` when
    /// you want to accept both representations.
    ///
    /// ⚠ #1781 footgun — do NOT write
    /// `if v.is_string() { /* read ptr */ } else { /* treat as pointer
    /// / number / array */ }`. An inline SSO short string (len 0..=5,
    /// `SHORT_STRING_TAG = 0x7FF9`) fails this STRICT check and falls into
    /// the else-branch, where its payload bytes get masked to 48 bits and
    /// dereferenced (SIGSEGV — the fault address spells the string) or
    /// silently produce a wrong result. This blind spot has been patched
    /// piecemeal at least five times (Buffer.from, querystring, str.replace,
    /// js_is_truthy, the #1781 batch). When a value can be *any* runtime
    /// string, branch on [`is_any_string`](Self::is_any_string) +
    /// [`is_short_string`](Self::is_short_string) (decode via
    /// [`short_string_to_buf`](Self::short_string_to_buf)), or route the
    /// whole value through `js_get_string_pointer_unified`, which
    /// materializes SSO bytes onto the heap so downstream `*StringHeader`
    /// code is unchanged. Reading keys out of a `keys_array` is the one
    /// safe exception: stored keys are always heap `STRING_TAG`.
    #[inline]
    pub fn is_string(&self) -> bool {
        (self.bits & !POINTER_MASK) == STRING_TAG
    }

    /// Accepts both heap `STRING_TAG` pointers and inline
    /// `SHORT_STRING_TAG` values. Use this for general "is this a
    /// string?" checks that don't care about representation —
    /// e.g., `typeof x === "string"`, string equality ops, string
    /// concatenation. Paired with `short_string_to_buf()` /
    /// `as_string_ptr()` on the respective branches to read the
    /// data.
    #[inline]
    pub fn is_any_string(&self) -> bool {
        let tag = self.bits & TAG_MASK;
        tag == STRING_TAG || tag == SHORT_STRING_TAG
    }

    /// Check if this is specifically an inline SSO string.
    #[inline]
    pub fn is_short_string(&self) -> bool {
        (self.bits & TAG_MASK) == SHORT_STRING_TAG
    }

    /// Check if this is a BigInt pointer
    #[inline]
    pub fn is_bigint(&self) -> bool {
        (self.bits & !POINTER_MASK) == BIGINT_TAG
    }

    /// Get as f64 (panics if not a number)
    #[inline]
    pub fn as_number(&self) -> f64 {
        debug_assert!(self.is_number(), "Value is not a number");
        f64::from_bits(self.bits)
    }

    /// Get as bool (panics if not a boolean)
    #[inline]
    pub fn as_bool(&self) -> bool {
        debug_assert!(self.is_bool(), "Value is not a boolean");
        self.bits == TAG_TRUE
    }

    /// Get as i32 (panics if not an int32)
    #[inline]
    pub fn as_int32(&self) -> i32 {
        debug_assert!(self.is_int32(), "Value is not an int32");
        (self.bits & INT32_MASK) as i32
    }

    /// Get as pointer (panics if not a pointer)
    #[inline]
    pub fn as_pointer<T>(&self) -> *const T {
        debug_assert!(self.is_pointer(), "Value is not a pointer");
        (self.bits & POINTER_MASK) as *const T
    }

    /// Convert to f64, coercing if necessary
    pub fn to_number(&self) -> f64 {
        if self.is_number() {
            self.as_number()
        } else if self.is_int32() {
            self.as_int32() as f64
        } else if self.is_bool() {
            if self.as_bool() {
                1.0
            } else {
                0.0
            }
        } else if self.is_null() {
            0.0
        } else if self.is_undefined() {
            f64::NAN
        } else {
            // Pointer types would need object-specific conversion
            f64::NAN
        }
    }

    /// Convert to boolean (JS truthiness)
    pub fn to_bool(&self) -> bool {
        if self.is_bool() {
            self.as_bool()
        } else if self.is_number() {
            let n = self.as_number();
            n != 0.0 && !n.is_nan()
        } else if self.is_int32() {
            self.as_int32() != 0
        } else if self.is_null() || self.is_undefined() {
            false
        } else {
            // Pointers (objects) are truthy
            true
        }
    }

    /// Raw bits access (for debugging)
    #[inline]
    pub fn bits(&self) -> u64 {
        self.bits
    }

    /// Create from raw bits
    #[inline]
    pub fn from_bits(bits: u64) -> Self {
        Self { bits }
    }

    /// Create a string pointer value (uses STRING_TAG for type discrimination)
    #[inline]
    pub fn string_ptr(ptr: *mut crate::string::StringHeader) -> Self {
        debug_assert!(
            (ptr as u64) <= POINTER_MASK,
            "Pointer too large for NaN-boxing"
        );
        Self {
            bits: STRING_TAG | (ptr as u64 & POINTER_MASK),
        }
    }

    /// Try to encode a byte slice as an inline SSO string. Returns
    /// `Some(Self)` when `bytes.len() <= SHORT_STRING_MAX_LEN`,
    /// `None` otherwise. Skips all heap allocation on success.
    ///
    /// Semantic note: strings containing U+0000 (the NUL byte) are
    /// fine — the NUL is stored verbatim in one of the 5 data bytes
    /// and the length field is authoritative. Length 0 (the empty
    /// string) is a valid SSO value with no data bytes read.
    #[inline]
    pub fn try_short_string(bytes: &[u8]) -> Option<Self> {
        if bytes.len() > SHORT_STRING_MAX_LEN {
            return None;
        }
        let mut payload: u64 = 0;
        for (i, &b) in bytes.iter().enumerate() {
            payload |= (b as u64) << (i * 8);
        }
        let len_bits = (bytes.len() as u64) << SHORT_STRING_LEN_SHIFT;
        Some(Self {
            bits: SHORT_STRING_TAG | len_bits | payload,
        })
    }

    /// Unconditional SSO constructor. Caller must ensure
    /// `bytes.len() <= SHORT_STRING_MAX_LEN`; debug-build panics on
    /// violation, release-build truncates silently.
    #[inline]
    pub fn short_string_unchecked(bytes: &[u8]) -> Self {
        debug_assert!(bytes.len() <= SHORT_STRING_MAX_LEN);
        Self::try_short_string(bytes).expect("short string must fit SHORT_STRING_MAX_LEN")
    }

    /// Extract the byte contents of an inline SSO string into a
    /// caller-provided buffer of at least `SHORT_STRING_MAX_LEN`
    /// bytes. Returns the actual length. Panics in debug builds if
    /// called on a non-SSO value.
    #[inline]
    pub fn short_string_to_buf(&self, buf: &mut [u8; SHORT_STRING_MAX_LEN]) -> usize {
        debug_assert!(self.is_short_string());
        let len = ((self.bits & SHORT_STRING_LEN_MASK) >> SHORT_STRING_LEN_SHIFT) as usize;
        let data = self.bits & SHORT_STRING_DATA_MASK;
        for i in 0..len {
            buf[i] = ((data >> (i * 8)) & 0xFF) as u8;
        }
        len
    }

    /// Return the length of an SSO string (0..=5).
    #[inline]
    pub fn short_string_len(&self) -> usize {
        debug_assert!(self.is_short_string());
        ((self.bits & SHORT_STRING_LEN_MASK) >> SHORT_STRING_LEN_SHIFT) as usize
    }

    /// Get string pointer (panics if not a string)
    #[inline]
    pub fn as_string_ptr(&self) -> *const crate::string::StringHeader {
        debug_assert!(self.is_string(), "Value is not a string");
        (self.bits & POINTER_MASK) as *const crate::string::StringHeader
    }

    /// Create a BigInt pointer value (uses BIGINT_TAG for type discrimination)
    #[inline]
    pub fn bigint_ptr(ptr: *mut crate::bigint::BigIntHeader) -> Self {
        debug_assert!(
            (ptr as u64) <= POINTER_MASK,
            "Pointer too large for NaN-boxing"
        );
        Self {
            bits: BIGINT_TAG | (ptr as u64 & POINTER_MASK),
        }
    }

    /// Get BigInt pointer (panics if not a BigInt)
    #[inline]
    pub fn as_bigint_ptr(&self) -> *const crate::bigint::BigIntHeader {
        debug_assert!(self.is_bigint(), "Value is not a BigInt");
        (self.bits & POINTER_MASK) as *const crate::bigint::BigIntHeader
    }

    /// Create an object pointer value
    #[inline]
    pub fn object_ptr(ptr: *mut u8) -> Self {
        Self::pointer(ptr)
    }

    /// Create an array pointer value
    #[inline]
    pub fn array_ptr(ptr: *mut crate::array::ArrayHeader) -> Self {
        Self::pointer(ptr as *const u8)
    }
}

impl std::fmt::Debug for JSValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_undefined() {
            write!(f, "undefined")
        } else if self.is_null() {
            write!(f, "null")
        } else if self.is_bool() {
            write!(f, "{}", self.as_bool())
        } else if self.is_number() {
            write!(f, "{}", self.as_number())
        } else if self.is_int32() {
            write!(f, "{}i", self.as_int32())
        } else if self.is_pointer() {
            write!(f, "<ptr {:p}>", self.as_pointer::<u8>())
        } else {
            write!(f, "<unknown 0x{:016x}>", self.bits)
        }
    }
}

impl Default for JSValue {
    fn default() -> Self {
        Self::undefined()
    }
}
