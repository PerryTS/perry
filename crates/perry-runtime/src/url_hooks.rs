//! URL value operations reached through the compiler-selected engine install.
use crate::feature_hooks::Hook;
pub(crate) use crate::object::url_layout::is_url_object_shape;
use crate::{ArrayHeader, ObjectHeader, StringHeader};
static CANONICALIZE_HOST: crate::feature_hooks::Hook<fn(&str) -> Option<String>> =
    crate::feature_hooks::Hook::empty();
static DOMAIN_TO_ASCII: crate::feature_hooks::Hook<fn(&str) -> Option<String>> =
    crate::feature_hooks::Hook::empty();
static DOMAIN_TO_UNICODE: crate::feature_hooks::Hook<fn(&str) -> String> =
    crate::feature_hooks::Hook::empty();

/// WHATWG host canonicalization via the `url` crate (#3056, #3059).
///
/// Perry's URL uses a custom hostname parser that runs only IDNA
/// (`idna::domain_to_ascii`) — it never applies the WHATWG IPv4 / numeric
/// host parser, so `123` stayed `"123"` instead of canonicalizing to the
/// IPv4 address `"0.0.0.123"`, and `0x7f.1` stayed `"0x7f.1"` instead of
/// `"127.0.0.1"`. We borrow the `url` crate's full WHATWG host parser by
/// reparsing `http://<host>/` and reading back `host_str()`:
///
/// * numeric / IPv4-shorthand hosts → canonical dotted-quad IPv4
///   (`"123"` → `"0.0.0.123"`, `"0x7f.1"` → `"127.0.0.1"`),
/// * ordinary registrable hostnames → returned unchanged
///   (`"example.com"` → `"example.com"`),
/// * already-punycode IDN labels → returned unchanged
///   (`"xn--mnchen-3ya.de"` → `"xn--mnchen-3ya.de"`),
/// * hosts the WHATWG parser rejects (out-of-range numeric like
///   `"999999999999"`, `"256.256.256.256"`) → `None`.
///
/// `host` is expected to already be a candidate host string (post-IDNA for
/// the domain helpers, or the raw setter value). Callers decide what `None`
/// means for them (the hostname setter leaves the host unchanged; the
/// `domainTo*` helpers return `""`), matching Node.
pub(crate) fn whatwg_canonicalize_host(host: &str) -> Option<String> {
    match CANONICALIZE_HOST.get() {
        Some(canonicalize) => canonicalize(host),
        // URL engine not installed: no WHATWG host parser, so pass the host
        // through unchanged (the hand-rolled URL paths handle the common cases).
        None => Some(host.to_string()),
    }
}

/// IDNA `domain_to_ascii`: `None` when the engine is not installed,
/// `Some(None)` when IDNA rejects the domain.
pub(crate) fn idna_domain_to_ascii(domain: &str) -> Option<Option<String>> {
    DOMAIN_TO_ASCII.get().map(|f| f(domain))
}

/// IDNA `domain_to_unicode`; `None` when the engine is not installed.
pub(crate) fn idna_domain_to_unicode(domain: &str) -> Option<String> {
    DOMAIN_TO_UNICODE.get().map(|f| f(domain))
}
pub(crate) fn install_idna(
    host: fn(&str) -> Option<String>,
    ascii: fn(&str) -> Option<String>,
    unicode: fn(&str) -> String,
) {
    CANONICALIZE_HOST.set(host);
    DOMAIN_TO_ASCII.set(ascii);
    DOMAIN_TO_UNICODE.set(unicode);
}

static FILE_URL_TO_PATH_BYTES: Hook<fn(f64, bool) -> Vec<u8>> = Hook::empty();
#[inline]
pub fn file_url_to_path_bytes(url_f64: f64, windows: bool) -> Vec<u8> {
    FILE_URL_TO_PATH_BYTES
        .get()
        .map_or_else(|| Vec::new(), |f| f(url_f64, windows))
}

static FILE_URL_TO_PATH_STRING_POSIX: Hook<fn(f64) -> String> = Hook::empty();
#[inline]
pub fn file_url_to_path_string_posix(url_f64: f64) -> String {
    FILE_URL_TO_PATH_STRING_POSIX
        .get()
        .map_or_else(|| String::new(), |f| f(url_f64))
}

