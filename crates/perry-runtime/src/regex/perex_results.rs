//! Complete exec-result construction from scalar Perex captures. Named groups
//! have null prototypes and indices.groups aliases the numbered pair objects.
use super::perex_api::{OUTPUT_BYTES, QUANTUM};
use super::perex_owner::{GcProgram, HeapSubject};
use super::perex_runtime::{self as host, EngineError, Match};
use super::perex_strings::{copy_name, copy_span_value_near};
use crate::array::ArrayHeader;
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::object::ObjectHeader;
use crate::string::StringHeader;
use perex::binding::{BoundProgram, BoundSubject};
use perex::Budget;

fn null_groups(scope: &RuntimeHandleScope) -> RuntimeHandle<'_> {
    let value = crate::object::js_object_create(f64::from_bits(crate::value::TAG_NULL));
    scope.root_nanbox_f64(value)
}

pub(super) fn materialize(
    input: &RuntimeHandle<'_>,
    subject: &BoundSubject<HeapSubject<'_>>,
    program: &BoundProgram<GcProgram<'_>>,
    found: &Match<'_>,
    near: Option<perex::input::Position>,
    has_indices: bool,
    budget: &mut Budget,
    poll: &mut impl FnMut() -> Result<(), EngineError>,
) -> Result<(*mut ArrayHeader, *mut ObjectHeader), EngineError> {
    let captures = found.captures.as_ref().ok_or(EngineError::InvalidSpan)?;
    let scope = RuntimeHandleScope::new();
    // Born with the named-property reserve: `index`/`input`/`groups` (and
    // `indices`) are installed below without moving an element.
    let result_keys = if has_indices {
        crate::array::NamedPropertyKeys::ExecResultIndices
    } else {
        crate::array::NamedPropertyKeys::ExecResult
    };
    #[cfg(test)]
    super::perex_api::before_result_alloc();
    let array =
        crate::array::js_array_alloc_named_props_reserved(captures.len() as u32, result_keys);
    let array = scope.root_raw_mut_ptr(array);
    array.with_mut_ptr::<ArrayHeader, _>(|array| unsafe {
        (*array).length = captures.len() as u32;
    });
    let mut stride = host::PieceStride::new();
    for (index, capture) in captures.iter().enumerate() {
        let value = if let Some(span) = capture {
            capture_value(subject, *span, near, budget, &mut stride, poll)?.to_bits()
        } else {
            crate::value::TAG_UNDEFINED
        };
        // Re-read after `copy_span`, which allocates; the store itself does not.
        array.with_mut_ptr::<ArrayHeader, _>(|array| unsafe {
            crate::array::store_array_slot(array, index, value);
        });
    }
    let indices = if has_indices {
        let array = crate::array::js_array_alloc_named_props_reserved(
            captures.len() as u32,
            crate::array::NamedPropertyKeys::IndicesGroups,
        );
        let array = scope.root_raw_mut_ptr(array);
        array.with_mut_ptr::<ArrayHeader, _>(|array| unsafe {
            (*array).length = captures.len() as u32;
        });
        for (index, capture) in captures.iter().enumerate() {
            let value = if let Some(span) = capture {
                let pair = crate::array::js_array_alloc(2);
                unsafe {
                    (*pair).length = 2;
                    crate::array::store_array_slot(pair, 0, (span.start() as f64).to_bits());
                    crate::array::store_array_slot(pair, 1, (span.end() as f64).to_bits());
                }
                crate::value::js_nanbox_pointer(pair as i64).to_bits()
            } else {
                crate::value::TAG_UNDEFINED
            };
            // Re-read after the pair allocation above; the store does not allocate.
            array.with_mut_ptr::<ArrayHeader, _>(|array| unsafe {
                crate::array::store_array_slot(array, index, value);
            });
        }
        Some(array)
    } else {
        None
    };
    let names = program
        .with_view(|p| p.name_count())
        .map_err(EngineError::Program)?;
    let groups = (names != 0).then(|| null_groups(&scope));
    let index_groups = (names != 0 && has_indices).then(|| null_groups(&scope));
    for name in 0..names {
        let member_count = program
            .with_view(|p| p.named_group(name).map(|n| n.capture_indices().len()))
            .map_err(EngineError::Program)?
            .ok_or(EngineError::InvalidSpan)?;
        let mut selected = None;
        let mut offset = 0;
        while offset < member_count && selected.is_none() {
            let end = offset.saturating_add(QUANTUM).min(member_count);
            host::charge(budget, end - offset)?;
            selected = program
                .with_view(|p| {
                    p.named_group(name)
                        .and_then(|n| n.capture_indices().get(offset..end))
                        .and_then(|members| {
                            members
                                .iter()
                                .copied()
                                .find(|&i| captures.get(i as usize).is_some_and(Option::is_some))
                        })
                })
                .map_err(EngineError::Program)?;
            offset = end;
            if offset < member_count && selected.is_none() {
                poll()?;
            }
        }
        let key = copy_name(program, name, budget, OUTPUT_BYTES, QUANTUM, poll)?;
        let key = scope.root_string_ptr(key);
        for (owner, array) in [
            (groups.as_ref(), Some(&array)),
            (index_groups.as_ref(), indices.as_ref()),
        ] {
            if let (Some(owner), Some(array)) = (owner, array) {
                let value = selected.map_or(f64::from_bits(crate::value::TAG_UNDEFINED), |index| {
                    array.with_const_ptr::<ArrayHeader, _>(|array| {
                        crate::array::js_array_get_f64(array, index)
                    })
                });
                // `create_data_property` roots all three values before it can allocate.
                let key = key.with_const_ptr::<StringHeader, _>(|key| {
                    crate::value::js_nanbox_string(key as i64)
                });
                if !crate::proxy::create_data_property(owner.get_nanbox_f64(), key, value) {
                    return Err(EngineError::InvalidSpan);
                }
            }
        }
    }
    let groups_value = groups.as_ref().map_or(
        f64::from_bits(crate::value::TAG_UNDEFINED),
        RuntimeHandle::get_nanbox_f64,
    );
    // Nothing from here to the install allocates on the GC heap: the refcount
    // bump is Rust-owned and the inline install is plain slot stores, so the
    // boxed input and every array address stay current.
    let input_value = input
        .with_const_ptr::<StringHeader, _>(|input| crate::value::js_nanbox_string(input as i64));
    crate::string::js_string_addref_if_heap_string(input_value);
    let mut values = [
        found.full.start() as f64,
        input_value,
        groups_value,
        f64::from_bits(crate::value::TAG_UNDEFINED),
    ];
    if let Some(indices) = indices.as_ref() {
        values[3] = indices.with_mut_ptr::<ArrayHeader, _>(|indices| {
            crate::value::js_nanbox_pointer(indices as i64)
        });
    }
    let value_count = result_keys.keys().len();
    array.with_mut_ptr::<ArrayHeader, _>(|array| unsafe {
        crate::array::array_named_props_install(array, &values[..value_count]);
    });
    if let Some(indices) = indices.as_ref() {
        let groups_value = index_groups.as_ref().map_or(
            f64::from_bits(crate::value::TAG_UNDEFINED),
            RuntimeHandle::get_nanbox_f64,
        );
        indices.with_mut_ptr::<ArrayHeader, _>(|indices| unsafe {
            crate::array::array_named_props_install(indices, &[groups_value]);
        });
    }
    let groups = groups.as_ref().map_or(std::ptr::null_mut(), |g| {
        crate::value::js_nanbox_get_pointer(g.get_nanbox_f64()) as *mut ObjectHeader
    });
    Ok((array.with_mut_ptr::<ArrayHeader, _>(|array| array), groups))
}

