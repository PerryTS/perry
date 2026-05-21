//! In-place / fresh-allocation string append (`js_string_append`).

use super::*;

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

        // Mark as uniquely owned — the caller (codegen) is about to assign
        // this pointer to a single variable, so in-place append is safe next time.
        (*new_ptr).refcount = 1;

        new_ptr
    }
}
