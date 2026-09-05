//! `replaceAll` / `matchAll` non-global receiver guards.
//!
//! Split out of `regex.rs` to keep that file under the 2000-line size gate.

use super::{js_string_from_str, RegExpHeader};

pub(super) fn throw_replace_all_non_global_regex() -> ! {
    let message = b"String.prototype.replaceAll called with a non-global RegExp argument";
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

#[cfg(feature = "regex-engine")]
pub(super) fn throw_match_all_non_global_regex() -> ! {
    let message = b"String.prototype.matchAll called with a non-global RegExp argument";
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

#[cfg(feature = "regex-engine")]
#[inline]
pub(super) fn ensure_replace_all_regex_global(re: *const RegExpHeader) {
    unsafe {
        if !(*re).global {
            throw_replace_all_non_global_regex();
        }
    }
}
