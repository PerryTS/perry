use super::*;
use crate::string::{js_string_char_code_at, js_string_from_str, test_utf16_index_entries};

#[test]
fn utf16_index_scanner_is_registered() {
    gc_init();
    assert!(
        MUTABLE_ROOT_SCANNERS.with(|scanners| scanners.borrow().iter().any(|entry| {
            entry.scanner as usize
                == crate::string::scan_utf16_index_roots_mut as MutableRootScanner as usize
        }))
    );
}

#[test]
fn utf16_index_follows_a_moved_string_without_retaining_dead_strings() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(crate::string::scan_utf16_index_roots_mut);
    let scope = RuntimeHandleScope::new();
    let text = "ä中😀Ö".repeat(256);
    let original = js_string_from_str(&text);
    assert!(crate::arena::pointer_in_nursery(original as usize));
    let live = scope.root_string_ptr(original);
    assert_eq!(js_string_char_code_at(original, 1279), 214.0);
    let before = test_utf16_index_entries();
    assert!(before
        .iter()
        .any(|&(owner, checkpoints)| owner == original as usize && checkpoints > 0));

    let dead = js_string_from_str(&"界🦀".repeat(256));
    assert_eq!(js_string_char_code_at(dead, 767), 0xdd80 as f64);
    assert!(test_utf16_index_entries()
        .iter()
        .any(|&(owner, _)| owner == dead as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    live.with_const_ptr(|moved: *const crate::StringHeader| {
        assert_ne!(
            moved as usize, original as usize,
            "fixture must actually move"
        );
        let entries = test_utf16_index_entries();
        let count = before
            .iter()
            .find(|&&(owner, _)| owner == original as usize)
            .unwrap()
            .1;
        assert!(
            entries.contains(&(moved as usize, count)),
            "relocation must preserve checkpoints"
        );
        assert!(
            !entries.iter().any(|&(owner, _)| owner == dead as usize),
            "cache must be weak"
        );
        for i in [1279, 0, 1277, 1278, 513, 1] {
            assert_eq!(
                js_string_char_code_at(moved, i),
                text.encode_utf16().nth(i as usize).unwrap() as f64
            );
        }
    });
}

#[test]
fn utf16_index_is_pruned_before_full_sweep_reuses_addresses() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(crate::string::scan_utf16_index_roots_mut);
    let s = js_string_from_str(&"中😀".repeat(256));
    assert_eq!(js_string_char_code_at(s, 767), 0xde00 as f64);
    assert!(!test_utf16_index_entries().is_empty());
    let force_marks = crate::gc::block_persist_force_mark_count();
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    assert_eq!(
        crate::gc::block_persist_force_mark_count(),
        force_marks,
        "fixture must not share a persisting block with a live object"
    );
    assert!(
        test_utf16_index_entries().is_empty(),
        "dead strings must release index storage"
    );
}
