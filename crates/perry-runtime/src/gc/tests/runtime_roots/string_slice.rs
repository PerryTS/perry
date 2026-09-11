use super::*;

#[test]
fn suffix_cursor_offsets_survive_source_evacuation_and_split_slice_owns_its_bytes() {
    use crate::string::suffix_cursor::*;
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _mode =
        crate::arena::ProtectionModeGuard::set(crate::arena::FromSpaceProtection::PoisonOnly);
    register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    let bytes = "ä中😀Ö".repeat(100);
    let source = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    assert!(crate::arena::pointer_in_nursery(source as usize));
    let root = scope.root_string_ptr(source);
    let before = source as usize;
    let boxed = |s| f64::from_bits(crate::value::JSValue::string_ptr(s).bits());
    let mut cursor = SuffixCursor::default();
    unsafe {
        js_string_suffix_advance(boxed(source), &mut cursor, 3);
    }
    let kept = crate::string::js_string_slice(source, 3, 5);
    let kept_root = scope.root_string_ptr(kept);
    gc_collect_minor();
    let source = root.get_raw_mut_ptr::<crate::StringHeader>();
    assert_ne!(
        source as usize, before,
        "the test must actually move the source"
    );
    unsafe {
        assert_eq!(js_string_suffix_length(boxed(source), &cursor), 497.0);
        assert_eq!(
            js_string_suffix_char_code_at(boxed(source), &cursor, 0),
            56832.0
        );
        js_string_suffix_advance(boxed(source), &mut cursor, 1);
        assert_eq!(
            js_string_suffix_char_code_at(boxed(source), &cursor, 0),
            214.0
        );
    }
    let kept = kept_root.get_raw_const_ptr::<crate::StringHeader>();
    assert_eq!(crate::string::js_string_char_code_at(kept, 0), 56832.0);
    assert_eq!(crate::string::js_string_char_code_at(kept, 1), 214.0);
}

/// #5062: `String.prototype.slice` copies the selected range out of the source
/// string AFTER allocating the destination, via a raw pointer derived from the
/// source (`string_data(s) + offset`). If that destination allocation trips a
/// moving/sweeping GC, the source is relocated/freed out from under the raw
/// pointer and the copy reads stale memory — under sustained server GC pressure
/// this corrupted the head of chunked slices (the new slice's own length word
/// got stamped over its first bytes). `string_copy_range` now roots the source
/// in a `RuntimeHandleScope`, so it survives and relocates across the
/// allocation. This mirrors the proven concat / dynamic-add tests in the parent.
#[test]
fn test_transient_runtime_handle_string_slice_gc() {
    let _legacy_pacing = crate::gc::policy::force_legacy_gc_pacing();
    let _guard = CopyingNurseryTestGuard::new(0);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    // Distinctive ASCII payload (`b'A' + i % 26`) so a corrupted head is
    // unmistakable; ASCII is the fast path the bug report exercises (base64).
    // Keep the source under LARGE_OBJECT_THRESHOLD_BYTES so it lives in the
    // movable nursery (large strings go straight to non-moving old-gen).
    const SRC_LEN: usize = 4_000;
    const SLICE_START: usize = 100;
    const SLICE_LEN: usize = 3_000;
    let src_bytes: Vec<u8> = (0..SRC_LEN as u32).map(|i| b'A' + (i % 26) as u8).collect();
    let s = crate::string::js_string_from_bytes(src_bytes.as_ptr(), src_bytes.len() as u32);
    assert!(crate::arena::pointer_in_nursery(s as usize));

    // Force the slice's destination allocation onto the slow path with a GC
    // trigger already due, so a copying-minor GC runs during `js_string_slice`.
    // `s` is held only as a native-stack local and `CopyingNurseryTestGuard`
    // disables conservative stack scanning — so without the handle root inside
    // `string_copy_range`, the source would be swept/relocated out from under
    // the in-flight copy pointer.
    force_next_general_arena_alloc_slow();
    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();
    let result =
        crate::string::js_string_slice(s, SLICE_START as i32, (SLICE_START + SLICE_LEN) as i32);

    let result_scope = RuntimeHandleScope::new();
    let result_root = result_scope.root_string_ptr(result);
    drain_scheduled_minor_gc(before, "slice destination allocation");
    let result = result_root.get_raw_const_ptr::<crate::StringHeader>();

    unsafe {
        assert_eq!((*result).byte_len, SLICE_LEN as u32);
        assert_eq!((*result).utf16_len, SLICE_LEN as u32);
        let data = (result as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        // Every byte must equal the live source range — the head especially,
        // where the corruption manifested as a leaked length word.
        for k in 0..SLICE_LEN {
            let expected = b'A' + (((SLICE_START + k) as u32) % 26) as u8;
            assert_eq!(*data.add(k), expected, "slice byte {k} corrupted");
        }
    }
}
