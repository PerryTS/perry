//! Fast pointer/usize hasher for runtime registries.
//!
//! Several runtime registries are keyed by raw heap pointers (`usize`):
//! `SET_REGISTRY`, `BUFFER_REGISTRY`, `MAP_REGISTRY`, the gen-GC's
//! `REMEMBERED_SET`, etc. With `std::collections::HashSet`'s default
//! `RandomState` (SipHash) every `contains` call pays ~30 ns of
//! cryptographic hashing — `core::hash::BuildHasher::hash_one` was
//! 14% leaf samples on perf-comprehensive before any optimization
//! and ~11% after the Map fast pre-filter landed.
//!
//! Pointers from a system allocator are already ~uniformly distributed
//! in their middle bits (slabs, alignment dropouts) — collision-resistant
//! hashing buys nothing, and DoS-resistance doesn't apply because no
//! external input ever reaches these keys. Multiplicative mixing with
//! a Fibonacci-hash constant gives a single `mul` per write_usize.
//!
//! Apply via `HashSet<usize, PtrHasher>::with_hasher(PtrHasher)` (or via
//! the `PtrHashSet` / `PtrHashMap` aliases) anywhere a pointer-keyed
//! registry doesn't need cryptographic hashing.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasher, Hasher};

/// Fibonacci-hash constant: 2^64 / φ, rounded to odd.
/// Standard Knuth multiplicative-hash recommendation.
const PTR_MIX: u64 = 0x9E37_79B9_7F4A_7C15;

#[derive(Default, Clone, Copy)]
pub struct PtrHasher;

impl BuildHasher for PtrHasher {
    type Hasher = PtrHasherImpl;
    #[inline]
    fn build_hasher(&self) -> PtrHasherImpl {
        PtrHasherImpl(0)
    }
}

pub struct PtrHasherImpl(u64);

impl Hasher for PtrHasherImpl {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
    /// Generic byte-stream fallback. Used when a non-`u64`/`usize` key is
    /// hashed — never on the registries since their key is `usize` whose
    /// `Hash` impl calls `write_usize`. Mixes each byte with a rotation +
    /// xor so the fallback isn't trivially zeroable.
    fn write(&mut self, bytes: &[u8]) {
        let mut h = self.0;
        for &b in bytes {
            h = h.rotate_left(5) ^ (b as u64);
        }
        self.0 = mix(h.wrapping_mul(PTR_MIX));
    }
    #[inline]
    fn write_u64(&mut self, n: u64) {
        self.0 = mix(n.wrapping_mul(PTR_MIX));
    }
    #[inline]
    fn write_usize(&mut self, n: usize) {
        self.0 = mix((n as u64).wrapping_mul(PTR_MIX));
    }
}

/// Avalanche step (xorshift on the upper half) so values with all-zero
/// low bits — typical of integer-encoded f64 keys (whole numbers
/// store as mantissa = 0) — don't all collide on a single bucket
/// when `HashMap` uses `hash & (capacity - 1)` for bucket indexing.
/// Pure multiplicative hashing puts entropy in the upper bits, but
/// std `HashMap` reads bucket indices from the lower bits.
///
/// Tested on perf-comprehensive: removing this step + applying
/// `PtrHasher` to `MAP_INDEX`'s inner `NumericKey(u64)` map (which
/// stores f64 bit-patterns of EntityIds, all with mantissa-zero
/// for whole numbers) regressed from 455 ms → 830 ms because
/// EntityId 1024..15000 all hashed to bucket 0. The `^= h >> 32`
/// fixes the case at ~1 cycle of cost on the heap-ptr-keyed
/// registries that don't need it.
#[inline(always)]
fn mix(h: u64) -> u64 {
    h ^ (h >> 32)
}

pub type PtrHashSet<T> = HashSet<T, PtrHasher>;
pub type PtrHashMap<K, V> = HashMap<K, V, PtrHasher>;

/// Constructor convenience: `PtrHashSet::default()` works because
/// `PtrHasher` impls `Default`, but call sites that need an explicit
/// builder for a `RefCell::new(...)` initializer reach for this helper.
#[inline]
pub fn new_ptr_hash_set<T: std::hash::Hash + Eq>() -> PtrHashSet<T> {
    HashSet::with_hasher(PtrHasher)
}

#[inline]
pub fn new_ptr_hash_map<K: std::hash::Hash + Eq, V>() -> PtrHashMap<K, V> {
    HashMap::with_hasher(PtrHasher)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ptr_set_basic() {
        let mut s = new_ptr_hash_set::<usize>();
        s.insert(0xdead_beef);
        s.insert(0x4242);
        assert!(s.contains(&0xdead_beef));
        assert!(!s.contains(&0xcafe));
        s.remove(&0xdead_beef);
        assert!(!s.contains(&0xdead_beef));
    }

    #[test]
    fn ptr_map_basic() {
        let mut m = new_ptr_hash_map::<usize, &'static str>();
        m.insert(0x1000, "a");
        m.insert(0x2000, "b");
        assert_eq!(m.get(&0x1000), Some(&"a"));
        assert_eq!(m.get(&0x9999), None);
    }

    /// Pointer-aligned keys collide trivially with multiply-only on the
    /// low bits — Fibonacci-hash mixing into the upper bits is what
    /// keeps the buckets evenly populated. Sanity-check that 1000 8-byte-
    /// aligned addresses end up in distinct slots (HashSet rebalances
    /// internally; just make sure inserts/contains all round-trip).
    #[test]
    fn aligned_keys_round_trip() {
        let mut s = new_ptr_hash_set::<usize>();
        let base = 0x100_0000_0000usize;
        for i in 0..1000 {
            s.insert(base + i * 8);
        }
        for i in 0..1000 {
            assert!(s.contains(&(base + i * 8)));
        }
        assert!(!s.contains(&(base + 1000 * 8)));
    }
}