static INSTALL_URL_PROTOTYPE_ACCESSORS: Hook<fn(*mut ObjectHeader)> = Hook::empty();
#[inline]
pub fn install_url_prototype_accessors(proto: *mut ObjectHeader) {
    INSTALL_URL_PROTOTYPE_ACCESSORS
        .get()
        .map_or_else(|| (), |f| f(proto))
}

static JS_URL_CAN_PARSE: Hook<extern "C" fn(*mut crate::StringHeader) -> i32> = Hook::empty();
#[inline]
pub extern "C" fn url_can_parse(input: *mut crate::StringHeader) -> i32 {
    JS_URL_CAN_PARSE.get().map_or_else(|| 0, |f| f(input))
}

static JS_URL_CAN_PARSE_WITH_BASE: Hook<
    extern "C" fn(*mut crate::StringHeader, *mut crate::StringHeader) -> i32,
> = Hook::empty();
#[inline]
pub extern "C" fn url_can_parse_with_base(
    input: *mut crate::StringHeader,
    base: *mut crate::StringHeader,
) -> i32 {
    JS_URL_CAN_PARSE_WITH_BASE
        .get()
        .map_or_else(|| 0, |f| f(input, base))
}

static JS_URL_COERCE_STRING: Hook<extern "C" fn(f64) -> *mut StringHeader> = Hook::empty();
#[inline]
pub extern "C" fn url_coerce_string(value: f64) -> *mut StringHeader {
    JS_URL_COERCE_STRING
        .get()
        .map_or_else(|| std::ptr::null_mut(), |f| f(value))
}

static JS_URL_DOMAIN_TO_ASCII: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn url_domain_to_ascii(input_f64: f64) -> f64 {
    JS_URL_DOMAIN_TO_ASCII.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(input_f64),
    )
}

static JS_URL_DOMAIN_TO_UNICODE: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn url_domain_to_unicode(input_f64: f64) -> f64 {
    JS_URL_DOMAIN_TO_UNICODE.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(input_f64),
    )
}

static JS_URL_FILE_URL_TO_PATH: Hook<extern "C" fn(f64, f64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn url_file_url_to_path(url_f64: f64, options_f64: f64) -> f64 {
    JS_URL_FILE_URL_TO_PATH.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(url_f64, options_f64),
    )
}

static JS_URL_FILE_URL_TO_PATH_BUFFER: Hook<extern "C" fn(f64, f64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn url_file_url_to_path_buffer(url_f64: f64, options_f64: f64) -> f64 {
    JS_URL_FILE_URL_TO_PATH_BUFFER.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(url_f64, options_f64),
    )
}

static JS_URL_FORMAT: Hook<extern "C" fn(f64, f64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn url_format(value: f64, options: f64) -> f64 {
    JS_URL_FORMAT.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(value, options),
    )
}

static JS_URL_GET_HREF: Hook<extern "C" fn(*mut ObjectHeader) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn url_get_href(url: *mut ObjectHeader) -> f64 {
    JS_URL_GET_HREF
        .get()
        .map_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED), |f| f(url))
}

static JS_URL_HREF_IF_URL: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn url_href_if_url(value: f64) -> f64 {
    JS_URL_HREF_IF_URL
        .get()
        .map_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED), |f| f(value))
}

static JS_URL_LEGACY_PARSE: Hook<extern "C" fn(f64, f64, f64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn url_legacy_parse(
    input: f64,
    parse_query_string: f64,
    slashes_denote_host: f64,
) -> f64 {
    JS_URL_LEGACY_PARSE.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(input, parse_query_string, slashes_denote_host),
    )
}

static JS_URL_LEGACY_RESOLVE: Hook<extern "C" fn(f64, f64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn url_legacy_resolve(from: f64, to: f64) -> f64 {
    JS_URL_LEGACY_RESOLVE.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(from, to),
    )
}

static JS_URL_LEGACY_RESOLVE_OBJECT: Hook<extern "C" fn(f64, f64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn url_legacy_resolve_object(from: f64, to: f64) -> f64 {
    JS_URL_LEGACY_RESOLVE_OBJECT.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(from, to),
    )
}

