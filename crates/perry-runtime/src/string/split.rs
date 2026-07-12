//! `String.prototype.split` — by-string and by-empty-delimiter, with limit.

use super::*;
use crate::array::ArrayHeader;

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

    const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

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
        let arr = crate::array::js_array_alloc(n as u32);
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
                let arr_now = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
                let elements_ptr =
                    (arr_now as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
                let nanboxed = STRING_TAG | (sh as u64 & POINTER_MASK);
                // GC_STORE_AUDIT(BARRIERED): split char slot is immediately recorded via note_array_slot.
                std::ptr::write(elements_ptr.add(idx), f64::from_bits(nanboxed));
                crate::array::note_array_slot(arr_now, idx, nanboxed);
                // Publish the slot only once it holds a real value: `js_array_alloc`
                // does NOT zero its storage, so a GC that scanned `length = n` up
                // front would read uninitialized slots as JSValues.
                (*arr_now).length = (idx + 1) as u32;
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

    let arr = crate::array::js_array_alloc(n as u32);
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
            let arr_now = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
            let elements_ptr =
                (arr_now as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
            let nanboxed = STRING_TAG | (sh as u64 & POINTER_MASK);
            // GC_STORE_AUDIT(BARRIERED): split part slot is immediately recorded via note_array_slot.
            std::ptr::write(elements_ptr.add(i), f64::from_bits(nanboxed));
            crate::array::note_array_slot(arr_now, i, nanboxed);
            // Publish incrementally — `js_array_alloc` leaves the storage
            // uninitialized, so `length` must never cover an unwritten slot.
            (*arr_now).length = (i + 1) as u32;
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
