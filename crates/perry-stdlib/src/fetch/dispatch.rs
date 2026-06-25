//! Untyped Web Fetch handle dispatch (refs #421, #1698).
//!
//! Split out of `fetch/mod.rs` to keep that file under the 2,000-line lint
//! gate (mirrors the `headers.rs` extraction). As a child module of `fetch`,
//! this sees `mod.rs`'s private items (the `*_REGISTRY` statics,
//! `handle_to_f64`, `handle_id`, `js_class_method_bind` decls, `request_headers_handle`,
//! `response_body_stream`, the `TAG_*` consts, the `js_request_*` / `js_fetch_*`
//! / `js_blob_*` / `js_headers_*` FFIs, …) through the glob `use super::*` — no
//! extra visibility changes required.
//!
//! These functions back `HANDLE_METHOD_DISPATCH` / `HANDLE_PROPERTY_DISPATCH`
//! (registered by `common/dispatch.rs`): when codegen loses a handle's static
//! Web-Fetch type (any-typed npm packages, computed keys, `as any`), method
//! calls and property reads land here. Each gates on registry-membership +
//! name vocabulary; fetch-family ids are unified (`NEXT_FETCH_HANDLE_ID`) so a
//! Request id never collides with a Response/Headers/Blob id.

use super::*;
use perry_runtime::{js_get_string_pointer_unified, js_jsvalue_to_string};

/// Coerce a `Response` body-init value to a `*const StringHeader` (returned as
/// i64, mirroring `js_get_string_pointer_unified`).
///
/// A `ReadableStream` handle — e.g. another Response's `.body` — is a plain
/// numeric f64 id with no NaN-box tag, so the generic string coercion would
/// stringify the handle to its number and discard the real bytes. Hono re-wraps
/// responses to mutate headers via `new Response(res.body, res)`, so the body
/// must be DRAINED from the stream's buffered chunks instead. Any non-stream
/// value falls back to the normal coercion, so plain string bodies
/// (`c.json`/`c.text`) are unaffected.
#[no_mangle]
pub extern "C" fn js_response_body_init_ptr(value: f64) -> i64 {
    // A binary body — Buffer / Uint8Array / typed array / ArrayBuffer — must
    // copy its RAW bytes. Such a value is a BufferHeader/TypedArrayHeader
    // pointer, NOT a StringHeader; passing it straight to `string_from_header`
    // read the byte length but data at the wrong offset (zero-fill), so the
    // payload came back all zeroes (#5435). Materialize the bytes into a heap
    // StringHeader so `js_response_new`'s lossless byte read recovers them.
    if let Some(bytes) = unsafe { body_value_buffer_bytes(value) } {
        return unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32) } as i64;
    }
    // A non-integral or out-of-range value can't be a stream, so skip the
    // registry probe for the common cases.
    if value.is_finite()
        && value.fract() == 0.0
        && ((crate::streams::STREAM_HANDLE_ID_START as f64)
            ..(crate::streams::STREAM_HANDLE_ID_END as f64))
            .contains(&value)
    {
        let id = value as usize;
        // kind == 1 ⇒ live ReadableStream.
        if crate::streams::js_stream_handle_kind(id) == 1 {
            let bytes = crate::streams::drain_readable_into_bytes(id);
            return unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32) } as i64;
        }
    }
    // #5437: a Node `IncomingMessage` body — the request-body bridge Next.js's
    // `NextRequestAdapter.fromNodeNextRequest` relies on. It sets the web
    // `Request` body to the `NodeNextRequest`'s `.body`, which is the underlying
    // `IncomingMessage` (a native handle: `POINTER_TAG | small id`, not bytes).
    // Stringifying it below yielded `"[object Object]"`, so `req.json()` /
    // `req.text()` saw garbage and POST bodies were silently lost. Read the
    // request's buffered bytes through the handle-property dispatch — the node
    // http impl exposes them as a Buffer under `rawBody` (`js_node_http_im_raw_body`)
    // — and materialize a lossless StringHeader from them. Only a small-handle
    // POINTER value is probed, so string / heap-object / buffer bodies above are
    // untouched. A handle without a buffered `rawBody` falls through to ToString.
    {
        let jsval = JSValue::from_bits(value.to_bits());
        if jsval.is_pointer() {
            let raw = jsval.as_pointer::<u8>() as usize;
            if raw != 0 && raw < 0x10000 {
                let key = unsafe { js_string_from_bytes(b"rawBody".as_ptr(), 7) };
                let raw_body = perry_runtime::object::js_object_get_field_by_name_f64(
                    raw as *const perry_runtime::object::ObjectHeader,
                    key,
                );
                if let Some(bytes) = unsafe { body_value_buffer_bytes(raw_body) } {
                    return unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32) }
                        as i64;
                }
            }
        }
    }
    // A heap-object body — a boxed `String` (hono's `raw()` / JSX `c.html()`
    // returns `new String(value)` with an `isEscaped` expando), an array, or a
    // plain object — is a `POINTER_TAG` value. `js_get_string_pointer_unified`
    // would hand back that object's raw address verbatim, so `js_response_new`
    // then read a bogus `byte_len` off the `ObjectHeader` (offset 4) and
    // `memmove`'d ~4 GB of it → SIGSEGV on the first server-rendered admin page
    // (#5453). The old `string_from_header` path survived only by luck: its
    // `str::from_utf8` bailed on the first non-UTF-8 byte and returned an empty
    // body; #5435's lossless raw-byte read removed that accidental guard.
    //
    // Per the Fetch spec a non-stream/non-buffer body is coerced with ToString,
    // so route object bodies through the runtime's ToString (`valueOf`/
    // `toString`/`[Symbol.toPrimitive]`) — for a boxed `String` that yields its
    // underlying primitive, the result is always a real `StringHeader`.
    if JSValue::from_bits(value.to_bits()).is_pointer() {
        return js_jsvalue_to_string(value) as i64;
    }
    js_get_string_pointer_unified(value)
}

