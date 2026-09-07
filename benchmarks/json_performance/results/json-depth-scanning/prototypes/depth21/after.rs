pub(crate) fn nesting_depth_exceeds(bytes: &[u8], limit: usize) -> bool {
    // A flat scalar array cannot exceed depth one. Slice byte searches skip
    // the byte-by-byte state machine for large numeric/boolean/null arrays.
    // Test for an object first so record arrays reject this hint immediately.
    // Quotes or another opening container always fall back to the full scan;
    // this is only a depth proof, never a substitute for syntax validation.
    if bytes.len() >= 256 && bytes[0] == b'[' && limit > 0 {
        let body = &bytes[1..];
        if !body.contains(&b'{') && !body.contains(&b'[') && !body.contains(&b'"') {
            return false;
        }
    }
    let mut depth = 0usize;
    let mut pos = 0usize;
    while pos < bytes.len() {
        match bytes[pos] {
            b'"' => {
                pos += 1;
                let start = pos;
                // A quoted span can be megabytes long. Skip ordinary bytes in
                // bulk while retaining the preflight's handling of malformed
                // input: only quotes/backslashes change string state here.
                while pos < bytes.len() {
                    let Some(offset) = crate::qscan::find_quote_or_backslash(&bytes[pos..]) else {
                        return false;
                    };
                    pos += offset;
                    if bytes[pos] == b'"' {
                        break;
                    }
                    // Skip the backslash and its escaped byte, including an
                    // escaped quote/backslash. A trailing escape ends the scan;
                    // the real parser remains responsible for syntax errors.
                    pos = (pos + 2).min(bytes.len());
                    if pos - start >= 128 {
                        let Some(end) = crate::scan::quoted_end(&bytes[pos..]) else {
                            return false;
                        };
                        pos += end;
                        break;
                    }
                }
            }
            b'[' | b'{' => {
                depth += 1;
                if depth > limit {
                    return true;
                }
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        pos += 1;
    }
    false
}

