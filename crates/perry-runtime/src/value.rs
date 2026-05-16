//! JSValue representation using NaN-boxing
//!
//! NaN-boxing is a technique that encodes type information and values
//! in a 64-bit float. IEEE 754 double-precision floats have a specific
//! bit pattern for NaN (Not a Number), and we can use the unused bits
//! in the NaN payload to store pointers or small values.
//!
//! Layout (64 bits):
//! - Regular f64 values (including NaN) are stored directly
//! - Tagged values use a quiet NaN pattern (mantissa bit 51 set), with the
//!   tag in the top 16 bits and the payload in the low 48 bits. Per IEEE
//!   754 §6.2.1, a NaN is quiet iff mantissa bit 51 is set; every tag
//!   prefix Perry uses (0x7FF8..=0x7FFF) has that bit set, so all tagged
//!   values are qNaN. Quiet matters because arithmetic on qNaN propagates
//!   silently (`undefined + 1 -> NaN`) whereas sNaN would trap the FPU.
//!
//! We use the top 16 bits for tagging:
//! - 0x7FF9: short string (SSO, inline 5-byte payload)
//! - 0x7FFA: bigint pointer
//! - 0x7FFC + tag: singleton specials (undefined / null / true / false / hole)
//! - 0x7FFD: object/array pointer (48-bit payload)
//! - 0x7FFE: int32 (low 32 bits)
//! - 0x7FFF: heap string pointer (48-bit payload)
//! - Other: regular f64 (including canonical qNaN 0x7FF8_0000_0000_0000)

/// Tag-marker for the singleton specials (undefined / null / true / false /
/// hole). 0x7FFC chosen so the first two mantissa bits are `11`: that keeps
/// it inside the qNaN encoding space (mantissa bit 51 set) while staying
/// distinct from the canonical qNaN 0x7FF8 the FPU produces from arithmetic
/// like `0/0` — code that wants to tell "Perry tagged" from "real NaN" can
/// gate on `top16 >= 0x7FFC` (see `JSValue::is_number` below).
const TAG_MARKER: u64 = 0x7FFC_0000_0000_0000;

/// Special singleton values
pub(crate) const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;

/// Issue #323: hole sentinel for sparse arrays. Slots in `new Array(n)` are
/// initialized to this value; reads through `js_array_get_f64` translate it
/// back to TAG_UNDEFINED so user code never observes the raw bits, while
/// `Object.keys` and the `in` operator inspect slots directly to distinguish a
/// hole from an explicit `undefined` write. Bits chosen in the same 0x7FFC
/// singleton namespace, distinct from UNDEFINED/NULL/FALSE/TRUE so a NaN-box
/// payload can never be mistaken for a hole.
pub(crate) const TAG_HOLE: u64 = 0x7FFC_0000_0000_0010;

/// Pointer tag: 0x7FFD_XXXX_XXXX_XXXX (48 bits for pointer) - objects/arrays
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
pub(crate) const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// Int32 tag: 0x7FFE_0000_XXXX_XXXX (32 bits for i32)
const INT32_TAG: u64 = 0x7FFE_0000_0000_0000;
const INT32_MASK: u64 = 0x0000_0000_FFFF_FFFF;

/// String pointer tag: 0x7FFF_XXXX_XXXX_XXXX (48 bits for string pointer)
pub(crate) const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;

/// Small String Optimization (SSO) — tier 1 #2 per
/// `docs/memory-perf-roadmap.md`. A string of length 0..=5 bytes
/// encodes inline in the 48-bit NaN-box payload instead of
/// allocating a `StringHeader`. Layout:
///
/// ```text
/// bits  63........48  47.....40  39...32 31..24 23..16 15..8  7..0
///       0x7FF9 tag    length     byte0   byte1  byte2  byte3  byte4
/// ```
///
/// Length in bits 40..=47 (0..=5 — 6 valid values, 3 bits would
/// suffice but we use a full byte for alignment). Data in bits
/// 0..=39 (5 bytes, little-endian by byte index — `byte0` is the
/// first character).
///
/// Why 5 bytes not 6: 6 bytes × 8 bits = 48 bits would fill the
/// entire payload leaving no room for length, forcing us to use 3
/// different tag values for length buckets or a null-terminator
/// convention (which breaks strings containing U+0000). Staying at
/// 5 bytes with one tag keeps decode simple: tag check + 40-bit
/// extract. Covers "id", "name", "age", "true", "false", "null",
/// single-byte ASCII, etc. — a large fraction of real-world JSON
/// keys and short values.
///
/// Strings with length > 5 fall through to the standard heap
/// `StringHeader` path; callers read-side use `is_string()` (which
/// accepts BOTH tags) + `string_bytes()` (which decodes either
/// form to a (ptr, len) slice view).
pub(crate) const SHORT_STRING_TAG: u64 = 0x7FF9_0000_0000_0000;
pub(crate) const SHORT_STRING_LEN_SHIFT: u64 = 40;
// Length byte at bits 40..=47 (byte index 5 from LSB). Not
// 0x00FF_0000_0000_0000 — that would be byte 6, overlapping the
// tag.
pub(crate) const SHORT_STRING_LEN_MASK: u64 = 0x0000_FF00_0000_0000;
// Data bytes at bits 0..=39 (5 bytes, byte indices 0..=4 from LSB).
pub(crate) const SHORT_STRING_DATA_MASK: u64 = 0x0000_00FF_FFFF_FFFF;
pub const SHORT_STRING_MAX_LEN: usize = 5;

/// BigInt pointer tag: 0x7FFA_XXXX_XXXX_XXXX (48 bits for bigint pointer)
const BIGINT_TAG: u64 = 0x7FFA_0000_0000_0000;

/// JS Handle tag: 0x7FFB_XXXX_XXXX_XXXX (48 bits for handle ID)
/// This is used by perry-jsruntime to reference V8 objects
pub(crate) const JS_HANDLE_TAG: u64 = 0x7FFB_0000_0000_0000;
pub(crate) const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;

/// Function pointers for JS handle operations (set by perry-jsruntime)
/// These allow the unified functions to dispatch to JS runtime when needed
use std::sync::atomic::{AtomicPtr, Ordering};

type JsHandleArrayGetFn = extern "C" fn(f64, i32) -> f64;
type JsHandleArrayLengthFn = extern "C" fn(f64) -> i32;
type JsHandleObjectGetPropertyFn = extern "C" fn(f64, *const i8, usize) -> f64;
type JsHandleToStringFn = extern "C" fn(f64) -> *mut crate::string::StringHeader;
type JsHandleCallMethodFn = unsafe extern "C" fn(f64, *const i8, usize, *const f64, usize) -> f64;
type JsNativeModuleJsLoaderFn = unsafe extern "C" fn(*const u8, usize, *const u8, usize) -> f64;
type JsNewFromHandleV8Fn = unsafe extern "C" fn(f64, *const f64, usize) -> f64;
/// Returns the JS spec `typeof` string discriminator for a V8 handle:
/// 1 = "function" (V8 callable), 0 = "object" (everything else — including arrays).
/// Negative values reserved for future use ("symbol" = 2 if V8 ever exposes it that way).
type JsHandleTypeofFn = unsafe extern "C" fn(f64) -> i32;

static JS_HANDLE_ARRAY_GET: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static JS_HANDLE_ARRAY_LENGTH: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static JS_HANDLE_OBJECT_GET_PROPERTY: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static JS_HANDLE_TO_STRING: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
pub static JS_HANDLE_CALL_METHOD: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
pub static JS_NATIVE_MODULE_JS_LOADER: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
pub static JS_NEW_FROM_HANDLE_V8: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
pub static JS_HANDLE_TYPEOF: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Set the JS handle array get function (called by perry-jsruntime)
#[no_mangle]
pub extern "C" fn js_set_handle_array_get(func: JsHandleArrayGetFn) {
    JS_HANDLE_ARRAY_GET.store(func as *mut (), Ordering::SeqCst);
}

/// Set the JS handle array length function (called by perry-jsruntime)
#[no_mangle]
pub extern "C" fn js_set_handle_array_length(func: JsHandleArrayLengthFn) {
    JS_HANDLE_ARRAY_LENGTH.store(func as *mut (), Ordering::SeqCst);
}

/// Set the JS handle object get property function (called by perry-jsruntime)
#[no_mangle]
pub extern "C" fn js_set_handle_object_get_property(func: JsHandleObjectGetPropertyFn) {
    JS_HANDLE_OBJECT_GET_PROPERTY.store(func as *mut (), Ordering::SeqCst);
}

/// Set the JS handle to string conversion function (called by perry-jsruntime)
#[no_mangle]
pub extern "C" fn js_set_handle_to_string(func: JsHandleToStringFn) {
    JS_HANDLE_TO_STRING.store(func as *mut (), Ordering::SeqCst);
}

/// Set the JS handle method call function (called by perry-jsruntime)
#[no_mangle]
pub extern "C" fn js_set_handle_call_method(func: JsHandleCallMethodFn) {
    JS_HANDLE_CALL_METHOD.store(func as *mut (), Ordering::SeqCst);
}

/// Set the native module JS property loader (called by perry-jsruntime)
/// This callback loads a native module via V8 and gets a property from it.
#[no_mangle]
pub extern "C" fn js_set_native_module_js_loader(func: JsNativeModuleJsLoaderFn) {
    JS_NATIVE_MODULE_JS_LOADER.store(func as *mut (), Ordering::SeqCst);
}

/// Set the V8 new-from-handle function (called by perry-jsruntime)
/// This callback calls V8's new_instance for JS handle constructors.
#[no_mangle]
pub extern "C" fn js_set_new_from_handle_v8(func: JsNewFromHandleV8Fn) {
    JS_NEW_FROM_HANDLE_V8.store(func as *mut (), Ordering::SeqCst);
}

/// Set the V8 handle typeof discriminator (called by perry-jsruntime).
/// Used by `js_value_typeof` so `typeof someJsFunction` returns `"function"`
/// instead of `"object"` when the handle wraps a V8 callable. (Issue #258.)
#[no_mangle]
pub extern "C" fn js_set_handle_typeof(func: JsHandleTypeofFn) {
    JS_HANDLE_TYPEOF.store(func as *mut (), Ordering::SeqCst);
}

/// Probe a V8 handle's JS `typeof` discriminator. Returns 1 for `"function"`,
/// 0 for `"object"`, and 0 if the V8 callback hasn't been registered (no V8 →
/// fall through to the default "object" classification). Internal helper for
/// `js_value_typeof`.
#[inline]
pub(crate) fn js_handle_is_function(value: f64) -> bool {
    let ptr = JS_HANDLE_TYPEOF.load(Ordering::Relaxed);
    if ptr.is_null() {
        return false;
    }
    let func: JsHandleTypeofFn = unsafe { std::mem::transmute(ptr) };
    unsafe { func(value) == 1 }
}

/// Get element from a JS handle array. Dispatches through the function pointer
/// set by perry-jsruntime, or returns TAG_UNDEFINED if JS runtime not loaded.
#[no_mangle]
pub extern "C" fn js_handle_array_get(array_handle: f64, index: i32) -> f64 {
    let ptr = JS_HANDLE_ARRAY_GET.load(Ordering::Relaxed);
    if ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let func: JsHandleArrayGetFn = unsafe { std::mem::transmute(ptr) };
    func(array_handle, index)
}

/// Get length of a JS handle array. Dispatches through the function pointer
/// set by perry-jsruntime, or returns 0 if JS runtime not loaded.
#[no_mangle]
pub extern "C" fn js_handle_array_length(array_handle: f64) -> i32 {
    let ptr = JS_HANDLE_ARRAY_LENGTH.load(Ordering::Relaxed);
    if ptr.is_null() {
        return 0;
    }
    let func: JsHandleArrayLengthFn = unsafe { std::mem::transmute(ptr) };
    func(array_handle)
}

