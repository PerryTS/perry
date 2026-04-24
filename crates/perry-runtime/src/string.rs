//! String runtime support for Perry
//!
//! Strings are heap-allocated UTF-8 (or WTF-8) sequences with capacity for efficient appending.
//! Layout:
//!   - StringHeader at the start (utf16_len at offset 0 for inline codegen access)
//!   - Followed by `capacity` bytes of data (only `byte_len` bytes are valid)
//!
//! Strings containing lone surrogates (U+D800..U+DFFF) are stored as WTF-8 bytes and
//! marked with STRING_FLAG_HAS_LONE_SURROGATES in the `flags` field.

use std::ptr;
use std::slice;
use std::str;

/// Flag: string bytes contain WTF-8 lone-surrogate sequences (U+D800..U+DFFF).
/// Set by js_string_from_wtf8_bytes. Checked by isWellFormed/toWellFormed.
pub const STRING_FLAG_HAS_LONE_SURROGATES: u32 = 1;

/// A static empty string that can be used as a safe fallback for null pointers.
/// Has utf16_len=0, byte_len=0, capacity=0, refcount=0, flags=0 (shared).
#[no_mangle]
pub static PERRY_EMPTY_STRING: StringHeader = StringHeader { utf16_len: 0, byte_len: 0, capacity: 0, refcount: 0, flags: 0 };

/// Get a pointer to the static empty string (for codegen null guards).
#[no_mangle]
pub extern "C" fn js_get_empty_string() -> *const StringHeader {
    &PERRY_EMPTY_STRING as *const StringHeader
}

/// Check if a pointer is valid (not null and not a small invalid value from bad NaN-unboxing).
/// When codegen extracts a "pointer" from TAG_UNDEFINED (0x7FFC_0000_0000_0001), the lower
/// 48-bit AND yields 1, which passes is_null() but crashes on dereference.
#[inline]
pub fn is_valid_string_ptr(p: *const StringHeader) -> bool {
    !p.is_null() && (p as usize) >= 0x1000
}

/// Header for heap-allocated strings
///
/// `utf16_len` is at offset 0 so codegen can inline `.length` as a single i32 load.
/// `byte_len` tracks the actual byte count for internal memcpy/slice operations.
///
/// The `refcount` field enables in-place append optimization in `js_string_append`:
/// - refcount=0: shared/unknown ownership — never mutated in-place (safe default)
/// - refcount=1: unique owner — `js_string_append` can append in-place if capacity allows
/// Only strings created by `js_string_append` get refcount=1. When a string pointer is
/// copied to another variable, codegen calls `js_string_addref` to set refcount=0 (shared).
///
/// `flags`: STRING_FLAG_HAS_LONE_SURROGATES (=1) marks WTF-8 strings with lone surrogates.
#[repr(C)]
pub struct StringHeader {
    /// Length in UTF-16 code units (JS `.length` semantics). At offset 0 for inline codegen.
    pub utf16_len: u32,
    /// Length in bytes (internal use for memcpy, capacity checks, etc.)
    pub byte_len: u32,
    /// Capacity in bytes (allocated space for data)
    pub capacity: u32,
    /// Reference hint: 0=shared (never mutate in-place), 1=unique (in-place append OK)
    pub refcount: u32,
    /// Bit flags: STRING_FLAG_HAS_LONE_SURROGATES = 1
    pub flags: u32,
}

// ── UTF-8 ↔ UTF-16 conversion helpers ──────────────────────────────────

/// Count UTF-16 code units for a UTF-8 byte slice. Returns 0 for empty/null.
#[inline]
fn compute_utf16_len(data: *const u8, byte_len: u32) -> u32 {
    if data.is_null() || byte_len == 0 {
        return 0;
    }
    let bytes = unsafe { slice::from_raw_parts(data, byte_len as usize) };
    // ASCII fast path: if no byte has high bit set, utf16_len == byte_len
    if bytes.iter().all(|&b| b < 0x80) {
        return byte_len;
    }
    let s = unsafe { str::from_utf8_unchecked(bytes) };
    s.encode_utf16().count() as u32
}

/// Convert a UTF-16 code unit index to a UTF-8 byte offset.
/// Returns `s.len()` if `utf16_idx` is past the end.
#[inline]
fn utf16_offset_to_byte_offset(s: &str, utf16_idx: usize) -> usize {
    if utf16_idx == 0 {
        return 0;
    }
    let mut byte_off = 0;
    let mut u16_count = 0;
    for ch in s.chars() {
        if u16_count >= utf16_idx {
            return byte_off;
        }
        byte_off += ch.len_utf8();
        u16_count += ch.len_utf16();
    }
    byte_off // past the end → return full byte length
}

/// Convert a UTF-8 byte offset to a UTF-16 code unit index.
#[inline]
fn byte_offset_to_utf16_index(s: &str, byte_off: usize) -> usize {
    if byte_off == 0 {
        return 0;
    }
    s[..byte_off].encode_utf16().count()
}

/// Allocate `total_size` bytes for a string (StringHeader + payload) from the
/// thread-local arena bump allocator. Issue #62 phase B: strings in tight
/// allocation loops (`"item_" + i`, `i.toString()`, template literals) were
/// the dominant `gc_malloc` caller. `gc_malloc` costs ~30-40ns even with
/// mimalloc (per-thread free list + GcHeader init + MALLOC_STATE tracking).
/// The arena is a pointer bump (~10-15ns with sync) and needs no tracking —
/// strings are discovered by walking arena blocks linearly, same as objects
/// and arrays. Block-persistence keeps interned/long-lived strings alive
/// for as long as any other arena object in the same block is reachable;
/// truly dead blocks reset to offset=0 in O(1).
#[inline]
fn string_arena_alloc(total_size: usize) -> *mut u8 {
    crate::arena::arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_STRING)
}

/// Create a string from raw bytes
/// Returns a pointer to StringHeader
#[no_mangle]
pub extern "C" fn js_string_from_bytes(data: *const u8, len: u32) -> *mut StringHeader {
    js_string_from_bytes_with_capacity(data, len, len)
}

/// Fast path: create a string from bytes known to be pure ASCII.
/// Skips the `compute_utf16_len` byte scan — sets utf16_len = byte_len directly.
#[inline]
fn js_string_from_ascii_bytes(data: *const u8, len: u32) -> *mut StringHeader {
    let total_size = std::mem::size_of::<StringHeader>() + len as usize;
    let raw = string_arena_alloc(total_size);
    let ptr = raw as *mut StringHeader;
    unsafe {
        (*ptr).utf16_len = len; // ASCII: utf16_len == byte_len
        (*ptr).byte_len = len;
        (*ptr).capacity = len;
        (*ptr).refcount = 0;
        (*ptr).flags = 0;
        if len > 0 && !data.is_null() {
            let data_ptr = (ptr as *mut u8).add(std::mem::size_of::<StringHeader>());
            ptr::copy_nonoverlapping(data, data_ptr, len as usize);
        }
    }
    ptr
}

/// Create a string from raw bytes with extra capacity for future appending
#[no_mangle]
pub extern "C" fn js_string_from_bytes_with_capacity(data: *const u8, len: u32, capacity: u32) -> *mut StringHeader {
    let capacity = capacity.max(len); // Ensure capacity >= len
    let total_size = std::mem::size_of::<StringHeader>() + capacity as usize;

    let raw = string_arena_alloc(total_size);
    let ptr = raw as *mut StringHeader;

    unsafe {
        let u16len = compute_utf16_len(data, len);
        (*ptr).utf16_len = u16len;
        (*ptr).byte_len = len;
        (*ptr).capacity = capacity;
        (*ptr).refcount = 0; // shared by default — caller can set to 1 if uniquely owned
        (*ptr).flags = 0;

        // Copy string data after header
        if len > 0 && !data.is_null() {
            let data_ptr = (ptr as *mut u8).add(std::mem::size_of::<StringHeader>());
            ptr::copy_nonoverlapping(data, data_ptr, len as usize);
        }
    }

    ptr
}

/// Count UTF-16 code units for a WTF-8 byte slice without using from_utf8.
/// Lone surrogate sequences (0xED 0xA0..0xBF 0x80..0xBF) each count as 1 unit,
/// same as any other BMP codepoint. Astral sequences (4-byte) count as 2.
#[inline]
fn compute_utf16_len_wtf8(bytes: &[u8]) -> u32 {
    let mut count = 0u32;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b < 0x80 {
            count += 1;
            i += 1;
        } else if b < 0xC0 {
            // continuation byte in lead position — skip
            i += 1;
        } else if b < 0xE0 {
            count += 1;
            i += 2;
        } else if b < 0xF0 {
            // 3-byte sequence: BMP codepoint or WTF-8 lone surrogate → 1 unit
            count += 1;
            i += 3;
        } else {
            // 4-byte sequence: astral codepoint → 2 UTF-16 units
            count += 2;
            i += 4;
        }
    }
    count
}

