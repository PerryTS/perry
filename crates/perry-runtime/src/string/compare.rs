//! Equality / comparison / starts-with / ends-with / well-formedness /
//! normalization / locale-compare.

use super::*;

/// Lexicographic comparison of two UTF-8 byte slices by **UTF-16 code unit**,
/// matching ECMAScript string relational comparison (`<`/`>` and the default
/// `Array.prototype.sort` order), which compares UTF-16 code units — NOT Unicode
/// code points. The two orders agree for BMP-only strings but diverge once an
/// astral character (code point > U+FFFF) is involved: its UTF-16 surrogate pair
/// leads with 0xD800–0xDBFF, sorting it *before* BMP characters in 0xE000–0xFFFF,
/// whereas raw UTF-8 byte order (= code-point order) sorts it after. Falls back
/// to raw byte order if either side is not valid UTF-8 (WTF-8 lone surrogates —
/// a known categorical gap).
///
/// # ASCII fast path
///
/// When **both** payloads are pure ASCII, every byte is its own UTF-16 code
/// unit (0x00–0x7F zero-extends to 0x0000–0x007F), so lexicographic byte order
/// and lexicographic UTF-16 code-unit order are the *same total order* — the
/// prefix tie-break included, since `<[u8]>::cmp` and `Iterator::cmp` both rank
/// a proper prefix `Less`. The fast path therefore returns `a.cmp(b)` (a
/// `memcmp` + length compare) and skips the two `from_utf8` validations and the
/// two scalar `encode_utf16` decoder iterators the general path runs.
///
/// The precondition is **checked, never assumed**. Perry heap-string payloads
/// are not guaranteed valid UTF-8 (WTF-8 lone surrogates, `Buffer.toString`
/// of arbitrary bytes, FFI blobs — #6085), so this must not lean on any
/// derived metadata:
///
/// * `<[u8]>::is_ascii` inspects the actual bytes word-at-a-time and is total
///   over arbitrary byte strings — no validity assumption at all.
/// * The header-cached predicate `is_ascii_string` (`utf16_len == byte_len`)
///   would be *cheaper* but is **not sound** here: `compute_utf16_len_wtf8`
///   charges a truncated multi-byte lead its full nominal unit count while the
///   payload holds fewer bytes, so e.g. `[0xC3]` records `utf16_len == 1 ==
///   byte_len` and `[0xF0, 0x41]` records `utf16_len == 2 == byte_len` — both
///   non-ASCII payloads that the cached predicate calls ASCII. Byte-scanning is
///   the only precondition that holds for non-UTF-8 payloads.
///
/// Mixed operands (one ASCII, one not) deliberately fall through unchanged
/// rather than reasoning about lead-byte ranges: the general path already
/// handles them, and this stays a decision about *both* operands.
#[inline]
pub(crate) fn utf16_cmp_bytes(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    // For short common prefixes, find the first unequal byte a word at a
    // time. An ASCII difference has the same order in UTF-8 and UTF-16 even
    // if the equal prefix contains Unicode; no scan of the suffix is needed.
    // A non-ASCII difference retains the full UTF-16/invalid-byte behavior.
    let common = a.len().min(b.len());
    if common <= 32 {
        let mut offset = 0;
        while offset + 8 <= common {
            let left = u64::from_le_bytes(a[offset..offset + 8].try_into().unwrap());
            let right = u64::from_le_bytes(b[offset..offset + 8].try_into().unwrap());
            let unequal = left ^ right;
            if unequal != 0 {
                return compare_unequal_words(left, right, a, b);
            }
            offset += 8;
        }
        if offset < common && common >= 8 {
            // Load the final word inside the payload, overlapping the equal
            // prefix instead of comparing a short tail one byte at a time.
            let tail = common - 8;
            let left = u64::from_le_bytes(a[tail..common].try_into().unwrap());
            let right = u64::from_le_bytes(b[tail..common].try_into().unwrap());
            if left != right {
                return compare_unequal_words(left, right, a, b);
            }
            return a.len().cmp(&b.len());
        }
        while offset < common {
            let (x, y) = (a[offset], b[offset]);
            if x != y {
                return if (x | y) < 0x80 {
                    x.cmp(&y)
                } else {
                    utf16_cmp_bytes_full(a, b)
                };
            }
            offset += 1;
        }
        return a.len().cmp(&b.len());
    }
    utf16_cmp_bytes_full(a, b)
}

#[inline]
fn compare_unequal_words(left: u64, right: u64, a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    if (left | right) & 0x8080_8080_8080_8080 == 0 {
        return left.swap_bytes().cmp(&right.swap_bytes());
    }
    let shift = (left ^ right).trailing_zeros() & !7;
    let (x, y) = ((left >> shift) as u8, (right >> shift) as u8);
    if (x | y) < 0x80 {
        x.cmp(&y)
    } else {
        utf16_cmp_bytes_full(a, b)
    }
}

fn utf16_cmp_bytes_full(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    if a.is_ascii() && b.is_ascii() {
        return a.cmp(b);
    }
    match (std::str::from_utf8(a), std::str::from_utf8(b)) {
        (Ok(a_str), Ok(b_str)) => a_str.encode_utf16().cmp(b_str.encode_utf16()),
        _ => a.cmp(b),
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
        match utf16_cmp_bytes(a_bytes, b_bytes) {
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
        if slice_a == slice_b {
            1
        } else {
            0
        }
    }
}

/// Repsel Phase 3a: relational comparison over NaN-boxed operands that may
/// be heap strings (`STRING_TAG`) or inline SSO values (`SHORT_STRING_TAG`)
/// in any mix — the boxed counterpart of `js_string_compare`, used by the
/// canonical-Str compare lowering's non-proven-heap arm. Decodes SSO
/// operands through a stack scratch buffer (no heap materialization).
///
/// Semantics for non-string operands mirror the legacy
/// `js_get_string_pointer_unified` → `js_string_compare` composition this
/// arm replaces: a plain number compares by its decimal string form; every
/// other non-string value ranks like `js_string_compare`'s invalid-pointer
/// handling (invalid < any valid string; two invalids compare equal).
/// Returns -1 / 0 / 1.
#[no_mangle]
pub extern "C" fn js_string_compare_value(a: f64, b: f64) -> i32 {
    if let Some(order) = compare_primitive_strings(a, b) {
        return order;
    }
    // Phase 1 — ALLOCATING coercions only. `js_number_to_string` allocates,
    // and an allocation can run a GC cycle that MOVES the other operand's
    // heap string (evacuation); the decimal bytes are therefore copied into
    // an owned `Vec` immediately, and no raw heap-string pointer may exist
    // yet. Both operands' coercions complete before phase 2 takes any view.
    fn number_bytes(v: f64) -> Option<Vec<u8>> {
        if !crate::JSValue::from_bits(v.to_bits()).is_number() {
            return None;
        }
        // Mirror the unified helper's number → decimal-string coercion.
        let s = crate::string::js_number_to_string(v);
        if !crate::string::is_valid_string_ptr(s) {
            return None;
        }
        unsafe {
            let len = (*s).byte_len;
            let data = crate::string::string_data(s);
            Some(std::slice::from_raw_parts(data, len as usize).to_vec())
        }
    }
    let a_num = number_bytes(a);
    let b_num = number_bytes(b);

    // Phase 2 — NON-allocating views only (heap payload pointers, SSO
    // scratch decode, or the owned number buffers). Nothing below allocates,
    // so the raw `from_raw_parts` reads cannot observe a moved string.
    fn view_of<'s>(
        v: f64,
        scratch: &'s mut [u8; crate::value::SHORT_STRING_MAX_LEN],
        num_buf: &'s Option<Vec<u8>>,
    ) -> Option<(*const u8, u32)> {
        if let Some(view) = crate::string::str_bytes_from_jsvalue(v, scratch) {
            return Some(view);
        }
        num_buf.as_ref().map(|buf| (buf.as_ptr(), buf.len() as u32))
    }
    let mut a_scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let mut b_scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let a_view = view_of(a, &mut a_scratch, &a_num);
    let b_view = view_of(b, &mut b_scratch, &b_num);
    match (a_view, b_view) {
        (None, None) => 0,
        (None, Some(_)) => -1,
        (Some(_), None) => 1,
        (Some((a_ptr, a_len)), Some((b_ptr, b_len))) => unsafe {
            let a_bytes = std::slice::from_raw_parts(a_ptr, a_len as usize);
            let b_bytes = std::slice::from_raw_parts(b_ptr, b_len as usize);
            match utf16_cmp_bytes(a_bytes, b_bytes) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            }
        },
    }
}