/// Try to load a property from a native module via V8 JS runtime.
/// Returns TAG_UNDEFINED if JS runtime is not available or property not found.
pub fn native_module_try_js_property(module_name: &str, property_name: &str) -> f64 {
    let loader_ptr = JS_NATIVE_MODULE_JS_LOADER.load(Ordering::Relaxed);
    if loader_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let loader: JsNativeModuleJsLoaderFn = unsafe { std::mem::transmute(loader_ptr) };
    unsafe {
        loader(
            module_name.as_ptr(),
            module_name.len(),
            property_name.as_ptr(),
            property_name.len(),
        )
    }
}

/// Check if a NaN-boxed value is a JS handle
#[inline]
pub fn is_js_handle(value: f64) -> bool {
    let bits = value.to_bits();
    (bits & TAG_MASK) == JS_HANDLE_TAG
}

/// A JavaScript value using NaN-boxing representation
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct JSValue {
    bits: u64,
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
        // A value is a number if upper 16 bits are not in our tagged range 0x7FFC-0x7FFF
        // This allows IEEE NaN (0x7FF8), negative numbers, and all other f64 through
        let upper = self.bits >> 48;
        !(0x7FFC..=0x7FFF).contains(&upper)
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

// FFI functions for creating NaN-boxed values from raw pointers

/// Create a NaN-boxed pointer value from an i64 raw pointer.
/// Returns the value as f64 for storage in union-typed variables.
/// If the value already has a NaN-box tag (JS_HANDLE, STRING, POINTER, etc.),
/// it is preserved as-is to prevent tag corruption.
#[no_mangle]
pub extern "C" fn js_nanbox_pointer(ptr: i64) -> f64 {
    // Guard: null pointer (ptr == 0) must NOT produce null POINTER_TAG (0x7FFD_0000_0000_0000).
    // Null POINTER_TAG causes crashes when code tries to dereference it as a real object pointer.
    if ptr == 0 {
        return f64::from_bits(TAG_NULL);
    }
    let bits = ptr as u64;
    // If value already has a NaN-box tag (top bits in NaN range), preserve it
    if bits & 0xFFF0_0000_0000_0000 >= 0x7FF0_0000_0000_0000 {
        return f64::from_bits(bits);
    }
    let jsval = JSValue::pointer(ptr as *const u8);
    f64::from_bits(jsval.bits())
}

/// Create a NaN-boxed string pointer value from an i64 raw pointer.
/// Returns the value as f64 for storage in union-typed variables.
/// This uses STRING_TAG (0x7FFF) to distinguish from object pointers.
/// If ptr is null, returns a NaN-boxed empty string to prevent null
/// dereference when callers access .length on the result.
#[no_mangle]
pub extern "C" fn js_nanbox_string(ptr: i64) -> f64 {
    let actual_ptr = if ptr == 0 {
        // Allocate an empty string instead of boxing null
        crate::string::js_string_from_bytes(b"".as_ptr(), 0) as i64
    } else {
        ptr
    };
    let jsval = JSValue::string_ptr(actual_ptr as *mut crate::string::StringHeader);
    f64::from_bits(jsval.bits())
}

/// Debug checkpoint function: prints checkpoint number to stderr.
/// Used to narrow down crash locations in generated code.
#[no_mangle]
pub extern "C" fn js_checkpoint(n: i32) {
    use std::io::Write;
    let mut stderr = std::io::stderr();
    let _ = writeln!(stderr, "[CHECKPOINT] {}", n);
    let _ = stderr.flush();
}

/// Debug: print a value's raw bits to stderr (for diagnosing NaN-boxing issues)
#[no_mangle]
pub extern "C" fn js_debug_val(label: i32, val: f64) {
    use std::io::Write;
    let bits = val.to_bits();
    let _ = writeln!(
        std::io::stderr(),
        "[DEBUG_VAL] label={} bits=0x{:016X} f64={}",
        label,
        bits,
        val
    );
    let _ = std::io::stderr().flush();
}

/// Create a NaN-boxed BigInt pointer value from an i64 raw pointer.
/// Returns the value as f64 for storage in union-typed variables.
/// This uses BIGINT_TAG (0x7FFA) to distinguish from other pointer types.
#[no_mangle]
pub extern "C" fn js_nanbox_bigint(ptr: i64) -> f64 {
    let jsval = JSValue::bigint_ptr(ptr as *mut crate::bigint::BigIntHeader);
    f64::from_bits(jsval.bits())
}

// ======================================================================
// Dynamic arithmetic dispatch: handles BigInt vs float at runtime.
// When a parameter has Type::Any (is_union=true), it may hold a BigInt
// (NaN-boxed with BIGINT_TAG) or a regular f64. These functions check
// the NaN-box tag at runtime and dispatch to the correct operation.
// ======================================================================

/// Convert a NaN-boxed JSValue to a *mut BigIntHeader for arithmetic.
/// If the value is already a BigInt, extracts the pointer.
/// Otherwise allocates a new BigInt from the f64 value.
#[inline]
unsafe fn coerce_to_bigint_ptr(val: f64) -> *mut crate::bigint::BigIntHeader {
    let jsval = JSValue::from_bits(val.to_bits());
    if jsval.is_bigint() {
        jsval.as_bigint_ptr() as *mut _
    } else {
        crate::bigint::js_bigint_from_f64(val)
    }
}

/// Dynamic multiply: BigInt * BigInt if either operand is BigInt, else f64 * f64.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_mul(a: f64, b: f64) -> f64 {
    let a_val = JSValue::from_bits(a.to_bits());
    let b_val = JSValue::from_bits(b.to_bits());
    if a_val.is_bigint() || b_val.is_bigint() {
        let a_ptr = coerce_to_bigint_ptr(a) as *const _;
        let b_ptr = coerce_to_bigint_ptr(b) as *const _;
        let result = crate::bigint::js_bigint_mul(a_ptr, b_ptr);
        return js_nanbox_bigint(result as i64);
    }
    a * b
}

/// Dynamic add: BigInt + BigInt if either operand is BigInt, else f64 + f64.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_add(a: f64, b: f64) -> f64 {
    let a_val = JSValue::from_bits(a.to_bits());
    let b_val = JSValue::from_bits(b.to_bits());
    if a_val.is_bigint() || b_val.is_bigint() {
        let result = crate::bigint::js_bigint_add(
            coerce_to_bigint_ptr(a) as *const _,
            coerce_to_bigint_ptr(b) as *const _,
        );
        return js_nanbox_bigint(result as i64);
    }
    a + b
}

/// Dynamic `a + b` for type-uncertain operands. Per JS spec, when either
/// operand is a string after ToPrimitive, the result is string concatenation;
/// otherwise both operands are coerced to numbers and summed (or BigInt-
/// summed when either is BigInt). The codegen dispatches here for `+` when
/// neither operand has a statically-known type — refs #486 (hono's
/// `Node.buildRegExpStr` does `k + c.buildRegExpStr()` inside a for-of loop
/// over `Object.keys(...)` results, both operands lower to plain f64s with
/// inferred type Any, the static-string-concat fast path doesn't fire, and
/// the previous fallback called `js_number_coerce` on each side and `fadd`d
/// the results — turning `"c" + ""` into `NaN + 0 = NaN`).
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_string_or_number_add(a: f64, b: f64) -> f64 {
    let a_val = JSValue::from_bits(a.to_bits());
    let b_val = JSValue::from_bits(b.to_bits());

    // String concat takes priority: either operand being a string forces
    // ToPrimitive on the other side via the spec's "if either is a string,
    // do concat" branch. js_string_concat_value handles the
    // `string + non-string` case (it calls js_jsvalue_to_string on the
    // non-string side); we use it for both orderings by pre-coercing the
    // other operand to string via js_jsvalue_to_string when it ISN'T a
    // string.
    if a_val.is_any_string() || b_val.is_any_string() {
        let a_str = if a_val.is_any_string() {
            js_get_string_pointer_unified(a) as *mut crate::string::StringHeader
        } else {
            js_jsvalue_to_string(a)
        };
        let b_str = if b_val.is_any_string() {
            js_get_string_pointer_unified(b) as *mut crate::string::StringHeader
        } else {
            js_jsvalue_to_string(b)
        };
        let result = crate::string::js_string_concat(a_str, b_str);
        return f64::from_bits(JSValue::string_ptr(result).bits());
    }

    // BigInt: same as js_dynamic_add.
    if a_val.is_bigint() || b_val.is_bigint() {
        let result = crate::bigint::js_bigint_add(
            coerce_to_bigint_ptr(a) as *const _,
            coerce_to_bigint_ptr(b) as *const _,
        );
        return js_nanbox_bigint(result as i64);
    }

    // Both numeric — coerce non-numbers (booleans, null, undefined) the
    // same way the static fallback path did.
    let a_num = if a_val.is_number() || a_val.is_int32() {
        a
    } else {
        crate::builtins::js_number_coerce(a)
    };
    let b_num = if b_val.is_number() || b_val.is_int32() {
        b
    } else {
        crate::builtins::js_number_coerce(b)
    };
    a_num + b_num
}

/// Dynamic subtract: BigInt - BigInt if either operand is BigInt, else f64 - f64.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_sub(a: f64, b: f64) -> f64 {
    let a_val = JSValue::from_bits(a.to_bits());
    let b_val = JSValue::from_bits(b.to_bits());
    if a_val.is_bigint() || b_val.is_bigint() {
        let result = crate::bigint::js_bigint_sub(
            coerce_to_bigint_ptr(a) as *const _,
            coerce_to_bigint_ptr(b) as *const _,
        );
        return js_nanbox_bigint(result as i64);
    }
    a - b
}

/// Dynamic divide: BigInt / BigInt if either operand is BigInt, else f64 / f64.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_div(a: f64, b: f64) -> f64 {
    let a_val = JSValue::from_bits(a.to_bits());
    let b_val = JSValue::from_bits(b.to_bits());
    if a_val.is_bigint() || b_val.is_bigint() {
        let result = crate::bigint::js_bigint_div(
            coerce_to_bigint_ptr(a) as *const _,
            coerce_to_bigint_ptr(b) as *const _,
        );
        return js_nanbox_bigint(result as i64);
    }
    a / b
}

/// Dynamic modulo: BigInt % BigInt if either operand is BigInt, else f64 % f64.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_mod(a: f64, b: f64) -> f64 {
    let a_val = JSValue::from_bits(a.to_bits());
    let b_val = JSValue::from_bits(b.to_bits());
    if a_val.is_bigint() || b_val.is_bigint() {
        let result = crate::bigint::js_bigint_mod(
            coerce_to_bigint_ptr(a) as *const _,
            coerce_to_bigint_ptr(b) as *const _,
        );
        return js_nanbox_bigint(result as i64);
    }
    // Float modulo: a - trunc(a / b) * b
    a - (a / b).trunc() * b
}

/// Dynamic negate: -BigInt if operand is BigInt, else -f64.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_neg(a: f64) -> f64 {
    let a_val = JSValue::from_bits(a.to_bits());
    if a_val.is_bigint() {
        let result = crate::bigint::js_bigint_neg(a_val.as_bigint_ptr());
        return js_nanbox_bigint(result as i64);
    }
    -a
}

