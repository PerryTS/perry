//! Exact capture strings, bounded original-input traversal and real moving GC.
use super::*;
use crate::regex::perex_memory::StorageError;
use crate::regex::perex_owner::HeapSubject;
use crate::regex::perex_runtime::EngineError;
use crate::regex::perex_strings::copy_span;
use perex::binding::BoundSubject;
use perex::executor::ExecError;
use perex::{span::Span, Budget};

fn subject<'s>(scope: &'s RuntimeHandleScope, bytes: &[u8]) -> BoundSubject<HeapSubject<'s>> {
    let ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    BoundSubject::new(unsafe { HeapSubject::new(scope.root_string_ptr(ptr)).unwrap() }).unwrap()
}

// Independent standard-library UTF-16 decoder for expected result strings.
fn expected_wtf8(units: &[u16]) -> (Vec<u8>, u32) {
    let mut bytes = Vec::new();
    let mut flags = 0;
    for point in char::decode_utf16(units.iter().copied()) {
        match point {
            Ok(ch) => bytes.extend_from_slice(ch.encode_utf8(&mut [0; 4]).as_bytes()),
            Err(error) => {
                let unit = error.unpaired_surrogate();
                bytes.extend_from_slice(&[
                    0xe0 | (unit >> 12) as u8,
                    0x80 | ((unit >> 6) & 63) as u8,
                    0x80 | (unit & 63) as u8,
                ]);
                flags |= crate::string::STRING_FLAG_HAS_LONE_SURROGATES;
            }
        }
    }
    (bytes, flags)
}

#[test]
fn perex_capture_strings_preserve_every_half_pair_and_lone_surrogate() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    let units = [0x61, 0xd83d, 0xde00, 0xd800, 0x62, 0xdc00, 0, 0xe9];
    // Test both scalar and separately encoded adjacent surrogate storage.
    for bytes in [
        &b"a\xf0\x9f\x98\x80\xed\xa0\x80b\xed\xb0\x80\0\xc3\xa9"[..],
        &b"a\xed\xa0\xbd\xed\xb8\x80\xed\xa0\x80b\xed\xb0\x80\0\xc3\xa9"[..],
    ] {
        let input = subject(&scope, bytes);
        for start in 0..=units.len() {
            for end in start..=units.len() {
                let (expected, flags) = expected_wtf8(&units[start..end]);
                let mut work = None;
                for quantum in [1, 3, 1000] {
                    let output_scope = RuntimeHandleScope::new();
                    let mut budget = Budget::new(100_000);
                    let result = copy_span(
                        &input,
                        Span::new(start, end).unwrap(),
                        &mut budget,
                        1000,
                        quantum,
                        &mut || {
                            gc_collect_minor();
                            Ok(())
                        },
                    )
                    .unwrap();
                    let result = output_scope.root_string_ptr(result);
                    gc_collect_minor();
                    result.with_const_ptr::<crate::StringHeader, _>(|header| unsafe {
                        assert_eq!((*header).utf16_len as usize, end - start);
                        assert_eq!((*header).byte_len as usize, expected.len());
                        assert_eq!((*header).capacity as usize, expected.len());
                        assert_eq!((*header).flags, flags);
                    });
                    unsafe {
                        result.with_string_bytes(|bytes| assert_eq!(bytes, expected));
                    }
                    if let Some(previous) = work {
                        assert_eq!(budget.remaining(), previous);
                    } else {
                        work = Some(budget.remaining());
                    }
                }
            }
        }
    }
}

/// Only initialized prefixes may be visible when the collector can run.
fn live_output(capacity: usize) -> Option<usize> {
    let mut found = None;
    let valid = build_valid_pointer_set();
    crate::arena::arena_walk_objects(|header| unsafe {
        let gc = header as *const GcHeader;
        if (*gc).obj_type != GC_TYPE_STRING
            || (*gc).gc_flags & GC_FLAG_FORWARDED != 0
            || ((*gc).size as usize)
                < GC_HEADER_SIZE + std::mem::size_of::<crate::StringHeader>() + capacity
        {
            return;
        }
        let address = header.add(GC_HEADER_SIZE) as usize;
        if !valid.contains(&address) {
            return;
        }
        let string = address as *const crate::StringHeader;
        if (*string).capacity as usize != capacity {
            return;
        }
        assert!(
            found.is_none(),
            "fixture output capacity must identify one live string"
        );
        let bytes = std::slice::from_raw_parts(
            crate::string::string_data(string),
            (*string).byte_len as usize,
        );
        let input = perex::input::Input::wtf8(bytes).unwrap();
        assert_eq!(input.len_utf16(), (*string).utf16_len as usize);
        assert_eq!((*string).flags, 0);
        found = Some(address);
    });
    found
}

