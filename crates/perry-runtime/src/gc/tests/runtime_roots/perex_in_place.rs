//! A builtin search reads the program cell and the subject string in place
//! (S6, `perex_owner::InPlace`). A long search polls between quanta, and a
//! poll may move both: every quantum must read both bases through their roots
//! for that quantum only, or the search reads the old (here: poisoned)
//! addresses. Keeping a pre-poll address across a poll (the program's
//! unrooted one, or a cached string base) must fail
//! `perex_in_place_search_reacquires_bases_a_poll_moved`; not reading the
//! receiver back after a poll must fail
//! `perex_test_entry_holds_no_handle_and_reads_every_base_back_after_a_moving_poll`.
use super::perex_public::register_host_roots;
use super::*;
use crate::regex::perex_api::{self as api, ExecOutput};
use crate::regex::perex_memory::MemoryBudget;
use crate::string::StringHeader;
use perex::Budget;

fn text<'s>(scope: &'s RuntimeHandleScope, bytes: &[u8]) -> RuntimeHandle<'s> {
    scope.root_string_ptr(crate::string::js_string_from_bytes(
        bytes.as_ptr(),
        bytes.len() as u32,
    ))
}

fn regex<'s>(scope: &'s RuntimeHandleScope, pattern: &str, flags: &str) -> RuntimeHandle<'s> {
    let pattern = text(scope, pattern.as_bytes());
    let flags = text(scope, flags.as_bytes());
    let re = pattern.with_const_ptr::<StringHeader, _>(|pattern| {
        flags.with_const_ptr::<StringHeader, _>(|flags| crate::regex::js_regexp_new(pattern, flags))
    });
    scope.root_nanbox_f64(crate::value::js_nanbox_pointer(re as i64))
}

fn address<T>(handle: &RuntimeHandle<'_>) -> usize {
    handle.with_const_ptr(|p: *const T| p as usize)
}

fn program_address(receiver: &RuntimeHandle<'_>) -> usize {
    unsafe { (*crate::regex::regexp_data_ptr(api::regexp(receiver))).perex_program as usize }
}

/// The subject's refcount word: 1 while unique, 0 once marked shared.
fn refcount(input: &RuntimeHandle<'_>) -> u32 {
    input.with_const_ptr::<StringHeader, _>(|s| unsafe { (*s).refcount })
}

/// Run one builtin exec, reporting every capture span, with `poll` as the
/// search's safepoint.
fn spans(
    receiver: &RuntimeHandle<'_>,
    input: &RuntimeHandle<'_>,
    poll: &mut impl FnMut() -> Result<(), crate::regex::perex_runtime::EngineError>,
) -> Option<Vec<u32>> {
    let memory = MemoryBudget::new(api::SCRATCH_BYTES);
    let mut budget = Budget::new(api::WORK);
    let mut spans = Vec::new();
    let found = api::finish(api::execute_rooted(
        receiver,
        input,
        ExecOutput::Spans(&mut spans),
        &mut budget,
        &memory,
        poll,
        None,
    ));
    found.map(|_| spans)
}

#[test]
fn perex_in_place_search_reacquires_bases_a_poll_moved() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    let _protection =
        crate::arena::ProtectionModeGuard::set(crate::arena::FromSpaceProtection::PoisonOnly);
    register_host_roots();
    let scope = RuntimeHandleScope::new();
    let receiver = regex(&scope, "(a+)(b)", "");
    // Several quanta of work before the match is decided, so the search
    // pauses and polls with both bases held; under the large-object
    // threshold, so the string is born in the nursery and can move.
    let units = 3 * api::QUANTUM;
    let mut bytes = vec![b'a'; units];
    bytes.push(b'b');
    let input = text(&scope, &bytes);
    assert!(
        crate::arena::pointer_in_nursery(address::<StringHeader>(&input)),
        "the subject must be movable for this test to mean anything"
    );
    input.with_mut_ptr::<StringHeader, _>(|s| unsafe { (*s).refcount = 1 });
    let string_before = address::<StringHeader>(&input);
    let program_before = program_address(&receiver);
    let cycles = copying_minor_cycles();
    let mut polls = 0;
    let found = spans(&receiver, &input, &mut || {
        polls += 1;
        gc_collect_minor();
        // Reuse whatever the collection freed, so a stale base cannot read
        // the old bytes by luck.
        for _ in 0..8 {
            let _ = crate::string::js_string_from_bytes(b"################".as_ptr(), 16);
        }
        Ok(())
    });
    assert!(polls >= 2, "the search must have paused, polled {polls}");
    assert!(copying_minor_cycles() > cycles);
    assert_ne!(
        address::<StringHeader>(&input),
        string_before,
        "the poll must have moved the subject"
    );
    assert_ne!(
        program_address(&receiver),
        program_before,
        "the poll must have moved the program cell"
    );
    let units = units as u32;
    assert_eq!(
        found,
        Some(vec![0, units + 1, 0, units, units, units + 1]),
        "a search continued across a moving poll must read the moved bases"
    );
    assert_eq!(
        refcount(&input),
        0,
        "a binding that outlives a poll marks its subject shared"
    );
}

