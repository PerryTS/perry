//! String match/search and their RegExp symbol methods. Original strings stay
//! rooted; only final JS output is materialized, and one budget spans a loop.
use super::perex_api as api;
use super::perex_dispatch as dispatch;
use super::perex_memory::MemoryBudget;
use super::perex_owner::HeapSubject;
use super::perex_runtime::{self as host, EngineError};
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::string::StringHeader;
use crate::value::{js_nanbox_pointer, js_nanbox_string, TAG_NULL, TAG_UNDEFINED};
use perex::binding::BoundSubject;
use perex::span::{BoundSpan, ReadProgress, Span};
use perex::Budget;

#[derive(Clone, Copy)]
pub(crate) enum Operation {
    Match,
    Search,
}
impl Operation {
    fn symbol(self) -> &'static str {
        match self {
            Self::Match => "match",
            Self::Search => "search",
        }
    }
}

pub(crate) fn flags(receiver: f64) -> Result<*mut StringHeader, EngineError> {
    dispatch::require_object(receiver)?;
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let mut output = [0u8; 8];
    let mut length = 0;
    for (name, byte) in [
        (b"hasIndices".as_slice(), b'd'),
        (b"global", b'g'),
        (b"ignoreCase", b'i'),
        (b"multiline", b'm'),
        (b"dotAll", b's'),
        (b"unicode", b'u'),
        (b"unicodeSets", b'v'),
        (b"sticky", b'y'),
    ] {
        if crate::value::js_is_truthy(dispatch::get(&receiver, name)?) != 0 {
            output[length] = byte;
            length += 1;
        }
    }
    api::caught(|| crate::string::js_string_from_bytes(output.as_ptr(), length as u32))
}

pub(super) fn subject(
    input: RuntimeHandle<'_>,
) -> Result<BoundSubject<HeapSubject<'_>>, EngineError> {
    BoundSubject::new(
        unsafe { HeapSubject::new(input) }
            .map_err(|e| EngineError::Subject(perex::binding::SubjectError::Resource(e)))?,
    )
    .map_err(|e| EngineError::Subject(e.error))
}

fn match_flags(
    receiver: &RuntimeHandle<'_>,
    budget: &mut Budget,
) -> Result<(bool, bool), EngineError> {
    let scope = RuntimeHandleScope::new();
    let flags = scope.root_nanbox_f64(dispatch::get(receiver, b"flags")?);
    let flags = scope.root_string_ptr(dispatch::to_string(&flags)?);
    scan_flags(&flags, budget)
}

pub(super) fn scan_flags(
    flags: &RuntimeHandle<'_>,
    budget: &mut Budget,
) -> Result<(bool, bool), EngineError> {
    let mut offset = 0;
    let (mut global, mut unicode) = (false, false);
    loop {
        let (end, done) = unsafe {
            flags.with_string_bytes(|bytes| {
                let end = (offset + api::QUANTUM).min(bytes.len());
                for byte in &bytes[offset..end] {
                    global |= *byte == b'g';
                    unicode |= *byte == b'u' || *byte == b'v';
                }
                (end, end == bytes.len())
            })
        };
        host::charge(budget, end - offset)?;
        if done {
            return Ok((global, unicode));
        }
        offset = end;
        host::poll()?;
    }
}

pub(super) fn advance(
    subject: &BoundSubject<HeapSubject<'_>>,
    index: f64,
    length: usize,
    unicode: bool,
    budget: &mut Budget,
) -> Result<f64, EngineError> {
    if !unicode || index + 1.0 >= length as f64 {
        return Ok(index + 1.0);
    }
    let start = index as usize;
    let mut reader = BoundSpan::new(
        subject,
        Span::new(start, start + 2).ok_or(EngineError::InvalidSpan)?,
    )
    .map_err(|e| super::perex_strings::read_error(e, |never| match never {}))?;
    let mut first = None;
    let mut pair = false;
    loop {
        let progress = reader
            .try_fold(api::QUANTUM, budget, |unit| {
                if let Some(high) = first {
                    pair = (0xd800..=0xdbff).contains(&high) && (0xdc00..=0xdfff).contains(&unit);
                } else {
                    first = Some(unit);
                }
                Ok::<(), EngineError>(())
            })
            .map_err(|e| super::perex_strings::read_error(e, |e| e))?;
        if progress == ReadProgress::Complete {
            return Ok(index + if pair { 2.0 } else { 1.0 });
        }
        host::poll()?;
    }
}

