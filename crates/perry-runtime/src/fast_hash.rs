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
    /// #8125: `u32` keys must not fall into the byte-stream fallback above.
    /// `Hash for u32` calls `write_u32`, and `Hasher`'s DEFAULT `write_u32`
    /// forwards to `write(&n.to_ne_bytes())` — four rotate/xor iterations plus
    /// the multiply, for a key that needs exactly one multiply. The shape
    /// descriptor table (`object::shapes`) is keyed by a bare `u32` ShapeId and
    /// is probed once per allocated object and once per array element-shape
    /// test, so this override is the difference between a fast path and a
    /// loop. Deliberately identical to `write_u64` on the same numeric value;
    /// `u32_keys_take_the_multiplicative_fast_path` pins that.
    #[inline]
    fn write_u32(&mut self, n: u32) {
        self.0 = mix((n as u64).wrapping_mul(PTR_MIX));
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

/// FNV-1a constants (64-bit): offset basis and prime (standard values).
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Fast, non-cryptographic hasher for composite keys that mix a `usize` and a
/// byte string — specifically the property/accessor descriptor side tables'
/// `(usize, String)` key. [`PtrHasher`] is tuned for a *bare* `usize` pointer
/// key (a single multiply, no per-byte work) and its generic byte `write` is
/// only a weak fallback, so it is the wrong tool for a key whose second half is
/// a program-supplied property name. This hasher folds every byte of the key
/// with FNV-1a — one xor plus one multiply per byte — which is far cheaper than
/// SipHash and needs no keyed (random-seed) initialization.
///
/// DoS-resistance is unnecessary for the same reason it is for [`PtrHasher`]:
/// no external / attacker-controlled input ever reaches these keys (the first
/// half is a runtime heap address, the second a property name baked into the
/// compiled program), so hash-flooding is not a concern.
#[derive(Default, Clone, Copy)]
pub struct FastKeyHasher;

impl BuildHasher for FastKeyHasher {
    type Hasher = FastKeyHasherImpl;
    #[inline]
    fn build_hasher(&self) -> FastKeyHasherImpl {
        FastKeyHasherImpl(FNV_OFFSET_BASIS)
    }
}

pub struct FastKeyHasherImpl(u64);

/// One FNV-1a fold step over a whole machine word.
///
/// FNV-1a is defined over bytes, and [`FastKeyHasherImpl::write`] keeps that
/// definition for genuine byte strings. For an integer field, though, folding
/// byte-by-byte buys nothing: the fold is a strictly serial
/// `xor` -> `wrapping_mul` dependency chain, so an eight-byte field costs eight
/// dependent multiplies (~3 cycles of latency each) to mix a value that one
/// multiply already mixes. This folds the whole word in one step.
///
/// It FOLDS (`h ^ word`, then multiply) rather than OVERWRITING the
/// accumulator, which is the property [`PtrHasher`] deliberately lacks and the
/// reason `PtrHasher` cannot serve a multi-field key: with an overwriting
/// `write_*`, a struct hash collapses to its last field alone. Every field of
/// a composite key reaches the accumulator here.
#[inline(always)]
fn fnv_fold_word(acc: u64, word: u64) -> u64 {
    (acc ^ word).wrapping_mul(FNV_PRIME)
}

impl Hasher for FastKeyHasherImpl {
    /// Final avalanche. FNV-1a accumulates most of its entropy in the HIGH
    /// bits (the prime is only ~2^40, so a low input bit cannot reach the top
    /// of the word in one round), but `hashbrown` takes its bucket index from
    /// the LOW bits. With the word-at-a-time folds below a short key may run as
    /// few as one round, so the low bits get very little mixing on their own.
    ///
    /// Multiplying by the Fibonacci constant and folding the top half down
    /// pushes the accumulated high-bit entropy into the bucket-index bits for
    /// one extra multiply per lookup — against the ~30 multiplies the word
    /// folds save on a `ShapeFacts` key. This is the same hazard, and the same
    /// remedy, as [`mix`] on [`PtrHasher`]: without it, keys that differ only
    /// in their high bits (or aligned pointers, whose low bits are constant)
    /// pile into one bucket and the map degrades to a linked list.
    #[inline]
    fn finish(&self) -> u64 {
        let h = self.0.wrapping_mul(PTR_MIX);
        h ^ (h >> 32)
    }
    /// FNV-1a byte fold, for the genuine byte-string half of a key: a
    /// `(usize, String)` key hashes its `String` half via `str`'s `Hash`
    /// (`write(bytes)` plus a `write_u8(0xff)` terminator), and both route
    /// here.
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut h = self.0;
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        self.0 = h;
    }

    // Integer writes fold one word per call instead of falling into `Hasher`'s
    // default `write_uN` -> `write(&n.to_ne_bytes())` byte loop.
    //
    // This is what the shape table's `ids_by_facts` key pays for: `ShapeFacts`
    // is six integer fields (two `u64`, three `u32`, one enum discriminant, an
    // `isize`), so the derived `Hash` fed ~36 bytes -- ~36 serial multiplies --
    // through the byte loop for a key that six folds mix just as well. That
    // lookup runs on every shape publish (`shape_descriptor_ensure_with_holes`
    // probes `ids_by_facts` before minting an id), i.e. on every object
    // property add/delete that transitions a shape.
    //
    // `write_u8` is deliberately included even though it is exactly equivalent
    // to the byte path for a single byte: routing it here keeps every integer
    // width in one place rather than leaving one width to a different rule.

    #[inline]
    fn write_u8(&mut self, n: u8) {
        self.0 = fnv_fold_word(self.0, n as u64);
    }
    #[inline]
    fn write_u16(&mut self, n: u16) {
        self.0 = fnv_fold_word(self.0, n as u64);
    }
    #[inline]
    fn write_u32(&mut self, n: u32) {
        self.0 = fnv_fold_word(self.0, n as u64);
    }
    #[inline]
    fn write_u64(&mut self, n: u64) {
        self.0 = fnv_fold_word(self.0, n);
    }
    #[inline]
    fn write_u128(&mut self, n: u128) {
        self.0 = fnv_fold_word(self.0, n as u64);
        self.0 = fnv_fold_word(self.0, (n >> 64) as u64);
    }
    #[inline]
    fn write_usize(&mut self, n: usize) {
        self.0 = fnv_fold_word(self.0, n as u64);
    }
    #[inline]
    fn write_i8(&mut self, n: i8) {
        self.0 = fnv_fold_word(self.0, n as u64);
    }
    #[inline]
    fn write_i16(&mut self, n: i16) {
        self.0 = fnv_fold_word(self.0, n as u64);
    }
    #[inline]
    fn write_i32(&mut self, n: i32) {
        self.0 = fnv_fold_word(self.0, n as u64);
    }
    #[inline]
    fn write_i64(&mut self, n: i64) {
        self.0 = fnv_fold_word(self.0, n as u64);
    }
    #[inline]
    fn write_i128(&mut self, n: i128) {
        self.write_u128(n as u128);
    }
    /// `derive(Hash)` on a fieldless enum hashes `discriminant_value(self)`,
    /// whose type is `isize` for a default-repr enum -- so `ShapeFacts`'
    /// `object_kind` field lands here, not on `write_u8`.
    #[inline]
    fn write_isize(&mut self, n: isize) {
        self.0 = fnv_fold_word(self.0, n as u64);
    }
}

