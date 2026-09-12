//! Slicing, substring, trimming, case conversion, and index-of operations.

use super::*;

/// Get a slice of a string in UTF-16 code units
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

    super::slice_range::copy_utf16_range(s, start as u32, end as u32)
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

    super::slice_range::copy_utf16_range(s, start as u32, end as u32)
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

    super::slice_range::copy_utf16_range(s, start as u32, end as u32)
}

// `#[used]` keepalive: `js_string_substr` is reached only from generated `.o`,
// so the whole-program auto-optimize bitcode rebuild would dead-strip it
// without an anchor (see project_auto_optimize_keepalive_3320).
#[cfg(feature = "keepalive-anchors")]
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

/// Is the WTF-8 sequence starting at `i` a JS whitespace code point?
/// Returns `(is_whitespace, advance)`.
///
/// The sequence must be COMPLETE — fully contained in `bytes` — to count as
/// whitespace. `wtf8_step` zero-fills continuation bytes that don't exist, so a
/// truncated tail like `E2 80` would otherwise decode as U+2000 (EN QUAD, which
/// IS JS whitespace) and `trim`/`trimEnd` would silently eat those bytes. A
/// truncated tail is therefore treated as non-whitespace and preserved verbatim
/// — consistent with `case_convert`, which also only maps complete sequences.
#[inline]
fn js_whitespace_seq_at(bytes: &[u8], i: usize) -> (bool, usize) {
    let (advance, units, cp) = crate::string::wtf8_step(bytes, i);
    let complete = i + advance <= bytes.len();
    let is_ws = complete && units > 0 && char::from_u32(cp).map(is_js_whitespace).unwrap_or(false);
    (is_ws, advance)
}

/// Byte range `[start, end)` of `bytes` after trimming JS whitespace from the
/// requested ends. Bounds-driven (#6085): decodes with `wtf8_step` instead of
/// `str::trim_matches`, whose `chars()` walk reads continuation bytes past an
/// exact-sized payload that ends in a truncated multi-byte lead.
///
/// Trimming behavior on valid input is unchanged; a truncated/invalid tail is
/// never treated as whitespace, so it survives the trim byte-for-byte.
pub(super) fn js_whitespace_trim_range(
    bytes: &[u8],
    trim_start: bool,
    trim_end: bool,
) -> (usize, usize) {
    let mut start = 0usize;
    if trim_start {
        while start < bytes.len() {
            let (is_ws, advance) = js_whitespace_seq_at(bytes, start);
            if !is_ws {
                break;
            }
            start = (start + advance).min(bytes.len());
        }
    }

    let mut end = bytes.len();
    if trim_end {
        end = js_whitespace_trim_end(bytes, start);
    }

    (start, end.max(start))
}

/// Walk only the trailing edge for valid UTF-8/WTF-8. At most four bytes
/// identify a candidate sequence. Malformed payloads can have ambiguous
/// boundaries (a lead can consume an ASCII space or another lead), so use the
/// historical bounded forward walk for those instead of guessing.
fn js_whitespace_trim_end(bytes: &[u8], start: usize) -> usize {
    let mut end = bytes.len();
    while end > start {
        let mut candidate = end - 1;
        while candidate > start && bytes[candidate] & 0xc0 == 0x80 && end - candidate < 4 {
            candidate -= 1;
        }
        // A lead in the preceding three bytes must not nominally extend into
        // this candidate. Valid text never does; malformed text may, even when
        // the candidate itself looks like a complete whitespace sequence.
        for i in candidate.saturating_sub(3).max(start)..candidate {
            if bytes[i] >= 0xc0 && i + wtf8_step(bytes, i).0 > candidate {
                return js_whitespace_trim_end_forward(bytes, start);
            }
        }
        let (is_ws, advance) = js_whitespace_seq_at(&bytes[..end], candidate);
        if candidate + advance != end {
            return js_whitespace_trim_end_forward(bytes, start);
        }
        if !is_ws {
            break;
        }
        end = candidate;
    }
    end
}

fn js_whitespace_trim_end_forward(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    let mut end = start;
    while i < bytes.len() {
        let (is_ws, advance) = js_whitespace_seq_at(bytes, i);
        i = (i + advance).min(bytes.len());
        if !is_ws {
            end = i;
        }
    }
    end
}

