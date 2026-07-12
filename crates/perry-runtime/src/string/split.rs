//! `String.prototype.split` — by-string and by-empty-delimiter, with limit.

use super::*;
use crate::array::ArrayHeader;

/// Store one freshly-created heap string into a `String.prototype.split`
/// result. The result array starts with an all-pointer layout, so advancing
/// `length` after the write makes its initialized prefix visible to GC without
/// a per-element layout-map update. The write barrier remains necessary if a
/// collection has promoted the rooted result array while it is being built.
#[inline]
unsafe fn store_split_string(arr: *mut ArrayHeader, index: usize, string: *mut StringHeader) {
    const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

    let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
    let value_bits = STRING_TAG | (string as u64 & POINTER_MASK);
    // GC_STORE_AUDIT(BARRIERED): split result string slot is followed by a runtime write barrier.
    std::ptr::write(elements_ptr.add(index), f64::from_bits(value_bits));
    crate::gc::runtime_write_barrier_slot(
        arr as usize,
        elements_ptr.add(index) as usize,
        value_bits,
    );
    // The all-pointer layout only covers the initialized prefix. Publish this
    // element after the write and barrier, before the next allocation can run
    // a collection.
    (*arr).length = (index + 1) as u32;
}

/// Advance to the next UTF-8 character boundary strictly after `i`.
#[cfg(feature = "regex-engine")]
fn next_char_boundary(s: &str, i: usize) -> usize {
    let mut j = i + 1;
    while j < s.len() && !s.is_char_boundary(j) {
        j += 1;
    }
    j
}

/// JS-spec `RegExp.prototype[Symbol.split]` (21.2.5.11) for the standard
/// `regex` engine. Walks the subject with a *sticky* match at each position,
/// applies the `e == p` empty-match skip (so a zero-width match at the current
/// segment start does not emit an empty string), and splices captured groups
/// (unmatched groups → `undefined`/`None`) after each segment. Honors `limit`
/// (`< 0` ⇒ unbounded) by stopping once `limit` elements have been produced.
/// Each element is `Some(substring)` or `None` for a spliced unmatched group.
#[cfg(feature = "regex-engine")]
pub(crate) fn spec_regex_split(regex: &regex::Regex, s: &str, limit: i32) -> Vec<Option<String>> {
    let mut out: Vec<Option<String>> = Vec::new();
    let unbounded = limit < 0;
    // Returns true once the limit is reached (caller must stop).
    let push = |out: &mut Vec<Option<String>>, v: Option<String>| -> bool {
        out.push(v);
        !unbounded && out.len() as i32 >= limit
    };
    let size = s.len();
    if size == 0 {
        // Empty subject: `[""]` unless the pattern matches the empty string.
        if regex.find(s).is_none() {
            out.push(Some(String::new()));
        }
        return out;
    }
    let mut p = 0usize; // start of the pending segment
    let mut q = 0usize; // scan cursor
    while q < size {
        match regex.find_at(s, q) {
            // Sticky: a match must begin exactly at `q`.
            Some(m) if m.start() == q => {
                let e = m.end().min(size);
                if e == p {
                    // Zero-width match at the segment start: skip it.
                    q = next_char_boundary(s, q);
                } else {
                    if push(&mut out, Some(s[p..q].to_string())) {
                        return out;
                    }
                    if let Some(caps) = regex.captures_at(s, q) {
                        for i in 1..caps.len() {
                            let g = caps.get(i).map(|gm| gm.as_str().to_string());
                            if push(&mut out, g) {
                                return out;
                            }
                        }
                    }
                    p = e;
                    q = p;
                }
            }
            // Leftmost match lies to the right of `q`; no match (and thus no
            // zero-width match) exists in between, so jump straight to it.
            Some(m) => q = m.start(),
            None => break,
        }
    }
    if unbounded || (out.len() as i32) < limit {
        out.push(Some(s[p..size].to_string()));
    }
    out
}

/// Split a string by a delimiter
/// Returns an array of string pointers (stored as f64 bit patterns)
#[no_mangle]
pub extern "C" fn js_string_split(
    s: *const StringHeader,
    delimiter: *const StringHeader,
) -> *mut ArrayHeader {
    js_string_split_n(s, delimiter, -1)
}

