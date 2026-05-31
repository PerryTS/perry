//! String concatenation: pairwise, fused with NaN-boxed value, and n-way chain.

use super::intern::{
    concat_content_matches, fnv1a_concat, with_intern_table, INTERN_MAX_BYTE_LEN, INTERN_TABLE_MASK,
};
use super::*;

/// SSO-aware string concatenation: takes both operands as NaN-boxed f64
/// values, returns the result as an SSO `f64` when total ≤
/// `SHORT_STRING_MAX_LEN` (zero heap alloc), or as a heap `STRING_TAG`-
/// boxed pointer otherwise.
///
/// This is the engine-style fast path for `s + t` in code where both
/// operands are statically-typed strings. The previous lowering had
/// codegen `unbox_str_handle` each operand (which materialises SSO →
/// heap, defeating the whole SSO win), call `js_string_concat`
/// (heap-only), then re-NaN-box the result. For ABC451D's recursive
/// `before + after` (1.4M concats with 1-9 byte operands, all SSO), that
/// was 3 heap allocations per concat. The new path keeps SSO inline
/// throughout — for the common case where both operands AND the
/// result fit SSO (≤ 5 bytes total), there's literally zero heap
/// allocation. Result is returned NaN-boxed so callers don't need a
/// follow-up wrap.
#[no_mangle]
pub extern "C" fn js_string_concat_box(l_value: f64, r_value: f64) -> f64 {
    let mut scratch_l = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let mut scratch_r = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let l = str_bytes_from_jsvalue(l_value, &mut scratch_l).unwrap_or((std::ptr::null(), 0));
    let r = str_bytes_from_jsvalue(r_value, &mut scratch_r).unwrap_or((std::ptr::null(), 0));
    let total_blen = l.1 + r.1;

    // SSO fast path — assemble the result inline when it fits (≤ 5
    // bytes). Pure bit arithmetic, no heap touch.
    if total_blen as usize <= crate::value::SHORT_STRING_MAX_LEN {
        unsafe {
            let mut payload: u64 = 0;
            for i in 0..l.1 as usize {
                payload |= (*l.0.add(i) as u64) << (i * 8);
            }
            for i in 0..r.1 as usize {
                payload |= (*r.0.add(i) as u64) << ((l.1 as usize + i) * 8);
            }
            let len_bits = (total_blen as u64) << crate::value::SHORT_STRING_LEN_SHIFT;
            return f64::from_bits(crate::value::SHORT_STRING_TAG | len_bits | payload);
        }
    }

    // Heap path — allocate a StringHeader and memcpy. Decode both
    // operands' byte slices via `str_bytes_from_jsvalue` (already done
    // above) and write directly into the new header's payload region.
    let (ptr, data_ptr) = string_storage_alloc(total_blen);
    unsafe {
        // ASCII-fast utf16 length: count bytes < 0x80 in both slices in
        // one pass. Most concat results are pure ASCII (number formatting,
        // ID building, slug construction, etc.); falling back to the
        // full Grisu-style codepoint walk for non-ASCII keeps spec
        // compliance for the edge case.
        let l_slice = if !l.0.is_null() {
            std::slice::from_raw_parts(l.0, l.1 as usize)
        } else {
            &[]
        };
        let r_slice = if !r.0.is_null() {
            std::slice::from_raw_parts(r.0, r.1 as usize)
        } else {
            &[]
        };
        let utf16_len = if l_slice.is_ascii() && r_slice.is_ascii() {
            total_blen
        } else {
            let mut u16 = 0u32;
            if !l_slice.is_empty() {
                u16 += compute_utf16_len(l.0, l.1);
            }
            if !r_slice.is_empty() {
                u16 += compute_utf16_len(r.0, r.1);
            }
            u16
        };

        init_string_header(ptr, utf16_len, total_blen, total_blen, 0, 0);
        if !l_slice.is_empty() {
            ptr::copy_nonoverlapping(l.0, data_ptr, l.1 as usize);
        }
        if !r_slice.is_empty() {
            ptr::copy_nonoverlapping(r.0, data_ptr.add(l.1 as usize), r.1 as usize);
        }
        // NaN-box as STRING_TAG.
        f64::from_bits(crate::value::JSValue::string_ptr(ptr).bits())
    }
}

