//! URL operations runtime support
//!
//! Provides JavaScript URL functionality for parsing and working with URLs.
//! URLs are represented as regular JavaScript objects with string fields.

use crate::array::{js_array_alloc, js_array_push_f64};
use crate::object::{js_object_set_field_f64, js_object_set_keys};
use crate::{js_object_alloc, js_string_from_bytes, ArrayHeader, ObjectHeader, StringHeader};

pub mod abort;
pub mod node_compat;
pub mod parse;
pub mod search_params;
pub mod url_class;
pub mod url_pattern;

// Explicit named re-exports for all public FFI symbols + helpers used by the
// rest of the runtime / codegen layer. Globs are intentionally avoided so
// callers see precisely which symbols cross module boundaries.

pub use self::abort::{
    js_abort_controller_abort, js_abort_controller_abort_reason, js_abort_controller_new,
    js_abort_controller_signal, js_abort_error_value, js_abort_signal_abort,
    js_abort_signal_add_listener, js_abort_signal_any, js_abort_signal_is_aborted,
    js_abort_signal_remove_listener, js_abort_signal_resolve_ptr, js_abort_signal_throw_if_aborted,
    js_abort_signal_timeout,
};
pub use self::node_compat::{
    js_url_domain_to_ascii, js_url_domain_to_unicode, js_url_file_url_to_path,
    js_url_file_url_to_path_buffer, js_url_format, js_url_legacy_parse, js_url_legacy_resolve,
    js_url_legacy_resolve_object, js_url_legacy_url_new, js_url_path_to_file_url,
    js_url_to_http_options,
};
pub use self::search_params::{
    js_url_search_params_append, js_url_search_params_delete, js_url_search_params_delete2,
    js_url_search_params_entries_arr, js_url_search_params_for_each, js_url_search_params_get,
    js_url_search_params_get_all, js_url_search_params_has, js_url_search_params_has2,
    js_url_search_params_keys_arr, js_url_search_params_new, js_url_search_params_new_any,
    js_url_search_params_new_empty, js_url_search_params_set, js_url_search_params_size,
    js_url_search_params_sort, js_url_search_params_throw_missing_args,
    js_url_search_params_to_string, js_url_search_params_values_arr,
};
// #1668: crate-internal detector so `Object.fromEntries`/spread can recognise
// a URLSearchParams (a plain class_id-0 ObjectHeader) and pull its entries.
pub(crate) use self::search_params::try_read_as_search_params;
pub(crate) use self::url_class::is_url_object_shape;
/// Canonical class-id for `URL` instances (see instanceof / branding paths).
pub(crate) const CLASS_ID_URL: u32 = 0xFFFF002F;
pub use self::url_class::{
    js_url_can_parse, js_url_can_parse_with_base, js_url_get_hash, js_url_get_host,
    js_url_get_hostname, js_url_get_href, js_url_get_origin, js_url_get_pathname, js_url_get_port,
    js_url_get_protocol, js_url_get_search, js_url_get_search_params, js_url_new,
    js_url_new_with_base, js_url_parse, js_url_parse_with_base, js_url_set_hash,
    js_url_set_hostname, js_url_set_href, js_url_set_password, js_url_set_pathname,
    js_url_set_port, js_url_set_protocol, js_url_set_search, js_url_set_username,
};
pub use self::url_pattern::{
    js_url_pattern_constructor_call, js_url_pattern_exec, js_url_pattern_new, js_url_pattern_test,
};

// ---------------------------------------------------------------------------
// Shared helpers used across the URL sub-modules. Promoted to `pub(crate)` so
// the sibling modules (which `use super::*`) can reach them without going
// back through the public re-export surface.
// ---------------------------------------------------------------------------

/// Create a string from a Rust str (returns a StringHeader pointer as f64)
/// Uses proper NaN-boxing with STRING_TAG so is_string() will return true
pub(crate) fn create_string_f64(s: &str) -> f64 {
    let bytes = s.as_bytes();
    let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    // Use js_nanbox_string to properly tag the string pointer
    crate::value::js_nanbox_string(ptr as i64)
}

