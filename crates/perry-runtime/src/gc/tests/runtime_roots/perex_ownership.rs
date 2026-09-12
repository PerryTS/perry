//! Actual collector witnesses for Perex's program edge and scoped input.
//! These assert movement and sole-root reachability, not just matching output.
use super::*;
use crate::regex::perex_owner::{BuildError, GcProgram, HeapSubject};
use perex::binding::{
    BoundProgram, BoundResources, BoundSubject, ImmutableProgram, ImmutableSubject, Subject,
};
use perex::compiler::{prepare, CompileError, Node, Range};
use perex::executor::{Frame, Progress, Scratch, Search, Undo};
use perex::input::Input;
use perex::{span::Span, Budget};

fn compile<'a>(scope: &'a RuntimeHandleScope, pattern: &str, flags: &str) -> GcProgram<'a> {
    let mut nodes = vec![Node::default(); 256];
    let mut ranges = vec![Range::default(); 128];
    let mut budget = Budget::new(100_000);
    let plan = prepare(
        Input::utf8(pattern),
        flags,
        &mut nodes,
        &mut ranges,
        &mut budget,
    )
    .unwrap();
    GcProgram::emit(scope, plan, 1 << 20).unwrap()
}

fn bound_program<'a>(owner: GcProgram<'a>) -> BoundProgram<GcProgram<'a>> {
    BoundProgram::new(owner, &mut Budget::new(100_000)).unwrap()
}

#[test]
fn perex_program_survives_only_through_a_moving_regexp_edge() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    let receiver =
        scope.root_raw_mut_ptr(crate::regex::test_alloc_nursery_regexp_for_move("a+", ""));
    let receiver_before = receiver.get_raw_mut_ptr::<crate::regex::RegExpHeader>() as usize;
    let program_before;
    let expected;
    {
        let compile_scope = RuntimeHandleScope::new();
        let program = compile(&compile_scope, "a+", "");
        expected = program.with_words(|words| words.to_vec()).unwrap();
        unsafe {
            program.install(&receiver);
        }
        program_before = receiver.with_const_ptr::<crate::regex::RegExpHeader, _>(|r| unsafe {
            (*r).perex_program as usize
        });
        assert!(crate::arena::pointer_in_nursery(program_before));
        assert_eq!(
            test_heap_child_slot_count(program_before as *mut u8),
            0,
            "program words are a GC leaf"
        );
    }
    // No independent program root survives here. Omitting the RegExp edge
    // from the layout visitor must make this test fail.
    let cycles = copying_minor_cycles();
    gc_collect_minor();
    assert!(copying_minor_cycles() > cycles);
    assert_ne!(
        receiver_before,
        receiver.get_raw_mut_ptr::<crate::regex::RegExpHeader>() as usize
    );
    let program_after = receiver.with_const_ptr::<crate::regex::RegExpHeader, _>(|r| unsafe {
        (*r).perex_program as usize
    });
    assert_ne!(
        program_before, program_after,
        "the program itself must move"
    );
    assert!(build_valid_pointer_set().contains(&program_after));
    let program = unsafe { GcProgram::from_receiver(&scope, &receiver).unwrap() };
    assert_eq!(
        program.with_words(|words| words.to_vec()).unwrap(),
        expected
    );
    let _validated = bound_program(program);
}