/// Create a StringHeader from WTF-8 bytes (may contain lone-surrogate sequences).
/// Sets STRING_FLAG_HAS_LONE_SURROGATES so isWellFormed()/toWellFormed() work correctly.
#[no_mangle]
pub extern "C" fn js_string_from_wtf8_bytes(data: *const u8, len: u32) -> *mut StringHeader {
    let total_size = std::mem::size_of::<StringHeader>() + len as usize;
    let raw = string_arena_alloc(total_size);
    let ptr = raw as *mut StringHeader;
    unsafe {
        let bytes = if len > 0 && !data.is_null() {
            slice::from_raw_parts(data, len as usize)
        } else {
            &[]
        };
        let u16len = compute_utf16_len_wtf8(bytes);
        (*ptr).utf16_len = u16len;
        (*ptr).byte_len = len;
        (*ptr).capacity = len;
        (*ptr).refcount = 0;
        (*ptr).flags = STRING_FLAG_HAS_LONE_SURROGATES;
        if len > 0 && !data.is_null() {
            let data_ptr = (ptr as *mut u8).add(std::mem::size_of::<StringHeader>());
            ptr::copy_nonoverlapping(data, data_ptr, len as usize);
        }
    }
    ptr
}

/// Append a string to another string in-place if possible.
/// Returns the (possibly new) string pointer.
///
/// When capacity is exceeded, allocates a fresh string and copies both
/// dest and src content into it. This avoids gc_realloc entirely, which
/// prevents stale-pointer issues when the conservative GC scanner misses
/// pointers in caller-saved registers. The old string becomes garbage and
/// is collected in the next GC cycle.
#[no_mangle]
pub extern "C" fn js_string_append(dest: *mut StringHeader, src: *const StringHeader) -> *mut StringHeader {
    if !is_valid_string_ptr(dest as *const StringHeader) {
        // If dest is invalid, just duplicate src
        if !is_valid_string_ptr(src) {
            return js_string_from_bytes(ptr::null(), 0);
        }
        let src_blen = unsafe { (*src).byte_len };
        let src_data = string_data(src);
        return js_string_from_bytes(src_data, src_blen);
    }

    if !is_valid_string_ptr(src) {
        return dest;
    }

    // Self-append (s += s): must allocate fresh to avoid reading from
    // memory that is being written to.
    if dest as *const StringHeader == src {
        return js_string_concat(dest as *const StringHeader, src);
    }

    unsafe {
        let dest_blen = (*dest).byte_len;
        let src_blen = (*src).byte_len;

        if src_blen == 0 {
            return dest;
        }

        let new_blen = dest_blen + src_blen;

        // In-place append optimization: if dest is uniquely owned (refcount==1)
        // and has enough capacity, append directly without allocation.
        // This turns O(n^2) string building loops into amortized O(n).
        if (*dest).refcount == 1 && new_blen <= (*dest).capacity {
            let dest_data = (dest as *mut u8).add(std::mem::size_of::<StringHeader>());
            let src_data_ptr = string_data(src);
            ptr::copy_nonoverlapping(src_data_ptr, dest_data.add(dest_blen as usize), src_blen as usize);
            (*dest).byte_len = new_blen;
            (*dest).utf16_len += (*src).utf16_len;
            return dest; // Same pointer, no allocation!
        }

        // Allocate fresh with 2x capacity for future in-place appends.
        // Perry aliases strings through `let x = y` (pointer copy), so in-place
        // mutation of shared strings would corrupt other references.
        // We do NOT use gc_realloc here because the conservative GC scanner
        // may have already swept the dest string (pointer in a caller-saved
        // register that setjmp/stack-walk didn't capture). Fresh allocation
        // is safe: old string becomes garbage for the next GC cycle.
        let new_cap = (new_blen * 2).max(32);
        let new_ptr = js_string_from_bytes_with_capacity(ptr::null(), 0, new_cap);

        // Copy old dest content
        let new_data = (new_ptr as *mut u8).add(std::mem::size_of::<StringHeader>());
        let dest_data = (dest as *const u8).add(std::mem::size_of::<StringHeader>());
        ptr::copy_nonoverlapping(dest_data, new_data, dest_blen as usize);

        // Copy src content after dest content
        let src_data_ptr = string_data(src);
        ptr::copy_nonoverlapping(src_data_ptr, new_data.add(dest_blen as usize), src_blen as usize);
        (*new_ptr).byte_len = new_blen;
        (*new_ptr).utf16_len = (*dest).utf16_len + (*src).utf16_len;

        // Mark as uniquely owned — the caller (codegen) is about to assign
        // this pointer to a single variable, so in-place append is safe next time.
        (*new_ptr).refcount = 1;

        new_ptr
    }
}

/// Create an empty string with initial capacity (for building strings)
#[no_mangle]
pub extern "C" fn js_string_builder_new(initial_capacity: u32) -> *mut StringHeader {
    js_string_from_bytes_with_capacity(ptr::null(), 0, initial_capacity.max(16))
}

/// Mark a string as shared (refcount=0) so `js_string_append` won't mutate it in-place.
/// Called by codegen when a string pointer is copied to another variable (`let y = x`),
/// passed as a function argument, or stored into an array/object.
/// This is a NaN-boxed f64 input — extract the raw pointer first.
#[no_mangle]
pub extern "C" fn js_string_addref(s: *mut StringHeader) {
    if is_valid_string_ptr(s as *const StringHeader) {
        unsafe {
            (*s).refcount = 0; // Mark as shared — prevent in-place mutation
        }
    }
}