static JS_URL_LEGACY_URL_NEW: Hook<extern "C" fn() -> f64> = Hook::empty();
#[inline]
pub extern "C" fn url_legacy_url_new() -> f64 {
    JS_URL_LEGACY_URL_NEW
        .get()
        .map_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED), |f| f())
}

static JS_URL_NEW: Hook<extern "C" fn(*mut crate::StringHeader) -> *mut ObjectHeader> =
    Hook::empty();
#[inline]
pub extern "C" fn url_new(url_str: *mut crate::StringHeader) -> *mut ObjectHeader {
    JS_URL_NEW
        .get()
        .map_or_else(|| std::ptr::null_mut(), |f| f(url_str))
}

static JS_URL_NEW_WITH_BASE: Hook<
    extern "C" fn(*mut crate::StringHeader, *mut crate::StringHeader) -> *mut ObjectHeader,
> = Hook::empty();
#[inline]
pub extern "C" fn url_new_with_base(
    url_str: *mut crate::StringHeader,
    base_str: *mut crate::StringHeader,
) -> *mut ObjectHeader {
    JS_URL_NEW_WITH_BASE
        .get()
        .map_or_else(|| std::ptr::null_mut(), |f| f(url_str, base_str))
}

static JS_URL_PARSE: Hook<extern "C" fn(*mut crate::StringHeader) -> *mut ObjectHeader> =
    Hook::empty();
#[inline]
pub extern "C" fn url_parse(input: *mut crate::StringHeader) -> *mut ObjectHeader {
    JS_URL_PARSE
        .get()
        .map_or_else(|| std::ptr::null_mut(), |f| f(input))
}

static JS_URL_PARSE_WITH_BASE: Hook<
    extern "C" fn(*mut crate::StringHeader, *mut crate::StringHeader) -> *mut ObjectHeader,
> = Hook::empty();
#[inline]
pub extern "C" fn url_parse_with_base(
    input: *mut crate::StringHeader,
    base: *mut crate::StringHeader,
) -> *mut ObjectHeader {
    JS_URL_PARSE_WITH_BASE
        .get()
        .map_or_else(|| std::ptr::null_mut(), |f| f(input, base))
}

static JS_URL_PATH_TO_FILE_URL: Hook<extern "C" fn(f64, f64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn url_path_to_file_url(path_f64: f64, options_f64: f64) -> f64 {
    JS_URL_PATH_TO_FILE_URL.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(path_f64, options_f64),
    )
}

static JS_URL_PATTERN_CONSTRUCTOR_CALL: Hook<extern "C" fn(f64, f64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn url_pattern_constructor_call(_input: f64, _base: f64) -> f64 {
    JS_URL_PATTERN_CONSTRUCTOR_CALL.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(_input, _base),
    )
}

static JS_URL_PATTERN_EXEC: Hook<extern "C" fn(*mut ObjectHeader, f64, f64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn url_pattern_exec(pattern: *mut ObjectHeader, input: f64, base: f64) -> f64 {
    JS_URL_PATTERN_EXEC.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(pattern, input, base),
    )
}

static JS_URL_PATTERN_TEST: Hook<extern "C" fn(*mut ObjectHeader, f64, f64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn url_pattern_test(pattern: *mut ObjectHeader, input: f64, base: f64) -> f64 {
    JS_URL_PATTERN_TEST.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(pattern, input, base),
    )
}

static JS_URL_SEARCH_PARAMS_ENTRIES_ARR: Hook<extern "C" fn(*mut ObjectHeader) -> f64> =
    Hook::empty();
#[inline]
pub extern "C" fn url_search_params_entries_arr(params: *mut ObjectHeader) -> f64 {
    JS_URL_SEARCH_PARAMS_ENTRIES_ARR.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(params),
    )
}

static JS_URL_SEARCH_PARAMS_KEYS_ARR: Hook<extern "C" fn(*mut ObjectHeader) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn url_search_params_keys_arr(params: *mut ObjectHeader) -> f64 {
    JS_URL_SEARCH_PARAMS_KEYS_ARR.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(params),
    )
}

static JS_URL_SEARCH_PARAMS_NEW: Hook<
    extern "C" fn(*mut crate::StringHeader) -> *mut ObjectHeader,
