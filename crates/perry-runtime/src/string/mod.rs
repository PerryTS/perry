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

// ── Submodules (topical split of the original `string.rs`) ─────────────
//
// Each sibling uses `use super::*;` and re-imports the shared helpers
// kept in this file. We re-export the public/FFI surface explicitly
// below so external callers see the same names as before.

mod alloc;
mod append;
mod base64_codec;
mod char_ops;
mod compare;
mod concat;
mod format;
mod html;
mod intern;
mod io;
mod iter_object;
mod locale;
mod pad;
mod raw;
mod slice_ops;
mod split;
#[cfg(feature = "regex-engine")]
pub(crate) use split::spec_regex_split;

#[cfg(test)]
mod tests;

// Explicit named re-exports — preserve the original `crate::string::*`
// surface 1:1. NO glob re-exports.
pub use alloc::{
    js_string_addref, js_string_addref_if_heap_string, js_string_builder_new, js_string_from_bytes,
    js_string_from_bytes_longlived, js_string_from_bytes_with_capacity, js_string_from_wtf8_bytes,
    js_string_length, js_string_materialize_to_heap, js_string_new_sso,
};
pub use append::js_string_append;
pub use base64_codec::{js_atob, js_btoa};
pub use char_ops::{
    js_string_at, js_string_char_at, js_string_char_code_at, js_string_code_point_at,
    js_string_end_index_to_i32, js_string_from_char_code, js_string_from_char_code_array,
    js_string_from_code_point, js_string_from_code_point_array, js_string_index_get,
    js_string_index_to_i32, js_string_to_char_array,
};
pub use compare::{
    js_string_compare, js_string_ends_with, js_string_ends_with_at, js_string_equals,
    js_string_is_well_formed, js_string_locale_compare, js_string_locale_compare_opts,
    js_string_normalize, js_string_search_value_to_string, js_string_starts_with,
    js_string_starts_with_at, js_string_to_well_formed,
};
// #1781: SSO-aware key lookup helpers, used to retire the
// `is_string() && js_string_equals(key, key_val.as_string_ptr())` shape
// across object/.
pub(crate) use compare::{js_string_key_bytes, js_string_key_matches, js_string_key_matches_bytes};
pub use concat::{
    js_string_concat, js_string_concat_box, js_string_concat_chain, js_string_concat_value,
    js_value_concat_string,
};
pub(crate) use format::fix_exponent_format;
pub(crate) use format::js_format_f64;
pub use format::{
    js_number_to_exponential, js_number_to_fixed, js_number_to_precision, js_number_to_string,
    scan_small_int_cache_roots, scan_small_int_cache_roots_mut,
};
pub use html::{
    js_string_anchor, js_string_big, js_string_blink, js_string_bold, js_string_fixed,
    js_string_fontcolor, js_string_fontsize, js_string_italics, js_string_link, js_string_small,
    js_string_strike, js_string_sub, js_string_sup,
};
pub use intern::{js_string_intern, scan_intern_table_roots, scan_intern_table_roots_mut};
pub use io::{js_string_error, js_string_print, js_string_warn};
pub use iter_object::{
    dispatch_string_iterator_method, string_values_iter, STRING_ITERATOR_CLASS_ID,
};
pub use locale::{
    js_string_to_locale_lower_case, js_string_to_locale_upper_case, js_string_validate_locales,
};
pub use pad::{js_string_alloc_space, js_string_pad_end, js_string_pad_start, js_string_repeat};
pub use raw::js_string_raw;
pub(crate) use slice_ops::is_js_whitespace;
pub use slice_ops::{
    js_string_index_of, js_string_index_of_from, js_string_last_index_of,
    js_string_last_index_of_from, js_string_slice, js_string_substr, js_string_substring,
    js_string_to_lower_case, js_string_to_upper_case, js_string_trim, js_string_trim_end,
    js_string_trim_start,
};
pub use split::{js_string_split, js_string_split_n};

#[cfg(test)]
pub(crate) use intern::{
    test_clear_intern_table_root, test_intern_table_root, test_seed_intern_table_root,
};

#[cfg(test)]
pub(crate) use format::{
    test_clear_small_int_cache_root, test_seed_small_int_cache_root, test_small_int_cache_root,
};

/// Flag: string bytes contain WTF-8 lone-surrogate sequences (U+D800..U+DFFF).
/// Set by js_string_from_wtf8_bytes. Checked by isWellFormed/toWellFormed.
pub const STRING_FLAG_HAS_LONE_SURROGATES: u32 = 1;