/// Read a body `*const StringHeader` losslessly as raw bytes (no UTF-8
/// validation). Mirrors `string_from_header` but never round-trips through
/// `str::from_utf8`, so a binary body materialized by `js_response_body_init_ptr`
/// (a Buffer / Uint8Array / ArrayBuffer copied verbatim into a StringHeader)
/// keeps every byte instead of being dropped when the bytes aren't valid UTF-8.
/// Refs #5435. `None` for a null/sub-page pointer (no body).
pub(crate) unsafe fn body_bytes_from_header(ptr: *const StringHeader) -> Option<Vec<u8>> {
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    Some(std::slice::from_raw_parts(data_ptr, len).to_vec())
}

/// If `value` is a binary body — a typed array, Buffer/Uint8Array, or
/// ArrayBuffer — return a copy of its raw bytes; `None` for anything else
/// (strings, stream handles, numbers) so callers fall back to string coercion.
///
/// A typed array / buffer is laid out as `BufferHeader` (length at offset 0,
/// data at offset 8) or `TypedArrayHeader`, NOT `StringHeader` (data at offset
/// 20). Feeding such a pointer straight to `string_from_header` read the byte
/// length correctly but the data at the wrong offset — zero-filled padding —
/// so `new Response(new Uint8Array([1,2,3]))` came back all zeroes (#5435).
/// Materializing the real bytes here lets the body round-trip byte-for-byte.
pub(crate) unsafe fn body_value_buffer_bytes(value: f64) -> Option<Vec<u8>> {
    let jsval = JSValue::from_bits(value.to_bits());
    // A typed array / Buffer / ArrayBuffer body is a POINTER_TAG value; decode
    // it via the shared `JSValue` accessors rather than reconstructing the tag
    // mask here. A raw untagged heap pointer (no NaN-box tag) is also accepted.
    // A string body is STRING_TAG, so `is_pointer()` is false and it falls
    // through to the ordinary string coercion.
    let addr = if jsval.is_pointer() {
        jsval.as_pointer::<u8>() as usize
    } else {
        let bits = value.to_bits();
        if (bits >> 48) == 0 && bits >= 0x10000 {
            bits as usize
        } else {
            return None;
        }
    };
    body_addr_buffer_bytes(addr)
}

