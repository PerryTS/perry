//! Moving-GC regressions for regex subject borrows (#8428, #8449).
//!
//! RegExpBuiltinExec step 4 is `ToLength(Get(R, "lastIndex"))`. When
//! `lastIndex` holds an object the ToNumber half runs OrdinaryToPrimitive —
//! user `valueOf`/`toString`, i.e. arbitrary JS, which reaches safepoints and
//! can run a copying minor. `js_regexp_exec` borrowed the subject's inline
//! WTF-8 payload (`string_as_str`) BEFORE that coercion, and rooting rewrites
//! slots, never an already materialized `&str` — so the whole match ran over
//! from-space bytes.
//!
//! The planted `valueOf` runs a copying minor and then refills the just
//! retired Eden with a distinctive pattern, so a pre-fix run matches against
//! the stomped bytes instead of the relocated subject and the match evaporates.
//! Liveness is asserted both ways (a copying minor ran; the subject actually
//! moved), per CLAUDE.md's "a gate must assert its subject was live".
//!
//! The #8449 witnesses plant the same moving minor at the first result-array
//! allocation, after the regex engine has returned captures borrowing the
//! subject. They cover the standard `exec` and fancy `String#match` paths.

use super::super::super::*;
use super::super::support::*;

use crate::array::ArrayHeader;
use crate::object::ObjectHeader;
use crate::regex::RegExpHeader;
use crate::string::StringHeader;

/// Stand-in for `{ valueOf() { …allocating user JS…; return 0 } }`.
extern "C" fn collect_then_zero(_closure: *const crate::closure::ClosureHeader) -> f64 {
    crate::gc::gc_collect_minor();
    // Recycle the just-retired from-space blocks back out as fresh strings, so
    // a stale borrow reads THESE bytes rather than the subject's old ones.
    let filler = [b'#'; 64];
    for _ in 0..64 {
        let _ = crate::string::js_string_from_bytes(filler.as_ptr(), filler.len() as u32);
    }
    0.0
}

fn heap_string(bytes: &[u8]) -> *mut StringHeader {
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

fn capture_text(element: f64, index: usize) -> String {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let (data, len) = crate::string::str_bytes_from_jsvalue(element, &mut scratch)
        .unwrap_or_else(|| panic!("capture {index} must be a string"));
    let bytes = unsafe { std::slice::from_raw_parts(data, len as usize) };
    String::from_utf8_lossy(bytes).into_owned()
}

fn assert_captures(matched: *mut ArrayHeader, expected: &[&str]) {
    assert!(!matched.is_null(), "regex must match the relocated subject");
    let scope = crate::gc::RuntimeHandleScope::new();
    let matched_handle = scope.root_raw_mut_ptr(matched);
    for (index, expected) in expected.iter().enumerate() {
        let element = matched_handle.with_mut_ptr::<ArrayHeader, _>(|arr| {
            crate::array::js_array_get_f64(arr, index as u32)
        });
        assert_eq!(
            capture_text(element, index),
            *expected,
            "capture {index} read from-space bytes"
        );
    }
}

#[test]
fn regexp_exec_survives_a_moving_minor_inside_the_lastindex_coercion() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();

    let scope = crate::gc::RuntimeHandleScope::new();

    // The subject must live in the movable nursery — a large string goes
    // straight to the non-moving old generation and the window never opens.
    let subject = heap_string(b"prefix-young-42-suffix");
    assert!(crate::arena::pointer_in_nursery(subject as usize));
    let s_handle = scope.root_string_ptr(subject);
    let subject_before = subject as usize;

    let re = crate::regex::js_regexp_new(heap_string(br"(young)-(\d+)"), heap_string(b"g"));
    let re_handle = scope.root_raw_mut_ptr(re);

    // `re.lastIndex = { valueOf() { … } }`.
    let coercer = crate::object::js_object_alloc(0, 1);
    let coercer_handle = scope.root_raw_mut_ptr(coercer);
    let fp = collect_then_zero as *const u8;
    crate::closure::js_register_closure_arity(fp, 0);
    let value_of = crate::closure::js_closure_alloc_singleton(fp);
    let value_of_value = crate::value::js_nanbox_pointer(value_of as i64);
    let value_of_key = heap_string(b"valueOf");
    coercer_handle.with_mut_ptr::<ObjectHeader, _>(|obj| {
        crate::object::js_object_set_field_by_name(obj, value_of_key, value_of_value)
    });
    let coercer_value = coercer_handle
        .with_mut_ptr::<ObjectHeader, _>(|obj| crate::value::js_nanbox_pointer(obj as i64));
    re_handle.with_mut_ptr::<RegExpHeader, _>(|hdr| unsafe {
        (*hdr).last_index = coercer_value.to_bits();
    });

    let cycles_before = copying_minor_cycles();
    let matched = s_handle.with_const_ptr::<StringHeader, _>(|subject_now| {
        re_handle.with_mut_ptr::<RegExpHeader, _>(|re_now| {
            crate::regex::js_regexp_exec(re_now, subject_now)
        })
    });
    let cycles_after = copying_minor_cycles();

    assert!(
        cycles_after > cycles_before,
        "subject not live: the lastIndex coercion must run a copying minor \
         (before={cycles_before}, after={cycles_after})"
    );
    let subject_after = s_handle.with_const_ptr::<StringHeader, _>(|p| p as usize);
    assert_ne!(
        subject_before, subject_after,
        "subject not live: the collection must actually move the subject string"
    );

    // The discriminating check: pre-fix the match ran over the retired
    // from-space copy of the subject, so `exec` returned null.
    assert!(
        !matched.is_null(),
        r"/(young)-(\d+)/g must match the relocated subject"
    );
    let matched_handle = scope.root_raw_mut_ptr(matched);
    for (index, expected) in ["young-42", "young", "42"].into_iter().enumerate() {
        let element = matched_handle.with_mut_ptr::<ArrayHeader, _>(|arr| {
            crate::array::js_array_get_f64(arr, index as u32)
        });
        assert_eq!(
            capture_text(element, index),
            expected,
            "capture {index} read from-space bytes"
        );
    }
    let last_index =
        re_handle.with_const_ptr::<RegExpHeader, _>(crate::regex::regex_last_index_offset);
    assert_eq!(
        last_index, 15,
        "lastIndex must advance past the relocated match"
    );
}

