//! Host-level compiler/search witnesses using Perry's real collector and
//! native-allocation accounting. Complete UTF-16 captures remain observable.
use super::*;
use crate::regex::perex_memory::{Buffer, MemoryBudget, StorageError};
use crate::regex::perex_owner::{GcProgram, HeapSubject};
use crate::regex::perex_runtime::{self as host, CaptureMode, EngineError};
use crate::regex::validate_and_canonicalize_flags;
use perex::binding::{BoundProgram, BoundSubject};
use perex::compiler::{CompileError, Node, Range};
use perex::executor::ExecError;
use perex::{span::Span, Budget};

fn subject<'s>(scope: &'s RuntimeHandleScope, bytes: &[u8]) -> BoundSubject<HeapSubject<'s>> {
    let ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    BoundSubject::new(unsafe { HeapSubject::new(scope.root_string_ptr(ptr)).unwrap() }).unwrap()
}

fn compile<'s>(
    scope: &'s RuntimeHandleScope,
    text: &str,
    flags: &str,
) -> BoundProgram<GcProgram<'s>> {
    let pattern = subject(scope, text.as_bytes());
    let memory = MemoryBudget::new(1 << 20);
    let mut budget = Budget::new(1_000_000);
    let program = host::compile(
        scope,
        &pattern,
        validate_and_canonicalize_flags(flags),
        &mut budget,
        &memory,
        1 << 20,
        &mut host::poll,
    )
    .unwrap();
    assert_eq!(memory.live_bytes(), 0);
    BoundProgram::new(program, &mut budget).unwrap()
}

#[test]
fn perex_host_buffers_account_overlap_failure_and_unwind() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let before = external_side_live_bytes();
    let memory = MemoryBudget::new(128);
    {
        let first = Buffer::<u8>::new(&memory, 64).unwrap();
        assert_eq!(external_side_live_bytes(), before + memory.live_bytes());
        assert!(matches!(
            Buffer::<u8>::new(&memory, 65),
            Err(StorageError::Limit)
        ));
        let replacement = Buffer::<u8>::new(&memory, 64).unwrap();
        assert_eq!(memory.peak_bytes(), 128);
        assert_eq!(external_side_live_bytes(), before + 128);
        drop(first);
        assert_eq!(external_side_live_bytes(), before + 64);
        drop(replacement);
    }
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _live = Buffer::<u8>::new(&memory, 100).unwrap();
        panic!("test callback unwind");
    }));
    assert!(unwind.is_err());
    assert_eq!(memory.live_bytes(), 0);
    assert_eq!(external_side_live_bytes(), before);
}

#[test]
fn perex_host_compile_grows_scratch_and_reborrows_a_moving_pattern() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    let text = "(a)".repeat(100);
    let ptr = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    let original = ptr as usize;
    let observe = scope.root_string_ptr(ptr);
    let pattern =
        BoundSubject::new(unsafe { HeapSubject::new(scope.root_string_ptr(ptr)).unwrap() })
            .unwrap();
    let before = external_side_live_bytes();
    let memory = MemoryBudget::new(1 << 20);
    let mut budget = Budget::new(1_000_000);
    let mut polls = 0;
    let program = host::compile(
        &scope,
        &pattern,
        validate_and_canonicalize_flags(""),
        &mut budget,
        &memory,
        1 << 20,
        &mut || {
            polls += 1;
            gc_collect_minor();
            Ok(())
        },
    )
    .unwrap();
    assert!(polls > 2, "must retry at least once before final emission");
    assert_ne!(
        observe.get_raw_mut_ptr::<crate::StringHeader>() as usize,
        original
    );
    assert!(
        memory.peak_bytes() > 64 * (std::mem::size_of::<Node>() + std::mem::size_of::<Range>())
    );
    assert!(budget.remaining() < 1_000_000);
    assert_eq!(memory.live_bytes(), 0);
    assert_eq!(external_side_live_bytes(), before);
    let program = BoundProgram::new(program, &mut budget).unwrap();
    let input = subject(&scope, "a".repeat(100).as_bytes());
    let result = host::find(
        &program,
        &input,
        0,
        CaptureMode::All,
        &mut budget,
        &memory,
        17,
        &mut host::poll,
    )
    .unwrap()
    .unwrap();
    assert_eq!(result.full, Span::new(0, 100).unwrap());
    let captures = result.captures.as_ref().unwrap();
    assert_eq!(captures.len(), 101);
    for i in 0..100 {
        assert_eq!(captures[i + 1], Span::new(i, i + 1));
    }
    drop(result);
    assert_eq!(memory.live_bytes(), 0);
    assert_eq!(external_side_live_bytes(), before);
}