/// Compare primitive strings without entering the allocating number-to-string
/// adapter. Both heap and inline strings use the same UTF-16 ordering.
#[inline(always)]
pub(crate) fn compare_primitive_strings(a: f64, b: f64) -> Option<i32> {
    let a_value = crate::JSValue::from_bits(a.to_bits());
    let b_value = crate::JSValue::from_bits(b.to_bits());
    if a_value.is_string() && b_value.is_string() {
        if a.to_bits() == b.to_bits() {
            return Some(0);
        }
        // Heap strings need no inline-string scratch storage. Keep that
        // representation adapter out of the hot frame entirely. No allocation
        // or callback can invalidate either borrowed byte view here.
        unsafe {
            let a_ptr = a_value.as_string_ptr();
            let b_ptr = b_value.as_string_ptr();
            let a_bytes = if a_ptr.is_null() {
                &[]
            } else {
                std::slice::from_raw_parts(string_data(a_ptr), (*a_ptr).byte_len as usize)
            };
            let b_bytes = if b_ptr.is_null() {
                &[]
            } else {
                std::slice::from_raw_parts(string_data(b_ptr), (*b_ptr).byte_len as usize)
            };
            return Some(match utf16_cmp_bytes(a_bytes, b_bytes) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            });
        }
    }
    if !a_value.is_any_string() || !b_value.is_any_string() {
        return None;
    }
    Some(compare_inline_or_mixed_strings(a, b))
}

#[inline(never)]
fn compare_inline_or_mixed_strings(a: f64, b: f64) -> i32 {
    if a.to_bits() == b.to_bits() {
        return 0;
    }
    let mut a_scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
    let mut b_scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
    let (a_ptr, a_len) = crate::string::str_bytes_from_jsvalue(a, &mut a_scratch).unwrap();
    let (b_ptr, b_len) = crate::string::str_bytes_from_jsvalue(b, &mut b_scratch).unwrap();
    // There is no allocation or user-code window while these views are live.
    // A null heap-string payload is the legacy empty view; from_raw_parts
    // itself still requires a non-null pointer even for an empty slice.
    let a_bytes = if a_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(a_ptr, a_len as usize) }
    };
    let b_bytes = if b_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(b_ptr, b_len as usize) }
    };
    match utf16_cmp_bytes(a_bytes, b_bytes) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// SSO-aware key match: compare a stored-key `JSValue` (which may be a
/// `STRING_TAG` heap pointer OR a `SHORT_STRING_TAG` inline SSO value)
/// against an incoming heap `*const StringHeader` key.
///
/// This is the safe replacement for the `key_val.is_string() && js_string_equals(key, key_val.as_string_ptr())`
/// pattern that recurs in `object/field_get_set.rs`, `object/object_ops.rs`,
/// `object/delete_rest.rs`, etc. — `is_string()` is STRING_TAG-only, so
/// any SSO-stored key is silently skipped, which makes `Object.keys`,
/// `key in obj`, `delete obj[k]`, `obj[k] = v`, and `Object.assign`
/// drop or duplicate keys whose name is ≤ 5 ASCII bytes (#1781).
///
/// Returns `true` iff the stored value is some kind of string AND its
/// byte contents are equal to the incoming heap key. Returns `false`
/// for non-string stored values or a null incoming key.
///
/// Inline byte comparison — no allocation, no heap materialization of
/// the SSO operand. Safe on the hot path.
#[inline]
pub(crate) unsafe fn js_string_key_matches(
    stored: crate::JSValue,
    incoming: *const StringHeader,
) -> bool {
    if incoming.is_null() {
        return false;
    }
    // Heap-stored key: defer to the existing equals routine.
    if stored.is_string() {
        return js_string_equals(incoming, stored.as_string_ptr()) != 0;
    }
    // SSO-stored key: compare the incoming heap bytes against the
    // inline SSO bytes without materializing the SSO to the heap.
    if stored.is_short_string() {
        let incoming_len = (*incoming).byte_len as usize;
        let sso_len = stored.short_string_len();
        if incoming_len != sso_len {
            return false;
        }
        let incoming_data = (incoming as *const u8).add(std::mem::size_of::<StringHeader>());
        let incoming_bytes = std::slice::from_raw_parts(incoming_data, incoming_len);
        let mut sso_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = stored.short_string_to_buf(&mut sso_buf);
        return &sso_buf[..n] == incoming_bytes;
    }
    false
}

