//! Historical raw array-returning split ABI. JavaScript callers use the boxed
//! ABI so Symbol.split can return any value; these native callers require an array.
use super::{perex_api as api, perex_runtime::EngineError, perex_split, RegExpHeader};
use crate::array::ArrayHeader;
use crate::string::StringHeader;
use crate::value::{
    js_nanbox_get_pointer, js_nanbox_pointer, js_nanbox_string, JSValue, TAG_UNDEFINED,
};

fn array(result: Result<f64, EngineError>) -> *mut ArrayHeader {
    let result = result.and_then(|value| {
        let ptr = js_nanbox_get_pointer(value) as *const u8;
        if JSValue::from_bits(value.to_bits()).is_pointer()
            && !crate::value::addr_class::is_handle_band(ptr as usize)
            && !ptr.is_null()
            && unsafe {
                (*ptr
                    .sub(crate::gc::GC_HEADER_SIZE)
                    .cast::<crate::gc::GcHeader>())
                .obj_type
                    == crate::gc::GC_TYPE_ARRAY
            }
        {
            Ok(value)
        } else {
            Err(EngineError::Type(
                "Native split result requires an ordinary array",
            ))
        }
    });
    js_nanbox_get_pointer(api::finish(result)) as *mut ArrayHeader
}

fn boxed_limit(limit: i32) -> f64 {
    if limit < 0 {
        f64::from_bits(TAG_UNDEFINED)
    } else {
        limit as f64
    }
}

#[no_mangle]
pub extern "C" fn js_string_split_regex(
    s: *const StringHeader,
    re: *const RegExpHeader,
) -> *mut ArrayHeader {
    js_string_split_regex_n(s, re, -1)
}
#[no_mangle]
pub extern "C" fn js_string_split_regex_n(
    s: *const StringHeader,
    re: *const RegExpHeader,
    limit: i32,
) -> *mut ArrayHeader {
    array(perex_split::regexp(
        js_nanbox_pointer(re as i64),
        js_nanbox_string(s as i64),
        boxed_limit(limit),
    ))
}
#[no_mangle]
pub extern "C" fn js_string_split_n(
    s: *const StringHeader,
    delimiter: *const StringHeader,
    limit: i32,
) -> *mut ArrayHeader {
    let separator = if super::is_regex_pointer(delimiter.cast()) {
        js_nanbox_pointer(delimiter as i64)
    } else {
        js_nanbox_string(delimiter as i64)
    };
    array(perex_split::string(
        js_nanbox_string(s as i64),
        separator,
        boxed_limit(limit),
    ))
}
#[no_mangle]
pub extern "C" fn js_string_split_value(
    s: *const StringHeader,
    separator: f64,
    limit: f64,
) -> *mut ArrayHeader {
    array(perex_split::string(
        js_nanbox_string(s as i64),
        separator,
        limit,
    ))
}