#[test]
fn perex_search_reborrows_relocated_original_wtf8_during_every_pause() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    let program = bound_program(compile(&scope, r"(?<letter>a)(😀|\ud800)", "u"));
    let bytes = b"prefix-a\xed\xa0\x80-suffix";
    let subject_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    assert!(crate::arena::pointer_in_nursery(subject_ptr as usize));
    unsafe {
        (*subject_ptr).refcount = 1;
    }
    let subject = unsafe { HeapSubject::new(scope.root_string_ptr(subject_ptr)).unwrap() };
    assert_eq!(
        unsafe { (*subject_ptr).refcount },
        0,
        "a binding must prevent unique-owner append mutation"
    );
    subject
        .with_subject(|subject| {
            let Subject::Wtf8(view) = subject else {
                panic!("must borrow original bytes")
            };
            assert_eq!(
                view.as_ptr(),
                crate::string::string_data(subject_ptr) as *const u8
            );
            assert_eq!(view, bytes);
        })
        .unwrap();
    let subject = BoundSubject::new(subject).unwrap();
    let program_before = program.with_view(|p| p.words().as_ptr() as usize).unwrap();
    assert_eq!(subject.with_view(|input| input.len_utf16()).unwrap(), 16);
    let resources = BoundResources {
        program: &program,
        subject: &subject,
    };
    let mut registers = [0; 32];
    let mut frames = [Frame::default(); 128];
    let mut undo = [Undo::default(); 256];
    let mut search = Search::new(
        &resources,
        0,
        Scratch {
            registers: &mut registers,
            frames: &mut frames,
            undo: &mut undo,
        },
        Budget::new(100_000),
    )
    .unwrap();
    let cycles = copying_minor_cycles();
    let mut pauses = 0;
    loop {
        match search.advance(1).unwrap() {
            Progress::Pending => {
                pauses += 1;
                assert!(pauses < 10_000);
                gc_collect_minor();
                // Reuse retired nursery storage so a stale base cannot pass
                // merely because its previous bytes have not changed yet.
                for _ in 0..8 {
                    let _ = crate::string::js_string_from_bytes(
                        b"########################".as_ptr(),
                        24,
                    );
                }
            }
            Progress::Matched => break,
            Progress::NoMatch => panic!("match vanished after relocation"),
        }
    }
    assert!(pauses > 1 && copying_minor_cycles() > cycles);
    assert_ne!(
        program_before,
        program.with_view(|p| p.words().as_ptr() as usize).unwrap()
    );
    assert_eq!(search.capture_count(), 3);
    assert_eq!(search.capture(0).unwrap(), Span::new(7, 9));
    assert_eq!(search.capture(1).unwrap(), Span::new(7, 8));
    assert_eq!(search.capture(2).unwrap(), Span::new(8, 9));
    // Validate the original lone surrogate remains observable as a code unit.
    subject
        .with_view(|input| {
            assert_eq!(
                Span::new(8, 9)
                    .unwrap()
                    .units(input)
                    .unwrap()
                    .collect::<Vec<_>>(),
                vec![0xd800]
            );
        })
        .unwrap();
    drop(search);
    subject
        .into_storage()
        .with_subject(|subject| {
            let Subject::Wtf8(view) = subject else {
                panic!("must retain original byte representation")
            };
            assert_ne!(
                view.as_ptr() as usize,
                subject_ptr as usize + std::mem::size_of::<crate::string::StringHeader>()
            );
            assert_eq!(view, bytes);
        })
        .unwrap();
}

#[test]
fn perex_unreferenced_program_is_reclaimed_without_a_native_owner_or_cache() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();
    let address;
    {
        let scope = RuntimeHandleScope::new();
        let receiver = scope.root_raw_mut_ptr(crate::regex::test_alloc_nursery_regexp_for_move(
            "reclaim", "",
        ));
        let program = compile(&scope, "reclaim", "");
        unsafe {
            program.install(&receiver);
        }
        address = receiver.with_const_ptr::<crate::regex::RegExpHeader, _>(|r| unsafe {
            (*r).perex_program as usize
        });
        assert!(build_valid_pointer_set().contains(&address));
        assert!(crate::arena::pointer_in_nursery(address));
    }
    let cycles = copying_minor_cycles();
    gc_collect_minor();
    assert!(copying_minor_cycles() > cycles);
    assert!(
        !build_valid_pointer_set().contains(&address),
        "the last owner's death must reclaim the program"
    );
}