/// Registry-probe core shared by `body_value_buffer_bytes` (which first decodes
/// a NaN-boxed body *value* to an address) and `js_request_new` (whose body
/// argument codegen already decoded to a raw heap address via
/// `js_get_string_pointer_unified`). Returns a copy of the raw bytes when `addr`
/// is a registered typed array / Buffer / ArrayBuffer, else `None` so the caller
/// falls back to a StringHeader read. A Buffer/Uint8Array body fed straight to
/// `string_from_header` read its byte length off the right field but its data
/// off the StringHeader data offset (20) instead of the buffer data offset (8),
/// shifting every binary body left by 12 bytes (#5483, the Request-side twin of
/// #5435's zero-fill).
pub(crate) unsafe fn body_addr_buffer_bytes(addr: usize) -> Option<Vec<u8>> {
    if addr < 0x1000 {
        return None;
    }
    if perry_runtime::typedarray::lookup_typed_array_kind(addr).is_some() {
        let ta = addr as *const perry_runtime::typedarray::TypedArrayHeader;
        return perry_runtime::typedarray::typed_array_bytes(ta).map(|b| b.to_vec());
    }
    if perry_runtime::buffer::is_registered_buffer(addr) {
        let ptr = addr as *const perry_runtime::buffer::BufferHeader;
        let len = (*ptr).length as usize;
        let data = perry_runtime::buffer::buffer_data(ptr);
        return Some(std::slice::from_raw_parts(data, len).to_vec());
    }
    None
}

