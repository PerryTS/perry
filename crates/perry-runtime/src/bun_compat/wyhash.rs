//! Wyhash (Zig `std.hash.Wyhash` flavor) — the algorithm behind `Bun.hash`.
//!
//! Bun's default hasher is Zig's Wyhash (wyhash "final 3" secret with Zig's
//! one-shot structure). Matching it bit-for-bit keeps `Bun.hash(...)
//! .toString(16)`-derived cache keys (e.g. opencode's skill-discovery cache
//! dirs) compatible between bun-run and perry-compiled installs. Vectors in
//! the tests below were produced by real Bun v1.3.12.

const SECRET: [u64; 4] = [
    0xa076_1d64_78bd_642f,
    0xe703_7ed1_a0b4_28db,
    0x8ebc_6af0_9c88_c6e3,
    0x5899_65cc_7537_4cc3,
];

#[inline]
fn mum(a: u64, b: u64) -> (u64, u64) {
    let x = (a as u128).wrapping_mul(b as u128);
    (x as u64, (x >> 64) as u64)
}

#[inline]
fn mix(a: u64, b: u64) -> u64 {
    let (a, b) = mum(a, b);
    a ^ b
}

#[inline]
fn read8(data: &[u8], i: usize) -> u64 {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&data[i..i + 8]);
    u64::from_le_bytes(buf)
}

#[inline]
fn read4(data: &[u8], i: usize) -> u64 {
    let mut buf = [0u8; 4];
    buf.copy_from_slice(&data[i..i + 4]);
    u32::from_le_bytes(buf) as u64
}

/// One-shot wyhash, port of Zig `std.hash.Wyhash.hash(seed, input)`.
pub fn wyhash(seed: u64, input: &[u8]) -> u64 {
    let len = input.len();
    let seed0 = seed ^ mix(seed ^ SECRET[0], SECRET[1]);
    let mut state = [seed0, seed0, seed0];
    let (mut a, mut b): (u64, u64);

    if len <= 16 {
        // smallKey
        if len >= 4 {
            let end = len - 4;
            let quarter = (len >> 3) << 2;
            a = (read4(input, 0) << 32) | read4(input, quarter);
            b = (read4(input, end) << 32) | read4(input, end - quarter);
        } else if len > 0 {
            a = ((input[0] as u64) << 16)
                | ((input[len >> 1] as u64) << 8)
                | (input[len - 1] as u64);
            b = 0;
        } else {
            a = 0;
            b = 0;
        }
    } else {
        let mut i: usize = 0;
        if len >= 48 {
            // 48-byte rounds; the exactly-aligned last block is left for the
            // tail loop (Zig's `while (i + 48 < len)` is strict).
            while i + 48 < len {
                for lane in 0..3 {
                    let x = read8(input, i + 8 * (2 * lane));
                    let y = read8(input, i + 8 * (2 * lane + 1));
                    state[lane] = mix(x ^ SECRET[lane + 1], y ^ state[lane]);
                }
                i += 48;
            }
            state[0] ^= state[1] ^ state[2];
        }
        // final1: 16-byte chunks, then the last 16 bytes of the WHOLE input
        // (deliberately overlapping the already-processed prefix).
        let mut j = i;
        while j + 16 < len {
            state[0] = mix(read8(input, j) ^ SECRET[1], read8(input, j + 8) ^ state[0]);
            j += 16;
        }
        a = read8(input, len - 16);
        b = read8(input, len - 8);
    }

    // final2
    a ^= SECRET[1];
    b ^= state[0];
    let (a2, b2) = mum(a, b);
    mix(a2 ^ SECRET[0] ^ (len as u64), b2 ^ SECRET[1])
}

#[cfg(test)]
mod tests {
    use super::wyhash;

    /// Bun-produced vectors (`Bun.hash(s).toString(16)` /
    /// `Bun.hash(s, 42).toString(16)`, Bun v1.3.12).
    #[test]
    fn matches_bun_hash_vectors() {
        let vectors: &[(&str, u64, u64)] = &[
            ("", 0x0409_638e_e2bd_e459, 0x7201_4e4e_ed7e_eb7d),
            ("a", 0x28d2_0533_09d2_8531, 0x9bbe_a1d2_4169_a57d),
            ("abc", 0x02a4_f1d7_cb51_6c72, 0x729d_41f0_62dc_5b37),
            ("hello world", 0x668d_5e43_1c3b_2573, 0x0e8a_b5bd_d736_12b2),
            (
                "The quick brown fox jumps over the lazy dog",
                0x6303_b3ba_de45_a571,
                0x4f0e_75ed_5d33_843d,
            ),
            (
                "skill-discovery-cache-key",
                0xf51c_b10d_1f69_d049,
                0xe87a_8333_a038_5cab,
            ),
            (
                "héllo wörld ünïcödé",
                0x5b4a_b9e3_ff37_51ee,
                0x5171_71bf_0afe_696d,
            ),
            (
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
                0xc47d_d9f7_a82c_cf9b,
                0xdf75_22e3_a985_e8a6,
            ),
        ];
        for (s, want0, want42) in vectors {
            assert_eq!(wyhash(0, s.as_bytes()), *want0, "seed 0 for {s:?}");
            assert_eq!(wyhash(42, s.as_bytes()), *want42, "seed 42 for {s:?}");
        }
        // Byte input (Bun.hash(new Uint8Array([1,2,3]))).
        assert_eq!(wyhash(0, &[1, 2, 3]), 0xc3e9_27b4_07f2_b4b3);
    }

    /// Branch-boundary lengths (smallKey / final1 / 48-byte rounds, incl. the
    /// exactly-48-aligned tail case). Vectors from Bun v1.3.12.
    #[test]
    fn matches_bun_hash_boundary_lengths() {
        let vectors: &[(usize, u64)] = &[
            (3, 0x4e1a_41f3_3146_f432),
            (4, 0x115f_256e_a86b_7d9a),
            (16, 0x0b7d_3179_8211_c0ed),
            (17, 0x7ba3_186d_c79f_5203),
            (47, 0x4d49_acfe_b563_d7a8),
            (48, 0x971f_3762_6c3a_fdce),
            (49, 0xf38f_5ae3_3546_2355),
            (96, 0xcc01_984f_3dfa_1ac2),
            (97, 0xf6ca_f671_d6ce_6be2),
            (1000, 0xb7fe_88ea_75c1_f19a),
        ];
        for (n, want) in vectors {
            let s = "x".repeat(*n);
            assert_eq!(wyhash(0, s.as_bytes()), *want, "len {n}");
        }
    }
}