/// Internal helper: Create a StringHeader from a Rust &str
#[inline]
fn js_string_from_str(s: &str) -> *mut StringHeader {
    js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

/// Get string length in UTF-16 code units (JS `.length` semantics)
#[no_mangle]
pub extern "C" fn js_string_length(s: *const StringHeader) -> u32 {
    if !is_valid_string_ptr(s) {
        return 0;
    }
    unsafe { (*s).utf16_len }
}

/// Get the data pointer for a string
fn string_data(s: *const StringHeader) -> *const u8 {
    unsafe {
        (s as *const u8).add(std::mem::size_of::<StringHeader>())
    }
}

/// Get string as a Rust &str (for internal use)
fn string_as_str<'a>(s: *const StringHeader) -> &'a str {
    unsafe {
        let blen = (*s).byte_len as usize;
        let cap = (*s).capacity as usize;
        debug_assert!(blen <= cap, "StringHeader byte_len {} > capacity {}", blen, cap);
        let data = string_data(s);
        let bytes = slice::from_raw_parts(data, blen);
        str::from_utf8_unchecked(bytes)
    }
}

/// Check if string is pure ASCII (utf16_len == byte_len → all single-byte chars)
#[inline]
fn is_ascii_string(s: *const StringHeader) -> bool {
    unsafe { (*s).utf16_len == (*s).byte_len }
}

/// Concatenate two strings
#[no_mangle]
pub extern "C" fn js_string_concat(a: *const StringHeader, b: *const StringHeader) -> *mut StringHeader {
    let blen_a = if is_valid_string_ptr(a) { unsafe { (*a).byte_len } } else { 0 };
    let blen_b = if is_valid_string_ptr(b) { unsafe { (*b).byte_len } } else { 0 };
    let total_blen = blen_a + blen_b;

    // Intern fast path: if result is short enough, check the intern table
    // before allocating. Repeated property-name concatenations like
    // "field_" + j return the existing interned pointer — zero allocation.
    if total_blen > 0 && total_blen <= INTERN_MAX_BYTE_LEN {
        unsafe {
            let hash = fnv1a_concat(a, blen_a, b, blen_b);
            let slot = (hash as usize) & INTERN_TABLE_MASK;
            let entry = &INTERN_TABLE[slot];
            if entry.string_ptr != 0 && entry.hash == hash {
                let existing = entry.string_ptr as *const StringHeader;
                if is_valid_string_ptr(existing)
                    && (*existing).byte_len == total_blen
                    && concat_content_matches(a, blen_a, b, blen_b, existing)
                {
                    return existing as *mut StringHeader;
                }
            }
        }
    }

    let u16len_a = if is_valid_string_ptr(a) { unsafe { (*a).utf16_len } } else { 0 };
    let u16len_b = if is_valid_string_ptr(b) { unsafe { (*b).utf16_len } } else { 0 };

    let total_size = std::mem::size_of::<StringHeader>() + total_blen as usize;

    let raw = string_arena_alloc(total_size);
    let ptr = raw as *mut StringHeader;

    unsafe {
        let flags_a = if is_valid_string_ptr(a) { (*a).flags } else { 0 };
        let flags_b = if is_valid_string_ptr(b) { (*b).flags } else { 0 };
        (*ptr).utf16_len = u16len_a + u16len_b;
        (*ptr).byte_len = total_blen;
        (*ptr).capacity = total_blen;
        (*ptr).refcount = 0; // shared by default
        (*ptr).flags = flags_a | flags_b;

        let data_ptr = (ptr as *mut u8).add(std::mem::size_of::<StringHeader>());

        if is_valid_string_ptr(a) && blen_a > 0 {
            ptr::copy_nonoverlapping(string_data(a), data_ptr, blen_a as usize);
        }
        if is_valid_string_ptr(b) && blen_b > 0 {
            ptr::copy_nonoverlapping(string_data(b), data_ptr.add(blen_a as usize), blen_b as usize);
        }

        ptr
    }
}

/// Fused string + NaN-boxed value concatenation (issue #58).
///
/// `"item_" + i` currently requires two gc_malloc calls:
///   1. `js_jsvalue_to_string(i)` → intermediate StringHeader
///   2. `js_string_concat(prefix, intermediate)` → result StringHeader
///
/// This function collapses both into a single allocation when the value
/// is a number (the common case for `"str" + i` patterns in loops).
/// For non-number values, it falls back to js_jsvalue_to_string + concat.
///
/// The number formatting uses `itoa` for integers and a stack buffer for
/// `format!`, eliminating the Rust heap allocation from `format!()`.
#[no_mangle]
pub extern "C" fn js_string_concat_value(
    prefix: *const StringHeader,
    value: f64,
) -> *mut StringHeader {
    let prefix_blen = if is_valid_string_ptr(prefix) { unsafe { (*prefix).byte_len } } else { 0 };
    let prefix_u16 = if is_valid_string_ptr(prefix) { unsafe { (*prefix).utf16_len } } else { 0 };

    // Fast path: value is a number (no NaN-boxing tag in upper 16 bits → plain f64).
    // This covers the hot `"item_" + i` pattern.
    let bits = value.to_bits();
    let tag = bits >> 48;
    let is_plain_f64 = tag < 0x7FF8 || (tag == 0x7FF8 && (bits & 0x000F_FFFF_FFFF_FFFF) == 0);

    if is_plain_f64 {
        // Format the number into a stack buffer
        let mut num_buf = [0u8; 32]; // max f64 string is ~24 chars
        let num_len: usize;

        if value.fract() == 0.0 && value.abs() < 1e15 && !value.is_nan() && !value.is_infinite() {
            // Integer path: format directly without Rust heap allocation
            let n = value as i64;
            if n >= 0 && n <= 999_999_999 {
                // Fast itoa for common positive integers
                num_len = fast_itoa_u32(n as u32, &mut num_buf);
            } else {
                let s = format!("{}", n);
                let len = s.len().min(num_buf.len());
                num_buf[..len].copy_from_slice(&s.as_bytes()[..len]);
                num_len = len;
            }
        } else if value.is_nan() {
            num_buf[..3].copy_from_slice(b"NaN");
            num_len = 3;
        } else if value.is_infinite() {
            if value > 0.0 {
                num_buf[..8].copy_from_slice(b"Infinity");
                num_len = 8;
            } else {
                num_buf[..9].copy_from_slice(b"-Infinity");
                num_len = 9;
            }
        } else if value == 0.0 {
            num_buf[0] = b'0';
            num_len = 1;
        } else {
            let s = format!("{}", value);
            let len = s.len().min(num_buf.len());
            num_buf[..len].copy_from_slice(&s.as_bytes()[..len]);
            num_len = len;
        }

        // Single allocation for prefix + number string
        let total_blen = prefix_blen as usize + num_len;
        let total_size = std::mem::size_of::<StringHeader>() + total_blen;
        let raw = string_arena_alloc(total_size);
        let ptr = raw as *mut StringHeader;

        unsafe {
            // Both prefix and number digits are ASCII, so utf16_len == byte_len for the number part
            (*ptr).utf16_len = prefix_u16 + num_len as u32;
            (*ptr).byte_len = total_blen as u32;
            (*ptr).capacity = total_blen as u32;
            (*ptr).refcount = 0;
            (*ptr).flags = if is_valid_string_ptr(prefix) { (*prefix).flags } else { 0 };

            let data_ptr = (ptr as *mut u8).add(std::mem::size_of::<StringHeader>());
            if is_valid_string_ptr(prefix) && prefix_blen > 0 {
                ptr::copy_nonoverlapping(string_data(prefix), data_ptr, prefix_blen as usize);
            }
            ptr::copy_nonoverlapping(num_buf.as_ptr(), data_ptr.add(prefix_blen as usize), num_len);
        }

        return ptr;
    }

    // Slow path: non-number value — fall back to js_jsvalue_to_string + js_string_concat
    let value_str = crate::value::js_jsvalue_to_string(value);
    js_string_concat(prefix, value_str)
}

/// Fused value + string concatenation (value on the LEFT, string on the RIGHT).
/// Handles the `i + "_suffix"` pattern.
#[no_mangle]
pub extern "C" fn js_value_concat_string(
    value: f64,
    suffix: *const StringHeader,
) -> *mut StringHeader {
    let suffix_blen = if is_valid_string_ptr(suffix) { unsafe { (*suffix).byte_len } } else { 0 };
    let suffix_u16 = if is_valid_string_ptr(suffix) { unsafe { (*suffix).utf16_len } } else { 0 };

    let bits = value.to_bits();
    let tag = bits >> 48;
    let is_plain_f64 = tag < 0x7FF8 || (tag == 0x7FF8 && (bits & 0x000F_FFFF_FFFF_FFFF) == 0);

    if is_plain_f64 {
        let mut num_buf = [0u8; 32];
        let num_len: usize;

        if value.fract() == 0.0 && value.abs() < 1e15 && !value.is_nan() && !value.is_infinite() {
            let n = value as i64;
            if n >= 0 && n <= 999_999_999 {
                num_len = fast_itoa_u32(n as u32, &mut num_buf);
            } else {
                let s = format!("{}", n);
                let len = s.len().min(num_buf.len());
                num_buf[..len].copy_from_slice(&s.as_bytes()[..len]);
                num_len = len;
            }
        } else if value.is_nan() {
            num_buf[..3].copy_from_slice(b"NaN");
            num_len = 3;
        } else if value.is_infinite() {
            if value > 0.0 {
                num_buf[..8].copy_from_slice(b"Infinity");
                num_len = 8;
            } else {
                num_buf[..9].copy_from_slice(b"-Infinity");
                num_len = 9;
            }
        } else if value == 0.0 {
            num_buf[0] = b'0';
            num_len = 1;
        } else {
            let s = format!("{}", value);
            let len = s.len().min(num_buf.len());
            num_buf[..len].copy_from_slice(&s.as_bytes()[..len]);
            num_len = len;
        }

        let total_blen = num_len + suffix_blen as usize;
        let total_size = std::mem::size_of::<StringHeader>() + total_blen;
        let raw = string_arena_alloc(total_size);
        let ptr = raw as *mut StringHeader;

        unsafe {
            (*ptr).utf16_len = num_len as u32 + suffix_u16;
            (*ptr).byte_len = total_blen as u32;
            (*ptr).capacity = total_blen as u32;
            (*ptr).refcount = 0;
            (*ptr).flags = if is_valid_string_ptr(suffix) { (*suffix).flags } else { 0 };

            let data_ptr = (ptr as *mut u8).add(std::mem::size_of::<StringHeader>());
            ptr::copy_nonoverlapping(num_buf.as_ptr(), data_ptr, num_len);
            if is_valid_string_ptr(suffix) && suffix_blen > 0 {
                ptr::copy_nonoverlapping(string_data(suffix), data_ptr.add(num_len), suffix_blen as usize);
            }
        }

        return ptr;
    }

    let value_str = crate::value::js_jsvalue_to_string(value);
    js_string_concat(value_str, suffix)
}

/// Fast integer-to-ASCII formatting into a provided buffer.
/// Returns the number of bytes written. Digits are written to the END
/// of the buffer and then shifted to the front.
#[inline]
fn fast_itoa_u32(mut n: u32, buf: &mut [u8; 32]) -> usize {
    if n == 0 {
        buf[0] = b'0';
        return 1;
    }
    let mut pos = 31usize;
    while n > 0 {
        buf[pos] = b'0' + (n % 10) as u8;
        n /= 10;
        pos -= 1;
    }
    let start = pos + 1;
    let len = 32 - start;
    // Shift digits to front
    buf.copy_within(start..32, 0);
    len
}

/// Cached small-integer string table (0..=255). Initialized lazily on
/// first access. Avoids gc_malloc + format! for commonly repeated
/// number-to-string conversions (loop counters, property name suffixes).
const SMALL_INT_CACHE_SIZE: usize = 256;
static mut SMALL_INT_CACHE: [*mut StringHeader; SMALL_INT_CACHE_SIZE] =
    [std::ptr::null_mut(); SMALL_INT_CACHE_SIZE];

/// Convert a number (f64) to a string
/// Returns a new string representing the number
#[no_mangle]
pub extern "C" fn js_number_to_string(value: f64) -> *mut StringHeader {
    // Fast path: small non-negative integers use a cached string table.
    if value.fract() == 0.0 && value >= 0.0 && value < SMALL_INT_CACHE_SIZE as f64 {
        let idx = value as usize;
        unsafe {
            let cached = SMALL_INT_CACHE[idx];
            if !cached.is_null() {
                return cached;
            }
        }
        // Allocate and cache
        let s = format!("{}", value as u64);
        let ptr = js_string_from_bytes(s.as_bytes().as_ptr(), s.len() as u32);
        unsafe {
            // Mark as shared so it's never mutated in-place
            (*ptr).refcount = 0;
            SMALL_INT_CACHE[idx] = ptr;
            // Mark as interned so GC roots it via the intern scanner
            let gc_header = (ptr as *const u8)
                .sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
            (*gc_header).gc_flags |= crate::gc::GC_FLAG_PINNED;
        }
        return ptr;
    }

    // Format the number as a string per JS semantics.
    let s = if value.is_nan() {
        "NaN".to_string()
    } else if value.is_infinite() {
        if value > 0.0 { "Infinity".to_string() } else { "-Infinity".to_string() }
    } else if value == 0.0 {
        // Cover both +0 and -0 as "0" (matches JS)
        "0".to_string()
    } else if value.fract() == 0.0 && value.abs() < 1e15 {
        // Integer-like, format without decimal
        format!("{}", value as i64)
    } else {
        // Float, format with appropriate precision
        format!("{}", value)
    };

    let bytes = s.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// Format a number with a fixed number of decimal places (Number.prototype.toFixed)
#[no_mangle]
pub extern "C" fn js_number_to_fixed(value: f64, decimals: f64) -> *mut StringHeader {
    let dp = decimals as usize;
    let s = format!("{:.prec$}", value, prec = dp);
    let bytes = s.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// Format a number with a precision (Number.prototype.toPrecision).
/// JS spec: total significant digits, switches to exponential for very small/large.
#[no_mangle]
pub extern "C" fn js_number_to_precision(value: f64, precision: f64) -> *mut StringHeader {
    let s = if value.is_nan() {
        "NaN".to_string()
    } else if value.is_infinite() {
        if value > 0.0 { "Infinity".to_string() } else { "-Infinity".to_string() }
    } else {
        let p = precision as usize;
        if p == 0 {
            // toPrecision() with no arg is same as toString
            format_number_for_js(value)
        } else if value == 0.0 {
            // 0.toPrecision(3) = "0.00"
            if p == 1 { "0".to_string() } else { format!("0.{}", "0".repeat(p - 1)) }
        } else {
            // Find the decimal exponent: floor(log10(|x|))
            let abs = value.abs();
            let exp = abs.log10().floor() as i32;
            // JS uses exponential notation when exp < -6 or exp >= precision
            if exp < -6 || exp >= p as i32 {
                // Exponential: precision-1 digits after decimal, e+/-exp
                let mantissa_digits = if p > 1 { p - 1 } else { 0 };
                let formatted = format!("{:.*e}", mantissa_digits, value);
                // Rust's "{:e}" format produces "1.23e4"; JS uses "1.23e+4"
                fix_exponent_format(&formatted)
            } else {
                // Fixed: precision - exp - 1 digits after decimal
                let dp = (p as i32 - exp - 1).max(0) as usize;
                format!("{:.prec$}", value, prec = dp)
            }
        }
    };
    let bytes = s.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// Format a number in exponential notation (Number.prototype.toExponential).
#[no_mangle]
pub extern "C" fn js_number_to_exponential(value: f64, decimals: f64) -> *mut StringHeader {
    let s = if value.is_nan() {
        "NaN".to_string()
    } else if value.is_infinite() {
        if value > 0.0 { "Infinity".to_string() } else { "-Infinity".to_string() }
    } else {
        let dp = decimals as usize;
        // Rust's `{:e}` produces e.g. "1.23e4"; JS expects "1.23e+4"
        let formatted = format!("{:.*e}", dp, value);
        fix_exponent_format(&formatted)
    };
    let bytes = s.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// Convert Rust's `{:e}` exponential format to JS's: "1.23e4" -> "1.23e+4", "1.23e-4" stays.
fn fix_exponent_format(s: &str) -> String {
    if let Some(e_pos) = s.find('e') {
        let (mantissa, exp_part) = s.split_at(e_pos);
        let exp_str = &exp_part[1..]; // skip 'e'
        if exp_str.starts_with('-') {
            format!("{}e{}", mantissa, exp_str)
        } else {
            // Add explicit + sign and strip leading zeros from exponent
            let n: i64 = exp_str.parse().unwrap_or(0);
            format!("{}e+{}", mantissa, n)
        }
    } else {
        s.to_string()
    }
}

/// Format a number per JS toString rules (helper for toPrecision when precision=0)
fn format_number_for_js(value: f64) -> String {
    if value.is_nan() { return "NaN".to_string(); }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity".to_string() } else { "-Infinity".to_string() };
    }
    if value == 0.0 { return "0".to_string(); }
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{}", value)
    }
}

/// Get a slice of a string (byte-based for now)
/// Returns a new string from start to end (exclusive).
/// start/end are in UTF-16 code unit indices (JS semantics).
#[no_mangle]
pub extern "C" fn js_string_slice(s: *const StringHeader, start: i32, end: i32) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }

    let len = unsafe { (*s).utf16_len } as i32;

    // Handle negative indices (from end)
    let start = if start < 0 { (len + start).max(0) } else { start.min(len) };
    let end = if end < 0 { (len + end).max(0) } else { end.min(len) };

    if start >= end {
        return js_string_from_bytes(ptr::null(), 0);
    }

    // ASCII fast path: byte offsets == UTF-16 offsets, skip utf16_len scan
    if is_ascii_string(s) {
        let slice_len = (end - start) as u32;
        unsafe {
            let src = string_data(s).add(start as usize);
            return js_string_from_ascii_bytes(src, slice_len);
        }
    }

    // Convert UTF-16 offsets to byte offsets
    let str_data = string_as_str(s);
    let byte_start = utf16_offset_to_byte_offset(str_data, start as usize);
    let byte_end = utf16_offset_to_byte_offset(str_data, end as usize);
    let slice_bytes = &str_data.as_bytes()[byte_start..byte_end];
    js_string_from_bytes(slice_bytes.as_ptr(), slice_bytes.len() as u32)
}

/// Get a substring (similar to slice but different behavior)
/// - Negative indices are treated as 0
/// - If start > end, arguments are swapped
/// start/end are in UTF-16 code unit indices (JS semantics).
#[no_mangle]
pub extern "C" fn js_string_substring(s: *const StringHeader, start: i32, end: i32) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }

    let len = unsafe { (*s).utf16_len } as i32;

    // Treat negative indices as 0
    let mut start = start.max(0).min(len);
    let mut end = end.max(0).min(len);

    // Swap if start > end
    if start > end {
        std::mem::swap(&mut start, &mut end);
    }

    if start >= end {
        return js_string_from_bytes(ptr::null(), 0);
    }

    // ASCII fast path: skip utf16_len scan in allocator
    if is_ascii_string(s) {
        let slice_len = (end - start) as u32;
        unsafe {
            let src = string_data(s).add(start as usize);
            return js_string_from_ascii_bytes(src, slice_len);
        }
    }

    let str_data = string_as_str(s);
    let byte_start = utf16_offset_to_byte_offset(str_data, start as usize);
    let byte_end = utf16_offset_to_byte_offset(str_data, end as usize);
    let slice_bytes = &str_data.as_bytes()[byte_start..byte_end];
    js_string_from_bytes(slice_bytes.as_ptr(), slice_bytes.len() as u32)
}