#[test]
fn perex_failed_emission_does_not_replace_a_live_receiver_program() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    let receiver =
        scope.root_raw_mut_ptr(crate::regex::test_alloc_nursery_regexp_for_move("old", ""));
    let old = compile(&scope, "old", "");
    unsafe {
        old.install(&receiver);
    }
    let expected = old.with_words(|words| words.to_vec()).unwrap();
    let mut witnessed_limit = false;
    for work in 1..500 {
        let mut nodes = [Node::default(); 64];
        let mut ranges = [Range::default(); 64];
        let mut budget = Budget::new(work);
        if let Ok(plan) = prepare(
            Input::utf8("different"),
            "",
            &mut nodes,
            &mut ranges,
            &mut budget,
        ) {
            if matches!(
                GcProgram::emit(&scope, plan, 1 << 20),
                Err(BuildError::Compile(CompileError::WorkLimit))
            ) {
                witnessed_limit = true;
                break;
            }
        };
    }
    assert!(
        witnessed_limit,
        "the failure must occur after preparation, during emission"
    );
    let retained = unsafe { GcProgram::from_receiver(&scope, &receiver).unwrap() };
    assert_eq!(
        retained.with_words(|words| words.to_vec()).unwrap(),
        expected
    );
    let mut nodes = [Node::default(); 64];
    let mut ranges = [Range::default(); 64];
    let mut budget = Budget::new(100_000);
    let plan = prepare(
        Input::utf8("different"),
        "",
        &mut nodes,
        &mut ranges,
        &mut budget,
    )
    .unwrap();
    assert!(matches!(
        GcProgram::emit(&scope, plan, 0),
        Err(BuildError::SizeLimit)
    ));
}

#[test]
fn perex_window_reads_original_allocation_after_collection_during_search() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    let text = "x😀y".as_bytes();
    let input = scope.root_string_ptr(crate::string::js_string_from_bytes(
        text.as_ptr(),
        text.len() as u32,
    ));
    let original = input.get_raw_const_ptr::<crate::StringHeader>() as usize;
    let owner = unsafe { HeapSubject::window(input, 1, 5).unwrap() };
    owner
        .with_subject(|view| {
            let Subject::Wtf8(bytes) = view else {
                panic!("window must retain original byte storage")
            };
            assert_eq!(bytes.as_ptr(), unsafe {
                crate::string::string_data(input.get_raw_const_ptr()).add(1)
            });
        })
        .unwrap();
    let subject = BoundSubject::new(owner).unwrap();
    let program = bound_program(compile(&scope, "^😀$", "u"));
    let memory = crate::regex::perex_memory::MemoryBudget::new(1 << 20);
    let mut budget = Budget::new(100_000);
    let mut polls = 0;
    let found = crate::regex::perex_runtime::find(
        &program,
        &subject,
        0,
        crate::regex::perex_runtime::CaptureMode::All,
        &mut budget,
        &memory,
        1,
        &mut || {
            polls += 1;
            gc_collect_minor();
            Ok(())
        },
    )
    .unwrap()
    .unwrap();
    assert!(polls > 1);
    assert_ne!(
        original,
        input.get_raw_const_ptr::<crate::StringHeader>() as usize
    );
    assert_eq!(found.full, Span::new(0, 2).unwrap());
    drop(found);
    assert_eq!(memory.live_bytes(), 0);
    subject
        .into_storage()
        .with_subject(|view| {
            let Subject::Wtf8(bytes) = view else {
                panic!("window changed representation")
            };
            assert_eq!(bytes, "😀".as_bytes());
            assert_eq!(bytes.as_ptr(), unsafe {
                crate::string::string_data(input.get_raw_const_ptr()).add(1)
            });
        })
        .unwrap();
}

#[test]
fn perex_window_rejects_invalid_bounds_and_partial_byte_sequences() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    let bytes = "x😀y".as_bytes();
    let input = scope.root_string_ptr(crate::string::js_string_from_bytes(
        bytes.as_ptr(),
        bytes.len() as u32,
    ));
    for (start, end) in [(4, 3), (0, 7), (2, 5), (1, 4)] {
        let owner = unsafe { HeapSubject::window(input, start, end).unwrap() };
        assert!(
            BoundSubject::new(owner).is_err(),
            "invalid byte window {start}..{end}"
        );
    }
}