/// Shared trim entry point over the raw payload bytes.
fn trim_impl(s: *const StringHeader, trim_start: bool, trim_end: bool) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let mode = u8::from(trim_start) | (u8::from(trim_end) << 1);
    if unsafe { (*s).byte_len } >= trim_cache::MIN_CACHED_BYTES {
        let cached = trim_cache::lookup(s, mode);
        if !cached.is_null() {
            return cached;
        }
    }
    let bytes = unsafe { slice::from_raw_parts(string_data(s), (*s).byte_len as usize) };
    let (start, end) = js_whitespace_trim_range(bytes, trim_start, trim_end);
    if start == end {
        return js_string_from_bytes(ptr::null(), 0);
    }
    if start == 0
        && end == bytes.len()
        && !matches!(
            crate::arena::classify_heap_space(s as usize),
            crate::arena::HeapSpace::Unknown
        )
    {
        // Returning the receiver creates an alias. Do not leave a unique
        // append buffer mutable. Foreign storage must still be copied because
        // a GC root cannot extend the external owner's lifetime.
        if unsafe { (*s).refcount } != 0 {
            js_string_addref(s.cast_mut());
        }
        return s.cast_mut();
    }
    // Count only removed edges. Recounting the retained payload would restore
    // O(interior length) scanning even after the reverse boundary walk.
    let removed_units =
        compute_utf16_len_wtf8(&bytes[..start]) + compute_utf16_len_wtf8(&bytes[end..]);
    let utf16_len = unsafe { (*s).utf16_len }.saturating_sub(removed_units);
    // Preserve the WTF-8 flag: trimming only removes well-formed whitespace, so
    // any lone surrogate in the source survives into the result.
    let flags = unsafe { (*s).flags } & STRING_FLAG_HAS_LONE_SURROGATES;
    let scope = crate::gc::RuntimeHandleScope::new();
    let source = scope.root_string_ptr(s);
    // string_copy_range re-reads its rooted source AFTER allocating. Never
    // hand the allocator a borrowed pointer into the moving source payload.
    let result = string_copy_range(s, start, (end - start) as u32, utf16_len, flags);
    source.with_const_ptr(|source_now: *const StringHeader| {
        trim_cache::remember(source_now, result, mode)
    });
    result
}

/// Trim whitespace from both ends of a string
#[no_mangle]
pub extern "C" fn js_string_trim(s: *const StringHeader) -> *mut StringHeader {
    trim_impl(s, true, true)
}

/// Trim whitespace from start of a string (trimStart/trimLeft)
#[no_mangle]
pub extern "C" fn js_string_trim_start(s: *const StringHeader) -> *mut StringHeader {
    trim_impl(s, true, false)
}

/// Trim whitespace from end of a string (trimEnd/trimRight)
#[no_mangle]
pub extern "C" fn js_string_trim_end(s: *const StringHeader) -> *mut StringHeader {
    trim_impl(s, false, true)
}

