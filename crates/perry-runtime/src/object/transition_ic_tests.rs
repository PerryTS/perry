//! #9367 transition-IC tests, split out of `tests.rs` to keep it under the
//! 2000-line cap — same reason as `own_key_probe_tests`.
#![cfg(test)]

use super::*;
use crate::codegen_abi::LayoutWord;

/// Every payload byte and the length must affect packed-key placement. The
/// old low-bit mask ignored later bytes and forced literal/literals to
/// collide throughout Effect schema construction, at every load address.
#[cfg(target_pointer_width = "64")]
#[test]
fn content_transition_slots_use_every_payload_byte_and_length() {
    for (a, b) in [
        (b"literal".as_slice(), b"literals".as_slice()),
        (b"aField".as_slice(), b"bField".as_slice()),
        (b"aaaaaaaA".as_slice(), b"aaaaaaaB".as_slice()),
        (b"abcdef".as_slice(), b"abcdef\0".as_slice()),
    ] {
        let a = crate::string::js_string_from_bytes(a.as_ptr(), a.len() as u32);
        let b = crate::string::js_string_from_bytes(b.as_ptr(), b.len() as u32);
        let (aid, alen) = transition_key_id(a);
        let (bid, blen) = transition_key_id(b);
        assert_ne!(
            transition_cache_slot(LayoutWord(0x8000_0042), aid, alen),
            transition_cache_slot(LayoutWord(0x8000_0042), bid, blen),
            "content differences must not be discarded before slot selection",
        );
    }
}

/// The content namespace is EXACT identity, not a hash: equal bytes are the
/// same property name, so two distinct string objects must share one entry.
/// Keys outside 6..=8 bytes keep interned-pointer identity, and the length
/// marker keeps the two namespaces from ever cross-matching.
#[test]
fn transition_key_id_namespaces_are_exact_and_disjoint() {
    let a = crate::string::js_string_from_bytes(b"field_7".as_ptr(), 7);
    let b = crate::string::js_string_from_bytes(b"field_7".as_ptr(), 7);
    assert_ne!(a as usize, b as usize, "test needs two distinct objects");
    assert_eq!(
        transition_key_id(a),
        transition_key_id(b),
        "equal bytes must give one cache identity"
    );

    let c = crate::string::js_string_from_bytes(b"field_8".as_ptr(), 7);
    assert_ne!(transition_key_id(a).0, transition_key_id(c).0);

    // Past the content window: pointer identity, marker 0.
    let long = crate::string::js_string_from_bytes(b"field_longer".as_ptr(), 12);
    assert_eq!(transition_key_id(long), (long as usize, 0));
    // Below it: also pointer identity (these reach sites as SSO immediates,
    // which the emitted probe rejects at its string-tag check).
    let short = crate::string::js_string_from_bytes(b"ab".as_ptr(), 2);
    assert_eq!(transition_key_id(short), (short as usize, 0));
}

/// A content-namespace entry must be reachable by a DIFFERENT string object
/// with the same bytes — the property the emitted probe relies on, since its
/// key is freshly concatenated at every write and never the interned one.
#[test]
#[allow(non_snake_case)]
fn transition_cache_hits_a_content_entry_through_a_fresh_key() {
    // Real published words: the insert reads the predecessor's record, and a
    // word names a row that exists only once it is published.
    let _lock = crate::gc::global_side_table_test_lock();
    let predecessor_keys = 0x8067_0000_0000_3100usize;
    let target_keys = 0x8067_0000_0000_3200usize;
    let PREDECESSOR =
        shapes::shape_descriptor_ensure(predecessor_keys as *const ArrayHeader, 1, 1).unwrap();
    let TARGET = shapes::shape_descriptor_ensure(target_keys as *const ArrayHeader, 2, 2).unwrap();
    let inserted = crate::string::js_string_from_bytes(b"field_3".as_ptr(), 7);
    let keys = crate::array::js_array_alloc(4);
    let keys = crate::array::js_array_push(keys, JSValue::string_ptr(inserted));

    transition_cache_insert(
        std::ptr::null(),
        PREDECESSOR,
        inserted,
        keys as usize,
        0,
        TARGET,
    );

    let fresh = crate::string::js_string_from_bytes(b"field_3".as_ptr(), 7);
    assert_ne!(fresh as usize, inserted as usize);
    assert_eq!(
        transition_cache_lookup(PREDECESSOR, fresh),
        Some((
            crate::object::ObjectKeys::new(keys as *mut ArrayHeader, 1),
            0,
            TARGET
        )),
        "a fresh key with equal bytes must reach the cached edge"
    );

    let slot = transition_cache_slot(PREDECESSOR, transition_key_id(inserted).0, 7);
    with_transition_cache(|t| unsafe {
        // GC_STORE_AUDIT(ROOT): test cleanup writes non-pointer sentinels into scanned TRANSITION_CACHE_GLOBAL roots.
        (*t)[slot] = TransitionEntry {
            key_ptr: 0,
            next_keys: 0,
            prev_shape_id: LayoutWord(0),
            target_shape_id: LayoutWord(0),
            slot_idx: 0,
            target_len: 0,
        };
    });
    shapes::test_drop_shape_descriptors(predecessor_keys);
    shapes::test_drop_shape_descriptors(target_keys);
}
