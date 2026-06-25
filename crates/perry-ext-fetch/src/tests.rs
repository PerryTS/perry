//! Unit tests for the fetch FFI surface. Split out of `lib.rs` to keep
//! that file under the 2,000-line lint gate. As a child module of the
//! crate root, `use super::*` reaches every crate-private item.

use super::*;

#[test]
fn response_count_starts_at_zero() {
    let initial = js_fetch_response_count();
    // Other tests may have populated, but it can't be negative.
    assert!(initial >= 0);
}

#[test]
fn response_status_invalid_handle() {
    assert_eq!(js_fetch_response_status(99_999_999.0), 0.0);
}

#[test]
fn headers_round_trip() {
    let h = js_headers_new();
    let key = alloc_string("Content-Type");
    let value = alloc_string("application/json");
    let set = unsafe { js_headers_set(h, key.as_raw(), value.as_raw()) };
    assert_eq!(set, 1.0);
    let got_ptr = unsafe { js_headers_get(h, key.as_raw()) };
    let got = perry_ffi::read_string(unsafe { JsString::from_raw(got_ptr) }).expect("non-null");
    assert_eq!(got, "application/json");
    let has = unsafe { js_headers_has(h, key.as_raw()) };
    assert_eq!(has, 1.0);
    let del = unsafe { js_headers_delete(h, key.as_raw()) };
    assert_eq!(del, 1.0);
    let has2 = unsafe { js_headers_has(h, key.as_raw()) };
    assert_eq!(has2, 0.0);
}

#[test]
fn headers_append_combines_values() {
    let h = js_headers_new();
    let key = alloc_string("X-Test");
    let first = alloc_string("a");
    let second = alloc_string("b");

    let append_first = unsafe { js_headers_append(h, key.as_raw(), first.as_raw()) };
    let append_second = unsafe { js_headers_append(h, key.as_raw(), second.as_raw()) };
    assert_eq!(append_first, 1.0);
    assert_eq!(append_second, 1.0);

    let got_ptr = unsafe { js_headers_get(h, key.as_raw()) };
    let got = perry_ffi::read_string(unsafe { JsString::from_raw(got_ptr) }).expect("non-null");
    assert_eq!(got, "a, b");
}

#[test]
fn blob_slice_basic() {
    let id = store_blob(BlobData {
        bytes: b"hello, world".to_vec(),
        content_type: "text/plain".to_string(),
    });
    let null = std::ptr::null::<StringHeader>();
    let sliced = unsafe { js_blob_slice(id as f64, 7.0, 12.0, null) };
    assert!(sliced > 0.0);
    let size = js_blob_size(sliced);
    assert_eq!(size, 5.0);
}

