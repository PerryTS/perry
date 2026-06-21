//! Slicing, substring, trimming, case conversion, and index-of operations.

use super::*;

/// Get a slice of a string (byte-based for now)
/// Returns a new string from start to end (exclusive).
/// start/end are in UTF-16 code unit indices (JS semantics).
#[no_mangle]
pub extern "C" fn js_string_slice(
    s: *const StringHeader,
    start: i32,
    end: i32,
) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }

    let len = unsafe { (*s).utf16_len } as i32;

    // Handle negative indices (from end)
    let start = if start < 0 {
        (len + start).max(0)
    } else {
        start.min(len)
    };
    let end = if end < 0 {
        (len + end).max(0)
    } else {
        end.min(len)
    };

    if start >= end {
        return js_string_from_bytes(ptr::null(), 0);
    }

    // ASCII fast path: byte offsets == UTF-16 offsets, skip utf16_len scan.
    // Copy GC-safely: the destination allocation can move/sweep `s` (#5062).
    if is_ascii_string(s) {
        let slice_len = (end - start) as u32;
        return string_copy_range(s, start as usize, slice_len, slice_len, 0);
    }

    // Convert UTF-16 offsets to byte offsets
    let str_data = string_as_str(s);
    let byte_start = utf16_offset_to_byte_offset(str_data, start as usize);
    let byte_end = utf16_offset_to_byte_offset(str_data, end as usize);
    string_copy_range(
        s,
        byte_start,
        (byte_end - byte_start) as u32,
        (end - start) as u32,
        0,
    )
}

/// Get a substring (similar to slice but different behavior)
/// - Negative indices are treated as 0
/// - If start > end, arguments are swapped
/// start/end are in UTF-16 code unit indices (JS semantics).
#[no_mangle]
pub extern "C" fn js_string_substring(
    s: *const StringHeader,
    start: i32,
    end: i32,
) -> *mut StringHeader {
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

    // ASCII fast path: skip utf16_len scan in allocator.
    // Copy GC-safely: the destination allocation can move/sweep `s` (#5062).
    if is_ascii_string(s) {
        let slice_len = (end - start) as u32;
        return string_copy_range(s, start as usize, slice_len, slice_len, 0);
    }

    let str_data = string_as_str(s);
    let byte_start = utf16_offset_to_byte_offset(str_data, start as usize);
    let byte_end = utf16_offset_to_byte_offset(str_data, end as usize);
    string_copy_range(
        s,
        byte_start,
        (byte_end - byte_start) as u32,
        (end - start) as u32,
        0,
    )
}

/// Legacy `String.prototype.substr(start, length)` (ECMA-262 Annex B.2.3.1).
///
/// Differs from `substring`/`slice`:
///   * a negative `start` counts from the END of the string
///     (`max(size + start, 0)`),
///   * the second argument is a LENGTH, not an end index, and an `undefined`
///     length (omitted OR explicitly `undefined`) means "to the end of the
///     string" — distinct from a `0`/non-positive length, which yields `""`.
///
/// `start` and `length` arrive as raw NaN-boxed JS values (`f64`). Both are run
/// through `ToIntegerOrInfinity` *here*, in spec order (start first, then
/// length), so a boolean / numeric string / `{ valueOf }` object coerces
/// correctly and a throwing `valueOf` or a `Symbol` propagates its exception —
/// matching `substring`/`slice`. Doing the coercion in the runtime (rather than
/// a codegen `fptosi`, which is UB on a NaN) also avoids a sentinel collision: a
/// `-Infinity` length must clamp to `0` (→ `""`), not be mistaken for "omitted".
/// `s` is a live argument, so it stays pinned by the conservative stack scan
/// across the inner `valueOf`. Closes #2897; fixes the substr tail of #5347.
#[no_mangle]
pub extern "C" fn js_string_substr(
    s: *const StringHeader,
    start_val: f64,
    length_val: f64,
) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }

    let size = unsafe { (*s).utf16_len } as i64;

    // Step 4: ToIntegerOrInfinity(start), observed FIRST. ±Infinity clamps to
    // the i32 bounds; widen to i64 so `size + i32::MIN` can't overflow below.
    let int_start = crate::string::js_string_index_to_i32(start_val) as i64;
    // Steps 5-7: -Infinity / large-negative → max(size + start, 0); else clamp
    // into [0, size]. (`size + i32::MIN` for -Infinity yields ≤ 0 → 0.)
    let start = if int_start < 0 {
        (size + int_start).max(0)
    } else {
        int_start.min(size)
    };

    // Step 8: an `undefined` length means the rest of the string (`size`);
    // otherwise ToIntegerOrInfinity(length), observed AFTER start.
    let int_length = if crate::value::JSValue::from_bits(length_val.to_bits()).is_undefined() {
        size
    } else {
        crate::string::js_string_index_to_i32(length_val) as i64
    };
    // Step 9: clamp the length into [0, size]. Step 10: end = min(start+len, size).
    let length = int_length.clamp(0, size);
    let end = (start + length).min(size);

    if start >= end {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let start = start as i32;
    let end = end as i32;

    // ASCII fast path: byte offsets == UTF-16 offsets.
    // Copy GC-safely: the destination allocation can move/sweep `s` (#5062).
    if is_ascii_string(s) {
        let slice_len = (end - start) as u32;
        return string_copy_range(s, start as usize, slice_len, slice_len, 0);
    }

    let str_data = string_as_str(s);
    let byte_start = utf16_offset_to_byte_offset(str_data, start as usize);
    let byte_end = utf16_offset_to_byte_offset(str_data, end as usize);
    string_copy_range(
        s,
        byte_start,
        (byte_end - byte_start) as u32,
        (end - start) as u32,
        0,
    )
}