> = Hook::empty();
#[inline]
pub extern "C" fn url_search_params_new(init_str: *mut crate::StringHeader) -> *mut ObjectHeader {
    JS_URL_SEARCH_PARAMS_NEW
        .get()
        .map_or_else(|| std::ptr::null_mut(), |f| f(init_str))
}

static JS_URL_SEARCH_PARAMS_NEW_ANY: Hook<extern "C" fn(f64) -> *mut ObjectHeader> = Hook::empty();
#[inline]
pub extern "C" fn url_search_params_new_any(init: f64) -> *mut ObjectHeader {
    JS_URL_SEARCH_PARAMS_NEW_ANY
        .get()
        .map_or_else(|| std::ptr::null_mut(), |f| f(init))
}

static JS_URL_SEARCH_PARAMS_NEW_EMPTY: Hook<extern "C" fn() -> *mut ObjectHeader> = Hook::empty();
#[inline]
pub extern "C" fn url_search_params_new_empty() -> *mut ObjectHeader {
    JS_URL_SEARCH_PARAMS_NEW_EMPTY
        .get()
        .map_or_else(|| std::ptr::null_mut(), |f| f())
}

static JS_URL_SEARCH_PARAMS_TO_STRING: Hook<
    extern "C" fn(*mut ObjectHeader) -> *mut crate::StringHeader,
> = Hook::empty();
#[inline]
pub extern "C" fn url_search_params_to_string(
    params: *mut ObjectHeader,
) -> *mut crate::StringHeader {
    JS_URL_SEARCH_PARAMS_TO_STRING
        .get()
        .map_or_else(|| std::ptr::null_mut(), |f| f(params))
}

static JS_URL_SEARCH_PARAMS_VALUES_ARR: Hook<extern "C" fn(*mut ObjectHeader) -> f64> =
    Hook::empty();
#[inline]
pub extern "C" fn url_search_params_values_arr(params: *mut ObjectHeader) -> f64 {
    JS_URL_SEARCH_PARAMS_VALUES_ARR.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(params),
    )
}

static JS_URL_SET_HASH: Hook<extern "C" fn(*mut ObjectHeader, f64)> = Hook::empty();
#[inline]
pub extern "C" fn url_set_hash(url: *mut ObjectHeader, value: f64) {
    JS_URL_SET_HASH.get().map_or_else(|| (), |f| f(url, value))
}

static JS_URL_SET_HOST: Hook<extern "C" fn(*mut ObjectHeader, f64)> = Hook::empty();
#[inline]
pub extern "C" fn url_set_host(url: *mut ObjectHeader, value: f64) {
    JS_URL_SET_HOST.get().map_or_else(|| (), |f| f(url, value))
}

static JS_URL_SET_HOSTNAME: Hook<extern "C" fn(*mut ObjectHeader, f64)> = Hook::empty();
#[inline]
pub extern "C" fn url_set_hostname(url: *mut ObjectHeader, value: f64) {
    JS_URL_SET_HOSTNAME
        .get()
        .map_or_else(|| (), |f| f(url, value))
}

static JS_URL_SET_HREF: Hook<extern "C" fn(*mut ObjectHeader, f64)> = Hook::empty();
#[inline]
pub extern "C" fn url_set_href(url: *mut ObjectHeader, value: f64) {
    JS_URL_SET_HREF.get().map_or_else(|| (), |f| f(url, value))
}

static JS_URL_SET_PASSWORD: Hook<extern "C" fn(*mut ObjectHeader, f64)> = Hook::empty();
#[inline]
pub extern "C" fn url_set_password(url: *mut ObjectHeader, value: f64) {
    JS_URL_SET_PASSWORD
        .get()
        .map_or_else(|| (), |f| f(url, value))
}

static JS_URL_SET_PATHNAME: Hook<extern "C" fn(*mut ObjectHeader, f64)> = Hook::empty();
#[inline]
pub extern "C" fn url_set_pathname(url: *mut ObjectHeader, value: f64) {
    JS_URL_SET_PATHNAME
        .get()
        .map_or_else(|| (), |f| f(url, value))
}

