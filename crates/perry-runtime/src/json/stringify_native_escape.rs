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

// Each low input lane contributes its payload byte and, when escaped,
// its escape code. The lookup removes unused second bytes from eight pairs.
const PACK: [[u8; 16]; 256] = {
    let mut rows = [[255; 16]; 256];
    let mut mask = 0;
    while mask < 256 {
        let mut lane = 0;
        let mut at = 0;
        while lane < 8 {
            rows[mask][at] = (lane * 2) as u8;
            at += 1;
            if mask & (1 << lane) != 0 {
                rows[mask][at] = (lane * 2 + 1) as u8;
                at += 1;
            }
            lane += 1;
        }
        mask += 1;
    }
    rows
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
        if vmaxvq_u8(mask) == 0 {
            // GC_STORE_AUDIT(POINTER_FREE): Bounded native JSON payload copy.
            vst1q_u8(output.add(at), block);
            pos += 16;
            at += 16;
            continue;
        }
        let controls = vqtbl2q_u8(
            uint8x16x2_t(
                vld1q_u8(SHORT_ESCAPE.as_ptr()),
                vld1q_u8(SHORT_ESCAPE.as_ptr().add(16)),
            ),
            block,
        );
        let rare = vandq_u8(
            vcltq_u8(block, vdupq_n_u8(32)),
            vceqq_u8(controls, vdupq_n_u8(0)),
        );
        if vmaxv_u8(vget_low_u8(rare)) == 0 {
            let punctuation = vorrq_u8(
                vceqq_u8(block, vdupq_n_u8(b'"')),
                vceqq_u8(block, vdupq_n_u8(b'\\')),
            );
            let codes = vorrq_u8(controls, vandq_u8(punctuation, block));
            let leading = vbslq_u8(mask, vdupq_n_u8(b'\\'), block);
            let pairs = vzip1q_u8(leading, codes);
            let weights = vld1_u8([1, 2, 4, 8, 16, 32, 64, 128].as_ptr());
            let bits = vaddv_u8(vand_u8(vget_low_u8(mask), weights));
            let packed = vqtbl1q_u8(pairs, vld1q_u8(PACK[bits as usize].as_ptr()));
            // Sixteen remaining source bytes prove this full speculative
            // store fits. Only the first eight lanes' output is committed.
            // GC_STORE_AUDIT(POINTER_FREE): Bounded packed JSON payload copy.
            vst1q_u8(output.add(at), packed);
            at += 8 + bits.count_ones() as usize;
            pos += 8;
            continue;
        }
        // GC_STORE_AUDIT(POINTER_FREE): Bounded prefix before a rare control.
        vst1q_u8(output.add(at), block);
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
