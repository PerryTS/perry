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

fn text<'s>(scope: &'s RuntimeHandleScope, bytes: &[u8]) -> RuntimeHandle<'s> {
    scope.root_string_ptr(crate::string::js_string_from_bytes(
        bytes.as_ptr(),
        bytes.len() as u32,
    ))
}

/// A NaN-boxed receiver handle, as split/replace/match root their receivers.
fn regex<'s>(scope: &'s RuntimeHandleScope, pattern: &str, flags: &str) -> RuntimeHandle<'s> {
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
                        gc_collect_minor();
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
    // Six searches (five matches and the final miss). Binding per search charges
    // program validation six times; reuse charged it once, in `setup`.
    let validation = api::WORK - setup.remaining();
    assert!(validation > 0);
    assert_eq!(fresh_work, reused_work + 6 * validation);
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
