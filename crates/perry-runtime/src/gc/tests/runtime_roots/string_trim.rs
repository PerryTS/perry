use super::*;
use crate::string::{self, trim_cache};

fn assert_payload(source: *const crate::StringHeader, expected: &[u8]) {
    assert_eq!(
        unsafe { string::OwnedStringBytes::copy_from_header(source) }.as_bytes(),
        expected
    );
}

struct TrimCacheGuard;

impl TrimCacheGuard {
    fn new() -> Self {
        trim_cache::test_clear_trim_cache();
        Self
    }
}

impl Drop for TrimCacheGuard {
    fn drop(&mut self) {
        trim_cache::test_clear_trim_cache();
    }
}

#[test]
fn trim_cache_keeps_and_rewrites_both_strings_during_copying_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _cache = TrimCacheGuard::new();
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(trim_cache::scan_trim_cache_roots_mut);

    let bytes = format!(" \t{}\n ", "aBcD".repeat(1000));
    let source = string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    let result = string::js_string_trim(source);
    assert!(crate::arena::pointer_in_nursery(source as usize));
    assert!(crate::arena::pointer_in_nursery(result as usize));

    // The cache is the only registered root for either string. The raw stack
    // locals deliberately cannot keep them alive under this test guard.
    let _ = gc_collect_minor();
    let (source_now, result_now) = trim_cache::test_trim_cache_pair();
    assert_ne!(source_now, source, "the source must actually move");
    assert_ne!(result_now, result, "the result must actually move");
    assert_payload(source_now, bytes.as_bytes());
    assert_payload(result_now, &bytes.as_bytes()[2..bytes.len() - 2]);
    assert_eq!(string::js_string_trim(source_now), result_now);
}

#[test]
fn trim_roots_source_across_destination_allocation() {
    let _legacy_pacing = crate::gc::policy::force_legacy_gc_pacing();
    let _guard = CopyingNurseryTestGuard::new(0);
    let triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _cache = TrimCacheGuard::new();
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(trim_cache::scan_trim_cache_roots_mut);

    let bytes = format!(" \t{}\n ", "ä中😀Ö".repeat(300));
    let source = string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    assert!(crate::arena::pointer_in_nursery(source as usize));
    force_next_general_arena_alloc_slow();
    triggers.make_arena_trigger_due();
    let before = gc_collection_count();
    let result = string::js_string_trim(source);
    let scope = RuntimeHandleScope::new();
    let result_root = scope.root_string_ptr(result);
    drain_scheduled_minor_gc(before, "trim destination allocation");
    let result_now = result_root.get_raw_const_ptr::<crate::StringHeader>();
    assert_payload(result_now, &bytes.as_bytes()[2..bytes.len() - 2]);
    assert_eq!(unsafe { (*result_now).utf16_len }, 1500);
    let (source_now, cached_result) = trim_cache::test_trim_cache_pair();
    assert_payload(source_now, bytes.as_bytes());
    assert_eq!(cached_result.cast_const(), result_now);
}

#[test]
fn trim_cache_scanner_is_registered() {
    crate::gc::gc_init();
    assert!(crate::gc::roots::MUTABLE_ROOT_SCANNERS.with(|scanners| {
        scanners.borrow().iter().any(|entry| {
            entry.scanner as usize
                == trim_cache::scan_trim_cache_roots_mut as MutableRootScanner as usize
        })
    }));
}
