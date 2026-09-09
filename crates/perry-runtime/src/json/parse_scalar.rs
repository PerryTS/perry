//! Scalar parsing that cannot allocate a managed value or invoke user code.
//!
//! A successful result contains no heap edge. The entry point can therefore
//! service pending collection work after decoding, with no input pointer left
//! to read and no temporary managed roots or suppressed allocation window.

use super::DirectParser;
use crate::value::{JSValue, SHORT_STRING_MAX_LEN};

#[inline(always)]
fn whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

#[inline(always)]
pub(super) fn try_parse_scalar(mut bytes: &[u8]) -> Option<JSValue> {
    // Containers are the common large-input case. Decline before scanning
    // trailing whitespace or constructing a parser.
    if matches!(bytes.first(), Some(b'{' | b'[')) {
        return None;
    }
    while bytes.first().copied().is_some_and(whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().copied().is_some_and(whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    match bytes {
        b"null" => Some(JSValue::null()),
        b"true" => Some(JSValue::bool(true)),
        b"false" => Some(JSValue::bool(false)),
        _ => match bytes.first()? {
            b'"' if (2..=SHORT_STRING_MAX_LEN + 2).contains(&bytes.len())
                && bytes.last() == Some(&b'"') =>
            {
                let text = &bytes[1..bytes.len() - 1];
                if text.iter().any(|&b| b < 0x20 || b == b'"' || b == b'\\')
                    || std::str::from_utf8(text).is_err()
                {
                    return None;
                }
                // Keep each constructor's length visible to the optimizer.
                // A dynamic packing loop grew a vector loop and a large
                // register-save frame even though the payload fits five bytes.
                // These fixed slices use the canonical SSO encoding and never
                // read beyond the validated payload.
                Some(match text {
                    [] => JSValue::short_string_unchecked(b""),
                    [a] => JSValue::short_string_unchecked(&[*a]),
                    [a, b] => JSValue::short_string_unchecked(&[*a, *b]),
                    [a, b, c] => JSValue::short_string_unchecked(&[*a, *b, *c]),
                    [a, b, c, d] => JSValue::short_string_unchecked(&[*a, *b, *c, *d]),
                    [a, b, c, d, e] => JSValue::short_string_unchecked(&[*a, *b, *c, *d, *e]),
                    _ => return None,
                })
            }
            b'-' | b'0'..=b'9' => parse_number(bytes),
            _ => None,
        },
    }
}

/// Preserve the top-level parser's cache bound even when the last producer
/// was lazy materialization or typed-key setup rather than another parse.
/// This reads only Rust-owned metadata and does not allocate a managed value.
#[inline(always)]
pub(super) fn clear_oversized_key_cache() {
    if super::PARSE_KEY_CACHE_OVERSIZED_THREADS.load(std::sync::atomic::Ordering::Relaxed) == 0 {
        return;
    }
    clear_oversized_key_cache_slow();
}

#[cold]
#[inline(never)]
fn clear_oversized_key_cache_slow() {
    if super::PARSE_KEY_CACHE_OVERSIZED.with(std::cell::Cell::get) {
        clear_key_cache();
    }
}

#[cold]
#[inline(never)]
pub(super) fn clear_key_cache() {
    super::PARSE_KEY_CACHE_OVERSIZED.with(|oversized| {
        if oversized.replace(false) {
            super::PARSE_KEY_CACHE_OVERSIZED_THREADS
                .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        }
    });
    super::PARSE_KEY_CACHE.with(|c| c.borrow_mut().clear());
    super::clear_parse_key_ring();
}

// Keep the numerical parser's larger stack frame out of the literal/string
// path. Use its existing grammar and rounding, including negative zero and
// exponent overflow/underflow. It creates only a numeric or undefined value.
#[inline(never)]
fn parse_number(bytes: &[u8]) -> Option<JSValue> {
    let mut parser = DirectParser::new(bytes);
    let value = unsafe { parser.parse_number() };
    parser.finish().then_some(value)
}

#[cfg(test)]
#[path = "parse_scalar_tests.rs"]
mod tests;
