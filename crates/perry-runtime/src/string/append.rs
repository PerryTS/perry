//! In-place / fresh-allocation string append (`js_string_append`).

use super::*;

/// True if the final WTF-8 unit of `bytes` is a lone HIGH surrogate
/// (`ED A0..AF ..`), matching the encoding `canonicalize_surrogate_pairs` uses.
/// `0xED` is always a 3-byte lead (never a continuation), so `bytes[n-3] == 0xED`
/// means the last code unit starts there.
#[inline]
fn ends_with_lone_high_surrogate(bytes: &[u8]) -> bool {
    let n = bytes.len();
    n >= 3 && bytes[n - 3] == 0xED && (0xA0..=0xAF).contains(&bytes[n - 2])
}

/// True if the first WTF-8 unit of `bytes` is a lone LOW surrogate (`ED B0..BF ..`).
#[inline]
fn starts_with_lone_low_surrogate(bytes: &[u8]) -> bool {
    bytes.len() >= 3 && bytes[0] == 0xED && (0xB0..=0xBF).contains(&bytes[1])
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
pub extern "C" fn js_string_append(
    dest: *mut StringHeader,
    src: *const StringHeader,
) -> *mut StringHeader {
    if !is_valid_string_ptr(dest as *const StringHeader) {
        // If dest is invalid, just duplicate src
        if !is_valid_string_ptr(src) {
            return js_string_from_bytes(ptr::null(), 0);
        }
        let scope = crate::gc::RuntimeHandleScope::new();
        let src_handle = scope.root_string_ptr(src);
        let src_blen = unsafe { (*src).byte_len };
        let new_ptr = js_string_from_bytes_with_capacity(ptr::null(), 0, src_blen);
        let src = src_handle.get_raw_const_ptr::<StringHeader>();
        if is_valid_string_ptr(src) {
            unsafe {
                let src_data = string_data(src);
                let new_data = (new_ptr as *mut u8).add(std::mem::size_of::<StringHeader>());
                ptr::copy_nonoverlapping(src_data, new_data, src_blen as usize);
                (*new_ptr).byte_len = src_blen;
                (*new_ptr).utf16_len = (*src).utf16_len;
                // Preserve the lone-surrogate flag on the duplicate so later
                // concats/appends still canonicalize correctly. (#6728)
                (*new_ptr).flags |= (*src).flags & STRING_FLAG_HAS_LONE_SURROGATES;
            }
        }
        return new_ptr;
    }

    if !is_valid_string_ptr(src) {
        return dest;
    }

    // Self-append (s += s): must allocate fresh to avoid reading from
    // memory that is being written to.
    if std::ptr::eq(dest, src) {
        return js_string_concat(dest as *const StringHeader, src);
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let dest_handle = scope.root_string_ptr(dest as *const StringHeader);
    let src_handle = scope.root_string_ptr(src);

    unsafe {
        let dest_blen = (*dest).byte_len;
        let src_blen = (*src).byte_len;

        if src_blen == 0 {
            return dest;
        }

        let new_blen = dest_blen + src_blen;

        // A high→low surrogate pair can only newly form at the dest|src join
        // (both operands are already canonical), so detect it in O(1) from the
        // boundary bytes: ordinary appends never pay for a whole-string scan;
        // only an actual straddling pair triggers canonicalization below. The
        // `+=` path previously skipped this entirely, so `s += hi; s += lo`
        // kept two lone 3-byte WTF-8 surrogates instead of the astral char's
        // 4-byte UTF-8 (unlike expression `hi + lo`, which canonicalizes). That
        // corrupted every emoji built up code-unit-by-code-unit. (#6728)
        let flag_bits = ((*dest).flags | (*src).flags) & STRING_FLAG_HAS_LONE_SURROGATES;
        let boundary_pair = {
            let d = std::slice::from_raw_parts(
                (dest as *const u8).add(std::mem::size_of::<StringHeader>()),
                dest_blen as usize,
            );
            let s = std::slice::from_raw_parts(string_data(src), src_blen as usize);
            ends_with_lone_high_surrogate(d) && starts_with_lone_low_surrogate(s)
        };

        // In-place append optimization: if dest is uniquely owned (refcount==1)
        // and has enough capacity, append directly without allocation.
        // This turns O(n^2) string building loops into amortized O(n).
        if (*dest).refcount == 1 && new_blen <= (*dest).capacity {
            let dest_data = (dest as *mut u8).add(std::mem::size_of::<StringHeader>());
            let src_data_ptr = string_data(src);
            ptr::copy_nonoverlapping(
                src_data_ptr,
                dest_data.add(dest_blen as usize),
                src_blen as usize,
            );
            (*dest).byte_len = new_blen;
            (*dest).utf16_len += (*src).utf16_len;
            (*dest).flags |= flag_bits;
            return if boundary_pair {
                // Merge the straddling pair (usually returns a new, smaller
                // string; rare, so the in-place win still holds in general).
                super::concat::canonicalize_surrogate_pairs(dest)
            } else {
                dest // Same pointer, no allocation!
            };
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
        let dest = dest_handle.get_raw_mut_ptr::<StringHeader>();
        let src = src_handle.get_raw_const_ptr::<StringHeader>();

        // Copy old dest content
        let new_data = (new_ptr as *mut u8).add(std::mem::size_of::<StringHeader>());
        let dest_data = (dest as *const u8).add(std::mem::size_of::<StringHeader>());
        ptr::copy_nonoverlapping(dest_data, new_data, dest_blen as usize);

        // Copy src content after dest content
        let src_data_ptr = string_data(src);
        ptr::copy_nonoverlapping(
            src_data_ptr,
            new_data.add(dest_blen as usize),
            src_blen as usize,
        );
        (*new_ptr).byte_len = new_blen;
        (*new_ptr).utf16_len = (*dest).utf16_len + (*src).utf16_len;
        (*new_ptr).flags |= flag_bits;

        // Mark as uniquely owned — the caller (codegen) is about to assign
        // this pointer to a single variable, so in-place append is safe next time.
        (*new_ptr).refcount = 1;

        if boundary_pair {
            super::concat::canonicalize_surrogate_pairs(new_ptr)
        } else {
            new_ptr
        }
    }
}