/// `HashMap` keyed by a byte-hashable composite key (e.g. `(usize, String)`)
/// using the non-cryptographic [`FastKeyHasher`] instead of SipHash.
pub type FastKeyHashMap<K, V> = HashMap<K, V, FastKeyHasher>;

#[inline]
pub fn new_fast_key_hash_map<K: std::hash::Hash + Eq, V>() -> FastKeyHashMap<K, V> {
    HashMap::with_hasher(FastKeyHasher)
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

    /// #8125: a `u32` key must reach the single-multiply path, not the
    /// per-byte fold. `Hash for u32` calls `write_u32`; without an override
    /// `Hasher`'s default `write_u32` forwards to `write(&n.to_ne_bytes())`.
    /// Deleting `PtrHasherImpl::write_u32` turns this red.
    #[test]
    fn u32_keys_take_the_multiplicative_fast_path() {
        use std::hash::Hash;
        for n in [0u32, 1, 0x8000_0000, 0x8000_02ff, u32::MAX] {
            let mut via_hash = PtrHasher.build_hasher();
            n.hash(&mut via_hash);
            let mut via_u64 = PtrHasher.build_hasher();
            via_u64.write_u64(n as u64);
            assert_eq!(
                via_hash.finish(),
                via_u64.finish(),
                "u32 key {n:#x} fell into the byte-stream fallback"
            );
        }
    }

    /// ShapeIds are minted sequentially from `SHAPE_ID_BASE` (0x8000_0000), so
    /// the descriptor table's keys are a dense run in the TOP half of the u32
    /// range. Multiplicative mixing has to spread that run across buckets —
    /// `HashMap` indexes on the LOW bits, and a run of consecutive integers has
    /// no entropy there. Dropping `mix` (the `^= h >> 32` avalanche) collapses
    /// this to a handful of buckets and turns the map into a linked list.
    #[test]
    fn sequential_shape_ids_spread_across_low_bit_buckets() {
        use std::collections::HashSet;
        let base = 0x8000_0000u32;
        let mut buckets = HashSet::new();
        for i in 0..1024u32 {
            let mut h = PtrHasher.build_hasher();
            h.write_u32(base + i);
            buckets.insert(h.finish() & 0x3ff);
        }
        assert!(
            buckets.len() > 512,
            "sequential ShapeIds hashed into only {} of 1024 buckets",
            buckets.len()
        );
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

    /// The word-at-a-time integer folds must actually be TAKEN — this is the
    /// `FastKeyHasher` analogue of
    /// `u32_keys_take_the_multiplicative_fast_path`. Deleting
    /// `FastKeyHasherImpl::write_u64` drops `u64` back onto `Hasher`'s default
    /// `write_u64` -> `write(&n.to_ne_bytes())` byte loop (eight dependent
    /// multiplies instead of one) and turns this red.
    #[test]
    fn integer_writes_take_the_word_fold_fast_path() {
        let n = 0x0123_4567_89ab_cdefu64;
        let mut h = FastKeyHasher.build_hasher();
        h.write_u64(n);
        let acc = (FNV_OFFSET_BASIS ^ n).wrapping_mul(FNV_PRIME);
        let expected = {
            let x = acc.wrapping_mul(PTR_MIX);
            x ^ (x >> 32)
        };
        assert_eq!(
            h.finish(),
            expected,
            "write_u64 fell into the per-byte fold"
        );
    }

    /// The defining difference from [`PtrHasher`]: the integer writes FOLD the
    /// accumulator instead of overwriting it, so every field of a composite key
    /// reaches the hash. With an overwriting `write_*` (what `PtrHasher` does,
    /// correctly, for its single-word key) a `ShapeFacts` would hash to its last
    /// field alone and the shape table's reverse index would collapse.
    #[test]
    fn word_folds_keep_every_field_of_a_composite_key() {
        fn hash(fields: &[u64]) -> u64 {
            let mut h = FastKeyHasher.build_hasher();
            for &f in fields {
                h.write_u64(f);
            }
            h.finish()
        }
        // Same trailing field, different leading fields.
        assert_ne!(hash(&[1, 2, 3]), hash(&[9, 9, 3]));
        // A prefix must not alias the whole key.
        assert_ne!(hash(&[1, 2, 3]), hash(&[3]));
        // Field ORDER is part of the identity.
        assert_ne!(hash(&[1, 2]), hash(&[2, 1]));
        // Mixed widths must not alias either.
        let mut a = FastKeyHasher.build_hasher();
        a.write_u32(1);
        a.write_u32(2);
        let mut b = FastKeyHasher.build_hasher();
        b.write_u64((1u64 << 32) | 2);
        assert_ne!(a.finish(), b.finish());
    }

    /// A `ShapeFacts`-shaped key is mostly SMALL integers (key counts, hole
    /// counts, a two-variant enum discriminant) plus one heap address. Those
    /// live in the low bits, which is exactly where `hashbrown` reads its
    /// bucket index — so the `finish()` avalanche has to carry the entropy
    /// down. Deleting the `wrapping_mul(PTR_MIX)` from `finish` collapses this.
    #[test]
    fn shape_facts_shaped_keys_spread_across_low_bit_buckets() {
        use std::collections::HashSet;
        let mut buckets = HashSet::new();
        let mut n = 0usize;
        // 8-byte-aligned keys array addresses, small counts, 2 object kinds.
        for keys in 0..64u64 {
            for logical in 0..8u32 {
                for holes in 0..2u32 {
                    for kind in 0..2isize {
                        let mut h = FastKeyHasher.build_hasher();
                        h.write_u64(0x1_0000_0000 + keys * 8);
                        h.write_u32(logical);
                        h.write_u32(logical);
                        h.write_u64(0);
                        h.write_isize(kind);
                        h.write_u32(holes);
                        buckets.insert(h.finish() & 0x3ff);
                        n += 1;
                    }
                }
            }
        }
        assert_eq!(n, 2048);
        assert!(
            buckets.len() > 600,
            "ShapeFacts-shaped keys hashed into only {} of 1024 buckets",
            buckets.len()
        );
    }

    /// Bare-`usize`-keyed `FastKeyHashMap`s exist too (`attr_keys_by_owner`,
    /// `SYMBOL_PROPERTY_ATTRS`' first half), and a bare key runs only ONE fold
    /// round. Aligned heap addresses must still spread.
    #[test]
    fn single_round_pointer_keys_spread_across_low_bit_buckets() {
        use std::collections::HashSet;
        let mut buckets = HashSet::new();
        let base = 0x1_0000_0000usize;
        for i in 0..1024 {
            let mut h = FastKeyHasher.build_hasher();
            h.write_usize(base + i * 16);
            buckets.insert(h.finish() & 0x3ff);
        }
        assert!(
            buckets.len() > 512,
            "aligned pointers hashed into only {} of 1024 buckets",
            buckets.len()
        );
    }

    /// The descriptor side tables key on `(usize, String)`. Verify the FNV-1a
    /// composite-key hasher round-trips: distinct owner addresses and distinct
    /// key strings must not alias, and lookups by borrowed `&str`/`&(…)` keys
    /// must find the same slot the owned key inserted.
    #[test]
    fn fast_key_map_composite_round_trip() {
        let mut m = new_fast_key_hash_map::<(usize, String), u8>();
        let base = 0x100_0000_0000usize;
        for i in 0..500 {
            m.insert((base + i * 8, format!("key{i}")), i as u8);
            // Same address, different key must be a distinct slot.
            m.insert((base + i * 8, "length".to_string()), 0x07);
        }
        for i in 0..500 {
            assert_eq!(m.get(&(base + i * 8, format!("key{i}"))), Some(&(i as u8)));
            assert_eq!(m.get(&(base + i * 8, "length".to_string())), Some(&0x07));
            // Distinct address, same key name must miss.
            assert_eq!(m.get(&(base + i * 8 + 4, format!("key{i}"))), None);
        }
        assert_eq!(m.len(), 500 + 500);
        m.remove(&(base, "key0".to_string()));
        assert_eq!(m.get(&(base, "key0".to_string())), None);
        assert_eq!(m.get(&(base, "length".to_string())), Some(&0x07));
    }
}
