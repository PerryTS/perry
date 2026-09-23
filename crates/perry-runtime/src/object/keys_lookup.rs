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
        crate::array::array_elements_ptr(arr as *const crate::array::ArrayHeader) as *const f64,
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
    // #10595: scan back-to-front. A subclass field that re-declares an
    // ancestor's field name (`class Sub extends Base { tag = ... }` where
    // `Base` also declares `tag`) is NOT deduplicated in the packed keys —
    // `codegen/mod.rs` lists ancestor fields first, then the class's own, so
    // the array holds one entry per DECLARATION, oldest ancestor first, most
    // derived last. `class_field_global_index` (the compile-time-typed read's
    // index resolver) already picks the most-derived declaration ("TS
    // shadowing"); this dynamic by-name lookup must agree, or a receiver
    // whose static type is unknown (an inherited accessor's `this.field`, a
    // computed `obj[key]`) sees the ancestor's stale slot instead of the
    // override. Scanning in reverse finds that same most-derived match first,
    // with no change to storage layout and no cost in the (common, no
    // shadowing) case where a name occurs once.
    for i in (0..n).rev() {
        let v = crate::JSValue::from_bits((*slots.add(i)).to_bits());
        if let Some(stored) = crate::string::js_string_key_bytes(v, &mut sso) {
            if stored == key_bytes {
                return Some(i as u32);
            }
        }
    }
    None
}

/// [`keys_array_dense_slots`] for a keys array the caller read out of a LIVE
/// `ShapeDescriptor`.
///
/// `descriptor.keys` is maintained by the COLLECTOR. When the keys array
/// moves, `shapes::scan_shape_table_rekey_mut` writes the forwarded address
/// back into every descriptor record in that family —
/// `unsafe { (*record).keys = addr as u64 }` — and a descriptor whose keys
/// array died is pruned in the same pass (`shape_keys_address_is_recycled`).
/// So the pointer read out of a live descriptor already IS the resolved live
/// head, and `clean_arr_ptr` on it re-derives a guarantee the collector has
/// already made.
///
/// Measured: `keys_array_dense_slots` was 16.2% of an `o[k]` read loop, and
/// `clean_arr_ptr` is what it spends that on.
///
/// # Safety
///
/// `keys` must be `ShapeDescriptor::keys` from a descriptor read on this same
/// straight-line path, with no allocation or safepoint since that read.
#[inline]
pub(crate) unsafe fn keys_array_dense_slots_resolved(
    keys: *const crate::array::ArrayHeader,
) -> (*const f64, usize) {
    if keys.is_null() {
        return (std::ptr::null(), 0);
    }
    let len = (*keys).length.min((*keys).capacity) as usize;
    (crate::array::array_elements_ptr(keys) as *const f64, len)
}