lazy_static::lazy_static! {
    static ref FORM_DATA_METHOD_VALUE_CACHE: Mutex<HashMap<(usize, &'static str), u64>> =
        Mutex::new(HashMap::new());
}

fn form_data_bound_method_value(form_id: usize, method_name: &'static str) -> f64 {
    if let Some(bits) = FORM_DATA_METHOD_VALUE_CACHE
        .lock()
        .unwrap()
        .get(&(form_id, method_name))
        .copied()
    {
        return f64::from_bits(bits);
    }

    extern "C" {
        fn js_write_barrier_root_nanbox(value_bits: u64);
    }

    let closure =
        perry_runtime::closure::js_closure_alloc(perry_runtime::closure::BOUND_METHOD_FUNC_PTR, 3);
    perry_runtime::closure::js_closure_set_capture_f64(closure, 0, handle_to_f64(form_id));
    perry_runtime::closure::js_closure_set_capture_ptr(closure, 1, method_name.as_ptr() as i64);
    perry_runtime::closure::js_closure_set_capture_ptr(closure, 2, method_name.len() as i64);
    let value = perry_runtime::value::js_nanbox_pointer(closure as i64);
    unsafe { js_write_barrier_root_nanbox(value.to_bits()) };
    FORM_DATA_METHOD_VALUE_CACHE
        .lock()
        .unwrap()
        .insert((form_id, method_name), value.to_bits());
    value
}

/// `instanceof` kind-probe for fetch handles (registered with the runtime at
/// init via `js_register_fetch_handle_kind_probe`). Returns 0 = none,
/// 1 = Response, 2 = Request, 3 = Headers, 4 = Blob. Lets `x instanceof
/// Response` (etc.) resolve for the pointer-tagged small-integer handles these
/// types use instead of heap objects. Lives here (not `mod.rs`) to keep that
/// file under the 2,000-line lint gate.
#[no_mangle]
pub extern "C" fn js_fetch_handle_kind(id: usize) -> u8 {
    if FETCH_RESPONSES.lock().unwrap().contains_key(&id) {
        return 1;
    }
    if REQUEST_REGISTRY.lock().unwrap().contains_key(&id) {
        return 2;
    }
    if HEADERS_REGISTRY.lock().unwrap().contains_key(&id) {
        return 3;
    }
    if BLOB_REGISTRY.lock().unwrap().contains_key(&id) {
        return 4;
    }
    0
}

// ----------------- Untyped property dispatch (refs #421) -----------------
//
// When user code accesses a property on a Web Fetch handle whose static type
// isn't known to codegen (e.g. hono's bundled JS where TS annotations are
// stripped: `(request) => request.url`), the codegen falls through to
// `js_object_get_field_by_name` in perry-runtime. That dispatcher strips
// the POINTER_TAG, sees a small id, and routes to `HANDLE_PROPERTY_DISPATCH`
// (registered by perry-stdlib's `dispatch.rs`). The functions below let the
// stdlib dispatcher resolve Web Fetch properties without exposing the
// per-subsystem registries publicly.

/// Try to read a property off a Request handle by registry id.
/// Returns `Some(value)` only when both (a) the id is in REQUEST_REGISTRY AND
/// (b) the property name is one this type exposes. Returns `None` otherwise so
/// the dispatcher falls through to the next handler — Web Fetch registries use
/// disjoint integer-id namespaces (Request id 1 ≠ Response id 1), so a
/// "membership only" Some(undefined) catch-all would shadow legitimate
/// property reads on a Response handle whose id collides with a Request id.
#[doc(hidden)]
pub fn dispatch_request_property(req_id: usize, prop: &str) -> Option<f64> {
    // `request.headers` — lazily allocate a Headers registry entry backed by
    // the request's stored header map and cache the id so repeat reads return
    // the same handle (`req.headers === req.headers`). Mirrors the Response
    // path. Without this arm `.headers` fell through to a numeric handle and
    // `req.headers.get(...)` threw "(number).get is not a function" — the
    // root cause of every Hono adapter crashing on the first request (#1649).
    if prop == "headers" {
        // Membership check first so a non-Request id falls through.
        REQUEST_REGISTRY.lock().unwrap().get(&req_id)?;
        return Some(request_headers_handle(req_id));
    }
    // #1698: body methods read AS VALUES (`typeof req.json`, `req[key]` where
    // key is a runtime string, `const f = req.json`). Return a bound-method
    // closure so `typeof` reports "function" and the value stays callable —
    // calling it routes back through `js_native_call_method` → small-handle →
    // `dispatch_request_method`. Mirrors #1670's stream property dispatch.
    if matches!(
        prop,
        "json" | "text" | "arrayBuffer" | "blob" | "bytes" | "formData" | "clone"
    ) {
        // Membership check first so a non-Request id falls through.
        REQUEST_REGISTRY.lock().unwrap().get(&req_id)?;
        extern "C" {
            fn js_class_method_bind(
                instance: f64,
                method_name_ptr: *const u8,
                method_name_len: usize,
            ) -> f64;
        }
        let name: &'static [u8] = match prop {
            "json" => b"json",
            "text" => b"text",
            "arrayBuffer" => b"arrayBuffer",
            "blob" => b"blob",
            "bytes" => b"bytes",
            "formData" => b"formData",
            "clone" => b"clone",
            _ => unreachable!(),
        };
        return Some(unsafe {
            js_class_method_bind(handle_to_f64(req_id), name.as_ptr(), name.len())
        });
    }
    let guard = REQUEST_REGISTRY.lock().unwrap();
    let req = guard.get(&req_id)?;
    let bits = match prop {
        "url" => {
            let p = js_string_from_bytes(req.url.as_ptr(), req.url.len() as u32);
            JSValue::string_ptr(p).bits()
        }
        "method" => {
            let p = js_string_from_bytes(req.method.as_ptr(), req.method.len() as u32);
            JSValue::string_ptr(p).bits()
        }
        "destination" => {
            let p = js_string_from_bytes(req.destination.as_ptr(), req.destination.len() as u32);
            JSValue::string_ptr(p).bits()
        }
        "referrer" => {
            let p = js_string_from_bytes(req.referrer.as_ptr(), req.referrer.len() as u32);
            JSValue::string_ptr(p).bits()
        }
        "referrerPolicy" => {
            let p = js_string_from_bytes(
                req.referrer_policy.as_ptr(),
                req.referrer_policy.len() as u32,
            );
            JSValue::string_ptr(p).bits()
        }
        "mode" => {
            let p = js_string_from_bytes(req.mode.as_ptr(), req.mode.len() as u32);
            JSValue::string_ptr(p).bits()
        }
        "credentials" => {
            let p = js_string_from_bytes(req.credentials.as_ptr(), req.credentials.len() as u32);
            JSValue::string_ptr(p).bits()
        }
        "cache" => {
            let p = js_string_from_bytes(req.cache.as_ptr(), req.cache.len() as u32);
            JSValue::string_ptr(p).bits()
        }
        "redirect" => {
            let p = js_string_from_bytes(req.redirect.as_ptr(), req.redirect.len() as u32);
            JSValue::string_ptr(p).bits()
        }
        "integrity" => {
            let p = js_string_from_bytes(req.integrity.as_ptr(), req.integrity.len() as u32);
            JSValue::string_ptr(p).bits()
        }
        "duplex" => {
            let p = js_string_from_bytes(req.duplex.as_ptr(), req.duplex.len() as u32);
            JSValue::string_ptr(p).bits()
        }
        "body" => match &req.body {
            Some(b) => {
                let p = js_string_from_bytes(b.as_ptr(), b.len() as u32);
                JSValue::string_ptr(p).bits()
            }
            None => TAG_NULL,
        },
        "bodyUsed" => {
            return Some(tagged_bool(req.body_used));
        }
        "keepalive" => return Some(tagged_bool(req.keepalive)),
        "signal" => return Some(req.signal),
        // Other Request properties not yet wired — fall through so other
        // dispatchers (or the final undefined fallback) can answer.
        _ => return None,
    };
    Some(f64::from_bits(bits))
}