/// Locate one part of a non-empty byte-delimiter split without constructing
/// `&str` slices. Perry payloads may contain malformed WTF-8, so the scan must
/// not rely on Rust's UTF-8 validity invariant.
fn split_part_byte_range(source: &[u8], delimiter: &[u8], target: usize) -> Option<(usize, usize)> {
    debug_assert!(!delimiter.is_empty());
    let mut part_start = 0usize;
    let mut part_index = 0usize;
    let mut scan = 0usize;
    while scan + delimiter.len() <= source.len() {
        if source[scan..].starts_with(delimiter) {
            if part_index == target {
                return Some((part_start, scan));
            }
            part_index += 1;
            scan += delimiter.len();
            part_start = scan;
        } else {
            scan += 1;
        }
    }
    (part_index == target).then_some((part_start, source.len()))
}

/// Materialize one element of a string-delimiter split as a boxed JS value.
/// This is used when codegen proves the result array does not escape and only
/// a constant element is observed. A missing element remains `undefined`.
#[no_mangle]
pub extern "C" fn js_string_split_part_value(
    s: *const StringHeader,
    delimiter: *const StringHeader,
    index: i32,
) -> f64 {
    if index < 0 || !is_valid_string_ptr(s) || !is_valid_string_ptr(delimiter) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let source = unsafe { slice::from_raw_parts(string_data(s), (*s).byte_len as usize) };
    let delimiter_bytes =
        unsafe { slice::from_raw_parts(string_data(delimiter), (*delimiter).byte_len as usize) };
    if delimiter_bytes.is_empty() {
        let mut byte_offset = 0usize;
        for _ in 0..index as usize {
            if byte_offset >= source.len() {
                return f64::from_bits(crate::value::TAG_UNDEFINED);
            }
            let (advance, _, _) = crate::string::wtf8_step(source, byte_offset);
            byte_offset = (byte_offset + advance).min(source.len());
        }
        if byte_offset >= source.len() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        let (advance, _, _) = crate::string::wtf8_step(source, byte_offset);
        let end = (byte_offset + advance).min(source.len());
        let mut buf = [0u8; 4];
        let part = &source[byte_offset..end];
        buf[..part.len()].copy_from_slice(part);
        let has_lone_surrogate = unsafe {
            (*s).flags & STRING_FLAG_HAS_LONE_SURROGATES != 0
                && crate::string::bytes_have_lone_surrogate(part)
        };
        let result = if has_lone_surrogate {
            js_string_from_wtf8_bytes(buf.as_ptr(), part.len() as u32)
        } else {
            js_string_from_bytes(buf.as_ptr(), part.len() as u32)
        };
        return crate::value::js_nanbox_string(result as i64);
    }

    let Some((start, end)) = split_part_byte_range(source, delimiter_bytes, index as usize) else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    let byte_len = (end - start) as u32;
    let source_all_ascii = source.iter().all(|&byte| byte < 0x80);
    let source_has_lone_surrogates = unsafe { (*s).flags & STRING_FLAG_HAS_LONE_SURROGATES != 0 };
    let scope = crate::gc::RuntimeHandleScope::new();
    let source_handle = scope.root_string_ptr(s);
    let (result, result_data) = string_storage_alloc(byte_len);
    unsafe {
        let source_now = source_handle.get_raw_const_ptr::<StringHeader>();
        let part_ptr = string_data(source_now).add(start);
        let (utf16_len, flags) = if source_all_ascii {
            (byte_len, 0)
        } else {
            let part = slice::from_raw_parts(part_ptr, byte_len as usize);
            let flags =
                if source_has_lone_surrogates && crate::string::bytes_have_lone_surrogate(part) {
                    STRING_FLAG_HAS_LONE_SURROGATES
                } else {
                    0
                };
            (compute_utf16_len(part_ptr, byte_len), flags)
        };
        init_string_header(result, utf16_len, byte_len, byte_len, 0, flags);
        if byte_len != 0 {
            ptr::copy_nonoverlapping(part_ptr, result_data, byte_len as usize);
        }
    }
    crate::value::js_nanbox_string(result as i64)
}

