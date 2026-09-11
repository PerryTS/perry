//! A bounded length proof for a large unescaped token in its original source.

use crate::StringHeader;

/// Choose the length-aware parser only when a dominant token may benefit.
/// Both specializations still validate the complete document.
#[inline]
pub(super) fn use_source_length_parser(input: &[u8]) -> bool {
    if input.len() <= 256 {
        false
    } else if input.len() <= 4096 {
        true
    } else {
        large_source_may_have_dominant_token(input)
    }
}

#[inline(never)]
fn large_source_may_have_dominant_token(input: &[u8]) -> bool {
    // A borrowed token leaving at most 256 surrounding bytes must open
    // before byte 256 and extend beyond byte 512 in a >4096-byte input.
    let prefix = &input[..512];
    let mut cursor = 0;
    let mut in_string = false;
    let mut unescaped = true;
    while let Some(hit) = super::super::simd::find_quote_or_backslash(&prefix[cursor..]) {
        let pos = cursor + hit;
        if prefix[pos] == b'"' {
            if in_string {
                in_string = false;
            } else {
                if pos >= 256 {
                    return false;
                }
                in_string = true;
                unescaped = true;
            }
            cursor = pos + 1;
        } else if in_string {
            unescaped = false;
            cursor = (pos + 2).min(prefix.len());
        } else {
            cursor = pos + 1;
        }
    }
    in_string && unescaped
}

/// Derive a token's UTF-16 length from its source metadata when all surrounding
/// bytes are ASCII. Syntax and escape validation remain in the ordinary parser.
///
/// # Safety
/// `source` must be null or a live, rooted StringHeader. No allocation or
/// safepoint may separate this proof from the caller's input borrow.
#[inline(never)]
pub(super) unsafe fn source_token_utf16_len(
    input: &[u8],
    source: *const StringHeader,
    token_start: usize,
    byte_len: usize,
) -> Option<u32> {
    if source.is_null() || byte_len < 256 || input.len().checked_sub(byte_len)? > 256 {
        return None;
    }
    let start = token_start.checked_add(1)?;
    let end = start.checked_add(byte_len)?;
    if end >= input.len() || input[token_start] != b'"' || input[end] != b'"' {
        return None;
    }
    // Standalone/tape parser slices need not be the full source payload.
    // A matching length alone is not an identity proof.
    if (*source).byte_len as usize != input.len()
        || crate::string::string_data(source) != input.as_ptr()
        || !input[..start].is_ascii()
        || !input[end..].is_ascii()
    {
        return None;
    }
    // Preserve the bounded WTF-8 walk even for malformed input bytes. A lead
    // that could claim a closing-quote/suffix byte makes subtraction unsafe.
    // A lead itself skipped by a preceding lead may cause a conservative miss.
    for distance in 1..=3.min(byte_len) {
        let byte = input[end - distance];
        let width = if byte >= 0xf0 {
            4
        } else if byte >= 0xe0 {
            3
        } else if byte >= 0xc0 {
            2
        } else {
            0
        };
        if width > distance {
            return None;
        }
    }
    (*source)
        .utf16_len
        .checked_sub((input.len() - byte_len) as u32)
}

/// Keep proof and large-leaf allocation out of the small-token parser frame.
/// Bytes must come from a borrowed value token after the surrogate-aware scanner.
#[inline(never)]
pub(super) unsafe fn string_from_dominant_token(
    batch: &mut Option<crate::arena::ConstructionBatch>,
    input: &[u8],
    source: *const StringHeader,
    token_start: usize,
    bytes: &[u8],
) -> *mut StringHeader {
    match source_token_utf16_len(input, source, token_start, bytes.len()) {
        Some(len) => crate::string::string_from_scanned_json_bytes_known_utf16(batch, bytes, len),
        None => crate::string::string_from_scanned_json_bytes(batch, bytes),
    }
}

#[cfg(test)]
#[path = "parser_source_length_tests.rs"]
mod tests;