/// #1698: try to dispatch a method call on a Request handle by registry id.
/// Returns `Some(result)` only when the id is a known Request AND `method` is a
/// body-consuming method. Returns `None` (fall through) otherwise. Mirrors
/// `dispatch_response_method`. This is the runtime path that makes computed-key
/// and any-typed calls work — `req[key]()`, `(req as any).json()`, and Hono's
/// internal `raw[key]()` all lose the static `Request` type at codegen and land
/// in `js_native_call_method` → small-handle range check → `js_handle_method_dispatch`
/// → here. (The typed `req.json()` direct-call form is still handled earlier by
/// the codegen `module == "Request"` arm from #1688.) Fetch-family ids are
/// unified (`NEXT_FETCH_HANDLE_ID`), so a Request id never collides with a
/// Response/Headers/Blob id and the registry-membership gate is unambiguous.
#[doc(hidden)]
pub fn dispatch_request_method(req_id: usize, method: &str, _args: &[f64]) -> Option<f64> {
    {
        let guard = REQUEST_REGISTRY.lock().unwrap();
        guard.get(&req_id)?;
    }
    let req_f64 = handle_to_f64(req_id);
    unsafe {
        match method {
            "text" => {
                let promise = js_request_text(req_f64);
                Some(f64::from_bits(JSValue::pointer(promise as *mut u8).bits()))
            }
            "json" => {
                let promise = js_request_json(req_f64);
                Some(f64::from_bits(JSValue::pointer(promise as *mut u8).bits()))
            }
            "arrayBuffer" => {
                let promise = js_request_array_buffer(req_f64);
                Some(f64::from_bits(JSValue::pointer(promise as *mut u8).bits()))
            }
            "blob" => {
                let promise = js_request_blob(req_f64);
                Some(f64::from_bits(JSValue::pointer(promise as *mut u8).bits()))
            }
            "bytes" => {
                let promise = js_request_bytes(req_f64);
                Some(f64::from_bits(JSValue::pointer(promise as *mut u8).bits()))
            }
            "formData" => {
                let promise = js_request_form_data(req_f64);
                Some(f64::from_bits(JSValue::pointer(promise as *mut u8).bits()))
            }
            "clone" => Some(js_request_clone(req_f64)),
            _ => None,
        }
    }
}

