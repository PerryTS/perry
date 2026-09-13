//! Empty-string replacement on a proven builtin RegExp needs only match spans.
//! No match returns the original string; matches assemble gaps without exec
//! arrays, capture strings, flags getters or a call through @@replace.
use super::perex_api as api;
use super::perex_match_search::{advance, subject};
use super::perex_memory::MemoryBudget;
use super::perex_replace_storage::{boxed, length, Pieces};
use super::perex_runtime::{self as host, CaptureMode, EngineError};
use super::RegExpHeader;
use crate::gc::RuntimeHandleScope;
use crate::string::StringHeader;
use crate::value::{js_nanbox_string, JSValue};
use perex::Budget;

/// None declines without observable work. All string decoding below happens
/// after admission, with every argument rooted before any materialization.
pub(super) fn try_remove(
    receiver: f64,
    search: f64,
    replacement: f64,
) -> Result<Option<f64>, EngineError> {
    let replacement_value = JSValue::from_bits(replacement.to_bits());
    // This cheap first guard leaves non-empty replacement workloads alone.
    let empty = if replacement_value.is_string() {
        unsafe { (*replacement_value.as_string_ptr()).byte_len == 0 }
    } else if replacement_value.is_short_string() {
        replacement_value.short_string_len() == 0
    } else {
        false
    };
    if !empty
        || !JSValue::from_bits(receiver.to_bits()).is_any_string()
        || !crate::object::regex_canonical::replace(search)
    {
        return Ok(None);
    }
    let re = crate::value::js_nanbox_get_pointer(search) as *mut RegExpHeader;
    let (global, sticky, unicode, stored) = unsafe {
        (
            (*re).global,
            (*re).sticky,
            (*re).unicode,
            JSValue::from_bits((*re).last_index),
        )
    };
    if !stored.is_number() {
        return Ok(None);
    }
    if crate::hot_diag::regex_on() {
        crate::hot_diag::regex_with(|d| d.perex_removes += 1);
    }
    let scope = RuntimeHandleScope::new();
    let re = scope.root_raw_mut_ptr(re);
    let receiver = scope.root_nanbox_f64(receiver);
    let input =
        api::caught(|| crate::value::js_get_string_pointer_unified(receiver.get_nanbox_f64()))?;
    let input = scope.root_string_ptr(input as *const StringHeader);
    let input_length = length(&input);
    let mut start = if global || !sticky {
        0
    } else {
        stored
            .as_number()
            .max(0.0)
            .floor()
            .min(9_007_199_254_740_991.0) as usize
    };
    if global {
        re.with_mut_ptr(|re| super::store_last_index_number(re, 0));
    }
    let mut budget = Budget::new(api::WORK);
    let memory = MemoryBudget::new(api::SCRATCH_BYTES);
    let program = api::program(&scope, &re, &mut budget, &memory, &mut host::poll)?;
    let subject = subject(input)?;
    let mut output: Option<Pieces<'_>> = None;
    let mut copied = 0;
    let mut near = None;
    loop {
        let found = if start > input_length {
            None
        } else {
            let (found, position) = host::find_near(
                &program,
                &subject,
                start,
                near,
                CaptureMode::Full,
                &mut budget,
                &memory,
                api::QUANTUM,
                &mut host::poll,
            )?;
            near = Some(position);
            found
        };
        if global || sticky {
            re.with_mut_ptr(|re| {
                super::store_last_index_number(re, found.as_ref().map_or(0, |m| m.full.end()))
            });
        }
        let Some(found) = found else {
            break;
        };
        let output = match &mut output {
            Some(output) => output,
            slot @ None => slot.insert(Pieces::new(&scope)?),
        };
        output.append(&input, copied, found.full.start(), &mut budget)?;
        copied = found.full.end();
        if !global {
            break;
        }
        start = if found.full.is_empty() {
            advance(&subject, copied as f64, input_length, unicode, &mut budget)? as usize
        } else {
            copied
        };
        if found.full.is_empty() {
            re.with_mut_ptr(|re| super::store_last_index_number(re, start));
        }
    }
    let Some(mut output) = output else {
        return Ok(Some(boxed(&input)));
    };
    output.append(&input, copied, input_length, &mut budget)?;
    output
        .finish(&input, None, &mut budget)
        .map(|s| Some(js_nanbox_string(s as i64)))
}
