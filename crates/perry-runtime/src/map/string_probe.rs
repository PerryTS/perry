//! String-key lookup lane (#10697).
//!
//! A string key against a small map used to reach `find_key_index_cold`'s
//! generic scan, which ran the full [`jsvalue_eq`] against every entry: two
//! symbol probes, two bigint probes, `is_string_like` on both sides, and a
//! fresh `string_view_from_bits` decode of the *probe key* per entry. On a
//! four-constant-key "count by category" loop that was two thirds of the
//! program. This lane decodes the key once and compares each entry by tag:
//! bit identity, then length, then bytes.
//!
//! It keeps `jsvalue_eq`'s answer exactly. A probe is only formed for a key
//! whose tag alone proves it a string (`STRING_TAG` or SSO), and neither tag
//! can name a symbol (`js_is_symbol` requires `POINTER_TAG`) or a bigint
//! (`bigint_ptr_from_bits` accepts `BIGINT_TAG`, `POINTER_TAG` and raw
//! pointers only). An entry with any other tag is handed back to
//! `jsvalue_eq` unchanged, so string-typed `POINTER_TAG`/raw keys, symbols and
//! bigints keep their existing path.
use super::*;

/// A string key decoded once for a scan.
#[derive(Clone, Copy)]
pub(super) enum StringProbe {
    /// SSO: the payload bytes masked to `len`, little-endian (byte `i` at bit
    /// `i * 8`) — the same layout `string_view_from_bits` decodes.
    Short { data: u64, len: u32 },
    /// A validated `GC_TYPE_STRING` pointee's inline bytes.
    Heap { ptr: *const u8, len: u32 },
}

#[inline(always)]
fn short_parts(bits: u64) -> (u64, u32) {
    let len = ((bits & crate::value::SHORT_STRING_LEN_MASK) >> crate::value::SHORT_STRING_LEN_SHIFT)
        as u32;
    (
        bits & crate::value::SHORT_STRING_DATA_MASK & byte_mask(len),
        len,
    )
}

/// Mask keeping the low `len` bytes; `len <= SHORT_STRING_MAX_LEN` (5), so the
/// shift never reaches 64.
#[inline(always)]
fn byte_mask(len: u32) -> u64 {
    if len as usize >= crate::value::SHORT_STRING_MAX_LEN {
        crate::value::SHORT_STRING_DATA_MASK
    } else {
        (1u64 << (len * 8)) - 1
    }
}

/// The inline bytes of a `STRING_TAG` value, or `None` when the pointee is not
/// a validated `GC_TYPE_STRING` allocation — the same check
/// `string_view_from_bits` makes for that tag.
#[inline(always)]
unsafe fn heap_view(bits: u64) -> Option<(*const u8, u32)> {
    let ptr = (bits & crate::value::POINTER_MASK) as *const StringHeader;
    match crate::value::addr_class::try_read_gc_header(ptr as usize) {
        Some(header) if header.obj_type == crate::gc::GC_TYPE_STRING => Some((
            (ptr as *const u8).add(std::mem::size_of::<StringHeader>()),
            (*ptr).byte_len,
        )),
        _ => None,
    }
}

/// Pack heap bytes into the SSO payload layout. Callers have already matched
/// `len` against an SSO length, so it is at most `SHORT_STRING_MAX_LEN`; the
/// clamp only keeps the shift in range if that invariant were ever broken.
#[inline(always)]
unsafe fn pack_short(ptr: *const u8, len: u32) -> u64 {
    let mut data = 0u64;
    for i in 0..(len as usize).min(crate::value::SHORT_STRING_MAX_LEN) {
        data |= (*ptr.add(i) as u64) << (i * 8);
    }
    data
}

/// Equal-length heap byte compare. Same-length keys in a small map usually
/// differ at an end ("alpha"/"gamma"/"delta"), so the first and last bytes
/// reject those without the `memcmp` call.
#[inline(always)]
unsafe fn heap_bytes_eq(a: *const u8, b: *const u8, len: u32) -> bool {
    if len == 0 {
        return true;
    }
    let last = len as usize - 1;
    *a == *b
        && *a.add(last) == *b.add(last)
        && std::slice::from_raw_parts(a, len as usize)
            == std::slice::from_raw_parts(b, len as usize)
}

impl StringProbe {
    /// Decode `bits` when its tag alone proves it a string. `None` for every
    /// other value, including a `STRING_TAG` whose pointee fails validation —
    /// the generic path decides those.
    #[inline(always)]
    pub(super) unsafe fn from_bits(bits: u64) -> Option<Self> {
        match bits >> 48 {
            upper if upper == crate::value::SHORT_STRING_TAG >> 48 => {
                let (data, len) = short_parts(bits);
                Some(StringProbe::Short { data, len })
            }
            0x7FFF => {
                let (ptr, len) = heap_view(bits)?;
                Some(StringProbe::Heap { ptr, len })
            }
            _ => None,
        }
    }

