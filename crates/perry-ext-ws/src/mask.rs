//! SIMD-ish WebSocket frame (un)masking.
//!
//! Every client→server WebSocket frame is XOR-masked with a 4-byte
//! key (RFC 6455 §5.3): `payload[i] ^= key[i % 4]`. The naive form is
//! a per-byte scalar loop; `tungstenite` already widens this to 4-byte
//! (`u32`) blocks. This module widens it further to 8-byte (`u64`)
//! blocks built from a phase-rotated key, which is portable across
//! every target (no `target_feature`, no intrinsics, no nightly
//! `std::simd`) and lets the optimizer auto-vectorize the hot `u64`
//! loop into wide SIMD stores on platforms that have them.
//!
//! Mask and unmask are the same operation (XOR is its own inverse), so
//! one routine serves both directions.
//!
//! Correctness contract — the output is **byte-identical** to the
//! scalar reference [`apply_mask_scalar_from`] for any payload length,
//! any key, and any starting key phase. The wide path differs from the
//! scalar path only in *how* it computes the same XOR, never in *what*
//! it computes. [`apply_mask_from`] carries the key phase across calls,
//! so a payload delivered in multiple chunks unmasks identically to the
//! same payload delivered whole.

/// Scalar reference unmask starting at an arbitrary key phase
/// `key_offset` (`buf[i] ^= mask[(key_offset + i) & 3]`).
///
/// This is the obviously-correct fallback the wide path is validated
/// against. It is also used for the unaligned head/tail remainders.
#[inline]
fn apply_mask_scalar_from(buf: &mut [u8], mask: [u8; 4], key_offset: usize) {
    for (i, byte) in buf.iter_mut().enumerate() {
        *byte ^= mask[(key_offset + i) & 3];
    }
}

/// Rotate the 4-byte key so that byte 0 of the returned array is the
/// key byte that applies at `phase` (`phase` taken mod 4). Returned in
/// native-endian `u32` form ready to broadcast into a `u64`.
#[inline]
fn key_u64_at_phase(mask: [u8; 4], phase: usize) -> u64 {
    let mut k = [0u8; 4];
    for (i, slot) in k.iter_mut().enumerate() {
        *slot = mask[(phase + i) & 3];
    }
    let word = u32::from_ne_bytes(k) as u64;
    // Two identical 4-byte lanes packed into one 8-byte word: a `u64`
    // XOR then masks 8 payload bytes at the same phase per iteration.
    word | (word << 32)
}

/// Unmask `buf` in place with `mask`, treating `buf[0]` as key phase 0.
///
/// Vectorized: byte-identical to [`apply_mask_scalar_from`].
#[inline]
pub fn apply_mask(buf: &mut [u8], mask: [u8; 4]) {
    apply_mask_from(buf, mask, 0);
}

