//! GC program lifetime after bounded cache eviction releases its strong roots.
use super::*;
use crate::regex::RegExpHeader;

fn text<'s>(scope: &'s RuntimeHandleScope, value: &str) -> RuntimeHandle<'s> {
    scope.root_string_ptr(crate::string::js_string_from_bytes(
        value.as_ptr(),
        value.len() as u32,
    ))
}

fn regex<'s>(scope: &'s RuntimeHandleScope, pattern: &str, flags: &str) -> RuntimeHandle<'s> {
    let source = text(scope, pattern);
    let flags = text(scope, flags);
    scope.root_raw_mut_ptr(source.with_const_ptr(|source| {
        flags.with_const_ptr(|flags| crate::regex::js_regexp_new(source, flags))
    }))
}

fn matches(re: &RuntimeHandle<'_>, input: &str) -> bool {
    let scope = RuntimeHandleScope::new();
    let input = text(&scope, input);
    re.with_const_ptr(|re| input.with_const_ptr(|input| crate::regex::js_regexp_test(re, input)))
        != 0
}

fn programs() -> usize {
    let mut cursor = crate::arena::ArenaObjectCursor::new(crate::arena::ArenaWalkOrder::Address);
    let mut remaining = usize::MAX;
    let mut count = 0;
    while let Some((header, _)) = cursor.next_budgeted(&mut remaining) {
        if unsafe { (*(header as *const GcHeader)).obj_type == GC_TYPE_REGEX_PROGRAM } {
            count += 1;
        }
    }
    count
}

#[test]
fn regexp_cache_registers_roots_only_on_use_and_not_again_after_eviction() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let _ = crate::object::js_get_global_this();
    let before = root_scanner_registry_counts().1;
    crate::regex::perex_cache::census();
    assert_eq!(root_scanner_registry_counts().1, before);
    let scope = RuntimeHandleScope::new();
    let re = regex(&scope, "registered-on-use", "");
    assert_eq!(root_scanner_registry_counts().1, before + 1);
    assert!(matches(&re, "registered-on-use"));
    assert_eq!(root_scanner_registry_counts().1, before + 1);
    crate::regex::perex_cache::clear_for_tests();
    let re = regex(&scope, "registered-on-use", "");
    assert!(matches(&re, "registered-on-use"));
    assert_eq!(root_scanner_registry_counts().1, before + 1);
}

#[test]
fn perex_lifecycle_reclaims_evicted_programs_when_their_only_receivers_die() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let before = programs();
    let survivor_scope = RuntimeHandleScope::new();
    let (survivor, dead_addresses) = {
        let temporary = RuntimeHandleScope::new();
        let ordinary = regex(&temporary, r"needle\d+", "");
        let lookbehind = regex(&temporary, r"(?<=pre)\d+", "");
        let quantified = regex(&temporary, r"(a?b??)*", "");
        assert!(matches(&ordinary, "needle42"));
        assert!(matches(&lookbehind, "pre77"));
        assert!(matches(&quantified, "ab"));
        assert_eq!(programs(), before + 3);
        let dead = [ordinary, quantified].map(|re| {
            re.with_const_ptr(|re: *const RegExpHeader| {
                (re as usize, crate::regex::test_regexp_program_address(re))
            })
        });
        // Handed to `survivor_scope` below with nothing allocating in between.
        (lookbehind.with_mut_ptr(|re: *mut RegExpHeader| re), dead)
    };
    let survivor = survivor_scope.root_raw_mut_ptr(survivor);
    let old = survivor.with_const_ptr(|p| crate::regex::test_regexp_program_address(p));
    crate::regex::perex_cache::clear_for_tests();
    gc_collect_minor();
    assert_eq!(
        programs(),
        before + 1,
        "evicted dead programs must be reclaimed"
    );
    assert_ne!(
        survivor.with_const_ptr(|p| crate::regex::test_regexp_program_address(p)),
        old
    );
    for (header, program) in dead_addresses {
        assert!(!crate::regex::test_regex_pointer_entry_exists(header));
        assert!(!build_valid_pointer_set().contains(&program));
    }
    assert!(matches(&survivor, "pre77"));
    assert!(!matches(&survivor, "nope77"));
    drop(survivor_scope);
    gc_collect_minor();
    assert_eq!(
        programs(),
        before,
        "the remaining program must also be reclaimed"
    );
}

#[test]
fn perex_lifecycle_eviction_releases_programs_without_disarming_receivers() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let ordinary = regex(&scope, r"needle\d+", "");
    let lookbehind = regex(&scope, r"(?<=pre)\d+", "");
    let quantified = regex(&scope, r"(a?b??)*", "");
    let retained = programs();
    assert!(retained >= 3);
    let native = external_side_live_bytes();
    // Releasing cached roots must let collection reclaim temporary programs
    // while original receiver owners survive. The cache unit tests exercise
    // actual LRU pressure separately.
    for i in 0..2078 {
        {
            let temporary = RuntimeHandleScope::new();
            let pattern = match i % 3 {
                0 => format!("cachefill{i}[a-z]+"),
                1 => format!("(?<=fill{i})x"),
                _ => format!("(repeat{i})*"),
            };
            let re = regex(&temporary, &pattern, "");
            assert!(
                re.with_const_ptr(|p| crate::regex::test_original_strings_and_program(p))
                    .2
            );
        }
        if i % 64 == 63 || i == 2077 {
            assert!(
                programs() > retained,
                "the allocation pressure must be real"
            );
            crate::regex::perex_cache::clear_for_tests();
            gc_collect_minor();
            assert_eq!(programs(), retained, "temporary programs must be reclaimed");
            assert_eq!(external_side_live_bytes(), native);
            assert!(matches(&ordinary, "xx needle42 yy"));
            assert!(!matches(&ordinary, "no match here"));
            assert!(matches(&lookbehind, "pre77"));
            assert!(!matches(&lookbehind, "nope77"));
            assert!(matches(&quantified, "ab"));
        }
    }
}

