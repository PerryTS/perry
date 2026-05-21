use super::*;

// Helper functions for encoding/decoding

/// Hex char → 4-bit nibble; 16 means invalid. 256-entry lookup so the
/// hot path is a single load + branchless OR (avoids range-match codegen
/// that LLVM doesn't always fold to a table).
const HEX_DECODE_TABLE: [u8; 256] = {
    let mut t = [16u8; 256];
    let mut i = 0u8;
    while i < 10 {
        t[(b'0' + i) as usize] = i;
        i += 1;
    }
    let mut i = 0u8;
    while i < 6 {
        t[(b'a' + i) as usize] = 10 + i;
        t[(b'A' + i) as usize] = 10 + i;
        i += 1;
    }
    t
};

const BASE64_DECODE_TABLE: [u8; 256] = {
    let mut t = [64u8; 256];
    let mut i = 0u8;
    while i < 26 {
        t[(b'A' + i) as usize] = i;
        t[(b'a' + i) as usize] = i + 26;
        i += 1;
    }
    let mut i = 0u8;
    while i < 10 {
        t[(b'0' + i) as usize] = i + 52;
        i += 1;
    }
    t[b'+' as usize] = 62;
    t[b'/' as usize] = 63;
    // base64url variants (per js_buffer_from_string's encoding=2 arm)
    t[b'-' as usize] = 62;
    t[b'_' as usize] = 63;
    t
};

const BASE64_ENCODE_TABLE: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

const HEX_ENCODE_TABLE: &[u8; 16] = b"0123456789abcdef";

/// Hex-decode `input` into a freshly-allocated `BufferHeader`.
///
/// Pre-fix this went via `decode_hex(&[u8]) -> Vec<u8>` which `push`-ed each
/// byte (bounds + capacity check per push) and then `copy_nonoverlapping`'d
/// the whole vec into the buffer. The new path allocates at the worst-case
/// size up front, writes bytes directly, and adjusts `length` to the actual
/// decoded count — no Vec, no extra copy, no per-byte capacity arithmetic.
#[inline]
pub fn hex_decode_into_buffer(input: &[u8]) -> *mut BufferHeader {
    let max_out = input.len() / 2;
    let buf = buffer_alloc(max_out as u32);
    if max_out == 0 {
        unsafe {
            (*buf).length = 0;
        }
        return buf;
    }
    unsafe {
        let dst = buffer_data_mut(buf);
        let mut written = 0usize;
        let mut i = 0usize;
        let n = input.len() & !1; // pair count (drop trailing odd byte, matches Node)
        while i < n {
            let hi = HEX_DECODE_TABLE[input[i] as usize];
            let lo = HEX_DECODE_TABLE[input[i + 1] as usize];
            if hi < 16 && lo < 16 {
                *dst.add(written) = (hi << 4) | lo;
                written += 1;
            } else {
                // Node stops hex decoding at the first non-hex pair instead
                // of skipping over invalid bytes (e.g. "abxxcd" -> "ab").
                break;
            }
            i += 2;
        }
        (*buf).length = written as u32;
    }
    buf
}

/// Hex-encode `input` directly into a fresh `StringHeader`. Output is pure
/// ASCII (`0-9`, `a-f`) — allocates the StringHeader uninitialised and writes
/// bytes straight into its payload. Avoids the intermediate `Vec<u8>` +
/// `copy_nonoverlapping` round-trip.
#[inline]
pub fn hex_encode_into_string(input: &[u8]) -> *mut StringHeader {
    let out_len = input.len() * 2;
    if out_len == 0 {
        return js_string_from_ascii_bytes(std::ptr::null(), 0);
    }
    let (hdr, dst) = js_string_alloc_ascii_uninit(out_len as u32);
    let table = HEX_ENCODE_TABLE;
    unsafe {
        for (i, &b) in input.iter().enumerate() {
            // SAFETY: i*2+1 < out_len because allocation is exactly input.len()*2.
            *dst.add(i * 2) = *table.get_unchecked((b >> 4) as usize);
            *dst.add(i * 2 + 1) = *table.get_unchecked((b & 0xF) as usize);
        }
    }
    hdr
}