/// Return the UTF-16 length of one string-delimiter split part without
/// materializing that part. Scalar replacement uses this for a direct
/// `split("literal")[constant].length` read.
///
/// A missing part returns zero, matching the existing scalar string-length
/// lowering's guarded pointer load for `undefined`.
#[no_mangle]
pub extern "C" fn js_string_split_part_utf16_length(
    s: *const StringHeader,
    delimiter: *const StringHeader,
    index: i32,
) -> f64 {
    if index < 0 || !is_valid_string_ptr(s) || !is_valid_string_ptr(delimiter) {
        return 0.0;
    }
    let source = unsafe { slice::from_raw_parts(string_data(s), (*s).byte_len as usize) };
    let delimiter_bytes =
        unsafe { slice::from_raw_parts(string_data(delimiter), (*delimiter).byte_len as usize) };
    if delimiter_bytes.is_empty() {
        let mut byte_offset = 0usize;
        for _ in 0..index as usize {
            if byte_offset >= source.len() {
                return 0.0;
            }
            let (advance, _, _) = crate::string::wtf8_step(source, byte_offset);
            byte_offset = (byte_offset + advance).min(source.len());
        }
        if byte_offset >= source.len() {
            return 0.0;
        }
        let (_, units, _) = crate::string::wtf8_step(source, byte_offset);
        return units as f64;
    }

    let Some((start, end)) = split_part_byte_range(source, delimiter_bytes, index as usize) else {
        return 0.0;
    };
    let part = &source[start..end];
    if part.iter().all(|&byte| byte < 0x80) {
        part.len() as f64
    } else {
        compute_utf16_len(part.as_ptr(), part.len() as u32) as f64
    }
}

#[inline]
fn ascii_upper_byte(byte: u8) -> u8 {
    if byte.is_ascii_lowercase() {
        byte - (b'a' - b'A')
    } else {
        byte
    }
}

/// Return the UTF-16 length of `s.toUpperCase().split(delimiter)[index]`
/// without materializing the uppercase JS string when `s` is ASCII.
#[no_mangle]
pub extern "C" fn js_string_to_upper_case_split_part_utf16_length(
    s: *const StringHeader,
    delimiter: *const StringHeader,
    index: i32,
) -> f64 {
    if index < 0 || !is_valid_string_ptr(s) || !is_valid_string_ptr(delimiter) {
        return 0.0;
    }
    let bytes = unsafe { slice::from_raw_parts(string_data(s), (*s).byte_len as usize) };
    let delimiter_bytes =
        unsafe { slice::from_raw_parts(string_data(delimiter), (*delimiter).byte_len as usize) };
    if !bytes.iter().all(|&byte| byte < 0x80) {
        let scope = crate::gc::RuntimeHandleScope::new();
        let source_handle = scope.root_string_ptr(s);
        let delimiter_handle = scope.root_string_ptr(delimiter);
        let upper = crate::string::js_string_to_upper_case(
            source_handle.get_raw_const_ptr::<StringHeader>(),
        );
        let upper_handle = scope.root_string_ptr(upper);
        return js_string_split_part_utf16_length(
            upper_handle.get_raw_const_ptr::<StringHeader>(),
            delimiter_handle.get_raw_const_ptr::<StringHeader>(),
            index,
        );
    }

    if delimiter_bytes.is_empty() {
        return ((index as usize) < bytes.len()) as u8 as f64;
    }
    if !delimiter_bytes.iter().all(|&byte| byte < 0x80) {
        return if index == 0 { bytes.len() as f64 } else { 0.0 };
    }

    let target = index as usize;
    let mut part_start = 0usize;
    let mut part_index = 0usize;
    let mut scan = 0usize;
    while scan + delimiter_bytes.len() <= bytes.len() {
        let matches = bytes[scan..scan + delimiter_bytes.len()]
            .iter()
            .zip(delimiter_bytes)
            .all(|(&source_byte, &delimiter_byte)| ascii_upper_byte(source_byte) == delimiter_byte);
        if matches {
            if part_index == target {
                return (scan - part_start) as f64;
            }
            part_index += 1;
            scan += delimiter_bytes.len();
            part_start = scan;
        } else {
            scan += 1;
        }
    }
    if part_index == target {
        (bytes.len() - part_start) as f64
    } else {
        0.0
    }
}

