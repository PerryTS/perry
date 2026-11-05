//! Always-live callers reach the RegExp engine only through its installer.
//! Empty slots retain ordinary string operations; no RegExp can exist until
//! the compiler-selected installer has filled these slots.
use super::RegExpHeader;
use crate::array::ArrayHeader;
use crate::feature_hooks::Hook;
use crate::object::ObjectHeader;
use crate::string::StringHeader;

static PROTO: Hook<fn(*mut ObjectHeader)> = Hook::empty();
static CONSTRUCT: Hook<extern "C" fn(f64, f64) -> *mut RegExpHeader> = Hook::empty();
static CONSTRUCT_CALL: Hook<extern "C" fn(f64, f64) -> *mut RegExpHeader> = Hook::empty();
static NEW: Hook<extern "C" fn(*const StringHeader, *const StringHeader) -> *mut RegExpHeader> =
    Hook::empty();
static TEST: Hook<extern "C" fn(*const RegExpHeader, *const StringHeader) -> i32> = Hook::empty();
static SOURCE: Hook<fn(*const RegExpHeader) -> *mut StringHeader> = Hook::empty();
static FLAGS: Hook<fn(*const RegExpHeader) -> *mut StringHeader> = Hook::empty();
static TO_STRING: Hook<fn(*const RegExpHeader) -> *mut StringHeader> = Hook::empty();
static MATCH: Hook<extern "C" fn(f64, f64) -> f64> = Hook::empty();
static MATCH_ALL: Hook<extern "C" fn(f64, f64) -> f64> = Hook::empty();
static SEARCH: Hook<extern "C" fn(f64, f64) -> f64> = Hook::empty();
static REPLACE: Hook<extern "C" fn(f64, f64, f64) -> f64> = Hook::empty();
static REPLACE_ALL: Hook<extern "C" fn(f64, f64, f64) -> f64> = Hook::empty();
static SPLIT: Hook<extern "C" fn(f64, f64, f64) -> f64> = Hook::empty();
static SPLIT_N: Hook<
    extern "C" fn(*const StringHeader, *const StringHeader, i32) -> *mut ArrayHeader,
> = Hook::empty();
static SPLIT_REGEX_N: Hook<
    extern "C" fn(*const StringHeader, *const RegExpHeader, i32) -> *mut ArrayHeader,
> = Hook::empty();

pub(crate) fn install_proto(proto: *mut ObjectHeader) {
    if let Some(install) = PROTO.get() {
        install(proto);
    }
}
pub(crate) fn construct(pattern: f64, flags: f64) -> *mut RegExpHeader {
    CONSTRUCT
        .get()
        .map_or(std::ptr::null_mut(), |f| f(pattern, flags))
}
pub(crate) fn construct_call(pattern: f64, flags: f64) -> *mut RegExpHeader {
    CONSTRUCT_CALL
        .get()
        .map_or(std::ptr::null_mut(), |f| f(pattern, flags))
}
pub(crate) fn new(pattern: *const StringHeader, flags: *const StringHeader) -> *mut RegExpHeader {
    NEW.get()
        .map_or(std::ptr::null_mut(), |f| f(pattern, flags))
}
pub(crate) fn test(re: *const RegExpHeader, s: *const StringHeader) -> i32 {
    TEST.get().map_or(0, |f| f(re, s))
}
pub(crate) fn source(re: *const RegExpHeader) -> Option<*mut StringHeader> {
    SOURCE.get().map(|f| f(re))
}
pub(crate) fn flags(re: *const RegExpHeader) -> Option<*mut StringHeader> {
    FLAGS.get().map(|f| f(re))
}
pub(crate) fn to_string(re: *const RegExpHeader) -> Option<*mut StringHeader> {
    TO_STRING.get().map(|f| f(re))
}