#[test]
fn perex_capture_output_itself_moves_during_bounded_construction() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    let source = format!("PRE{}POST", "é".repeat(37));
    let input = subject(&scope, source.as_bytes());
    assert!(live_output(74).is_none());
    let mut first = None;
    let mut observed_move = false;
    let result = copy_span(
        &input,
        Span::new(3, 40).unwrap(),
        &mut Budget::new(100_000),
        1000,
        1,
        &mut || {
            if let Some(current) = live_output(74) {
                if let Some(original) = first {
                    observed_move |= original != current;
                } else {
                    first = Some(current);
                }
            }
            gc_collect_minor();
            for _ in 0..4 {
                let _ = crate::string::js_string_from_bytes(b"################".as_ptr(), 16);
            }
            Ok(())
        },
    )
    .unwrap();
    let result = scope.root_string_ptr(result);
    assert!(first.is_some() && observed_move);
    assert_ne!(
        result.get_raw_const_ptr::<crate::StringHeader>() as usize,
        first.unwrap()
    );
    unsafe {
        result.with_string_bytes(|bytes| assert_eq!(bytes, "é".repeat(37).as_bytes()));
    }
}

#[test]
fn perex_capture_cancellation_unwind_and_limits_release_partial_output_roots() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    let source = format!("{}x", "é".repeat(37));
    let input = subject(&scope, source.as_bytes());
    let span = Span::new(0, 37).unwrap();
    let root_count = RuntimeHandleScope::active_len_for_tests();
    for unwind in [false, true] {
        let mut address = None;
        let mut budget = Budget::new(100_000);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            copy_span(&input, span, &mut budget, 1000, 1, &mut || {
                if let Some(output) = live_output(74) {
                    address = Some(output);
                    if unwind {
                        panic!("capture output callback unwound");
                    }
                    return Err(EngineError::Cancelled);
                }
                gc_collect_minor();
                Ok(())
            })
        }));
        if unwind {
            assert!(result.is_err());
        } else {
            assert!(matches!(result, Ok(Err(EngineError::Cancelled))));
        }
        assert!(
            address.is_some(),
            "must cancel after final-output allocation"
        );
        assert!(budget.remaining() < 100_000);
        assert_eq!(RuntimeHandleScope::active_len_for_tests(), root_count);
        gc_collect_minor();
        assert!(!build_valid_pointer_set().contains(&address.unwrap()));
        assert!(live_output(74).is_none());
    }
    // Enough work for the complete size pass, none for the allocated fill pass.
    let mut budget = Budget::new(37);
    assert!(matches!(
        copy_span(&input, span, &mut budget, 1000, 1, &mut || Ok(())),
        Err(EngineError::Execution(ExecError::WorkLimit))
    ));
    assert_eq!(budget.remaining(), 0);
    assert_eq!(RuntimeHandleScope::active_len_for_tests(), root_count);
    let unrooted = live_output(74).expect("failure must have reached output allocation");
    gc_collect_minor();
    assert!(!build_valid_pointer_set().contains(&unrooted));
    assert!(matches!(
        copy_span(&input, span, &mut Budget::new(1000), 73, 1, &mut || Ok(())),
        Err(EngineError::Storage(StorageError::Limit))
    ));
    assert!(live_output(74).is_none());
    assert!(matches!(
        copy_span(
            &input,
            Span::new(0, 99).unwrap(),
            &mut Budget::new(1000),
            1000,
            1,
            &mut || Ok(())
        ),
        Err(EngineError::InvalidSpan)
    ));
}
