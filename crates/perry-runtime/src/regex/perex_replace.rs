//! String replace/replaceAll and RegExp @@replace. RegExpExec results are
//! collected before calling replacers; offsets and traced strings drive output.
use super::perex_api as api;
use super::perex_dispatch as dispatch;
use super::perex_match_search::{advance, scan_flags, subject};
use super::perex_memory::{MemoryBudget, StorageError};
use super::perex_output::{Offsets, Output, Text};
use super::perex_replace_storage::{boxed, call, call_native, length, text, List, NativeArgs};
use super::perex_runtime::{self as host, EngineError};
use super::perex_substitution::{emit, parse, Parts};
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::string::StringHeader;
use crate::value::{TAG_NULL, TAG_UNDEFINED};
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
    // A primitive is never callable; a string replacement is the common case.
    if crate::value::JSValue::from_bits(value.get_nanbox_u64()).is_any_string() {
        return Ok(false);
    }
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

/// RegExp.prototype[@@replace] over a subject the pattern provably cannot
/// match: the subject itself, with a failed RegExpBuiltinExec's `lastIndex`,
/// before any handle scope, binding or coercion frame. `None` leaves the call
/// to the ordinary algorithm below.
///
/// Steps 2-5 (ToString of the subject; IsCallable and ToString of the
/// replacement) run no user code for a string subject and a string
/// replacement. The builtin RegExp fact proves that steps 6-8 read the
/// builtin flags getters, Set a writable data `lastIndex` and call the
/// builtin exec, and the pattern's leading units (`perex_lead`) prove that
/// exec fails from every start. A failed exec of a global or sticky RegExp
/// leaves `lastIndex` 0; a non-stateful one only reads it, which is
/// unobservable for a Number (anything else takes the ordinary path).
#[inline]
fn unmatched(receiver: f64, argument: f64, replacement: f64) -> Option<f64> {
    use crate::value::JSValue;
    #[cfg(test)]
    if super::perex_replace_direct::direct_disabled() {
        return None;
    }
    if !JSValue::from_bits(argument.to_bits()).is_any_string()
        || !JSValue::from_bits(replacement.to_bits()).is_any_string()
    {
        return None;
    }
    let re = crate::regex::instance::builtin(receiver)?;
    // SAFETY: admitted just now; nothing below collects.
    unsafe {
        let data = &*crate::regex::instance::builtin_data(re);
        if !super::perex_lead::excludes_value(data, argument) {
            return None;
        }
        if data.global {
            crate::regex::instance::builtin_reset_last_index(re);
        } else {
            let last_index = crate::regex::instance::builtin_last_index(re);
            if !JSValue::from_bits(last_index.to_bits()).is_number() {
                return None;
            }
            if data.sticky {
                crate::regex::instance::builtin_reset_last_index(re);
            }
        }
    }
    // The result aliases the subject: no later in-place append may reuse it.
    crate::string::js_string_addref_if_heap_string(argument);
    #[cfg(test)]
    super::perex_replace_direct::note_unmatched_exit();
    Some(argument)
}