pub(crate) fn regexp(
    operation: Operation,
    receiver: f64,
    argument: f64,
) -> Result<f64, EngineError> {
    dispatch::require_object(receiver)?;
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let argument = scope.root_nanbox_f64(argument);
    let input = scope.root_string_ptr(dispatch::to_string(&argument)?);
    regexp_string(operation, &receiver, &input)
}

fn regexp_string(
    operation: Operation,
    receiver: &RuntimeHandle<'_>,
    input: &RuntimeHandle<'_>,
) -> Result<f64, EngineError> {
    let memory = MemoryBudget::new(api::SCRATCH_BYTES);
    let mut budget = Budget::new(api::WORK);
    match operation {
        Operation::Search => search(receiver, input, &mut budget, &memory),
        Operation::Match => matches(receiver, input, &mut budget, &memory),
    }
}

fn search(
    receiver: &RuntimeHandle<'_>,
    input: &RuntimeHandle<'_>,
    budget: &mut Budget,
    memory: &MemoryBudget,
) -> Result<f64, EngineError> {
    let scope = RuntimeHandleScope::new();
    let previous = scope.root_nanbox_f64(dispatch::get(receiver, b"lastIndex")?);
    if previous.get_nanbox_f64().to_bits() != 0 {
        dispatch::set_last_index(receiver, 0.0)?;
    }
    let result = dispatch::execute(receiver, input, true, budget, memory, &mut host::poll)?;
    let result = scope.root_nanbox_f64(result.map_or(f64::from_bits(TAG_NULL), |r| r.object()));
    let current = scope.root_nanbox_f64(dispatch::get(receiver, b"lastIndex")?);
    if !dispatch::same_value(&current, &previous)? {
        dispatch::set_last_index(receiver, previous.get_nanbox_f64())?;
    }
    if result.get_nanbox_f64().to_bits() == TAG_NULL {
        Ok(-1.0)
    } else {
        dispatch::get(&result, b"index")
    }
}

fn matches(
    receiver: &RuntimeHandle<'_>,
    input: &RuntimeHandle<'_>,
    budget: &mut Budget,
    memory: &MemoryBudget,
) -> Result<f64, EngineError> {
    let (global, unicode) = match_flags(receiver, budget)?;
    if !global {
        return dispatch::execute(receiver, input, true, budget, memory, &mut host::poll)
            .map(|result| result.map_or(f64::from_bits(TAG_NULL), |r| r.object()));
    }
    dispatch::set_last_index(receiver, 0.0)?;
    let scope = RuntimeHandleScope::new();
    let array = scope.root_raw_mut_ptr(api::caught(|| crate::array::js_array_alloc(0))?);
    let subject = subject(*input)?;
    let length = input.with_const_ptr::<StringHeader, _>(|s| unsafe { (*s).utf16_len as usize });
    let mut count = 0u32;
    loop {
        // A fresh scope per iteration bounds roots regardless of match count.
        let iteration = RuntimeHandleScope::new();
        let result = dispatch::execute(receiver, input, false, budget, memory, &mut host::poll)?;
        let Some(result) = result else {
            return Ok(if count == 0 {
                f64::from_bits(TAG_NULL)
            } else {
                js_nanbox_pointer(array.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64)
            });
        };
        let string = match result {
            dispatch::ExecResult::Builtin(found) => api::caught(|| {
                super::perex_strings::copy_span(
                    &subject,
                    found.full,
                    budget,
                    api::OUTPUT_BYTES,
                    api::QUANTUM,
                    &mut host::poll,
                )
            })??,
            dispatch::ExecResult::Override(value) => {
                let result = iteration.root_nanbox_f64(value);
                let value = iteration.root_nanbox_f64(dispatch::get(&result, b"0")?);
                dispatch::to_string(&value)?
            }
        };
        let string = iteration.root_string_ptr(string);
        count = count.checked_add(1).ok_or(EngineError::Storage(
            super::perex_memory::StorageError::Limit,
        ))?;
        let grown = api::caught(|| {
            crate::array::js_array_push_f64(
                array.get_raw_mut_ptr(),
                js_nanbox_string(string.get_raw_const_ptr::<StringHeader>() as i64),
            )
        })?;
        array.set_raw_mut_ptr(grown);
        if string.with_const_ptr::<StringHeader, _>(|s| unsafe { (*s).utf16_len == 0 }) {
            let index = iteration.root_nanbox_f64(dispatch::get(receiver, b"lastIndex")?);
            let index = dispatch::to_length(&index)?;
            let next = advance(&subject, index, length, unicode, budget)?;
            dispatch::set_last_index(receiver, next)?;
        }
        host::poll()?;
    }
}

