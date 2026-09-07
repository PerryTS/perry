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
    let raw = batch.as_mut().map_or(std::ptr::null_mut(), |b| {
        b.try_alloc(size, crate::gc::GC_TYPE_STRING)
    });
    let (header, data) = if raw.is_null() {
        string_storage_alloc(len)
    } else {
        zero_alignment_padding_tail(raw, size);
        (raw.cast(), raw.add(std::mem::size_of::<StringHeader>()))
    };
    init_string_header(header, utf16_len, len, len, 0, 0);
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
    header
}
