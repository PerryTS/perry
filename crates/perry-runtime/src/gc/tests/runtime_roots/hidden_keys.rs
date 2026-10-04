//! `node_stream_readwrite.rs`'s `HIDDEN_KEYS` caches one longlived string per
//! hidden-field literal per thread, by raw address (#11828). The longlived
//! arena is not immortal: a full mark-sweep reclaims an unmarked longlived
//! object like any other (`gc/oldgen/sweep_objects.rs`, `process_object`), and
//! `arena_alloc_gc_longlived` documents that "the cache's root scanner keeps
//! them marked". So the table needs a registered scanner that MARKS the keys
//! and REWRITES the addresses, and both halves are asserted here, plus the
//! wiring through the real registry and a real `gc()`.

use super::*;
use crate::StringHeader;

static KEY_A: &[u8] = b"__perryHiddenKeyRootTestA";
static KEY_B: &[u8] = b"__perryHiddenKeyRootTestB";
static KEY_R: &[u8] = b"__perryHiddenKeyRootTestR";

fn key_bytes(key: *const StringHeader) -> Vec<u8> {
    unsafe {
        std::slice::from_raw_parts(crate::string::string_data(key), (*key).byte_len as usize)
            .to_vec()
    }
}

/// MARK. Nothing but the cache references these strings, so an unmarked key is
/// a swept key, and `hidden_key` would hand it out forever after.
#[test]
fn hidden_key_cache_is_marked_by_its_scanner() {
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let a = crate::node_stream::hidden_key_for_test(KEY_A) as usize;
    let b = crate::node_stream::hidden_key_for_test(KEY_B) as usize;
    clear_marks();
    clear_mark_seeds();
    let valid_ptrs = build_valid_pointer_set();

    crate::node_stream::hidden_key_root_scanner(&mut RuntimeRootVisitor::for_mark(&valid_ptrs));

    assert_marked_user_ptr(a, "hidden key A (only HIDDEN_KEYS references it)");
    assert_marked_user_ptr(b, "hidden key B (only HIDDEN_KEYS references it)");
    clear_marks();
    clear_mark_seeds();
}

/// REWRITE. Marking keeps the string alive; only the rewrite makes the cached
/// address name the surviving copy if the string is ever relocated.
#[test]
fn hidden_key_cache_is_rewritten_by_its_scanner() {
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let from = crate::node_stream::hidden_key_for_test(KEY_R);
    let size = unsafe { (*header_from_user_ptr(from as *const u8)).size as usize };
    let valid_ptrs = build_valid_pointer_set();
    let to = crate::arena::arena_alloc_gc_old(size, 8, GC_TYPE_STRING);
    let first_word = unsafe { *(from as *const usize) };
    unsafe {
        let payload = size.min(std::mem::size_of::<StringHeader>() + KEY_R.len());
        std::ptr::copy_nonoverlapping(from as *const u8, to, payload);
        set_forwarding_address(header_from_user_ptr(from as *const u8), to);
    }

    crate::node_stream::hidden_key_root_scanner(&mut RuntimeRootVisitor::for_rewrite(&valid_ptrs));
    let rewritten = crate::node_stream::hidden_key_peek_for_test(KEY_R);

    // Undo the hand-built evacuation (the cache entry and the from-space
    // header) so later tests on this thread see an ordinary longlived key.
    unsafe {
        *(from as *mut usize) = first_word;
        (*header_from_user_ptr(from as *const u8)).gc_flags &= !GC_FLAG_FORWARDED;
    }
    crate::node_stream::hidden_key_set_for_test(KEY_R, from as usize);

    assert_eq!(
        rewritten,
        Some(to as usize),
        "HIDDEN_KEYS must be rewritten to the relocated key, or every later \
         set_hidden_value/get_hidden_value names a from-space string"
    );
}

/// WIRING + a real `gc()`. Taking a key registers the scanner; the registered
/// set (not a direct call) must mark the key, and after a full mark-sweep the
/// cache still hands out the same, intact string.
#[test]
fn hidden_key_scanner_is_registered_and_survives_gc() {
    crate::gc::gc_init();
    let key = crate::node_stream::hidden_key_for_test(KEY_B);
    let scanner = crate::node_stream::hidden_key_scanner_for_test();
    let registered: Vec<MutableRootScanner> = crate::gc::roots::MUTABLE_ROOT_SCANNERS
        .with(|s| s.borrow().iter().map(|entry| entry.scanner).collect());
    assert!(
        registered.iter().any(|s| *s as usize == scanner as usize),
        "taking a hidden key must register hidden_key_root_scanner; unregistered, \
         the first full mark-sweep reclaims every cached key"
    );

    {
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        clear_marks();
        clear_mark_seeds();
        let valid_ptrs = build_valid_pointer_set();
        let mut visitor = RuntimeRootVisitor::for_mark(&valid_ptrs);
        for scan in &registered {
            scan(&mut visitor);
        }
        assert_marked_user_ptr(key as usize, "hidden key via the registered scanners");
        clear_marks();
        clear_mark_seeds();
    }

    crate::gc::js_gc_collect();

    let after = crate::node_stream::hidden_key_for_test(KEY_B);
    assert_eq!(
        after, key,
        "a longlived hidden key must not move across gc()"
    );
    assert_eq!(key_bytes(after), KEY_B.to_vec());
    unsafe {
        let header = header_from_user_ptr(after as *const u8);
        assert_eq!(
            (*header).obj_type,
            GC_TYPE_STRING,
            "hidden key header survived gc()"
        );
    }
}