/// Unmask `buf` in place with `mask`, where `buf[0]` is at key phase
/// `key_offset` (i.e. `buf[i] ^= mask[(key_offset + i) & 3]`). Returns
/// the key phase (`0..=3`) the *next* contiguous byte would use, so a
/// frame split across multiple buffers can be unmasked chunk-by-chunk
/// while keeping the key phase continuous across chunk boundaries:
///
/// ```ignore
/// let mut phase = 0;
/// phase = apply_mask_from(chunk_a, mask, phase);
/// phase = apply_mask_from(chunk_b, mask, phase); // continues the key
/// ```
///
/// The result is identical to unmasking `chunk_a ++ chunk_b` as one
/// buffer. The return is always normalized to `0..=3` (any
/// `key_offset` is reduced mod 4 on entry), so feeding it straight
/// back never lets the phase cursor grow unbounded across many chunks.
#[inline]
pub fn apply_mask_from(buf: &mut [u8], mask: [u8; 4], key_offset: usize) -> usize {
    let len = buf.len();
    let phase = key_offset & 3;

    // The wide path needs a sufficiently long body to be worth the
    // alignment dance; tiny buffers go straight to the scalar loop.
    if len < 16 {
        apply_mask_scalar_from(buf, mask, phase);
        return (phase + len) & 3;
    }

    // Split into an unaligned head, a `u64`-aligned body, and a tail.
    // SAFETY: `align_to_mut` only reinterprets the already-owned `[u8]`
    // as `[u64]` for the aligned middle; the head/tail it leaves as
    // `[u8]`. No bytes outside `buf` are touched.
    let (head, words, tail) = unsafe { buf.align_to_mut::<u64>() };

    // Head: scalar, advancing the key phase.
    apply_mask_scalar_from(head, mask, phase);
    let body_phase = (phase + head.len()) & 3;

    // Body: one phase-rotated `u64` XOR per 8 bytes. Because the
    // aligned body begins at a multiple of 8 bytes from the head and
    // the key repeats every 4 bytes, every body word is at the SAME
    // phase (`body_phase`), so a single broadcast key word applies to
    // all of them.
    let key = key_u64_at_phase(mask, body_phase);
    for word in words.iter_mut() {
        *word ^= key;
    }

    // Tail: scalar, continuing the phase past the body.
    let tail_phase = (body_phase + words.len() * 8) & 3;
    apply_mask_scalar_from(tail, mask, tail_phase);

    (phase + len) & 3
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Independent oracle: recompute the masked bytes from scratch with
    /// the textbook `payload[i] ^= key[(offset + i) % 4]` definition.
    fn oracle(input: &[u8], mask: [u8; 4], key_offset: usize) -> Vec<u8> {
        input
            .iter()
            .enumerate()
            .map(|(i, &b)| b ^ mask[(key_offset + i) & 3])
            .collect()
    }

    /// Tiny xorshift PRNG — deterministic, no `rand` dependency.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
        fn byte(&mut self) -> u8 {
            self.next() as u8
        }
    }

    #[test]
    fn wide_matches_scalar_across_lengths_and_phases() {
        // Empty, sub-word, exactly-word, around the 16-byte wide
        // threshold, and a long body with non-aligned head/tail are all
        // covered by sweeping length 0..=80 against every key phase.
        let mask = [0x6d, 0xb6, 0xb2, 0x80];
        let base: Vec<u8> = (0..80u16).map(|i| (i as u8).wrapping_mul(31)).collect();

        for len in 0..=base.len() {
            for phase in 0..4 {
                let want = oracle(&base[..len], mask, phase);

                let mut got = base[..len].to_vec();
                let ret = apply_mask_from(&mut got, mask, phase);

                assert_eq!(got, want, "len={len} phase={phase}: wide != oracle");
                assert_eq!(
                    ret,
                    (phase + len) & 3,
                    "len={len} phase={phase}: bad next phase"
                );
            }
        }
    }

    #[test]
    fn empty_payload_is_a_noop() {
        let mut buf: [u8; 0] = [];
        assert_eq!(apply_mask_from(&mut buf, [1, 2, 3, 4], 2), 2);
    }

    /// The returned phase is always normalized to `0..=3`, never an
    /// unbounded `key_offset + len` cursor — both the scalar (`len<16`)
    /// and wide (`len>=16`) paths, and any large incoming `key_offset`.
    #[test]
    fn returned_phase_is_normalized() {
        let mask = [0x6d, 0xb6, 0xb2, 0x80];
        // Scalar path (len < 16) and wide path (len >= 16), large offset.
        for len in [0usize, 1, 7, 15, 16, 17, 100] {
            for key_offset in [0usize, 2, 5, 103, 4096] {
                let mut buf: Vec<u8> = (0..len as u16).map(|i| i as u8).collect();
                let ret = apply_mask_from(&mut buf, mask, key_offset);
                assert!(
                    ret <= 3,
                    "len={len} off={key_offset}: ret={ret} not in 0..=3"
                );
                assert_eq!(
                    ret,
                    (key_offset + len) & 3,
                    "len={len} off={key_offset}: ret not the normalized phase"
                );
            }
        }
    }

    #[test]
    fn xor_is_its_own_inverse() {
        let mask = [0xde, 0xad, 0xbe, 0xef];
        let original: Vec<u8> = (0..73u8).collect();
        let mut buf = original.clone();
        apply_mask(&mut buf, mask);
        assert_ne!(buf, original, "masking must change the payload");
        apply_mask(&mut buf, mask);
        assert_eq!(buf, original, "double-mask must restore the payload");
    }

    /// The key-phase carry across chunk boundaries is the property a
    /// naive "reset phase to 0 each chunk" implementation gets wrong.
    /// Unmasking a buffer split at every possible boundary must equal
    /// unmasking it whole.
    #[test]
    fn multi_chunk_carries_key_phase_across_boundaries() {
        let mask = [0x12, 0x34, 0x56, 0x78];
        let whole: Vec<u8> = (0..100u16).map(|i| (i as u8) ^ 0x5a).collect();

        let want = oracle(&whole, mask, 0);

        // Split at every index, including splits that land the body on
        // a non-multiple-of-4 phase (the regression a wrong carry hits).
        for split in 0..=whole.len() {
            let mut buf = whole.clone();
            let (a, b) = buf.split_at_mut(split);

            let phase = apply_mask_from(a, mask, 0);
            apply_mask_from(b, mask, phase);

            assert_eq!(buf, want, "split at {split}: chunked != whole");
        }
    }

    #[test]
    fn three_chunks_uneven_splits() {
        let mask = [0xa1, 0xb2, 0xc3, 0xd4];
        let whole: Vec<u8> = (0..130u16).map(|i| i as u8).collect();
        let want = oracle(&whole, mask, 0);

        // Deliberately uneven, non-4-aligned chunk sizes: 7, 19, rest.
        let mut buf = whole.clone();
        let (c0, rest) = buf.split_at_mut(7);
        let (c1, c2) = rest.split_at_mut(19);

        let mut phase = apply_mask_from(c0, mask, 0);
        phase = apply_mask_from(c1, mask, phase);
        apply_mask_from(c2, mask, phase);

        assert_eq!(buf, want, "uneven 3-chunk split != whole");
    }

    /// Property/fuzz-style sweep: random lengths, random masks, random
    /// key offsets, random chunk split points — the wide path must
    /// always equal the textbook oracle.
    #[test]
    fn fuzz_random_lengths_masks_offsets() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);

        for _ in 0..2_000 {
            let len = (rng.next() % 300) as usize;
            let mask = [rng.byte(), rng.byte(), rng.byte(), rng.byte()];
            let key_offset = (rng.next() % 4) as usize;

            let input: Vec<u8> = (0..len).map(|_| rng.byte()).collect();
            let want = oracle(&input, mask, key_offset);

            // Whole-buffer.
            let mut whole = input.clone();
            let ret = apply_mask_from(&mut whole, mask, key_offset);
            assert_eq!(
                whole, want,
                "whole: len={len} mask={mask:?} off={key_offset}"
            );
            assert_eq!(ret, (key_offset + len) & 3);

            // Two random chunks — phase must carry.
            if len > 0 {
                let split = (rng.next() as usize) % (len + 1);
                let mut chunked = input.clone();
                let (a, b) = chunked.split_at_mut(split);
                let p = apply_mask_from(a, mask, key_offset);
                apply_mask_from(b, mask, p);
                assert_eq!(
                    chunked, want,
                    "chunked: len={len} mask={mask:?} off={key_offset} split={split}"
                );
            }
        }
    }
}