pub(crate) fn string(
    operation: Operation,
    receiver: f64,
    pattern: f64,
) -> Result<f64, EngineError> {
    let r = crate::value::JSValue::from_bits(receiver.to_bits());
    if r.is_null() || r.is_undefined() {
        return Err(EngineError::Type(
            "String method called on null or undefined",
        ));
    }
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let pattern = scope.root_nanbox_f64(pattern);
    if crate::proxy::reflect_value_is_object(pattern.get_nanbox_f64()) {
        let method = scope.root_nanbox_f64(dispatch::get_symbol(&pattern, operation.symbol())?);
        if !matches!(method.get_nanbox_f64().to_bits(), TAG_UNDEFINED | TAG_NULL) {
            if !crate::proxy::proxy_wraps_callable(method.get_nanbox_f64()) {
                return Err(EngineError::Type("RegExp symbol method is not callable"));
            }
            return dispatch::call_one(&method, &pattern, &receiver);
        }
    }
    let input = scope.root_string_ptr(dispatch::to_string(&receiver)?);
    // RegExpCreate initializes a new intrinsic RegExp. It does not perform
    // the RegExp constructor's IsRegExp/constructor-identity shortcuts.
    let source = if pattern.get_nanbox_f64().to_bits() == TAG_UNDEFINED {
        api::caught(|| crate::string::js_string_from_bytes(b"".as_ptr(), 0))?
    } else {
        dispatch::to_string(&pattern)?
    };
    let source = scope.root_string_ptr(source);
    let flags = scope.root_string_ptr(api::caught(|| {
        crate::string::js_string_from_bytes(b"".as_ptr(), 0)
    })?);
    let re = api::caught(|| {
        super::perex_construct::new(source.get_raw_const_ptr(), flags.get_raw_const_ptr())
    })??;
    let re = scope.root_nanbox_f64(js_nanbox_pointer(re as i64));
    let method = scope.root_nanbox_f64(dispatch::get_symbol(&re, operation.symbol())?);
    if !crate::proxy::proxy_wraps_callable(method.get_nanbox_f64()) {
        return Err(EngineError::Type("RegExp symbol method is not callable"));
    }
    let argument = scope.root_nanbox_f64(js_nanbox_string(
        input.get_raw_const_ptr::<StringHeader>() as i64,
    ));
    dispatch::call_one(&method, &re, &argument)
}

pub(crate) extern "C" fn match_thunk(_: *const crate::closure::ClosureHeader, arg: f64) -> f64 {
    api::finish(regexp(
        Operation::Match,
        crate::object::js_implicit_this_get(),
        arg,
    ))
}
pub(crate) extern "C" fn search_thunk(_: *const crate::closure::ClosureHeader, arg: f64) -> f64 {
    api::finish(regexp(
        Operation::Search,
        crate::object::js_implicit_this_get(),
        arg,
    ))
}