/// Get string content from a NaN-boxed StringHeader pointer (passed as f64)
pub(crate) fn get_string_content(ptr_f64: f64) -> String {
    // Extract the pointer from NaN-boxed value using proper unboxing
    let ptr_i64 = crate::value::js_nanbox_get_string_pointer(ptr_f64);
    let ptr: *mut StringHeader = ptr_i64 as *mut StringHeader;
    if ptr.is_null() || ptr_i64 == 0 {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let slice = std::slice::from_raw_parts(data_ptr, len);
        String::from_utf8_lossy(slice).into_owned()
    }
}

pub(crate) fn string_from_header(ptr: *mut crate::StringHeader) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data_ptr = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let slice = std::slice::from_raw_parts(data_ptr, len);
        String::from_utf8_lossy(slice).into_owned()
    }
}

/// `String(value)` per Web-IDL / ECMAScript string conversion, used to
/// normalize arguments to the WHATWG URL / URLSearchParams APIs (#3054,
/// #3055). Numbers, `null`, `undefined`, booleans, BigInts, and objects with
/// a custom `toString`/`valueOf` are stringified; **Symbols throw**
/// `TypeError: Cannot convert a Symbol value to a string` (matching Node /
/// V8). Returns a heap `*mut StringHeader` (never null for non-symbol inputs).
///
/// Callers previously routed arguments through `js_get_string_pointer_unified`,
/// which only extracts an existing string pointer and yields a null/garbage
/// pointer for non-string values — so `new URL(123, base)` lost its argument
/// and symbols silently produced the wrong result instead of throwing. This
/// mirrors `text::text_encoder_string_ptr`, the analogous coercion used by
/// `TextEncoder.encode`.
#[no_mangle]
pub extern "C" fn js_url_coerce_string(value: f64) -> *mut StringHeader {
    if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
        let msg = b"Cannot convert a Symbol value to a string";
        let m = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err = crate::error::js_typeerror_new(m);
        let bits = crate::value::JSValue::pointer(err as *const u8).bits();
        crate::exception::js_throw(f64::from_bits(bits));
    }
    crate::value::js_jsvalue_to_string(value)
}

pub(crate) fn object_from_f64(value: f64) -> Option<*mut ObjectHeader> {
    let bits = value.to_bits();
    if (bits & 0xFFFF_0000_0000_0000) == 0x7FFD_0000_0000_0000 {
        let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
        if crate::value::addr_class::is_plausible_heap_addr(addr) {
            return Some(addr as *mut ObjectHeader);
        }
    }
    None
}

pub(crate) fn object_prop_string(obj: *mut ObjectHeader, key: &str) -> String {
    let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
    let val = crate::object::js_object_get_field_by_name_f64(obj, key_ptr);
    get_string_content(val)
}

pub(crate) fn object_prop_f64(obj: *mut ObjectHeader, key: &str) -> f64 {
    let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
    crate::object::js_object_get_field_by_name_f64(obj, key_ptr)
}

/// Read a `*mut StringHeader` (NULL → empty) into a Rust `String`.
pub(crate) fn string_header_to_string(value: *mut crate::StringHeader) -> String {
    if value.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*value).byte_len as usize;
        let data_ptr = (value as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let slice = std::slice::from_raw_parts(data_ptr, len);
        String::from_utf8_lossy(slice).into_owned()
    }
}

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
    #[cfg(feature = "url-engine")]
    {
        url::Url::parse(&format!("http://{host}/"))
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
    }
    // URL engine gated off: no WHATWG host parser, so pass the host through
    // unchanged (the hand-rolled URL paths handle the common cases).
    #[cfg(not(feature = "url-engine"))]
    Some(host.to_string())
}

