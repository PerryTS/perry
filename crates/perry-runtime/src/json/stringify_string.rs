//! Quote an escape-free heap string directly into its final managed result.
//! The source is the only root needed across the output allocation. There is
//! no intermediate JSON buffer and no second UTF-16 length scan.

use crate::string::{init_string_header, string_data, string_storage_alloc, StringHeader};
use crate::value::{POINTER_MASK, STRING_TAG, TAG_MASK};

/// Only call when replacer/spacer processing cannot run user code.
#[inline]
pub(super) unsafe fn try_heap_string(bits: u64) -> Option<*mut StringHeader> {
    if bits & TAG_MASK != STRING_TAG {
        return None;
    }
    let source = (bits & POINTER_MASK) as *const StringHeader;
    if (source as usize) < 0x1000 {
        return None;
    }
    // Small inputs already have cheap paths. Keep this scan and the rooted
    // output allocation out of their instruction stream.
    if (*source).byte_len < 64 {
        return None;
    }
    quote_heap_string(source)
}

#[inline(never)]
unsafe fn quote_heap_string(source: *const StringHeader) -> Option<*mut StringHeader> {
    let len = (*source).byte_len;
    let output_len = len.checked_add(2)?;
    let output_units = (*source).utf16_len.checked_add(2)?;
    let bytes = std::slice::from_raw_parts(string_data(source), len as usize);
    if super::simd::find_string_escape(bytes).is_some() {
        return super::stringify_escaped_output::quote(source);
    }
    if has_incomplete_tail(bytes) {
        return None;
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let input = scope.root_string_ptr(source);
    let (result, output) = string_storage_alloc(output_len);
    // Only scalar lengths survive the allocation. Re-read the source through
    // its handle so a moving collection cannot invalidate the copy address.
    let source = input.get_raw_const_ptr::<StringHeader>();
    init_string_header(result, output_units, output_len, output_len, 0, 0);
    output.write(b'"');
    std::ptr::copy_nonoverlapping(string_data(source), output.add(1), len as usize);
    output.add(output_len as usize - 1).write(b'"');
    Some(result)
}

/// Raw internal strings can contain malformed UTF-8. The existing fallback
/// counter advances by a lead byte's declared width even for a truncated
/// sequence. Appending a quote could put it inside that last step, making
/// input.length + 2 incorrect. Decline such tails using at most three bytes.
/// Other malformed bytes retain their existing byte/count interpretation.
#[inline]
pub(super) fn has_incomplete_tail(bytes: &[u8]) -> bool {
    for remaining in 1..=bytes.len().min(3) {
        let b = bytes[bytes.len() - remaining];
        if (b >= 0xc0 && b < 0xe0 && remaining < 2)
            || (b >= 0xe0 && b < 0xf0 && remaining < 3)
            || (b >= 0xf0 && remaining < 4)
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
#[path = "stringify_string_tests.rs"]
mod tests;