// `#[used]` keepalive: `js_string_substr` is reached only from generated `.o`,
// so the whole-program auto-optimize bitcode rebuild would dead-strip it
// without an anchor (see project_auto_optimize_keepalive_3320).
#[used]
static KEEP_SUBSTR: extern "C" fn(*const StringHeader, f64, f64) -> *mut StringHeader =
    js_string_substr;

/// JS `TrimString` whitespace set (ECMA-262 §22.1.3.32, `WhiteSpace` +
/// `LineTerminator`). Differs from Rust's `char::is_whitespace` (Unicode
/// `White_Space`): JS *includes* U+FEFF (`<ZWNBSP>` / BOM) and *excludes*
/// U+0085 (NEL), so `str::trim()` both under- and over-trims for JS.
#[inline]
pub(crate) fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{0009}'        // TAB
        | '\u{000A}'      // LF  <LineTerminator>
        | '\u{000B}'      // VT
        | '\u{000C}'      // FF
        | '\u{000D}'      // CR  <LineTerminator>
        | '\u{0020}'      // SPACE
        | '\u{00A0}'      // NBSP
        | '\u{1680}'      // OGHAM SPACE MARK
        | '\u{2000}'
            ..='\u{200A}' // EN QUAD .. HAIR SPACE
        | '\u{2028}'      // LINE SEPARATOR      <LineTerminator>
        | '\u{2029}'      // PARAGRAPH SEPARATOR <LineTerminator>
        | '\u{202F}'      // NARROW NO-BREAK SPACE
        | '\u{205F}'      // MEDIUM MATHEMATICAL SPACE
        | '\u{3000}'      // IDEOGRAPHIC SPACE
        | '\u{FEFF}' // ZERO WIDTH NO-BREAK SPACE / BOM
    )
}

/// Trim whitespace from both ends of a string
#[no_mangle]
pub extern "C" fn js_string_trim(s: *const StringHeader) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }

    let str_data = string_as_str(s);
    let trimmed = str_data.trim_matches(is_js_whitespace);
    js_string_from_str(trimmed)
}

/// Trim whitespace from start of a string (trimStart/trimLeft)
#[no_mangle]
pub extern "C" fn js_string_trim_start(s: *const StringHeader) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let str_data = string_as_str(s);
    js_string_from_str(str_data.trim_start_matches(is_js_whitespace))
}

/// Trim whitespace from end of a string (trimEnd/trimRight)
#[no_mangle]
pub extern "C" fn js_string_trim_end(s: *const StringHeader) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let str_data = string_as_str(s);
    js_string_from_str(str_data.trim_end_matches(is_js_whitespace))
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
pub extern "C" fn js_string_index_of(
    haystack: *const StringHeader,
    needle: *const StringHeader,
) -> i32 {
    js_string_index_of_from(haystack, needle, 0)
}

/// Find index of substring starting from a given position (-1 if not found).
/// from_index and return value are in UTF-16 code unit indices (JS semantics).
#[no_mangle]
pub extern "C" fn js_string_index_of_from(
    haystack: *const StringHeader,
    needle: *const StringHeader,
    from_index: i32,
) -> i32 {
    if !is_valid_string_ptr(haystack) || !is_valid_string_ptr(needle) {
        return -1;
    }

    unsafe {
        let h_blen = (*haystack).byte_len as usize;
        let n_blen = (*needle).byte_len as usize;

        // ASCII fast path: byte offset == UTF-16 offset, use Rust's
        // optimized Two-Way str::find (avoids O(n*m) naive scan).
        if is_ascii_string(haystack) {
            let start = if from_index < 0 {
                0usize
            } else {
                from_index as usize
            };
            if n_blen == 0 {
                return start.min(h_blen) as i32;
            }
            if start + n_blen > h_blen {
                return -1;
            }
            let h =
                std::str::from_utf8_unchecked(slice::from_raw_parts(string_data(haystack), h_blen));
            let n =
                std::str::from_utf8_unchecked(slice::from_raw_parts(string_data(needle), n_blen));
            return match h[start..].find(n) {
                Some(pos) => (start + pos) as i32,
                None => -1,
            };
        }

        // Non-ASCII: construct &str, convert UTF-16 from_index to byte offset
        let h = string_as_str(haystack);
        let n = string_as_str(needle);
        let u16_start = if from_index < 0 {
            0usize
        } else {
            from_index as usize
        };
        let byte_start = utf16_offset_to_byte_offset(h, u16_start);
        if byte_start > h.len() {
            if n.is_empty() {
                return (*haystack).utf16_len as i32;
            }
            return -1;
        }
        match h[byte_start..].find(n) {
            Some(byte_pos) => byte_offset_to_utf16_index(h, byte_start + byte_pos) as i32,
            None => -1,
        }
    }
}

