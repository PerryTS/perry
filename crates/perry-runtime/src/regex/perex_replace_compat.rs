//! Historical string-returning ABI, retained for native callers and tests.
//! Compiler and JS dispatch use the boxed ABI to preserve arbitrary hook values.
use super::{perex_api as api, perex_dispatch as dispatch, perex_replace, RegExpHeader};
use crate::gc::RuntimeHandleScope;
use crate::string::StringHeader;
use crate::value::{js_nanbox_pointer, js_nanbox_string, TAG_UNDEFINED};

fn string_value(s: *const StringHeader) -> f64 {
    if s.is_null() {
        f64::from_bits(TAG_UNDEFINED)
    } else {
        js_nanbox_string(s as i64)
    }
}
fn run(all: bool, s: *const StringHeader, search: f64, replacement: f64) -> *mut StringHeader {
    let result = api::finish(perex_replace::string(
        all,
        string_value(s),
        search,
        replacement,
    ));
    let scope = RuntimeHandleScope::new();
    let result = scope.root_nanbox_f64(result);
    api::finish(dispatch::to_string(&result))
}
macro_rules! string_text {
    ($name:ident, $all:expr) => {
        #[no_mangle]
        pub extern "C" fn $name(
            s: *const StringHeader,
            needle: *const StringHeader,
            replacement: *const StringHeader,
        ) -> *mut StringHeader {
            run($all, s, string_value(needle), string_value(replacement))
        }
    };
}
macro_rules! string_value {
    ($name:ident, $all:expr) => {
        #[no_mangle]
        pub extern "C" fn $name(
            s: *const StringHeader,
            needle: *const StringHeader,
            replacement: f64,
        ) -> *mut StringHeader {
            run($all, s, string_value(needle), replacement)
        }
    };
}
macro_rules! regex_text {
    ($name:ident, $all:expr) => {
        #[no_mangle]
        pub extern "C" fn $name(
            s: *const StringHeader,
            re: *const RegExpHeader,
            replacement: *const StringHeader,
        ) -> *mut StringHeader {
            run(
                $all,
                s,
                js_nanbox_pointer(re as i64),
                string_value(replacement),
            )
        }
    };
}
macro_rules! regex_value {
    ($name:ident, $all:expr) => {
        #[no_mangle]
        pub extern "C" fn $name(
            s: *const StringHeader,
            re: *const RegExpHeader,
            replacement: f64,
        ) -> *mut StringHeader {
            run($all, s, js_nanbox_pointer(re as i64), replacement)
        }
    };
}
string_text!(js_string_replace_string, false);
string_text!(js_string_replace_all_string, true);
string_value!(js_string_replace_string_fn, false);
string_value!(js_string_replace_all_string_fn, true);
string_value!(js_string_replace_string_dyn, false);
string_value!(js_string_replace_all_string_dyn, true);
regex_text!(js_string_replace_regex, false);
regex_text!(js_string_replace_all_regex, true);
regex_text!(js_string_replace_regex_named, false);
regex_text!(js_string_replace_all_regex_named, true);
regex_value!(js_string_replace_regex_fn, false);
regex_value!(js_string_replace_all_regex_fn, true);
regex_value!(js_string_replace_regex_dyn, false);
regex_value!(js_string_replace_all_regex_dyn, true);
#[no_mangle]
pub extern "C" fn js_string_replace_search_dyn(
    s: *const StringHeader,
    needle: f64,
    replacement: f64,
) -> *mut StringHeader {
    run(false, s, needle, replacement)
}
#[no_mangle]
pub extern "C" fn js_string_replace_all_search_dyn(
    s: *const StringHeader,
    needle: f64,
    replacement: f64,
) -> *mut StringHeader {
    run(true, s, needle, replacement)
}

#[cfg(all(test, feature = "regex-engine"))]
mod tests {
    use super::*;
    use crate::regex::{js_string_from_str, string_as_str};

    /// #4871: a RegExp arriving as an opaque NaN-boxed value (object-property
    /// read) must dispatch to the regex path, not be ToString-coerced into a
    /// literal "/foo/g" search.
    #[test]
    fn search_dyn_dispatches_runtime_regex_and_coerces_non_regex() {
        let s = js_string_from_str("foofoo");
        let pat = js_string_from_str("foo");
        let flags = js_string_from_str("g");
        let re = crate::regex::js_regexp_new(pat, flags);
        let re_boxed = f64::from_bits(0x7FFD_0000_0000_0000u64 | (re as u64 & 0xFFFF_FFFF_FFFF));
        let repl = js_nanbox_string(js_string_from_str("X") as i64);

        // /foo/g: the g flag makes .replace substitute every match.
        let out = js_string_replace_search_dyn(s, re_boxed, repl);
        assert_eq!(string_as_str(out), "XX");

        let out_all = js_string_replace_all_search_dyn(s, re_boxed, repl);
        assert_eq!(string_as_str(out_all), "XX");

        // Non-regex needle: ToString-coerce and search literally.
        let needle_num = 12.0_f64;
        let s2 = js_string_from_str("a12b");
        let out2 = js_string_replace_search_dyn(s2, needle_num, repl);
        assert_eq!(string_as_str(out2), "aXb");
    }
}