/// SSO-aware byte-slice match for cases where the incoming key already
/// lives as a `&[u8]` slice (typed-feedback guards, `js_object_get_own_field_or_undef`,
/// etc.) — same SSO blind-spot fix as [`js_string_key_matches`] but
/// without the round-trip through a heap `StringHeader` for the
/// incoming side. Returns `true` iff the stored value is some kind of
/// string and its bytes equal `incoming_bytes`.
#[inline]
pub(crate) unsafe fn js_string_key_matches_bytes(
    stored: crate::JSValue,
    incoming_bytes: &[u8],
) -> bool {
    if stored.is_string() {
        let stored_ptr = stored.as_string_ptr();
        if stored_ptr.is_null() {
            return false;
        }
        let stored_len = (*stored_ptr).byte_len as usize;
        if stored_len != incoming_bytes.len() {
            return false;
        }
        let stored_data = (stored_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let stored_slice = std::slice::from_raw_parts(stored_data, stored_len);
        return stored_slice == incoming_bytes;
    }
    if stored.is_short_string() {
        let sso_len = stored.short_string_len();
        if sso_len != incoming_bytes.len() {
            return false;
        }
        let mut sso_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = stored.short_string_to_buf(&mut sso_buf);
        return &sso_buf[..n] == incoming_bytes;
    }
    false
}

/// Extract the bytes of a stored-key JSValue (STRING_TAG or SHORT_STRING_TAG)
/// into a caller-provided buffer + length. Returns `None` for non-string
/// stored values. The slice borrowing into either the SSO buffer
/// (`stored_buf`) or the heap pointer is the caller's responsibility.
///
/// Used by paths like `Object.keys` and `Object.assign` that need to
/// materialize the key string into a usable form regardless of which
/// representation it currently has.
#[inline]
pub(crate) unsafe fn js_string_key_bytes(
    stored: crate::JSValue,
    stored_buf: &mut [u8; crate::value::SHORT_STRING_MAX_LEN],
) -> Option<&[u8]> {
    if stored.is_string() {
        let stored_ptr = stored.as_string_ptr();
        if stored_ptr.is_null() {
            return None;
        }
        let len = (*stored_ptr).byte_len as usize;
        let data = (stored_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        return Some(std::slice::from_raw_parts(data, len));
    }
    if stored.is_short_string() {
        let n = stored.short_string_to_buf(stored_buf);
        return Some(&stored_buf[..n]);
    }
    None
}

/// Validate and coerce the search string for String.prototype.includes,
/// startsWith, and endsWith.
///
/// The ECMAScript path is IsRegExp(searchString) before ToString(searchString):
/// a real RegExp or an object with truthy Symbol.match must throw, while
/// Symbol.match === false/null explicitly opts out and then stringifies.
#[no_mangle]
pub extern "C" fn js_string_search_value_to_string(
    value: f64,
    method_id: i32,
) -> *mut StringHeader {
    if string_search_is_regexp(value) {
        throw_regexp_search_type_error(method_id);
    }
    // ToString(searchString): a Symbol throws a TypeError (§7.1.17) rather than
    // stringifying to "Symbol(...)".
    crate::builtins::reject_symbol_to_string(value);
    crate::value::js_jsvalue_to_string(value)
}

fn string_search_is_regexp(value: f64) -> bool {
    let jsval = crate::value::JSValue::from_bits(value.to_bits());
    if !jsval.is_pointer() {
        return false;
    }

    let raw_ptr = jsval.as_pointer::<u8>() as usize;
    if raw_ptr < 0x10000 || crate::symbol::is_registered_symbol(raw_ptr) {
        return false;
    }

    let match_sym = crate::symbol::well_known_symbol("match");
    if !match_sym.is_null() {
        let match_sym_f64 =
            f64::from_bits(crate::value::JSValue::pointer(match_sym as *const u8).bits());
        let matcher = unsafe { crate::symbol::js_object_get_symbol_property(value, match_sym_f64) };
        if matcher.to_bits() != crate::value::TAG_UNDEFINED {
            return crate::value::js_is_truthy(matcher) != 0;
        }
    }

    crate::regex::is_regex_pointer(jsval.as_pointer::<u8>())
}

fn throw_regexp_search_type_error(method_id: i32) -> ! {
    let method = match method_id {
        1 => "startsWith",
        2 => "endsWith",
        _ => "includes",
    };
    let message =
        format!("First argument to String.prototype.{method} must not be a regular expression");
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Check if a string starts with a prefix
#[no_mangle]
pub extern "C" fn js_string_starts_with(
    s: *const StringHeader,
    prefix: *const StringHeader,
) -> i32 {
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

/// Check if a string starts with `prefix` at UTF-16 code-unit `position`.
/// Mirrors `String.prototype.startsWith(searchString, position)` — clamps
/// negative positions to 0 and positions past the end to length.
#[no_mangle]
pub extern "C" fn js_string_starts_with_at(
    s: *const StringHeader,
    prefix: *const StringHeader,
    position: i32,
) -> i32 {
    if !is_valid_string_ptr(s) || !is_valid_string_ptr(prefix) {
        return 0;
    }

    let u16len = unsafe { (*s).utf16_len } as i32;
    let pos = position.max(0).min(u16len) as usize;

    let prefix_blen = unsafe { (*prefix).byte_len } as usize;

    let byte_start = if is_ascii_string(s) {
        pos
    } else {
        utf16_offset_to_byte_offset(string_as_str(s), pos)
    };

    let blen = unsafe { (*s).byte_len } as usize;
    if byte_start + prefix_blen > blen {
        return 0;
    }

    unsafe {
        let data = string_data(s).add(byte_start);
        let prefix_data = string_data(prefix);
        for i in 0..prefix_blen {
            if *data.add(i) != *prefix_data.add(i) {
                return 0;
            }
        }
    }

    1
}

/// Check if a string ends with `suffix` if truncated to UTF-16 code-unit
/// `end_position`. Mirrors `String.prototype.endsWith(searchString, endPosition)`
/// — clamps negative positions to 0 and positions past the end to length.
#[no_mangle]
pub extern "C" fn js_string_ends_with_at(
    s: *const StringHeader,
    suffix: *const StringHeader,
    end_position: i32,
) -> i32 {
    if !is_valid_string_ptr(s) || !is_valid_string_ptr(suffix) {
        return 0;
    }

    let u16len = unsafe { (*s).utf16_len } as i32;
    let end_u16 = end_position.max(0).min(u16len) as usize;

    let byte_end = if is_ascii_string(s) {
        end_u16
    } else {
        utf16_offset_to_byte_offset(string_as_str(s), end_u16)
    };

    let suffix_blen = unsafe { (*suffix).byte_len } as usize;
    if suffix_blen > byte_end {
        return 0;
    }

    let byte_start = byte_end - suffix_blen;

    unsafe {
        let data = string_data(s).add(byte_start);
        let suffix_data = string_data(suffix);
        for i in 0..suffix_blen {
            if *data.add(i) != *suffix_data.add(i) {
                return 0;
            }
        }
    }

    1
}

/// String.prototype.normalize(form) — Unicode normalization.
///
/// `form_value` is the raw NaN-boxed argument (or NaN-boxed `undefined`
/// when the call site omitted it). Per ECMA-262 §22.1.3.13: when `form` is
/// `undefined` the form defaults to `"NFC"`; otherwise the form is coerced
/// with `ToString` and must be exactly one of `"NFC"`, `"NFD"`, `"NFKC"`,
/// `"NFKD"` — anything else (including explicit `null` → `"null"`, the empty
/// string, or `"BAD"`) throws a `RangeError`. (#2782)
#[no_mangle]
pub extern "C" fn js_string_normalize(
    s: *const StringHeader,
    form_value: f64,
) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(std::ptr::null(), 0);
    }

    // `undefined` (omitted argument) → default NFC. Note: explicit `null`
    // is NOT undefined — it stringifies to "null" and falls through to the
    // invalid-form error path below.
    let form_jsval = crate::value::JSValue::from_bits(form_value.to_bits());

    // Coerce the form BEFORE borrowing the subject's payload. `ToString(form)`
    // is a collection point twice over: an inline short-string form
    // materializes to the heap (so even `s.normalize("NFC")` allocates here),
    // and an object form runs user `toString`, whose loop back-edge polls can
    // run a moving minor. Either can evacuate `s`, and a `&str` taken
    // beforehand is a copy the collector cannot rewrite — rooting rewrites
    // slots, never already-materialized borrows
    // (`docs/src/internals/gc-rooting-invariant.md`). Root the subject across
    // the coercion and borrow only from the address handed back. (#8426)
    let scope = crate::gc::RuntimeHandleScope::new();
    let s_handle = scope.root_string_ptr(s);
    let (form_owned, s) = s_handle.across_const::<StringHeader, _>(|| -> String {
        if form_jsval.is_undefined() {
            "NFC".to_string()
        } else {
            // ToString(form) runs before the form-validity check, so a Symbol
            // form throws a TypeError (§7.1.17) — not the RangeError of an
            // invalid form. The reorder preserves that ordering: coercion
            // still precedes validation.
            crate::builtins::reject_symbol_to_string(form_value);
            let form_ptr = crate::value::js_jsvalue_to_string(form_value);
            if is_valid_string_ptr(form_ptr) {
                string_as_str(form_ptr).to_string()
            } else {
                String::new()
            }
        }
    });
    let str_data = string_as_str(s);

    #[cfg(feature = "string-normalize")]
    let normalized: String = {
        use unicode_normalization::UnicodeNormalization;
        match form_owned.as_str() {
            "NFC" => str_data.nfc().collect(),
            "NFD" => str_data.nfd().collect(),
            "NFKC" => str_data.nfkc().collect(),
            "NFKD" => str_data.nfkd().collect(),
            _ => throw_invalid_normalize_form(),
        }
    };
    // Normalize engine gated off: still validate the form (so a bad form throws
    // the spec RangeError), but pass the string through unchanged for the four
    // valid forms (no Unicode decomposition tables linked).
    #[cfg(not(feature = "string-normalize"))]
    let normalized: String = match form_owned.as_str() {
        "NFC" | "NFD" | "NFKC" | "NFKD" => str_data.to_string(),
        _ => throw_invalid_normalize_form(),
    };
    let bytes = normalized.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

fn throw_invalid_normalize_form() -> ! {
    let message = "The normalization form should be one of NFC, NFD, NFKC, NFKD.";
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_rangeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Apply the canonical-equivalence requirement shared by all locale-aware
/// comparison modes before their approximate collation. NFC is sufficient:
/// canonically equivalent strings have the same NFC representation.
fn locale_compare_canonical(a: &str, b: &str, compare: fn(&str, &str) -> f64) -> f64 {
    if a == b {
        return 0.0;
    }
    // ASCII is already NFC, which keeps the overwhelmingly common path
    // allocation-free.
    if a.is_ascii() && b.is_ascii() {
        return compare(a, b);
    }
    #[cfg(feature = "string-normalize")]
    {
        use unicode_normalization::{is_nfc_quick, IsNormalized, UnicodeNormalization};
        // Non-ASCII text is still usually *already* NFC — precomposed letters,
        // CJK and emoji all are; only combining marks and decomposable
        // singletons are not. The quick check is a table lookup per scalar and
        // allocates nothing, so only text that genuinely needs rewriting pays
        // for the two `String`s. (#10094: this ran on every comparison, and a
        // sort pays it O(n log n) times.)
        if is_nfc_quick(a.chars()) == IsNormalized::Yes
            && is_nfc_quick(b.chars()) == IsNormalized::Yes
        {
            return compare(a, b);
        }
        let a_nfc: String = a.nfc().collect();
        let b_nfc: String = b.nfc().collect();
        compare(&a_nfc, &b_nfc)
    }
    #[cfg(not(feature = "string-normalize"))]
    compare(a, b)
}

/// One step of the streaming lowercase view that the primary collation pass
/// walks instead of materializing `str::to_lowercase`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LowerStep {
    /// The next scalar of the lowercased form.
    Char(char),
    /// Input exhausted.
    End,
    /// U+03A3 GREEK CAPITAL LETTER SIGMA — the one *contextual* (but
    /// language-independent) lowercase mapping in `SpecialCasing.txt`: it
    /// becomes ς at the end of a word and σ everywhere else, which needs the
    /// `Cased` / `Case_Ignorable` properties of the surrounding text. This
    /// walk reports it instead of guessing, and the caller falls back to
    /// `str::to_lowercase`, which implements the rule.
    Contextual,
}

/// `str::to_lowercase` as a borrow-only iterator over scalars.
///
/// `str::to_lowercase` is exactly `chars().flat_map(char::to_lowercase)` apart
/// from the final-sigma rule above, so walking two of these in lockstep
/// answers the primary comparison without materializing either lowercased
/// string — and, because the walk stops at the first difference, usually
/// without case-mapping more than the first scalar or two.
struct LowerChars<'a> {
    rest: std::str::Chars<'a>,
    /// Tail of a one-to-many expansion. U+0130 (`İ` → `i` + U+0307) is the
    /// only unconditional one, but `char::to_lowercase` is allowed up to
    /// three scalars and this holds whatever it yields.
    pending: Option<std::char::ToLowercase>,
}

impl<'a> LowerChars<'a> {
    fn new(s: &'a str) -> Self {
        LowerChars {
            rest: s.chars(),
            pending: None,
        }
    }

    fn next(&mut self) -> LowerStep {
        if let Some(pending) = self.pending.as_mut() {
            if let Some(c) = pending.next() {
                return LowerStep::Char(c);
            }
            self.pending = None;
        }
        let c = match self.rest.next() {
            Some(c) => c,
            None => return LowerStep::End,
        };
        // ASCII is one-to-one and needs no case table.
        if c.is_ascii() {
            return LowerStep::Char(c.to_ascii_lowercase());
        }
        if c == GREEK_CAPITAL_SIGMA {
            return LowerStep::Contextual;
        }
        let mut expansion = c.to_lowercase();
        let first = expansion.next().unwrap_or(c);
        self.pending = Some(expansion);
        LowerStep::Char(first)
    }
}

/// U+03A3, the only scalar whose lowercase mapping depends on its context.
const GREEK_CAPITAL_SIGMA: char = '\u{03A3}';

/// Primary (case-insensitive) collation pass: order the two inputs exactly as
/// `a.to_lowercase().cmp(&b.to_lowercase())` would, without allocating either
/// lowercased form.
///
/// Comparing the lowercased scalar streams by code point is equivalent to
/// `String::cmp`, because UTF-8 (and WTF-8) byte order and code point order
/// agree.
///
/// Returns `None` when a context-dependent mapping is reached before the
/// answer is decided — the caller's signal to fall back to the allocating
/// comparison, which resolves the final-sigma rule properly.
fn locale_primary_cmp(a: &str, b: &str) -> Option<std::cmp::Ordering> {
    // ASCII maps one-to-one under `to_lowercase`, so while both sides are
    // ASCII the lowercased streams stay byte-aligned with the inputs and a
    // plain byte walk decides the comparison without decoding anything.
    let (a_bytes, b_bytes) = (a.as_bytes(), b.as_bytes());
    let common = a_bytes.len().min(b_bytes.len());
    let mut i = 0;
    while i < common && a_bytes[i].is_ascii() && b_bytes[i].is_ascii() {
        let x = a_bytes[i].to_ascii_lowercase();
        let y = b_bytes[i].to_ascii_lowercase();
        if x != y {
            return Some(x.cmp(&y));
        }
        i += 1;
    }
    // Everything before `i` was ASCII on both sides, so `i` is a scalar
    // boundary in both strings and both lowercased streams are `i` scalars in.
    locale_primary_cmp_scalars(&a[i..], &b[i..])
}

fn locale_primary_cmp_scalars(a: &str, b: &str) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    let mut ai = LowerChars::new(a);
    let mut bi = LowerChars::new(b);
    loop {
        match (ai.next(), bi.next()) {
            (LowerStep::Contextual, _) | (_, LowerStep::Contextual) => return None,
            (LowerStep::End, LowerStep::End) => return Some(Ordering::Equal),
            (LowerStep::End, LowerStep::Char(_)) => return Some(Ordering::Less),
            (LowerStep::Char(_), LowerStep::End) => return Some(Ordering::Greater),
            (LowerStep::Char(x), LowerStep::Char(y)) => {
                if x != y {
                    return Some(x.cmp(&y));
                }
            }
        }
    }
}

/// Approximate the Unicode default collation with a two-pass comparison:
/// first case-insensitive (so the character class wins) and then
/// case-sensitive with lowercase < uppercase (matching V8's default ICU
/// behavior where 'a' < 'A').
///
/// Both passes are allocation-free and short-circuit at the first difference;
/// see [`locale_primary_cmp`]. The ordering is deliberately unchanged from the
/// allocating formulation it replaced (#10094).
fn locale_compare_default(a_str: &str, b_str: &str) -> f64 {
    // Case-insensitive primary comparison.
    let primary = match locale_primary_cmp(a_str, b_str) {
        Some(ordering) => ordering,
        None => {
            // Final sigma reached before the answer was decided. Rare enough
            // to be worth two allocations rather than a second, divergent copy
            // of the Final_Sigma rule here.
            let a_lower = a_str.to_lowercase();
            let b_lower = b_str.to_lowercase();
            a_lower.cmp(&b_lower)
        }
    };
    match primary {
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

/// `String.prototype.localeCompare(other)` — returns a negative number, zero,
/// or a positive number.
///
/// **What this guarantees, and what it deliberately does not.** Perry ships no
/// collation table — no DUCET or CLDR root weights — and no locale tailoring:
/// the `locales` argument is accepted and ignored. The ordering is
/// *approximate by design*, not an unimplemented path. See #10094 and the
/// `localeCompare()` row of `docs/typescript-parity-gaps.md`, which record the
/// decision not to link ICU data (~27 MB) for this. What it does guarantee:
///
/// 1. **Canonical equivalence.** Decomposed and precomposed spellings of the
///    same text compare equal — a mandatory part of the `localeCompare`
///    contract (see [`locale_compare_canonical`]).
/// 2. **Primary: case-insensitive *code point* order** — the order of the two
///    `toLowerCase` forms, including the contextual final-sigma rule.
/// 3. **Tertiary: case.** Strings that differ only in case order lowercase
///    first, matching the default Unicode tertiary weight (`'a' < 'A'`).
///
/// Because the primary key is the code point rather than a collation weight,
/// the result differs from Node/ICU wherever root collation reorders the code
/// point space: accented letters sort after the whole unaccented alphabet
/// instead of beside their base letter (`ä` is U+00E4, above `z` at U+007A),
/// and symbols and emoji sort after letters instead of before them. So
/// `"ä".localeCompare("😀")` is negative here and positive in Node. Note that
/// the "correct" answer is locale-dependent even with a table — German sorts
/// `ä` with `a`, Swedish after `z` — which is part of why one untailored table
/// was not judged worth its bytes.
///
/// The relation is nonetheless a strict weak ordering (in fact a total order
/// on distinct canonical forms), so `Array.prototype.sort` results are
/// well-defined; `locale_compare_is_a_strict_weak_ordering` proves it over a
/// mixed-script corpus.
#[no_mangle]
pub extern "C" fn js_string_locale_compare(a: *const StringHeader, b: *const StringHeader) -> f64 {
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
    locale_compare_canonical(string_as_str(a), string_as_str(b), locale_compare_default)
}

/// Natural-order collation for `localeCompare(other, locales, { numeric: true })`:
/// maximal runs of ASCII digits compare by numeric value (leading zeros
/// ignored, then by digit-count and lexicographically), and non-digit runs
/// compare with the same case-insensitive primary / case tertiary rule as
/// `js_string_locale_compare`. So `"10" > "9"` and `"file10" > "file9"`.
fn locale_compare_numeric_raw(a: &str, b: &str) -> f64 {
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return 0.0,
            (None, Some(_)) => return -1.0,
            (Some(_), None) => return 1.0,
            (Some(ca), Some(cb)) if ca.is_ascii_digit() && cb.is_ascii_digit() => {
                let mut da = String::new();
                while let Some(&c) = ai.peek() {
                    if c.is_ascii_digit() {
                        da.push(c);
                        ai.next();
                    } else {
                        break;
                    }
                }
                let mut db = String::new();
                while let Some(&c) = bi.peek() {
                    if c.is_ascii_digit() {
                        db.push(c);
                        bi.next();
                    } else {
                        break;
                    }
                }
                // Compare by numeric value: strip leading zeros, then longer
                // run wins, then lexicographically among equal lengths.
                let na = da.trim_start_matches('0');
                let nb = db.trim_start_matches('0');
                match na.len().cmp(&nb.len()).then_with(|| na.cmp(nb)) {
                    std::cmp::Ordering::Less => return -1.0,
                    std::cmp::Ordering::Greater => return 1.0,
                    std::cmp::Ordering::Equal => {} // equal numeric value — keep going
                }
            }
            (Some(ca), Some(cb)) => {
                ai.next();
                bi.next();
                if ca == cb {
                    continue;
                }
                let la = ca.to_lowercase().next().unwrap_or(ca);
                let lb = cb.to_lowercase().next().unwrap_or(cb);
                if la != lb {
                    return if la < lb { -1.0 } else { 1.0 };
                }
                // Same letter, different case: lowercase sorts before uppercase.
                let a_lower = ca.is_lowercase();
                let b_lower = cb.is_lowercase();
                if a_lower != b_lower {
                    return if a_lower { -1.0 } else { 1.0 };
                }
                return if (ca as u32) < (cb as u32) { -1.0 } else { 1.0 };
            }
        }
    }
}

fn locale_compare_numeric(a: &str, b: &str) -> f64 {
    locale_compare_canonical(a, b, locale_compare_numeric_raw)
}

/// `String.prototype.localeCompare(other, locales, options)` — honors the
/// `{ numeric: true }` collation option (natural sort); `locales` is ignored
/// (no Intl/ICU). `options` arrives as a NaN-boxed JSValue (the options object,
/// or undefined when absent). Reads `options.numeric` (ToBoolean) and routes to
/// `locale_compare_numeric` when set, else to the default `js_string_locale_compare`.
#[no_mangle]
pub extern "C" fn js_string_locale_compare_opts(
    a: *const StringHeader,
    b: *const StringHeader,
    options: f64,
) -> f64 {
    let numeric = {
        let ptr =
            crate::value::js_nanbox_get_pointer(options) as *const crate::object::ObjectHeader;
        if ptr.is_null() || (ptr as usize) < 0x10000 {
            false
        } else {
            let key = crate::string::js_string_from_bytes(b"numeric".as_ptr(), 7);
            let v = crate::object::js_object_get_field_by_name_f64(ptr, key);
            crate::value::js_is_truthy(v) != 0
        }
    };
    if !numeric {
        return js_string_locale_compare(a, b);
    }
    if !is_valid_string_ptr(a) || !is_valid_string_ptr(b) {
        // Match the validity edge-cases of the default path.
        return js_string_locale_compare(a, b);
    }
    locale_compare_numeric(string_as_str(a), string_as_str(b))
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
    if flags & STRING_FLAG_HAS_LONE_SURROGATES == 0 {
        // Well-formed UTF-8: return a copy without scanning. The destination
        // allocation can move `s`, so refresh its payload pointer afterwards.
        let utf16_len = unsafe { (*s).utf16_len };
        return string_copy_range(s, 0, blen as u32, utf16_len, flags);
    }
    let data = string_data(s);
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
            if i + 1 < blen {
                result.push(bytes[i + 1]);
            }
            i += 2;
        } else if b < 0xF0 {
            result.push(b);
            if i + 1 < blen {
                result.push(bytes[i + 1]);
            }
            if i + 2 < blen {
                result.push(bytes[i + 2]);
            }
            i += 3;
        } else {
            result.push(b);
            if i + 1 < blen {
                result.push(bytes[i + 1]);
            }
            if i + 2 < blen {
                result.push(bytes[i + 2]);
            }
            if i + 3 < blen {
                result.push(bytes[i + 3]);
            }
            i += 4;
        }
    }
    js_string_from_bytes(result.as_ptr(), result.len() as u32)
}

