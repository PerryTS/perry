//! Complete exec-result construction from scalar Perex captures. Named groups
//! have null prototypes and indices.groups aliases the numbered pair objects.
use super::perex_api::{OUTPUT_BYTES, QUANTUM};
use super::perex_owner::{GcProgram, HeapSubject};
use super::perex_runtime::{self as host, EngineError, Match};
use super::perex_strings::{copy_name, copy_span};
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
    has_indices: bool,
    budget: &mut Budget,
    poll: &mut impl FnMut() -> Result<(), EngineError>,
) -> Result<(*mut ArrayHeader, *mut ObjectHeader), EngineError> {
    let captures = found.captures.as_ref().ok_or(EngineError::InvalidSpan)?;
    let scope = RuntimeHandleScope::new();
    let result = crate::array::js_array_alloc(captures.len() as u32);
    let result = scope.root_raw_mut_ptr(result);
    result.with_mut_ptr::<ArrayHeader, _>(|result| unsafe {
        (*result).length = captures.len() as u32;
    });
    for (index, capture) in captures.iter().enumerate() {
        let value = if let Some(span) = capture {
            let text = copy_span(subject, *span, budget, OUTPUT_BYTES, QUANTUM, poll)?;
            crate::value::js_nanbox_string(text as i64).to_bits()
        } else {
            crate::value::TAG_UNDEFINED
        };
        // Re-read after `copy_span`, which allocates; the store itself does not.
        result.with_mut_ptr::<ArrayHeader, _>(|result| unsafe {
            crate::array::store_array_slot(result, index, value);
        });
    }
    let indices = if has_indices {
        let array = crate::array::js_array_alloc(captures.len() as u32);
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
            (groups.as_ref(), Some(&result)),
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
    // bump and the named-property side table are Rust-owned, so the boxed input
    // and the array address stay current.
    let input_value = input
        .with_const_ptr::<StringHeader, _>(|input| crate::value::js_nanbox_string(input as i64));
    crate::string::js_string_addref_if_heap_string(input_value);
    result.with_mut_ptr::<ArrayHeader, _>(|result| unsafe {
        crate::array::array_named_props_install_fresh(
            result,
            &[
                ("index", found.full.start() as f64),
                ("input", input_value),
                ("groups", groups_value),
            ],
        );
    });
    if let Some(indices) = indices {
        let groups_value = index_groups.as_ref().map_or(
            f64::from_bits(crate::value::TAG_UNDEFINED),
            RuntimeHandle::get_nanbox_f64,
        );
        indices.with_mut_ptr::<ArrayHeader, _>(|indices| unsafe {
            crate::array::array_named_props_install_fresh(indices, &[("groups", groups_value)]);
            result.with_mut_ptr::<ArrayHeader, _>(|result| {
                crate::array::array_named_props_install_fresh(
                    result,
                    &[("indices", crate::value::js_nanbox_pointer(indices as i64))],
                );
            });
        });
    }
    let groups = groups.as_ref().map_or(std::ptr::null_mut(), |g| {
        crate::value::js_nanbox_get_pointer(g.get_nanbox_f64()) as *mut ObjectHeader
    });
    Ok((
        result.with_mut_ptr::<ArrayHeader, _>(|result| result),
        groups,
    ))
}
