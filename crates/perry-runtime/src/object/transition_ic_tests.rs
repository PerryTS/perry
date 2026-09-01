//! #9367 transition-IC tests, split out of `tests.rs` to keep it under the
//! 2000-line cap — same reason as `own_key_probe_tests`.
#![cfg(test)]

use super::*;

/// #9287: the emitted transition-IC probe replicates `transition_cache_slot`
/// and `transition_key_id` as raw arithmetic in codegen. Drift there is
/// invisible in program output — a probe that hashes to the wrong slot is
/// correct, only slow — so the contract is pinned here.
///
/// This is not hypothetical: the probe first shipped with a hand-transcribed
/// decimal for `TRANSITION_HASH_MUL_KEY` that was off, and every write missed
/// while the benchmark still printed the right answer.

#[test]
fn emitted_transition_probe_matches_runtime_slot() {
    // Transcribed from `lower_put_value_dyn_ic_inline`'s trans.entry block.
    fn emitted_slot(shape_id: u32, key_id: u64) -> usize {
        let h1 = (shape_id as u64).wrapping_mul(TRANSITION_HASH_MUL_SHAPE);
        let h2 = (key_id >> 3).wrapping_mul(TRANSITION_HASH_MUL_KEY);
        ((h1 ^ h2) & 16383) as usize
    }
    // The emitted content id: an i64 load at key+20 masked down to byte_len.
    fn emitted_key_id(bytes: &[u8]) -> u64 {
        let mut word = [0u8; 8];
        word[..bytes.len()].copy_from_slice(bytes);
        u64::from_le_bytes(word) & (u64::MAX >> ((8 - bytes.len() as u32) * 8))
    }

    for (shape_id, key) in [
        (0x8000_0002u32, "field_0"),
        (0x8000_0003, "field_19"),
        (0xBFFF_FFFF, "abcdef"),
    ] {
        let s = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
        let (kid, marker) = transition_key_id(s);
        assert_eq!(
            marker,
            key.len() as u32,
            "{key} must take the content namespace"
        );
        assert_eq!(
            kid as u64,
            emitted_key_id(key.as_bytes()),
            "content id drift for {key}"
        );
        assert_eq!(
            transition_cache_slot(shape_id, kid),
            emitted_slot(shape_id, kid as u64),
            "slot hash drift for ({shape_id:#x}, {key})"
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
fn transition_cache_hits_a_content_entry_through_a_fresh_key() {
    const PREDECESSOR: u32 = 0x8000_0301;
    const TARGET: u32 = 0x8000_0302;
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
        Some((keys as usize, 0, TARGET)),
        "a fresh key with equal bytes must reach the cached edge"
    );

    let slot = transition_cache_slot(PREDECESSOR, transition_key_id(inserted).0);
    with_transition_cache(|t| unsafe {
        // GC_STORE_AUDIT(ROOT): test cleanup writes non-pointer sentinels into scanned TRANSITION_CACHE_GLOBAL roots.
        (*t)[slot] = TransitionEntry {
            key_ptr: 0,
            next_keys: 0,
            prev_shape_id: 0,
            target_shape_id: 0,
            slot_idx: 0,
            target_len: 0,
        };
    });
}