/// A static empty string that can be used as a safe fallback for null pointers.
/// Has utf16_len=0, byte_len=0, capacity=0, refcount=0, flags=0 (shared).
#[no_mangle]
pub static PERRY_EMPTY_STRING: StringHeader = StringHeader {
    utf16_len: 0,
    byte_len: 0,
    capacity: 0,
    refcount: 0,
    flags: 0,
};

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

/// Borrowed byte view for a Perry string-like dispatch key.
///
/// Static dispatch IDs currently use the raw interned `StringHeader*` pointer
/// payload. Newer lowering paths may naturally carry a full NaN-boxed string
/// value, including `SHORT_STRING_TAG`. This view lets by-ID wrappers accept
/// both forms without open-coding heap-only string reads at each callsite.
#[derive(Clone, Copy)]
pub struct PerryStringRef {
    pub ptr: *const u8,
    pub len: usize,
    pub heap: *const StringHeader,
}

/// Resolve a static property/method id into a byte view.
///
/// Accepted forms:
/// - raw interned `StringHeader*` pointer payload (today's StringPool id);
/// - boxed heap `STRING_TAG` bits;
/// - boxed inline `SHORT_STRING_TAG` bits, copied into `scratch`.
#[inline]
pub fn perry_string_ref_from_dispatch_id(
    id: i64,
    scratch: &mut [u8; crate::value::SHORT_STRING_MAX_LEN],
) -> Option<PerryStringRef> {
    if id == 0 {
        return None;
    }

    let bits = id as u64;
    let tag = bits & crate::value::TAG_MASK;
    if tag == crate::value::STRING_TAG || tag == crate::value::SHORT_STRING_TAG {
        return str_bytes_from_jsvalue(f64::from_bits(bits), scratch).map(|(ptr, len)| {
            let jsval = crate::value::JSValue::from_bits(bits);
            PerryStringRef {
                ptr,
                len: len as usize,
                heap: if jsval.is_string() {
                    jsval.as_string_ptr()
                } else {
                    std::ptr::null()
                },
            }
        });
    }

    let addr = id as usize;
    let hdr = addr as *const StringHeader;
    if !is_valid_string_ptr(hdr) || (addr & 0x7) != 0 {
        return None;
    }
    if matches!(
        crate::arena::classify_heap_space(addr),
        crate::arena::HeapSpace::Unknown
    ) {
        return None;
    }
    unsafe {
        Some(PerryStringRef {
            ptr: (hdr as *const u8).add(std::mem::size_of::<StringHeader>()),
            len: (*hdr).byte_len as usize,
            heap: hdr,
        })
    }
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
///
/// Defensive against invalid UTF-8 input: `str::from_utf8_unchecked` is UB on
/// non-UTF-8 bytes and `.encode_utf16().count()` walking via `chars()` reads
/// past the slice end, surfacing as a SIGSEGV in the multi-byte handler.
/// Issue #609 hit this when `@perryts/mysql` fell through to
/// `Buffer.from(String(buf), 'utf8')` for a random-bytes Buffer parameter
/// — the load-bearing call chain was `Buffer.toString()` →
/// `js_buffer_to_string` → `js_string_from_bytes` → here.
///
/// Caller paths that already validated UTF-8 (codegen string-literal init,
/// JSON parser, `from_utf8`-fronted runtime helpers) hit the same fast path
/// they did before. Untrusted callers that hand us raw Buffer bytes now
/// fall back to a byte-walking WTF-8-shape counter that never reads past
/// the slice end.
#[inline]
pub(crate) fn compute_utf16_len(data: *const u8, byte_len: u32) -> u32 {
    if data.is_null() || byte_len == 0 {
        return 0;
    }
    let bytes = unsafe { slice::from_raw_parts(data, byte_len as usize) };
    // ASCII fast path: if no byte has high bit set, utf16_len == byte_len
    if bytes.iter().all(|&b| b < 0x80) {
        return byte_len;
    }
    match str::from_utf8(bytes) {
        Ok(s) => s.encode_utf16().count() as u32,
        Err(_) => compute_utf16_len_wtf8(bytes),
    }
}

/// Convert a UTF-16 code unit index to a UTF-8 byte offset.
/// Returns `s.len()` if `utf16_idx` is past the end.
#[inline]
pub(crate) fn utf16_offset_to_byte_offset(s: &str, utf16_idx: usize) -> usize {
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
pub(crate) fn byte_offset_to_utf16_index(s: &str, byte_off: usize) -> usize {
    if byte_off == 0 {
        return 0;
    }
    s[..byte_off].encode_utf16().count()
}

/// Heap storage policy for `StringHeader` strings.
///
/// - `js_string_new_sso` returns inline `SHORT_STRING_TAG` values for short boxed
///   strings that do not require a real `StringHeader*`.
/// - Every heap `StringHeader` allocation uses GC-managed arenas, not
///   `gc_malloc`, so it stays out of `MALLOC_STATE`.
/// - `arena_alloc_gc` routes small and medium payloads to nursery pages and
///   large payloads to old-gen pages using `LARGE_OBJECT_THRESHOLD_BYTES`.
///
/// Keep this helper as the single normal heap-string storage entry point. Other
/// `GC_TYPE_STRING` users, notably `SymbolHeader` and JSON tape scratch buffers,
/// are compatibility residents with different layouts and should not be forced
/// through `StringHeader` initialization.
#[inline]
pub(crate) fn string_storage_alloc(capacity: u32) -> (*mut StringHeader, *mut u8) {
    let payload_size = std::mem::size_of::<StringHeader>() + capacity as usize;
    let raw = crate::arena::arena_alloc_gc(payload_size, 8, crate::gc::GC_TYPE_STRING);
    let ptr = raw as *mut StringHeader;
    let data = unsafe { raw.add(std::mem::size_of::<StringHeader>()) };
    (ptr, data)
}

#[inline]
pub(crate) fn string_storage_alloc_longlived(capacity: u32) -> (*mut StringHeader, *mut u8) {
    let payload_size = std::mem::size_of::<StringHeader>() + capacity as usize;
    let raw = crate::arena::arena_alloc_gc_longlived(payload_size, 8, crate::gc::GC_TYPE_STRING);
    let ptr = raw as *mut StringHeader;
    let data = unsafe { raw.add(std::mem::size_of::<StringHeader>()) };
    (ptr, data)
}

#[inline]
pub(crate) unsafe fn init_string_header(
    ptr: *mut StringHeader,
    utf16_len: u32,
    byte_len: u32,
    capacity: u32,
    refcount: u32,
    flags: u32,
) {
    debug_assert!(byte_len <= capacity);
    (*ptr).utf16_len = utf16_len;
    (*ptr).byte_len = byte_len;
    (*ptr).capacity = capacity;
    (*ptr).refcount = refcount;
    (*ptr).flags = flags;
}

#[inline]
pub(crate) fn js_string_from_bytes_known_utf16(
    data: *const u8,
    len: u32,
    utf16_len: u32,
    flags: u32,
) -> *mut StringHeader {
    let (ptr, data_ptr) = string_storage_alloc(len);
    unsafe {
        init_string_header(ptr, utf16_len, len, len, 0, flags);
        if len > 0 && !data.is_null() {
            ptr::copy_nonoverlapping(data, data_ptr, len as usize);
        }
    }
    ptr
}

/// SSO-aware decoder. Returns `Some((ptr, len))` view over the
/// bytes of a string JSValue, regardless of representation:
/// - Heap `STRING_TAG` → returns the `StringHeader`'s data pointer
///   + `byte_len`.
/// - Inline `SHORT_STRING_TAG` → copies into the caller's scratch
///   buffer (which must live at least `SHORT_STRING_MAX_LEN` bytes)
///   and returns a pointer into it.
/// - Anything else → `None`.
///
/// Safety: the returned pointer is valid for the lifetime of either
/// (a) the underlying `StringHeader`, OR (b) the caller-owned
/// `scratch` buffer. Callers must not hold this pointer past a
/// subsequent `scratch` modification or a GC cycle that could sweep
/// the heap-backed `StringHeader`.
#[inline]
pub fn str_bytes_from_jsvalue(
    value: f64,
    scratch: &mut [u8; crate::value::SHORT_STRING_MAX_LEN],
) -> Option<(*const u8, u32)> {
    let bits = value.to_bits();
    let jsval = crate::value::JSValue::from_bits(bits);
    unsafe {
        if jsval.is_short_string() {
            let n = jsval.short_string_to_buf(scratch);
            return Some((scratch.as_ptr(), n as u32));
        }
        if jsval.is_string() {
            let hdr = jsval.as_string_ptr();
            if hdr.is_null() {
                return Some((std::ptr::null(), 0));
            }
            let data = (hdr as *const u8).add(std::mem::size_of::<StringHeader>());
            return Some((data, (*hdr).byte_len));
        }
    }
    None
}

/// Fast path: create a string from bytes known to be pure ASCII.
/// Skips the `compute_utf16_len` byte scan — sets utf16_len = byte_len directly.
#[inline]
pub(crate) fn js_string_from_ascii_bytes(data: *const u8, len: u32) -> *mut StringHeader {
    js_string_from_bytes_known_utf16(data, len, len, 0)
}

/// Allocate an uninitialised ASCII-typed string of `len` bytes and return
/// `(header_ptr, data_ptr)`. Caller MUST write all `len` bytes into the data
/// region before any read (other than `byte_len`) observes them.
///
/// Use case: encoders that produce known-ASCII output (hex, base64) where
/// the caller can write directly into the StringHeader's payload — avoids
/// an intermediate `Vec<u8>` allocation + a follow-up `copy_nonoverlapping`.
#[inline]
pub(crate) fn js_string_alloc_ascii_uninit(len: u32) -> (*mut StringHeader, *mut u8) {
    let (ptr, data_ptr) = string_storage_alloc(len);
    unsafe {
        init_string_header(ptr, len, len, len, 0, 0);
    }
    (ptr, data_ptr)
}

/// GC-safe copy of `byte_len` bytes starting at `byte_start` of source string
/// `s` into a freshly allocated `StringHeader`.
///
/// Why this exists (issue #5062): the normal `js_string_from_bytes` family
/// allocates the destination *before* copying, and `string_storage_alloc` can
/// trip a moving/sweeping GC. Slice/substring fast paths hand it a raw pointer
/// derived from a GC-managed source string (`string_data(s) + offset`), so if a
/// collection relocates or sweeps `s` during the destination allocation, the
/// subsequent copy reads from a dangling buffer. Under sustained server GC
/// pressure this surfaced as a chunked `String.prototype.slice` loop stamping
/// the new slice's own length word over the first bytes of the payload.
///
/// Rooting `s` in a `RuntimeHandleScope` keeps it alive across the allocation
/// AND refreshes its address if the GC moves it, so the copy always reads from
/// the live source. `byte_start`/`byte_len` are byte offsets into the source
/// payload (callers translate UTF-16 indices first); `utf16_len`/`flags` become
/// the new header's fields.
pub(crate) fn string_copy_range(
    s: *const StringHeader,
    byte_start: usize,
    byte_len: u32,
    utf16_len: u32,
    flags: u32,
) -> *mut StringHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let handle = scope.root_string_ptr(s);
    let (ptr, data_ptr) = string_storage_alloc(byte_len);
    unsafe {
        init_string_header(ptr, utf16_len, byte_len, byte_len, 0, flags);
        if byte_len > 0 {
            // Re-read the (possibly relocated) source AFTER the allocation.
            let s_now = handle.get_raw_const_ptr::<StringHeader>();
            let src = string_data(s_now).add(byte_start);
            ptr::copy_nonoverlapping(src, data_ptr, byte_len as usize);
        }
    }
    ptr
}

/// Count UTF-16 code units for a WTF-8 byte slice without using from_utf8.
/// Lone surrogate sequences (0xED 0xA0..0xBF 0x80..0xBF) each count as 1 unit,
/// same as any other BMP codepoint. Astral sequences (4-byte) count as 2.
#[inline]
pub(crate) fn compute_utf16_len_wtf8(bytes: &[u8]) -> u32 {
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

/// Internal helper: Create a StringHeader from a Rust &str
#[inline]
pub(crate) fn js_string_from_str(s: &str) -> *mut StringHeader {
    js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

/// Get the data pointer for a string
pub(crate) fn string_data(s: *const StringHeader) -> *const u8 {
    unsafe { (s as *const u8).add(std::mem::size_of::<StringHeader>()) }
}

/// Get string as a Rust &str (for internal use)
pub(crate) fn string_as_str<'a>(s: *const StringHeader) -> &'a str {
    unsafe {
        let blen = (*s).byte_len as usize;
        let cap = (*s).capacity as usize;
        debug_assert!(
            blen <= cap,
            "StringHeader byte_len {} > capacity {}",
            blen,
            cap
        );
        let data = string_data(s);
        let bytes = slice::from_raw_parts(data, blen);
        str::from_utf8_unchecked(bytes)
    }
}

/// Check if string is pure ASCII (utf16_len == byte_len → all single-byte chars)
#[inline]
pub(crate) fn is_ascii_string(s: *const StringHeader) -> bool {
    unsafe { (*s).utf16_len == (*s).byte_len }
}
