//! Compound-operation binding reuse (#10165): one subject and program binding
//! serves every search of an operation, across actual moving collections, and
//! is abandoned whenever the receiver's program or the string is not the one
//! it bound.
use super::*;
use crate::array::ArrayHeader;
use crate::regex::perex_api::{self as api, Reuse};
use crate::regex::perex_memory::MemoryBudget;
use crate::regex::perex_owner::HeapSubject;
use crate::regex::RegExpHeader;
use crate::string::StringHeader;
use crate::value::{js_nanbox_pointer, js_nanbox_string};
use perex::binding::BoundSubject;
use perex::Budget;

pub(super) fn text<'s>(scope: &'s RuntimeHandleScope, bytes: &[u8]) -> RuntimeHandle<'s> {
    scope.root_string_ptr(crate::string::js_string_from_bytes(
        bytes.as_ptr(),
        bytes.len() as u32,
    ))
}

/// A NaN-boxed receiver handle, as split/replace/match root their receivers.
pub(super) fn regex<'s>(
    scope: &'s RuntimeHandleScope,
    pattern: &str,
    flags: &str,
) -> RuntimeHandle<'s> {
    let pattern = text(scope, pattern.as_bytes());
    let flags = text(scope, flags.as_bytes());
    let re = pattern.with_const_ptr::<StringHeader, _>(|pattern| {
        flags.with_const_ptr::<StringHeader, _>(|flags| crate::regex::js_regexp_new(pattern, flags))
    });
    scope.root_nanbox_f64(js_nanbox_pointer(re as i64))
}

fn receiver_ptr(receiver: &RuntimeHandle<'_>) -> *mut RegExpHeader {
    crate::value::js_nanbox_get_pointer(receiver.get_nanbox_f64()) as *mut RegExpHeader
}

fn first_item(scope: &RuntimeHandleScope, array: *mut ArrayHeader) -> Vec<u8> {
    let array = scope.root_raw_mut_ptr(array);
    let value = array.with_const_ptr::<ArrayHeader, _>(|a| crate::array::js_array_get_f64(a, 0));
    let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
    let (data, len) = crate::string::str_bytes_from_jsvalue(value, &mut scratch).unwrap();
    unsafe { std::slice::from_raw_parts(data, len as usize).to_vec() }
}

/// Run a global exec loop to exhaustion with a collection at every poll,
/// returning each full match and the work the loop charged.
fn global_loop(
    receiver: &RuntimeHandle<'_>,
    input: &RuntimeHandle<'_>,
    reuse: Option<&Reuse<'_, '_>>,
) -> (Vec<Vec<u8>>, usize) {
    global_loop_collecting(receiver, input, reuse, true)
}

pub(super) fn global_loop_collecting(
    receiver: &RuntimeHandle<'_>,
    input: &RuntimeHandle<'_>,
    reuse: Option<&Reuse<'_, '_>>,
    collect: bool,
) -> (Vec<Vec<u8>>, usize) {
    let memory = MemoryBudget::new(api::SCRATCH_BYTES);
    let mut budget = Budget::new(api::WORK);
    let mut matches = Vec::new();
    let roots = RuntimeHandleScope::active_len_for_tests();
    loop {
        let iteration = RuntimeHandleScope::new();
        // Re-read both addresses every search: the previous one collected.
        let found = input
            .with_const_ptr::<StringHeader, _>(|input| {
                api::execute_with_resources(
                    receiver_ptr(receiver),
                    input,
                    true,
                    &mut budget,
                    &memory,
                    &mut || {
                        if collect {
                            gc_collect_minor();
                        }
                        Ok(())
                    },
                    reuse,
                )
            })
            .unwrap();
        let Some(found) = found else { break };
        matches.push(first_item(&iteration, found.array));
        drop(iteration);
        assert_eq!(RuntimeHandleScope::active_len_for_tests(), roots);
    }
    (matches, api::WORK - budget.remaining())
}