#[cfg(test)]
mod utf16_cmp_ascii_fast_path_tests {
    use super::utf16_cmp_bytes;
    use std::cmp::Ordering;

    /// Independent restatement of the pre-fast-path semantics: validate both
    /// sides as UTF-8 and compare the UTF-16 code-unit sequences, falling back
    /// to raw byte order when either side is not valid UTF-8.
    ///
    /// Deliberately *not* the production fallthrough — it is the oracle the
    /// fast path is differentially checked against, so breaking either arm of
    /// `utf16_cmp_bytes` in production code turns these tests red.
    fn reference_cmp(a: &[u8], b: &[u8]) -> Ordering {
        match (std::str::from_utf8(a), std::str::from_utf8(b)) {
            (Ok(a_str), Ok(b_str)) => {
                let au: Vec<u16> = a_str.encode_utf16().collect();
                let bu: Vec<u16> = b_str.encode_utf16().collect();
                au.cmp(&bu)
            }
            _ => a.cmp(b),
        }
    }

    #[test]
    fn short_prefix_word_boundaries_match_full_utf16_order() {
        let tails = corpus();
        for len in [0, 1, 5, 7, 8, 9, 15, 16, 23, 24, 31, 32, 33, 64] {
            for prefix in [b"a".as_slice(), "é".as_bytes(), &[0xff]] {
                let prefix: Vec<u8> = prefix.iter().copied().cycle().take(len).collect();
                for a in &tails {
                    for b in &tails {
                        let a = [prefix.as_slice(), a].concat();
                        let b = [prefix.as_slice(), b].concat();
                        assert_eq!(utf16_cmp_bytes(&a, &b), reference_cmp(&a, &b));
                    }
                }
            }
        }
    }

