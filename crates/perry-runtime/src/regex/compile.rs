//! Annex B compilation uses the same original-input Perex initialization.
use super::RegExpHeader;
#[no_mangle]
pub extern "C" fn js_regexp_compile_value(re: *mut RegExpHeader, pattern: f64, flags: f64) -> f64 {
    if !crate::regex::regexp_data_of(crate::value::js_nanbox_pointer((re) as i64)).is_some() {
        crate::collection_iter::throw_type_error(
            "RegExp.prototype.compile called on incompatible receiver",
        );
    }
    super::perex_construct::recompile(re, pattern, flags)
}
