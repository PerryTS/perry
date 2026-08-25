//! Native compatibility binding for [`qs`](https://www.npmjs.com/package/qs).
//!
//! The binding exists primarily so packages such as Stripe can retain qs'
//! nested request encoding without asking Perry's AOT compiler to compile the
//! legacy `get-intrinsic` / ES-shims dependency chain. The implementation is
//! intentionally dependency-light and crosses the runtime only through the
//! stable `perry-ffi` surface plus existing C ABI symbols.

mod codec;
mod options;
mod parse;
mod runtime;
mod stringify;

#[cfg(test)]
mod test_async_shims;

use perry_ffi::{alloc_string, read_string, JsString, StringHeader, TransientRootScope};

/// `qs.stringify(value, options?)`.
#[no_mangle]
pub extern "C" fn js_qs_stringify(value: f64, options: f64) -> *mut StringHeader {
    alloc_string(&stringify::stringify(value, options)).as_raw()
}

/// `qs.parse(input, options?)`.
///
/// # Safety
/// `input` must be null or a live Perry `StringHeader` pointer.
#[no_mangle]
pub unsafe extern "C" fn js_qs_parse(
    input: *const StringHeader,
    options: f64,
) -> *mut StringHeader {
    let input = if input.is_null() {
        String::new()
    } else {
        let input = JsString::from_raw(input as *mut StringHeader);
        read_string(input).unwrap_or_default().to_owned()
    };
    let scope = TransientRootScope::enter();
    let mut options = options::ParseOptions::from_js(&scope, options);
    let value = parse::parse(&input, &mut options);
    let json = serde_json::to_string(&value).expect("qs parse tree is JSON serializable");
    alloc_string(&json).as_raw()
}