pub extern "C" fn js_string_match_js(receiver: f64, pattern: f64) -> f64 {
    MATCH.get().map_or_else(
        || f64::from_bits(crate::value::TAG_NULL),
        |f| f(receiver, pattern),
    )
}
pub extern "C" fn js_string_match_all_js(receiver: f64, pattern: f64) -> f64 {
    MATCH_ALL.get().map_or_else(
        || f64::from_bits(crate::value::TAG_NULL),
        |f| f(receiver, pattern),
    )
}
pub extern "C" fn js_string_search_js(receiver: f64, pattern: f64) -> f64 {
    SEARCH.get().map_or(-1.0, |f| f(receiver, pattern))
}
#[cfg_attr(feature = "regex-engine", no_mangle)]
pub extern "C" fn js_string_replace_js(receiver: f64, search: f64, replacement: f64) -> f64 {
    if let Some(f) = REPLACE.get() {
        return f(receiver, search, replacement);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let search = scope.root_nanbox_f64(search);
    let replacement = scope.root_nanbox_f64(replacement);
    if let Some(value) = symbol_call("replace", &receiver, &search, &replacement) {
        return value;
    }
    crate::string::literal_replace::replace(
        false,
        receiver.get_nanbox_f64(),
        search.get_nanbox_f64(),
        replacement.get_nanbox_f64(),
    )
}
#[cfg_attr(feature = "regex-engine", no_mangle)]
pub extern "C" fn js_string_replace_all_js(receiver: f64, search: f64, replacement: f64) -> f64 {
    if let Some(f) = REPLACE_ALL.get() {
        return f(receiver, search, replacement);
    }
    // IsRegExp observes @@match even on an ordinary user-defined object.
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let search = scope.root_nanbox_f64(search);
    let replacement = scope.root_nanbox_f64(replacement);
    require_coercible(receiver.get_nanbox_f64(), "replace");
    if crate::proxy::reflect_value_is_object(search.get_nanbox_f64()) {
        let key = crate::symbol::well_known_symbol("match");
        let value = search.get_nanbox_f64();
        let marker =
            crate::proxy::js_reflect_get(value, crate::value::js_nanbox_pointer(key as i64), value);
        if crate::value::js_is_truthy(marker) != 0 {
            let key = crate::string::canonical_key(b"flags");
            let value = search.get_nanbox_f64();
            let flags = crate::proxy::js_reflect_get(
                value,
                crate::value::js_nanbox_string(key as i64),
                value,
            );
            require_coercible(flags, "replace");
            let flags = crate::value::js_jsvalue_to_string_coerce(flags);
            if !crate::regex::string_as_str(flags)
                .as_bytes()
                .contains(&b'g')
            {
                crate::collection_iter::throw_type_error(
                    "String.prototype.replaceAll called with a non-global RegExp argument",
                );
            }
        }
    }
    if let Some(value) = symbol_call("replace", &receiver, &search, &replacement) {
        return value;
    }
    crate::string::literal_replace::replace(
        true,
        receiver.get_nanbox_f64(),
        search.get_nanbox_f64(),
        replacement.get_nanbox_f64(),
    )
}
#[cfg_attr(feature = "regex-engine", no_mangle)]
pub extern "C" fn js_string_split_js(receiver: f64, separator: f64, limit: f64) -> f64 {
    if let Some(f) = SPLIT.get() {
        return f(receiver, separator, limit);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let separator = scope.root_nanbox_f64(separator);
    let limit = scope.root_nanbox_f64(limit);
    if let Some(value) = symbol_call("split", &receiver, &separator, &limit) {
        return value;
    }
    crate::string::literal_replace::split(
        receiver.get_nanbox_f64(),
        separator.get_nanbox_f64(),
        limit.get_nanbox_f64(),
    )
}
#[cfg_attr(feature = "regex-engine", no_mangle)]
pub extern "C" fn js_string_split_n(
    s: *const StringHeader,
    delimiter: *const StringHeader,
    limit: i32,
) -> *mut ArrayHeader {
    if let Some(f) = SPLIT_N.get() {
        return f(s, delimiter, limit);
    }
    let limit = if limit < 0 {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    } else {
        limit as f64
    };
    crate::value::js_nanbox_get_pointer(js_string_split_js(
        crate::value::js_nanbox_string(s as i64),
        crate::value::js_nanbox_string(delimiter as i64),
        limit,
    )) as *mut ArrayHeader
}
pub(crate) fn js_string_split_regex_n(
    s: *const StringHeader,
    re: *const RegExpHeader,
    limit: i32,
) -> *mut ArrayHeader {
    // No RegExp exists with this slot empty.
    SPLIT_REGEX_N
        .get()
        .map_or_else(|| crate::array::js_array_alloc(0), |f| f(s, re, limit))
}
fn require_coercible(value: f64, operation: &str) {
    if matches!(
        value.to_bits(),
        crate::value::TAG_NULL | crate::value::TAG_UNDEFINED
    ) {
        crate::collection_iter::throw_type_error(if operation == "split" {
            "String.split requires a non-null receiver"
        } else {
            "String replacement requires a non-null receiver"
        });
    }
}
/// GetMethod must run before coercing the string receiver. Root every input
/// before a user getter, and re-read the receiver and arguments after it.
fn symbol_call(
    symbol: &str,
    receiver: &crate::gc::RuntimeHandle<'_>,
    search: &crate::gc::RuntimeHandle<'_>,
    argument: &crate::gc::RuntimeHandle<'_>,
) -> Option<f64> {
    require_coercible(receiver.get_nanbox_f64(), symbol);
    let scope = crate::gc::RuntimeHandleScope::new();
    if !crate::proxy::reflect_value_is_object(search.get_nanbox_f64()) {
        return None;
    }
    if matches!(
        search.get_nanbox_u64(),
        crate::value::TAG_NULL | crate::value::TAG_UNDEFINED
    ) {
        return None;
    }
    let key = crate::symbol::well_known_symbol(symbol);
    let value = search.get_nanbox_f64();
    let method =
        crate::proxy::js_reflect_get(value, crate::value::js_nanbox_pointer(key as i64), value);
    if matches!(
        method.to_bits(),
        crate::value::TAG_NULL | crate::value::TAG_UNDEFINED
    ) {
        return None;
    }
    let method = scope.root_nanbox_f64(method);
    if !crate::string::literal_replace::callable(&method) {
        crate::collection_iter::throw_type_error(if symbol == "split" {
            "Symbol.split is not callable"
        } else {
            "Symbol.replace is not callable"
        });
    }
    let args = [receiver.get_nanbox_f64(), argument.get_nanbox_f64()];
    Some(crate::string::literal_replace::call(
        &method,
        search.get_nanbox_f64(),
        &args,
    ))
}

#[cfg(feature = "regex-engine")]
pub(super) fn install() {
    PROTO.set(|proto| {
        crate::object::regex_proto_thunks::install_regex_proto_accessors(proto);
        crate::object::regex_proto_thunks::install_regex_proto_methods(proto);
    });
    CONSTRUCT.set(super::js_regexp_construct);
    CONSTRUCT_CALL.set(super::js_regexp_construct_call);
    NEW.set(super::js_regexp_new);
    TEST.set(super::js_regexp_test);
    SOURCE.set(|re| super::perex_api::finish(super::perex_display::source(re)));
    FLAGS.set(|re| {
        super::perex_api::finish(super::perex_match_search::flags(
            crate::value::js_nanbox_pointer(re as i64),
        ))
    });
    TO_STRING.set(|re| super::perex_api::finish(super::perex_display::to_string(re)));
    MATCH.set(super::js_string_match_js);
    MATCH_ALL.set(super::js_string_match_all_js);
    SEARCH.set(super::js_string_search_js);
    REPLACE.set(super::perex_replace::js_string_replace_js);
    REPLACE_ALL.set(super::perex_replace::js_string_replace_all_js);
    SPLIT.set(super::perex_split::js_string_split_js);
    SPLIT_N.set(super::perex_split_compat::js_string_split_n);
    SPLIT_REGEX_N.set(super::js_string_split_regex_n);
}