/// Dynamic right shift: BigInt >> if either operand is BigInt, else i32 >> for numbers.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_shr(a: f64, b: f64) -> f64 {
    let a_val = JSValue::from_bits(a.to_bits());
    let b_val = JSValue::from_bits(b.to_bits());
    if a_val.is_bigint() || b_val.is_bigint() {
        let result = crate::bigint::js_bigint_shr(
            coerce_to_bigint_ptr(a) as *const _,
            coerce_to_bigint_ptr(b) as *const _,
        );
        return js_nanbox_bigint(result as i64);
    }
    // JS ToInt32: f64 -> i64 -> i32 (wrapping), NOT f64 -> i32 (saturating).
    // Rust `f64 as i32` saturates at i32::MAX for values >= 2^31, but JS wraps.
    let ai = (a as i64) as i32;
    let bi = ((b as i64) as i32) & 0x1f;
    (ai >> bi) as f64
}

/// Dynamic left shift: BigInt << if either operand is BigInt, else i32 << for numbers.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_shl(a: f64, b: f64) -> f64 {
    let a_val = JSValue::from_bits(a.to_bits());
    let b_val = JSValue::from_bits(b.to_bits());
    if a_val.is_bigint() || b_val.is_bigint() {
        let result = crate::bigint::js_bigint_shl(
            coerce_to_bigint_ptr(a) as *const _,
            coerce_to_bigint_ptr(b) as *const _,
        );
        return js_nanbox_bigint(result as i64);
    }
    // JS ToInt32: f64 -> i64 -> i32 (wrapping), NOT f64 -> i32 (saturating).
    let ai = (a as i64) as i32;
    let bi = ((b as i64) as i32) & 0x1f;
    (ai << bi) as f64
}

/// Dynamic bitwise AND: BigInt & if either operand is BigInt, else i32 & for numbers.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_bitand(a: f64, b: f64) -> f64 {
    let a_val = JSValue::from_bits(a.to_bits());
    let b_val = JSValue::from_bits(b.to_bits());
    if a_val.is_bigint() || b_val.is_bigint() {
        let result = crate::bigint::js_bigint_and(
            coerce_to_bigint_ptr(a) as *const _,
            coerce_to_bigint_ptr(b) as *const _,
        );
        return js_nanbox_bigint(result as i64);
    }
    // JS ToInt32: f64 -> i64 -> i32 (wrapping), NOT f64 -> i32 (saturating).
    (((a as i64) as i32) & ((b as i64) as i32)) as f64
}

/// Dynamic bitwise OR: BigInt | if either operand is BigInt, else i32 | for numbers.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_bitor(a: f64, b: f64) -> f64 {
    let a_val = JSValue::from_bits(a.to_bits());
    let b_val = JSValue::from_bits(b.to_bits());
    if a_val.is_bigint() || b_val.is_bigint() {
        let result = crate::bigint::js_bigint_or(
            coerce_to_bigint_ptr(a) as *const _,
            coerce_to_bigint_ptr(b) as *const _,
        );
        return js_nanbox_bigint(result as i64);
    }
    // JS ToInt32: f64 -> i64 -> i32 (wrapping), NOT f64 -> i32 (saturating).
    (((a as i64) as i32) | ((b as i64) as i32)) as f64
}

/// Dynamic bitwise XOR: BigInt ^ if either operand is BigInt, else i32 ^ for numbers.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_bitxor(a: f64, b: f64) -> f64 {
    let a_val = JSValue::from_bits(a.to_bits());
    let b_val = JSValue::from_bits(b.to_bits());
    if a_val.is_bigint() || b_val.is_bigint() {
        let result = crate::bigint::js_bigint_xor(
            coerce_to_bigint_ptr(a) as *const _,
            coerce_to_bigint_ptr(b) as *const _,
        );
        return js_nanbox_bigint(result as i64);
    }
    // JS ToInt32: f64 -> i64 -> i32 (wrapping), NOT f64 -> i32 (saturating).
    (((a as i64) as i32) ^ ((b as i64) as i32)) as f64
}

/// Check if an f64 value (interpreted as NaN-boxed) represents a BigInt.
#[no_mangle]
pub extern "C" fn js_nanbox_is_bigint(value: f64) -> i32 {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_bigint() {
        1
    } else {
        0
    }
}

/// Extract a BigInt pointer from a NaN-boxed f64 value.
/// Returns the pointer as i64.
#[no_mangle]
pub extern "C" fn js_nanbox_get_bigint(value: f64) -> i64 {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);
    if jsval.is_bigint() {
        return jsval.as_bigint_ptr() as i64;
    }
    if value.is_nan() {
        return 0;
    }
    bits as i64
}

/// Check if an f64 value (interpreted as NaN-boxed) represents a pointer.
#[no_mangle]
pub extern "C" fn js_nanbox_is_pointer(value: f64) -> i32 {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_pointer() {
        1
    } else {
        0
    }
}

/// Extract a pointer from a NaN-boxed f64 value.
/// Also handles raw pointer bits (bitcast from i64) for backward compatibility.
/// Handles POINTER_TAG, STRING_TAG, BIGINT_TAG, and JS_HANDLE_TAG.
/// Returns the pointer as i64.
#[no_mangle]
pub extern "C" fn js_nanbox_get_pointer(value: f64) -> i64 {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);

    if jsval.is_pointer() {
        return jsval.as_pointer::<u8>() as i64;
    }

    if jsval.is_string() {
        return jsval.as_string_ptr() as i64;
    }

    if jsval.is_bigint() {
        return jsval.as_bigint_ptr() as i64;
    }

    // JS_HANDLE_TAG (0x7FFB): used for V8 handles and Perry UI widget handles
    // when values pass through inline_nanbox_pointer's "already tagged" path.
    if (bits & TAG_MASK) == JS_HANDLE_TAG {
        return (bits & POINTER_MASK) as i64;
    }

    if bits != 0 && bits <= POINTER_MASK {
        let upper = bits >> 48;
        if upper == 0 || (upper > 0 && upper < 0x7FF0) {
            return bits as i64;
        }
    }

    0
}

/// Returns the pointer as i64.
#[no_mangle]
pub extern "C" fn js_nanbox_get_string_pointer(value: f64) -> i64 {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_string() {
        jsval.as_string_ptr() as i64
    } else {
        0
    }
}

/// Extract a string pointer from an f64 value that may be either:
/// 1. A properly NaN-boxed string (with STRING_TAG)
/// 2. A raw pointer bitcast to f64 (for locally-created strings)
/// This unified function handles both cases for function parameters.
#[no_mangle]
pub extern "C" fn js_get_string_pointer_unified(value: f64) -> i64 {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);

    // Check if it's a properly NaN-boxed string (STRING_TAG = 0x7FFF)
    if jsval.is_string() {
        return jsval.as_string_ptr() as i64;
    }

    // SSO inline value (SHORT_STRING_TAG = 0x7FF9) — caller wants a
    // `*const StringHeader`, so materialize the inline bytes onto the
    // heap. Pre-fix this fell through every branch (SSO bits are NaN
    // so the raw-pointer / number-to-string fallbacks rejected it),
    // returned 0, and any consumer that did
    // `js_string_equals(handle_a, handle_b)` saw "one side is null
    // → not equal" — which is why `JSON.parse(...).foo === "perry"`
    // returned false (SSO === heap string mixed compare). Materialize
    // here defeats the SSO win for the comparison path but is the
    // smallest-blast-radius correctness fix; future codegen sites can
    // avoid the alloc by routing through `js_jsvalue_equals` directly.
    if jsval.is_short_string() {
        return crate::string::js_string_materialize_to_heap(value) as i64;
    }

    // Check if it's a POINTER_TAG (0x7FFD) NaN-boxed pointer (used for cross-module returns)
    if jsval.is_pointer() {
        return (bits & 0x0000_FFFF_FFFF_FFFF) as i64;
    }

    // Raw pointer fallback: only accept values that look like valid heap pointers.
    // Must be non-NaN, non-zero, within 48-bit address space, AND at least 4-byte aligned.
    // The alignment check prevents subnormal f64 numbers like 2.16e-314 (bits=0x1100000003)
    // from being misidentified as pointers.
    if !value.is_nan() && bits != 0 && bits < 0x0001_0000_0000_0000 {
        // Must be at least 4-byte aligned (StringHeader starts with u32 length)
        // and above minimum heap address
        if (bits & 0x3) == 0 && bits >= 0x10000 {
            return bits as i64;
        }
    }

    // For numeric values used as property keys (e.g., obj[pool.id], obj[Direction.Up]),
    // convert the number to a string representation.
    // Note: 0.0 (bits == 0) is a valid number that should produce "0", so we must
    // NOT skip it. The bits != 0 guard above is only for the raw-pointer fallback.
    if !value.is_nan() {
        let s = crate::string::js_number_to_string(value);
        if !s.is_null() {
            return s as i64;
        }
    }

    0
}

/// Check if a NaN-boxed f64 value represents a string.
#[no_mangle]
pub extern "C" fn js_nanbox_is_string(value: f64) -> i32 {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_string() {
        1
    } else {
        0
    }
}

/// Tag-aware dynamic index dispatch for `obj[key]` where `obj` has unknown
/// static type. Issue #514. Strings → js_string_char_at; everything else
/// uses the same `raw_ptr + 8 + idx*8` direct-read offset hack the existing
/// IndexGet fallback uses (which happens to be load-bearing for
/// Object-with-numeric-keys + TypedArrays). LAZY_ARRAY / FORWARDED arrays
/// route through `js_array_get_f64` to chase the materialized chain.
#[no_mangle]
pub extern "C" fn js_dyn_index_get(value: f64, index: f64) -> f64 {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);
    if jsval.is_string() || jsval.is_short_string() {
        let s_ptr = js_get_string_pointer_unified(value) as *const crate::StringHeader;
        if s_ptr.is_null() {
            return f64::from_bits(TAG_UNDEFINED);
        }
        let idx_i32 = if index.is_nan() || index.is_infinite() {
            0
        } else {
            index as i32
        };
        let result = crate::string::js_string_char_at(s_ptr, idx_i32);
        if result.is_null() {
            return f64::from_bits(TAG_UNDEFINED);
        }
        return f64::from_bits(JSValue::string_ptr(result).bits());
    }
    let raw_ptr = if jsval.is_pointer() {
        (bits & POINTER_MASK) as usize
    } else if !value.is_nan()
        && bits != 0
        && bits < 0x0001_0000_0000_0000
        && (bits & 0x3) == 0
        && bits >= 0x10000
    {
        bits as usize
    } else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    let idx_i32 = if index.is_nan() || index.is_infinite() {
        return f64::from_bits(TAG_UNDEFINED);
    } else {
        index as i32
    };
    if idx_i32 < 0 || raw_ptr < 0x10000 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    if raw_ptr >= crate::gc::GC_HEADER_SIZE {
        let gc_hdr = unsafe {
            (raw_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader
        };
        let obj_type = unsafe { (*gc_hdr).obj_type };
        let gc_flags = unsafe { (*gc_hdr).gc_flags };
        if obj_type == crate::gc::GC_TYPE_LAZY_ARRAY
            || (gc_flags & crate::gc::GC_FLAG_FORWARDED) != 0
        {
            let arr = raw_ptr as *const crate::array::ArrayHeader;
            return crate::array::js_array_get_f64(arr, idx_i32 as u32);
        }
    }
    let elem_addr = raw_ptr.wrapping_add(8 + (idx_i32 as usize) * 8);
    let v = unsafe { *(elem_addr as *const f64) };
    if v.to_bits() == crate::value::TAG_HOLE {
        return f64::from_bits(TAG_UNDEFINED);
    }
    v
}

/// Check if a value should trigger a destructuring default.
/// Returns 1 if the value is TAG_UNDEFINED, or a bare IEEE NaN (e.g., from
/// out-of-bounds array read), 0 otherwise. All other NaN-boxed values
/// (strings, pointers, booleans, etc.) return 0 because their NaN payload
/// does not match NaN or TAG_UNDEFINED exactly.
#[no_mangle]
pub extern "C" fn js_is_undefined_or_bare_nan(value: f64) -> i32 {
    let bits = value.to_bits();
    // TAG_UNDEFINED = 0x7FFC_0000_0000_0001
    if bits == 0x7FFC_0000_0000_0001 {
        return 1;
    }
    // Bare IEEE NaN (0.0/0.0) — produced by OOB array reads
    // Canonical NaN is 0x7FF8_0000_0000_0000 on most platforms
    if bits == 0x7FF8_0000_0000_0000 {
        return 1;
    }
    0
}

