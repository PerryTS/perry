//! String replace/replaceAll and RegExp @@replace. RegExpExec results are
//! collected before calling replacers; offsets and traced strings drive output.
use super::perex_api as api;
use super::perex_dispatch as dispatch;
use super::perex_match_search::{advance, scan_flags, subject};
use super::perex_memory::{MemoryBudget, StorageError};
use super::perex_replace_storage::{boxed, call, length, text, List, Pieces};
use super::perex_runtime::{self as host, EngineError};
use super::perex_substitution::Substitution;
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::value::{js_nanbox_string, TAG_NULL, TAG_UNDEFINED};
use perex::Budget;

fn coercible(value: f64) -> Result<(), EngineError> {
    if matches!(value.to_bits(), TAG_NULL | TAG_UNDEFINED) {
        Err(EngineError::Type(
            "String replacement requires a non-null receiver",
        ))
    } else {
        Ok(())
    }
}

pub(super) fn callable(value: &RuntimeHandle<'_>) -> Result<bool, EngineError> {
    if crate::proxy::proxy_wraps_callable(value.get_nanbox_f64()) {
        return Ok(true);
    }
    if !crate::proxy::reflect_value_is_object(value.get_nanbox_f64()) {
        return Ok(false);
    }
    // Function.prototype has an ordinary object representation in Perry.
    // Reacquire the candidate after possibly lazy intrinsic initialization.
    let prototype = api::caught(|| crate::object::builtin_prototype_value("Function"))?;
    Ok(prototype.to_bits() == value.get_nanbox_f64().to_bits())
}

pub(super) fn index_property(
    result: &RuntimeHandle<'_>,
    mut index: usize,
) -> Result<f64, EngineError> {
    let mut key = [0u8; 20];
    let mut start = key.len();
    loop {
        start -= 1;
        key[start] = b'0' + (index % 10) as u8;
        index /= 10;
        if index == 0 {
            break;
        }
    }
    dispatch::get(result, &key[start..])
}

pub(crate) fn regexp(receiver: f64, argument: f64, replacement: f64) -> Result<f64, EngineError> {
    dispatch::require_object(receiver)?;
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let argument = scope.root_nanbox_f64(argument);
    let replacement = scope.root_nanbox_f64(replacement);
    let input = text(&scope, &argument)?;
    let functional = callable(&replacement)?;
    let template = if functional {
        None
    } else {
        Some(text(&scope, &replacement)?)
    };
    let memory = MemoryBudget::new(api::SCRATCH_BYTES);
    let mut budget = Budget::new(api::WORK);
    let flags = scope.root_nanbox_f64(dispatch::get(&receiver, b"flags")?);
    let flags = text(&scope, &flags)?;
    let (global, unicode) = scan_flags(&flags, &mut budget)?;
    if global {
        dispatch::set_last_index(&receiver, 0.0)?;
    }
    let mut results = List::new(&scope)?;
    let bound = subject(input)?;
    let input_length = length(&input);
    loop {
        let local = RuntimeHandleScope::new();
        let Some(result) = dispatch::execute(
            &receiver,
            &input,
            true,
            &mut budget,
            &memory,
            &mut host::poll,
        )?
        else {
            break;
        };
        let result = local.root_nanbox_f64(result.object());
        results.push(result.get_nanbox_f64(), &mut budget)?;
        if !global {
            break;
        }
        let matched = local.root_nanbox_f64(dispatch::get(&result, b"0")?);
        let matched = text(&local, &matched)?;
        if length(&matched) == 0 {
            let index = local.root_nanbox_f64(dispatch::get(&receiver, b"lastIndex")?);
            let index = dispatch::to_length(&index)?;
            let next = advance(&bound, index, input_length, unicode, &mut budget)?;
            dispatch::set_last_index(&receiver, next)?;
        }
        host::poll()?;
    }
    if results.len() == 0 {
        return Ok(boxed(&input));
    }
    let mut output = Pieces::new(&scope)?;
    let mut next_source = 0;
    for index in 0..results.len() {
        let local = RuntimeHandleScope::new();
        let result = local.root_nanbox_f64(results.get(index));
        let count = local.root_nanbox_f64(dispatch::get(&result, b"length")?);
        let count = (dispatch::to_length(&count)? - 1.0).max(0.0);
        if count > (api::SCRATCH_BYTES / 8) as f64 {
            return Err(StorageError::Limit.into());
        }
        let matched = local.root_nanbox_f64(dispatch::get(&result, b"0")?);
        let matched = text(&local, &matched)?;
        let position = local.root_nanbox_f64(dispatch::get(&result, b"index")?);
        // ToLength followed by this clamp agrees with ToIntegerOrInfinity
        // followed by clamping to [0, input.length], including negative zero.
        let position = dispatch::to_length(&position)?.min(input_length as f64) as usize;
        let mut captures = List::new(&local)?;
        for capture in 1..=count as usize {
            let capture_scope = RuntimeHandleScope::new();
            let value = capture_scope.root_nanbox_f64(index_property(&result, capture)?);
            if value.get_nanbox_f64().to_bits() == TAG_UNDEFINED {
                captures.push(value.get_nanbox_f64(), &mut budget)?;
            } else {
                let value = text(&capture_scope, &value)?;
                captures.push(boxed(&value), &mut budget)?;
            }
            if capture % api::QUANTUM == 0 {
                host::poll()?;
            }
        }
        let groups = local.root_nanbox_f64(dispatch::get(&result, b"groups")?);
        let accepted = position >= next_source;
        if accepted {
            output.append(&input, next_source, position, &mut budget)?;
        }
        if functional {
            let mut args = List::new(&local)?;
            args.push(boxed(&matched), &mut budget)?;
            for i in 0..captures.len() {
                args.push(captures.get(i), &mut budget)?;
            }
            args.push(position as f64, &mut budget)?;
            args.push(boxed(&input), &mut budget)?;
            if groups.get_nanbox_f64().to_bits() != TAG_UNDEFINED {
                args.push(groups.get_nanbox_f64(), &mut budget)?;
            }
            let this = local.root_nanbox_f64(f64::from_bits(TAG_UNDEFINED));
            let value = local.root_nanbox_f64(call(&replacement, &this, &args, &memory)?);
            let value = text(&local, &value)?;
            if accepted {
                output.whole(&value, &mut budget)?;
            }
        } else {
            let groups = if groups.get_nanbox_f64().to_bits() == TAG_UNDEFINED {
                None
            } else {
                coercible(groups.get_nanbox_f64())?;
                Some(local.root_nanbox_f64(api::caught(|| {
                    crate::object::js_object_coerce(groups.get_nanbox_f64())
                })?))
            };
            Substitution {
                input: &input,
                matched: &matched,
                position,
                captures: &captures,
                groups: groups.as_ref(),
                template: template.as_ref().unwrap(),
            }
            .append(&mut output, accepted, &mut budget)?;
        }
        if accepted {
            next_source = position + length(&matched);
        }
        host::poll()?;
    }
    if next_source < input_length {
        output.append(&input, next_source, input_length, &mut budget)?;
    }
    output
        .finish(&input, template.as_ref(), &mut budget)
        .map(|s| js_nanbox_string(s as i64))
}