static JS_URL_SET_PORT: Hook<extern "C" fn(*mut ObjectHeader, f64)> = Hook::empty();
#[inline]
pub extern "C" fn url_set_port(url: *mut ObjectHeader, value: f64) {
    JS_URL_SET_PORT.get().map_or_else(|| (), |f| f(url, value))
}

static JS_URL_SET_PROTOCOL: Hook<extern "C" fn(*mut ObjectHeader, f64)> = Hook::empty();
#[inline]
pub extern "C" fn url_set_protocol(url: *mut ObjectHeader, value: f64) {
    JS_URL_SET_PROTOCOL
        .get()
        .map_or_else(|| (), |f| f(url, value))
}

static JS_URL_SET_SEARCH: Hook<extern "C" fn(*mut ObjectHeader, f64)> = Hook::empty();
#[inline]
pub extern "C" fn url_set_search(url: *mut ObjectHeader, value: f64) {
    JS_URL_SET_SEARCH
        .get()
        .map_or_else(|| (), |f| f(url, value))
}

static JS_URL_SET_USERNAME: Hook<extern "C" fn(*mut ObjectHeader, f64)> = Hook::empty();
#[inline]
pub extern "C" fn url_set_username(url: *mut ObjectHeader, value: f64) {
    JS_URL_SET_USERNAME
        .get()
        .map_or_else(|| (), |f| f(url, value))
}

static JS_URL_TO_HTTP_OPTIONS: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn url_to_http_options(url_f64: f64) -> f64 {
    JS_URL_TO_HTTP_OPTIONS.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(url_f64),
    )
}

static SHAPE_IS_URL_SEARCH_PARAMS: Hook<fn(*const ObjectHeader) -> bool> = Hook::empty();
#[inline]
pub fn shape_is_url_search_params(obj: *const ObjectHeader) -> bool {
    SHAPE_IS_URL_SEARCH_PARAMS
        .get()
        .map_or_else(|| false, |f| f(obj))
}

static TRY_READ_AS_SEARCH_PARAMS: Hook<fn(*mut ObjectHeader) -> Option<Vec<(String, String)>>> =
    Hook::empty();
#[inline]
pub fn try_read_as_search_params(params: *mut ObjectHeader) -> Option<Vec<(String, String)>> {
    TRY_READ_AS_SEARCH_PARAMS
        .get()
        .map_or_else(|| None, |f| f(params))
}

static TRY_URL_SEARCH_PARAMS_DYNAMIC_DISPATCH: Hook<
    fn(f64, &str, *const f64, usize) -> Option<f64>,
> = Hook::empty();
#[inline]
pub fn try_url_search_params_dynamic_dispatch(
    object: f64,
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    TRY_URL_SEARCH_PARAMS_DYNAMIC_DISPATCH
        .get()
        .map_or_else(|| None, |f| f(object, method_name, args_ptr, args_len))
}

static URL_SEARCH_PARAMS_BACKING_OF: Hook<fn(f64) -> Option<*mut ObjectHeader>> = Hook::empty();
#[inline]
pub fn url_search_params_backing_of(object: f64) -> Option<*mut ObjectHeader> {
    URL_SEARCH_PARAMS_BACKING_OF
        .get()
        .map_or_else(|| None, |f| f(object))
}

