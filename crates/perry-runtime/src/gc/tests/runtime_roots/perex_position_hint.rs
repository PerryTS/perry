//! Cross-call search positions on non-ASCII strings (#10164): a JavaScript-level
//! loop that runs one search per call resumes from where the previous call's
//! search stopped, and never from a position that belonged to another string.
use super::perex_reuse::{global_loop_collecting, regex, text};
use super::*;
use crate::regex::perex_api as api;
use crate::regex::perex_memory::MemoryBudget;
use crate::regex::perex_position_hint::{self as hints, DisableHintsForTest};
use crate::regex::RegExpHeader;
use crate::string::StringHeader;
use perex::Budget;

fn receiver_ptr(receiver: &RuntimeHandle<'_>) -> *mut RegExpHeader {
    crate::value::js_nanbox_get_pointer(receiver.get_nanbox_f64()) as *mut RegExpHeader
}

fn address(input: &RuntimeHandle<'_>) -> usize {
    input.with_const_ptr::<StringHeader, _>(|s| s as usize)
}

/// Work charged by a JS-level global exec loop (one call per match, nothing
/// carried by the operation itself) over `repeats` copies of a non-ASCII unit.
fn js_loop_work(repeats: usize) -> usize {
    let local = RuntimeHandleScope::new();
    let input = text(&local, "ä1 ö22 ".repeat(repeats).as_bytes());
    let receiver = regex(&local, "[a-zäö]+\\d+", "gu");
    let (matches, work) = global_loop_collecting(&receiver, &input, None, false);
    assert_eq!(matches.len(), 2 * repeats);
    work
}

/// One non-materializing search from `last_index`; the match span.
fn search_from(
    receiver: &RuntimeHandle<'_>,
    input: &RuntimeHandle<'_>,
    last_index: usize,
) -> Option<(usize, usize)> {
    crate::regex::set_last_index_throwing(receiver_ptr(receiver), last_index);
    let memory = MemoryBudget::new(api::SCRATCH_BYTES);
    let mut budget = Budget::new(api::WORK);
    input
        .with_const_ptr::<StringHeader, _>(|s| {
            api::execute_with_resources(
                receiver_ptr(receiver),
                s,
                false,
                &mut budget,
                &memory,
                &mut || Ok(()),
                None,
            )
        })
        .unwrap()
        .map(|found| (found.full.start(), found.full.end()))
}

#[test]
fn cross_call_positions_keep_a_js_level_non_ascii_loop_linear() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    hints::clear_for_test();

    let positioned = js_loop_work(2_000) as f64 / js_loop_work(1_000) as f64;
    assert!(
        hints::hint_uses() > 0,
        "the loop must have resumed from recorded positions"
    );
    let unpositioned = {
        let _off = DisableHintsForTest::new();
        js_loop_work(2_000) as f64 / js_loop_work(1_000) as f64
    };
    assert!(
        unpositioned > 3.0,
        "without positions the loop must be quadratic here, got {unpositioned:.2}x"
    );
    assert!(
        positioned < 2.2,
        "with positions the loop must be linear, got {positioned:.2}x"
    );
}

#[test]
fn a_moved_string_does_not_reuse_its_position() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let receiver = regex(&scope, "[a-zäö]+\\d+", "gu");
    let input = text(&scope, "ä1 ö22 ".repeat(64).as_bytes());
    hints::clear_for_test();

    let first = search_from(&receiver, &input, 0).unwrap();
    let second = search_from(&receiver, &input, first.1).unwrap();
    assert_eq!(
        hints::hint_uses(),
        1,
        "an unmoved string resumes from its position"
    );

    let before = address(&input);
    gc_collect_minor();
    assert_ne!(
        address(&input),
        before,
        "the witness string must have moved"
    );
    let uses = hints::hint_uses();
    let third = search_from(&receiver, &input, second.1).unwrap();
    assert_eq!(
        hints::hint_uses(),
        uses,
        "a moved string must not reuse its position"
    );
    assert_eq!((first, second, third), ((0, 2), (3, 6), (7, 9)));
}

#[test]
fn another_string_at_the_same_address_after_a_free_does_not_use_the_position() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let receiver = regex(&scope, "[a-zäö]+\\d+", "gu");
    hints::clear_for_test();
    // Start from an empty nursery, so the first string allocated lands at the
    // same address before and after the collection that frees it.
    gc_collect_minor();

    // Same byte and UTF-16 lengths, different arrangement: the ASCII and
    // non-ASCII letters trade places, so a byte position means another unit.
    let a_bytes = "ä1 ab22 ".repeat(40);
    let b_bytes = "ab1 ä22 ".repeat(40);
    assert_eq!(a_bytes.len(), b_bytes.len());

    let freed_address;
    let a_end;
    {
        let a_scope = RuntimeHandleScope::new();
        let a = text(&a_scope, a_bytes.as_bytes());
        freed_address = address(&a);
        let first = search_from(&receiver, &a, 0).unwrap();
        let second = search_from(&receiver, &a, first.1).unwrap();
        assert_eq!(
            hints::hint_uses(),
            1,
            "the first string records and uses a position"
        );
        a_end = second.1;
    }
    gc_collect_minor();

    let b = text(&scope, b_bytes.as_bytes());
    assert_eq!(
        address(&b),
        freed_address,
        "precondition: the second string must occupy the freed string's address"
    );
    let uses = hints::hint_uses();
    let found = search_from(&receiver, &b, a_end).unwrap();
    assert_eq!(
        hints::hint_uses(),
        uses,
        "a position recorded on a freed string must not serve the string now at its address"
    );
    // "ä1 ab22 " ends its second match at UTF-16 index 7; in "ab1 ä22 ab1 ..."
    // the next match from index 7 is the second "ab1".
    assert_eq!(a_end, 7);
    assert_eq!(found, (8, 11));
}