#[test]
fn perex_lifecycle_literal_and_dynamic_construction_have_independent_state() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let source = text(&scope, "born[0-9]+built");
    let flags = text(&scope, "g");
    static SITE: u64 = 0x5045524558;
    let mut owners = Vec::new();
    for _ in 0..2 {
        owners.push(scope.root_raw_mut_ptr(source.with_const_ptr(|source| {
            flags.with_const_ptr(|flags| {
                crate::regex::js_regexp_new_site(source, flags, &SITE as *const u64 as i64)
            })
        })));
    }
    owners.push(regex(&scope, "born[0-9]+built", "g"));
    for (i, owner) in owners.iter().enumerate() {
        assert!(
            owner
                .with_const_ptr(|p| crate::regex::test_original_strings_and_program(p))
                .2
        );
        for other in &owners[..i] {
            assert_ne!(
                handle_address::<RegExpHeader>(owner),
                handle_address::<RegExpHeader>(other)
            );
        }
    }
    assert!(matches(&owners[0], "xx born42built"));
    assert_eq!(
        owners[0].with_const_ptr(|p| crate::regex::regex_last_index_offset(p)),
        14
    );
    for owner in &owners[1..] {
        assert_eq!(
            owner.with_const_ptr(|p| crate::regex::regex_last_index_offset(p)),
            0
        );
    }
    gc_collect_minor();
    assert!(matches(&owners[1], "born7built"));
    assert!(!matches(&owners[1], "nothing"));
    assert!(matches(&owners[2], "born8built"));
    let insensitive = regex(&scope, "born[0-9]+built", "i");
    assert!(matches(&insensitive, "BORN9BUILT"));
}

#[test]
fn regexp_cache_is_the_only_root_and_rekeys_after_actual_movement() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let before = programs();
    let old = {
        let scope = RuntimeHandleScope::new();
        let re = regex(&scope, "cache-only-root", "g");
        assert!(matches(&re, "cache-only-root"));
        re.with_const_ptr(|p| crate::regex::test_regexp_program_address(p))
    };
    let cycles = copying_minor_cycles();
    gc_collect_minor();
    assert!(copying_minor_cycles() > cycles);
    assert_eq!(programs(), before + 1, "cache must retain the live program");
    {
        let scope = RuntimeHandleScope::new();
        let re = regex(&scope, "cache-only-root", "g");
        assert_ne!(
            old,
            re.with_const_ptr(|p| crate::regex::test_regexp_program_address(p))
        );
        assert_eq!(
            programs(),
            before + 1,
            "post-move lookup must reuse the program"
        );
        assert!(matches(&re, "cache-only-root"));
    }
    crate::regex::perex_cache::clear_for_tests();
    gc_collect_minor();
    assert_eq!(
        programs(),
        before,
        "eviction must release the construction cache's roots"
    );
}

#[test]
fn regexp_active_binding_survives_eviction_and_relocation() {
    use crate::regex::perex_memory::MemoryBudget;
    use crate::regex::{perex_api as api, perex_runtime as host};
    use perex::{binding::BoundSubject, Budget};
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let before = programs();
    let scope = RuntimeHandleScope::new();
    let raw = {
        let temporary = RuntimeHandleScope::new();
        let re = regex(&temporary, "active-binding", "");
        re.with_const_ptr::<RegExpHeader, _>(|re| unsafe { (*re).perex_program })
    };
    // No collecting operation between extracting the live cache entry and
    // rooting it in the outer scope, after the temporary scope has ended.
    let owner = unsafe { crate::regex::perex_owner::GcProgram::from_cached(&scope, raw) };
    let program = api::bind_program(owner, &mut Budget::new(api::WORK)).unwrap();
    let old = program.with_view(|p| p.words().as_ptr() as usize).unwrap();
    crate::regex::perex_cache::clear_for_tests();
    gc_collect_minor();
    assert_eq!(programs(), before + 1);
    assert_ne!(
        program.with_view(|p| p.words().as_ptr() as usize).unwrap(),
        old
    );
    let subject =
        BoundSubject::new(crate::regex::perex_glob::NativeText("active-binding")).unwrap();
    assert!(host::find(
        &program,
        &subject,
        0,
        host::CaptureMode::Full,
        &mut Budget::new(api::WORK),
        &MemoryBudget::new(api::SCRATCH_BYTES),
        api::QUANTUM,
        &mut host::poll
    )
    .unwrap()
    .is_some());
    drop(program);
    drop(scope);
    gc_collect_minor();
    assert_eq!(programs(), before);
}