/// [`keys_find_slot_by_bytes`] for a keys array obtained from a live
/// descriptor — see [`keys_array_dense_slots_resolved`] for why the receiver
/// needs no second resolution.
///
/// # Safety
///
/// As [`keys_array_dense_slots_resolved`].
pub(crate) unsafe fn keys_find_slot_by_bytes_resolved(
    keys: *const crate::array::ArrayHeader,
    key_count: u32,
    key_bytes: &[u8],
) -> Option<u32> {
    if key_count >= KEYS_INDEX_THRESHOLD {
        // The indexed path owns its own receiver handling; hand it the
        // unresolved entry so its behaviour is bit-for-bit what it was.
        return keys_find_slot_by_bytes(keys, key_count, key_bytes);
    }
    let (slots, slot_len) = keys_array_dense_slots_resolved(keys);
    if slots.is_null() {
        return None;
    }
    let n = (key_count as usize).min(slot_len);
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    // #10595: back-to-front, like [`keys_find_slot_by_bytes`] above — a
    // subclass field that re-declares an ancestor's name holds two entries
    // in the packed keys array and only the LAST (most-derived) slot is ever
    // written. This resolved twin scanned FORWARD, so the one caller that
    // reaches it (`native_get::try_data_get_bytes`, the inherited/prototype
    // read) returned the never-initialised ancestor slot for exactly the
    // receiver #10595 fixed everywhere else.
    for i in (0..n).rev() {
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
    // #10595: same most-derived-wins scan direction as the fast path above.
    for i in (0..n).rev() {
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
    keys: ObjectKeys,
    key_bytes: &[u8],
    key_hash: u64,
) -> Option<u32> {
    let key_count = keys.count();
    if key_count < KEYS_INDEX_THRESHOLD {
        return None;
    }
    shapes::shape_slot_lookup(keys.arr(), key_bytes, key_hash, key_count, true)
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

#[cfg(test)]
mod tests_10595 {
    use super::*;

    /// #10595: a subclass field that re-declares an ancestor's field name is
    /// not deduplicated in the packed keys array built by
    /// `crates/perry-codegen/src/codegen/mod.rs` (ancestor fields first,
    /// then the class's own) — the array genuinely holds two entries for
    /// one logical property, oldest declaration first. Only the LAST
    /// (most-derived) slot is ever written, matching
    /// `class_field_global_index`'s "TS shadowing" resolution for the
    /// compile-time-typed path. A dynamic by-name lookup that returned the
    /// first match instead found the never-initialized ancestor slot.
    #[test]
    fn duplicate_key_name_resolves_to_the_last_occurrence() {
        let ancestor_key = crate::string::js_string_from_bytes(b"tag".as_ptr(), 3);
        let override_key = crate::string::js_string_from_bytes(b"tag".as_ptr(), 3);
        let keys = crate::array::js_array_alloc(4);
        let keys = crate::array::js_array_push(keys, JSValue::string_ptr(ancestor_key));
        let keys = crate::array::js_array_push(keys, JSValue::string_ptr(override_key));

        let lookup_key = crate::string::js_string_from_bytes(b"tag".as_ptr(), 3);
        unsafe {
            assert_eq!(
                keys_find_slot_by_key_ptr(keys, 2, lookup_key),
                Some(1),
                "must resolve to the most-derived slot (index 1), not the ancestor's (index 0)"
            );
            // `keys_find_slot_by_bytes` is the byte-slice twin the pointer
            // form delegates to for a valid header; pin it directly too.
            assert_eq!(
                keys_find_slot_by_bytes(keys, 2, b"tag"),
                Some(1),
                "byte-slice lookup must agree with the pointer-key lookup"
            );
            // The RESOLVED twin — the one `native_get::try_data_get_bytes`
            // reaches for an inherited read — scanned forward until it was
            // brought in line with the other two. Pinned separately because
            // its fast arm is a separate loop, not a delegation.
            assert_eq!(
                keys_find_slot_by_bytes_resolved(keys, 2, b"tag"),
                Some(1),
                "the resolved lookup must agree: most-derived slot (index 1), \
                 not the ancestor's (index 0)"
            );
        }
    }

    /// The common (non-shadowing) case — a name that occurs exactly once —
    /// must be completely unaffected by scanning in reverse.
    #[test]
    fn single_occurrence_key_is_unaffected_by_scan_direction() {
        let a = crate::string::js_string_from_bytes(b"x".as_ptr(), 1);
        let b = crate::string::js_string_from_bytes(b"y".as_ptr(), 1);
        let keys = crate::array::js_array_alloc(4);
        let keys = crate::array::js_array_push(keys, JSValue::string_ptr(a));
        let keys = crate::array::js_array_push(keys, JSValue::string_ptr(b));

        let lookup_x = crate::string::js_string_from_bytes(b"x".as_ptr(), 1);
        let lookup_y = crate::string::js_string_from_bytes(b"y".as_ptr(), 1);
        unsafe {
            assert_eq!(keys_find_slot_by_key_ptr(keys, 2, lookup_x), Some(0));
            assert_eq!(keys_find_slot_by_key_ptr(keys, 2, lookup_y), Some(1));
        }
    }

    /// A key that is genuinely absent must still miss, in both scan
    /// directions.
    #[test]
    fn absent_key_is_not_found() {
        let a = crate::string::js_string_from_bytes(b"tag".as_ptr(), 3);
        let keys = crate::array::js_array_alloc(4);
        let keys = crate::array::js_array_push(keys, JSValue::string_ptr(a));

        let lookup = crate::string::js_string_from_bytes(b"tagViaGetter".as_ptr(), 12);
        unsafe {
            assert_eq!(keys_find_slot_by_key_ptr(keys, 1, lookup), None);
        }
    }
}
