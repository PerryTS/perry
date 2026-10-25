//! Canonical property-name hashes shared by image producers and runtime.
//! Finalize at production: shape-owned indexes mask this existing word.
#[inline(always)]
pub const fn finish(hash: u64) -> u64 {
    hash ^ (hash >> 32)
}

#[inline(always)]
pub const fn hash(bytes: &[u8]) -> u64 {
    hash_prefix(bytes, bytes.len())
}

/// Hash an image's content prefix, excluding its trailing NUL.
#[inline(always)]
pub const fn hash_prefix(bytes: &[u8], len: usize) -> u64 {
    assert!(len <= bytes.len());
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut i = 0;
    while i < len {
        hash = (hash ^ bytes[i] as u64).wrapping_mul(0x0100_0000_01b3);
        i += 1;
    }
    finish(hash)
}