/// Split a string by a delimiter, with optional limit (issue #567).
/// `limit < 0` → no limit (matches `js_string_split`).
/// `limit == 0` → empty array.
/// `limit > 0` → at most `limit` substrings.
#[no_mangle]
pub extern "C" fn js_string_split_n(
    s: *const StringHeader,
    delimiter: *const StringHeader,
    limit: i32,
) -> *mut ArrayHeader {
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
    #[cfg(feature = "regex-engine")]
    if crate::regex::is_regex_pointer(delimiter as *const u8) {
        return crate::regex::js_string_split_regex_n(
            s,
            delimiter as *const crate::regex::RegExpHeader,
            limit,
        );
    }

    if limit == 0 {
        return crate::array::js_array_alloc(0);
    }

    let str_data = string_as_str(s);
    let delim = if !is_valid_string_ptr(delimiter) {
        ""
    } else {
        string_as_str(delimiter)
    };

    // Per-part metadata inputs, derived ONCE from the source payload.
    //
    // NOT `is_ascii_string(s)`: that compares `byte_len == utf16_len` over the
    // WHOLE source, which malformed bytes can satisfy while the individual
    // parts do not. For `[0x80, b'|', 0xF0]` the source is 3 == 3 (so the old
    // check said "ASCII"), but the parts need utf16_len 0 and 2 — the fast path
    // stamped 1 and 1 onto them, corrupting `.length` and every downstream
    // index operation. Scan the bytes instead: a genuinely all-ASCII source has
    // all-ASCII parts, which IS sound per-part.
    //
    // Both of these are plain `bool`s, so they stay valid even if a later
    // allocation moves the source string.
    let (src_all_ascii, src_has_lone_surrogates) = unsafe {
        let bytes = slice::from_raw_parts(string_data(s), (*s).byte_len as usize);
        (
            bytes.iter().all(|&b| b < 0x80),
            (*s).flags & STRING_FLAG_HAS_LONE_SURROGATES != 0,
        )
    };

    if delim.is_empty() {
        // Empty delimiter: split into individual characters (single pass).
        //
        // #6085: `str_data.chars()` decodes through std's UTF-8-validity
        // assumption (`next_code_point` reads continuation bytes with
        // `unwrap_unchecked`). Perry payloads are EXACT-SIZED and not
        // guaranteed valid UTF-8 — a `Buffer`/FFI blob sliced at a byte
        // delimiter can end in a truncated multi-byte lead — so that walk read
        // up to 3 bytes past the end of the allocation. Step the raw bytes with
        // the bounded WTF-8 decoder instead and emit each sequence verbatim;
        // well-formed input yields byte-identical parts.
        // Pass 1: count the sequences. No allocation happens here, so the
        // source payload cannot move under us.
        let mut n = 0usize;
        unsafe {
            let src = slice::from_raw_parts(string_data(s), (*s).byte_len as usize);
            let mut i = 0usize;
            while i < src.len() {
                let (advance, _, _) = crate::string::wtf8_step(src, i);
                i = (i + advance).min(src.len());
                n += 1;
                if limit > 0 && n as i64 >= limit as i64 {
                    break;
                }
            }
        }

        // Pass 2: allocate the result array, then fill it slot by slot.
        //
        // GC safety: `js_string_from_bytes` allocates, and a collection can
        // both RECLAIM and (under C4b evacuation) MOVE the source string and
        // the result array. A raw `*mut StringHeader` parked in a plain `Vec`
        // is neither a root nor rewritten, so accumulating the parts there and
        // writing them into the array afterwards is unsound. Root the source
        // and the array in a `RuntimeHandleScope`, re-read both after every
        // allocation, and store each part into the (rooted) array immediately —
        // from then on the array keeps it alive.
        let arr = crate::array::js_array_alloc_pointer_elements(n as u32);
        let scope = crate::gc::RuntimeHandleScope::new();
        let s_handle = scope.root_string_ptr(s);
        let arr_handle = scope.root_raw_mut_ptr(arr);

        let mut i = 0usize;
        for idx in 0..n {
            // Copy the sequence into a stack buffer BEFORE allocating:
            // `js_string_from_bytes` allocates first and copies second, so
            // handing it a pointer into the GC heap is the #5062 dangling-source
            // class. A WTF-8 sequence is at most 4 bytes.
            let mut buf = [0u8; 4];
            let seq_len;
            unsafe {
                let s_now = s_handle.get_raw_const_ptr::<StringHeader>();
                let src = slice::from_raw_parts(string_data(s_now), (*s_now).byte_len as usize);
                if i >= src.len() {
                    break;
                }
                let (advance, _, _) = crate::string::wtf8_step(src, i);
                let end = (i + advance).min(src.len());
                seq_len = end - i;
                buf[..seq_len].copy_from_slice(&src[i..end]);
                i = end;
            }
            // `js_string_from_bytes` derives utf16_len from the bytes (correct
            // even for a malformed sequence) but hardcodes flags = 0. A lone
            // surrogate carved out of a WTF-8 source must keep its flag, or
            // `isWellFormed()` on the part wrongly reports true.
            let seq = &buf[..seq_len];
            let sh = if src_has_lone_surrogates && crate::string::bytes_have_lone_surrogate(seq) {
                js_string_from_wtf8_bytes(seq.as_ptr(), seq_len as u32)
            } else {
                js_string_from_bytes(seq.as_ptr(), seq_len as u32)
            };
            unsafe {
                store_split_string(arr_handle.get_raw_mut_ptr::<ArrayHeader>(), idx, sh);
            }
        }
        return arr_handle.get_raw_mut_ptr::<ArrayHeader>();
    }

    // Non-empty delimiter: record the parts as BYTE RANGES into the source
    // payload, not as `&str` slices. `string_storage_alloc` below allocates, and
    // a collection can move the source string — borrowed slices (and the raw
    // pointers inside them) would dangle, and `ptr::copy_nonoverlapping` from a
    // stale address is the #5062 class. Offsets stay valid across a move; the
    // source address is re-read from a rooted handle on every iteration.
    let src_base = str_data.as_ptr() as usize;
    let mut part_ranges: Vec<(usize, usize)> = str_data
        .split(delim)
        .map(|part| (part.as_ptr() as usize - src_base, part.len()))
        .collect();
    if limit > 0 && (part_ranges.len() as i64) > (limit as i64) {
        part_ranges.truncate(limit as usize);
    }
    let n = part_ranges.len();

    let arr = crate::array::js_array_alloc_pointer_elements(n as u32);
    let scope = crate::gc::RuntimeHandleScope::new();
    let s_handle = scope.root_string_ptr(s);
    let arr_handle = scope.root_raw_mut_ptr(arr);

    unsafe {
        for (i, &(offset, byte_len_usize)) in part_ranges.iter().enumerate() {
            let byte_len = byte_len_usize as u32;
            // Allocate the destination FIRST (it may move the source), then
            // re-read the source address before touching its bytes.
            let (sh, data_ptr) = string_storage_alloc(byte_len);
            let s_now = s_handle.get_raw_const_ptr::<StringHeader>();
            let part_ptr = string_data(s_now).add(offset);
            // Derive metadata from THIS PART's own bytes. The only shortcut
            // taken is the all-ASCII one, which was verified by scanning the
            // source payload (so it holds for every part).
            let (utf16_len, flags) = if src_all_ascii {
                (byte_len, 0)
            } else {
                let part_bytes = slice::from_raw_parts(part_ptr, byte_len as usize);
                let flags = if src_has_lone_surrogates
                    && crate::string::bytes_have_lone_surrogate(part_bytes)
                {
                    STRING_FLAG_HAS_LONE_SURROGATES
                } else {
                    0
                };
                (compute_utf16_len(part_ptr, byte_len), flags)
            };
            init_string_header(sh, utf16_len, byte_len, byte_len, 0, flags);
            if byte_len > 0 {
                ptr::copy_nonoverlapping(part_ptr, data_ptr, byte_len as usize);
            }
            store_split_string(arr_handle.get_raw_mut_ptr::<ArrayHeader>(), i, sh);
        }
    }

    arr_handle.get_raw_mut_ptr::<ArrayHeader>()
}