/// Unicode case conversion over the raw payload (#6085).
///
/// `str::to_lowercase`/`to_uppercase` iterate `chars()`, which reads
/// continuation bytes past an exact-sized payload ending in a truncated
/// multi-byte lead. Decode with the bounded `wtf8_step` instead: sequences that
/// form a real Unicode scalar get the full `char` case mapping (identical
/// output for well-formed input, including multi-char expansions like `ß`→`SS`),
/// while a lone surrogate or a truncated/invalid sequence is copied through
/// VERBATIM — which also preserves the WTF-8 round-trip (#4793) that the old
/// `from_utf8_unchecked` path only got by accident.
fn case_convert(s: *const StringHeader, upper: bool) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let bytes = unsafe { slice::from_raw_parts(string_data(s), (*s).byte_len as usize) };

    // ASCII fast path (#10090): default-locale ASCII case mapping is
    // context-free and strictly 1-byte-in/1-byte-out, so skip the scalar
    // wtf8_step decode / per-char to_lowercase()/to_uppercase() iterator
    // construction / re-encode loop entirely and let `to_ascii_lowercase`/
    // `to_ascii_uppercase` do a vectorizable byte-table transform instead.
    //
    // Must gate on `bytes.is_ascii()` (a real per-byte scan), NOT the
    // `is_ascii_string(s)` `byte_len == utf16_len` AGGREGATE proxy used
    // elsewhere for O(1) checks: that aggregate can lie for malformed WTF-8,
    // where a stray continuation byte (0 UTF-16 units) and a truncated
    // multi-byte lead (2 units) cancel out to look ASCII while containing
    // non-ASCII bytes (see `split_parts_get_metadata_from_their_own_bytes`).
    // A genuinely all-ASCII input can never carry a lone surrogate, so the
    // result's flags are trivially 0 and its utf16_len == its byte_len.
    if bytes.is_ascii() {
        let out = if upper {
            bytes.to_ascii_uppercase()
        } else {
            bytes.to_ascii_lowercase()
        };
        let len = out.len() as u32;
        return js_string_from_bytes_known_utf16(out.as_ptr(), len, len, 0);
    }

    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut has_lone_surrogate = false;
    let mut buf = [0u8; 4];
    let mut i = 0usize;
    while i < bytes.len() {
        let (advance, units, cp) = crate::string::wtf8_step(bytes, i);
        let end = (i + advance).min(bytes.len());
        // A sequence is only case-mapped when it decodes to a real scalar value
        // AND the encoder round-trips it (a truncated tail must not be
        // "repaired" into a different character).
        let mapped = if units > 0 && advance == end - i {
            char::from_u32(cp)
        } else {
            None
        };
        match mapped {
            Some(ch) => {
                if upper {
                    for c in ch.to_uppercase() {
                        out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                    }
                } else {
                    for c in ch.to_lowercase() {
                        out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                    }
                }
            }
            None => {
                // Lone surrogate / truncated / stray continuation byte: copy the
                // raw bytes so the payload round-trips unchanged.
                if (0xD800..=0xDFFF).contains(&cp) {
                    has_lone_surrogate = true;
                }
                out.extend_from_slice(&bytes[i..end]);
            }
        }
        i = end;
    }
    if has_lone_surrogate || unsafe { (*s).flags } & STRING_FLAG_HAS_LONE_SURROGATES != 0 {
        return js_string_from_wtf8_bytes(out.as_ptr(), out.len() as u32);
    }
    js_string_from_bytes(out.as_ptr(), out.len() as u32)
}

/// Convert string to lowercase
#[no_mangle]
pub extern "C" fn js_string_to_lower_case(s: *const StringHeader) -> *mut StringHeader {
    case_convert(s, false)
}

/// Convert string to uppercase
#[no_mangle]
pub extern "C" fn js_string_to_upper_case(s: *const StringHeader) -> *mut StringHeader {
    case_convert(s, true)
}

/// Find a substring in `s.toUpperCase()` without allocating that intermediate
/// JS string for ASCII inputs. Used only by scalar replacement of a
/// non-escaping uppercase local.
#[no_mangle]
pub extern "C" fn js_string_to_upper_case_index_of(
    s: *const StringHeader,
    needle: *const StringHeader,
) -> i32 {
    if !is_valid_string_ptr(s) || !is_valid_string_ptr(needle) {
        return -1;
    }
    let source = string_as_str(s);
    let needle = string_as_str(needle);
    if is_ascii_string(s) {
        if !needle.is_ascii() {
            return -1;
        }
        let needle = needle.as_bytes();
        if needle.is_empty() {
            return 0;
        }
        let bytes = source.as_bytes();
        if needle.len() > bytes.len() {
            return -1;
        }
        for start in 0..=bytes.len() - needle.len() {
            if bytes[start..start + needle.len()].iter().zip(needle).all(
                |(&source_byte, &needle_byte)| {
                    let upper = if source_byte.is_ascii_lowercase() {
                        source_byte - (b'a' - b'A')
                    } else {
                        source_byte
                    };
                    upper == needle_byte
                },
            ) {
                return start as i32;
            }
        }
        return -1;
    }

    let upper = source.to_uppercase();
    upper.find(needle).map_or(-1, |byte_offset| {
        byte_offset_to_utf16_index(&upper, byte_offset) as i32
    })
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
#[cfg(feature = "keepalive-anchors")]
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