pub(crate) fn string(
    all: bool,
    receiver: f64,
    search: f64,
    replacement: f64,
) -> Result<f64, EngineError> {
    coercible(receiver)?;
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let search = scope.root_nanbox_f64(search);
    let replacement = scope.root_nanbox_f64(replacement);
    let memory = MemoryBudget::new(api::SCRATCH_BYTES);
    let mut budget = Budget::new(api::WORK);
    if crate::proxy::reflect_value_is_object(search.get_nanbox_f64()) {
        if all && dispatch::is_regexp(&search)? {
            let flags = scope.root_nanbox_f64(dispatch::get(&search, b"flags")?);
            coercible(flags.get_nanbox_f64())?;
            let flags = text(&scope, &flags)?;
            if !scan_flags(&flags, &mut budget)?.0 {
                return Err(EngineError::Type(
                    "String.prototype.replaceAll requires a global RegExp",
                ));
            }
        }
        let method = scope.root_nanbox_f64(dispatch::get_symbol(&search, "replace")?);
        if !matches!(method.get_nanbox_f64().to_bits(), TAG_NULL | TAG_UNDEFINED) {
            if !callable(&method)? {
                return Err(EngineError::Type("Symbol.replace is not callable"));
            }
            let mut args = List::new(&scope)?;
            args.push(receiver.get_nanbox_f64(), &mut budget)?;
            args.push(replacement.get_nanbox_f64(), &mut budget)?;
            return call(&method, &search, &args, &memory);
        }
    }
    let input = text(&scope, &receiver)?;
    let needle = text(&scope, &search)?;
    let functional = callable(&replacement)?;
    let template = if functional {
        None
    } else {
        Some(text(&scope, &replacement)?)
    };
    let mut positions = List::new(&scope)?;
    super::perex_literal_search::positions(
        &input,
        &needle,
        all,
        &mut positions,
        &mut budget,
        &memory,
    )?;
    if positions.len() == 0 {
        return Ok(boxed(&input));
    }
    let mut output = Pieces::new(&scope)?;
    let captures = List::new(&scope)?;
    let mut end = 0;
    for index in 0..positions.len() {
        let local = RuntimeHandleScope::new();
        let position = positions.get(index) as usize;
        output.append(&input, end, position, &mut budget)?;
        if functional {
            let mut args = List::new(&local)?;
            args.push(boxed(&needle), &mut budget)?;
            args.push(position as f64, &mut budget)?;
            args.push(boxed(&input), &mut budget)?;
            let this = local.root_nanbox_f64(f64::from_bits(TAG_UNDEFINED));
            let value = local.root_nanbox_f64(call(&replacement, &this, &args, &memory)?);
            let value = text(&local, &value)?;
            output.whole(&value, &mut budget)?;
        } else {
            Substitution {
                input: &input,
                matched: &needle,
                position,
                captures: &captures,
                groups: None,
                template: template.as_ref().unwrap(),
            }
            .append(&mut output, true, &mut budget)?;
        }
        end = position + length(&needle);
        host::poll()?;
    }
    output.append(&input, end, length(&input), &mut budget)?;
    output
        .finish(&input, template.as_ref(), &mut budget)
        .map(|s| js_nanbox_string(s as i64))
}

pub(crate) extern "C" fn regexp_thunk(
    _: *const crate::closure::ClosureHeader,
    input: f64,
    replacement: f64,
) -> f64 {
    api::finish(regexp(
        crate::object::js_implicit_this_get(),
        input,
        replacement,
    ))
}

#[no_mangle]
pub extern "C" fn js_string_replace_js(receiver: f64, search: f64, replacement: f64) -> f64 {
    api::finish(string(false, receiver, search, replacement))
}
#[no_mangle]
pub extern "C" fn js_string_replace_all_js(receiver: f64, search: f64, replacement: f64) -> f64 {
    api::finish(string(true, receiver, search, replacement))
}