/// Convert a NaN-boxed f64 value to a string pointer.
/// Handles all value types: strings (extract pointer), numbers (convert), JS handles, etc.
#[no_mangle]
pub extern "C" fn js_jsvalue_to_string(value: f64) -> *mut crate::string::StringHeader {
    // Check for JS handle first - these come from the JS runtime (e.g., process.env values)
    if is_js_handle(value) {
        let func_ptr = JS_HANDLE_TO_STRING.load(Ordering::SeqCst);
        if !func_ptr.is_null() {
            let func: JsHandleToStringFn = unsafe { std::mem::transmute(func_ptr) };
            return func(value);
        }
        // Fallback if no handler registered
        return crate::string::js_string_from_bytes(b"[JS Handle]".as_ptr(), 11);
    }

    let jsval = JSValue::from_bits(value.to_bits());

    if jsval.is_string() {
        // Already a heap string — return the pointer directly.
        jsval.as_string_ptr() as *mut crate::string::StringHeader
    } else if jsval.is_short_string() {
        // Inline SSO — materialize into a heap StringHeader so the
        // caller gets a uniform `*mut StringHeader`. This defeats
        // the SSO benefit for this particular conversion, but it's
        // a correctness-preserving compatibility shim for the many
        // call sites that currently expect a heap pointer.
        crate::string::js_string_materialize_to_heap(value)
    } else if jsval.is_undefined() {
        crate::string::js_string_from_bytes(b"undefined".as_ptr(), 9)
    } else if jsval.is_null() {
        crate::string::js_string_from_bytes(b"null".as_ptr(), 4)
    } else if jsval.is_bool() {
        if jsval.as_bool() {
            crate::string::js_string_from_bytes(b"true".as_ptr(), 4)
        } else {
            crate::string::js_string_from_bytes(b"false".as_ptr(), 5)
        }
    } else if jsval.is_int32() {
        // Convert int32 to string
        let n = jsval.as_int32();
        let s = n.to_string();
        crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
    } else if jsval.is_bigint() {
        // BigInt - convert to decimal string
        let ptr = jsval.as_bigint_ptr();
        crate::bigint::js_bigint_to_string(ptr)
    } else if jsval.is_pointer() {
        // Pointer: could be an array, object, or other heap type. Arrays
        // stringify via `Array.prototype.join(",")` per JS semantics; other
        // objects fall back to "[object Object]".
        let ptr: *const u8 = jsval.as_pointer();
        if !ptr.is_null() && (ptr as usize) >= 0x10000 {
            // Symbols: detect via the side-table before any GC header read.
            if crate::symbol::is_registered_symbol(ptr as usize) {
                return unsafe {
                    crate::symbol::js_symbol_to_string(value) as *mut crate::string::StringHeader
                };
            }
            // Consult `[Symbol.toPrimitive]("string")` if the object has a
            // custom toPrimitive method registered in the symbol side-table.
            // A changed result means the user-defined method produced a
            // string-hint primitive — recurse so strings pass through as-is
            // and numbers get js_number_to_string.
            let primitive = unsafe { crate::symbol::js_to_primitive(value, 2) };
            if primitive.to_bits() != value.to_bits() {
                return js_jsvalue_to_string(primitive);
            }
            // Buffers: BufferHeader has no GC header, so we must detect via
            // BUFFER_REGISTRY before computing gc_header (which would read
            // garbage one word before the buffer). `Buffer.toString()` with
            // no arg defaults to UTF-8 — Node prints the raw bytes.
            if crate::buffer::is_registered_buffer(ptr as usize) {
                return crate::buffer::js_buffer_to_string(
                    ptr as *const crate::buffer::BufferHeader,
                    0,
                );
            }
            unsafe {
                let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
                    // Use js_array_join with a "," separator to match Array.prototype.toString.
                    let sep = crate::string::js_string_from_bytes(b",".as_ptr(), 1);
                    return crate::array::js_array_join(
                        ptr as *const crate::array::ArrayHeader,
                        sep as *const crate::string::StringHeader,
                    );
                }
            }
        }
        crate::string::js_string_from_bytes(b"[object Object]".as_ptr(), 15)
    } else {
        // Regular number - use js_number_to_string
        crate::string::js_number_to_string(value)
    }
}

/// Convert a NaN-boxed f64 value to a string with the given radix.
/// Handles BigInt (uses bigint_to_string_radix), numbers, strings, etc.
#[no_mangle]
pub extern "C" fn js_jsvalue_to_string_radix(
    value: f64,
    radix: i32,
) -> *mut crate::string::StringHeader {
    let jsval = JSValue::from_bits(value.to_bits());

    if jsval.is_bigint() {
        let ptr = jsval.as_bigint_ptr();
        crate::bigint::js_bigint_to_string_radix(ptr, radix)
    } else if jsval.is_string() {
        jsval.as_string_ptr() as *mut crate::string::StringHeader
    } else if jsval.is_int32() {
        let n = jsval.as_int32();
        let s = if radix == 16 {
            format!("{:x}", n)
        } else if radix == 10 || radix == 0 {
            n.to_string()
        } else {
            // General radix conversion
            let mut result = String::new();
            let mut val = if n < 0 { -(n as i64) as u64 } else { n as u64 };
            let r = radix as u64;
            if val == 0 {
                return crate::string::js_string_from_bytes(b"0".as_ptr(), 1);
            }
            while val > 0 {
                let digit = (val % r) as u8;
                result.push(if digit < 10 {
                    (b'0' + digit) as char
                } else {
                    (b'a' + digit - 10) as char
                });
                val /= r;
            }
            if n < 0 {
                result.push('-');
            }
            let s: String = result.chars().rev().collect();
            return crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
        };
        crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
    } else {
        // Regular f64 number
        let n = value;
        if n.is_nan() {
            return crate::string::js_string_from_bytes(b"NaN".as_ptr(), 3);
        }
        if n.is_infinite() {
            if n > 0.0 {
                return crate::string::js_string_from_bytes(b"Infinity".as_ptr(), 8);
            } else {
                return crate::string::js_string_from_bytes(b"-Infinity".as_ptr(), 9);
            }
        }
        if radix == 10 || radix == 0 {
            return crate::string::js_number_to_string(value);
        }
        // For hex and other radixes, convert via integer
        let n_i64 = n as i64;
        let s = if radix == 16 {
            if n_i64 < 0 {
                format!("-{:x}", -n_i64)
            } else {
                format!("{:x}", n_i64)
            }
        } else {
            let mut result = String::new();
            let mut val = if n_i64 < 0 {
                (-n_i64) as u64
            } else {
                n_i64 as u64
            };
            let r = radix as u64;
            if val == 0 {
                return crate::string::js_string_from_bytes(b"0".as_ptr(), 1);
            }
            while val > 0 {
                let digit = (val % r) as u8;
                result.push(if digit < 10 {
                    (b'0' + digit) as char
                } else {
                    (b'a' + digit - 10) as char
                });
                val /= r;
            }
            if n_i64 < 0 {
                result.push('-');
            }
            result.chars().rev().collect()
        };
        crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
    }
}

/// Ensure a value is a native string pointer.
/// This is specifically for fetch headers where we need to handle:
/// 1. Raw string pointers (literal strings - f64 bits ARE the pointer)
/// 2. NaN-boxed strings (STRING_TAG)
/// 3. JS handle strings (from process.env)
/// Returns the string pointer as i64.
#[no_mangle]
pub extern "C" fn js_ensure_string_ptr(value: f64) -> i64 {
    let bits = value.to_bits();

    // Check for JS handle first - these need conversion
    if is_js_handle(value) {
        let func_ptr = JS_HANDLE_TO_STRING.load(Ordering::SeqCst);
        if !func_ptr.is_null() {
            let func: JsHandleToStringFn = unsafe { std::mem::transmute(func_ptr) };
            return func(value) as i64;
        }
        // Fallback - create a placeholder string
        return crate::string::js_string_from_bytes(b"[JS Handle]".as_ptr(), 11) as i64;
    }

    // Check for NaN-boxed string (STRING_TAG)
    if (bits & TAG_MASK) == STRING_TAG {
        let ptr = (bits & POINTER_MASK) as i64;
        if ptr != 0 {
            let str_header = ptr as *const crate::string::StringHeader;
            unsafe {
                let length = (*str_header).byte_len;
                // Make a copy of the string to ensure we have a Perry-allocated string
                let data_ptr = (str_header as *const u8)
                    .add(std::mem::size_of::<crate::string::StringHeader>());
                let copy = crate::string::js_string_from_bytes(data_ptr, length);
                return copy as i64;
            }
        }
        return ptr;
    }

    // Otherwise, treat the f64 bits directly as a pointer (raw string literal)
    bits as i64
}

/// Compare two NaN-boxed f64 values for equality (JavaScript `===` semantics).
/// SameValueZero algorithm (ECMA-262) — used by `Array.prototype.includes`,
/// `Map` / `Set` keys. Same as strict equality except `NaN` is considered
/// equal to itself. Returns 1 if equal, 0 if not.
#[no_mangle]
pub extern "C" fn js_jsvalue_same_value_zero(a: f64, b: f64) -> i32 {
    // NaN-equals-NaN under SameValueZero (the only difference from ===).
    let abits = a.to_bits();
    let bbits = b.to_bits();
    if (abits >> 48) == 0x7FF8 && (bbits >> 48) == 0x7FF8 {
        return 1;
    }
    js_jsvalue_equals(a, b)
}