/// Trim whitespace from both ends of a string
#[no_mangle]
pub extern "C" fn js_string_trim(s: *const StringHeader) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }

    let str_data = string_as_str(s);
    let trimmed = str_data.trim();
    js_string_from_str(trimmed)
}

/// Trim whitespace from start of a string (trimStart/trimLeft)
#[no_mangle]
pub extern "C" fn js_string_trim_start(s: *const StringHeader) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let str_data = string_as_str(s);
    js_string_from_str(str_data.trim_start())
}

/// Trim whitespace from end of a string (trimEnd/trimRight)
#[no_mangle]
pub extern "C" fn js_string_trim_end(s: *const StringHeader) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let str_data = string_as_str(s);
    js_string_from_str(str_data.trim_end())
}

/// Convert string to lowercase
#[no_mangle]
pub extern "C" fn js_string_to_lower_case(s: *const StringHeader) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }

    let str_data = string_as_str(s);
    let lower = str_data.to_lowercase();
    js_string_from_str(&lower)
}

/// Convert string to uppercase
#[no_mangle]
pub extern "C" fn js_string_to_upper_case(s: *const StringHeader) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }

    let str_data = string_as_str(s);
    let upper = str_data.to_uppercase();
    js_string_from_str(&upper)
}

/// Find index of substring (-1 if not found)
#[no_mangle]
pub extern "C" fn js_string_index_of(haystack: *const StringHeader, needle: *const StringHeader) -> i32 {
    js_string_index_of_from(haystack, needle, 0)
}

/// Find index of substring starting from a given position (-1 if not found).
/// from_index and return value are in UTF-16 code unit indices (JS semantics).
#[no_mangle]
pub extern "C" fn js_string_index_of_from(haystack: *const StringHeader, needle: *const StringHeader, from_index: i32) -> i32 {
    if !is_valid_string_ptr(haystack) || !is_valid_string_ptr(needle) {
        return -1;
    }

    unsafe {
        let h_blen = (*haystack).byte_len as usize;
        let n_blen = (*needle).byte_len as usize;

        // ASCII fast path: byte offset == UTF-16 offset, use Rust's
        // optimized Two-Way str::find (avoids O(n*m) naive scan).
        if is_ascii_string(haystack) {
            let start = if from_index < 0 { 0usize } else { from_index as usize };
            if n_blen == 0 {
                return start.min(h_blen) as i32;
            }
            if start + n_blen > h_blen {
                return -1;
            }
            let h = std::str::from_utf8_unchecked(
                slice::from_raw_parts(string_data(haystack), h_blen),
            );
            let n = std::str::from_utf8_unchecked(
                slice::from_raw_parts(string_data(needle), n_blen),
            );
            return match h[start..].find(n) {
                Some(pos) => (start + pos) as i32,
                None => -1,
            };
        }

        // Non-ASCII: construct &str, convert UTF-16 from_index to byte offset
        let h = string_as_str(haystack);
        let n = string_as_str(needle);
        let u16_start = if from_index < 0 { 0usize } else { from_index as usize };
        let byte_start = utf16_offset_to_byte_offset(h, u16_start);
        if byte_start > h.len() {
            if n.is_empty() { return (*haystack).utf16_len as i32; }
            return -1;
        }
        match h[byte_start..].find(n) {
            Some(byte_pos) => byte_offset_to_utf16_index(h, byte_start + byte_pos) as i32,
            None => -1,
        }
    }
}

