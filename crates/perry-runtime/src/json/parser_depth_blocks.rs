//! Allocation-free nesting preflight for little-endian ARM64.
//! Classify quoted spans and brackets together, without assuming valid JSON.
#[inline(always)]
fn byte(b: u8, depth: &mut usize, quoted: &mut bool, escaped: &mut bool, limit: usize) -> bool {
    if *quoted {
        if *escaped {
            *escaped = false;
        } else if b == b'\\' {
            *escaped = true;
        } else if b == b'"' {
            *quoted = false;
        }
    } else {
        match b {
            b'"' => *quoted = true,
            b'[' | b'{' => {
                *depth += 1;
                if *depth > limit {
                    return true;
                }
            }
            b']' | b'}' => *depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    false
}

#[inline(always)]
unsafe fn pack(mask: std::arch::aarch64::uint8x16_t) -> u64 {
    use std::arch::aarch64::*;
    vget_lane_u64::<0>(vreinterpret_u64_u8(vshrn_n_u16::<4>(vreinterpretq_u16_u8(
        mask,
    ))))
}

pub(crate) fn nesting_depth_exceeds(bytes: &[u8], limit: usize) -> bool {
    // No opening-container byte after the root proves depth <= 1 even when
    // the text contains quotes, escapes or malformed closing delimiters. A
    // bracket byte inside a string is only a conservative false positive.
    if bytes.len() >= 256 && matches!(bytes[0], b'[' | b'{') && limit > 0 {
        let body = &bytes[1..];
        if !contains_open_container(body) {
            return false;
        }
    }
    let (mut depth, mut pos) = (0usize, 0usize);
    let (mut quoted, mut escaped) = (false, false);
    unsafe {
        use std::arch::aarch64::*;
        const ONES: u64 = 0x1111_1111_1111_1111;
        while bytes.len() - pos >= 16 {
            // The bound covers the complete unaligned vector load.
            let chunk = vld1q_u8(bytes.as_ptr().add(pos));
            let quotes = pack(vceqq_u8(chunk, vdupq_n_u8(b'"')));
            let slashes = pack(vceqq_u8(chunk, vdupq_n_u8(b'\\')));
            if quoted && quotes | slashes == 0 {
                // The first window consumes a possible incoming escaped byte.
                // Its bytes are all ordinary, so searching from here is safe.
                let Some(end) = super::depth_string::quoted_end(&bytes[pos..]) else {
                    return false;
                };
                pos += end + 1;
                quoted = false;
                escaped = false;
                continue;
            }
            let (quotes, carry) = super::depth_string::unescaped(quotes, slashes, escaped);
            if quoted && quotes == 0 {
                escaped = carry;
                pos += 16;
                continue;
            }
            // Prefix XOR gives string state after each quote. Brackets and
            // backslashes cannot occupy a quote lane, so that is also their
            // state before the byte. Each byte uses the low bit of a nibble.
            let mut inside = quotes;
            inside ^= inside << 4;
            inside ^= inside << 8;
            inside ^= inside << 16;
            inside ^= inside << 32;
            if quoted {
                inside ^= ONES;
            }
            if slashes & ONES & !inside != 0 {
                // Outside a string, even an invalid backslash has no escape
                // semantics in the old preflight. Recompute this block from
                // its original state when the tentative global mask sees one.
                for &b in &bytes[pos..pos + 16] {
                    if byte(b, &mut depth, &mut quoted, &mut escaped, limit) {
                        return true;
                    }
                }
                pos += 16;
                continue;
            }
            let opens = pack(vorrq_u8(
                vceqq_u8(chunk, vdupq_n_u8(b'[')),
                vceqq_u8(chunk, vdupq_n_u8(b'{')),
            )) & ONES
                & !inside;
            let closes = pack(vorrq_u8(
                vceqq_u8(chunk, vdupq_n_u8(b']')),
                vceqq_u8(chunk, vdupq_n_u8(b'}')),
            )) & ONES
                & !inside;
            let nopen = opens.count_ones() as usize;
            let nclose = closes.count_ones() as usize;
            if depth >= nclose && depth + nopen <= limit {
                // No ordering can clamp at zero or exceed the limit here.
                depth = depth + nopen - nclose;
            } else {
                let mut brackets = opens | closes;
                while brackets != 0 {
                    let first = brackets & brackets.wrapping_neg();
                    brackets ^= first;
                    if opens & first != 0 {
                        depth += 1;
                        if depth > limit {
                            return true;
                        }
                    } else {
                        depth = depth.saturating_sub(1);
                    }
                }
            }
            quoted = inside >> 60 != 0;
            escaped = carry;
            pos += 16;
        }
    }
    for &b in &bytes[pos..] {
        if byte(b, &mut depth, &mut quoted, &mut escaped, limit) {
            return true;
        }
    }
    false
}

#[inline(always)]
fn contains_open_container(bytes: &[u8]) -> bool {
    use std::arch::aarch64::*;
    unsafe {
        // '[' and '{' differ only by bit 0x20. No other byte folds to '{'.
        let fold = vdupq_n_u8(0x20);
        let object = vdupq_n_u8(b'{');
        // Preserve early admission for arrays whose first record opens here.
        if bytes.len() >= 16 {
            let chunk = vld1q_u8(bytes.as_ptr());
            if vmaxvq_u8(vceqq_u8(vorrq_u8(chunk, fold), object)) != 0 {
                return true;
            }
            return contains_open_container_tail(&bytes[16..]);
        }
    }
    contains_open_container_tail(bytes)
}

// Keep the initial positive probe in the depth scanner. Its quote/depth loop
// does not need the wide no-opening search in its instruction footprint.
#[inline(never)]
fn contains_open_container_tail(bytes: &[u8]) -> bool {
    use std::arch::aarch64::*;
    let mut i = 0usize;
    unsafe {
        let fold = vdupq_n_u8(0x20);
        let object = vdupq_n_u8(b'{');
        while bytes.len() - i >= 64 {
            let p = bytes.as_ptr().add(i);
            let a = vceqq_u8(vorrq_u8(vld1q_u8(p), fold), object);
            let b = vceqq_u8(vorrq_u8(vld1q_u8(p.add(16)), fold), object);
            let c = vceqq_u8(vorrq_u8(vld1q_u8(p.add(32)), fold), object);
            let d = vceqq_u8(vorrq_u8(vld1q_u8(p.add(48)), fold), object);
            if vmaxvq_u8(vorrq_u8(vorrq_u8(a, b), vorrq_u8(c, d))) != 0 {
                return true;
            }
            i += 64;
        }
        while bytes.len() - i >= 16 {
            let chunk = vld1q_u8(bytes.as_ptr().add(i));
            if vmaxvq_u8(vceqq_u8(vorrq_u8(chunk, fold), object)) != 0 {
                return true;
            }
            i += 16;
        }
    }
    bytes[i..].iter().any(|&b| matches!(b, b'[' | b'{'))
}

#[cfg(test)]
mod tests {
    use super::contains_open_container;

    #[test]
    fn json_open_scan_matches_every_byte_and_vector_tail() {
        for len in [
            0, 1, 3, 7, 8, 15, 16, 17, 31, 32, 47, 48, 63, 64, 65, 79, 80, 81, 127, 128, 129, 255,
            256, 257,
        ] {
            for byte in 0..=255u8 {
                for offset in 0..16 {
                    // An accidental read outside the slice sees an opening.
                    let mut storage = vec![b'{'; len + 32];
                    let bytes = &mut storage[offset..offset + len];
                    bytes.fill(byte);
                    assert_eq!(
                        contains_open_container(bytes),
                        bytes.iter().any(|&b| matches!(b, b'[' | b'{')),
                        "len={len}, byte={byte}, offset={offset}"
                    );
                }
            }
        }
    }

    #[test]
    fn json_open_scan_finds_every_position_with_poisoned_surroundings() {
        for len in 0..260 {
            for offset in 0..16 {
                let mut storage = vec![b'['; len + 32];
                let bytes = &mut storage[offset..offset + len];
                bytes.fill(b'x');
                assert!(!contains_open_container(bytes));
                for position in 0..len {
                    for opening in [b'[', b'{'] {
                        bytes[position] = opening;
                        assert!(
                            contains_open_container(bytes),
                            "{len}, {offset}, {position}"
                        );
                    }
                    bytes[position] = b'x';
                }
            }
        }
    }
}