pub(crate) fn regexp(receiver: f64, argument: f64, replacement: f64) -> Result<f64, EngineError> {
    if let Some(unchanged) = unmatched(receiver, argument, replacement) {
        return Ok(unchanged);
    }
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
    // Coercions above may execute user code; prove the current receiver now.
    // This one proof authorizes both the flags read and direct-loop admission.
    // A builtin RegExp's nonconfigurable own lastIndex is data: its reset
    // either stores without user code or throws, even when nonwritable.
    let builtin = crate::regex::instance::builtin(receiver.get_nanbox_f64());
    let (global, unicode) = match builtin {
        // SAFETY: admitted just now; nothing collected since.
        Some(re) => unsafe {
            let data = &*crate::regex::instance::builtin_data(re);
            (data.global, data.unicode)
        },
        None => super::perex_match_search::match_flags(&receiver, &mut budget)?,
    };
    if global {
        match builtin {
            // SAFETY: as above; reading the flags collected nothing.
            Some(re) => unsafe { crate::regex::instance::builtin_reset_last_index(re) },
            None => dispatch::set_last_index(&receiver, 0.0)?,
        }
    }
    let bound = subject(input)?;
    let reuse = api::Reuse::new(&scope, &receiver, input, &bound, &mut budget);
    if super::perex_replace_direct::admissible(&receiver, &reuse) {
        return super::perex_replace_direct::replace(
            &scope,
            &receiver,
            &input,
            &bound,
            &reuse,
            global,
            unicode,
            &replacement,
            template.as_ref(),
            &mut budget,
            &memory,
        );
    }
    // Only the general path collects results; the direct one never did.
    let mut results = List::new(&scope)?;
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
            Some(&reuse),
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
    let mut subject_text = Text::new(&bound)?;
    let mut output = Output::with_capacity(byte_length(&input))?;
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
                output.span(&mut subject_text, next_source, position, &mut budget)?;
                output.string(&value, &mut budget)?;
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
            let template = template.as_ref().unwrap();
            // A result's capture count and groups are its own, so the
            // template is read against each result.
            let tokens = parse(template, captures.len(), groups.is_some())?;
            if accepted {
                output.span(&mut subject_text, next_source, position, &mut budget)?;
            }
            // Named references Get from the groups object even for a result
            // that is not replaced; the output is discarded then.
            let mut scratch;
            let target = if accepted {
                &mut output
            } else {
                scratch = Output::with_capacity(0)?;
                &mut scratch
            };
            emit(
                &tokens,
                template,
                &mut subject_text,
                position,
                &mut Results {
                    matched: &matched,
                    captures: &captures,
                    groups: groups.as_ref(),
                },
                target,
                &mut budget,
            )?;
        }
        if accepted {
            next_source = position + length(&matched);
        }
        host::poll()?;
    }
    if next_source < input_length {
        output.span(&mut subject_text, next_source, input_length, &mut budget)?;
    }
    output.finish()
}

fn byte_length(s: &RuntimeHandle<'_>) -> usize {
    s.with_const_ptr::<StringHeader, _>(|s| unsafe { (*s).byte_len as usize })
}

/// An exec result's parts: JS strings the result object supplied, which need
/// not be spans of the subject.
struct Results<'a, 's> {
    matched: &'a RuntimeHandle<'s>,
    captures: &'a List<'s>,
    groups: Option<&'a RuntimeHandle<'s>>,
}

impl Parts for Results<'_, '_> {
    fn matched_len(&self) -> usize {
        length(self.matched)
    }
    fn matched(
        &mut self,
        _: &mut Text<'_, '_>,
        output: &mut Output,
        budget: &mut Budget,
    ) -> Result<(), EngineError> {
        output.string(self.matched, budget)
    }
    fn capture(
        &mut self,
        index: usize,
        _: &mut Text<'_, '_>,
        output: &mut Output,
        budget: &mut Budget,
    ) -> Result<(), EngineError> {
        let value = self.captures.get(index - 1);
        if value.to_bits() == TAG_UNDEFINED {
            return Ok(());
        }
        let scope = RuntimeHandleScope::new();
        let value = scope.root_nanbox_f64(value);
        output.value(&value, budget)
    }
    fn groups(&self) -> Option<&RuntimeHandle<'_>> {
        self.groups
    }
}

/// A literal pattern's match: the matched text is a span of the subject.
struct Literal {
    start: usize,
    length: usize,
}

impl Parts for Literal {
    fn matched_len(&self) -> usize {
        self.length
    }
    fn matched(
        &mut self,
        input: &mut Text<'_, '_>,
        output: &mut Output,
        budget: &mut Budget,
    ) -> Result<(), EngineError> {
        output.span(input, self.start, self.start + self.length, budget)
    }
    fn capture(
        &mut self,
        _: usize,
        _: &mut Text<'_, '_>,
        _: &mut Output,
        _: &mut Budget,
    ) -> Result<(), EngineError> {
        Err(EngineError::InvalidSpan)
    }
}

