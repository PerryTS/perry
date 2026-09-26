//! String-key comparison lane for `Map` lookups (#10697).
//!
//! A string key used to reach every entry through the generic
//! [`jsvalue_eq`](super::jsvalue_eq), which on each MISMATCH re-probed both
//! operands for symbol-ness and bigint-ness, re-validated both GC headers and
//! decoded both inline (SSO) strings byte-by-byte into scratch buffers before
//! a length compare could reject the pair. A key whose bits happened to equal
//! the stored entry's matched on the bit-identity fast path and skipped all of
//! that, so the cost of a lookup was set by how many entries it had to reject
//! first: `m.get("alpha")` against the first entry measured 2.2x cheaper than
//! `m.get(CATS[i & 3])` spread over four, although both reach the same entry
//! point.
//!
//! [`StrKey`] decodes the key once per lookup; [`StrKey::matches`] rejects an
//! entry by its tag and byte length before it touches any bytes, and compares
//! two inline strings as one masked word.

use super::{jsvalue_eq, string_view_from_bits};
use crate::value::{
    POINTER_TAG, SHORT_STRING_DATA_MASK, SHORT_STRING_LEN_MASK, SHORT_STRING_LEN_SHIFT,
    SHORT_STRING_MAX_LEN, SHORT_STRING_TAG, STRING_TAG,
};

const SSO_UPPER: u64 = SHORT_STRING_TAG >> 48;
const STRING_UPPER: u64 = STRING_TAG >> 48;
const POINTER_UPPER: u64 = POINTER_TAG >> 48;

/// A string-like Map key, decoded once for the whole scan.
#[derive(Clone, Copy)]
pub(super) enum StrKey {
    /// Inline (SSO) string: byte length, and the data bytes masked to it.
    Short { len: u32, data: u64 },
    /// Heap string whose GC header was validated as `GC_TYPE_STRING`.
    Heap { ptr: *const u8, len: u32 },
}

/// `(byte_len, data)` of an inline string, with every byte at or past `len`
/// cleared, so two encodings of the same content compare equal as words even
/// if a producer left stale bytes in the unused tail — the byte-wise decode
/// this replaces only ever read the first `len` bytes.
#[inline(always)]
fn sso_parts(bits: u64) -> (u32, u64) {
    let len = ((bits & SHORT_STRING_LEN_MASK) >> SHORT_STRING_LEN_SHIFT) as u32;
    let live = if len as usize >= SHORT_STRING_MAX_LEN {
        SHORT_STRING_DATA_MASK
    } else {
        (1u64 << (len * 8)) - 1
    };
    (len, bits & live)
}

/// Heap bytes `[ptr, ptr+len)` against an inline string's data word.
#[inline(always)]
unsafe fn heap_eq_short(ptr: *const u8, data: u64, len: u32) -> bool {
    (0..len as usize).all(|i| *ptr.add(i) == (data >> (i * 8)) as u8)
}

/// Byte equality of two heap strings of the same `len`. Distinct keys of one
/// length nearly always differ in their first or last word — a shared
/// namespace prefix (`category-alpha` / `category-gamma`) only defeats the
/// first — so both are compared inline before the out-of-line `memcmp` a
/// slice comparison lowers to. The two words cover every key up to 16 bytes.
#[inline(always)]
unsafe fn heap_bytes_eq(a: *const u8, b: *const u8, len: usize) -> bool {
    if len < 8 {
        return (0..len).all(|i| *a.add(i) == *b.add(i));
    }
    let word = |p: *const u8, at: usize| (p.add(at) as *const u64).read_unaligned();
    if word(a, 0) != word(b, 0) || word(a, len - 8) != word(b, len - 8) {
        return false;
    }
    len <= 16 || std::slice::from_raw_parts(a, len) == std::slice::from_raw_parts(b, len)
}