/// Handles string comparison by comparing actual string contents.
/// Handles BigInt comparison by comparing underlying bigint values (not pointers).
/// Returns 1 if equal, 0 if not.
#[no_mangle]
pub extern "C" fn js_jsvalue_equals(a: f64, b: f64) -> i32 {
    let abits = a.to_bits();
    let bbits = b.to_bits();

    // NaN === NaN is false in JS (strict equality follows IEEE 754).
    // Raw IEEE NaN has top16 = 0x7FF8 (the canonical quiet-NaN). NaN-
    // boxed tagged values use top16 0x7FFA–0x7FFF and never collide.
    // Must come BEFORE the abits==bbits fast path: `[1, NaN, 3].indexOf(NaN)`
    // routes through this helper, both sides decode to f64::NAN (same
    // bit pattern), and pre-fix the fast path returned 1 (wrongly equal)
    // so indexOf reported index 1 instead of -1.
    let a_top16 = abits >> 48;
    let b_top16 = bbits >> 48;
    if a_top16 == 0x7FF8 && b_top16 == 0x7FF8 {
        return 0;
    }

    // Fast path: same bit pattern → equal (same number, same pointer, same boolean, etc.)
    if abits == bbits {
        return 1;
    }

    let a_val = JSValue::from_bits(abits);
    let b_val = JSValue::from_bits(bbits);

    // BigInt comparison: compare by value, not by pointer
    // Two BigInt allocations with the same value must be equal under ===
    if a_val.is_bigint() && b_val.is_bigint() {
        let a_ptr = a_val.as_bigint_ptr();
        let b_ptr = b_val.as_bigint_ptr();
        return crate::bigint::js_bigint_eq(a_ptr, b_ptr);
    }

    // String comparison: compare by content, not by pointer. Must
    // accept both STRING_TAG heap strings and SHORT_STRING_TAG
    // inline SSO values, in any combination.
    if a_val.is_any_string() && b_val.is_any_string() {
        // Fast path: both SSO → identical bits ↔ identical content,
        // because SSO encoding is canonical (same bytes + same
        // length ⇒ same bit pattern).
        if a_val.is_short_string() && b_val.is_short_string() {
            return if abits == bbits { 1 } else { 0 };
        }
        // Decode each side to a (ptr, len) view via a stack scratch
        // buffer for the SSO side; compare by bytes.
        let mut a_scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let mut b_scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let a_view = crate::string::str_bytes_from_jsvalue(a, &mut a_scratch);
        let b_view = crate::string::str_bytes_from_jsvalue(b, &mut b_scratch);
        if let (Some((a_ptr, a_len)), Some((b_ptr, b_len))) = (a_view, b_view) {
            if a_len != b_len {
                return 0;
            }
            if a_len == 0 {
                return 1;
            }
            unsafe {
                let a_slice = std::slice::from_raw_parts(a_ptr, a_len as usize);
                let b_slice = std::slice::from_raw_parts(b_ptr, b_len as usize);
                return if a_slice == b_slice { 1 } else { 0 };
            }
        }
        return 0;
    }

    // Helper: check if bits represent a plain IEEE 754 number (not a NaN-boxed tagged value).
    // NaN-boxing uses tags 0x7FF8-0x7FFF in the upper 16 bits. Regular numbers (positive,
    // negative, zero, infinities) have upper16 outside this range. Negative numbers have
    // sign bit set (upper16 >= 0x8000), so the old check `bits < 0x7FF8...` missed them.
    #[inline(always)]
    fn is_plain_number(bits: u64) -> bool {
        let tag = bits >> 48;
        !(0x7FF8..=0x7FFF).contains(&tag)
    }

    // INT32 comparison: one or both operands may be NaN-boxed INT32 (0x7FFE tag).
    // Convert INT32 to f64 for numeric comparison (e.g., INT32(5) === 5.0 should be true).
    // This mirrors the conversion in js_jsvalue_compare.
    if a_val.is_int32() || b_val.is_int32() {
        let af = if a_val.is_int32() {
            a_val.as_int32() as f64
        } else if is_plain_number(abits) {
            a
        } else {
            return 0;
        }; // non-numeric type → not equal
        let bf = if b_val.is_int32() {
            b_val.as_int32() as f64
        } else if is_plain_number(bbits) {
            b
        } else {
            return 0;
        }; // non-numeric type → not equal
        return if af == bf { 1 } else { 0 };
    }

    // Regular f64 numbers (not NaN-boxed): use IEEE 754 equality
    // This handles -0.0 === 0.0 correctly (both are equal per IEEE 754)
    // Also correctly handles NaN !== NaN (IEEE 754 NaN comparison returns false)
    if is_plain_number(abits) && is_plain_number(bbits) {
        return if a == b { 1 } else { 0 };
    }

    // Different types or different NaN-boxed values → not equal
    0
}

/// JS Abstract Equality Comparison (==).
/// Implements the type coercion rules from ECMA-262 §7.2.14:
/// - null == undefined → true
/// - string == number → ToNumber(string) == number
/// - boolean == anything → ToNumber(boolean) == anything
/// - Same type → strict equality
#[no_mangle]
pub extern "C" fn js_jsvalue_loose_equals(a: f64, b: f64) -> i32 {
    let abits = a.to_bits();
    let bbits = b.to_bits();

    // Fast path: same bit pattern
    if abits == bbits {
        return 1;
    }

    let a_val = JSValue::from_bits(abits);
    let b_val = JSValue::from_bits(bbits);

    // null == undefined (and vice versa)
    let a_null = a_val.is_null() || a_val.is_undefined();
    let b_null = b_val.is_null() || b_val.is_undefined();
    if a_null && b_null {
        return 1;
    }
    // null/undefined != anything else
    if a_null || b_null {
        return 0;
    }

    #[inline(always)]
    fn is_plain_number(bits: u64) -> bool {
        let tag = bits >> 48;
        !(0x7FF8..=0x7FFF).contains(&tag)
    }

    // Helper: convert a JSValue to f64 for numeric comparison
    fn to_number(val: &JSValue, bits: u64, raw: f64) -> Option<f64> {
        if val.is_int32() {
            Some(val.as_int32() as f64)
        } else if is_plain_number(bits) {
            Some(raw)
        } else if val.is_bool() {
            Some(if val.as_bool() { 1.0 } else { 0.0 })
        } else if val.is_string() {
            let ptr = val.as_string_ptr();
            if ptr.is_null() {
                return Some(f64::NAN);
            }
            let header = unsafe { &*ptr };
            let s = unsafe {
                let data =
                    (ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
                std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                    data,
                    header.byte_len as usize,
                ))
            };
            let trimmed = s.trim();
            if trimmed.is_empty() {
                Some(0.0)
            } else {
                trimmed.parse::<f64>().ok()
            }
        } else {
            None
        }
    }

    // If both are same type, delegate to strict equals
    let a_is_num = a_val.is_int32() || is_plain_number(abits);
    let b_is_num = b_val.is_int32() || is_plain_number(bbits);
    let a_is_str = a_val.is_string();
    let b_is_str = b_val.is_string();
    let a_is_bool = a_val.is_bool();
    let b_is_bool = b_val.is_bool();

    // Both strings: strict string comparison
    if a_is_str && b_is_str {
        let a_ptr = a_val.as_string_ptr();
        let b_ptr = b_val.as_string_ptr();
        return crate::string::js_string_equals(a_ptr, b_ptr);
    }

    // Both numbers: numeric comparison
    if a_is_num && b_is_num {
        let af = if a_val.is_int32() {
            a_val.as_int32() as f64
        } else {
            a
        };
        let bf = if b_val.is_int32() {
            b_val.as_int32() as f64
        } else {
            b
        };
        return if af == bf { 1 } else { 0 };
    }

    // Boolean == anything: convert boolean to number, then recurse
    if a_is_bool {
        let a_num = if a_val.as_bool() { 1.0 } else { 0.0 };
        return js_jsvalue_loose_equals(a_num, b);
    }
    if b_is_bool {
        let b_num = if b_val.as_bool() { 1.0 } else { 0.0 };
        return js_jsvalue_loose_equals(a, b_num);
    }

    // String == Number: convert string to number
    if a_is_str && b_is_num {
        if let Some(af) = to_number(&a_val, abits, a) {
            let bf = if b_val.is_int32() {
                b_val.as_int32() as f64
            } else {
                b
            };
            return if af == bf { 1 } else { 0 };
        }
        return 0;
    }
    if a_is_num && b_is_str {
        if let Some(bf) = to_number(&b_val, bbits, b) {
            let af = if a_val.is_int32() {
                a_val.as_int32() as f64
            } else {
                a
            };
            return if af == bf { 1 } else { 0 };
        }
        return 0;
    }

    // BigInt comparisons
    if a_val.is_bigint() && b_val.is_bigint() {
        let a_ptr = a_val.as_bigint_ptr();
        let b_ptr = b_val.as_bigint_ptr();
        return crate::bigint::js_bigint_eq(a_ptr, b_ptr);
    }

    0
}

/// Compare two JSValues for relational ordering (< <= > >=).
/// Returns -1 if a < b, 0 if a == b, 1 if a > b.
/// Handles BigInt, String, Number, and INT32 types.
#[no_mangle]
pub extern "C" fn js_jsvalue_compare(a: f64, b: f64) -> i32 {
    let abits = a.to_bits();
    let bbits = b.to_bits();

    let a_val = JSValue::from_bits(abits);
    let b_val = JSValue::from_bits(bbits);

    // BigInt comparison
    if a_val.is_bigint() && b_val.is_bigint() {
        let a_ptr = a_val.as_bigint_ptr();
        let b_ptr = b_val.as_bigint_ptr();
        return crate::bigint::js_bigint_cmp(a_ptr, b_ptr);
    }

    // String comparison (lexicographic). Accepts SSO in either
    // operand — decode via `str_bytes_from_jsvalue` into stack
    // scratch, then compare slices.
    if a_val.is_any_string() && b_val.is_any_string() {
        let mut a_scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let mut b_scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let a_view = crate::string::str_bytes_from_jsvalue(a, &mut a_scratch);
        let b_view = crate::string::str_bytes_from_jsvalue(b, &mut b_scratch);
        if let (Some((a_ptr, a_len)), Some((b_ptr, b_len))) = (a_view, b_view) {
            if (!a_ptr.is_null() || a_len == 0) && (!b_ptr.is_null() || b_len == 0) {
                unsafe {
                    let a_bytes = std::slice::from_raw_parts(a_ptr, a_len as usize);
                    let b_bytes = std::slice::from_raw_parts(b_ptr, b_len as usize);
                    return match a_bytes.cmp(b_bytes) {
                        std::cmp::Ordering::Less => -1,
                        std::cmp::Ordering::Equal => 0,
                        std::cmp::Ordering::Greater => 1,
                    };
                }
            }
        }
    }

    // INT32 comparison
    if a_val.is_int32() && b_val.is_int32() {
        let ai = a_val.as_int32();
        let bi = b_val.as_int32();
        return if ai < bi {
            -1
        } else if ai > bi {
            1
        } else {
            0
        };
    }

    // Convert to f64 for numeric comparison (handles Number, INT32 mixed with Number, etc.)
    // Return 2 (sentinel) for undefined/null — makes all comparisons false
    // Convert to f64 — use tag check that correctly handles negative numbers
    // (sign bit set → upper16 >= 0x8000, which is > 0x7FFF, not a NaN-box tag)
    let a_tag = abits >> 48;
    let b_tag = bbits >> 48;
    let af = if a_val.is_int32() {
        a_val.as_int32() as f64
    } else if a_val.is_bigint() {
        crate::bigint::js_bigint_to_f64(a_val.as_bigint_ptr())
    } else if !(0x7FF8..=0x7FFF).contains(&a_tag) {
        a
    } else {
        return 2;
    }; // undefined/null/boolean → incomparable sentinel
    let bf = if b_val.is_int32() {
        b_val.as_int32() as f64
    } else if b_val.is_bigint() {
        crate::bigint::js_bigint_to_f64(b_val.as_bigint_ptr())
    } else if !(0x7FF8..=0x7FFF).contains(&b_tag) {
        b
    } else {
        return 2;
    }; // undefined/null/boolean → incomparable sentinel

    if af < bf {
        -1
    } else if af > bf {
        1
    } else {
        0
    }
}