/// Only the demanded capture is observable; neither the result nor any of its
/// metadata escapes. Keep the numbered slot at its original index, including
/// an own undefined slot for an unmatched group. Out-of-range reads continue
/// to use the ordinary array prototype chain.
pub(super) fn materialize_capture(
    subject: &BoundSubject<HeapSubject<'_>>,
    found: &Match<'_>,
    near: Option<perex::input::Position>,
    index: usize,
    budget: &mut Budget,
    poll: &mut impl FnMut() -> Result<(), EngineError>,
) -> Result<*mut ArrayHeader, EngineError> {
    let captures = found.captures.as_ref().ok_or(EngineError::InvalidSpan)?;
    let scope = RuntimeHandleScope::new();
    #[cfg(test)]
    super::perex_api::before_result_alloc();
    let array = scope.root_raw_mut_ptr(crate::array::js_array_alloc(captures.len() as u32));
    array.with_mut_ptr::<ArrayHeader, _>(|array| unsafe {
        (*array).length = captures.len() as u32;
    });
    let mut stride = host::PieceStride::new();
    if let Some(capture) = captures.get(index) {
        let value = match capture {
            Some(span) => capture_value(subject, *span, near, budget, &mut stride, poll)?,
            None => f64::from_bits(crate::value::TAG_UNDEFINED),
        };
        array.with_mut_ptr::<ArrayHeader, _>(|array| unsafe {
            crate::array::store_array_slot(array, index, value.to_bits());
        });
    }
    Ok(array.with_mut_ptr::<ArrayHeader, _>(|array| array))
}