impl StrKey {
    /// Decode an inline or `STRING_TAG` key, or `None` for anything else —
    /// the caller keeps the generic path then.
    ///
    /// A pointer-tagged or raw-pointer key is deliberately NOT decoded even
    /// when it names string bytes: a description-less `Symbol()` exposes a
    /// zero-length string view there, and only `jsvalue_eq`'s symbol guard
    /// keeps it from colliding with the `""` key (#4570).
    #[inline(always)]
    pub(super) fn decode(bits: u64) -> Option<Self> {
        match bits >> 48 {
            SSO_UPPER => {
                let (len, data) = sso_parts(bits);
                Some(StrKey::Short { len, data })
            }
            STRING_UPPER => {
                // Not SSO, so the view points into the heap string itself and
                // outlives `scratch`.
                let mut scratch = [0u8; SHORT_STRING_MAX_LEN];
                let (ptr, len) = string_view_from_bits(bits, &mut scratch)?;
                Some(StrKey::Heap { ptr, len })
            }
            _ => None,
        }
    }

    /// Content equality against a validated heap string's bytes.
    #[inline(always)]
    unsafe fn eq_heap(self, ptr: *const u8, len: u32) -> bool {
        match self {
            StrKey::Short { len: klen, data } => klen == len && heap_eq_short(ptr, data, len),
            StrKey::Heap {
                ptr: kptr,
                len: klen,
            } => klen == len && (kptr == ptr || heap_bytes_eq(kptr, ptr, len as usize)),
        }
    }

    /// SameValueZero of this key (whose NaN-boxed bits are `key_bits`)
    /// against a stored entry key. Agrees with `jsvalue_eq(entry, key)` for
    /// every entry; only the order of the checks changed.
    #[inline(always)]
    pub(super) fn matches(self, key_bits: u64, entry_bits: u64) -> bool {
        if entry_bits == key_bits {
            return true;
        }
        match entry_bits >> 48 {
            SSO_UPPER => {
                let (elen, edata) = sso_parts(entry_bits);
                match self {
                    StrKey::Short { len, data } => len == elen && data == edata,
                    StrKey::Heap { ptr, len } => {
                        len == elen && unsafe { heap_eq_short(ptr, edata, len) }
                    }
                }
            }
            STRING_UPPER => {
                let mut scratch = [0u8; SHORT_STRING_MAX_LEN];
                match string_view_from_bits(entry_bits, &mut scratch) {
                    Some((ptr, len)) => unsafe { self.eq_heap(ptr, len) },
                    None => false,
                }
            }
            // A string stored under a generic pointer tag, a legacy raw
            // pointer, or a small subnormal double: rare, and the generic
            // comparison already decides every one of them.
            POINTER_UPPER | 0 => jsvalue_eq(f64::from_bits(entry_bits), f64::from_bits(key_bits)),
            // Numbers, int32, booleans, nullish, holes, bigints and handles
            // are never equal to a string.
            _ => false,
        }
    }

