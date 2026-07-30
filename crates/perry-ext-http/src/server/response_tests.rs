//! Unit tests for `response.rs`. Split into a sibling file so `response.rs`
//! stays under the repository's 2000-line-per-file lint cap; declared as a
//! `#[path]` child module of `response` so `use super::*` resolves to it.

use super::*;

fn empty_response() -> ServerResponse {
    let (tx, _rx) = oneshot::channel::<HyperResponseShape>();
    ServerResponse::new(tx)
}

#[test]
fn write_head_headers_json_merges_and_preserves_case() {
    // #2132: a `writeHead(status, headers)` object is JSON-serialized and
    // merged here; the lookup key is lowercase but the original case is
    // retained for `getHeaderNames()`.
    let mut sr = empty_response();
    apply_headers_json(&mut sr, r#"{"Content-Type":"text/plain","X-Custom":"abc"}"#);
    assert_eq!(
        sr.headers.get("content-type").map(String::as_str),
        Some("text/plain")
    );
    assert_eq!(sr.headers.get("x-custom").map(String::as_str), Some("abc"));
    assert_eq!(
        sr.raw_header_names.get("content-type").map(String::as_str),
        Some("Content-Type")
    );
    assert_eq!(
        sr.raw_header_names.get("x-custom").map(String::as_str),
        Some("X-Custom")
    );
}

#[test]
fn write_head_headers_json_stringifies_non_string_values() {
    let mut sr = empty_response();
    apply_headers_json(&mut sr, r#"{"Content-Length":42,"X-Flag":true}"#);
    assert_eq!(
        sr.headers.get("content-length").map(String::as_str),
        Some("42")
    );
    assert_eq!(sr.headers.get("x-flag").map(String::as_str), Some("true"));
}

#[test]
fn write_head_headers_json_ignores_empty_and_sentinels() {
    let mut sr = empty_response();
    apply_headers_json(&mut sr, "");
    apply_headers_json(&mut sr, "null");
    apply_headers_json(&mut sr, "undefined");
    assert!(sr.headers.is_empty());
}

// #4965 — `setHeaders` entries normalizer output.

#[test]
fn apply_headers_entries_lowercases_and_preserves_case() {
    let mut sr = empty_response();
    apply_headers_entries(&mut sr, r#"[["Foo","1"],["Bar","2"]]"#);
    assert_eq!(sr.headers.get("foo").map(String::as_str), Some("1"));
    assert_eq!(sr.headers.get("bar").map(String::as_str), Some("2"));
    assert_eq!(
        sr.raw_header_names.get("foo").map(String::as_str),
        Some("Foo")
    );
}

#[test]
fn apply_headers_entries_set_cookie_array_keeps_per_element_list() {
    let mut sr = empty_response();
    apply_headers_entries(&mut sr, r#"[["set-cookie",["a=b","c=d"]]]"#);
    assert_eq!(
        sr.header_value_lists.get("set-cookie").map(Vec::as_slice),
        Some(["a=b".to_string(), "c=d".to_string()].as_slice())
    );
    assert_eq!(
        sr.headers.get("set-cookie").map(String::as_str),
        Some("a=b, c=d")
    );
}

#[test]
fn apply_headers_entries_ignores_non_array_and_short_pairs() {
    let mut sr = empty_response();
    apply_headers_entries(&mut sr, r#"[{"foo":"1"},["only-name"],["k","v"]]"#);
    assert_eq!(sr.headers.get("k").map(String::as_str), Some("v"));
    assert_eq!(sr.headers.len(), 1);
}

#[test]
fn write_head_flat_array_applies_pairs_and_overrides() {
    let mut sr = empty_response();
    sr.headers.insert("foo".into(), "1".into());
    apply_headers_flat_array(&mut sr, r#"["foo","3","X-New","z"]"#);
    // even/odd offsets are name/value; `foo` overrides the prior value.
    assert_eq!(sr.headers.get("foo").map(String::as_str), Some("3"));
    assert_eq!(sr.headers.get("x-new").map(String::as_str), Some("z"));
}

// `res.once(event, cb)` — one-shot listener semantics. Regression guard for
// the static-file 64 KB truncation: `res.once('drain')` used to be dropped
// entirely (no `once` dispatch arm), so `createReadStream().pipe(res)`
// stalled after the first high-water-mark chunk.

#[test]
fn once_drain_listener_fires_exactly_once() {
    let mut sr = empty_response();
    sr.once_listeners.entry("drain".into()).or_default().push(7);
    // First consumption returns the once listener...
    assert_eq!(take_event_listeners(&mut sr, "drain"), vec![7]);
    // ...and drains it, so a second drain edge sees nothing.
    assert!(take_event_listeners(&mut sr, "drain").is_empty());
}

#[test]
fn on_drain_listener_survives_repeated_edges() {
    let mut sr = empty_response();
    sr.listeners.entry("drain".into()).or_default().push(9);
    // A persistent `on` listener fires on every drain edge.
    assert_eq!(take_event_listeners(&mut sr, "drain"), vec![9]);
    assert_eq!(take_event_listeners(&mut sr, "drain"), vec![9]);
}

#[test]
fn on_and_once_combine_then_once_drops() {
    let mut sr = empty_response();
    sr.listeners.entry("finish".into()).or_default().push(1);
    sr.once_listeners
        .entry("finish".into())
        .or_default()
        .push(2);
    // `on` listeners first, then `once` listeners.
    assert_eq!(take_event_listeners(&mut sr, "finish"), vec![1, 2]);
    // The `once` listener is gone; only the persistent one remains.
    assert_eq!(take_event_listeners(&mut sr, "finish"), vec![1]);
}