#[test]
fn perex_reuse_serves_a_whole_global_loop_across_moving_collections() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    // Non-ASCII storage (the byte representation, not the ASCII layout). Every
    // object is allocated immediately before its loop so it is still young
    // and the loop's collections must actually relocate it.
    const SUBJECT: &str = "ä1 b22 c333 ä4444 é55555";
    const PATTERN: &str = "[a-zäé]+\\d+";
    let expected: Vec<Vec<u8>> = ["ä1", "b22", "c333", "ä4444", "é55555"]
        .iter()
        .map(|s| s.as_bytes().to_vec())
        .collect();

    let input = text(&scope, SUBJECT.as_bytes());
    let reused = regex(&scope, PATTERN, "gu");
    let subject = BoundSubject::new(unsafe { HeapSubject::new(input) }.unwrap()).unwrap();
    let mut setup = Budget::new(api::WORK);
    let reuse = Reuse::new(&scope, &reused, input, &subject, &mut setup);
    let input_before = input.with_const_ptr::<StringHeader, _>(|p| p as usize);
    let program_before = unsafe { (*receiver_ptr(&reused)).perex_program as usize };
    let cycles = copying_minor_cycles();
    let (reused_matches, reused_work) = global_loop(&reused, &input, Some(&reuse));

    assert_eq!(reused_matches, expected);
    assert!(
        copying_minor_cycles() > cycles,
        "the loop must actually collect"
    );
    assert_ne!(
        input.with_const_ptr::<StringHeader, _>(|p| p as usize),
        input_before,
        "the bound subject must have been relocated during the loop"
    );
    assert_ne!(
        unsafe { (*receiver_ptr(&reused)).perex_program as usize },
        program_before,
        "the reused program must have moved, and still be recognised as the same cell"
    );

    // The same operation on identical, independent objects without reuse.
    let fresh_input = text(&scope, SUBJECT.as_bytes());
    let fresh = regex(&scope, PATTERN, "gu");
    let (fresh_matches, fresh_work) = global_loop(&fresh, &fresh_input, None);
    assert_eq!(fresh_matches, expected);
    // Construction now shares the same program cell and its validation witness.
    // The second receiver pays no validation; the remaining work difference
    // must come from resuming searches near the preceding match.
    assert_eq!(unsafe { (*receiver_ptr(&fresh)).perex_program }, unsafe {
        (*receiver_ptr(&reused)).perex_program
    });
    let validation = api::WORK - setup.remaining();
    assert!(validation > 0, "the first binding must really validate");
    assert!(
        fresh_work > reused_work,
        "reuse must save seeks even when validation is shared: fresh {fresh_work}, reused {reused_work}"
    );
}

#[test]
fn perex_reuse_uses_the_receivers_current_program_after_recompile() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let input = text(&scope, b"aaa bbb aaa");
    let receiver = regex(&scope, "a+", "g");
    let subject = BoundSubject::new(unsafe { HeapSubject::new(input) }.unwrap()).unwrap();
    let reuse = Reuse::new(
        &scope,
        &receiver,
        input,
        &subject,
        &mut Budget::new(api::WORK),
    );
    let search = |scope: &RuntimeHandleScope| {
        let memory = MemoryBudget::new(api::SCRATCH_BYTES);
        input
            .with_const_ptr::<StringHeader, _>(|s| {
                api::execute_with_resources(
                    receiver_ptr(&receiver),
                    s,
                    true,
                    &mut Budget::new(api::WORK),
                    &memory,
                    &mut || {
                        gc_collect_minor();
                        Ok(())
                    },
                    Some(&reuse),
                )
            })
            .unwrap()
            .map(|found| first_item(scope, found.array))
    };
    let first = RuntimeHandleScope::new();
    assert_eq!(search(&first).as_deref(), Some(&b"aaa"[..]));
    drop(first);
    // RegExp.prototype.compile publishes a new program and resets lastIndex.
    let pattern = text(&scope, b"b+");
    let flags = text(&scope, b"g");
    crate::regex::js_regexp_compile_value(
        receiver_ptr(&receiver),
        pattern.with_const_ptr::<StringHeader, _>(|p| js_nanbox_string(p as i64)),
        flags.with_const_ptr::<StringHeader, _>(|p| js_nanbox_string(p as i64)),
    );
    let second = RuntimeHandleScope::new();
    assert_eq!(search(&second).as_deref(), Some(&b"bbb"[..]));
}

#[test]
fn perex_reuse_binds_a_different_string_afresh() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let bound_input = text(&scope, b"x1");
    let other_input = text(&scope, b"yy22");
    let receiver = regex(&scope, "\\d+", "");
    let subject = BoundSubject::new(unsafe { HeapSubject::new(bound_input) }.unwrap()).unwrap();
    let reuse = Reuse::new(
        &scope,
        &receiver,
        bound_input,
        &subject,
        &mut Budget::new(api::WORK),
    );
    let memory = MemoryBudget::new(api::SCRATCH_BYTES);
    let found = other_input
        .with_const_ptr::<StringHeader, _>(|s| {
            api::execute_with_resources(
                receiver_ptr(&receiver),
                s,
                true,
                &mut Budget::new(api::WORK),
                &memory,
                &mut || Ok(()),
                Some(&reuse),
            )
        })
        .unwrap()
        .unwrap();
    assert_eq!(first_item(&scope, found.array), b"22");
}