/// Inline captures own their bytes before any collecting action. Account for
/// them as bounded output pieces, using the same work stride as split and
/// replace. Heap/non-ASCII copies retain their quantum polls. The search has
/// already performed its capture poll, so fewer than one output block remains
/// unpolled at result completion; there is no per-thread batching state here.
fn capture_value(
    subject: &BoundSubject<HeapSubject<'_>>,
    span: perex::span::Span,
    near: Option<perex::input::Position>,
    budget: &mut Budget,
    stride: &mut host::PieceStride,
    poll: &mut impl FnMut() -> Result<(), EngineError>,
) -> Result<f64, EngineError> {
    if let Some(value) = super::perex_strings::short_ascii_span(subject, span, budget)? {
        if stride.due(span.len()) {
            poll()?;
        }
        return Ok(value);
    }
    copy_span_value_near(subject, span, near, budget, OUTPUT_BYTES, QUANTUM, poll)
}

#[cfg(test)]
mod projection_tests {
    use super::*;
    use crate::regex::tests::make_string;
    use perex::span::Span;

    #[test]
    fn inline_capture_output_polls_by_bounded_work_and_cancels() {
        let _global = crate::gc::global_side_table_test_lock();
        let _threshold = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let scope = RuntimeHandleScope::new();
        let input = scope.root_string_ptr(make_string("abc"));
        let subject = crate::regex::perex_api::bind_heap_subject(input).unwrap();
        let mut budget = Budget::new(100_000);
        let mut stride = host::PieceStride::new();
        let mut polls = 0;
        // Four work units per copy: three bytes plus its piece. A run with no
        // heap allocations still must reach a poll within the output block.
        for _ in 0..256 {
            capture_value(
                &subject,
                Span::new(0, 3).unwrap(),
                None,
                &mut budget,
                &mut stride,
                &mut || {
                    polls += 1;
                    Ok(())
                },
            )
            .unwrap();
        }
        assert_eq!(polls, 2);
        assert_eq!(budget.remaining(), 100_000 - 256 * 3);
        let mut cancelled = false;
        for _ in 0..128 {
            let result = capture_value(
                &subject,
                Span::new(0, 3).unwrap(),
                None,
                &mut budget,
                &mut stride,
                &mut || Err(EngineError::Cancelled),
            );
            if matches!(result, Err(EngineError::Cancelled)) {
                cancelled = true;
                break;
            }
        }
        assert!(cancelled);
    }

    #[test]
    fn builtin_and_projected_exec_reject_symbol_arguments() {
        let _global = crate::gc::global_side_table_test_lock();
        let _threshold = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let scope = RuntimeHandleScope::new();
        let re = scope.root_raw_mut_ptr(crate::regex::js_regexp_new(
            make_string("(x)"),
            make_string(""),
        ));
        let receiver =
            scope.root_nanbox_f64(re.with_const_ptr::<crate::regex::RegExpHeader, _>(|re| {
                crate::value::js_nanbox_pointer(re as i64)
            }));
        let method =
            scope.root_nanbox_f64(crate::regex::perex_dispatch::get(&receiver, b"exec").unwrap());
        let symbol = scope.root_nanbox_f64(unsafe { crate::symbol::js_symbol_new_empty() });
        assert!(crate::regex::perex_dispatch::call_one(&method, &receiver, &symbol).is_err());
        assert!(
            crate::exception::catch_js_throw(|| crate::regex::js_regexp_exec_projected(
                receiver.get_nanbox_f64(),
                method.get_nanbox_f64(),
                symbol.get_nanbox_f64(),
                1.0
            ))
            .is_err()
        );
    }

    #[test]
    fn projected_result_only_materializes_demanded_numbered_capture() {
        let _global = crate::gc::global_side_table_test_lock();
        let _threshold = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let scope = RuntimeHandleScope::new();
        let re = scope.root_raw_mut_ptr(crate::regex::js_regexp_new(
            make_string("(a)(b)(c)(d)(e)(f)(g)"),
            make_string(""),
        ));
        let input = scope.root_string_ptr(make_string("abcdefg"));
        let mut polls = 0;
        let found = crate::regex::perex_api::execute_output(
            re.with_mut_ptr(|p| p),
            input.with_const_ptr(|p| p),
            crate::regex::perex_api::ExecOutput::Capture(7),
            &mut Budget::new(crate::regex::perex_api::WORK),
            &crate::regex::perex_memory::MemoryBudget::new(crate::regex::perex_api::SCRATCH_BYTES),
            &mut || {
                polls += 1;
                Ok(())
            },
            None,
        )
        .unwrap()
        .unwrap();
        let array = scope.root_raw_mut_ptr(found.array);
        array.with_const_ptr::<ArrayHeader, _>(|array| unsafe {
            assert_eq!((*array).length, 8);
            assert_eq!(
                crate::array::js_array_get_f64(array, 7).to_bits(),
                crate::value::JSValue::short_string_unchecked(b"g").bits()
            );
            assert_eq!(
                crate::array::js_array_get_f64(array, 0).to_bits(),
                crate::value::TAG_UNDEFINED
            );
        });
        // The mandatory capture poll remains; no per-inline-string poll.
        assert!(polls <= 2);
        assert!(polls >= 1);
    }
}