/// Base64-decode `input` directly into a freshly-allocated `BufferHeader`.
///
/// v0.5.772 perf: writes bytes directly into the `BufferHeader`'s data region
/// instead of routing through `Vec::push` (`decode_base64 -> Vec<u8>` then
/// `copy_nonoverlapping`).
///
/// v0.5.78x perf: 4-byte chunk fast path. Most encoded inputs are clean
/// (produced by an encoder, no whitespace, no invalid chars, optional
/// trailing `=` padding). For those we can decode 4 input bytes → 3
/// output bytes per iteration with all four table lookups happening in
/// parallel (no serial accum/bits state). On a 4 KB sample this drops
/// per-iteration cost from ~7 dependent ops down to 4 independent loads
/// + 1 OR + 3 writes. Falls back to the permissive byte-at-a-time loop
/// when we encounter `=`, an invalid byte, or a whitespace character —
/// matches Node's `Buffer.from(s, 'base64')` semantics for those cases
/// (skip invalid, stop at first `=`).
#[inline]
pub fn base64_decode_into_buffer(input: &[u8]) -> *mut BufferHeader {
    let max_out = input.len().saturating_mul(3) / 4 + 3;
    let buf = buffer_alloc(max_out as u32);
    if input.is_empty() {
        unsafe {
            (*buf).length = 0;
        }
        return buf;
    }
    unsafe {
        let dst = buffer_data_mut(buf);
        let mut written = 0usize;

        // Fast path: 4-byte chunks while all four bytes decode cleanly.
        // Bails to the slow-path tail when we hit `=`, an invalid byte,
        // or whitespace.
        let mut i = 0usize;
        let n = input.len();
        while i + 4 <= n {
            let b0 = *input.get_unchecked(i);
            let b1 = *input.get_unchecked(i + 1);
            let b2 = *input.get_unchecked(i + 2);
            let b3 = *input.get_unchecked(i + 3);
            // Bail to slow path if any chunk byte is special (= padding /
            // invalid / whitespace). We let the slow path read these.
            if b0 == b'=' || b1 == b'=' || b2 == b'=' || b3 == b'=' {
                break;
            }
            let v0 = *BASE64_DECODE_TABLE.get_unchecked(b0 as usize);
            let v1 = *BASE64_DECODE_TABLE.get_unchecked(b1 as usize);
            let v2 = *BASE64_DECODE_TABLE.get_unchecked(b2 as usize);
            let v3 = *BASE64_DECODE_TABLE.get_unchecked(b3 as usize);
            // 64 = invalid / skip char. Fall back to the slow path so
            // the permissive whitespace-skipping behavior fires correctly.
            if (v0 | v1 | v2 | v3) >= 64 {
                break;
            }
            let chunk =
                ((v0 as u32) << 18) | ((v1 as u32) << 12) | ((v2 as u32) << 6) | (v3 as u32);
            *dst.add(written) = (chunk >> 16) as u8;
            *dst.add(written + 1) = (chunk >> 8) as u8;
            *dst.add(written + 2) = chunk as u8;
            written += 3;
            i += 4;
        }

        // Slow-path tail: byte-at-a-time, accumulator-based, handles
        // padding, whitespace, and invalid characters per Node spec.
        let mut accum: u32 = 0;
        let mut bits: u32 = 0;
        while i < n {
            let byte = *input.get_unchecked(i);
            i += 1;
            if byte == b'=' {
                break;
            }
            let v = BASE64_DECODE_TABLE[byte as usize];
            if v == 64 {
                continue;
            }
            accum = (accum << 6) | v as u32;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                *dst.add(written) = (accum >> bits) as u8;
                written += 1;
                accum &= (1 << bits) - 1;
            }
        }
        (*buf).length = written as u32;
    }
    buf
}