/// Try to read a property off a Response handle by registry id.
/// Returns `None` if the id isn't a known Response or the property is unknown.
#[doc(hidden)]
pub fn dispatch_response_property(resp_id: usize, prop: &str) -> Option<f64> {
    // `response.headers` — lazily allocate a Headers registry entry
    // backed by the response's stored headers and cache the id on the
    // FetchResponse so repeat reads return the same handle (preserves
    // `res.headers === res.headers`). Hono's `#newResponse` mutates the
    // returned Headers object via `.set(k, v)`, but our snapshot is a
    // copy of the response's HeadersStore — mutations land on the
    // Headers handle's HeadersStore, not back on the FetchResponse.
    // For the read-only case (the issue #486 acceptance) this is
    // sufficient; spec-perfect "live header view" would need the
    // FetchResponse's storage to be the same Vec as the Headers
    // entries, which is a wider refactor.
    if prop == "headers" {
        let cached = {
            let guard = FETCH_RESPONSES.lock().unwrap();
            guard.get(&resp_id)?.cached_headers_id
        };
        let id = match cached {
            Some(id) => id,
            None => {
                let store = {
                    let guard = FETCH_RESPONSES.lock().unwrap();
                    guard.get(&resp_id)?.headers.clone()
                };
                let new_id = alloc_headers(store);
                if let Some(resp) = FETCH_RESPONSES.lock().unwrap().get_mut(&resp_id) {
                    resp.cached_headers_id = Some(new_id);
                }
                new_id
            }
        };
        return Some(handle_to_f64(id));
    }
    // `response.body` — `ReadableStream | null` per the Web Fetch spec.
    // Returns a NaN-boxed (POINTER_TAG) single-chunk ReadableStream handle
    // over the buffered body so `typeof res.body === 'object'` and
    // `res.body.getReader()` route through the untyped stream dispatch
    // (see dispatch_readable_stream_method). `null` when the response was
    // constructed with no body. Cached so `.body` is stable across reads
    // (the spec mandates a single stream; a fresh stream each call would
    // silently unlock a held reader) (#1650).
    if prop == "body" {
        // Membership check first so a non-Response id falls through.
        FETCH_RESPONSES.lock().unwrap().get(&resp_id)?;
        return Some(response_body_stream(resp_id));
    }
    if matches!(
        prop,
        "text" | "json" | "arrayBuffer" | "blob" | "bytes" | "formData" | "clone"
    ) {
        FETCH_RESPONSES.lock().unwrap().get(&resp_id)?;
        extern "C" {
            fn js_class_method_bind(
                instance: f64,
                method_name_ptr: *const u8,
                method_name_len: usize,
            ) -> f64;
        }
        let name: &'static [u8] = match prop {
            "text" => b"text",
            "json" => b"json",
            "arrayBuffer" => b"arrayBuffer",
            "blob" => b"blob",
            "bytes" => b"bytes",
            "formData" => b"formData",
            "clone" => b"clone",
            _ => unreachable!(),
        };
        return Some(unsafe {
            js_class_method_bind(handle_to_f64(resp_id), name.as_ptr(), name.len())
        });
    }
    let guard = FETCH_RESPONSES.lock().unwrap();
    let resp = guard.get(&resp_id)?;
    let bits = match prop {
        "status" => return Some(resp.status as f64),
        "statusText" => {
            let p = js_string_from_bytes(resp.status_text.as_ptr(), resp.status_text.len() as u32);
            JSValue::string_ptr(p).bits()
        }
        "type" => {
            let p = js_string_from_bytes(resp.type_name.as_ptr(), resp.type_name.len() as u32);
            JSValue::string_ptr(p).bits()
        }
        "url" => {
            let p = js_string_from_bytes(resp.url.as_ptr(), resp.url.len() as u32);
            JSValue::string_ptr(p).bits()
        }
        "ok" => {
            return Some(f64::from_bits(if resp.status >= 200 && resp.status < 300 {
                TAG_TRUE
            } else {
                TAG_FALSE
            }))
        }
        "bodyUsed" => return Some(tagged_bool(resp.body_used)),
        "redirected" => return Some(tagged_bool(resp.redirected)),
        _ => return None,
    };
    Some(f64::from_bits(bits))
}

/// Try to read a property off a Headers handle by registry id.
/// Headers exposes prototype methods as function-valued properties, so method
/// feature checks such as `typeof headers.getSetCookie === "function"` work
/// while call sites still route through `HANDLE_METHOD_DISPATCH`.
#[doc(hidden)]
pub fn dispatch_headers_property(headers_id: usize, prop: &str) -> Option<f64> {
    {
        let guard = HEADERS_REGISTRY.lock().unwrap();
        guard.get(&headers_id)?;
    }
    let method_name: &'static str = match prop {
        "append" => "append",
        "delete" => "delete",
        // WHATWG aliases `Headers.prototype[Symbol.iterator]` to `entries`.
        // The property value must be identical to `headers.entries`, so both
        // names return the same cached bound-method closure.
        "entries" | "Symbol.iterator" | "@@iterator" => "entries",
        "forEach" => "forEach",
        "get" => "get",
        "getSetCookie" => "getSetCookie",
        "has" => "has",
        "keys" => "keys",
        "set" => "set",
        "values" => "values",
        _ => return None,
    };
    Some(headers_bound_method_value(headers_id, method_name))
}

/// Try to read a property off a FormData handle by registry id. FormData
/// exposes prototype methods as function-valued properties, so any-typed
/// feature checks such as `typeof form.append === "function"` work.
#[doc(hidden)]
pub fn dispatch_form_data_property(form_id: usize, prop: &str) -> Option<f64> {
    if !form_data_contains_handle(form_id) {
        return None;
    }
    let method_name: &'static str = match prop {
        "append" => "append",
        "delete" => "delete",
        "entries" | "Symbol.iterator" | "@@iterator" => "entries",
        "forEach" => "forEach",
        "get" => "get",
        "getAll" => "getAll",
        "has" => "has",
        "keys" => "keys",
        "set" => "set",
        "values" => "values",
        _ => return None,
    };
    Some(form_data_bound_method_value(form_id, method_name))
}