#[test]
fn perex_in_place_search_decided_in_one_quantum_roots_and_marks_nothing() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_host_roots();
    let scope = RuntimeHandleScope::new();
    let receiver = regex(&scope, "(a+)(b)", "");
    let input = text(&scope, b"xxaab");
    // The first search on a thread grows its lent scratch through the owned
    // path, which polls; measure a warm one.
    let _ = spans(&receiver, &input, &mut || Ok(()));
    input.with_mut_ptr::<StringHeader, _>(|s| unsafe { (*s).refcount = 1 });
    let handles = RuntimeHandleScope::active_len_for_tests();
    let mut polls = 0;
    let found = spans(&receiver, &input, &mut || {
        polls += 1;
        Ok(())
    });
    assert_eq!(found, Some(vec![2, 5, 2, 4, 4, 5]));
    // Only the capture-slot safepoint polls; nothing pauses.
    assert_eq!(polls, 1);
    assert_eq!(RuntimeHandleScope::active_len_for_tests(), handles);
    assert_eq!(
        refcount(&input),
        0,
        "the capture-slot poll keeps the subject binding across a safepoint"
    );
    // A boolean test never polls: the string stays unique.
    let other = text(&scope, b"xxaab");
    other.with_mut_ptr::<StringHeader, _>(|s| unsafe { (*s).refcount = 1 });
    let memory = MemoryBudget::new(api::SCRATCH_BYTES);
    let mut budget = Budget::new(api::WORK);
    let mut test_polls = 0;
    let matched = api::finish(api::execute_rooted(
        &receiver,
        &other,
        ExecOutput::Test,
        &mut budget,
        &memory,
        &mut || {
            test_polls += 1;
            Ok(())
        },
        None,
    ));
    assert!(matched.is_some());
    assert_eq!(matched.unwrap().full, perex::span::Span::new(2, 5).unwrap());
    assert!(test_polls <= 1, "at most the strided pre-search poll");
    assert_eq!(
        refcount(&other),
        1,
        "a search decided in its first quantum must not mark the subject shared"
    );
}

#[test]
fn perex_in_place_stateful_search_writes_last_index_without_a_trap() {
    let _guard = CopyingNurseryTestGuard::new(0);
    register_host_roots();
    let scope = RuntimeHandleScope::new();
    let receiver = regex(&scope, "a", "g");
    let input = text(&scope, b"baab");
    let memory = MemoryBudget::new(api::SCRATCH_BYTES);
    let run = |budget: &mut Budget| {
        api::execute_rooted(
            &receiver,
            &input,
            ExecOutput::Test,
            budget,
            &memory,
            &mut || Ok(()),
            None,
        )
        .map(|m| m.map(|m| (m.full.start(), m.full.end())))
    };
    let last_index = || {
        crate::value::JSValue::from_bits(
            crate::regex::get_last_index(api::regexp(&receiver)).to_bits(),
        )
        .as_number()
    };
    let mut budget = Budget::new(api::WORK);
    assert_eq!(api::finish(run(&mut budget)), Some((1, 2)));
    assert_eq!(last_index(), 2.0);
    assert_eq!(api::finish(run(&mut budget)), Some((2, 3)));
    assert_eq!(api::finish(run(&mut budget)), None);
    assert_eq!(last_index(), 0.0);
    // A non-writable lastIndex is a TypeError, returned, not thrown.
    let attrs = crate::object::PropertyAttrs::new(false, false, false);
    crate::object::set_property_attrs(
        api::regexp(&receiver) as usize,
        "lastIndex".to_string(),
        attrs,
    );
    let thrown = match run(&mut budget) {
        Err(crate::regex::perex_runtime::EngineError::Abrupt(thrown)) => thrown,
        other => panic!("ordinary throwing Set must return its TypeError: {other:?}"),
    };
    let thrown = scope.root_raw_mut_ptr(
        crate::value::js_nanbox_get_pointer(thrown) as *mut crate::error::ErrorHeader
    );
    let name = thrown.with_mut_ptr(|e| crate::error::js_error_get_name(e));
    assert_eq!(crate::regex::string_as_bytes(name), b"TypeError");
}