    /// `jsvalue_eq(entry, key)` for the key this probe was decoded from, or
    /// `None` when `entry_bits` is neither `STRING_TAG` nor SSO and the caller
    /// must ask `jsvalue_eq`. Bit identity is the caller's first check.
    #[inline(always)]
    pub(super) unsafe fn eq_entry(&self, entry_bits: u64) -> Option<bool> {
        match entry_bits >> 48 {
            upper if upper == crate::value::SHORT_STRING_TAG >> 48 => {
                let (entry_data, entry_len) = short_parts(entry_bits);
                Some(match *self {
                    StringProbe::Short { data, len } => len == entry_len && data == entry_data,
                    StringProbe::Heap { ptr, len } => {
                        len == entry_len && pack_short(ptr, len) == entry_data
                    }
                })
            }
            0x7FFF => {
                // A STRING_TAG entry that fails validation compares unequal,
                // exactly as `jsvalue_eq` falls through to `false` for it.
                let Some((entry_ptr, entry_len)) = heap_view(entry_bits) else {
                    return Some(false);
                };
                Some(match *self {
                    StringProbe::Short { data, len } => {
                        len == entry_len && pack_short(entry_ptr, len) == data
                    }
                    StringProbe::Heap { ptr, len } => {
                        len == entry_len && (ptr == entry_ptr || heap_bytes_eq(ptr, entry_ptr, len))
                    }
                })
            }
            _ => None,
        }
    }

    /// Does the entry at `entry_bits` hold this key?
    #[inline(always)]
    pub(super) unsafe fn matches(&self, entry_bits: u64, key: f64) -> bool {
        if entry_bits == key.to_bits() {
            return true;
        }
        match self.eq_entry(entry_bits) {
            Some(eq) => eq,
            // Holes are `TAG_HOLE`, never SSO/STRING_TAG, so they land here;
            // `jsvalue_eq` never equates a hole with a string.
            None => entry_bits != MAP_HOLE_KEY_BITS && jsvalue_eq(f64::from_bits(entry_bits), key),
        }
    }
}

/// Linear scan of a small map's entries for a probed string key.
#[inline(always)]
pub(super) unsafe fn scan_entries(
    map: *const MapHeader,
    probe: &StringProbe,
    key: f64,
    used: u32,
) -> i32 {
    let entries = entries_ptr(map);
    for i in 0..used {
        let entry_bits = ptr::read(entries.add((i as usize) * 2)).to_bits();
        if probe.matches(entry_bits, key) {
            return i as i32;
        }
    }
    -1
}

/// Every string-key lookup: the small-map scan, or the content-hash index's
/// candidates, each compared through the decoded probe. `None` when `key` is
/// not a tag-proven string; the caller's generic path then decides it.
#[inline(never)]
pub(super) unsafe fn find_string_key(map: *const MapHeader, key: f64) -> Option<i32> {
    let probe = StringProbe::from_bits(key.to_bits())?;
    let used = (*map).used;
    if used <= SIDE_TABLE_THRESHOLD {
        return Some(scan_entries(map, &probe, key, used));
    }
    let Some(store) = (*map).store.as_ref() else {
        return Some(scan_entries(map, &probe, key, used));
    };
    let hash = string_content_hash(key.to_bits())?;
    let entries = entries_ptr(map);
    for cand_idx in store.strings.candidates(hash) {
        if cand_idx >= used {
            continue;
        }
        let entry_bits = ptr::read(entries.add((cand_idx as usize) * 2)).to_bits();
        if probe.matches(entry_bits, key) {
            return Some(cand_idx as i32);
        }
    }
    Some(-1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::string::js_string_from_bytes;

    fn heap(bytes: &[u8]) -> f64 {
        boxed_heap_string_key(js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32))
    }

    fn short(bytes: &[u8]) -> f64 {
        f64::from_bits(
            crate::value::JSValue::try_short_string(bytes)
                .unwrap()
                .bits(),
        )
    }

    /// The pre-#10697 answer: `jsvalue_eq` against every live entry.
    unsafe fn reference_index(map: *const MapHeader, key: f64) -> i32 {
        let entries = entries_ptr(map);
        for i in 0..(*map).used {
            let entry = ptr::read(entries.add((i as usize) * 2));
            if entry.to_bits() != MAP_HOLE_KEY_BITS && jsvalue_eq(entry, key) {
                return i as i32;
            }
        }
        -1
    }

    /// Every probe-able key encoding against every entry encoding, on a small
    /// (linear-scan) map and on an indexed one, must agree with `jsvalue_eq`.
    #[test]
    fn string_probe_matches_jsvalue_eq_across_encodings() {
        let symbol = unsafe { crate::symbol::js_symbol_new_empty() };
        let words: [&[u8]; 6] = [b"", b"a", b"beta", b"gamma", b"delta", b"alpha"];
        for pad in [0usize, 20] {
            let map = js_map_alloc(4);
            // Mixed encodings in one map: SSO, heap, a symbol and numbers.
            js_map_set(map, symbol, 1.0);
            js_map_set(map, short(b"beta"), 2.0);
            js_map_set(map, heap(b"gamma"), 3.0);
            js_map_set(map, heap(b"category_long"), 4.0);
            js_map_set(map, 42.0, 5.0);
            for i in 0..pad {
                js_map_set(map, heap(format!("pad_{i}").as_bytes()), i as f64);
            }
            js_map_set(map, short(b""), 6.0);
            let mut keys: Vec<f64> = Vec::new();
            for w in words {
                keys.push(heap(w));
                keys.push(short(w));
            }
            keys.push(heap(b"category_long"));
            keys.push(heap(b"category_lonG"));
            keys.push(heap(b"pad_7"));
            keys.push(heap(b"pad_99"));
            for key in keys {
                unsafe {
                    let expected = reference_index(map, key);
                    assert_eq!(
                        find_key_index(map, key),
                        expected,
                        "pad={pad} key bits {:#x}",
                        key.to_bits()
                    );
                }
            }
            // A description-less symbol must never collide with "" (#4570).
            assert_eq!(js_map_get(map, short(b"")), 6.0);
            assert_eq!(js_map_get(map, heap(b"")), 6.0);
            assert_eq!(js_map_get(map, symbol), 1.0);
        }
    }
}
