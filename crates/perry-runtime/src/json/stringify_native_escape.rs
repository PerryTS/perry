//! Bounded escaping into an already reserved native JSON output buffer.

use std::arch::aarch64::*;

const SHORT_ESCAPE: [u8; 256] = {
    let mut codes = [0; 256];
    codes[b'"' as usize] = b'"';
    codes[b'\\' as usize] = b'\\';
    codes[b'\n' as usize] = b'n';
    codes[b'\r' as usize] = b'r';
    codes[b'\t' as usize] = b't';
    codes[8] = b'b';
    codes[12] = b'f';
    codes
};

#[inline(always)]
unsafe fn escape(byte: u8, output: *mut u8) -> usize {
    debug_assert!(byte < 32 || byte == b'"' || byte == b'\\');
    let code = SHORT_ESCAPE[byte as usize];
    if code != 0 {
        // GC_STORE_AUDIT(POINTER_FREE): Two JSON escape bytes.
        output
            .cast::<u16>()
            .write_unaligned(u16::from_ne_bytes([b'\\', code]));
        2
    } else {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        // GC_STORE_AUDIT(POINTER_FREE): JSON control-escape prefix.
        output
            .cast::<u32>()
            .write_unaligned(u32::from_ne_bytes(*b"\\u00"));
        // GC_STORE_AUDIT(POINTER_FREE): JSON control-escape hex digits.
        output
            .add(4)
            .cast::<u16>()
            .write_unaligned(u16::from_ne_bytes([
                HEX[(byte >> 4) as usize],
                HEX[(byte & 15) as usize],
            ]));
        6
    }
}

/// The existing expansion plan must validate the source and reserve the exact
/// quoted output size first. Source and output must not overlap, and neither
/// may move during this write. This routine allocates nothing and calls no GC.
#[inline(never)]
pub(super) unsafe fn write(source: &[u8], output: *mut u8) -> usize {
    // GC_STORE_AUDIT(POINTER_FREE): Opening JSON string delimiter.
    output.write(b'"');
    let mut pos = 0;
    let mut at = 1;
    while source.len() - pos >= 16 {
        let block = vld1q_u8(source.as_ptr().add(pos));
        let mask = vorrq_u8(
            vorrq_u8(
                vceqq_u8(block, vdupq_n_u8(b'"')),
                vceqq_u8(block, vdupq_n_u8(b'\\')),
            ),
            vcltq_u8(block, vdupq_n_u8(32)),
        );
        // Sixteen remaining input bytes imply at least sixteen remaining
        // output bytes. Any bytes after the first escape stay uncommitted and
        // are overwritten before the containing Vec's length is published.
        // GC_STORE_AUDIT(POINTER_FREE): Bounded native JSON payload copy.
        vst1q_u8(output.add(at), block);
        if vmaxvq_u8(mask) == 0 {
            pos += 16;
            at += 16;
            continue;
        }
        let words = vreinterpretq_u64_u8(mask);
        let low = u64::from_le(vgetq_lane_u64::<0>(words));
        let prefix = if low != 0 {
            low.trailing_zeros() as usize / 8
        } else {
            // The nonzero mask and empty low word prove a high-word hit.
            8 + u64::from_le(vgetq_lane_u64::<1>(words)).trailing_zeros() as usize / 8
        };
        pos += prefix;
        at += prefix;
        at += escape(source.as_ptr().add(pos).read(), output.add(at));
        pos += 1;
    }
    for &byte in &source[pos..] {
        if byte >= 32 && byte != b'"' && byte != b'\\' {
            // GC_STORE_AUDIT(POINTER_FREE): Final JSON payload bytes.
            output.add(at).write(byte);
            at += 1;
        } else {
            at += escape(byte, output.add(at));
        }
    }
    // GC_STORE_AUDIT(POINTER_FREE): Closing JSON string delimiter.
    output.add(at).write(b'"');
    at + 1
}