/// Convert a `position` argument (a NaN-boxed double) into an `i32` start
/// index using JS `ToIntegerOrInfinity` + clamp semantics, as used by
/// `String.prototype.includes(search, position)`:
/// `NaN`/`-Infinity` → 0, `+Infinity` → `i32::MAX` (past the end → no match),
/// otherwise truncate toward zero and saturate into `i32` range. This avoids
/// LLVM `fptosi`'s undefined result on non-finite inputs and matches Node's
/// behavior (`"ababa".includes("a", Infinity) === false`).
#[no_mangle]
pub extern "C" fn js_string_position_to_index(pos_f64: f64) -> i32 {
    // The typed `includes` lowering passes a raw numeric double here.
    let n = pos_f64;
    if n.is_nan() {
        return 0;
    }
    if n == f64::INFINITY {
        return i32::MAX;
    }
    if n == f64::NEG_INFINITY {
        return 0;
    }
    let truncated = n.trunc();
    if truncated >= i32::MAX as f64 {
        i32::MAX
    } else if truncated <= i32::MIN as f64 {
        i32::MIN
    } else {
        truncated as i32
    }
}

// `#[used]` keepalive: `js_string_position_to_index` is reached only from
// generated `.o`, so the auto-optimize whole-program bitcode pass would
// otherwise dead-strip it.
#[used]
static KEEP_POSITION_TO_INDEX: extern "C" fn(f64) -> i32 = js_string_position_to_index;

/// Find the last index of a substring (-1 if not found).
/// Returns the UTF-16 code unit offset of the LAST occurrence, or -1 if not found.
/// An empty needle returns the string's UTF-16 length.
#[no_mangle]
pub extern "C" fn js_string_last_index_of(
    haystack: *const StringHeader,
    needle: *const StringHeader,
) -> i32 {
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
            if n_blen > h_blen {
                return -1;
            }
            let h =
                std::str::from_utf8_unchecked(slice::from_raw_parts(string_data(haystack), h_blen));
            let n =
                std::str::from_utf8_unchecked(slice::from_raw_parts(string_data(needle), n_blen));
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

/// `String.prototype.lastIndexOf(searchString, position)` (ECMA-262 §22.1.3.9):
/// the highest match-start index `<= position` (UTF-16 units), or -1.
/// `has_pos == 0` means no `position` argument (defaults to +Infinity, i.e.
/// search the whole string) and delegates to the fast `js_string_last_index_of`.
/// `position` is `ToIntegerOrInfinity`-clamped to `[0, length]`; `NaN` → end.
#[no_mangle]
pub extern "C" fn js_string_last_index_of_from(
    haystack: *const StringHeader,
    needle: *const StringHeader,
    position: f64,
    has_pos: i32,
) -> i32 {
    if has_pos == 0 {
        return js_string_last_index_of(haystack, needle);
    }
    if !is_valid_string_ptr(haystack) {
        return -1;
    }
    let hlen16 = unsafe { (*haystack).utf16_len as i64 };
    // ToIntegerOrInfinity(position), clamped to [0, length]. NaN → search end.
    let pos16: i64 = if position.is_nan() || position >= hlen16 as f64 {
        hlen16
    } else if position <= 0.0 {
        0
    } else {
        position as i64
    };
    if !is_valid_string_ptr(needle) || unsafe { (*needle).byte_len } == 0 {
        // Empty needle matches at every position; the answer is min(pos, len).
        return pos16 as i32;
    }
    // Walk matches in ascending UTF-16 order; keep the highest start <= pos16.
    let h = string_as_str(haystack);
    let n = string_as_str(needle);
    let mut best: i32 = -1;
    for (byte_pos, _) in h.match_indices(n) {
        let u16idx = byte_offset_to_utf16_index(h, byte_pos) as i64;
        if u16idx <= pos16 {
            best = u16idx as i32;
        } else {
            break; // ascending — no later match can satisfy <= pos16
        }
    }
    best
}