    /// Every shape the fast path has to get right or fall through on.
    /// WTF-8 lone surrogates are raw byte literals — they are not
    /// representable as Rust `&str`.
    fn corpus() -> Vec<Vec<u8>> {
        let mut v: Vec<Vec<u8>> = Vec::new();
        // Pure ASCII, incl. empty, prefixes, NUL, and the 0x7F boundary.
        for s in [
            "",
            "a",
            "A",
            "ab",
            "abc",
            "abd",
            "abcd",
            "b",
            "z",
            "0",
            "9",
            "~",
            "\u{7f}",
            " ",
            "!",
            "Zebra",
            "apple",
            "Apple",
            "apple pie",
            "record-00001",
            "record-00002",
        ] {
            v.push(s.as_bytes().to_vec());
        }
        // Embedded NUL — must not terminate the comparison early.
        v.push(b"ab\0cd".to_vec());
        v.push(b"ab\0ce".to_vec());
        v.push(b"ab\0".to_vec());
        v.push(vec![0u8]);
        // Non-ASCII BMP, incl. the ASCII/non-ASCII 0x80 boundary and the
        // 0xE000..0xFFFF band that is where UTF-16 order diverges from
        // code-point order.
        for s in [
            "\u{80}",
            "\u{7ff}",
            "\u{800}",
            "café",
            "cafè",
            "caf\u{e9}x",
            "日本",
            "日本語",
            "\u{e000}",
            "\u{fffd}",
            "\u{ffff}",
        ] {
            v.push(s.as_bytes().to_vec());
        }
        // Astral (4-byte UTF-8 / surrogate pair in UTF-16).
        for s in ["\u{10000}", "\u{1f600}", "a\u{1f600}", "\u{10ffff}"] {
            v.push(s.as_bytes().to_vec());
        }
        // WTF-8 lone surrogates (invalid UTF-8) — the fallthrough's byte-order arm.
        v.push(vec![0xED, 0xA0, 0x80]); // lone high surrogate U+D800
        v.push(vec![0xED, 0xB0, 0x80]); // lone low surrogate U+DC00
        v.push(vec![b'a', 0xED, 0xA0, 0x80]);
        // Truncated / malformed sequences — exactly the payloads whose cached
        // `utf16_len == byte_len` would lie about being ASCII.
        v.push(vec![0xC3]);
        v.push(vec![0xF0, 0x41]);
        v.push(vec![0x80]);
        v
    }