/// Find the last index of a substring (-1 if not found).
/// Returns the UTF-16 code unit offset of the LAST occurrence, or -1 if not found.
/// An empty needle returns the string's UTF-16 length.
#[no_mangle]
pub extern "C" fn js_string_last_index_of(haystack: *const StringHeader, needle: *const StringHeader) -> i32 {
    if !is_valid_string_ptr(haystack) {
        return -1;
    }
    if !is_valid_string_ptr(needle) {
        return unsafe { (*haystack).utf16_len as i32 };
    }

    unsafe {
        let n_blen = (*needle).byte_len as usize;
        if n_blen == 0 {
            return (*haystack).utf16_len as i32;
        }

        // ASCII fast path: byte offset == UTF-16 offset, use rfind
        if is_ascii_string(haystack) {
            let h_blen = (*haystack).byte_len as usize;
            if n_blen > h_blen { return -1; }
            let h = std::str::from_utf8_unchecked(
                slice::from_raw_parts(string_data(haystack), h_blen),
            );
            let n = std::str::from_utf8_unchecked(
                slice::from_raw_parts(string_data(needle), n_blen),
            );
            return match h.rfind(n) {
                Some(pos) => pos as i32,
                None => -1,
            };
        }
    }

    // Non-ASCII path
    let h = string_as_str(haystack);
    let n = string_as_str(needle);
    match h.rfind(n) {
        Some(byte_pos) => byte_offset_to_utf16_index(h, byte_pos) as i32,
        None => -1,
    }
}

/// Compare two strings lexicographically.
/// Returns -1 if a < b, 0 if a == b, 1 if a > b.
#[no_mangle]
pub extern "C" fn js_string_compare(a: *const StringHeader, b: *const StringHeader) -> i32 {
    let a_valid = is_valid_string_ptr(a);
    let b_valid = is_valid_string_ptr(b);
    if !a_valid && !b_valid {
        return 0;
    }
    if !a_valid {
        return -1;
    }
    if !b_valid {
        return 1;
    }

    unsafe {
        let len_a = (*a).byte_len as usize;
        let len_b = (*b).byte_len as usize;
        let data_a = string_data(a);
        let data_b = string_data(b);
        let a_bytes = std::slice::from_raw_parts(data_a, len_a);
        let b_bytes = std::slice::from_raw_parts(data_b, len_b);
        match a_bytes.cmp(b_bytes) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    }
}

/// Compare two strings for equality
#[no_mangle]
pub extern "C" fn js_string_equals(a: *const StringHeader, b: *const StringHeader) -> i32 {
    // Pointer identity fast path
    if std::ptr::eq(a, b) {
        return 1;
    }

    let a_valid = is_valid_string_ptr(a);
    let b_valid = is_valid_string_ptr(b);
    if !a_valid && !b_valid {
        return 1;
    }
    if !a_valid || !b_valid {
        return 0;
    }

    let blen_a = unsafe { (*a).byte_len };
    let blen_b = unsafe { (*b).byte_len };

    if blen_a != blen_b {
        return 0;
    }

    unsafe {
        let data_a = string_data(a);
        let data_b = string_data(b);
        let slice_a = std::slice::from_raw_parts(data_a, blen_a as usize);
        let slice_b = std::slice::from_raw_parts(data_b, blen_b as usize);
        if slice_a == slice_b { 1 } else { 0 }
    }
}

/// Check if a string starts with a prefix
#[no_mangle]
pub extern "C" fn js_string_starts_with(s: *const StringHeader, prefix: *const StringHeader) -> i32 {
    if !is_valid_string_ptr(s) || !is_valid_string_ptr(prefix) {
        return 0;
    }

    let blen = unsafe { (*s).byte_len };
    let prefix_blen = unsafe { (*prefix).byte_len };

    if prefix_blen > blen {
        return 0;
    }

    unsafe {
        let data = string_data(s);
        let prefix_data = string_data(prefix);

        for i in 0..prefix_blen as usize {
            if *data.add(i) != *prefix_data.add(i) {
                return 0;
            }
        }
    }

    1
}

/// Check if a string ends with a suffix
#[no_mangle]
pub extern "C" fn js_string_ends_with(s: *const StringHeader, suffix: *const StringHeader) -> i32 {
    if !is_valid_string_ptr(s) || !is_valid_string_ptr(suffix) {
        return 0;
    }

    let blen = unsafe { (*s).byte_len };
    let suffix_blen = unsafe { (*suffix).byte_len };

    if suffix_blen > blen {
        return 0;
    }

    unsafe {
        let data = string_data(s);
        let suffix_data = string_data(suffix);
        let start = blen - suffix_blen;

        for i in 0..suffix_blen as usize {
            if *data.add(start as usize + i) != *suffix_data.add(i) {
                return 0;
            }
        }
    }

    1
}

/// Get character code at index (returns UTF-16 code unit, or NaN if out of bounds).
/// Index is in UTF-16 code units (matches JS spec). For ASCII strings this is
/// equivalent to byte indexing; for multi-byte UTF-8 we walk codepoints without
/// allocating — the old `encode_utf16().collect()` path made hashing a 68 MB
/// string O(n²) (issue #65).
#[no_mangle]
pub extern "C" fn js_string_char_code_at(s: *const StringHeader, index: i32) -> f64 {
    if !is_valid_string_ptr(s) || index < 0 {
        return f64::NAN;
    }

    let u16len = unsafe { (*s).utf16_len } as usize;
    let idx = index as usize;
    if idx >= u16len {
        return f64::NAN;
    }

    // ASCII fast path: byte_len == utf16_len means every byte is one
    // UTF-16 code unit. Direct byte index, no scan, no allocation.
    if is_ascii_string(s) {
        unsafe { return *string_data(s).add(idx) as f64; }
    }

    // Non-ASCII: walk codepoints counting UTF-16 units. Allocation-free.
    let str_data = string_as_str(s);
    let mut utf16_pos = 0usize;
    for ch in str_data.chars() {
        let clen = ch.len_utf16();
        if utf16_pos + clen > idx {
            if clen == 1 {
                return ch as u32 as f64;
            }
            let mut buf = [0u16; 2];
            ch.encode_utf16(&mut buf);
            return buf[idx - utf16_pos] as f64;
        }
        utf16_pos += clen;
    }
    f64::NAN
}

/// Get character at UTF-16 code unit index (returns single-character string).
/// For a BMP character this returns the character itself; for a surrogate half
/// of an astral character this returns the lone surrogate (matching JS behavior).
#[no_mangle]
pub extern "C" fn js_string_char_at(s: *const StringHeader, index: i32) -> *mut StringHeader {
    if !is_valid_string_ptr(s) || index < 0 {
        return js_string_from_bytes(std::ptr::null(), 0);
    }

    let u16len = unsafe { (*s).utf16_len };
    if index as u32 >= u16len {
        return js_string_from_bytes(std::ptr::null(), 0);
    }

    // ASCII fast path: skip utf16_len scan
    if is_ascii_string(s) {
        unsafe {
            let data = string_data(s);
            let char_ptr = data.add(index as usize);
            return js_string_from_ascii_bytes(char_ptr, 1);
        }
    }

    // UTF-16 path: find the UTF-8 bytes for the character at this UTF-16 index
    let str_data = string_as_str(s);
    let byte_off = utf16_offset_to_byte_offset(str_data, index as usize);
    let remaining = &str_data[byte_off..];
    if let Some(ch) = remaining.chars().next() {
        let ch_len = ch.len_utf8();
        js_string_from_bytes(remaining.as_ptr(), ch_len as u32)
    } else {
        js_string_from_bytes(std::ptr::null(), 0)
    }
}

/// Split a string into an array of single-character strings.
/// Used by the spread operator: `[..."hello"]` → `["h","e","l","l","o"]`.
/// JS spread iterates by codepoints (not UTF-16 units), so "😀" → ["😀"] (1 element).
/// Returns an ArrayHeader pointer with NaN-boxed STRING_TAG elements.
#[no_mangle]
pub extern "C" fn js_string_to_char_array(s: i64) -> i64 {
    let str_ptr = (s as u64 & crate::value::POINTER_MASK) as *const StringHeader;
    if str_ptr.is_null() || !is_valid_string_ptr(str_ptr) {
        return crate::array::js_array_alloc(0) as i64;
    }
    let str_data = string_as_str(str_ptr);
    let char_count = str_data.chars().count();
    let arr = crate::array::js_array_alloc_with_length(char_count as u32);
    let elements = unsafe { (arr as *mut u8).add(8) as *mut f64 };
    for (i, ch) in str_data.chars().enumerate() {
        let mut buf = [0u8; 4];
        let encoded = ch.encode_utf8(&mut buf);
        let ch_ptr = js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32);
        let nanboxed = f64::from_bits(
            crate::value::STRING_TAG | (ch_ptr as u64 & crate::value::POINTER_MASK),
        );
        unsafe { *elements.add(i) = nanboxed; }
    }
    arr as i64
}

/// Create a string from a character code (String.fromCharCode)
/// Takes a single character code and returns a 1-character string
#[no_mangle]
pub extern "C" fn js_string_from_char_code(code: i32) -> *mut StringHeader {
    if code < 0 || code > 0xFFFF {
        // Invalid character code, return empty string
        return js_string_from_bytes(std::ptr::null(), 0);
    }

    // For ASCII characters, create a simple 1-byte string
    if code < 128 {
        let byte = code as u8;
        return js_string_from_bytes(&byte as *const u8, 1);
    }

    // For non-ASCII, encode as UTF-8
    let ch = char::from_u32(code as u32).unwrap_or('\u{FFFD}');
    let mut buf = [0u8; 4];
    let encoded = ch.encode_utf8(&mut buf);
    js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32)
}

