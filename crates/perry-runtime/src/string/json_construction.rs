use super::*;

/// Construction context is specialized at compile time. The parser can expose
/// source metadata on the large-leaf path without loading it for every string.
pub(crate) trait JsonStringContext {
    fn json_string_batch(&mut self) -> &mut Option<crate::arena::ConstructionBatch>;

    unsafe fn json_string_utf16_len(&self, _bytes: &[u8]) -> Option<u32> {
        None
    }
}

impl JsonStringContext for Option<crate::arena::ConstructionBatch> {
    #[inline(always)]
    fn json_string_batch(&mut self) -> &mut Option<crate::arena::ConstructionBatch> {
        self
    }
}

/// Unescaped JSON bytes remain valid throughout the parser's suppression
/// window. Escaped/WTF-8 builder output keeps its existing canonicalizer.
#[inline(never)]
pub(crate) unsafe fn string_from_json_bytes<C: JsonStringContext>(
    context: &mut C,
    bytes: &[u8],
) -> *mut StringHeader {
    let len = bytes.len() as u32;
    if len >= JSON_MALLOC_OUTPUT_THRESHOLD {
        return string_from_json_large_bytes(context, bytes);
    }
    let utf16_len = if bytes.is_ascii() {
        len
    } else {
        compute_utf16_len(bytes.as_ptr(), len)
    };
    string_from_json_bytes_known_utf16(context.json_string_batch(), bytes, utf16_len)
}

#[inline(never)]
unsafe fn string_from_json_large_bytes<C: JsonStringContext>(
    context: &mut C,
    bytes: &[u8],
) -> *mut StringHeader {
    let utf16_len = context.json_string_utf16_len(bytes).unwrap_or_else(|| {
        if bytes.is_ascii() {
            bytes.len() as u32
        } else {
            compute_utf16_len(bytes.as_ptr(), bytes.len() as u32)
        }
    });
    string_from_json_bytes_known_utf16(context.json_string_batch(), bytes, utf16_len)
}

/// The parser has rooted `source` and suppressed collection. `start` names the
/// first decoded byte of an unescaped token in that source; no view is retained.
#[inline(never)]
pub(crate) unsafe fn json_source_token_utf16_len(
    bytes: &[u8],
    source: *const StringHeader,
    start: usize,
) -> Option<u32> {
    if source.is_null() {
        None
    } else {
        let input = std::slice::from_raw_parts(string_data(source), (*source).byte_len as usize);
        debug_assert_eq!(bytes.as_ptr(), input.as_ptr().add(start));
        source_token_utf16_len(input, (*source).utf16_len, start, bytes.len())
    }
}

fn source_token_utf16_len(
    input: &[u8],
    source_units: u32,
    start: usize,
    len: usize,
) -> Option<u32> {
    let end = start.checked_add(len)?;
    let token = input.get(start..end)?;
    let outside = input.len().checked_sub(len)?;
    // Only inspect surrounding syntax when it is small relative to the token.
    // The parser calls this helper only for individually allocated large leaves.
    if outside > len / 8 {
        return None;
    }
    // An incomplete WTF-8 lead can consume the closing quote in the source's
    // legacy length counter. Reject any lead that could cross the token end,
    // even when a preceding malformed lead might make it unreachable. Complete
    // UTF-8 sequences pass; this is a counting boundary proof, not validation.
    for (distance, minimum_lead) in [(1, 0xc0), (2, 0xe0), (3, 0xf0)] {
        if len >= distance && token[len - distance] >= minimum_lead {
            return None;
        }
    }
    if !input[..start].is_ascii() || !input[end..].is_ascii() {
        return None;
    }
    source_units.checked_sub(u32::try_from(outside).ok()?)
}

#[inline(always)]
unsafe fn string_from_json_bytes_known_utf16(
    batch: &mut Option<crate::arena::ConstructionBatch>,
    bytes: &[u8],
    utf16_len: u32,
) -> *mut StringHeader {
    let len = bytes.len() as u32;
    let size = std::mem::size_of::<StringHeader>() + bytes.len();
    let large_json_leaf = len >= JSON_MALLOC_OUTPUT_THRESHOLD;
    let raw = if large_json_leaf {
        std::ptr::null_mut()
    } else {
        batch.as_mut().map_or(std::ptr::null_mut(), |b| {
            b.try_alloc(size, crate::gc::GC_TYPE_STRING)
        })
    };
    let (header, data) = if raw.is_null() {
        if large_json_leaf {
            json_output_storage_alloc(len)
        } else {
            string_storage_alloc(len)
        }
    } else {
        zero_alignment_padding_tail(raw, size);
        let header = raw.cast::<StringHeader>();
        (header, string_data(header).cast_mut())
    };
    // `ParsedStr::Borrowed` reaches this constructor only when the JSON token
    // contained no backslash. JSON syntax itself excludes unescaped quote and
    // control bytes, so the decoded payload can be quoted again without an
    // escape scan.
    init_string_header(header, utf16_len, len, len, 0, STRING_FLAG_JSON_ESCAPE_FREE);
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
    if large_json_leaf {
        crate::json::note_completed_malloc_json_output(len);
    }
    header
}

#[cfg(test)]
#[path = "json_construction_tests.rs"]
mod tests;
