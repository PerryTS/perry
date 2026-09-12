//! String.split and generic RegExp @@split using original UTF-16 spans.
//! Species/exec hooks retain ordinary traced values; no input conversion buffer.
use super::perex_api as api;
use super::perex_dispatch as dispatch;
use super::perex_match_search::subject;
use super::perex_memory::MemoryBudget;
use super::perex_owner::HeapSubject;
use super::perex_replace::{callable, index_property};
use super::perex_replace_storage::{boxed, call, length, text, List, Pieces, Units};
use super::perex_runtime::{self as host, EngineError};
use super::perex_strings::SpanCopies;
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::value::{js_nanbox_pointer, js_nanbox_string, TAG_NULL, TAG_UNDEFINED};
use perex::binding::{BoundSubject, SubjectError};
use perex::Budget;

/// Literal String operations also accept Perry's raw Buffer/FFI payloads.
/// An encoding error selects byte coordinates for that host operation only;
/// resource failures still propagate and regex input validation stays strict.
fn literal_subject(
    input: RuntimeHandle<'_>,
) -> Result<Option<BoundSubject<HeapSubject<'_>>>, EngineError> {
    match subject(input) {
        Ok(bound) => Ok(Some(bound)),
        Err(EngineError::Subject(SubjectError::Encoding(_))) => Ok(None),
        Err(error) => Err(error),
    }
}

fn limit(value: &RuntimeHandle<'_>) -> Result<usize, EngineError> {
    if value.get_nanbox_f64().to_bits() == TAG_UNDEFINED {
        return Ok(u32::MAX as usize);
    }
    let n = dispatch::to_number(value)?;
    Ok(if !n.is_finite() || n == 0.0 {
        0
    } else {
        n.trunc().rem_euclid(4_294_967_296.0) as usize
    })
}

fn advance(
    units: &mut Units<'_, '_>,
    index: usize,
    size: usize,
    unicode: bool,
    budget: &mut Budget,
) -> Result<usize, EngineError> {
    if unicode
        && index + 1 < size
        && (0xd800..=0xdbff).contains(&units.at(index, budget)?)
        && (0xdc00..=0xdfff).contains(&units.at(index + 1, budget)?)
    {
        return Ok(index + 2);
    }
    Ok(index + 1)
}

fn push_span(
    output: &mut List<'_>,
    copies: &mut SpanCopies<'_, '_>,
    start: usize,
    end: usize,
    budget: &mut Budget,
) -> Result<(), EngineError> {
    let result = copies.copy(start, end, budget)?;
    output.push(js_nanbox_string(result as i64), budget)
}

pub(crate) fn regexp(receiver: f64, argument: f64, limit_value: f64) -> Result<f64, EngineError> {
    dispatch::require_object(receiver)?;
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let argument = scope.root_nanbox_f64(argument);
    let limit_value = scope.root_nanbox_f64(limit_value);
    let input = text(&scope, &argument)?;
    let constructor = super::match_all::species(&receiver)?.map(|v| scope.root_nanbox_f64(v));
    let flags = scope.root_nanbox_f64(dispatch::get(&receiver, b"flags")?);
    let flags = text(&scope, &flags)?;
    let mut budget = Budget::new(api::WORK);
    let memory = MemoryBudget::new(api::SCRATCH_BYTES);
    let mut unicode = false;
    let mut sticky = false;
    {
        let bound = subject(flags)?;
        let mut units = Units::new(&bound)?;
        for index in 0..length(&flags) {
            match units.at(index, &mut budget)? {
                117 | 118 => unicode = true,
                121 => sticky = true,
                _ => (),
            }
        }
    }
    let new_flags = if sticky {
        flags
    } else {
        let y = scope.root_string_ptr(api::caught(|| {
            crate::string::js_string_from_bytes(b"y".as_ptr(), 1)
        })?);
        let mut pieces = Pieces::new(&scope)?;
        pieces.whole(&flags, &mut budget)?;
        pieces.whole(&y, &mut budget)?;
        scope.root_string_ptr(pieces.finish(&flags, Some(&y), &mut budget)?)
    };
    let splitter = scope.root_nanbox_f64(api::caught(|| match &constructor {
        None => js_nanbox_pointer(super::js_regexp_construct(
            receiver.get_nanbox_f64(),
            boxed(&new_flags),
        ) as i64),
        Some(constructor) => crate::object::construct_two_rooted(
            constructor.get_nanbox_f64(),
            receiver.get_nanbox_f64(),
            boxed(&new_flags),
        ),
    })?);
    let mut output = List::new(&scope)?;
    let lim = limit(&limit_value)?;
    if lim == 0 {
        return Ok(output.value());
    }
    let size = length(&input);
    if size == 0 {
        if dispatch::execute(
            &splitter,
            &input,
            true,
            &mut budget,
            &memory,
            &mut host::poll,
        )?
        .is_none()
        {
            output.push(boxed(&input), &mut budget)?;
        }
        return Ok(output.value());
    }
    let bound = subject(input)?;
    let mut units = Units::new(&bound)?;
    let mut copies = SpanCopies::new(&bound)?;
    let (mut p, mut q) = (0, 0);
    while q < size {
        let local = RuntimeHandleScope::new();
        dispatch::set_last_index(&splitter, q as f64)?;
        let found = dispatch::execute(
            &splitter,
            &input,
            true,
            &mut budget,
            &memory,
            &mut host::poll,
        )?;
        if let Some(found) = found {
            let found = local.root_nanbox_f64(found.object());
            let end = local.root_nanbox_f64(dispatch::get(&splitter, b"lastIndex")?);
            let end = dispatch::to_length(&end)?.min(size as f64) as usize;
            if end != p {
                push_span(&mut output, &mut copies, p, q, &mut budget)?;
                if output.len() == lim {
                    return Ok(output.value());
                }
                p = end;
                let count = local.root_nanbox_f64(dispatch::get(&found, b"length")?);
                let count = dispatch::to_length(&count)?;
                let mut capture = 1usize;
                while (capture as f64) < count {
                    // Captures are arbitrary JS values. No Get(0), index,
                    // groups or ToString(capture) occurs in this operation.
                    let value = index_property(&found, capture)?;
                    output.push(value, &mut budget)?;
                    if output.len() == lim {
                        return Ok(output.value());
                    }
                    capture += 1;
                }
                q = p;
            } else {
                q = advance(&mut units, q, size, unicode, &mut budget)?;
            }
        } else {
            q = advance(&mut units, q, size, unicode, &mut budget)?;
        }
        host::poll()?;
    }
    push_span(&mut output, &mut copies, p, size, &mut budget)?;
    Ok(output.value())
}