/// Create a string from a Unicode code point (String.fromCodePoint).
/// Supports the full Unicode range (0..0x10FFFF), unlike fromCharCode (0..0xFFFF).
#[no_mangle]
pub extern "C" fn js_string_from_code_point(code: i32) -> *mut StringHeader {
    if code < 0 || code > 0x10FFFF {
        return js_string_from_bytes(std::ptr::null(), 0);
    }
    let ch = match char::from_u32(code as u32) {
        Some(c) => c,
        None => return js_string_from_bytes(std::ptr::null(), 0),
    };
    let mut buf = [0u8; 4];
    let encoded = ch.encode_utf8(&mut buf);
    js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32)
}

/// String.prototype.at(index) — supports negative indices.
/// Returns NaN-boxed single-char string, or NaN-boxed undefined if out of bounds.
/// Index is in UTF-16 code units (matches JS spec).
#[no_mangle]
pub extern "C" fn js_string_at(s: *const StringHeader, index: i32) -> f64 {
    if !is_valid_string_ptr(s) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let str_data = string_as_str(s);
    let utf16: Vec<u16> = str_data.encode_utf16().collect();
    let len = utf16.len() as i32;
    let resolved = if index < 0 { len + index } else { index };
    if resolved < 0 || resolved >= len {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // Decode the UTF-16 code unit at `resolved`. If it's a high surrogate followed
    // by a low surrogate, decode the pair; otherwise the unit is the code point.
    let unit = utf16[resolved as usize];
    let cp: u32 = if (0xD800..=0xDBFF).contains(&unit) && (resolved + 1) < len {
        let next = utf16[(resolved + 1) as usize];
        if (0xDC00..=0xDFFF).contains(&next) {
            0x10000 + ((unit as u32 - 0xD800) << 10) + (next as u32 - 0xDC00)
        } else {
            unit as u32
        }
    } else {
        unit as u32
    };
    let ch = char::from_u32(cp).unwrap_or('\u{FFFD}');
    let mut buf = [0u8; 4];
    let encoded = ch.encode_utf8(&mut buf);
    let ptr = js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32);
    crate::value::js_nanbox_string(ptr as i64)
}

/// String.prototype.codePointAt(index) — returns the Unicode code point at the given
/// UTF-16 code unit position, or NaN-boxed undefined if out of bounds.
#[no_mangle]
pub extern "C" fn js_string_code_point_at(s: *const StringHeader, index: i32) -> f64 {
    if !is_valid_string_ptr(s) || index < 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let u16len = unsafe { (*s).utf16_len } as usize;
    let idx = index as usize;
    if idx >= u16len {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }

    // ASCII fast path — identical to charCodeAt's.
    if is_ascii_string(s) {
        unsafe { return *string_data(s).add(idx) as f64; }
    }

    // Non-ASCII: walk codepoints without allocating a Vec<u16>.
    let str_data = string_as_str(s);
    let mut utf16_pos = 0usize;
    for ch in str_data.chars() {
        let clen = ch.len_utf16();
        if utf16_pos + clen > idx {
            if clen == 1 || utf16_pos == idx {
                // Either a BMP char, or the start of a surrogate pair
                // (which is the whole codepoint per the spec).
                return ch as u32 as f64;
            }
            // Index lands on the low surrogate — return the bare unit.
            let mut buf = [0u16; 2];
            ch.encode_utf16(&mut buf);
            return buf[idx - utf16_pos] as f64;
        }
        utf16_pos += clen;
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// String.prototype.normalize(form) — Unicode normalization.
/// `form` is one of: NFC (default), NFD, NFKC, NFKD. Pass null/empty for default NFC.
#[no_mangle]
pub extern "C" fn js_string_normalize(
    s: *const StringHeader,
    form: *const StringHeader,
) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(std::ptr::null(), 0);
    }
    let str_data = string_as_str(s);
    let form_str = if is_valid_string_ptr(form) {
        string_as_str(form)
    } else {
        "NFC"
    };
    use unicode_normalization::UnicodeNormalization;
    let normalized: String = match form_str {
        "NFC" => str_data.nfc().collect(),
        "NFD" => str_data.nfd().collect(),
        "NFKC" => str_data.nfkc().collect(),
        "NFKD" => str_data.nfkd().collect(),
        _ => str_data.nfc().collect(),
    };
    let bytes = normalized.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// String.prototype.localeCompare(other) — returns negative/zero/positive number.
/// We don't ship a true ICU collator. We approximate the Unicode default
/// collation with a two-pass comparison: first case-insensitive (so the
/// character class wins) and then case-sensitive with lowercase < uppercase
/// (matching V8's default ICU behavior where 'a' < 'A').
#[no_mangle]
pub extern "C" fn js_string_locale_compare(
    a: *const StringHeader,
    b: *const StringHeader,
) -> f64 {
    let a_valid = is_valid_string_ptr(a);
    let b_valid = is_valid_string_ptr(b);
    if !a_valid && !b_valid {
        return 0.0;
    }
    if !a_valid {
        return -1.0;
    }
    if !b_valid {
        return 1.0;
    }
    let a_str = string_as_str(a);
    let b_str = string_as_str(b);
    // Case-insensitive primary comparison
    let a_lower = a_str.to_lowercase();
    let b_lower = b_str.to_lowercase();
    match a_lower.cmp(&b_lower) {
        std::cmp::Ordering::Less => return -1.0,
        std::cmp::Ordering::Greater => return 1.0,
        std::cmp::Ordering::Equal => {}
    }
    // Same letters ignoring case — order by case (lowercase < uppercase
    // per the default Unicode collation tertiary weight).
    let mut ai = a_str.chars();
    let mut bi = b_str.chars();
    loop {
        match (ai.next(), bi.next()) {
            (None, None) => return 0.0,
            (None, Some(_)) => return -1.0,
            (Some(_), None) => return 1.0,
            (Some(ca), Some(cb)) => {
                if ca == cb {
                    continue;
                }
                let a_lower = ca.is_lowercase();
                let b_lower = cb.is_lowercase();
                if a_lower && !b_lower {
                    return -1.0;
                }
                if !a_lower && b_lower {
                    return 1.0;
                }
                return if (ca as u32) < (cb as u32) { -1.0 } else { 1.0 };
            }
        }
    }
}

/// String.prototype.isWellFormed() — returns NaN-boxed boolean.
/// A string is well-formed if it contains no lone surrogates.
/// Lone-surrogate strings are marked with STRING_FLAG_HAS_LONE_SURROGATES at construction.
#[no_mangle]
pub extern "C" fn js_string_is_well_formed(s: *const StringHeader) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    if !is_valid_string_ptr(s) {
        return f64::from_bits(TAG_TRUE);
    }
    let flags = unsafe { (*s).flags };
    if flags & STRING_FLAG_HAS_LONE_SURROGATES != 0 {
        return f64::from_bits(TAG_FALSE);
    }
    f64::from_bits(TAG_TRUE)
}

/// String.prototype.toWellFormed() — replaces lone surrogates with U+FFFD (U+FFFD = EF BF BD).
/// Works directly on WTF-8 bytes: replaces each 3-byte surrogate sequence
/// (ED A0..BF 80..BF) with the 3-byte U+FFFD encoding.
#[no_mangle]
pub extern "C" fn js_string_to_well_formed(s: *const StringHeader) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(std::ptr::null(), 0);
    }
    let flags = unsafe { (*s).flags };
    let blen = unsafe { (*s).byte_len } as usize;
    let data = string_data(s);
    if flags & STRING_FLAG_HAS_LONE_SURROGATES == 0 {
        // Well-formed UTF-8: return a copy without scanning
        return js_string_from_bytes(data, blen as u32);
    }
    // Scan raw bytes and replace every WTF-8 lone-surrogate sequence with U+FFFD.
    // WTF-8 surrogate: first byte = 0xED, second = 0xA0..=0xBF, third = 0x80..=0xBF.
    let bytes = unsafe { slice::from_raw_parts(data, blen) };
    let mut result: Vec<u8> = Vec::with_capacity(blen);
    let mut i = 0;
    while i < blen {
        let b = bytes[i];
        if b == 0xED
            && i + 2 < blen
            && (0xA0..=0xBF).contains(&bytes[i + 1])
            && (0x80..=0xBF).contains(&bytes[i + 2])
        {
            // Lone surrogate → U+FFFD (EF BF BD)
            result.extend_from_slice(&[0xEF, 0xBF, 0xBD]);
            i += 3;
        } else if b < 0x80 {
            result.push(b);
            i += 1;
        } else if b < 0xC0 {
            result.push(b);
            i += 1;
        } else if b < 0xE0 {
            result.push(b);
            if i + 1 < blen { result.push(bytes[i + 1]); }
            i += 2;
        } else if b < 0xF0 {
            result.push(b);
            if i + 1 < blen { result.push(bytes[i + 1]); }
            if i + 2 < blen { result.push(bytes[i + 2]); }
            i += 3;
        } else {
            result.push(b);
            if i + 1 < blen { result.push(bytes[i + 1]); }
            if i + 2 < blen { result.push(bytes[i + 2]); }
            if i + 3 < blen { result.push(bytes[i + 3]); }
            i += 4;
        }
    }
    js_string_from_bytes(result.as_ptr(), result.len() as u32)
}

/// Print a string to stdout
#[no_mangle]
pub extern "C" fn js_string_print(s: *const StringHeader) {
    if !is_valid_string_ptr(s) {
        println!("");
        return;
    }

    let str_data = string_as_str(s);
    println!("{}", str_data);
}