/// Check if a JavaScript value is truthy.
/// In JavaScript, the following values are falsy:
/// - false
/// - 0 (and -0)
/// - NaN
/// - "" (empty string)
/// - null
/// - undefined
/// Everything else is truthy.
/// Returns 1 if truthy, 0 if falsy.
#[no_mangle]
pub extern "C" fn js_is_truthy(value: f64) -> i32 {
    let bits = value.to_bits();

    // Check for special tagged values first
    if bits == TAG_UNDEFINED || bits == TAG_NULL || bits == TAG_FALSE {
        return 0;
    }

    // TAG_TRUE is truthy
    if bits == TAG_TRUE {
        return 1;
    }

    // Check for NaN-boxed string (empty string is falsy)
    if (bits & TAG_MASK) == STRING_TAG {
        let str_ptr = (bits & POINTER_MASK) as *const crate::string::StringHeader;
        if str_ptr.is_null() {
            return 0;
        }
        // Empty string is falsy
        let len = crate::string::js_string_length(str_ptr);
        if len == 0 {
            return 0;
        }
        return 1;
    }

    // Check for NaN-boxed pointer (objects/arrays are always truthy)
    if (bits & TAG_MASK) == POINTER_TAG {
        // Null pointer (0x7FFD_0000_0000_0000) is falsy — like null in JS
        if (bits & POINTER_MASK) == 0 {
            return 0;
        }
        return 1;
    }

    // Check for BigInt (0n is falsy, non-zero is truthy)
    if (bits & !POINTER_MASK) == BIGINT_TAG {
        let ptr = (bits & POINTER_MASK) as *const u8;
        if ptr.is_null() {
            return 0;
        }
        return if crate::bigint::js_bigint_is_zero(ptr as *const crate::bigint::BigIntHeader) != 0 {
            0
        } else {
            1
        };
    }

    // Check for JS handle (always truthy - they represent objects)
    if (bits & TAG_MASK) == JS_HANDLE_TAG {
        return 1;
    }

    // Check for int32 tag
    if (bits & TAG_MASK) == INT32_TAG {
        let int_val = (bits & INT32_MASK) as i32;
        return if int_val == 0 { 0 } else { 1 };
    }

    // Check for raw pointer bits (from bitcast of string literal)
    // In a 64-bit system, valid heap pointers are typically in the range
    // 0x0000_0000_0000_1000 to 0x0000_FFFF_FFFF_FFFF
    // This handles strings that were compiled as direct pointers, not NaN-boxed
    if bits > 0x1000 && bits < 0x0001_0000_0000_0000 {
        // This could be a raw string pointer - check if it's a valid string
        let str_ptr = bits as *const crate::string::StringHeader;
        // Try to read the string length - empty string is falsy
        let len = crate::string::js_string_length(str_ptr);
        if len == 0 {
            return 0;
        }
        return 1;
    }

    // Regular f64 number: 0.0, -0.0, and NaN are falsy
    if value == 0.0 || value.is_nan() {
        return 0;
    }

    // Everything else is truthy
    1
}

/// Dynamic string comparison that handles both NaN-boxed strings and raw pointer bitcasts.
/// This is needed when comparing a PropertyGet result (NaN-boxed) with a string literal (raw bitcast).
/// Returns 1 if equal, 0 if not.
#[no_mangle]
pub extern "C" fn js_dynamic_string_equals(a: f64, b: f64) -> i32 {
    // Extract string pointers from both values, handling both representations
    let a_ptr = extract_string_ptr(a);
    let b_ptr = extract_string_ptr(b);

    if a_ptr.is_null() && b_ptr.is_null() {
        return 1;
    }
    if a_ptr.is_null() || b_ptr.is_null() {
        return 0;
    }

    if crate::string::js_string_equals(a_ptr, b_ptr) != 0 {
        1
    } else {
        0
    }
}

/// Extract a string pointer from an f64 value that might be:
/// - NaN-boxed with STRING_TAG
/// - NaN-boxed with POINTER_TAG (for strings stored as generic pointers)
/// - Raw pointer bits (from bitcast)
fn extract_string_ptr(value: f64) -> *const crate::StringHeader {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);

    // Check for STRING_TAG first (e.g., from PropertyGet)
    if jsval.is_string() {
        return jsval.as_string_ptr();
    }

    // Check for POINTER_TAG (generic pointer that might be a string)
    if jsval.is_pointer() {
        return jsval.as_pointer::<crate::StringHeader>();
    }

    // Assume raw pointer bits (from bitcast of string literal)
    // In a 64-bit system, valid heap pointers are typically in the range
    // 0x0000_0000_0000_0000 to 0x0000_7FFF_FFFF_FFFF
    // Check if it looks like a valid pointer (not NaN, not a small number)
    if bits > 0x1000 && bits < 0x0001_0000_0000_0000 {
        return bits as *const crate::StringHeader;
    }

    std::ptr::null()
}

/// Unified index access that handles strings, arrays, and JS handles.
/// This is called from compiled code when the value type is not known at compile time.
/// For strings, returns the character at the given index as a NaN-boxed string.
/// For arrays, returns the element at the given index.
#[no_mangle]
pub extern "C" fn js_dynamic_array_get(value: f64, index: i32) -> f64 {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);

    // Check if this is a NaN-boxed string
    if jsval.is_string() {
        // String character access
        let str_ptr = jsval.as_string_ptr();
        if !str_ptr.is_null() && index >= 0 {
            let result_ptr = crate::string::js_string_char_at(str_ptr, index);
            if !result_ptr.is_null() {
                // NaN-box the result string pointer
                return f64::from_bits(STRING_TAG | (result_ptr as u64 & POINTER_MASK));
            }
        }
        // Return empty string for invalid index
        let empty = crate::string::js_string_from_bytes(std::ptr::null(), 0);
        return f64::from_bits(STRING_TAG | (empty as u64 & POINTER_MASK));
    }

    // Check if this is a JS handle
    if is_js_handle(value) {
        // Try to use the JS runtime function if it's been registered
        let func_ptr = JS_HANDLE_ARRAY_GET.load(Ordering::SeqCst);
        if !func_ptr.is_null() {
            let func: JsHandleArrayGetFn = unsafe { std::mem::transmute(func_ptr) };
            return func(value, index);
        }
        // JS runtime not available - return undefined
        return f64::from_bits(TAG_UNDEFINED);
    }

    // Not a JS handle - it's a native array/buffer pointer
    let ptr = js_nanbox_get_pointer(value);
    if ptr == 0 {
        // Invalid pointer - return undefined
        return f64::from_bits(TAG_UNDEFINED);
    }

    // Check if this is a buffer (Uint8Array) - read individual bytes, not f64 values
    if crate::buffer::is_registered_buffer(ptr as usize) {
        let byte_val =
            crate::buffer::js_buffer_get(ptr as *const crate::buffer::BufferHeader, index);
        return byte_val as f64;
    }

    // Call the native array get function
    let result_bits =
        crate::array::js_array_get_jsvalue(ptr as *const crate::array::ArrayHeader, index as u32);
    let _result_top16 = result_bits >> 48;
    // debug: DYNAMIC-ARRAY-GET-DEBUG disabled
    f64::from_bits(result_bits)
}

/// Unified array length access that handles both JS handle arrays and native arrays.
#[no_mangle]
pub extern "C" fn js_dynamic_array_length(arr_value: f64) -> i32 {
    let bits = arr_value.to_bits();
    let _top16 = bits >> 48;

    // Check if this is a JS handle
    if is_js_handle(arr_value) {
        let func_ptr = JS_HANDLE_ARRAY_LENGTH.load(Ordering::SeqCst);
        if !func_ptr.is_null() {
            let func: JsHandleArrayLengthFn = unsafe { std::mem::transmute(func_ptr) };
            return func(arr_value);
        }
        return 0;
    }

    // Not a JS handle - extract the pointer
    let ptr = js_nanbox_get_pointer(arr_value);
    if ptr == 0 {
        return 0;
    }

    crate::array::js_array_length(ptr as *const crate::array::ArrayHeader) as i32
}