pub(crate) fn string(receiver: f64, separator: f64, limit_value: f64) -> Result<f64, EngineError> {
    if matches!(receiver.to_bits(), TAG_NULL | TAG_UNDEFINED) {
        return Err(EngineError::Type(
            "String.split requires a non-null receiver",
        ));
    }
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let separator = scope.root_nanbox_f64(separator);
    let limit_value = scope.root_nanbox_f64(limit_value);
    let mut budget = Budget::new(api::WORK);
    let memory = MemoryBudget::new(api::SCRATCH_BYTES);
    if crate::proxy::reflect_value_is_object(separator.get_nanbox_f64()) {
        let method = scope.root_nanbox_f64(dispatch::get_symbol(&separator, "split")?);
        if !matches!(method.get_nanbox_f64().to_bits(), TAG_NULL | TAG_UNDEFINED) {
            if !callable(&method)? {
                return Err(EngineError::Type("Symbol.split is not callable"));
            }
            let mut args = List::new(&scope)?;
            args.push(receiver.get_nanbox_f64(), &mut budget)?;
            args.push(limit_value.get_nanbox_f64(), &mut budget)?;
            return call(&method, &separator, &args, &memory);
        }
    }
    let input = text(&scope, &receiver)?;
    let lim = limit(&limit_value)?;
    let needle = text(&scope, &separator)?;
    let mut output = List::new(&scope)?;
    if lim == 0 {
        return Ok(output.value());
    }
    if separator.get_nanbox_f64().to_bits() == TAG_UNDEFINED {
        output.push(boxed(&input), &mut budget)?;
        return Ok(output.value());
    }
    let bound = literal_subject(input)?;
    let pattern = literal_subject(needle)?;
    let (Some(bound), Some(pattern)) = (bound, pattern) else {
        super::perex_literal_bytes::split(&input, &needle, lim, &mut output, &mut budget, &memory)?;
        return Ok(output.value());
    };
    let mut copies = SpanCopies::new(&bound)?;
    let n = bound
        .with_view(|s| s.len_utf16())
        .map_err(EngineError::Subject)?;
    let m = pattern
        .with_view(|s| s.len_utf16())
        .map_err(EngineError::Subject)?;
    if m == 0 {
        for i in 0..n.min(lim) {
            push_span(&mut output, &mut copies, i, i + 1, &mut budget)?;
        }
        return Ok(output.value());
    }
    let mut end = 0;
    super::perex_literal_search::each_bound(
        &bound,
        &pattern,
        &mut budget,
        &memory,
        |position, budget| {
            push_span(&mut output, &mut copies, end, position, budget)?;
            end = position + m;
            Ok(output.len() == lim)
        },
    )?;
    if output.len() < lim {
        push_span(&mut output, &mut copies, end, n, &mut budget)?;
    }
    Ok(output.value())
}

pub(crate) extern "C" fn regexp_thunk(
    _: *const crate::closure::ClosureHeader,
    input: f64,
    limit: f64,
) -> f64 {
    api::finish(regexp(crate::object::js_implicit_this_get(), input, limit))
}

#[no_mangle]
pub extern "C" fn js_string_split_js(receiver: f64, separator: f64, limit: f64) -> f64 {
    api::finish(string(receiver, separator, limit))
}
