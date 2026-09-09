use super::*;

/// Unescaped JSON bytes remain valid throughout the parser's suppression
/// window. Escaped/WTF-8 builder output keeps its existing canonicalizer.
pub(crate) unsafe fn string_from_json_bytes(
    batch: &mut Option<crate::arena::ConstructionBatch>,
    bytes: &[u8],
) -> *mut StringHeader {
    let len = bytes.len() as u32;
    let utf16_len = if bytes.is_ascii() {
        len
    } else {
        compute_utf16_len(bytes.as_ptr(), len)
    };
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