/// Try to dispatch a method call on a Response handle. Returns `Some(result)`
/// only when the id is a known Response AND the method is supported. Returns
/// `None` (fall through to the next dispatcher) otherwise. Required because
/// Web Fetch handle id namespaces are disjoint from each other and from other
/// stdlib registries — id collision (Request id 1 ≠ Response id 1) means
/// claiming membership alone would shadow legitimate calls on other handles.
#[doc(hidden)]
pub fn dispatch_response_method(resp_id: usize, method: &str, _args: &[f64]) -> Option<f64> {
    {
        let guard = FETCH_RESPONSES.lock().unwrap();
        guard.get(&resp_id)?;
    }
    let resp_f64 = handle_to_f64(resp_id);
    unsafe {
        match method {
            "text" => {
                let promise = js_fetch_response_text(resp_f64);
                Some(f64::from_bits(JSValue::pointer(promise as *mut u8).bits()))
            }
            "json" => {
                let promise = js_fetch_response_json(resp_f64);
                Some(f64::from_bits(JSValue::pointer(promise as *mut u8).bits()))
            }
            "arrayBuffer" => {
                let promise = js_response_array_buffer(resp_f64);
                Some(f64::from_bits(JSValue::pointer(promise as *mut u8).bits()))
            }
            "blob" => {
                let promise = js_response_blob(resp_f64);
                Some(f64::from_bits(JSValue::pointer(promise as *mut u8).bits()))
            }
            "bytes" => {
                let promise = js_response_bytes(resp_f64);
                Some(f64::from_bits(JSValue::pointer(promise as *mut u8).bits()))
            }
            "formData" => {
                let promise = js_response_form_data(resp_f64);
                Some(f64::from_bits(JSValue::pointer(promise as *mut u8).bits()))
            }
            "clone" => Some(js_response_clone(resp_f64)),
            _ => None,
        }
    }
}

/// Try to dispatch a method call on a FormData handle. Returns `None` for
/// unknown ids or methods.
#[doc(hidden)]
pub fn dispatch_form_data_method(form_id: usize, method: &str, args: &[f64]) -> Option<f64> {
    if !form_data_contains_handle(form_id) {
        return None;
    }
    let form_f64 = handle_to_f64(form_id);
    let str_arg = |i: usize| -> *const StringHeader {
        if i < args.len() {
            js_get_string_pointer_unified(args[i]) as *const StringHeader
        } else {
            std::ptr::null()
        }
    };
    unsafe {
        match method {
            "append" => Some(js_form_data_append(
                form_f64,
                args.first()
                    .copied()
                    .unwrap_or(f64::from_bits(TAG_UNDEFINED)),
                args.get(1)
                    .copied()
                    .unwrap_or(f64::from_bits(TAG_UNDEFINED)),
            )),
            "set" => Some(js_form_data_set(
                form_f64,
                args.first()
                    .copied()
                    .unwrap_or(f64::from_bits(TAG_UNDEFINED)),
                args.get(1)
                    .copied()
                    .unwrap_or(f64::from_bits(TAG_UNDEFINED)),
            )),
            "delete" => Some(js_form_data_delete(form_f64, str_arg(0))),
            "get" => Some(js_form_data_get(form_f64, str_arg(0))),
            "getAll" => Some(js_form_data_get_all(form_f64, str_arg(0))),
            "has" => Some(js_form_data_has(form_f64, str_arg(0))),
            "entries" | "Symbol.iterator" | "@@iterator" => Some(js_form_data_entries(form_f64)),
            "keys" => Some(js_form_data_keys(form_f64)),
            "values" => Some(js_form_data_values(form_f64)),
            "forEach" => {
                let cb = args
                    .first()
                    .copied()
                    .unwrap_or(f64::from_bits(TAG_UNDEFINED));
                Some(js_form_data_for_each(form_f64, cb))
            }
            _ => None,
        }
    }
}

/// Try to dispatch a method call on a Blob handle. Returns `None` for unknown
/// ids or methods.
#[doc(hidden)]
pub fn dispatch_blob_method(blob_id: usize, method: &str, args: &[f64]) -> Option<f64> {
    {
        let guard = BLOB_REGISTRY.lock().unwrap();
        guard.get(&blob_id)?;
    }
    let blob_f64 = handle_to_f64(blob_id);
    unsafe {
        match method {
            "text" => {
                let promise = js_blob_text(blob_f64);
                Some(f64::from_bits(JSValue::pointer(promise as *mut u8).bits()))
            }
            "arrayBuffer" => {
                let promise = js_blob_array_buffer(blob_f64);
                Some(f64::from_bits(JSValue::pointer(promise as *mut u8).bits()))
            }
            "bytes" => {
                let promise = js_blob_bytes(blob_f64);
                Some(f64::from_bits(JSValue::pointer(promise as *mut u8).bits()))
            }
            "slice" => {
                let start = args.first().copied().unwrap_or(f64::NAN);
                let end = args.get(1).copied().unwrap_or(f64::NAN);
                Some(js_blob_slice(blob_f64, start, end, std::ptr::null()))
            }
            _ => None,
        }
    }
}