/// True when `host` is a canonical dotted-quad IPv4 literal. Used by
/// `domainToUnicode` to decide whether to return the canonicalized IPv4
/// (Node yields the IP for numeric hosts) versus the Unicode IDN form.
pub(crate) fn is_ipv4_host(host: &str) -> bool {
    host.parse::<std::net::Ipv4Addr>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::parse::{parse_url, resolve_url};
    use super::search_params::{
        create_url_search_params_object, get_url_search_params_entries, parse_query_string,
    };
    use super::{create_string_f64, get_string_content};

    #[test]
    fn test_parse_simple_url() {
        let (protocol, host, hostname, port, pathname, search, hash) =
            parse_url("https://example.com/path?query=1#section");
        assert_eq!(protocol, "https:");
        assert_eq!(host, "example.com");
        assert_eq!(hostname, "example.com");
        assert_eq!(port, "");
        assert_eq!(pathname, "/path");
        assert_eq!(search, "?query=1");
        assert_eq!(hash, "#section");
    }

    #[test]
    fn test_parse_url_with_port() {
        let (protocol, host, hostname, port, pathname, _, _) =
            parse_url("http://localhost:3000/api");
        assert_eq!(protocol, "http:");
        assert_eq!(host, "localhost:3000");
        assert_eq!(hostname, "localhost");
        assert_eq!(port, "3000");
        assert_eq!(pathname, "/api");
    }

    #[test]
    fn test_parse_file_url() {
        let (protocol, host, hostname, _, pathname, _, _) = parse_url("file:///Users/test/file.ts");
        assert_eq!(protocol, "file:");
        assert_eq!(host, "");
        assert_eq!(hostname, "");
        assert_eq!(pathname, "/Users/test/file.ts");
    }

    #[test]
    fn test_parse_file_url_host() {
        let (protocol, host, hostname, _, pathname, _, _) =
            parse_url("file://example.com/Users/test/file.ts");
        assert_eq!(protocol, "file:");
        assert_eq!(host, "example.com");
        assert_eq!(hostname, "example.com");
        assert_eq!(pathname, "/Users/test/file.ts");

        let (_, host, hostname, _, pathname, _, _) =
            parse_url("file://localhost/Users/test/file.ts");
        assert_eq!(host, "");
        assert_eq!(hostname, "");
        assert_eq!(pathname, "/Users/test/file.ts");
    }

    #[test]
    fn test_resolve_relative_url() {
        let resolved = resolve_url(".", "file:///Users/test/lib/file.ts");
        assert_eq!(resolved, "file:/Users/test/lib");

        let resolved = resolve_url("..", "file:///Users/test/lib/file.ts");
        assert_eq!(resolved, "file:/Users/test");
    }

    #[test]
    fn test_parse_query_string() {
        let entries = parse_query_string("foo=bar&baz=qux");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], ("foo".to_string(), "bar".to_string()));
        assert_eq!(entries[1], ("baz".to_string(), "qux".to_string()));
    }

    #[test]
    fn test_url_search_params_entries() {
        let entries = vec![
            ("key1".to_string(), "value1".to_string()),
            ("key2".to_string(), "value2".to_string()),
        ];
        let params = create_url_search_params_object(entries);

        let read_entries = get_url_search_params_entries(params);
        assert_eq!(
            read_entries.len(),
            2,
            "Expected 2 entries, got {}",
            read_entries.len()
        );
        assert_eq!(read_entries[0].0, "key1");
        assert_eq!(read_entries[0].1, "value1");
        assert_eq!(read_entries[1].0, "key2");
        assert_eq!(read_entries[1].1, "value2");
    }

    #[test]
    fn test_string_round_trip() {
        // Test that create_string_f64 and get_string_content round-trip correctly
        let original = "test string";
        let f64_val = create_string_f64(original);
        let recovered = get_string_content(f64_val);
        assert_eq!(
            recovered, original,
            "String round-trip failed: expected '{}', got '{}'",
            original, recovered
        );
    }
}
