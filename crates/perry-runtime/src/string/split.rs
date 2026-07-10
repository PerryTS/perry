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
    std::ptr::write(elements_ptr.add(index), f64::from_bits(value_bits));
    // The all-pointer layout only covers the initialized prefix. Publish this
    // element before the next allocation can run a collection.
    (*arr).length = (index + 1) as u32;
    crate::gc::runtime_write_barrier_slot(
        arr as usize,
        elements_ptr.add(index) as usize,
        value_bits,
    );
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
    let str_data = string_as_str(s);
    let delim = string_as_str(delimiter);
    if delim.is_empty() {
        let Some(c) = str_data.chars().nth(index as usize) else {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        };
        let mut buf = [0u8; 4];
        let part = c.encode_utf8(&mut buf);
        return crate::value::js_nanbox_string(
            js_string_from_bytes(part.as_ptr(), part.len() as u32) as i64,
        );
    }

    let part = str_data.split(delim).nth(index as usize);
    let Some(part) = part else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    let byte_len = part.len() as u32;
    let src_is_ascii = is_ascii_string(s);
    let (result, result_data) = string_storage_alloc(byte_len);
    unsafe {
        let utf16_len = if src_is_ascii {
            byte_len
        } else {
            compute_utf16_len(part.as_ptr(), byte_len)
        };
        init_string_header(result, utf16_len, byte_len, byte_len, 0, 0);
        if byte_len != 0 {
            ptr::copy_nonoverlapping(part.as_ptr(), result_data, byte_len as usize);
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
    let str_data = string_as_str(s);
    let delim = string_as_str(delimiter);
    if delim.is_empty() {
        return str_data
            .chars()
            .nth(index as usize)
            .map_or(0.0, |c| c.len_utf16() as f64);
    }

    let Some(part) = str_data.split(delim).nth(index as usize) else {
        return 0.0;
    };
    if is_ascii_string(s) {
        part.len() as f64
    } else {
        compute_utf16_len(part.as_ptr(), part.len() as u32) as f64
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

    if delim.is_empty() {
        // Empty delimiter: count then materialize directly. Besides avoiding a
        // temporary `Vec`, rooting the result before each character allocation
        // keeps it valid if a collection runs while the array is filled.
        let mut n = str_data.chars().count();
        if limit > 0 {
            n = n.min(limit as usize);
        }
        let arr = crate::array::js_array_alloc_pointer_elements(n as u32);
        let scope = crate::gc::RuntimeHandleScope::new();
        let arr_handle = scope.root_raw_mut_ptr(arr);
        unsafe {
            for (i, c) in str_data.chars().take(n).enumerate() {
                let mut buf = [0u8; 4];
                let char_str = c.encode_utf8(&mut buf);
                let string = js_string_from_bytes(char_str.as_ptr(), char_str.len() as u32);
                store_split_string(arr_handle.get_raw_mut_ptr::<ArrayHeader>(), i, string);
            }
        }
        return arr_handle.get_raw_mut_ptr::<ArrayHeader>();
    }

    // Non-empty delimiter: count first, then materialize directly into the
    // result array. Collecting `Vec<&str>` for every split made this hot path
    // perform a separate Rust heap allocation and copy of the slice list.
    // `matches` and `split` share the same non-overlapping delimiter semantics.
    let mut n = str_data.matches(delim).count().saturating_add(1);
    if limit > 0 {
        n = n.min(limit as usize);
    }

    let src_is_ascii = is_ascii_string(s);

    let arr = crate::array::js_array_alloc_pointer_elements(n as u32);
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_mut_ptr(arr);
    unsafe {
        for (i, part) in str_data.split(delim).take(n).enumerate() {
            let byte_len = part.len() as u32;
            let (sh, data_ptr) = string_storage_alloc(byte_len);
            let utf16_len = if src_is_ascii {
                byte_len
            } else {
                compute_utf16_len(part.as_ptr(), byte_len)
            };
            init_string_header(sh, utf16_len, byte_len, byte_len, 0, 0);
            if byte_len > 0 {
                ptr::copy_nonoverlapping(part.as_ptr(), data_ptr, byte_len as usize);
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