/// `Some(global)` when `search` is an untouched RegExp whose `@@replace` is
/// the builtin, and for replaceAll also its `@@match` (IsRegExp), so every Get
/// String.prototype.replace/replaceAll makes of it is unobservable. The flag
/// accessors are part of the proof, so `global` is read from the header.
fn builtin_replace(_all: bool, search: f64) -> Option<bool> {
    let re = crate::regex::instance::builtin(search)?;
    // SAFETY: admitted just now; nothing collected since.
    Some(unsafe { (*crate::regex::instance::builtin_data(re)).global })
}

pub(crate) fn string(
    all: bool,
    receiver: f64,
    search: f64,
    replacement: f64,
) -> Result<f64, EngineError> {
    coercible(receiver)?;
    if let Some(global) = builtin_replace(all, search) {
        // replaceAll's IsRegExp and flags Gets, and the @@replace Get, would
        // each reach a builtin without running code. Through the generic
        // property path they were most of a short `replace` (#10518).
        if all && !global {
            return Err(EngineError::Type(
                "String.prototype.replaceAll called with a non-global RegExp argument",
            ));
        }
        return regexp(search, receiver, replacement);
    }
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
                    "String.prototype.replaceAll called with a non-global RegExp argument",
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
    let source = subject(input)?;
    let pattern = subject(needle)?;
    let mut positions = Offsets::new();
    super::perex_literal_search::positions(
        &source,
        &pattern,
        all,
        &mut positions,
        &mut budget,
        &memory,
    )?;
    if positions.values.is_empty() {
        return Ok(boxed(&input));
    }
    let tokens = template
        .as_ref()
        .map(|template| parse(template, 0, false))
        .transpose()?;
    let needle_length = length(&needle);
    let input_length = length(&input);
    let mut subject_text = Text::new(&source)?;
    let mut output = Output::with_capacity(byte_length(&input))?;
    // An ordinary replacer's arguments go straight into shadow-stack slots;
    // a proxy's reach its `apply` trap as an array.
    let mut native_args =
        if functional && crate::proxy::js_proxy_is_proxy(replacement.get_nanbox_f64()) != 1 {
            Some(NativeArgs::new(3)?)
        } else {
            None
        };
    let this = scope.root_nanbox_f64(f64::from_bits(TAG_UNDEFINED));
    let mut end = 0;
    for &position in &positions.values {
        let position = position as usize;
        output.span(&mut subject_text, end, position, &mut budget)?;
        match (&tokens, &template) {
            (Some(tokens), Some(template)) => emit(
                tokens,
                template,
                &mut subject_text,
                position,
                &mut Literal {
                    start: position,
                    length: needle_length,
                },
                &mut output,
                &mut budget,
            )?,
            _ => {
                let local = RuntimeHandleScope::new();
                let value = match native_args.as_mut() {
                    Some(args) => call_native(&replacement, &this, args, &memory, |set| {
                        set(0, boxed(&needle));
                        set(1, position as f64);
                        set(2, boxed(&input));
                        Ok(())
                    })?,
                    None => {
                        let mut args = List::new(&local)?;
                        args.push(boxed(&needle), &mut budget)?;
                        args.push(position as f64, &mut budget)?;
                        args.push(boxed(&input), &mut budget)?;
                        call(&replacement, &this, &args, &memory)?
                    }
                };
                let value = local.root_nanbox_f64(value);
                output.value(&value, &mut budget)?;
            }
        }
        end = position + needle_length;
    }
    output.span(&mut subject_text, end, input_length, &mut budget)?;
    output.finish()
}

pub(crate) extern "C" fn regexp_thunk(
    _: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    input: f64,
    replacement: f64,
) -> f64 {
    api::finish(regexp(this.as_f64(), input, replacement))
}

pub extern "C" fn js_string_replace_js(receiver: f64, search: f64, replacement: f64) -> f64 {
    api::finish(string(false, receiver, search, replacement))
}
pub extern "C" fn js_string_replace_all_js(receiver: f64, search: f64, replacement: f64) -> f64 {
    api::finish(string(true, receiver, search, replacement))
}
