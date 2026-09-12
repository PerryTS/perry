//! Lazy RegExp String Iterators. All retained values are ordinary traced
//! fields; each next call reacquires the original subject and owns its scratch.
use super::perex_api as api;
use super::perex_dispatch as dispatch;
use super::perex_match_search::{advance, scan_flags, subject};
use super::perex_memory::MemoryBudget;
use super::perex_runtime::{self as host, EngineError};
use super::REGEXP_STRING_ITERATOR_CLASS_ID;
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::object::ObjectHeader;
use crate::string::StringHeader;
use crate::value::{
    js_nanbox_get_pointer, js_nanbox_pointer, js_nanbox_string, JSValue, TAG_NULL, TAG_UNDEFINED,
};
use perex::Budget;

const MATCHER: u32 = 0;
const INPUT: u32 = 1;
const GLOBAL: u32 = 2;
const UNICODE: u32 = 3;
const DONE: u32 = 4;

fn get(iter: &RuntimeHandle<'_>, slot: u32) -> f64 {
    f64::from_bits(crate::object::js_object_get_field(iter.get_raw_const_ptr(), slot).bits())
}
fn set(iter: &RuntimeHandle<'_>, slot: u32, value: f64) {
    crate::object::js_object_set_field(
        iter.get_raw_mut_ptr(),
        slot,
        JSValue::from_bits(value.to_bits()),
    );
}
fn complete(iter: &RuntimeHandle<'_>) {
    set(iter, DONE, f64::from_bits(crate::value::TAG_TRUE));
    // Further next calls only inspect DONE. Release the retained graph as soon
    // as it is no longer needed, even if JavaScript keeps the iterator alive.
    set(iter, MATCHER, f64::from_bits(TAG_UNDEFINED));
    set(iter, INPUT, f64::from_bits(TAG_UNDEFINED));
}
fn result(value: f64, done: bool) -> Result<f64, EngineError> {
    api::caught(|| unsafe {
        crate::iter_result::make_iter_result(JSValue::from_bits(value.to_bits()), done)
    })
}

fn allocate(
    matcher: &RuntimeHandle<'_>,
    input: &RuntimeHandle<'_>,
    global: bool,
    unicode: bool,
) -> Result<f64, EngineError> {
    let scope = RuntimeHandleScope::new();
    // Initialize the prototype before allocating the iterator. Lazy intrinsic
    // setup may collect; matcher and input already have independent roots.
    let proto = scope.root_nanbox_f64(api::caught(|| {
        crate::object::iterator_prototype_for_class_id(REGEXP_STRING_ITERATOR_CLASS_ID).unwrap()
    })?);
    let iter = scope.root_raw_mut_ptr(api::caught(|| {
        crate::object::js_object_alloc(REGEXP_STRING_ITERATOR_CLASS_ID, 5)
    })?);
    set(&iter, MATCHER, matcher.get_nanbox_f64());
    set(
        &iter,
        INPUT,
        js_nanbox_string(input.get_raw_const_ptr::<StringHeader>() as i64),
    );
    set(&iter, GLOBAL, f64::from_bits(JSValue::bool(global).bits()));
    set(
        &iter,
        UNICODE,
        f64::from_bits(JSValue::bool(unicode).bits()),
    );
    set(&iter, DONE, f64::from_bits(crate::value::TAG_FALSE));
    api::caught(|| {
        crate::proxy::js_reflect_set_prototype_of(
            js_nanbox_pointer(iter.get_raw_mut_ptr::<ObjectHeader>() as i64),
            proto.get_nanbox_f64(),
        )
    })?;
    Ok(js_nanbox_pointer(
        iter.get_raw_mut_ptr::<ObjectHeader>() as i64
    ))
}

/// SpeciesConstructor: None denotes the intrinsic default, independent of a
/// user replacement of globalThis.RegExp. Preserve the selected constructor
/// through subsequent flag getters and coercions.
pub(super) fn species(receiver: &RuntimeHandle<'_>) -> Result<Option<f64>, EngineError> {
    let scope = RuntimeHandleScope::new();
    let constructor = scope.root_nanbox_f64(dispatch::get(receiver, b"constructor")?);
    if constructor.get_nanbox_f64().to_bits() == TAG_UNDEFINED {
        return Ok(None);
    }
    dispatch::require_object(constructor.get_nanbox_f64())?;
    let species = dispatch::get_symbol(&constructor, "species")?;
    if matches!(species.to_bits(), TAG_NULL | TAG_UNDEFINED) {
        return Ok(None);
    }
    if !crate::proxy::is_constructor_function(species) {
        return Err(EngineError::Type("RegExp species is not a constructor"));
    }
    Ok(Some(species))
}

