//! URL operations runtime support
//!
//! Provides JavaScript URL functionality for parsing and working with URLs.
//! URLs use ordinary GC objects with numbered component slots and accessors
//! on `URL.prototype`.

use crate::array::{js_array_alloc, js_array_push_f64};
use crate::object::{js_object_set_field_f64, js_object_set_keys};
use crate::{js_object_alloc, js_string_from_bytes, ArrayHeader, ObjectHeader, StringHeader};

pub mod abort;
pub mod node_compat;
pub mod parse;
pub(crate) mod prototype;
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
pub(crate) use self::url_class::is_url_object_shape;
pub use self::url_class::{
    js_url_can_parse, js_url_can_parse_with_base, js_url_get_hash, js_url_get_host,
    js_url_get_hostname, js_url_get_href, js_url_get_origin, js_url_get_pathname, js_url_get_port,
    js_url_get_protocol, js_url_get_search, js_url_get_search_params, js_url_new,
    js_url_new_with_base, js_url_parse, js_url_parse_with_base, js_url_set_hash, js_url_set_host,
    js_url_set_hostname, js_url_set_href, js_url_set_password, js_url_set_pathname,
    js_url_set_port, js_url_set_protocol, js_url_set_search, js_url_set_username,
};
pub use self::url_pattern::{
    js_url_pattern_constructor_call, js_url_pattern_exec, js_url_pattern_new, js_url_pattern_test,
};

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
#[cfg(feature = "url-engine")]
pub(crate) fn install_engine() {
    crate::url_hooks::install_idna(
        |host| {
            url::Url::parse(&format!("http://{host}/"))
                .ok()
                .and_then(|u| u.host_str().map(str::to_string))
        },
        |domain| idna::domain_to_ascii(domain).ok(),
        |domain| idna::domain_to_unicode(domain).0,
    );
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
    fn test_parse_authority_without_path_special_vs_non_special() {
        // #11322: only special schemes seed an empty path segment (`/`).
        for (input, expected_path) in [
            ("http://a", "/"),
            ("https://a:8443", "/"),
            ("ws://a", "/"),
            ("wss://a", "/"),
            ("ftp://a", "/"),
            ("ilovejs://127.0.0.1:1234", ""),
            ("mongodb://db.local:27017", ""),
            ("foo://a?q=1", ""),
            ("foo://a#h", ""),
            ("foo://a/", "/"),
        ] {
            let (_, _, _, _, pathname, _, _) = parse_url(input);
            assert_eq!(pathname, expected_path, "pathname of {input}");
        }
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
    fn test_parse_file_url_windows_drive_authority() {
        // Matches Node 26.5.1 WHATWG URL normalization for both spellings:
        // `new URL("file://C:\\Users\\Ralph\\config.env")` and the
        // slash-separated equivalent serialize as `file:///C:/Users/...`.
        for input in [
            "file://C:\\Users\\Ralph\\config.env",
            "file://C:/Users/Ralph/config.env",
        ] {
            assert_eq!(
                super::parse::normalize_file_drive_authority(input),
                "file:///C:/Users/Ralph/config.env",
                "normalized {input}"
            );
            assert!(super::parse::is_valid_absolute_url(input), "{input}");
            let (protocol, host, hostname, port, pathname, search, hash) = parse_url(input);
            assert_eq!(protocol, "file:", "{input}");
            assert_eq!(host, "", "{input}");
            assert_eq!(hostname, "", "{input}");
            assert_eq!(port, "", "{input}");
            assert_eq!(pathname, "/C:/Users/Ralph/config.env", "{input}");
            assert_eq!(search, "", "{input}");
            assert_eq!(hash, "", "{input}");
        }

        let (_, host, _, _, pathname, search, hash) =
            parse_url("file://C:/Users/Ralph/config.env?x=\\\\y#\\\\z");
        assert_eq!(host, "");
        assert_eq!(pathname, "/C:/Users/Ralph/config.env");
        assert_eq!(search, "?x=\\\\y");
        assert_eq!(hash, "#\\\\z");
    }

    #[test]
    fn test_parse_file_url_windows_drive_does_not_capture_unc_hosts() {
        let (protocol, host, hostname, _, pathname, _, _) =
            parse_url("file://server/share/config.env");
        assert_eq!(protocol, "file:");
        assert_eq!(host, "server");
        assert_eq!(hostname, "server");
        assert_eq!(pathname, "/share/config.env");

        let (_, host, hostname, _, pathname, _, _) =
            parse_url("file://localhost/C:/share/config.env");
        assert_eq!(host, "");
        assert_eq!(hostname, "");
        assert_eq!(pathname, "/C:/share/config.env");
    }

    #[test]
    fn test_resolve_relative_url() {
        let resolved = resolve_url(".", "file:///Users/test/lib/file.ts");
        assert_eq!(resolved, "file:/Users/test/lib");

        let resolved = resolve_url("..", "file:///Users/test/lib/file.ts");
        assert_eq!(resolved, "file:/Users/test");

        let fixture = resolve_url("../fixtures/", "file:///Users/test/tls/handshake/test.ts");
        assert_eq!(fixture, "file:/Users/test/tls/fixtures/");
        assert_eq!(
            resolve_url("localhost-key.pem", &fixture),
            "file:/Users/test/tls/fixtures/localhost-key.pem"
        );
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

pub(crate) use crate::value::boxed_text::{
    create_string_f64, get_string_content, object_from_f64, object_prop_f64, object_prop_string,
    string_from_header,
};

pub(crate) use crate::url_hooks::{
    idna_domain_to_ascii, idna_domain_to_unicode, whatwg_canonicalize_host,
};