    /// The fast path must agree with the reference on every ordered pair, and
    /// the corpus must actually exercise *both* arms (a green run that never
    /// entered the fast path would prove nothing).
    #[test]
    fn fast_path_agrees_with_reference_on_every_pair() {
        let corpus = corpus();
        let (mut fast_arm, mut slow_arm) = (0usize, 0usize);
        for a in &corpus {
            for b in &corpus {
                let got = utf16_cmp_bytes(a, b);
                let want = reference_cmp(a, b);
                assert_eq!(got, want, "utf16_cmp_bytes({a:?}, {b:?})");
                if a.is_ascii() && b.is_ascii() {
                    fast_arm += 1;
                } else {
                    slow_arm += 1;
                }
            }
        }
        assert!(fast_arm > 0, "corpus never entered the ASCII fast path");
        assert!(slow_arm > 0, "corpus never entered the fallthrough");
    }

    /// Sort-safety: the relation must stay a total order (antisymmetric,
    /// reflexive-equal) with the fast path spliced in — a comparator that
    /// disagrees with itself corrupts `Array.prototype.sort`.
    #[test]
    fn fast_path_keeps_a_total_order() {
        let corpus = corpus();
        for a in &corpus {
            assert_eq!(utf16_cmp_bytes(a, a), Ordering::Equal, "{a:?} != itself");
            for b in &corpus {
                assert_eq!(
                    utf16_cmp_bytes(a, b),
                    utf16_cmp_bytes(b, a).reverse(),
                    "antisymmetry broken for ({a:?}, {b:?})"
                );
            }
        }
    }

    /// Pure-ASCII orderings, spelled out (these are the pairs the fast path
    /// answers on its own).
    #[test]
    fn pure_ascii_orderings() {
        let c = |a: &str, b: &str| utf16_cmp_bytes(a.as_bytes(), b.as_bytes());
        assert_eq!(c("", ""), Ordering::Equal);
        assert_eq!(c("", "a"), Ordering::Less);
        assert_eq!(c("a", ""), Ordering::Greater);
        assert_eq!(c("abc", "abc"), Ordering::Equal);
        assert_eq!(c("abc", "abd"), Ordering::Less);
        assert_eq!(c("abd", "abc"), Ordering::Greater);
        assert_eq!(c("abc", "abcd"), Ordering::Less); // proper prefix sorts first
        assert_eq!(c("abcd", "abc"), Ordering::Greater);
        // Uppercase < lowercase in code-unit order (JS `<`, not localeCompare).
        assert_eq!(c("Zebra", "apple"), Ordering::Less);
        assert_eq!(c("apple", "Apple"), Ordering::Greater);
        // Embedded NUL is an ordinary code unit.
        assert_eq!(
            utf16_cmp_bytes(b"ab\0cd", b"ab\0ce"),
            Ordering::Less,
            "embedded NUL must not truncate the comparison"
        );
        assert_eq!(utf16_cmp_bytes(b"ab\0", b"ab"), Ordering::Greater);
    }

    /// Non-ASCII pairs must keep going through the general path — including
    /// the astral-vs-BMP case where UTF-16 order and byte/code-point order
    /// disagree, which is the whole reason this helper is not `a.cmp(b)`.
    #[test]
    fn non_ascii_keeps_utf16_code_unit_order() {
        let c = |a: &str, b: &str| utf16_cmp_bytes(a.as_bytes(), b.as_bytes());
        // U+FFFD (BMP) vs U+10000 (astral): code-point/byte order says Less,
        // UTF-16 order says Greater because the surrogate lead is 0xD800.
        assert_eq!(c("\u{fffd}", "\u{10000}"), Ordering::Greater);
        assert_eq!(c("\u{10000}", "\u{fffd}"), Ordering::Less);
        assert_eq!(c("\u{e000}", "\u{1f600}"), Ordering::Greater);
        // Same, but only reachable past a shared ASCII prefix.
        assert_eq!(c("a\u{ffff}", "a\u{10000}"), Ordering::Greater);
        // Strings differing only past the ASCII range.
        assert_eq!(c("café", "cafè"), Ordering::Greater); // U+00E9 > U+00E8
        assert_eq!(c("cafe", "café"), Ordering::Less);
        assert_eq!(c("日本", "日本語"), Ordering::Less);
        assert_eq!(c("\u{7f}", "\u{80}"), Ordering::Less);
    }

    /// Mixed ASCII / non-ASCII operands take the fallthrough (`b.is_ascii()`
    /// is false) and must still be ordered by UTF-16 code unit.
    #[test]
    fn mixed_ascii_and_non_ascii_operands() {
        let c = |a: &str, b: &str| utf16_cmp_bytes(a.as_bytes(), b.as_bytes());
        assert_eq!(c("z", "\u{80}"), Ordering::Less);
        assert_eq!(c("\u{80}", "z"), Ordering::Greater);
        assert_eq!(c("abc", "abc\u{e9}"), Ordering::Less);
        assert_eq!(c("abc\u{e9}", "abc"), Ordering::Greater);
        assert_eq!(c("", "\u{1f600}"), Ordering::Less);
        assert_eq!(c("\u{1f600}", ""), Ordering::Greater);
    }

    /// WTF-8 lone surrogates are not valid UTF-8, so the general path falls
    /// back to raw byte order. The fast path must not claim them (0xED > 0x7F)
    /// and must not change the answer.
    #[test]
    fn lone_surrogates_fall_back_to_byte_order() {
        let high: &[u8] = &[0xED, 0xA0, 0x80]; // U+D800
        let low: &[u8] = &[0xED, 0xB0, 0x80]; // U+DC00
        assert!(!high.is_ascii() && !low.is_ascii());
        assert_eq!(utf16_cmp_bytes(high, low), Ordering::Less);
        assert_eq!(utf16_cmp_bytes(low, high), Ordering::Greater);
        assert_eq!(utf16_cmp_bytes(high, high), Ordering::Equal);
        assert_eq!(utf16_cmp_bytes(high, b"a"), Ordering::Greater);
        assert_eq!(utf16_cmp_bytes(b"a", high), Ordering::Less);
        assert_eq!(utf16_cmp_bytes(high, b""), Ordering::Greater);
    }

    /// The header-cached `utf16_len == byte_len` predicate is unsound as an
    /// ASCII test for these payloads; the byte scan the fast path uses is not.
    /// This pins the reason the cheaper flag was rejected.
    #[test]
    fn cached_utf16_len_predicate_would_misclassify_these() {
        for payload in [vec![0xC3u8], vec![0xF0u8, 0x41]] {
            let cached_says_ascii =
                crate::string::compute_utf16_len(payload.as_ptr(), payload.len() as u32) as usize
                    == payload.len();
            assert!(
                cached_says_ascii,
                "expected the cached predicate to (wrongly) call {payload:?} ASCII"
            );
            assert!(
                !payload.is_ascii(),
                "{payload:?} is not ASCII — the byte scan must reject it"
            );
            // And the answer is unchanged either way for these.
            assert_eq!(
                utf16_cmp_bytes(&payload, b"a"),
                reference_cmp(&payload, b"a")
            );
        }
    }
}

#[cfg(test)]
mod numeric_collation_tests {
    use super::{locale_compare_default, locale_compare_numeric};

    #[test]
    fn natural_order_compares_digit_runs_numerically() {
        // Numeric runs compare by value, not lexicographically.
        assert_eq!(locale_compare_numeric("10", "9"), 1.0);
        assert_eq!(locale_compare_numeric("9", "10"), -1.0);
        assert_eq!(locale_compare_numeric("file10", "file9"), 1.0);
        assert_eq!(locale_compare_numeric("file2", "file10"), -1.0);
        // Leading zeros: equal numeric value → equal.
        assert_eq!(locale_compare_numeric("08", "8"), 0.0);
        assert_eq!(locale_compare_numeric("100", "99"), 1.0);
        // Mixed runs and pure alpha.
        assert_eq!(locale_compare_numeric("a10b", "a9b"), 1.0);
        assert_eq!(locale_compare_numeric("a", "b"), -1.0);
        assert_eq!(locale_compare_numeric("abc", "abc"), 0.0);
        // A digit run vs the end of the shorter string.
        assert_eq!(locale_compare_numeric("x", "x10"), -1.0);
        assert_eq!(locale_compare_numeric("2foo", "10foo"), -1.0);
    }

