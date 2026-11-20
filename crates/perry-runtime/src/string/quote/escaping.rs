//! JSON-compatible string literal escaping, shared with error descriptions.
#[inline]
pub(crate) unsafe fn write_escaped_string(buf: &mut String, s: &str) {
    let bytes = s.as_bytes();
    // Fast path: scan for any escape-triggering byte. JSON output is
    // overwhelmingly escape-free (ASCII identifiers, simple values), so
    // a straight-line SIMD-friendly scan + one `push_str` beats the
    // scalar per-byte escape loop. Needs_escape fires for `"`, `\`, or
    // any control byte (< 0x20).
    // Also trip the slow path for WTF-8 lone surrogate sequences
    // (issue #1182): a lead byte of 0xED followed by 0xA0..=0xBF means
    // we have a 3-byte encoding of U+D800..=U+DFFF and need to emit a
    // `\uXXXX` escape rather than the raw (invalid-UTF-8) bytes.
    let first_escape = super::simd::find_string_escape(bytes);
    let Some(first_escape) = first_escape else {
        buf.reserve(bytes.len() + 2);
        buf.push('"');
        super::stringify_copy::push_str(buf, s);
        buf.push('"');
        return;
    };

    write_escaped_bytes_from(buf, bytes, first_escape);
}

/// Quote a key rejected by the UTF-8 key decoder. Validate its WTF-8 first so
/// malformed internal bytes cannot enter the Rust output String unchanged.
#[inline(never)]
pub(crate) unsafe fn write_wtf8_key(buf: &mut String, bytes: &[u8]) -> bool {
    let mut rest = bytes;
    while let Err(error) = std::str::from_utf8(rest) {
        rest = &rest[error.valid_up_to()..];
        if rest.len() < 3
            || rest[0] != 0xed
            || !(0xa0..=0xbf).contains(&rest[1])
            || !(0x80..=0xbf).contains(&rest[2])
        {
            return false;
        }
        rest = &rest[3..];
    }
    let Some(first_escape) = super::simd::find_string_escape(bytes) else {
        return false;
    };
    write_escaped_bytes_from(buf, bytes, first_escape);
    true
}

#[test]
fn json_wtf8_key_fallback_validates_before_writing() {
    for bytes in [b"\xed\xa0".as_slice(), b"\xff", b"\xed\xa0\x80\xff"] {
        let mut output = "prefix".to_owned();
        assert!(!unsafe { write_wtf8_key(&mut output, bytes) });
        assert_eq!(output, "prefix");
    }
}

#[inline]
unsafe fn write_escaped_bytes_from(buf: &mut String, bytes: &[u8], first_escape: usize) {
    buf.push('"');
    let mut start = 0;
    // Issue #548: `s` reaches us via `str_from_header`, which uses
    // `from_utf8_unchecked` — a misclassified pointer (e.g. an
    // ArrayHeader interpreted as a StringHeader through the GC-type
    // fallback heuristic) can produce a `&str` whose bytes are not
    // valid UTF-8. The original `&s[start..i]` slice operation
    // panics in `core::str::is_char_boundary` whenever `i` lands
    // mid-multibyte (or on a stray continuation byte). Switching to
    // byte-level `extend_from_slice` writes the raw bytes through and
    // never inspects char boundaries; the JSON output stays
    // byte-identical for valid UTF-8 inputs and degrades gracefully
    // (non-UTF-8 bytes pass through verbatim) instead of aborting the
    // whole process. The String we hand back is technically
    // ill-formed in the worst case, but every consumer in this
    // codebase treats stringify output as a byte stream — and an
    // ill-formed result is strictly preferable to a SIGABRT.
    let buf_vec = buf.as_mut_vec();
    let mut i = first_escape;
    while i < bytes.len() {
        let b = bytes[i];
        // WTF-8 surrogate handling (issue #1182). A 0xED 0xA0..=0xBF
        // 0x80..=0xBF triple encodes a code unit in U+D800..=U+DFFF.
        // Two cases mirror Node's JSON.stringify:
        //
        //   * A high surrogate (0xA0..=0xAF mid byte) immediately
        //     followed by a low surrogate (0xB0..=0xBF mid byte) is
        //     a *valid* UTF-16 pair — re-encode it as the 4-byte
        //     UTF-8 of the astral codepoint, no escape. This is the
        //     only way `'\uD83D' + '\uDC4D'` round-trips through
        //     `dec.end()` + concat as 👍 instead of two escapes.
        //
        //   * Any remaining (lone) surrogate triple emits the
        //     `\uXXXX` escape.
        if b == 0xED
            && i + 2 < bytes.len()
            && (0xA0..=0xBF).contains(&bytes[i + 1])
            && (0x80..=0xBF).contains(&bytes[i + 2])
        {
            let high_cu: u32 = (((b & 0x0F) as u32) << 12)
                | (((bytes[i + 1] & 0x3F) as u32) << 6)
                | ((bytes[i + 2] & 0x3F) as u32);
            // Valid pair? Need the next 3 bytes to be a low surrogate
            // (0xED 0xB0..=0xBF 0x80..=0xBF) directly adjacent.
            let pair_low = if (0xD800..=0xDBFF).contains(&high_cu)
                && i + 5 < bytes.len()
                && bytes[i + 3] == 0xED
                && (0xB0..=0xBF).contains(&bytes[i + 4])
                && (0x80..=0xBF).contains(&bytes[i + 5])
            {
                let low_cu: u32 = (((bytes[i + 3] & 0x0F) as u32) << 12)
                    | (((bytes[i + 4] & 0x3F) as u32) << 6)
                    | ((bytes[i + 5] & 0x3F) as u32);
                Some(low_cu)
            } else {
                None
            };
            if start < i {
                buf_vec.extend_from_slice(&bytes[start..i]);
            }
            if let Some(low_cu) = pair_low {
                let cp = 0x10000 + ((high_cu - 0xD800) << 10) + (low_cu - 0xDC00);
                if let Some(c) = char::from_u32(cp) {
                    let mut tmp = [0u8; 4];
                    let s = c.encode_utf8(&mut tmp);
                    buf_vec.extend_from_slice(s.as_bytes());
                    i += 6;
                    start = i;
                    continue;
                }
            }
            append_code_unit_escape(buf_vec, high_cu as u16);
            i += 3;
            start = i;
            continue;
        }
        let escape = match b {
            b'"' => Some("\\\""),
            b'\\' => Some("\\\\"),
            b'\n' => Some("\\n"),
            b'\r' => Some("\\r"),
            b'\t' => Some("\\t"),
            0x08 => Some("\\b"),
            0x0c => Some("\\f"),
            0..=0x1f => {
                if start < i {
                    buf_vec.extend_from_slice(&bytes[start..i]);
                }
                append_code_unit_escape(buf_vec, b as u16);
                start = i + 1;
                i += 1;
                continue;
            }
            _ => None,
        };
        if let Some(esc) = escape {
            if start < i {
                buf_vec.extend_from_slice(&bytes[start..i]);
            }
            buf_vec.extend_from_slice(esc.as_bytes());
            start = i + 1;
        }
        i += 1;
    }
    if start < bytes.len() {
        buf_vec.extend_from_slice(&bytes[start..]);
    }
    buf_vec.push(b'"');
}

#[inline]
fn append_code_unit_escape(buf: &mut Vec<u8>, unit: u16) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    buf.extend_from_slice(&[
        b'\\',
        b'u',
        HEX[(unit >> 12) as usize],
        HEX[((unit >> 8) & 15) as usize],
        HEX[((unit >> 4) & 15) as usize],
        HEX[(unit & 15) as usize],
    ]);
}