/// Try to dispatch a method call on a Headers handle. Returns `None` for
/// unknown ids or methods.
#[doc(hidden)]
pub fn dispatch_headers_method(headers_id: usize, method: &str, args: &[f64]) -> Option<f64> {
    {
        let guard = HEADERS_REGISTRY.lock().unwrap();
        guard.get(&headers_id)?;
    }
    let h_f64 = handle_to_f64(headers_id);
    // String args can arrive as heap strings or SSO/short-string values; use
    // the runtime's unified coercion helper before passing a StringHeader to
    // the FFI helpers.
    let str_arg = |i: usize| -> *const StringHeader {
        if i < args.len() {
            js_get_string_pointer_unified(args[i]) as *const StringHeader
        } else {
            std::ptr::null()
        }
    };
    unsafe {
        match method {
            // WHATWG `Headers.get` returns `null` for an absent header, not the
            // empty string. `js_headers_get` signals absence with a null
            // `StringHeader` pointer; wrapping that in `string_ptr` would render
            // as `""` and break `headers.get(x) === null` checks (the Hono
            // adapter path behind #4004).
            "get" => {
                let p = js_headers_get(h_f64, str_arg(0));
                if p.is_null() {
                    Some(f64::from_bits(TAG_NULL))
                } else {
                    Some(f64::from_bits(JSValue::string_ptr(p).bits()))
                }
            }
            "set" => Some(js_headers_set(h_f64, str_arg(0), str_arg(1))),
            "append" => Some(js_headers_append(h_f64, str_arg(0), str_arg(1))),
            "has" => Some(js_headers_has(h_f64, str_arg(0))),
            "delete" => Some(js_headers_delete(h_f64, str_arg(0))),
            "getSetCookie" => Some(js_headers_get_set_cookie(h_f64)),
            "forEach" => {
                let cb = args.first().copied().unwrap_or(f64::NAN);
                Some(js_headers_for_each(h_f64, cb))
            }
            // The iterator helpers each return a (sorted) JS array, which is
            // itself iterable — `for (const [k,v] of req.headers.entries())`
            // and `[...req.headers.keys()]` both work off the untyped path
            // Hono's type-stripped JS takes (#1649).
            "keys" => Some(js_headers_keys(h_f64)),
            "values" => Some(js_headers_values(h_f64)),
            "entries" | "Symbol.iterator" | "@@iterator" => Some(js_headers_entries(h_f64)),
            _ => None,
        }
    }
}

/// Try to read a property off a Blob handle by registry id.
/// Returns `None` if the id isn't a known Blob or the property is unknown.
#[doc(hidden)]
pub fn dispatch_blob_property(blob_id: usize, prop: &str) -> Option<f64> {
    let guard = BLOB_REGISTRY.lock().unwrap();
    let blob = guard.get(&blob_id)?;
    if matches!(prop, "text" | "arrayBuffer" | "bytes" | "slice") {
        extern "C" {
            fn js_class_method_bind(
                instance: f64,
                method_name_ptr: *const u8,
                method_name_len: usize,
            ) -> f64;
        }
        let name: &'static [u8] = match prop {
            "text" => b"text",
            "arrayBuffer" => b"arrayBuffer",
            "bytes" => b"bytes",
            "slice" => b"slice",
            _ => unreachable!(),
        };
        return Some(unsafe {
            js_class_method_bind(handle_to_f64(blob_id), name.as_ptr(), name.len())
        });
    }
    let bits = match prop {
        "size" => return Some(blob.body.len() as f64),
        "type" => {
            let p =
                js_string_from_bytes(blob.content_type.as_ptr(), blob.content_type.len() as u32);
            JSValue::string_ptr(p).bits()
        }
        "name" => {
            let name = blob.file_name.as_ref()?;
            let p = js_string_from_bytes(name.as_ptr(), name.len() as u32);
            JSValue::string_ptr(p).bits()
        }
        "lastModified" => return blob.last_modified_ms,
        _ => return None,
    };
    Some(f64::from_bits(bits))
}