    #[cfg(feature = "string-normalize")]
    #[test]
    fn locale_compare_treats_canonical_equivalents_as_equal() {
        for (a, b) in [
            ("o\u{0308}", "ö"),
            ("a\u{0308}\u{0323}", "a\u{0323}\u{0308}"),
            ("\u{1111}\u{1171}\u{11b6}", "퓛"),
            ("Å", "A\u{030a}"),
        ] {
            assert_eq!(
                super::locale_compare_canonical(a, b, locale_compare_default),
                0.0
            );
            assert_eq!(locale_compare_numeric(a, b), 0.0);
        }
    }
}

/// #10094: the primary collation pass stopped materializing two lowercased
/// `String`s per comparison. These tests pin the two properties that makes
/// safe — the ordering is byte-for-byte what the allocating formulation
/// produced, and the relation is a strict weak ordering so
/// `Array.prototype.sort` stays well-defined.
#[cfg(test)]
mod locale_collation_tests {
    use super::{locale_compare_canonical, locale_compare_default, locale_primary_cmp};
    use std::cmp::Ordering;

    /// The formulation this replaced, kept verbatim as the oracle: lowercase
    /// both sides with `str::to_lowercase` and compare the results. If the
    /// streaming walk ever disagrees with this, the ordering has moved.
    fn reference_compare(a_str: &str, b_str: &str) -> f64 {
        let a_lower = a_str.to_lowercase();
        let b_lower = b_str.to_lowercase();
        match a_lower.cmp(&b_lower) {
            Ordering::Less => return -1.0,
            Ordering::Greater => return 1.0,
            Ordering::Equal => {}
        }
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

    /// Spans every class the issue names *except* WTF-8 lone surrogates:
    /// ASCII (incl. case-only and long-common-prefix pairs), Latin-1 accented
    /// letters in both precomposed and decomposed spellings, CJK, emoji, bare
    /// combining marks, and the two special case mappings (U+0130, U+03A3).
    ///
    /// Lone surrogates are deliberately absent. This comparator takes `&str`,
    /// and a lone surrogate is not representable as one: forging it with
    /// `from_utf8_unchecked` makes `chars()` yield a value that is not a valid
    /// `char`, which std's UB precondition check catches in a debug build —
    /// the test aborts with SIGABRT rather than reporting an ordering. That is
    /// a property of the runtime's WTF-8-as-`&str` view (`string_as_str`),
    /// unchanged by this rewrite and identical on both sides of the
    /// differential, so a corpus entry could only prove the checker works. The
    /// byte-level lone-surrogate coverage that *is* sound lives in
    /// `utf16_cmp_ascii_fast_path_tests::lone_surrogates_fall_back_to_byte_order`,
    /// where the helper takes `&[u8]`.
    fn corpus() -> Vec<&'static str> {
        vec![
            "",
            "a",
            "A",
            "b",
            "B",
            "z",
            "Z",
            "ab",
            "aB",
            "Ab",
            "AB",
            "abc",
            "abd",
            "abcd",
            "aBcDa",
            "aBcDz",
            "aBcDB",
            "aBcD7",
            "record-000000000001",
            "record-000000000002",
            "Record-000000000001",
            "0",
            "9",
            " ",
            "~",
            "\u{7f}",
            "ä",
            "Ä",
            "a\u{308}",
            "A\u{308}",
            "ö",
            "Ö",
            "o\u{308}",
            "é",
            "è",
            "ß",
            "\u{1e9e}",
            "\u{308}",
            "\u{323}",
            "a\u{308}\u{323}",
            "a\u{323}\u{308}",
            "İ",
            "i\u{307}",
            "ı",
            "I",
            "i",
            "Σ",
            "σ",
            "ς",
            "ΣΑ",
            "ΑΣ",
            "ΟΔΟΣ",
            "Οδος",
            "οδος",
            "漢",
            "字",
            "漢字",
            "日本語",
            "ä中😀Öa",
            "ä中😀Öz",
            "ä中😀ÖB",
            "ä中😀Ö7",
            "😀",
            "🚀",
            "\u{10ffff}",
            "ä:123",
            "Ö:123",
            "😀:9",
        ]
    }

    /// Behaviour preservation, the issue's first acceptance criterion: the
    /// allocation-free walk must return exactly what the two-`to_lowercase`
    /// formulation returned, on every ordered pair. The corpus must also reach
    /// all three arms — the ASCII byte loop, the scalar walk, and the
    /// contextual fallback — or a green run would prove nothing.
    #[test]
    fn matches_the_allocating_reference_on_every_pair() {
        let corpus = corpus();
        let (mut ascii_arm, mut scalar_arm, mut contextual_arm) = (0usize, 0usize, 0usize);
        for a in &corpus {
            for b in &corpus {
                assert_eq!(
                    locale_compare_default(a, b),
                    reference_compare(a, b),
                    "locale_compare_default({a:?}, {b:?})"
                );
                if locale_primary_cmp(a, b).is_none() {
                    contextual_arm += 1;
                } else if a.is_ascii() && b.is_ascii() {
                    ascii_arm += 1;
                } else {
                    scalar_arm += 1;
                }
            }
        }
        assert!(ascii_arm > 0, "corpus never took the ASCII byte loop");
        assert!(scalar_arm > 0, "corpus never took the scalar walk");
        assert!(
            contextual_arm > 0,
            "corpus never reached the final-sigma fallback"
        );
    }

    /// xorshift32, so a failure is reproducible from the seed alone.
    fn xorshift(state: &mut u32) -> u32 {
        *state ^= *state << 13;
        *state ^= *state >> 17;
        *state ^= *state << 5;
        *state
    }

    /// Randomized differential coverage of shapes the hand-written corpus does
    /// not enumerate: mixed-script strings, shared prefixes of every length,
    /// and case expansions landing at arbitrary offsets.
    #[test]
    fn matches_the_allocating_reference_on_random_strings() {
        const ALPHABET: [&str; 16] = [
            "a", "B", "z", "7", "-", "ä", "Ö", "ß", "İ", "Σ", "ς", "\u{308}", "漢", "😀", "\u{7f}",
            "i\u{307}",
        ];
        let mut state: u32 = 0x1234_5678;
        for _ in 0..20_000 {
            let shared = xorshift(&mut state) % 6;
            let mut prefix = String::new();
            for _ in 0..shared {
                prefix.push_str(ALPHABET[(xorshift(&mut state) % 16) as usize]);
            }
            let mut pair = [prefix.clone(), prefix];
            for s in pair.iter_mut() {
                let tail = xorshift(&mut state) % 5;
                for _ in 0..tail {
                    s.push_str(ALPHABET[(xorshift(&mut state) % 16) as usize]);
                }
            }
            let (a, b) = (&pair[0], &pair[1]);
            assert_eq!(
                locale_compare_default(a, b),
                reference_compare(a, b),
                "locale_compare_default({a:?}, {b:?})"
            );
        }
    }