    /// FNV-1a over the key's bytes — the same hash `string_content_hash`
    /// computes from the NaN-boxed value, without decoding it a second time.
    #[inline(always)]
    pub(super) fn content_hash(self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut step = |b: u8| {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        };
        match self {
            StrKey::Short { len, data } => {
                for i in 0..len.min(SHORT_STRING_MAX_LEN as u32) as usize {
                    step((data >> (i * 8)) as u8);
                }
            }
            StrKey::Heap { ptr, len } => {
                for i in 0..len as usize {
                    step(unsafe { *ptr.add(i) });
                }
            }
        }
        h
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use super::StrKey;
    use crate::string::js_string_from_bytes;
    use crate::value::{JSValue, SHORT_STRING_TAG, TAG_TRUE};

    fn heap(s: &str) -> f64 {
        boxed_heap_string_key(js_string_from_bytes(s.as_ptr(), s.len() as u32))
    }

    fn sso(s: &str) -> f64 {
        f64::from_bits(JSValue::try_short_string(s.as_bytes()).unwrap().bits())
    }

    /// Every (key, entry) pairing the lane decides agrees with the generic
    /// comparison it replaces: same content across SSO / heap / distinct heap
    /// allocations, and every flavour of mismatch.
    #[test]
    fn matches_agrees_with_jsvalue_eq() {
        let values = [
            sso(""),
            sso("a"),
            sso("ab"),
            sso("alpha"),
            sso("beta"),
            heap(""),
            heap("a"),
            heap("alpha"),
            heap("alpha"),
            heap("alphabet"),
            heap("alphabeT"),
            heap("category-alpha"),
            0.0,
            1.5,
            f64::from_bits(TAG_TRUE),
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(MAP_HOLE_KEY_BITS),
            f64::from_bits(crate::value::INT32_TAG | 5),
        ];
        for &key in &values {
            let Some(skey) = StrKey::decode(key.to_bits()) else {
                assert!(!is_string_like(key.to_bits()));
                continue;
            };
            for &entry in &values {
                if entry.to_bits() == MAP_HOLE_KEY_BITS {
                    assert!(!skey.matches(key.to_bits(), entry.to_bits()));
                    continue;
                }
                assert_eq!(
                    skey.matches(key.to_bits(), entry.to_bits()),
                    jsvalue_eq(entry, key),
                    "key {:#x} vs entry {:#x}",
                    key.to_bits(),
                    entry.to_bits()
                );
            }
        }
    }

    /// Inline strings compare by their first `len` bytes only, as the
    /// byte-wise decode did: stale bytes in the unused tail are not content.
    #[test]
    fn inline_strings_ignore_bytes_past_their_length() {
        let canonical = sso("ab").to_bits();
        let stale_tail = canonical | (0x7A << 24);
        assert_ne!(canonical, stale_tail);
        let skey = StrKey::decode(stale_tail).unwrap();
        assert!(skey.matches(stale_tail, canonical));
        assert!(skey.matches(stale_tail, heap("ab").to_bits()));
        assert!(!skey.matches(stale_tail, sso("abz").to_bits()));
        assert_eq!(stale_tail >> 48, SHORT_STRING_TAG >> 48);
    }

    /// The hash the side table is keyed by must be the one the key's
    /// decoded form produces, or the hashed path misses stored keys.
    #[test]
    fn content_hash_matches_the_side_table_hash() {
        for v in [sso(""), sso("gamma"), heap("gamma"), heap("category-delta")] {
            let skey = StrKey::decode(v.to_bits()).unwrap();
            assert_eq!(Some(skey.content_hash()), string_content_hash(v.to_bits()));
        }
    }

    /// A description-less symbol exposes a zero-length string view; it must
    /// stay a distinct key from `""` in both the small scan and the hashed
    /// index, from either side of the lookup (#4570).
    #[test]
    fn empty_symbol_and_empty_string_stay_distinct() {
        for filler in [0usize, 12] {
            let map = js_map_alloc(4);
            for i in 0..filler {
                js_map_set(map, heap(&format!("filler-{i}")), -1.0);
            }
            let symbol = unsafe { crate::symbol::js_symbol_new_empty() };
            js_map_set(map, sso(""), 1.0);
            js_map_set(map, symbol, 2.0);
            assert_eq!(js_map_size(map) as usize, filler + 2);
            assert_eq!(js_map_get(map, sso("")), 1.0);
            assert_eq!(js_map_get(map, heap("")), 1.0);
            assert_eq!(js_map_get(map, symbol), 2.0);
            let other = unsafe { crate::symbol::js_symbol_new_empty() };
            assert_eq!(js_map_get(map, other).to_bits(), TAG_UNDEFINED);
        }
    }

    /// End to end through both the small linear scan and the hashed index:
    /// a key stored in one representation is found by the other.
    #[test]
    fn lookups_cross_string_representations() {
        for count in [4usize, 40] {
            let map = js_map_alloc(4);
            let names: Vec<String> = (0..count).map(|i| format!("k{i}")).collect();
            for (i, name) in names.iter().enumerate() {
                let key = if i % 2 == 0 { sso(name) } else { heap(name) };
                js_map_set(map, key, i as f64);
            }
            js_map_set(map, 7.0, -1.0);
            assert_eq!(js_map_size(map) as usize, count + 1);
            for (i, name) in names.iter().enumerate() {
                let other = if i % 2 == 0 { heap(name) } else { sso(name) };
                assert_eq!(js_map_get(map, other), i as f64, "{name} (count {count})");
                assert_eq!(js_map_has(map, heap(name)), 1);
            }
            assert_eq!(js_map_get(map, sso("k")).to_bits(), TAG_UNDEFINED);
            assert_eq!(js_map_get(map, heap("k999")).to_bits(), TAG_UNDEFINED);
            assert_eq!(js_map_get(map, sso("7")).to_bits(), TAG_UNDEFINED);
            let typed = js_string_from_bytes(b"k1".as_ptr(), 2);
            assert_eq!(js_map_get_string_key(map, typed), 1.0);
        }
    }
}