/// Concatenate two strings
///
/// v0.5.78x perf: consolidate the eight is_valid_string_ptr checks into
/// two (one per input) and read all per-input fields in a single unsafe
/// block. The compiler should CSE the calls but visible source-level
/// duplication makes the codegen path harder to follow and adds a
/// real per-call cost on hot paths (1M concats / 24 ms = 24 ns each).
#[no_mangle]
pub extern "C" fn js_string_concat(
    a: *const StringHeader,
    b: *const StringHeader,
) -> *mut StringHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let a_handle = scope.root_string_ptr(a);
    let b_handle = scope.root_string_ptr(b);

    // Snapshot all validity-gated reads from `a` in one pass. For invalid
    // pointers this stays at the zero-defaults so the rest of the function
    // sees a "behaves like an empty string" view.
    let a_valid = is_valid_string_ptr(a);
    let b_valid = is_valid_string_ptr(b);
    let (blen_a, u16len_a, flags_a) = if a_valid {
        unsafe { ((*a).byte_len, (*a).utf16_len, (*a).flags) }
    } else {
        (0, 0, 0)
    };
    let (blen_b, u16len_b, flags_b) = if b_valid {
        unsafe { ((*b).byte_len, (*b).utf16_len, (*b).flags) }
    } else {
        (0, 0, 0)
    };
    let total_blen = blen_a + blen_b;

    // Intern fast path: if result is short enough, check the intern table
    // before allocating. Repeated property-name concatenations like
    // "field_" + j return the existing interned pointer — zero allocation.
    if total_blen > 0 && total_blen <= INTERN_MAX_BYTE_LEN {
        unsafe {
            let hash = fnv1a_concat(a, blen_a, b, blen_b);
            let slot = (hash as usize) & INTERN_TABLE_MASK;
            let hit = with_intern_table(|table| {
                let entry = &(*table)[slot];
                if entry.string_ptr != 0 && entry.hash == hash {
                    let existing = entry.string_ptr as *const StringHeader;
                    if is_valid_string_ptr(existing)
                        && (*existing).byte_len == total_blen
                        && concat_content_matches(a, blen_a, b, blen_b, existing)
                    {
                        return Some(existing);
                    }
                }
                None
            });
            if let Some(existing) = hit {
                return existing as *mut StringHeader;
            }
        }
    }

    let (ptr, data_ptr) = string_storage_alloc(total_blen);
    let a = a_handle.get_raw_const_ptr::<StringHeader>();
    let b = b_handle.get_raw_const_ptr::<StringHeader>();

    unsafe {
        init_string_header(
            ptr,
            u16len_a + u16len_b,
            total_blen,
            total_blen,
            0,
            flags_a | flags_b,
        );

        if a_valid && blen_a > 0 {
            ptr::copy_nonoverlapping(string_data(a), data_ptr, blen_a as usize);
        }
        if b_valid && blen_b > 0 {
            ptr::copy_nonoverlapping(
                string_data(b),
                data_ptr.add(blen_a as usize),
                blen_b as usize,
            );
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
    let prefix_blen = if is_valid_string_ptr(prefix) {
        unsafe { (*prefix).byte_len }
    } else {
        0
    };
    let prefix_u16 = if is_valid_string_ptr(prefix) {
        unsafe { (*prefix).utf16_len }
    } else {
        0
    };

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
            if (0..=999_999_999).contains(&n) {
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
        let (ptr, data_ptr) = string_storage_alloc(total_blen as u32);

        unsafe {
            // Both prefix and number digits are ASCII, so utf16_len == byte_len for the number part
            let flags = if is_valid_string_ptr(prefix) {
                (*prefix).flags
            } else {
                0
            };
            init_string_header(
                ptr,
                prefix_u16 + num_len as u32,
                total_blen as u32,
                total_blen as u32,
                0,
                flags,
            );

            if is_valid_string_ptr(prefix) && prefix_blen > 0 {
                ptr::copy_nonoverlapping(string_data(prefix), data_ptr, prefix_blen as usize);
            }
            ptr::copy_nonoverlapping(
                num_buf.as_ptr(),
                data_ptr.add(prefix_blen as usize),
                num_len,
            );
        }

        return ptr;
    }

    // Slow path: non-number value — fall back to js_jsvalue_to_string + js_string_concat
    let value_str = crate::value::js_jsvalue_to_string(value);
    js_string_concat(prefix, value_str)
}

/// N-way string concatenation (v0.5.771).
///
/// Replaces a left-spine of `Binary { Add }` string-concat nodes with a
/// single allocation. Pre-fix `id + "," + name + "," + email + "," + score
/// + "," + ternary + ",2026-05-09"` lowers to nine nested `js_string_concat`
/// calls — each allocates a fresh StringHeader, copies the accumulating
/// prefix, then copies the next part. Total work is quadratic in the
/// number of parts: 9 allocs, ~225 bytes copied per row for the
/// `string_concat_csv` kernel.
///
/// This function does the entire chain in one pass:
///   1. Walk the parts, recording (data_ptr, byte_len) for strings and
///      formatting numbers into a small-int cache or per-part stack buffer.
///   2. Sum the byte lengths.
///   3. One arena allocation sized to the total.
///   4. Copy each part's bytes into the destination.
///
/// `parts` is an array of `n` NaN-boxed `f64` values. The codegen-side
/// fold in `Expr::Binary { Add }` flattens left-spines of string-typed
/// adds and emits this call instead of the pairwise chain.
///
/// Returns a fresh shared (refcount=0) StringHeader. Callers NaN-box
/// with STRING_TAG via the standard `nanbox_string_inline` helper.
#[no_mangle]
pub extern "C" fn js_string_concat_chain(parts: *const f64, n: i32) -> *mut StringHeader {
    // Cap the per-call part count. The codegen-side fold limits chains
    // to 32; in practice user code rarely exceeds 8-10 (CSV row, log
    // line, prompt template). The cap keeps the stack arrays bounded so
    // we don't risk stack overflow on a pathological 10k-element fold.
    const MAX_PARTS: usize = 32;
    let n = (n as usize).min(MAX_PARTS);
    if n == 0 {
        return crate::string::js_string_from_bytes(b"".as_ptr(), 0);
    }
    if parts.is_null() {
        return crate::string::js_string_from_bytes(b"".as_ptr(), 0);
    }

    // Per-part scratch buffer for number formatting. 32 bytes is enough
    // for any f64 string representation (max ~24 chars).
    let mut num_bufs: [[u8; 32]; MAX_PARTS] = [[0u8; 32]; MAX_PARTS];
    // For each part: (ptr, len, flags). ptr is either a pointer into
    // num_bufs[i] (numeric path) or null for a rooted string handle;
    // len is the byte count; flags carries STRING_FLAG_HAS_LONE_SURROGATES
    // if the part is a string with that flag set.
    let scope = crate::gc::RuntimeHandleScope::new();
    let mut piece_string_handles = [None; MAX_PARTS];
    let mut piece_ptrs: [*const u8; MAX_PARTS] = [std::ptr::null(); MAX_PARTS];
    let mut piece_lens: [u32; MAX_PARTS] = [0; MAX_PARTS];
    let mut piece_u16: [u32; MAX_PARTS] = [0; MAX_PARTS];
    let mut piece_flags: u32 = 0;
    let mut total_blen: u32 = 0;
    let mut total_u16: u32 = 0;

    // Slow-path string headers from js_jsvalue_to_string (need to keep
    // the StringHeader alive for the duration; arena strings stay live
    // since the GC won't run mid-FFI-call, and we won't trigger more
    // allocations between formatting and copying).
    for i in 0..n {
        let value = unsafe { *parts.add(i) };
        let bits = value.to_bits();
        let tag = bits >> 48;

        // STRING_TAG = 0x7FFF — heap string pointer in lower 48 bits.
        if tag == 0x7FFF {
            let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
            if is_valid_string_ptr(ptr) {
                let blen = unsafe { (*ptr).byte_len };
                let u16len = unsafe { (*ptr).utf16_len };
                let flags = unsafe { (*ptr).flags };
                if blen > 0 {
                    piece_string_handles[i] = Some(scope.root_string_ptr(ptr));
                    piece_lens[i] = blen;
                    piece_u16[i] = u16len;
                    piece_flags |= flags;
                    total_blen = total_blen.saturating_add(blen);
                    total_u16 = total_u16.saturating_add(u16len);
                }
                continue;
            }
        }

        // SHORT_STRING_TAG = 0x7FF9 — payload encoded inline. Materialize
        // through the slow path (rare in hot loops).
        if tag == 0x7FF9 {
            let s = crate::value::js_jsvalue_to_string(value);
            if is_valid_string_ptr(s) {
                let blen = unsafe { (*s).byte_len };
                let u16len = unsafe { (*s).utf16_len };
                let flags = unsafe { (*s).flags };
                if blen > 0 {
                    piece_string_handles[i] = Some(scope.root_string_ptr(s));
                    piece_lens[i] = blen;
                    piece_u16[i] = u16len;
                    piece_flags |= flags;
                    total_blen = total_blen.saturating_add(blen);
                    total_u16 = total_u16.saturating_add(u16len);
                }
            }
            continue;
        }

        // Plain f64 (no NaN-box tag in upper 16 bits). Format inline.
        let is_plain_f64 = tag < 0x7FF8 || (tag == 0x7FF8 && (bits & 0x000F_FFFF_FFFF_FFFF) == 0);
        if is_plain_f64 {
            let len = format_number_into(value, &mut num_bufs[i]);
            piece_ptrs[i] = num_bufs[i].as_ptr();
            piece_lens[i] = len as u32;
            piece_u16[i] = len as u32; // ASCII for all formatted numbers
            total_blen = total_blen.saturating_add(len as u32);
            total_u16 = total_u16.saturating_add(len as u32);
            continue;
        }

        // INT32_TAG = 0x7FFE — extract int from lower 32 bits.
        if tag == 0x7FFE {
            let v = (bits & 0xFFFF_FFFF) as u32 as i32;
            let len = if v >= 0 {
                fast_itoa_u32(v as u32, &mut num_bufs[i])
            } else {
                let s = format!("{}", v);
                let l = s.len().min(32);
                num_bufs[i][..l].copy_from_slice(&s.as_bytes()[..l]);
                l
            };
            piece_ptrs[i] = num_bufs[i].as_ptr();
            piece_lens[i] = len as u32;
            piece_u16[i] = len as u32;
            total_blen = total_blen.saturating_add(len as u32);
            total_u16 = total_u16.saturating_add(len as u32);
            continue;
        }

        // Anything else (bool, null, undefined, object, etc.) — slow path.
        let s = crate::value::js_jsvalue_to_string(value);
        if is_valid_string_ptr(s) {
            let blen = unsafe { (*s).byte_len };
            let u16len = unsafe { (*s).utf16_len };
            let flags = unsafe { (*s).flags };
            if blen > 0 {
                piece_string_handles[i] = Some(scope.root_string_ptr(s));
                piece_lens[i] = blen;
                piece_u16[i] = u16len;
                piece_flags |= flags;
                total_blen = total_blen.saturating_add(blen);
                total_u16 = total_u16.saturating_add(u16len);
            }
        }
    }

    // Single allocation for the entire result.
    let (ptr, mut cursor) = string_storage_alloc(total_blen);

    unsafe {
        init_string_header(ptr, total_u16, total_blen, total_blen, 0, piece_flags);
        for i in 0..n {
            let l = piece_lens[i] as usize;
            if l == 0 {
                continue;
            }
            if let Some(handle) = piece_string_handles[i] {
                let piece = handle.get_raw_const_ptr::<StringHeader>();
                if is_valid_string_ptr(piece) {
                    ptr::copy_nonoverlapping(string_data(piece), cursor, l);
                    cursor = cursor.add(l);
                }
            } else if !piece_ptrs[i].is_null() {
                ptr::copy_nonoverlapping(piece_ptrs[i], cursor, l);
                cursor = cursor.add(l);
            }
        }

        ptr
    }
}

/// Format an f64 into a 32-byte stack buffer using the fast paths from
/// `js_string_concat_value` / `js_value_concat_string`. Returns the number
/// of bytes written.
#[inline]
pub(crate) fn format_number_into(value: f64, buf: &mut [u8; 32]) -> usize {
    if value.fract() == 0.0 && value.abs() < 1e15 && !value.is_nan() && !value.is_infinite() {
        let n = value as i64;
        if (0..=999_999_999).contains(&n) {
            return fast_itoa_u32(n as u32, buf);
        }
        let s = format!("{}", n);
        let len = s.len().min(buf.len());
        buf[..len].copy_from_slice(&s.as_bytes()[..len]);
        return len;
    }
    if value.is_nan() {
        buf[..3].copy_from_slice(b"NaN");
        return 3;
    }
    if value.is_infinite() {
        if value > 0.0 {
            buf[..8].copy_from_slice(b"Infinity");
            return 8;
        }
        buf[..9].copy_from_slice(b"-Infinity");
        return 9;
    }
    if value == 0.0 {
        buf[0] = b'0';
        return 1;
    }
    let s = format!("{}", value);
    let len = s.len().min(buf.len());
    buf[..len].copy_from_slice(&s.as_bytes()[..len]);
    len
}

/// Fused value + string concatenation (value on the LEFT, string on the RIGHT).
/// Handles the `i + "_suffix"` pattern.
#[no_mangle]
pub extern "C" fn js_value_concat_string(
    value: f64,
    suffix: *const StringHeader,
) -> *mut StringHeader {
    let suffix_blen = if is_valid_string_ptr(suffix) {
        unsafe { (*suffix).byte_len }
    } else {
        0
    };
    let suffix_u16 = if is_valid_string_ptr(suffix) {
        unsafe { (*suffix).utf16_len }
    } else {
        0
    };

    let bits = value.to_bits();
    let tag = bits >> 48;
    let is_plain_f64 = tag < 0x7FF8 || (tag == 0x7FF8 && (bits & 0x000F_FFFF_FFFF_FFFF) == 0);

    if is_plain_f64 {
        let mut num_buf = [0u8; 32];
        let num_len: usize;

        if value.fract() == 0.0 && value.abs() < 1e15 && !value.is_nan() && !value.is_infinite() {
            let n = value as i64;
            if (0..=999_999_999).contains(&n) {
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
        let (ptr, data_ptr) = string_storage_alloc(total_blen as u32);

        unsafe {
            let flags = if is_valid_string_ptr(suffix) {
                (*suffix).flags
            } else {
                0
            };
            init_string_header(
                ptr,
                num_len as u32 + suffix_u16,
                total_blen as u32,
                total_blen as u32,
                0,
                flags,
            );

            ptr::copy_nonoverlapping(num_buf.as_ptr(), data_ptr, num_len);
            if is_valid_string_ptr(suffix) && suffix_blen > 0 {
                ptr::copy_nonoverlapping(
                    string_data(suffix),
                    data_ptr.add(num_len),
                    suffix_blen as usize,
                );
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
pub(crate) fn fast_itoa_u32(mut n: u32, buf: &mut [u8; 32]) -> usize {
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