pub(crate) fn regexp(receiver: f64, argument: f64) -> Result<f64, EngineError> {
    dispatch::require_object(receiver)?;
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let argument = scope.root_nanbox_f64(argument);
    let input = scope.root_string_ptr(dispatch::to_string(&argument)?);
    let constructor = species(&receiver)?.map(|value| scope.root_nanbox_f64(value));
    let flags_value = scope.root_nanbox_f64(dispatch::get(&receiver, b"flags")?);
    let flags = scope.root_string_ptr(dispatch::to_string(&flags_value)?);
    let flags_argument = scope.root_nanbox_f64(js_nanbox_string(
        flags.get_raw_const_ptr::<StringHeader>() as i64,
    ));
    let matcher = scope.root_nanbox_f64(api::caught(|| match &constructor {
        None => js_nanbox_pointer(super::js_regexp_construct(
            receiver.get_nanbox_f64(),
            flags_argument.get_nanbox_f64(),
        ) as i64),
        Some(constructor) => crate::object::construct_two_rooted(
            constructor.get_nanbox_f64(),
            receiver.get_nanbox_f64(),
            flags_argument.get_nanbox_f64(),
        ),
    })?);
    let index = scope.root_nanbox_f64(dispatch::get(&receiver, b"lastIndex")?);
    dispatch::set_last_index(&matcher, dispatch::to_length(&index)?)?;
    let (global, unicode) = scan_flags(&flags, &mut Budget::new(api::WORK))?;
    allocate(&matcher, &input, global, unicode)
}

pub(crate) extern "C" fn regexp_thunk(
    _: *const crate::closure::ClosureHeader,
    argument: f64,
) -> f64 {
    api::finish(regexp(crate::object::js_implicit_this_get(), argument))
}

fn string(receiver: f64, pattern: f64) -> Result<f64, EngineError> {
    if matches!(receiver.to_bits(), TAG_NULL | TAG_UNDEFINED) {
        return Err(EngineError::Type(
            "String.matchAll called on null or undefined",
        ));
    }
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let pattern = scope.root_nanbox_f64(pattern);
    if crate::proxy::reflect_value_is_object(pattern.get_nanbox_f64()) {
        if dispatch::is_regexp(&pattern)? {
            let flags = scope.root_nanbox_f64(dispatch::get(&pattern, b"flags")?);
            if matches!(flags.get_nanbox_f64().to_bits(), TAG_NULL | TAG_UNDEFINED) {
                return Err(EngineError::Type(
                    "RegExp flags cannot be null or undefined",
                ));
            }
            let flags = scope.root_string_ptr(dispatch::to_string(&flags)?);
            if !scan_flags(&flags, &mut Budget::new(api::WORK))?.0 {
                return Err(EngineError::Type(
                    "String.prototype.matchAll called with a non-global RegExp argument",
                ));
            }
        }
        let method = scope.root_nanbox_f64(dispatch::get_symbol(&pattern, "matchAll")?);
        if !matches!(method.get_nanbox_f64().to_bits(), TAG_NULL | TAG_UNDEFINED) {
            if !crate::proxy::proxy_wraps_callable(method.get_nanbox_f64()) {
                return Err(EngineError::Type("RegExp matchAll method is not callable"));
            }
            return dispatch::call_one(&method, &pattern, &receiver);
        }
    }
    let input = scope.root_string_ptr(dispatch::to_string(&receiver)?);
    let source = if pattern.get_nanbox_f64().to_bits() == TAG_UNDEFINED {
        api::caught(|| crate::string::js_string_from_bytes(b"".as_ptr(), 0))?
    } else {
        dispatch::to_string(&pattern)?
    };
    let source = scope.root_string_ptr(source);
    let flags = scope.root_string_ptr(api::caught(|| {
        crate::string::js_string_from_bytes(b"g".as_ptr(), 1)
    })?);
    let re = api::caught(|| {
        super::perex_construct::new(source.get_raw_const_ptr(), flags.get_raw_const_ptr())
    })??;
    let re = scope.root_nanbox_f64(js_nanbox_pointer(re as i64));
    let method = scope.root_nanbox_f64(dispatch::get_symbol(&re, "matchAll")?);
    if !crate::proxy::proxy_wraps_callable(method.get_nanbox_f64()) {
        return Err(EngineError::Type("RegExp matchAll method is not callable"));
    }
    let argument = scope.root_nanbox_f64(js_nanbox_string(
        input.get_raw_const_ptr::<StringHeader>() as i64,
    ));
    dispatch::call_one(&method, &re, &argument)
}

