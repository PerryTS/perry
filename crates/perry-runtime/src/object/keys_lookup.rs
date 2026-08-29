//! Keys-array slot lookup helpers, in a sibling file.
//!
//! Extracted from `object/mod.rs` to keep it under the repo's 2000-line cap.
//! A child module, so these still reach the parent's private items through
//! `use super::*`. Moved verbatim apart from sharing one payload-offset helper.

use super::*;

/// The payload bytes of a `StringHeader`, in one place, so the key helpers
/// here do not each add a site to the payload-access ratchet.
#[inline(always)]
pub(crate) unsafe fn string_header_payload(key: *const crate::StringHeader) -> *const u8 {
    (key as *const u8).add(std::mem::size_of::<crate::StringHeader>())
}

/// Raw dense-slot view of a (validated) keys array: resolve a grow-forward
/// pointer ONCE, then hand back the backing slots for direct indexing. The
/// generic `js_array_get` element getter re-runs the whole per-element
/// gauntlet — forward-resolution, lazy/Map/Set receiver probes (each a TLS +
/// registry HashMap hit), descriptor gates — on EVERY slot, which made the
/// keys_array scan loops (`own_key_present`, the sidecar/wide-index builds)
/// pay ~µs per element. Callers have already validated `keys` is a
/// `GC_TYPE_ARRAY`; keys arrays are dense (no holes), and a slot that is not
/// a string simply fails the key match. (#6748 grind)
#[inline]
pub(crate) unsafe fn keys_array_dense_slots(
    keys: *const crate::array::ArrayHeader,
) -> (*const f64, usize) {
    let arr = crate::array::clean_arr_ptr(keys);
    if arr.is_null() {
        return (std::ptr::null(), 0);
    }
    let len = (*arr).length.min((*arr).capacity) as usize;
    (
        (arr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const f64,
        len,
    )
}

/// FNV-1a hash of the bytes behind a string header. Same hash function
/// as `key_content_hash_impl` so callers can mix paths.
#[inline(always)]
pub(crate) fn key_bytes_hash(name_ptr: *const u8, name_len: usize) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    unsafe {
        for i in 0..name_len {
            h ^= *name_ptr.add(i) as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// Find `key_bytes` among the first `key_count` keys of `keys`.
///
/// The [[Set]]/[[Get]] fallback walks used to do this with a per-element
/// `js_array_get` + `js_string_key_matches` loop — the full JS-facing array
/// accessor (pointer cleaning, typed-array and buffer registry probes,
/// descriptor gates) per element, per property access. A computed-key site
/// allocates a fresh key string every evaluation, so the pointer-keyed read
/// plan in front of those walks never hits and every access paid the scan:
/// measured 90.8 MILLION `js_array_get_f64` calls for 1.5 M property
/// operations (~60 per access) on the dynamic-property benchmark.
///
/// Strategy: the shared shape index (`shape_slot_lookup`, content-validated,
/// built once per shape) answers in O(1) for receivers at or above
/// `KEYS_INDEX_THRESHOLD`; below it — and as a correctness fallback if the
/// index declines — a linear scan over the DENSE raw slots
/// (`keys_array_dense_slots`, no per-element accessor) does the compare.
pub(crate) unsafe fn keys_find_slot_by_bytes(
    keys: *const crate::array::ArrayHeader,
    key_count: u32,
    key_bytes: &[u8],
) -> Option<u32> {
    if key_count >= KEYS_INDEX_THRESHOLD {
        let h = key_bytes_hash(key_bytes.as_ptr(), key_bytes.len());
        // build=false — consult-only. These call sites run on delete-churn
        // workloads where every delete drops the index; rebuilding it on the
        // next access (500 hashes) to use it once DOUBLED delete-heavy time
        // (1570 -> 3064 ms measured). Appends maintain the index incrementally
        // (shape_note_append), so stable-shape workloads still hit; churny
        // ones fall back to the raw dense scan below instead of thrashing.
        match shapes::shape_slot_lookup_verdict(keys, key_bytes, h, key_count, false) {
            shapes::KeysIndexVerdict::Found(slot) => return Some(slot),
            // A COMPLETE index (indexed_len == key_count) proves absence:
            // every present key is indexed, holes index as nothing, and a
            // stale bucket entry for a tombstoned key fails its content
            // validation without disproving completeness. Skipping the
            // backstop here is what makes tombstone-delete churn cheap — the
            // re-add's find-before-append otherwise linear-scanned up to 2x
            // the live keys per delete (60.4% of the flag-on
            // bench_populated_delete profile in one symbol).
            shapes::KeysIndexVerdict::Absent => return None,
            shapes::KeysIndexVerdict::Unindexed => {}
        }
    }
    let (slots, slot_len) = keys_array_dense_slots(keys);
    if slots.is_null() {
        return None;
    }
    let n = (key_count as usize).min(slot_len);
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    for i in 0..n {
        let v = crate::JSValue::from_bits((*slots.add(i)).to_bits());
        if let Some(stored) = crate::string::js_string_key_bytes(v, &mut sso) {
            if stored == key_bytes {
                return Some(i as u32);
            }
        }
    }
    None
}

/// [`keys_find_slot_by_bytes`] for a key held as a `StringHeader`.
pub(crate) unsafe fn keys_find_slot_by_key_ptr(
    keys: *const crate::array::ArrayHeader,
    key_count: u32,
    key: *const crate::StringHeader,
) -> Option<u32> {
    // Magnitude only, deliberately: the original `< 0x10000` rejected the
    // handle band and nothing else, and this helper's callers tolerate a
    // `key` that is not a valid header (the length guard below catches it).
    if key.is_null() || !crate::value::addr_class::is_above_handle_band(key as usize) {
        return None;
    }
    // The callers this replaced tolerated a `key` that is not actually a
    // valid string header: `js_string_key_matches` compares LENGTHS first, so
    // a garbage `byte_len` was just a harmless mismatch. Building a slice from
    // that length instead reads it — the first version of this helper panicked
    // in an unrelated stream test with `range start index 2613749136200`.
    // Keep the old tolerance: a length that cannot be a real key falls back to
    // the length-guarded per-candidate compare below.
    let len = (*key).byte_len as usize;
    if len <= (*key).capacity as usize && len < (1 << 28) {
        let data = string_header_payload(key);
        return keys_find_slot_by_bytes(keys, key_count, std::slice::from_raw_parts(data, len));
    }
    let (slots, slot_len) = keys_array_dense_slots(keys);
    if slots.is_null() {
        return None;
    }
    let n = (key_count as usize).min(slot_len);
    for i in 0..n {
        let v = crate::JSValue::from_bits((*slots.add(i)).to_bits());
        if crate::string::js_string_key_matches(v, key) {
            return Some(i as u32);
        }
    }
    None
}

/// Locate `key` in `obj`'s keys array via the shape record (#6759 C1:
/// keyed on keys_array identity — shared across same-shape objects —
/// replacing the per-object sidecar). Returns `Some(slot)` on a
/// content-validated hit, `None` on miss (caller falls through to
/// append/grow or the linear scan).
#[inline]
pub(crate) unsafe fn keys_index_lookup(
    _obj: *const ObjectHeader,
    keys: *const crate::array::ArrayHeader,
    key_bytes: &[u8],
    key_hash: u64,
) -> Option<u32> {
    let key_count = crate::array::js_array_length(keys);
    if key_count < KEYS_INDEX_THRESHOLD {
        return None;
    }
    shapes::shape_slot_lookup(keys, key_bytes, key_hash, key_count, true)
}

/// Record a new (key_hash → slot) entry on the POST-append keys array's
/// shape after a key was appended. Caller passes `crate::object::object_keys_array(obj)`
/// (the definitive post-append array — a clone or grow-realloc lands
/// under its new identity, or nowhere if no shape entry exists yet) and
/// ensures `new_count` equals the new keys_array length.
#[inline]
pub(crate) fn keys_index_insert(
    keys: *const crate::array::ArrayHeader,
    new_count: u32,
    key_hash: u64,
    slot: u32,
) {
    if new_count < KEYS_INDEX_THRESHOLD {
        return;
    }
    shapes::shape_note_append(keys, new_count, key_hash, slot);
}