/// Base64-encode `input` directly into a fresh `StringHeader`. Output is pure
/// ASCII so we skip the `compute_utf16_len` walk and write bytes directly into
/// the StringHeader's payload (no intermediate Vec, no follow-up
/// copy_nonoverlapping).
#[inline]
pub fn base64_encode_into_string(input: &[u8]) -> *mut StringHeader {
    let out_len = input.len().div_ceil(3) * 4;
    if out_len == 0 {
        return js_string_from_ascii_bytes(std::ptr::null(), 0);
    }
    let (hdr, dst) = js_string_alloc_ascii_uninit(out_len as u32);
    let table = BASE64_ENCODE_TABLE;
    unsafe {
        let mut i = 0usize;
        let mut o = 0usize;
        let triple_end = input.len() - input.len() % 3;
        while i < triple_end {
            let a = *input.get_unchecked(i) as u32;
            let b = *input.get_unchecked(i + 1) as u32;
            let c = *input.get_unchecked(i + 2) as u32;
            let n = (a << 16) | (b << 8) | c;
            *dst.add(o) = *table.get_unchecked((n >> 18) as usize);
            *dst.add(o + 1) = *table.get_unchecked(((n >> 12) & 0x3F) as usize);
            *dst.add(o + 2) = *table.get_unchecked(((n >> 6) & 0x3F) as usize);
            *dst.add(o + 3) = *table.get_unchecked((n & 0x3F) as usize);
            i += 3;
            o += 4;
        }
        let rem = input.len() - i;
        if rem == 1 {
            let a = *input.get_unchecked(i) as u32;
            let n = a << 16;
            *dst.add(o) = *table.get_unchecked((n >> 18) as usize);
            *dst.add(o + 1) = *table.get_unchecked(((n >> 12) & 0x3F) as usize);
            *dst.add(o + 2) = b'=';
            *dst.add(o + 3) = b'=';
        } else if rem == 2 {
            let a = *input.get_unchecked(i) as u32;
            let b = *input.get_unchecked(i + 1) as u32;
            let n = (a << 16) | (b << 8);
            *dst.add(o) = *table.get_unchecked((n >> 18) as usize);
            *dst.add(o + 1) = *table.get_unchecked(((n >> 12) & 0x3F) as usize);
            *dst.add(o + 2) = *table.get_unchecked(((n >> 6) & 0x3F) as usize);
            *dst.add(o + 3) = b'=';
        }
    }
    hdr
}

// Legacy Vec-returning helpers — kept for the unit tests at the bottom of this
// file and any out-of-tree callers. Hot transcode paths now go through the
// `*_into_buffer` / `*_into_string` variants above.
pub fn decode_hex(input: &[u8]) -> Vec<u8> {
    let buf = hex_decode_into_buffer(input);
    unsafe {
        let n = (*buf).length as usize;
        std::slice::from_raw_parts(buffer_data(buf), n).to_vec()
    }
}

#[cfg(test)]
pub fn encode_hex(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() * 2);
    let table = HEX_ENCODE_TABLE;
    for &b in input {
        out.push(table[(b >> 4) as usize]);
        out.push(table[(b & 0xF) as usize]);
    }
    out
}

pub fn decode_base64(input: &[u8]) -> Vec<u8> {
    let buf = base64_decode_into_buffer(input);
    unsafe {
        let n = (*buf).length as usize;
        std::slice::from_raw_parts(buffer_data(buf), n).to_vec()
    }
}

#[cfg(test)]
pub fn encode_base64(input: &[u8]) -> Vec<u8> {
    let out_len = input.len().div_ceil(3) * 4;
    let mut out = vec![0u8; out_len];
    let table = BASE64_ENCODE_TABLE;
    let mut i = 0usize;
    let mut o = 0usize;
    let triple_end = input.len() - input.len() % 3;
    while i < triple_end {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8) | (input[i + 2] as u32);
        out[o] = table[(n >> 18) as usize];
        out[o + 1] = table[((n >> 12) & 0x3F) as usize];
        out[o + 2] = table[((n >> 6) & 0x3F) as usize];
        out[o + 3] = table[(n & 0x3F) as usize];
        i += 3;
        o += 4;
    }
    let rem = input.len() - i;
    if rem == 1 {
        let n = (input[i] as u32) << 16;
        out[o] = table[(n >> 18) as usize];
        out[o + 1] = table[((n >> 12) & 0x3F) as usize];
        out[o + 2] = b'=';
        out[o + 3] = b'=';
    } else if rem == 2 {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8);
        out[o] = table[(n >> 18) as usize];
        out[o + 1] = table[((n >> 12) & 0x3F) as usize];
        out[o + 2] = table[((n >> 6) & 0x3F) as usize];
        out[o + 3] = b'=';
    }
    out
}