/// Issue #73: safe `.length` lookup by runtime type. Called from the
/// inline PropertyGet length path when the GC-type-byte check at
/// `handle-8` doesn't prove the receiver is a GC_TYPE_ARRAY or
/// GC_TYPE_STRING. Routes by runtime registry / GC header so that a
/// Named-typed receiver that turns out to hold a Buffer, TypedArray,
/// Closure, Error, number, etc. at runtime returns a sensible length
/// instead of dereferencing garbage at `recv & 0xFFFFFFFFFFFF`.
///
/// Returns a double so the inline caller can phi the fast and slow
/// results without another conversion.
#[no_mangle]
pub extern "C" fn js_value_length_f64(value: f64) -> f64 {
    let bits = value.to_bits();
    let top16 = bits >> 48;

    // SHORT_STRING_TAG (SSO) — length is the byte count stored in
    // bits 40..=47. Fast path, no heap access. For multibyte UTF-8
    // content the byte length and UTF-16 code-unit count differ,
    // but SSO strings are ≤5 bytes and the vast majority are ASCII
    // where they match. Non-ASCII SSO values go through a slower
    // full-parse path — tolerated because the distinction doesn't
    // come up in practice for 5-byte strings.
    if top16 == 0x7FF9 {
        return ((bits & SHORT_STRING_LEN_MASK) >> SHORT_STRING_LEN_SHIFT) as f64;
    }

    // STRING_TAG — length is code-unit count from js_string_length.
    if top16 == 0x7FFF {
        let ptr = (bits & POINTER_MASK) as *const crate::string::StringHeader;
        if ptr.is_null() || (ptr as usize) < 0x10000 {
            return 0.0;
        }
        return crate::string::js_string_length(ptr) as f64;
    }

    // POINTER_TAG — Buffer / TypedArray via registries first (they
    // don't have GC headers — `buffer_alloc` + `typed_array_alloc`
    // use `std::alloc` directly). Falling through to the GC-header
    // path would read mimalloc bookkeeping as obj_type and return
    // nonsense.
    if top16 == 0x7FFD {
        let handle = (bits & POINTER_MASK) as usize;
        // Heap window: Darwin mimalloc lands in 3-5 TB, but Android scudo,
        // Linux glibc, and Windows mimalloc all allocate much lower (often
        // hundreds of GB or less). Using the Darwin-tight 2 TB floor on
        // Android / Windows null-s every real pointer. See clean_arr_ptr
        // for the same platform split.
        #[cfg(any(target_os = "android", target_os = "linux", target_os = "windows"))]
        let heap_min: usize = 0x1000;
        #[cfg(not(any(target_os = "android", target_os = "linux", target_os = "windows")))]
        let heap_min: usize = 0x200_0000_0000;
        if handle < heap_min || handle >= 0x8000_0000_0000 {
            return 0.0;
        }
        if crate::buffer::is_registered_buffer(handle) {
            let buf = handle as *const crate::buffer::BufferHeader;
            return unsafe { (*buf).length as f64 };
        }
        if crate::typedarray::lookup_typed_array_kind(handle).is_some() {
            let ta = handle as *const crate::typedarray::TypedArrayHeader;
            return unsafe { (*ta).length as f64 };
        }
        let gc_header = (handle - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let obj_type = unsafe { (*gc_header).obj_type };
        // Issue #233: a FORWARDED array's first 4 bytes are no longer
        // length but the lower 32 bits of the forwarding pointer.
        // Follow the chain via the unified array-pointer cleaner so
        // `samples.length` after a grow returns the real length.
        if obj_type == crate::gc::GC_TYPE_ARRAY
            && unsafe { (*gc_header).gc_flags } & crate::gc::GC_FLAG_FORWARDED != 0
        {
            let cleaned = crate::array::js_array_get_length(handle as i64);
            return cleaned as f64;
        }
        match obj_type {
            crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_STRING => {
                return unsafe { *(handle as *const u32) } as f64;
            }
            // Issue #179 Phase 2: lazy arrays also have `length` at
            // offset 0 (cached_length). The inline-length codegen
            // only recognizes GC_TYPE_ARRAY/STRING in its check so
            // lazy values land here via the slow path — read the
            // u32 from offset 0 just like regular arrays.
            crate::gc::GC_TYPE_LAZY_ARRAY => {
                return unsafe { *(handle as *const u32) } as f64;
            }
            // Closures, BigInts, Promises, Errors, plain Objects, Maps:
            // no `.length`. Return 0 to match Perry's existing
            // fallback for missing fields (JS would produce
            // `undefined`, but the generic PropertyGet slow path
            // already degrades to 0 here).
            _ => return 0.0,
        }
    }

    // Raw pointer bitcast to f64 (no NaN-box tag — top16 == 0).
    // TypedArrays are allocated via `std::alloc` and the codegen
    // sometimes hands their pointer through as `bitcast i64 → double`
    // without a POINTER_TAG. Without this path, `Int32Array.length`
    // returned 0 because the value's top16 was 0, not 0x7FFD.
    #[cfg(any(target_os = "android", target_os = "linux", target_os = "windows"))]
    let raw_heap_min: u64 = 0x1000;
    #[cfg(not(any(target_os = "android", target_os = "linux", target_os = "windows")))]
    let raw_heap_min: u64 = 0x200_0000_0000;
    if top16 == 0 && bits >= raw_heap_min && bits < 0x8000_0000_0000 {
        let handle = bits as usize;
        if crate::buffer::is_registered_buffer(handle) {
            let buf = handle as *const crate::buffer::BufferHeader;
            return unsafe { (*buf).length as f64 };
        }
        if crate::typedarray::lookup_typed_array_kind(handle).is_some() {
            let ta = handle as *const crate::typedarray::TypedArrayHeader;
            return unsafe { (*ta).length as f64 };
        }
    }

    // Everything else — undefined, null, booleans, int32, plain
    // doubles, BigInt pointers — has no `.length`.
    0.0
}

/// Dynamic array find that handles both JS handle arrays and native arrays.
/// Takes the array as f64 (may be NaN-boxed or JS handle) and a callback closure.
/// Returns the found element as f64, or NaN (undefined) if not found.
#[no_mangle]
pub extern "C" fn js_dynamic_array_find(
    arr_value: f64,
    callback: *const crate::closure::ClosureHeader,
) -> f64 {
    // Check if callback is null
    if callback.is_null() {
        return f64::NAN;
    }

    // Check if this is a JS handle array
    if is_js_handle(arr_value) {
        // For JS handle arrays, iterate using dynamic access
        let length = js_dynamic_array_length(arr_value);
        for i in 0..length {
            let element = js_dynamic_array_get(arr_value, i);
            let result = crate::closure::js_closure_call1(callback, element);
            // Proper truthy check: handles NaN-boxed booleans
            if js_is_truthy(result) != 0 {
                return element;
            }
        }
        // Not found - return undefined (NaN)
        return f64::NAN;
    }

    // Not a JS handle - extract the native array pointer
    let ptr = js_nanbox_get_pointer(arr_value);
    if ptr == 0 {
        return f64::NAN;
    }

    // Use the native array find
    crate::array::js_array_find(ptr as *const crate::array::ArrayHeader, callback)
}

/// Dynamic array findIndex that handles both JS handle arrays and native arrays.
/// Takes the array as f64 (may be NaN-boxed or JS handle) and a callback closure.
/// Returns the index as f64 (-1.0 if not found).
#[no_mangle]
pub extern "C" fn js_dynamic_array_findIndex(
    arr_value: f64,
    callback: *const crate::closure::ClosureHeader,
) -> f64 {
    // Check if this is a JS handle array
    if is_js_handle(arr_value) {
        // For JS handle arrays, iterate using dynamic access
        let length = js_dynamic_array_length(arr_value);
        for i in 0..length {
            let element = js_dynamic_array_get(arr_value, i);
            let result = crate::closure::js_closure_call1(callback, element);
            // Proper truthy check: handles NaN-boxed booleans
            if js_is_truthy(result) != 0 {
                return i as f64;
            }
        }
        // Not found
        return -1.0;
    }

    // Not a JS handle - extract the native array pointer
    let ptr = js_nanbox_get_pointer(arr_value);
    if ptr == 0 {
        return -1.0;
    }

    // Use the native array findIndex and convert to f64
    crate::array::js_array_findIndex(ptr as *const crate::array::ArrayHeader, callback) as f64
}

/// Unified object property access that handles both JS handle objects and native objects.
/// Also handles strings for property access like `.length`.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_object_get_property(
    obj_value: f64,
    property_name_ptr: *const i8,
    property_name_len: usize,
) -> f64 {
    // Check if this is a JS handle
    if is_js_handle(obj_value) {
        // Try to use the JS runtime function if it's been registered
        let func_ptr = JS_HANDLE_OBJECT_GET_PROPERTY.load(Ordering::SeqCst);
        if !func_ptr.is_null() {
            let func: JsHandleObjectGetPropertyFn = unsafe { std::mem::transmute(func_ptr) };
            return func(obj_value, property_name_ptr, property_name_len);
        }
        // JS runtime not available - return undefined
        return f64::from_bits(TAG_UNDEFINED);
    }

    // Check if this is a NaN-boxed string - handle string properties like .length
    let bits = obj_value.to_bits();
    if (bits & TAG_MASK) == STRING_TAG {
        let str_ptr = (bits & POINTER_MASK) as *const crate::string::StringHeader;
        if !str_ptr.is_null() {
            // Get the property name
            let name_slice = if property_name_ptr.is_null() {
                return f64::from_bits(TAG_UNDEFINED);
            } else if property_name_len > 0 {
                std::slice::from_raw_parts(property_name_ptr as *const u8, property_name_len)
            } else {
                std::ffi::CStr::from_ptr(property_name_ptr as *const std::ffi::c_char).to_bytes()
            };

            // Handle string properties
            if name_slice == b"length" {
                let len = crate::string::js_string_length(str_ptr);
                return len as f64;
            }
            // Other string properties return undefined
            return f64::from_bits(TAG_UNDEFINED);
        }
    }

    // Not a JS handle - it's a native object pointer
    let ptr = js_nanbox_get_pointer(obj_value);

    if ptr == 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }

    // Check if this is a handle-based object (small integer, not a real heap pointer)
    if ptr < 0x100000 {
        if let Some(dispatch) = crate::object::HANDLE_PROPERTY_DISPATCH {
            return dispatch(ptr, property_name_ptr as *const u8, property_name_len);
        }
        return f64::from_bits(TAG_UNDEFINED);
    }

    // Get the key string
    let name_slice = if property_name_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    } else if property_name_len > 0 {
        std::slice::from_raw_parts(property_name_ptr as *const u8, property_name_len)
    } else {
        // Null-terminated C string
        std::ffi::CStr::from_ptr(property_name_ptr as *const std::ffi::c_char).to_bytes()
    };

    let property_name = match std::str::from_utf8(name_slice) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(TAG_UNDEFINED),
    };

    // Check if this is a ClosureHeader (CLOSURE_MAGIC at offset 12).
    // ClosureHeader layout: func_ptr (8B), capture_count u32 (4B), type_tag u32 (4B), captures at 16+
    // ObjectHeader layout: object_type u32 (4B), class_id u32 (4B), parent_class_id u32 (4B), field_count u32 (4B), keys_array (8B), ...
    // Without this check, the closure's capture[0] at offset 16 would be read as keys_array → crash.
    if crate::closure::is_closure_ptr(ptr as usize) {
        return crate::closure::closure_get_dynamic_prop(ptr as usize, property_name);
    }

    // Handle Buffer/Uint8Array properties (buffer, byteOffset, byteLength, length)
    // BufferHeader has same layout as ArrayHeader (length u32, capacity u32, data...)
    // and doesn't have ObjectHeader fields, so we must check before treating as ObjectHeader.
    if crate::buffer::is_registered_buffer(ptr as usize) {
        let buf = ptr as *const crate::buffer::BufferHeader;
        match property_name {
            "length" | "byteLength" => {
                return (*buf).length as f64;
            }
            "byteOffset" => {
                return 0.0;
            }
            "buffer" => {
                // Return the buffer itself (Perry doesn't separate ArrayBuffer)
                return obj_value;
            }
            _ => {
                return f64::from_bits(TAG_UNDEFINED);
            }
        }
    }

    // Check if this is a registered Map
    if crate::map::is_registered_map(ptr as usize) {
        let map_ptr = ptr as *const crate::map::MapHeader;
        if name_slice == b"size" {
            return (*map_ptr).size as f64;
        }
        return f64::from_bits(TAG_UNDEFINED);
    }

    // Check if this is a registered Set
    if crate::set::is_registered_set(ptr as usize) {
        let set_ptr = ptr as *const crate::set::SetHeader;
        if name_slice == b"size" {
            return (*set_ptr).size as f64;
        }
        return f64::from_bits(TAG_UNDEFINED);
    }

    // Check the object type tag (first u32 field of both ObjectHeader and ErrorHeader)
    let object_type = *(ptr as *const u32);

    // Handle native module namespace objects (e.g., `const fn = fs.lstatSync`)
    // Create a bound method closure so the method reference can be called later
    let obj_header = ptr as *const crate::object::ObjectHeader;
    if (*obj_header).class_id == crate::object::NATIVE_MODULE_CLASS_ID {
        return crate::object::js_native_module_bind_method(
            obj_value,
            property_name.as_ptr(),
            property_name.len(),
        );
    }

    // Handle Error objects specially
    if object_type == crate::error::OBJECT_TYPE_ERROR {
        let error_ptr = ptr as *mut crate::error::ErrorHeader;
        match property_name {
            "message" => {
                let msg = crate::error::js_error_get_message(error_ptr);
                return js_nanbox_string(msg as i64);
            }
            "name" => {
                let name = crate::error::js_error_get_name(error_ptr);
                return js_nanbox_string(name as i64);
            }
            "stack" => {
                let stack = crate::error::js_error_get_stack(error_ptr);
                return js_nanbox_string(stack as i64);
            }
            "cause" => {
                return crate::error::js_error_get_cause(error_ptr);
            }
            "errors" => {
                let arr = crate::error::js_error_get_errors(error_ptr);
                if arr.is_null() {
                    return f64::from_bits(TAG_UNDEFINED);
                }
                return js_nanbox_pointer(arr as i64);
            }
            _ => {
                // Error objects don't have other properties
                return f64::from_bits(TAG_UNDEFINED);
            }
        }
    }

    // Check vtable for a registered getter or method before falling back to field lookup
    let class_id = (*obj_header).class_id;
    if class_id != 0 {
        if let Ok(registry) = crate::object::CLASS_VTABLE_REGISTRY.read() {
            if let Some(ref reg) = *registry {
                if let Some(vtable) = reg.get(&class_id) {
                    if let Some(&getter_ptr) = vtable.getters.get(property_name) {
                        // Methods take `this` as f64 (NaN-boxed), not i64.
                        // On Windows x64 ABI, i64 and f64 use different registers.
                        let this_f64: f64 = f64::from_bits(ptr as u64);
                        let f: extern "C" fn(f64) -> f64 = std::mem::transmute(getter_ptr);
                        return f(this_f64);
                    }
                    // If the property is a registered method, return truthy so that
                    // `if (obj.method)` works (method existence checks).
                    if vtable.methods.contains_key(property_name) {
                        return f64::from_bits(TAG_TRUE);
                    }
                }
            }
        }
    }

    // Create a Perry string for the key
    let key_ptr =
        crate::string::js_string_from_bytes(property_name.as_ptr(), property_name.len() as u32);

    // Call native object property access

    crate::object::js_object_get_field_by_name_f64(
        ptr as *const crate::object::ObjectHeader,
        key_ptr,
    )
}