#[no_mangle]
pub extern "C" fn js_string_match_all_js(receiver: f64, pattern: f64) -> f64 {
    api::finish(string(receiver, pattern))
}
#[no_mangle]
pub extern "C" fn js_string_match_all_value(s: *const StringHeader, pattern: f64) -> f64 {
    js_string_match_all_js(js_nanbox_string(s as i64), pattern)
}
#[cfg(test)]
pub fn js_string_match_all(
    s: *const StringHeader,
    re: *const super::RegExpHeader,
) -> *mut ObjectHeader {
    js_nanbox_get_pointer(js_string_match_all_value(s, js_nanbox_pointer(re as i64)))
        as *mut ObjectHeader
}

fn next(iter: *mut ObjectHeader) -> Result<f64, EngineError> {
    let scope = RuntimeHandleScope::new();
    let iter = scope.root_raw_mut_ptr(iter);
    if get(&iter, DONE).to_bits() == crate::value::TAG_TRUE {
        return result(f64::from_bits(TAG_UNDEFINED), true);
    }
    let matcher = scope.root_nanbox_f64(get(&iter, MATCHER));
    let input = scope.root_string_ptr(
        crate::value::js_get_string_pointer_unified(get(&iter, INPUT)) as *const StringHeader,
    );
    let global = get(&iter, GLOBAL).to_bits() == crate::value::TAG_TRUE;
    let unicode = get(&iter, UNICODE).to_bits() == crate::value::TAG_TRUE;
    // Each next call is an independent operation. No scratch or native owner
    // is retained between calls or exposed to reentrant next invocations.
    let memory = MemoryBudget::new(api::SCRATCH_BYTES);
    let mut budget = Budget::new(api::WORK);
    let found = dispatch::execute(
        &matcher,
        &input,
        true,
        &mut budget,
        &memory,
        &mut host::poll,
    )?;
    let Some(found) = found else {
        complete(&iter);
        return result(f64::from_bits(TAG_UNDEFINED), true);
    };
    let found = scope.root_nanbox_f64(found.object());
    if !global {
        complete(&iter);
    } else {
        let text = scope.root_nanbox_f64(dispatch::get(&found, b"0")?);
        let text = scope.root_string_ptr(dispatch::to_string(&text)?);
        if text.with_const_ptr::<StringHeader, _>(|s| unsafe { (*s).utf16_len == 0 }) {
            let index = scope.root_nanbox_f64(dispatch::get(&matcher, b"lastIndex")?);
            let index = dispatch::to_length(&index)?;
            let subject = subject(input)?;
            let length =
                input.with_const_ptr::<StringHeader, _>(|s| unsafe { (*s).utf16_len as usize });
            let index = advance(&subject, index, length, unicode, &mut budget)?;
            dispatch::set_last_index(&matcher, index)?;
        }
    }
    result(found.get_nanbox_f64(), false)
}

pub unsafe fn dispatch_regexp_string_iterator_method(iter: *mut ObjectHeader, method: &str) -> f64 {
    let scope = RuntimeHandleScope::new();
    let iter = scope.root_raw_mut_ptr(iter);
    if method == "next" {
        if let Some(value) = crate::object::call_overridden_iterator_next(
            iter.get_raw_mut_ptr(),
            REGEXP_STRING_ITERATOR_CLASS_ID,
        ) {
            return value;
        }
    }
    dispatch_regexp_string_iterator_method_builtin(iter.get_raw_mut_ptr(), method)
}

pub(crate) unsafe fn dispatch_regexp_string_iterator_method_builtin(
    iter: *mut ObjectHeader,
    method: &str,
) -> f64 {
    match method {
        "next" => api::finish(next(iter)),
        "Symbol.iterator" | "@@iterator" => js_nanbox_pointer(iter as i64),
        _ => f64::from_bits(TAG_UNDEFINED),
    }
}
