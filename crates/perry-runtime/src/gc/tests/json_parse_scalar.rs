use super::super::*;
use super::support::*;
use crate::gc::policy::{GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING, GC_TRIGGER_ARMED};

struct ParseStateGuard {
    pending: bool,
    flags: u8,
    armed: bool,
    before_suppression: usize,
}

impl ParseStateGuard {
    fn new() -> Self {
        Self {
            pending: GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|c| c.replace(false)),
            flags: GC_FLAGS.with(|c| c.get()),
            armed: GC_TRIGGER_ARMED.with(|c| c.replace(false)),
            before_suppression: GC_PRE_SUPPRESS_BYTES.with(|c| c.replace(123)),
        }
    }
}

impl Drop for ParseStateGuard {
    fn drop(&mut self) {
        GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|c| c.set(self.pending));
        GC_FLAGS.with(|c| c.set(self.flags));
        GC_TRIGGER_ARMED.with(|c| c.set(self.armed));
        GC_PRE_SUPPRESS_BYTES.with(|c| c.set(self.before_suppression));
    }
}

#[test]
fn json_scalar_parse_preserves_blocked_debt_and_services_it_when_safe() {
    let _isolation = GcTestIsolationGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _state = ParseStateGuard::new();
    register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    for (text, fallible) in [(b"null".as_slice(), false), (b"\"a\"".as_slice(), true)] {
        let input = crate::js_string_from_bytes(text.as_ptr(), text.len() as u32);
        let root = scope.root_string_ptr(input);
        let parse = || unsafe {
            if fallible {
                crate::json::js_json_parse_result(root.get_raw_const_ptr()).unwrap()
            } else {
                crate::json::js_json_parse(root.get_raw_const_ptr())
            }
        };
        GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|c| c.set(true));
        let old_flags = GC_FLAGS.with(|c| c.replace(c.get() | GC_FLAG_SUPPRESSED));
        let before = gc_collection_count();
        let value = parse();
        assert!(value.is_null() || value.is_short_string());
        assert_eq!(gc_collection_count(), before);
        assert!(GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|c| c.get()));
        GC_FLAGS.with(|c| c.set(old_flags));

        // Model the raise-only post-parse threshold: the debt hook must lower
        // and arm it. A just-due threshold with the arm manually cleared is
        // inconsistent with the state that schedules this debt.
        GC_NEXT_TRIGGER_BYTES.with(|c| c.set(usize::MAX));
        GC_TRIGGER_ARMED.with(|c| c.set(false));
        let value = parse();
        assert!(value.is_null() || value.is_short_string());
        assert!(!GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|c| c.get()));
        assert!(
            gc_collection_count() > before
                || gc_budgeted_cycle_active()
                || GC_TRIGGER_ARMED.with(|c| c.get()),
            "pending work must reach the collector"
        );
    }
}

#[test]
fn json_scalar_parse_allocates_no_managed_scratch_and_does_not_rebaseline() {
    let _isolation = GcTestIsolationGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _state = ParseStateGuard::new();
    let input = crate::js_string_from_bytes(b"true".as_ptr(), 4);
    let before = crate::arena::arena_total_bytes();
    let roots = RuntimeHandleScope::active_len_for_tests();
    let collections = gc_collection_count();
    for _ in 0..1000 {
        assert!(unsafe { crate::json::js_json_parse(input) }.as_bool());
        assert!(unsafe { crate::json::js_json_parse_result(input) }
            .unwrap()
            .as_bool());
    }
    assert_eq!(crate::arena::arena_total_bytes(), before);
    assert_eq!(RuntimeHandleScope::active_len_for_tests(), roots);
    assert_eq!(gc_collection_count(), collections);
    assert_eq!(GC_PRE_SUPPRESS_BYTES.with(|c| c.get()), 123);
}