pub(crate) fn install_values() {
    FILE_URL_TO_PATH_BYTES.set(crate::url::node_compat::file_url_to_path_bytes);
    FILE_URL_TO_PATH_STRING_POSIX.set(crate::url::node_compat::file_url_to_path_string_posix);
    INSTALL_URL_PROTOTYPE_ACCESSORS.set(crate::url::prototype::install_url_prototype_accessors);
    JS_URL_CAN_PARSE.set(crate::url::url_class::js_url_can_parse);
    JS_URL_CAN_PARSE_WITH_BASE.set(crate::url::url_class::js_url_can_parse_with_base);
    JS_URL_COERCE_STRING.set(crate::url::js_url_coerce_string);
    JS_URL_DOMAIN_TO_ASCII.set(crate::url::node_compat::js_url_domain_to_ascii);
    JS_URL_DOMAIN_TO_UNICODE.set(crate::url::node_compat::js_url_domain_to_unicode);
    JS_URL_FILE_URL_TO_PATH.set(crate::url::node_compat::js_url_file_url_to_path);
    JS_URL_FILE_URL_TO_PATH_BUFFER.set(crate::url::node_compat::js_url_file_url_to_path_buffer);
    JS_URL_FORMAT.set(crate::url::node_compat::js_url_format);
    JS_URL_GET_HREF.set(crate::url::url_class::js_url_get_href);
    JS_URL_HREF_IF_URL.set(crate::url::url_class::js_url_href_if_url);
    JS_URL_LEGACY_PARSE.set(crate::url::node_compat::js_url_legacy_parse);
    JS_URL_LEGACY_RESOLVE.set(crate::url::node_compat::js_url_legacy_resolve);
    JS_URL_LEGACY_RESOLVE_OBJECT.set(crate::url::node_compat::js_url_legacy_resolve_object);
    JS_URL_LEGACY_URL_NEW.set(crate::url::node_compat::js_url_legacy_url_new);
    JS_URL_NEW.set(crate::url::url_class::js_url_new);
    JS_URL_NEW_WITH_BASE.set(crate::url::url_class::js_url_new_with_base);
    JS_URL_PARSE.set(crate::url::url_class::js_url_parse);
    JS_URL_PARSE_WITH_BASE.set(crate::url::url_class::js_url_parse_with_base);
    JS_URL_PATH_TO_FILE_URL.set(crate::url::node_compat::js_url_path_to_file_url);
    JS_URL_PATTERN_CONSTRUCTOR_CALL.set(crate::url::url_pattern::js_url_pattern_constructor_call);
    JS_URL_PATTERN_EXEC.set(crate::url::url_pattern::js_url_pattern_exec);
    JS_URL_PATTERN_TEST.set(crate::url::url_pattern::js_url_pattern_test);
    JS_URL_SEARCH_PARAMS_ENTRIES_ARR
        .set(crate::url::search_params::js_url_search_params_entries_arr);
    JS_URL_SEARCH_PARAMS_KEYS_ARR.set(crate::url::search_params::js_url_search_params_keys_arr);
    JS_URL_SEARCH_PARAMS_NEW.set(crate::url::search_params::js_url_search_params_new);
    JS_URL_SEARCH_PARAMS_NEW_ANY.set(crate::url::search_params::js_url_search_params_new_any);
    JS_URL_SEARCH_PARAMS_NEW_EMPTY.set(crate::url::search_params::js_url_search_params_new_empty);
    JS_URL_SEARCH_PARAMS_TO_STRING.set(crate::url::search_params::js_url_search_params_to_string);
    JS_URL_SEARCH_PARAMS_VALUES_ARR.set(crate::url::search_params::js_url_search_params_values_arr);
    JS_URL_SET_HASH.set(crate::url::url_class::js_url_set_hash);
    JS_URL_SET_HOST.set(crate::url::url_class::js_url_set_host);
    JS_URL_SET_HOSTNAME.set(crate::url::url_class::js_url_set_hostname);
    JS_URL_SET_HREF.set(crate::url::url_class::js_url_set_href);
    JS_URL_SET_PASSWORD.set(crate::url::url_class::js_url_set_password);
    JS_URL_SET_PATHNAME.set(crate::url::url_class::js_url_set_pathname);
    JS_URL_SET_PORT.set(crate::url::url_class::js_url_set_port);
    JS_URL_SET_PROTOCOL.set(crate::url::url_class::js_url_set_protocol);
    JS_URL_SET_SEARCH.set(crate::url::url_class::js_url_set_search);
    JS_URL_SET_USERNAME.set(crate::url::url_class::js_url_set_username);
    JS_URL_TO_HTTP_OPTIONS.set(crate::url::node_compat::js_url_to_http_options);
    SHAPE_IS_URL_SEARCH_PARAMS.set(crate::url::search_params::shape_is_url_search_params);
    TRY_READ_AS_SEARCH_PARAMS.set(crate::url::search_params::try_read_as_search_params);
    TRY_URL_SEARCH_PARAMS_DYNAMIC_DISPATCH
        .set(crate::url::search_params::try_url_search_params_dynamic_dispatch);
    URL_SEARCH_PARAMS_BACKING_OF.set(crate::url::search_params::url_search_params_backing_of);
}