/// Print a string to stderr (console.error)
#[no_mangle]
pub extern "C" fn js_string_error(s: *const StringHeader) {
    if !is_valid_string_ptr(s) {
        eprintln!("");
        return;
    }

    let str_data = string_as_str(s);
    eprintln!("{}", str_data);
}

/// Print a string to stderr (console.warn)
#[no_mangle]
pub extern "C" fn js_string_warn(s: *const StringHeader) {
    if !is_valid_string_ptr(s) {
        eprintln!("");
        return;
    }

    let str_data = string_as_str(s);
    eprintln!("{}", str_data);
}

use crate::array::ArrayHeader;

/// Split a string by a delimiter
/// Returns an array of string pointers (stored as f64 bit patterns)
#[no_mangle]
pub extern "C" fn js_string_split(s: *const StringHeader, delimiter: *const StringHeader) -> *mut ArrayHeader {
    if !is_valid_string_ptr(s) {
        // Return empty array
        return crate::array::js_array_alloc(0);
    }

    // The LLVM backend can't always statically distinguish `s.split(regex)`
    // from `s.split(string)` at the call site — it uses a single decl for
    // both. Detect regex delimiters by checking whether the pointer was
    // recorded by `js_regexp_new` and delegate to `js_string_split_regex`
    // on a match. Otherwise the regex header would be read as a
    // StringHeader and segfault on the first byte of its `regex_ptr`.
    if crate::regex::is_regex_pointer(delimiter as *const u8) {
        return crate::regex::js_string_split_regex(
            s,
            delimiter as *const crate::regex::RegExpHeader,
        );
    }

    let str_data = string_as_str(s);
    let delim = if !is_valid_string_ptr(delimiter) {
        ""
    } else {
        string_as_str(delimiter)
    };

    const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
    let header_size = std::mem::size_of::<StringHeader>();

    if delim.is_empty() {
        // Empty delimiter: split into individual characters (single pass)
        let parts: Vec<*mut StringHeader> = str_data.chars().map(|c| {
            let mut buf = [0u8; 4];
            let char_str = c.encode_utf8(&mut buf);
            js_string_from_bytes(char_str.as_ptr(), char_str.len() as u32)
        }).collect();

        let arr = crate::array::js_array_alloc(parts.len() as u32);
        unsafe {
            (*arr).length = parts.len() as u32;
            let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
            for (i, p) in parts.iter().enumerate() {
                let nanboxed = STRING_TAG | (*p as u64 & POINTER_MASK);
                std::ptr::write(elements_ptr.add(i), f64::from_bits(nanboxed));
            }
        }
        return arr;
    }

    // Non-empty delimiter: arena-allocate parts (bump-pointer, no tracking overhead)
    let part_slices: Vec<&str> = str_data.split(delim).collect();
    let n = part_slices.len();

    let src_is_ascii = is_ascii_string(s);

    let arr = crate::array::js_array_alloc(n as u32);
    unsafe {
        (*arr).length = n as u32;
        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        for (i, part) in part_slices.iter().enumerate() {
            let byte_len = part.len() as u32;
            let alloc_size = header_size + byte_len as usize;
            let raw = crate::arena::arena_alloc_gc(alloc_size, 8, crate::gc::GC_TYPE_STRING);
            let sh = raw as *mut StringHeader;
            (*sh).byte_len = byte_len;
            (*sh).capacity = byte_len;
            (*sh).refcount = 0;
            (*sh).utf16_len = if src_is_ascii {
                byte_len
            } else {
                compute_utf16_len(part.as_ptr(), byte_len)
            };
            if byte_len > 0 {
                let data_ptr = (sh as *mut u8).add(header_size);
                ptr::copy_nonoverlapping(part.as_ptr(), data_ptr, byte_len as usize);
            }
            let nanboxed = STRING_TAG | (raw as u64 & POINTER_MASK);
            std::ptr::write(elements_ptr.add(i), f64::from_bits(nanboxed));
        }
    }

    arr
}

/// Allocate a string containing a single space character " "
/// Used as default pad string for padStart/padEnd
#[no_mangle]
pub extern "C" fn js_string_alloc_space() -> *mut StringHeader {
    js_string_from_bytes(" ".as_ptr(), 1)
}

/// Pad the start of a string to reach target length (in UTF-16 code units).
/// str.padStart(targetLength, padString)
#[no_mangle]
pub extern "C" fn js_string_pad_start(s: *const StringHeader, target_length: u32, pad_string: *const StringHeader) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let str_data = string_as_str(s);
    let pad_data = if is_valid_string_ptr(pad_string) { string_as_str(pad_string) } else { " " };

    let current_len = unsafe { (*s).utf16_len } as usize;
    let target_len = target_length as usize;

    if current_len >= target_len || pad_data.is_empty() {
        return js_string_from_bytes(str_data.as_ptr(), str_data.len() as u32);
    }

    let pad_needed = target_len - current_len;
    let pad_u16: Vec<u16> = pad_data.encode_utf16().collect();
    let mut result = String::with_capacity(target_len * 4);

    // Build padding by UTF-16 code units
    let mut u16_added = 0;
    let pad_chars: Vec<char> = pad_data.chars().collect();
    let mut pad_idx = 0;
    while u16_added < pad_needed {
        let ch = pad_chars[pad_idx % pad_chars.len()];
        let ch_u16_len = ch.len_utf16();
        if u16_added + ch_u16_len > pad_needed { break; }
        result.push(ch);
        u16_added += ch_u16_len;
        pad_idx += 1;
    }

    result.push_str(str_data);

    let ret = js_string_from_bytes(result.as_ptr(), result.len() as u32);
    std::hint::black_box(&result);
    ret
}

/// Pad the end of a string to reach target length (in UTF-16 code units).
/// str.padEnd(targetLength, padString)
#[no_mangle]
pub extern "C" fn js_string_pad_end(s: *const StringHeader, target_length: u32, pad_string: *const StringHeader) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let str_data = string_as_str(s);
    let pad_data = if is_valid_string_ptr(pad_string) { string_as_str(pad_string) } else { " " };

    let current_len = unsafe { (*s).utf16_len } as usize;
    let target_len = target_length as usize;

    if current_len >= target_len || pad_data.is_empty() {
        return js_string_from_bytes(str_data.as_ptr(), str_data.len() as u32);
    }

    let pad_needed = target_len - current_len;
    let mut result = String::with_capacity(target_len * 4);

    result.push_str(str_data);

    // Build padding by UTF-16 code units
    let pad_chars: Vec<char> = pad_data.chars().collect();
    let mut pad_idx = 0;
    let mut u16_added = 0;
    while u16_added < pad_needed {
        let ch = pad_chars[pad_idx % pad_chars.len()];
        let ch_u16_len = ch.len_utf16();
        if u16_added + ch_u16_len > pad_needed { break; }
        result.push(ch);
        u16_added += ch_u16_len;
        pad_idx += 1;
    }

    let ret = js_string_from_bytes(result.as_ptr(), result.len() as u32);
    std::hint::black_box(&result);
    ret
}

/// Repeat a string a specified number of times
/// str.repeat(count)
#[no_mangle]
pub extern "C" fn js_string_repeat(s: *const StringHeader, count: i32) -> *mut StringHeader {
    if !is_valid_string_ptr(s) || count <= 0 {
        // Return empty string
        return js_string_from_bytes("".as_ptr(), 0);
    }

    let str_data = string_as_str(s);
    if str_data.is_empty() {
        return js_string_from_bytes("".as_ptr(), 0);
    }

    let count = count as usize;
    let result = str_data.repeat(count);
    let ret = js_string_from_bytes(result.as_ptr(), result.len() as u32);
    std::hint::black_box(&result);
    ret
}