    /// Sort-safety. `Array.prototype.sort` is only well-defined for a
    /// consistent comparator, so an approximate ordering still has to be a
    /// strict weak ordering: irreflexive-equal, antisymmetric, and transitive.
    /// Checked through `locale_compare_canonical`, which is what
    /// `js_string_locale_compare` actually calls.
    #[test]
    fn locale_compare_is_a_strict_weak_ordering() {
        let corpus = corpus();
        let cmp = |a: &str, b: &str| {
            let v = locale_compare_canonical(a, b, locale_compare_default);
            if v < 0.0 {
                Ordering::Less
            } else if v > 0.0 {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        };
        for a in &corpus {
            assert_eq!(cmp(a, a), Ordering::Equal, "cmp({a:?}, itself)");
            for b in &corpus {
                assert_eq!(
                    cmp(a, b),
                    cmp(b, a).reverse(),
                    "antisymmetry broken for ({a:?}, {b:?})"
                );
            }
        }
        // Transitivity of both `<` and the equivalence it induces, over every
        // triple.
        for a in &corpus {
            for b in &corpus {
                let ab = cmp(a, b);
                for c in &corpus {
                    let bc = cmp(b, c);
                    let ac = cmp(a, c);
                    if ab == Ordering::Equal && bc == Ordering::Equal {
                        assert_eq!(
                            ac,
                            Ordering::Equal,
                            "equivalence not transitive: {a:?} ~ {b:?} ~ {c:?}"
                        );
                    }
                    if ab != Ordering::Greater && bc != Ordering::Greater {
                        assert_ne!(
                            ac,
                            Ordering::Greater,
                            "order not transitive: {a:?} <= {b:?} <= {c:?}"
                        );
                    }
                }
            }
        }
    }

    /// The contract the doc comment now states out loud, spelled out as
    /// assertions so a future rewrite has to face them.
    #[test]
    fn documented_guarantees_hold() {
        let cmp = |a: &str, b: &str| locale_compare_canonical(a, b, locale_compare_default);
        // Identical, empty, and prefix pairs.
        assert_eq!(cmp("", ""), 0.0);
        assert_eq!(cmp("abc", "abc"), 0.0);
        assert_eq!(cmp("", "a"), -1.0);
        assert_eq!(cmp("a", ""), 1.0);
        assert_eq!(cmp("abc", "abcd"), -1.0);
        assert_eq!(cmp("abcd", "abc"), 1.0);
        // Differing only after a long common prefix.
        let prefix = "x".repeat(512);
        assert_eq!(cmp(&format!("{prefix}a"), &format!("{prefix}b")), -1.0);
        assert_eq!(cmp(&format!("{prefix}b"), &format!("{prefix}a")), 1.0);
        // Case-only differences: lowercase first (tertiary weight).
        assert_eq!(cmp("a", "A"), -1.0);
        assert_eq!(cmp("A", "a"), 1.0);
        assert_eq!(cmp("aBc", "AbC"), -1.0);
        assert_eq!(cmp("ä", "Ä"), -1.0);
        // Primary beats tertiary: the letter class wins over case.
        assert_eq!(cmp("B", "a"), 1.0);
        assert_eq!(cmp("a", "B"), -1.0);
        // The documented divergence from ICU, asserted rather than implied:
        // code point order puts accented letters after `z` and emoji last.
        assert_eq!(cmp("ä", "z"), 1.0);
        assert_eq!(cmp("ä", "😀"), -1.0);
        assert_eq!(cmp("Ö", "字"), -1.0);
    }

    /// Canonical equivalence is a mandatory part of the contract, so it has to
    /// survive the rewrite of the pass that runs after it.
    #[cfg(feature = "string-normalize")]
    #[test]
    fn canonical_equivalents_stay_equal() {
        for (a, b) in [
            ("o\u{308}", "ö"),
            ("O\u{308}", "Ö"),
            ("a\u{308}\u{323}", "a\u{323}\u{308}"),
            ("\u{1111}\u{1171}\u{11b6}", "퓛"),
            ("Å", "A\u{30a}"),
            ("ä中😀Öa", "a\u{308}中😀O\u{308}a"),
        ] {
            assert_eq!(
                locale_compare_canonical(a, b, locale_compare_default),
                0.0,
                "{a:?} vs {b:?}"
            );
            // …and the case tiebreak still applies across spellings.
            let upper_a = a.to_uppercase();
            assert!(
                locale_compare_canonical(a, &upper_a, locale_compare_default) <= 0.0,
                "{a:?} vs {upper_a:?}"
            );
        }
    }

    /// Final sigma is the one mapping the streaming walk refuses to guess.
    /// It must both report the fallback and get the answer right.
    #[test]
    fn final_sigma_falls_back_and_stays_correct() {
        // "ΟΔΟΣ".to_lowercase() is "οδος" — word-final Σ becomes ς.
        assert_eq!("ΟΔΟΣ".to_lowercase(), "οδος");
        assert!(locale_primary_cmp("ΟΔΟΣ", "οδος").is_none());
        assert_eq!(locale_compare_default("ΟΔΟΣ", "οδος"), 1.0); // case tiebreak
        for (a, b) in [
            ("ΟΔΟΣ", "οδος"),
            ("ΟΔΟΣ", "οδοσ"),
            ("Σ", "σ"),
            ("Σ", "ς"),
            ("ΣΑ", "σα"),
            ("aΣ", "aς"),
        ] {
            assert_eq!(locale_compare_default(a, b), reference_compare(a, b));
            assert_eq!(locale_compare_default(b, a), reference_compare(b, a));
        }
        // A Σ *after* the deciding position must not force the fallback.
        assert!(locale_primary_cmp("aΣ", "bΣ").is_some());
    }
}

#[cfg(test)]
mod tests_sso_helpers {
    use super::*;
    use crate::value::SHORT_STRING_MAX_LEN;
    use crate::{js_string_from_bytes, JSValue};

    /// #1781: a STRING_TAG heap key and a SHORT_STRING_TAG inline key
    /// with the same bytes must both match an incoming heap key.
    #[test]
    fn key_matches_heap_and_sso_for_same_bytes() {
        for name in ["a", "id", "tag", "name", "mango"] {
            let bytes = name.as_bytes();
            assert!(bytes.len() <= SHORT_STRING_MAX_LEN);

            let incoming = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            let heap_stored = JSValue::string_ptr(incoming);
            let sso_stored = JSValue::try_short_string(bytes).expect("len<=5 encodes as SSO");
            assert!(sso_stored.is_short_string(), "{name:?} should be SSO");

            unsafe {
                assert!(
                    js_string_key_matches(heap_stored, incoming),
                    "heap match failed for {name:?}"
                );
                assert!(
                    js_string_key_matches(sso_stored, incoming),
                    "SSO match failed for {name:?}"
                );
                assert!(
                    js_string_key_matches_bytes(heap_stored, bytes),
                    "heap bytes-match failed for {name:?}"
                );
                assert!(
                    js_string_key_matches_bytes(sso_stored, bytes),
                    "SSO bytes-match failed for {name:?}"
                );
            }
        }
    }

    /// Different-length stored vs incoming must return false even when one
    /// is SSO and the other is heap.
    #[test]
    fn key_matches_rejects_different_bytes_across_reps() {
        let incoming = js_string_from_bytes(b"id".as_ptr(), 2);
        let sso_other = JSValue::try_short_string(b"tag").expect("SSO");
        let heap_other_ptr = js_string_from_bytes(b"other".as_ptr(), 5);
        let heap_other = JSValue::string_ptr(heap_other_ptr);

        unsafe {
            assert!(!js_string_key_matches(sso_other, incoming));
            assert!(!js_string_key_matches(heap_other, incoming));
        }
    }

    /// Non-string stored values (undefined / number / pointer) must return false
    /// without dereferencing the payload.
    #[test]
    fn key_matches_rejects_non_string_stored() {
        let incoming = js_string_from_bytes(b"id".as_ptr(), 2);
        for stored in [
            JSValue::undefined(),
            JSValue::null(),
            JSValue::int32(42),
            JSValue::bool(true),
        ] {
            unsafe {
                assert!(!js_string_key_matches(stored, incoming));
                assert!(!js_string_key_matches_bytes(stored, b"id"));
            }
        }
    }

    /// SSO key_bytes() round-trip: returns the inline bytes for SSO,
    /// the heap bytes for STRING_TAG, None for everything else.
    #[test]
    fn key_bytes_round_trips_sso_and_heap() {
        let sso = JSValue::try_short_string(b"path").expect("SSO");
        let heap = JSValue::string_ptr(js_string_from_bytes(b"longish".as_ptr(), 7));
        let mut buf = [0u8; SHORT_STRING_MAX_LEN];
        unsafe {
            assert_eq!(js_string_key_bytes(sso, &mut buf), Some(b"path".as_ref()));
            assert_eq!(
                js_string_key_bytes(heap, &mut buf),
                Some(b"longish".as_ref())
            );
            assert_eq!(js_string_key_bytes(JSValue::int32(7), &mut buf), None);
        }
    }
}