/// Work a global loop over `repeats` copies of a non-ASCII record charges.
fn non_ascii_loop_work(repeats: usize, reuse: bool) -> usize {
    // The unpositioned control must not pick up a cross-call position either.
    let _hints = (!reuse).then(crate::regex::perex_position_hint::DisableHintsForTest::new);
    let local = RuntimeHandleScope::new();
    let input = text(&local, "ä1 ö22 ".repeat(repeats).as_bytes());
    let receiver = regex(&local, "[a-zäö]+\\d+", "gu");
    let subject = BoundSubject::new(unsafe { HeapSubject::new(input) }.unwrap()).unwrap();
    let mut setup = Budget::new(api::WORK);
    let reused = Reuse::new(&local, &receiver, input, &subject, &mut setup);
    let (matches, work) =
        global_loop_collecting(&receiver, &input, reuse.then_some(&reused), false);
    assert_eq!(matches.len(), 2 * repeats);
    work
}

#[test]
fn perex_reuse_positions_keep_a_non_ascii_global_loop_linear() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    // Doubling the subject doubles the searches. Seeking each one from an end
    // of the subject makes the total roughly quadruple (#10164); resuming from
    // the previous search keeps it roughly double.
    let fresh = non_ascii_loop_work(2_000, false) as f64 / non_ascii_loop_work(1_000, false) as f64;
    let reused = non_ascii_loop_work(2_000, true) as f64 / non_ascii_loop_work(1_000, true) as f64;
    assert!(
        fresh > 3.0,
        "the unpositioned loop must be quadratic here, got {fresh:.2}x"
    );
    assert!(
        reused < 2.2,
        "the positioned loop must be linear, got {reused:.2}x"
    );
}

fn nanbox_text(scope: &RuntimeHandleScope, value: &str) -> f64 {
    text(scope, value.as_bytes()).with_const_ptr::<StringHeader, _>(|p| js_nanbox_string(p as i64))
}

fn utf16_length(value: f64) -> u32 {
    let string = crate::value::js_nanbox_get_pointer(value) as *const StringHeader;
    unsafe { (*string).utf16_len }
}

#[test]
fn perex_split_and_replace_no_longer_hit_the_work_limit_on_linear_inputs() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();

    // The #10164 reduction: a 32,000-unit split threw RangeError("Regular
    // expression work limit exceeded"); Node returns 6,001 pieces.
    let subject = "ä中12，Ö漢345；ef6😀".repeat(2_000);
    let input = nanbox_text(&scope, &subject);
    let input = scope.root_nanbox_f64(input);
    assert_eq!(utf16_length(input.get_nanbox_f64()), 32_000);
    let re = regex(&scope, "[，；😀]+", "u");
    let pieces = crate::regex::perex_split::regexp(
        re.get_nanbox_f64(),
        input.get_nanbox_f64(),
        f64::from_bits(crate::value::TAG_UNDEFINED),
    )
    .expect("a 32,000-unit split must not exhaust the work allowance");
    let pieces = crate::value::js_nanbox_get_pointer(pieces) as *const ArrayHeader;
    assert_eq!(unsafe { (*pieces).length }, 6_001);

    // A 60,000-unit global replace threw the same error; Node's result wraps
    // each of the 8,000 matches in brackets, 76,000 units in all.
    let subject = "ä中😀12 Ö漢🦊345;".repeat(4_000);
    let input = scope.root_nanbox_f64(nanbox_text(&scope, &subject));
    assert_eq!(utf16_length(input.get_nanbox_f64()), 60_000);
    let re = regex(&scope, "[ä中😀Ö漢🦊]+", "gu");
    let template = scope.root_nanbox_f64(nanbox_text(&scope, "[$&]"));
    let replaced = crate::regex::perex_replace::regexp(
        re.get_nanbox_f64(),
        input.get_nanbox_f64(),
        template.get_nanbox_f64(),
    )
    .expect("a 60,000-unit global replace must not exhaust the work allowance");
    assert_eq!(utf16_length(replaced), 76_000);
}