#[test]
fn perex_host_search_preserves_all_captures_and_work_across_growth_and_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();
    let cases: &[(&str, &str, &[u8], &[Option<(usize, usize)>])] = &[
        ("(a|(b))+", "", b"ba", &[Some((0, 2)), Some((1, 2)), None]),
        (r"(?<=(a+))b\1", "", b"aabaa", &[Some((2, 5)), Some((0, 2))]),
        (
            r"(?<letter>a)(😀|\ud800)",
            "u",
            b"prefix-a\xed\xa0\x80-suffix",
            &[Some((7, 9)), Some((7, 8)), Some((8, 9))],
        ),
        ("(.)", "", "😀".as_bytes(), &[Some((0, 1)), Some((0, 1))]),
    ];
    for &(pattern, flags, bytes, expected) in cases {
        let mut remaining = None;
        for quantum in [1, 17, 100_000] {
            let scope = RuntimeHandleScope::new();
            let program = compile(&scope, pattern, flags);
            let program_before = program.with_view(|p| p.words().as_ptr() as usize).unwrap();
            let input = subject(&scope, bytes);
            let memory = MemoryBudget::new(1 << 20);
            let before = external_side_live_bytes();
            let mut budget = Budget::new(100_000);
            let cycles = copying_minor_cycles();
            let result = host::find(
                &program,
                &input,
                0,
                CaptureMode::All,
                &mut budget,
                &memory,
                quantum,
                &mut || {
                    gc_collect_minor();
                    for _ in 0..4 {
                        let _ =
                            crate::string::js_string_from_bytes(b"################".as_ptr(), 16);
                    }
                    Ok(())
                },
            )
            .unwrap()
            .unwrap();
            let actual = result.captures.as_ref().unwrap();
            assert_eq!(actual.len(), expected.len());
            for (actual, expected) in actual.iter().zip(expected) {
                assert_eq!(
                    *actual,
                    expected.and_then(|(a, b)| Span::new(a, b)),
                    "{pattern}, q{quantum}"
                );
            }
            assert_eq!(result.full, actual[0].unwrap());
            assert!(copying_minor_cycles() > cycles);
            assert_ne!(
                program_before,
                program.with_view(|p| p.words().as_ptr() as usize).unwrap()
            );
            if let Some(reference) = remaining {
                assert_eq!(budget.remaining(), reference);
            } else {
                remaining = Some(budget.remaining());
            }
            // Only the explicitly returned capture slots remain charged.
            assert_eq!(
                memory.live_bytes(),
                actual.len() * std::mem::size_of::<Option<Span>>()
            );
            assert!(memory.peak_bytes() > memory.live_bytes());
            drop(result);
            assert_eq!(memory.live_bytes(), 0);
            assert_eq!(external_side_live_bytes(), before);
        }
    }
}

#[test]
fn perex_host_running_search_survives_reentrant_receiver_recompile() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    let receiver =
        scope.root_raw_mut_ptr(crate::regex::test_alloc_nursery_regexp_for_move("(a+)", ""));
    {
        let initial_scope = RuntimeHandleScope::new();
        let initial = compile(&initial_scope, "(a+)", "").into_storage();
        unsafe {
            initial.install(&receiver);
        }
    }
    let active = BoundProgram::new(
        unsafe { GcProgram::from_receiver(&scope, &receiver).unwrap() },
        &mut Budget::new(100_000),
    )
    .unwrap();
    let input = subject(&scope, b"aaa");
    let memory = MemoryBudget::new(1 << 20);
    let mut budget = Budget::new(100_000);
    let mut polls = 0;
    let old_result = host::find(
        &active,
        &input,
        0,
        CaptureMode::All,
        &mut budget,
        &memory,
        1,
        &mut || {
            polls += 1;
            if polls == 2 {
                let callback_scope = RuntimeHandleScope::new();
                let next = compile(&callback_scope, "z+", "").into_storage();
                unsafe {
                    next.install(&receiver);
                }
            }
            gc_collect_minor();
            Ok(())
        },
    )
    .unwrap()
    .unwrap();
    assert!(polls > 2);
    assert_eq!(
        &**old_result.captures.as_ref().unwrap(),
        &[Span::new(0, 3), Span::new(0, 3)]
    );
    drop(old_result);
    let next = BoundProgram::new(
        unsafe { GcProgram::from_receiver(&scope, &receiver).unwrap() },
        &mut Budget::new(100_000),
    )
    .unwrap();
    assert!(host::find(
        &next,
        &input,
        0,
        CaptureMode::Full,
        &mut budget,
        &memory,
        1,
        &mut host::poll
    )
    .unwrap()
    .is_none());
    assert_eq!(memory.live_bytes(), 0);
}