/// `RegExp.prototype.test`'s entry runs RegExpBuiltinExec on raw addresses
/// (`api::search_builtin`): no handle at entry, roots only when the search
/// polls, and every address read back from those roots after each poll. A
/// global search across moving polls must still answer from the moved string
/// and program, and store `lastIndex` on the moved receiver.
#[test]
fn perex_test_entry_holds_no_handle_and_reads_every_base_back_after_a_moving_poll() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    let _protection =
        crate::arena::ProtectionModeGuard::set(crate::arena::FromSpaceProtection::PoisonOnly);
    register_host_roots();
    let scope = RuntimeHandleScope::new();
    let receiver = regex(&scope, "(a+)(b)", "g");
    let units = 3 * api::QUANTUM;
    let mut bytes = vec![b'a'; units];
    bytes.push(b'b');
    let input = text(&scope, &bytes);
    assert!(
        crate::arena::pointer_in_nursery(address::<StringHeader>(&input))
            && crate::arena::pointer_in_nursery(api::regexp(&receiver) as usize),
        "the subject and the receiver must be movable for this test to mean anything"
    );
    let string_before = address::<StringHeader>(&input);
    let program_before = program_address(&receiver);
    let receiver_before = api::regexp(&receiver) as usize;
    let handles = RuntimeHandleScope::active_len_for_tests();
    let memory = MemoryBudget::new(api::SCRATCH_BYTES);
    let mut budget = Budget::new(api::WORK);
    let mut polls = 0;
    let mut handles_in_poll = 0;
    let found = api::finish(api::search_builtin(
        api::regexp(&receiver),
        crate::regex::regexp_data_ptr(api::regexp(&receiver)),
        input.with_const_ptr::<StringHeader, _>(|s| s),
        crate::regex::perex_runtime::CaptureMode::Full,
        &mut budget,
        &memory,
        &mut None,
        &mut || {
            polls += 1;
            handles_in_poll = RuntimeHandleScope::active_len_for_tests();
            gc_collect_minor();
            for _ in 0..8 {
                let _ = crate::string::js_string_from_bytes(b"################".as_ptr(), 16);
            }
            Ok(())
        },
    ));
    assert!(polls >= 2, "the search must have paused, polled {polls}");
    assert_eq!(
        handles_in_poll,
        handles + 3,
        "the first poll roots exactly the receiver, the program and the subject"
    );
    assert_ne!(address::<StringHeader>(&input), string_before);
    assert_ne!(program_address(&receiver), program_before);
    assert_ne!(api::regexp(&receiver) as usize, receiver_before);
    let units = units as u32;
    assert_eq!(
        found.map(|(full, _)| (full.start(), full.end())),
        Some((0, units as usize + 1)),
        "a search continued across moving polls must read the moved bases"
    );
    assert_eq!(
        crate::value::JSValue::from_bits(
            crate::regex::get_last_index(api::regexp(&receiver)).to_bits()
        )
        .as_number(),
        f64::from(units + 1),
        "lastIndex must land on the receiver where the polls left it"
    );
    assert_eq!(
        RuntimeHandleScope::active_len_for_tests(),
        handles,
        "the search's roots close with it"
    );
}

/// A test decided in its first quantum, through the public entry, holds no
/// handle and leaves the subject unique. Worker startup is process-wide and
/// sends the exec proof to the observable path, so this runs in a fresh
/// process.
#[test]
fn perex_test_entry_decided_in_one_quantum_holds_no_handle() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "gc::tests::runtime_roots::perex_in_place::perex_test_entry_decided_in_one_quantum_holds_no_handle",
    ) {
        return;
    }
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_host_roots();
    let scope = RuntimeHandleScope::new();
    let receiver = regex(&scope, "^\\p{Default_Ignorable_Code_Point}$", "u");
    let hit = text(&scope, "\u{200B}".as_bytes());
    let miss = text(&scope, b"x");
    for input in [&hit, &miss] {
        input.with_mut_ptr::<StringHeader, _>(|s| unsafe { (*s).refcount = 1 });
    }
    let test = |input: &RuntimeHandle<'_>| {
        api::finish(crate::regex::perex_dispatch::test_string(
            receiver.get_nanbox_f64(),
            input.with_const_ptr::<StringHeader, _>(|s| s),
        ))
    };
    // Warm: the first search on a thread grows its lent scratch.
    let _ = test(&miss);
    let _ = test(&hit);
    let handles = RuntimeHandleScope::active_len_for_tests();
    // The strided pre-search poll may run a pending collection, which can
    // move and re-mark a string; judge only calls no collection ran across,
    // and require that nearly all of them were such calls.
    let mut judged = 0;
    for round in 0..20 {
        let input = if round % 2 == 0 { &hit } else { &miss };
        input.with_mut_ptr::<StringHeader, _>(|s| unsafe { (*s).refcount = 1 });
        let cycles = copying_minor_cycles();
        assert_eq!(test(input), round % 2 == 0);
        assert_eq!(
            RuntimeHandleScope::active_len_for_tests(),
            handles,
            "the test entry must hold no handle after it returns"
        );
        if copying_minor_cycles() == cycles {
            assert_eq!(
                refcount(input),
                1,
                "a test decided in its first quantum must not mark the subject shared"
            );
            judged += 1;
        }
    }
    assert!(
        judged >= 18,
        "only {judged} of 20 calls ran without a collection"
    );
}