/// Dynamic method dispatch for Map/Set collection types.
/// Checks the magic tag of the object and dispatches known methods.
/// Returns TAG_UNDEFINED if the object is not a Map/Set or method is unknown.
/// This handles cases like `map.get(key).add(value)` where the intermediate
/// result type is unknown at codegen time.
#[no_mangle]
pub unsafe extern "C" fn js_collection_method_dispatch(
    obj_value: f64,
    method_ptr: *const u8,
    method_len: usize,
    arg0: f64,
    arg1: f64,
) -> f64 {
    let ptr = js_nanbox_get_pointer(obj_value);
    if ptr == 0 || ptr < 0x10000 {
        return f64::from_bits(TAG_UNDEFINED);
    }

    let method = std::slice::from_raw_parts(method_ptr, method_len);

    // Check if this is a registered Map
    if crate::map::is_registered_map(ptr as usize) {
        let map = ptr as *mut crate::map::MapHeader;
        return match method {
            b"get" => crate::map::js_map_get(map, arg0),
            b"set" => {
                let result = crate::map::js_map_set(map, arg0, arg1);
                js_nanbox_pointer(result as i64)
            }
            b"has" => crate::map::js_map_has(map, arg0) as f64,
            b"delete" => crate::map::js_map_delete(map, arg0) as f64,
            b"size" => crate::map::js_map_size(map) as f64,
            b"clear" => {
                crate::map::js_map_clear(map);
                f64::from_bits(TAG_UNDEFINED)
            }
            b"entries" => {
                let arr = crate::map::js_map_entries(map);
                js_nanbox_pointer(arr as i64)
            }
            b"keys" => {
                let arr = crate::map::js_map_keys(map);
                js_nanbox_pointer(arr as i64)
            }
            b"values" => {
                let arr = crate::map::js_map_values(map);
                js_nanbox_pointer(arr as i64)
            }
            _ => f64::from_bits(TAG_UNDEFINED),
        };
    }

    // Check if this is a registered Set
    if crate::set::is_registered_set(ptr as usize) {
        let set = ptr as *mut crate::set::SetHeader;
        return match method {
            b"add" => {
                let result = crate::set::js_set_add(set, arg0);
                js_nanbox_pointer(result as i64)
            }
            b"has" => crate::set::js_set_has(set, arg0) as f64,
            b"delete" => crate::set::js_set_delete(set, arg0) as f64,
            b"size" => crate::set::js_set_size(set) as f64,
            b"clear" => {
                crate::set::js_set_clear(set);
                f64::from_bits(TAG_UNDEFINED)
            }
            _ => f64::from_bits(TAG_UNDEFINED),
        };
    }

    f64::from_bits(TAG_UNDEFINED)
}

/// Dynamic Object.keys() that handles both regular objects and Error objects.
/// Takes a raw pointer (extracted from NaN-boxed value) and returns array of keys.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_object_keys(ptr: i64) -> *mut crate::array::ArrayHeader {
    if ptr == 0 {
        return crate::array::js_array_alloc(0);
    }

    // Check the object type tag (first u32 field of both ObjectHeader and ErrorHeader)
    let object_type = *(ptr as *const u32);

    // Handle Error objects specially - they have fixed keys
    if object_type == crate::error::OBJECT_TYPE_ERROR {
        // Error objects have keys: "message", "name", "stack"
        let keys = crate::array::js_array_alloc(3);

        let msg_key = crate::string::js_string_from_bytes(b"message".as_ptr(), 7);
        crate::array::js_array_push(keys, JSValue::string_ptr(msg_key));

        let name_key = crate::string::js_string_from_bytes(b"name".as_ptr(), 4);
        crate::array::js_array_push(keys, JSValue::string_ptr(name_key));

        let stack_key = crate::string::js_string_from_bytes(b"stack".as_ptr(), 5);
        crate::array::js_array_push(keys, JSValue::string_ptr(stack_key));

        return keys;
    }

    // Regular object - delegate to js_object_keys
    crate::object::js_object_keys(ptr as *const crate::object::ObjectHeader)
}

/// Get a property from an object by name.
/// This is the main entry point used by codegen for dynamic property access.
/// Delegates to js_dynamic_object_get_property which handles JS handles, native objects,
/// strings, and error objects.
///
/// Parameters:
/// - object: NaN-boxed f64 containing the object
/// - name_ptr: i64 pointer to the property name bytes
/// - name_len: i64 length of the property name
///
/// Returns: NaN-boxed f64 containing the property value (or undefined)
#[no_mangle]
pub unsafe extern "C" fn js_get_property(object: f64, name_ptr: i64, name_len: i64) -> f64 {
    js_dynamic_object_get_property(object, name_ptr as *const i8, name_len as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_undefined() {
        let v = JSValue::undefined();
        assert!(v.is_undefined());
        assert!(!v.is_null());
        assert!(!v.is_number());
    }

    #[test]
    fn test_null() {
        let v = JSValue::null();
        assert!(v.is_null());
        assert!(!v.is_undefined());
    }

    #[test]
    fn test_bool() {
        let t = JSValue::bool(true);
        let f = JSValue::bool(false);
        assert!(t.is_bool());
        assert!(f.is_bool());
        assert!(t.as_bool());
        assert!(!f.as_bool());
    }

    #[test]
    fn test_number() {
        let v = JSValue::number(42.5);
        assert!(v.is_number());
        assert_eq!(v.as_number(), 42.5);

        let zero = JSValue::number(0.0);
        assert!(zero.is_number());
        assert_eq!(zero.as_number(), 0.0);

        let neg = JSValue::number(-123.456);
        assert!(neg.is_number());
        assert_eq!(neg.as_number(), -123.456);
    }

    #[test]
    fn test_int32() {
        let v = JSValue::int32(42);
        assert!(v.is_int32());
        assert_eq!(v.as_int32(), 42);

        let neg = JSValue::int32(-100);
        assert!(neg.is_int32());
        assert_eq!(neg.as_int32(), -100);
    }

    #[test]
    fn test_truthiness() {
        assert!(!JSValue::undefined().to_bool());
        assert!(!JSValue::null().to_bool());
        assert!(!JSValue::bool(false).to_bool());
        assert!(JSValue::bool(true).to_bool());
        assert!(!JSValue::number(0.0).to_bool());
        assert!(JSValue::number(1.0).to_bool());
        assert!(JSValue::number(-1.0).to_bool());
        assert!(!JSValue::number(f64::NAN).to_bool());
    }

    #[test]
    fn test_jsvalue_equals_booleans() {
        let t = f64::from_bits(TAG_TRUE);
        let f = f64::from_bits(TAG_FALSE);
        // Same boolean values
        assert_eq!(js_jsvalue_equals(t, t), 1);
        assert_eq!(js_jsvalue_equals(f, f), 1);
        // Different boolean values
        assert_eq!(js_jsvalue_equals(t, f), 0);
        assert_eq!(js_jsvalue_equals(f, t), 0);
        // Boolean vs number (strict equality: different types)
        assert_eq!(js_jsvalue_equals(t, 1.0), 0);
        assert_eq!(js_jsvalue_equals(f, 0.0), 0);
    }

    #[test]
    fn test_jsvalue_equals_int32() {
        let int5 = f64::from_bits(INT32_TAG | 5);
        let float5 = 5.0f64;
        let int0 = f64::from_bits(INT32_TAG | 0);
        let float0 = 0.0f64;
        let int_neg = f64::from_bits(INT32_TAG | ((-3i32 as u32) as u64));
        let float_neg = -3.0f64;
        // INT32 vs f64 with same numeric value
        assert_eq!(js_jsvalue_equals(int5, float5), 1);
        assert_eq!(js_jsvalue_equals(float5, int5), 1);
        assert_eq!(js_jsvalue_equals(int0, float0), 1);
        assert_eq!(js_jsvalue_equals(int_neg, float_neg), 1);
        // INT32 vs INT32
        assert_eq!(js_jsvalue_equals(int5, int5), 1);
        // INT32 vs different f64
        assert_eq!(js_jsvalue_equals(int5, 6.0), 0);
        assert_eq!(js_jsvalue_equals(int5, 4.0), 0);
    }

    #[test]
    fn test_short_string_encoding_roundtrip() {
        for s in [b"" as &[u8], b"a", b"ab", b"abc", b"abcd", b"abcde"] {
            let v = JSValue::try_short_string(s).unwrap();
            assert!(v.is_short_string(), "tag mismatch for {:?}", s);
            assert!(v.is_any_string(), "is_any_string should accept SSO");
            assert!(!v.is_string(), "legacy is_string should NOT accept SSO");
            assert_eq!(v.short_string_len(), s.len(), "length mismatch for {:?}", s);
            let mut buf = [0u8; SHORT_STRING_MAX_LEN];
            let n = v.short_string_to_buf(&mut buf);
            assert_eq!(n, s.len());
            assert_eq!(&buf[..n], s, "bytes mismatch for {:?}", s);
        }
    }

    #[test]
    fn test_short_string_too_long_rejects() {
        assert!(JSValue::try_short_string(b"abcdef").is_none()); // 6 bytes
        assert!(JSValue::try_short_string(b"hello world").is_none()); // 11 bytes
    }

    #[test]
    fn test_short_string_embedded_nul_ok() {
        // Strings with embedded U+0000 work fine in SSO — length
        // is authoritative, NULs are plain data bytes.
        let s = &[b'a', 0, b'b', 0, b'c'];
        let v = JSValue::try_short_string(s).unwrap();
        assert_eq!(v.short_string_len(), 5);
        let mut buf = [0u8; SHORT_STRING_MAX_LEN];
        let n = v.short_string_to_buf(&mut buf);
        assert_eq!(&buf[..n], s);
    }

    #[test]
    fn test_short_string_tag_distinct_from_others() {
        // Any valid SSO value must not collide with other NaN-box
        // tags. `is_short_string()` is strict — returns false for
        // everything except the SSO tag band.
        let sso = JSValue::try_short_string(b"abcde").unwrap();
        let heap_string = JSValue {
            bits: STRING_TAG | 0x1234,
        };
        let pointer = JSValue {
            bits: POINTER_TAG | 0x5678,
        };
        let int32 = JSValue::int32(42);
        let number = JSValue::number(3.14);
        let undef = JSValue::undefined();
        assert!(sso.is_short_string());
        assert!(!heap_string.is_short_string());
        assert!(!pointer.is_short_string());
        assert!(!int32.is_short_string());
        assert!(!number.is_short_string());
        assert!(!undef.is_short_string());
        // is_any_string accepts both SSO and heap string, rejects others.
        assert!(sso.is_any_string());
        assert!(heap_string.is_any_string());
        assert!(!pointer.is_any_string());
        assert!(!int32.is_any_string());
        assert!(!number.is_any_string());
    }

    #[test]
    fn test_short_string_empty_roundtrip() {
        let v = JSValue::try_short_string(b"").unwrap();
        assert!(v.is_short_string());
        assert_eq!(v.short_string_len(), 0);
        let mut buf = [0u8; SHORT_STRING_MAX_LEN];
        assert_eq!(v.short_string_to_buf(&mut buf), 0);
    }

    #[test]
    fn test_short_string_byte_order_stability() {
        // First byte should land in the least-significant byte of
        // the payload. This invariant is relied on by any future
        // SIMD-style decoder that bulk-reads the payload.
        let v = JSValue::try_short_string(b"abcde").unwrap();
        let payload = v.bits() & SHORT_STRING_DATA_MASK;
        assert_eq!((payload & 0xFF) as u8, b'a');
        assert_eq!(((payload >> 8) & 0xFF) as u8, b'b');
        assert_eq!(((payload >> 16) & 0xFF) as u8, b'c');
        assert_eq!(((payload >> 24) & 0xFF) as u8, b'd');
        assert_eq!(((payload >> 32) & 0xFF) as u8, b'e');
    }

    #[test]
    fn test_jsvalue_equals_numbers() {
        // Same numbers
        assert_eq!(js_jsvalue_equals(42.0, 42.0), 1);
        assert_eq!(js_jsvalue_equals(0.0, 0.0), 1);
        // -0 === 0 is true in JS
        assert_eq!(js_jsvalue_equals(-0.0, 0.0), 1);
        // Different numbers
        assert_eq!(js_jsvalue_equals(1.0, 2.0), 0);
        // null/undefined
        let null = f64::from_bits(TAG_NULL);
        let undef = f64::from_bits(TAG_UNDEFINED);
        assert_eq!(js_jsvalue_equals(null, null), 1);
        assert_eq!(js_jsvalue_equals(undef, undef), 1);
        assert_eq!(js_jsvalue_equals(null, undef), 0); // strict: null !== undefined
        assert_eq!(js_jsvalue_equals(null, 0.0), 0);
    }
}