/// `ToUint32(ToNumber(value))` (ECMA-262 §7.1.7). Runs the full `ToNumber`
/// (so a boxed `{ valueOf }` / `{ toString }` argument is coerced and may
/// throw), then reduces mod 2^32. `NaN`/`±Infinity`/`0` → 0.
fn split_limit_to_uint32(boxed: f64) -> u32 {
    let n = crate::builtins::js_number_coerce(boxed);
    if !n.is_finite() || n == 0.0 {
        return 0;
    }
    n.trunc().rem_euclid(4_294_967_296.0) as u32
}

/// Build the single-element array `[S]` (the `separator === undefined` result
/// of `String.prototype.split`).
fn split_single_element(s: *const StringHeader) -> *mut ArrayHeader {
    const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
    let arr = crate::array::js_array_alloc(1);
    unsafe {
        (*arr).length = 1;
        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        let nanboxed = STRING_TAG | (s as u64 & POINTER_MASK);
        // GC_STORE_AUDIT(BARRIERED): slot recorded via note_array_slot.
        std::ptr::write(elements_ptr, f64::from_bits(nanboxed));
        crate::array::note_array_slot(arr, 0, nanboxed);
    }
    arr
}

/// `String.prototype.split(separator, limit)` (ECMA-262 §22.1.3.21) with full
/// argument coercion. `s` is the already-`ToString`-coerced `this`;
/// `separator` and `limit` arrive as boxed `JSValue`s. The codegen fast path
/// and the runtime dispatch arm both route here so coercion is uniform:
///   - a RegExp separator takes over via `RegExp[Symbol.split]` (detected by
///     the regex-pointer registry), with `ToUint32(limit)`;
///   - `lim = ToUint32(limit)` is computed BEFORE `ToString(separator)` (spec
///     order — either may run user `valueOf`/`toString` and throw);
///   - `limit === 0` ⇒ empty array;
///   - `separator === undefined` ⇒ single-element `[S]`;
///   - otherwise split by `ToString(separator)`, capped at `lim`.
#[no_mangle]
pub extern "C" fn js_string_split_value(
    s: *const StringHeader,
    separator: f64,
    limit: f64,
) -> *mut ArrayHeader {
    use crate::value::JSValue;
    let sep_jv = JSValue::from_bits(separator.to_bits());
    let lim_jv = JSValue::from_bits(limit.to_bits());

    // Step 2: a separator with a `[Symbol.split]` method (a RegExp) takes over.
    #[cfg(feature = "regex-engine")]
    if sep_jv.is_pointer() {
        let ptr = crate::value::js_nanbox_get_pointer(separator) as *const u8;
        if crate::regex::is_regex_pointer(ptr) {
            let limit_i32 = if lim_jv.is_undefined() {
                -1
            } else {
                let u = split_limit_to_uint32(limit);
                if u > i32::MAX as u32 {
                    i32::MAX
                } else {
                    u as i32
                }
            };
            return crate::regex::js_string_split_regex_n(
                s,
                ptr as *const crate::regex::RegExpHeader,
                limit_i32,
            );
        }
    }

    // Step 6: lim = limit===undefined ? 2^32-1 : ToUint32(limit) (may throw).
    let lim: u32 = if lim_jv.is_undefined() {
        u32::MAX
    } else {
        split_limit_to_uint32(limit)
    };

    // Step 7: R = ToString(separator) (may throw). For `undefined` the result
    // is unused (step 9) and `ToString(undefined)` is side-effect-free, so we
    // skip it.
    let sep_is_undefined = sep_jv.is_undefined();
    let r_str: *mut StringHeader = if sep_is_undefined {
        std::ptr::null_mut()
    } else {
        crate::builtins::js_string_coerce(separator)
    };

    // Step 8: limit 0 → empty array.
    if lim == 0 {
        return crate::array::js_array_alloc(0);
    }
    // Step 9: undefined separator → [S].
    if sep_is_undefined {
        return split_single_element(s);
    }

    // `js_string_split_n` takes an i32 limit (< 0 ⇒ unbounded); cap `lim`.
    let limit_i32 = if lim > i32::MAX as u32 {
        i32::MAX
    } else {
        lim as i32
    };
    js_string_split_n(s, r_str, limit_i32)
}
