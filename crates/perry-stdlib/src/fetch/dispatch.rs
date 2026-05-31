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
    // String args arrive as NaN-boxed STRING_TAG f64 values; extract the raw
    // StringHeader pointer for the runtime helpers.
    let str_arg = |i: usize| -> *const StringHeader {
        if i < args.len() {
            let v = args[i];
            (v.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader
        } else {
            std::ptr::null()
        }
    };
    unsafe {
        match method {
            "get" => Some(f64::from_bits(
                JSValue::string_ptr(js_headers_get(h_f64, str_arg(0))).bits(),
            )),
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
    let bits = match prop {
        "size" => return Some(blob.body.len() as f64),
        "type" => {
            let p =
                js_string_from_bytes(blob.content_type.as_ptr(), blob.content_type.len() as u32);
            JSValue::string_ptr(p).bits()
        }
        _ => return None,
    };
    Some(f64::from_bits(bits))
}