/// #8449: once the engine has produced `Captures`, the result-array allocation
/// itself may move the subject. The captures borrow the subject payload, so all
/// ranges and UTF-16 indices must be snapshotted before this planted collection.
///
/// SABOTAGE CHECK: restoring the pre-fix `caps`/`str_data` materialization in
/// `js_regexp_exec` makes this fail after the subject moves at `js_array_alloc`.
#[test]
fn regexp_exec_materializes_an_owned_snapshot_after_an_alloc_point_minor() {
    let _guard = CopyingNurseryTestGuard::new(4);
    let _pacing = crate::gc::policy::force_alloc_point_minor_pacing();
    let _scan = ConservativeScanDisabledGuard::new();
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    let scope = crate::gc::RuntimeHandleScope::new();
    let subject = heap_string(b"prefix-young-42-suffix");
    assert!(crate::arena::pointer_in_nursery(subject as usize));
    let subject_handle = scope.root_string_ptr(subject);
    let subject_before = subject as usize;

    let re = crate::regex::js_regexp_new(heap_string(br"(?<word>young)-(\d+)"), heap_string(b"gd"));
    let re_handle = scope.root_raw_mut_ptr(re);

    // The next general-arena block allocation is the result array created only
    // after the engine has matched and Phase 1 has snapshotted the captures.
    super::force_next_general_arena_alloc_slow();
    trigger_guard.make_arena_trigger_due();
    let collections_before = gc_collection_count();
    let matched = subject_handle.with_const_ptr::<StringHeader, _>(|subject_now| {
        re_handle.with_mut_ptr::<RegExpHeader, _>(|re_now| {
            crate::regex::js_regexp_exec(re_now, subject_now)
        })
    });

    assert!(
        gc_collection_count() > collections_before,
        "subject not live: the result-array allocation must run a copying minor"
    );
    assert_ne!(
        subject_before,
        subject_handle.with_const_ptr::<StringHeader, _>(|p| p as usize),
        "subject not live: the allocation-point minor must move the subject"
    );
    assert_captures(matched, &["young-42", "young", "42"]);
}

/// Fancy-regex and non-global `String#match` used a separate allocation path,
/// including an `.input` copy and named-group construction from live captures.
/// Exercise that path with the same allocation-point relocation witness.
#[test]
fn string_match_fancy_materializes_an_owned_snapshot_after_an_alloc_point_minor() {
    let _guard = CopyingNurseryTestGuard::new(4);
    let _pacing = crate::gc::policy::force_alloc_point_minor_pacing();
    let _scan = ConservativeScanDisabledGuard::new();
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    let scope = crate::gc::RuntimeHandleScope::new();
    let subject = heap_string(b"prefix-young-42-suffix");
    assert!(crate::arena::pointer_in_nursery(subject as usize));
    let subject_handle = scope.root_string_ptr(subject);
    let subject_before = subject as usize;
    let re = crate::regex::js_regexp_new(
        heap_string(br"(?<=prefix-)(?<word>young)-(\d+)"),
        heap_string(b"d"),
    );
    let re_handle = scope.root_raw_const_ptr(re);

    super::force_next_general_arena_alloc_slow();
    trigger_guard.make_arena_trigger_due();
    let collections_before = gc_collection_count();
    let matched = subject_handle.with_const_ptr::<StringHeader, _>(|subject_now| {
        re_handle.with_const_ptr::<RegExpHeader, _>(|re_now| {
            crate::regex::js_string_match(subject_now, re_now)
        })
    });

    assert!(
        gc_collection_count() > collections_before,
        "subject not live: the match-array allocation must run a copying minor"
    );
    assert_ne!(
        subject_before,
        subject_handle.with_const_ptr::<StringHeader, _>(|p| p as usize),
        "subject not live: the allocation-point minor must move the subject"
    );
    assert_captures(matched, &["young-42", "young", "42"]);
}
