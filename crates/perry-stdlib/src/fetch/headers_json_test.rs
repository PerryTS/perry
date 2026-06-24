//! Test for `js_headers_fetch_object_json`, kept out of `mod.rs` so that file
//! stays under the 2000-LOC CI limit. `use super::*` carries the fetch
//! module's items (`pub use headers::*` re-exports `js_headers_fetch_object_json`).

use super::*;

/// `js_headers_fetch_object_json` must read a `Headers` handle from the
/// registry and emit a flat `{name:value}` JSON object that
/// `js_fetch_with_options` can parse — WITHOUT dereferencing the handle id
/// as a heap pointer (the `claude -p` `fetch(url, { headers: Headers })`
/// SIGSEGV). Unknown handles yield a null pointer so the caller falls back
/// to `{}`.
#[test]
fn headers_fetch_object_json_serializes_registry_store() {
    let mut store = HeadersStore::default();
    store.set("Content-Type", "application/json");
    store.set("X-Api-Key", "secret");
    let id = alloc_headers(store);
    let handle = handle_to_f64(id);

    let ptr = js_headers_fetch_object_json(handle);
    assert!(!ptr.is_null());
    let json = unsafe { string_from_header(ptr as *const StringHeader) }.unwrap();
    let parsed: std::collections::HashMap<String, String> =
        serde_json::from_str(&json).expect("flat object JSON");
    assert_eq!(
        parsed.get("content-type").map(String::as_str),
        Some("application/json")
    );
    assert_eq!(parsed.get("x-api-key").map(String::as_str), Some("secret"));

    // An unknown handle (never allocated) must not be dereferenced.
    let bogus = handle_to_f64(perry_runtime::value::addr_class::FETCH_HANDLE_BAND_START + 0xABCD);
    assert!(js_headers_fetch_object_json(bogus).is_null());
}