/// atob(base64) — decode a base64-encoded string to a binary string.
/// Input is a NaN-boxed STRING_TAG f64. Output is a raw *const StringHeader (codegen NaN-boxes).
#[no_mangle]
pub extern "C" fn js_atob(value: f64) -> *const StringHeader {
    use base64::Engine as _;
    const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
    let bits = value.to_bits();
    if (bits & 0xFFFF_0000_0000_0000) != STRING_TAG {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let str_ptr = (bits & POINTER_MASK) as *const StringHeader;
    if !is_valid_string_ptr(str_ptr) {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let s = string_as_str(str_ptr);
    match base64::engine::general_purpose::STANDARD.decode(s.as_bytes()) {
        Ok(decoded) => js_string_from_bytes(decoded.as_ptr(), decoded.len() as u32),
        Err(_) => js_string_from_bytes(ptr::null(), 0),
    }
}

/// btoa(string) — base64-encode a binary string.
#[no_mangle]
pub extern "C" fn js_btoa(value: f64) -> *const StringHeader {
    use base64::Engine as _;
    const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
    let bits = value.to_bits();
    if (bits & 0xFFFF_0000_0000_0000) != STRING_TAG {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let str_ptr = (bits & POINTER_MASK) as *const StringHeader;
    if !is_valid_string_ptr(str_ptr) {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let s = string_as_str(str_ptr);
    let encoded = base64::engine::general_purpose::STANDARD.encode(s.as_bytes());
    js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32)
}

// ---------------------------------------------------------------------------
// Property-name string interning
// ---------------------------------------------------------------------------

/// Intern table entry. Each slot holds one interned string.
#[derive(Clone, Copy)]
#[repr(C)]
struct InternEntry {
    hash: u64,        // FNV-1a content hash
    string_ptr: usize, // pointer to StringHeader (0 = empty slot)
}

const INTERN_TABLE_SIZE: usize = 8192;
const INTERN_TABLE_MASK: usize = INTERN_TABLE_SIZE - 1;

/// Maximum byte length for strings eligible for interning.
const INTERN_MAX_BYTE_LEN: u32 = 64;

#[no_mangle]
static mut INTERN_TABLE: [InternEntry; INTERN_TABLE_SIZE] =
    [InternEntry { hash: 0, string_ptr: 0 }; INTERN_TABLE_SIZE];

/// Intern a property-name string. Returns the canonical pointer for
/// the given content. `hash` is the pre-computed FNV-1a hash.
#[no_mangle]
pub extern "C" fn js_string_intern(
    key: *const StringHeader,
    hash: u64,
) -> *const StringHeader {
    if key.is_null() || !is_valid_string_ptr(key) {
        return key;
    }
    unsafe {
        let byte_len = (*key).byte_len;
        if byte_len > INTERN_MAX_BYTE_LEN {
            return key;
        }

        let slot = (hash as usize) & INTERN_TABLE_MASK;
        let entry = &mut INTERN_TABLE[slot];

        if entry.string_ptr != 0 && entry.hash == hash {
            let existing = entry.string_ptr as *const StringHeader;
            if is_valid_string_ptr(existing)
                && (*existing).byte_len == byte_len
                && intern_content_equals(key, existing, byte_len)
            {
                return existing;
            }
        }

        // Miss or collision — insert (evict on collision)
        entry.hash = hash;
        entry.string_ptr = key as usize;

        // Mark as interned in GcHeader
        let gc_header = (key as *const u8)
            .sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
        (*gc_header).gc_flags |= crate::gc::GC_FLAG_INTERNED;

        // Force shared — never mutate interned strings in-place
        (*(key as *mut StringHeader)).refcount = 0;

        key
    }
}

/// Byte-level content comparison for intern table lookups.
#[inline(always)]
unsafe fn intern_content_equals(a: *const StringHeader, b: *const StringHeader, byte_len: u32) -> bool {
    let data_a = (a as *const u8).add(std::mem::size_of::<StringHeader>());
    let data_b = (b as *const u8).add(std::mem::size_of::<StringHeader>());
    std::slice::from_raw_parts(data_a, byte_len as usize)
        == std::slice::from_raw_parts(data_b, byte_len as usize)
}

/// Compute FNV-1a hash incrementally over concatenated content a||b
/// without allocating the result. Caller guarantees both pointers are
/// valid when their respective lengths are >0.
#[inline(always)]
unsafe fn fnv1a_concat(a: *const StringHeader, a_len: u32, b: *const StringHeader, b_len: u32) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    if a_len > 0 {
        let data = (a as *const u8).add(std::mem::size_of::<StringHeader>());
        for i in 0..a_len as usize {
            h ^= *data.add(i) as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    if b_len > 0 {
        let data = (b as *const u8).add(std::mem::size_of::<StringHeader>());
        for i in 0..b_len as usize {
            h ^= *data.add(i) as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// Check if concat(a, b) matches the content of an existing interned string.
/// Caller guarantees pointers are valid when their respective lengths are >0.
#[inline(always)]
unsafe fn concat_content_matches(
    a: *const StringHeader, a_len: u32,
    b: *const StringHeader, b_len: u32,
    existing: *const StringHeader,
) -> bool {
    let ex_data = (existing as *const u8).add(std::mem::size_of::<StringHeader>());
    if a_len > 0 {
        let a_data = (a as *const u8).add(std::mem::size_of::<StringHeader>());
        if std::slice::from_raw_parts(a_data, a_len as usize)
            != std::slice::from_raw_parts(ex_data, a_len as usize)
        {
            return false;
        }
    }
    if b_len > 0 {
        let b_data = (b as *const u8).add(std::mem::size_of::<StringHeader>());
        if std::slice::from_raw_parts(b_data, b_len as usize)
            != std::slice::from_raw_parts(ex_data.add(a_len as usize), b_len as usize)
        {
            return false;
        }
    }
    true
}

/// GC root scanner for the intern table.
pub fn scan_intern_table_roots(mark: &mut dyn FnMut(f64)) {
    unsafe {
        for entry in INTERN_TABLE.iter() {
            if entry.string_ptr != 0 {
                let nanboxed = 0x7FFF_0000_0000_0000u64
                    | (entry.string_ptr as u64 & 0x0000_FFFF_FFFF_FFFFu64);
                mark(f64::from_bits(nanboxed));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_create() {
        let data = b"hello";
        let s = js_string_from_bytes(data.as_ptr(), data.len() as u32);
        assert_eq!(js_string_length(s), 5);
    }

    #[test]
    fn test_string_concat() {
        let a = js_string_from_bytes(b"hello".as_ptr(), 5);
        let b = js_string_from_bytes(b" world".as_ptr(), 6);
        let c = js_string_concat(a, b);
        assert_eq!(js_string_length(c), 11);
        assert_eq!(string_as_str(c), "hello world");
    }

    #[test]
    fn test_string_slice() {
        let s = js_string_from_bytes(b"hello world".as_ptr(), 11);
        let slice = js_string_slice(s, 0, 5);
        assert_eq!(string_as_str(slice), "hello");

        let slice2 = js_string_slice(s, 6, 11);
        assert_eq!(string_as_str(slice2), "world");
    }

    #[test]
    fn test_string_index_of() {
        let s = js_string_from_bytes(b"hello world".as_ptr(), 11);
        let needle = js_string_from_bytes(b"world".as_ptr(), 5);
        assert_eq!(js_string_index_of(s, needle), 6);

        let not_found = js_string_from_bytes(b"xyz".as_ptr(), 3);
        assert_eq!(js_string_index_of(s, not_found), -1);
    }

    #[test]
    fn test_string_split() {
        use crate::array::{js_array_length, js_array_get_f64};

        let s = js_string_from_bytes(b"a,b,c".as_ptr(), 5);
        let delim = js_string_from_bytes(b",".as_ptr(), 1);
        let arr = js_string_split(s, delim);

        assert_eq!(js_array_length(arr), 3);

        // Get the string pointers from the array and verify their contents
        // Note: split() stores NaN-boxed string pointers with STRING_TAG
        const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
        const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

        unsafe {
            // Extract pointer from NaN-boxed value by masking off STRING_TAG
            let ptr0 = (js_array_get_f64(arr, 0).to_bits() & POINTER_MASK) as *const StringHeader;
            let ptr1 = (js_array_get_f64(arr, 1).to_bits() & POINTER_MASK) as *const StringHeader;
            let ptr2 = (js_array_get_f64(arr, 2).to_bits() & POINTER_MASK) as *const StringHeader;

            assert_eq!(string_as_str(ptr0), "a");
            assert_eq!(string_as_str(ptr1), "b");
            assert_eq!(string_as_str(ptr2), "c");
        }
    }

    #[test]
    fn test_string_append_inplace() {
        // First append: creates new string with 2x capacity and refcount=1
        let a = js_string_from_bytes(b"hello".as_ptr(), 5);
        let b = js_string_from_bytes(b" world".as_ptr(), 6);
        let result = js_string_append(a, b);
        assert_eq!(string_as_str(result), "hello world");
        assert_eq!(unsafe { (*result).refcount }, 1); // uniquely owned
        assert!(unsafe { (*result).capacity } >= 22); // 2x capacity

        // Second append: should reuse same allocation (in-place)
        let c = js_string_from_bytes(b"!".as_ptr(), 1);
        let result2 = js_string_append(result, c);
        assert_eq!(result2, result); // Same pointer — in-place append!
        assert_eq!(string_as_str(result2), "hello world!");
        assert_eq!(unsafe { (*result2).refcount }, 1); // still uniquely owned
    }

    #[test]
    fn test_string_append_shared_no_inplace() {
        // Create a string via append (refcount=1)
        let a = js_string_from_bytes(b"hello".as_ptr(), 5);
        let b = js_string_from_bytes(b" ".as_ptr(), 1);
        let result = js_string_append(a, b);
        assert_eq!(unsafe { (*result).refcount }, 1);

        // Mark as shared (simulates `let y = x` in codegen)
        js_string_addref(result);
        assert_eq!(unsafe { (*result).refcount }, 0); // shared

        // Append should NOT be in-place — must allocate fresh
        let c = js_string_from_bytes(b"world".as_ptr(), 5);
        let result2 = js_string_append(result, c);
        assert_ne!(result2, result); // Different pointer — allocated fresh
        assert_eq!(string_as_str(result2), "hello world");
        assert_eq!(string_as_str(result), "hello "); // Original unchanged
    }

    #[test]
    fn test_string_append_self() {
        // Self-append (s += s) must always allocate fresh
        let a = js_string_from_bytes(b"ab".as_ptr(), 2);
        let result = js_string_append(a, a);
        assert_eq!(string_as_str(result), "abab");
    }

    #[test]
    fn test_string_append_loop() {
        // Simulate the common loop pattern: result = result + "x" repeated
        let mut result = js_string_from_bytes(b"".as_ptr(), 0);
        let x = js_string_from_bytes(b"x".as_ptr(), 1);
        let mut inplace_count = 0u32;
        for _ in 0..1000 {
            let old_ptr = result;
            result = js_string_append(result, x);
            if result == old_ptr {
                inplace_count += 1;
            }
        }
        assert_eq!(js_string_length(result), 1000);
        // Most appends should be in-place (only ~10 re-allocations for 1000 appends)
        assert!(inplace_count > 980, "Expected >980 in-place appends, got {}", inplace_count);
    }
}