#[test]
fn perex_host_failures_release_scratch_and_preserve_consumed_work() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    let memory = MemoryBudget::new(1 << 20);
    let before = external_side_live_bytes();
    // Nonclass v patterns are now supported. Keep the former failure input
    // as an explicit successful match, and exercise Unsupported cleanup with
    // the still-unimplemented v class grammar.
    {
        let program = compile(&scope, "a", "v");
        let input = subject(&scope, b"ba");
        let result = host::find(
            &program,
            &input,
            0,
            CaptureMode::All,
            &mut Budget::new(100_000),
            &memory,
            1,
            &mut host::poll,
        )
        .unwrap()
        .unwrap();
        assert_eq!(&**result.captures.as_ref().unwrap(), &[Span::new(1, 2)]);
    }
    assert_eq!(memory.live_bytes(), 0);
    assert_eq!(external_side_live_bytes(), before);
    // `[a]` under `v` compiles now that the engine implements the union
    // grammar; its set *operators* are what remain unimplemented.
    for (pattern, flags, work) in [("(", "", 100_000), ("[a--b]", "v", 100_000), ("a", "", 0)] {
        let input = subject(&scope, pattern.as_bytes());
        let mut budget = Budget::new(work);
        let result = host::compile(
            &scope,
            &input,
            validate_and_canonicalize_flags(flags),
            &mut budget,
            &memory,
            1 << 20,
            &mut host::poll,
        );
        assert!(matches!(result, Err(EngineError::Compile(_))));
        assert_eq!(memory.live_bytes(), 0);
        assert_eq!(external_side_live_bytes(), before);
        if work == 0 {
            assert!(matches!(
                result,
                Err(EngineError::Compile(CompileError::WorkLimit))
            ));
        } else if flags == "v" {
            assert!(matches!(
                result,
                Err(EngineError::Compile(CompileError::Unsupported {
                    feature: "Unicode sets",
                    ..
                }))
            ));
        } else {
            assert!(matches!(
                result,
                Err(EngineError::Compile(CompileError::Syntax { .. }))
            ));
        }
    }
    let input = subject(&scope, b"a+");
    let mut compile_work = Budget::new(100_000);
    let mut calls = 0;
    let cancelled = host::compile(
        &scope,
        &input,
        validate_and_canonicalize_flags(""),
        &mut compile_work,
        &memory,
        1 << 20,
        &mut || {
            calls += 1;
            if calls == 2 {
                Err(EngineError::Cancelled)
            } else {
                Ok(())
            }
        },
    );
    assert!(matches!(cancelled, Err(EngineError::Cancelled)));
    assert!(compile_work.remaining() < 100_000);
    assert_eq!(memory.live_bytes(), 0);
    let program = compile(&scope, "(a+)", "");
    let input = subject(&scope, b"aaaa");
    let mut work = Budget::new(0);
    assert!(matches!(
        host::find(
            &program,
            &input,
            0,
            CaptureMode::All,
            &mut work,
            &memory,
            1,
            &mut host::poll
        ),
        Err(EngineError::Execution(ExecError::WorkLimit))
    ));
    let tiny = MemoryBudget::new(0);
    assert!(matches!(
        host::find(
            &program,
            &input,
            0,
            CaptureMode::All,
            &mut work,
            &tiny,
            1,
            &mut host::poll
        ),
        Err(EngineError::Storage(StorageError::Limit))
    ));
    assert_eq!(tiny.live_bytes(), 0);
    for unwind in [false, true] {
        let mut work = Budget::new(100_000);
        let mut calls = 0;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            host::find(
                &program,
                &input,
                0,
                CaptureMode::All,
                &mut work,
                &memory,
                1,
                &mut || {
                    calls += 1;
                    if calls == 2 {
                        if unwind {
                            panic!("test search callback unwind");
                        }
                        return Err(EngineError::Cancelled);
                    }
                    Ok(())
                },
            )
        }));
        if unwind {
            assert!(result.is_err());
        } else {
            assert!(matches!(result, Ok(Err(EngineError::Cancelled))));
        }
        assert!(work.remaining() < 100_000);
        assert_eq!(memory.live_bytes(), 0);
        assert_eq!(external_side_live_bytes(), before);
    }
}

#[test]
fn perex_host_global_empty_matches_keep_surrogate_halves_and_share_work() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    let input = subject(&scope, b"\xf0\x9f\x98\x80\xed\xa0\x80x");
    let memory = MemoryBudget::new(1 << 20);
    for (flags, expected) in [("g", &[0, 1, 2, 3, 4][..]), ("gu", &[0, 2, 3, 4][..])] {
        let program = compile(&scope, "(?:)", flags);
        let mut budget = Budget::new(100_000);
        let mut start = 0;
        for &index in expected {
            let before = budget.remaining();
            let result = host::find(
                &program,
                &input,
                start,
                CaptureMode::Full,
                &mut budget,
                &memory,
                1,
                &mut host::poll,
            )
            .unwrap()
            .unwrap();
            assert_eq!(result.full, Span::new(index, index).unwrap());
            assert!(result.captures.is_none());
            assert!(budget.remaining() < before);
            start = host::advance_empty(&input, index, flags.contains('u')).unwrap();
        }
        assert_eq!(start, 5);
        assert!(host::find(
            &program,
            &input,
            start,
            CaptureMode::Full,
            &mut budget,
            &memory,
            1,
            &mut host::poll
        )
        .unwrap()
        .is_none());
        assert_eq!(memory.live_bytes(), 0);
    }
    assert_eq!(host::advance_empty(&input, 1, true).unwrap(), 2);
}