#[test]
fn request_round_trip() {
    let url = alloc_string("https://example.com");
    let method = alloc_string("POST");
    let body = alloc_string(r#"{"x":1}"#);
    let null = std::ptr::null::<StringHeader>();
    let h = unsafe {
        js_request_new(
            url.as_raw(),
            method.as_raw(),
            body.as_raw(),
            0.0,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            0.0,
            null,
            0.0,
        )
    };
    assert!(h > 0.0);
    let url_ptr = js_request_get_url(h);
    let url_str = perry_ffi::read_string(unsafe { JsString::from_raw(url_ptr) }).expect("non-null");
    assert_eq!(url_str, "https://example.com");
    let method_ptr = js_request_get_method(h);
    let method_str =
        perry_ffi::read_string(unsafe { JsString::from_raw(method_ptr) }).expect("non-null");
    assert_eq!(method_str, "POST");
}

#[test]
fn response_static_json() {
    let v = JsValue::from_string_ptr(alloc_string("hello").as_raw());
    // No init: status defaults to 200, no statusText, no headers.
    let resp =
        unsafe { js_response_static_json(f64::from_bits(v.bits()), 0.0, std::ptr::null(), 0.0) };
    assert!(resp > 0.0);
    let status = js_fetch_response_status(resp);
    assert_eq!(status, 200.0);
}

// #1688: request.text()/.json()/.arrayBuffer() were unimplemented. The
// FFIs build a JsPromise (runtime symbols unavailable in the unittest
// binary, as with every other promise-returning fetch FFI), so this
// exercises the shared body data path they consume: a stored body
// round-trips, a bodiless request reads as "", and an invalid handle is
// None (→ the FFI rejects).
#[test]
fn request_body_data_path() {
    let url = alloc_string("https://example.com");
    let method = alloc_string("POST");
    let body = alloc_string(r#"{"x":1}"#);
    let null = std::ptr::null::<StringHeader>();
    let h = unsafe {
        js_request_new(
            url.as_raw(),
            method.as_raw(),
            body.as_raw(),
            0.0,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            0.0,
            null,
            0.0,
        )
    };
    assert!(h > 0.0);
    assert_eq!(request_body_string(h).as_deref(), Some(r#"{"x":1}"#));

    let url2 = alloc_string("https://example.com/empty");
    let h2 = unsafe {
        js_request_new(
            url2.as_raw(),
            null,
            null,
            0.0,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            0.0,
            null,
            0.0,
        )
    };
    assert_eq!(request_body_string(h2).as_deref(), Some(""));

    assert_eq!(request_body_string(99_999_999.0), None);
}

// A body holding non-UTF-8 bytes (here: 0xFF 0xFE 0x00 0x80 — invalid
// UTF-8, with an embedded NUL). Asserts the same bytes survive every
// stage that matters.
const NON_UTF8: &[u8] = &[0xFF, 0xFE, 0x00, 0x80, b'P', b'N', b'G'];

fn store_with_body(body: Bytes) -> usize {
    store_response(FetchResponse {
        status: 200,
        status_text: "OK".to_string(),
        headers: HeadersStore::default(),
        body,
        type_name: "basic".to_string(),
        url: "https://example.com/bin".to_string(),
        redirected: false,
    })
}

// Regression for the `arrayBuffer`/`bytes` non-UTF-8 corruption bug. The
// old path round-tripped the body through `from_utf8_unchecked` → String
// → `from_string_ptr`, which (a) is UB on non-UTF-8 and (b) resolved a
// STRING_TAG value that the JS `new Uint8Array(...)` dispatch reads as an
// empty buffer. FAIL-BEFORE: a string-tagged value, not a buffer, so
// `is_pointer()` is false and `read_buffer_bytes` cannot recover the
// payload. PASS-AFTER: a POINTER_TAG Buffer whose bytes are byte-exact.
#[test]
fn array_buffer_value_is_byte_exact_buffer() {
    let value = body_to_buffer_value(NON_UTF8);
    assert!(
        value.is_pointer(),
        "arrayBuffer()/bytes() must resolve a Buffer (POINTER_TAG), not a string"
    );
    assert!(!value.is_string());
    let buf = value.as_pointer::<perry_ffi::BufferHeader>();
    let read = perry_ffi::read_buffer_bytes(buf).expect("non-null buffer");
    assert_eq!(read, NON_UTF8, "fetched binary body must be byte-exact");
}

// The blob path covers the two halves `js_blob_array_buffer` composes:
// `blob()` stores the body bytes intact, and the buffer seam those bytes
// flow through resolves them byte-exact. (The FFI itself returns a
// `*mut Promise`; the promise-resolution machinery is unavailable in the
// unittest binary, as for every promise-returning fetch FFI — hence the
// two-halves decomposition rather than an end-to-end call. The end-to-end
// `blob().arrayBuffer()` chain is exercised by the e2e shell test.)
#[test]
fn blob_round_trips_non_utf8_bytes() {
    let blob_id = store_blob(BlobData {
        bytes: NON_UTF8.to_vec(),
        content_type: "image/png".to_string(),
    });
    // The stored blob keeps the exact bytes that `js_blob_array_buffer`
    // reads and hands to `body_to_buffer_value`.
    let stored = BLOB_HANDLES
        .lock()
        .unwrap()
        .get(&blob_id)
        .map(|b| b.bytes.clone())
        .expect("blob stored");
    assert_eq!(stored, NON_UTF8);
    // …and the seam resolves those bytes as a byte-exact Buffer.
    let value = body_to_buffer_value(&stored);
    assert!(value.is_pointer());
    let read = perry_ffi::read_buffer_bytes(value.as_pointer::<perry_ffi::BufferHeader>())
        .expect("non-null buffer");
    assert_eq!(read, NON_UTF8);
}

// Output-preserving regression guard for the `Vec<u8>` → `Bytes` migration
// (NOT a fail-before-the-bug test — it guards part 1 of the change, the
// decode fold + zero-copy body, against future regression). A valid-UTF-8
// body must still decode (via `from_utf8_lossy`, as `text()`/`json()` do)
// to the identical string, and the `Bytes` body must preserve arbitrary
// bytes verbatim across `store_response` + the refcount clone.
#[test]
fn text_decode_preserves_utf8_and_bytes_round_trip() {
    let utf8 = "héllo, wörld — 𝓊𝓃𝒾𝒸ℴ𝒹ℯ";
    let id = store_with_body(Bytes::from(utf8.as_bytes().to_vec()));
    let stored = FETCH_RESPONSES
        .lock()
        .unwrap()
        .get(&id)
        .map(|r| r.body.clone())
        .expect("response stored");
    // What `text()`/`json()` now produce: a lossless decode of valid UTF-8.
    assert_eq!(String::from_utf8_lossy(&stored), utf8);
    // The `Bytes` body holds arbitrary bytes verbatim (no copy on clone).
    let bin_id = store_with_body(Bytes::from(NON_UTF8.to_vec()));
    let bin = FETCH_RESPONSES
        .lock()
        .unwrap()
        .get(&bin_id)
        .map(|r| r.body.clone())
        .expect("response stored");
    assert_eq!(&bin[..], NON_UTF8);
}
