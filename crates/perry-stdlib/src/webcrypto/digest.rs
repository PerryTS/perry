use super::*;

/// Native closure body: resolve the captured Promise (slot 0) with the
/// captured digest value (slot 1). Scheduled via `setImmediate` so
/// `crypto.subtle.digest()` settles on a later event-loop iteration, matching
/// Node — whose WebCrypto digest runs on the libuv threadpool (a macrotask),
/// not synchronously. Resolving it synchronously let an `await`ing caller
/// continue an event-loop iteration early: Auth.js hashes its CSRF token with
/// `subtle.digest`, so a Next.js Server Component's `await auth()` completed
/// ahead of Node and reordered the React Flight (RSC) rows of the response.
extern "C" fn webcrypto_digest_settle(closure: *const perry_runtime::ClosureHeader) -> f64 {
    let promise_bits = perry_runtime::closure::js_closure_get_capture_ptr(closure, 0) as u64;
    let value_bits = perry_runtime::closure::js_closure_get_capture_ptr(closure, 1) as u64;
    // Slot 2 holds the remaining macrotask hops (raw i64). Node's threadpool
    // WebCrypto digest observably yields TWO setImmediate iterations (submit +
    // completion), so re-arm one more time before resolving.
    let remaining = perry_runtime::closure::js_closure_get_capture_ptr(closure, 2);
    if remaining > 1 {
        let cl = perry_runtime::closure::js_closure_alloc(webcrypto_digest_settle as *const u8, 3);
        perry_runtime::closure::js_closure_set_capture_ptr(cl, 0, promise_bits as i64);
        perry_runtime::closure::js_closure_set_capture_ptr(cl, 1, value_bits as i64);
        perry_runtime::closure::js_closure_set_capture_ptr(cl, 2, remaining - 1);
        perry_runtime::timer::js_set_immediate_callback(cl as i64);
        return f64::from_bits(JSValue::undefined().bits());
    }
    let promise = perry_runtime::value::js_nanbox_get_pointer(f64::from_bits(promise_bits))
        as *mut perry_runtime::promise::Promise;
    if !promise.is_null() {
        perry_runtime::promise::js_promise_resolve(promise, f64::from_bits(value_bits));
    }
    f64::from_bits(JSValue::undefined().bits())
}

/// `crypto.subtle.digest(algorithm, data)` → Promise<Uint8Array>
///
/// `algorithm` is "SHA-1" / "SHA-256" / "SHA-384" / "SHA-512" (string)
/// or `{ name: "SHA-256" }`. Unknown algorithms reject with a TypeError.
#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_digest(algo_bits: f64, data_bits: f64) -> *mut Promise {
    let algo = match extract_hash_algo(algo_bits.to_bits()) {
        Some(a) => a,
        None => {
            return reject_with_dom_exception("NotSupportedError", "Unrecognized algorithm name")
        }
    };
    let bytes = bytes_from_jsvalue(data_bits.to_bits());
    let digest = compute_digest(algo, &bytes);
    // The digest bytes are computed eagerly, but the Promise settles on the
    // next event-loop iteration (setImmediate) — Node runs the hash on the
    // threadpool, so `await subtle.digest(...)` observably yields a macrotask.
    let buf = alloc_uint8array_from_slice(&digest);
    if buf.is_null() {
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    let value = f64::from_bits(JSValue::pointer(buf as *const u8).bits());
    let promise = perry_runtime::promise::js_promise_new();
    let promise_val = f64::from_bits(JSValue::pointer(promise as *const u8).bits());
    let cl = perry_runtime::closure::js_closure_alloc(webcrypto_digest_settle as *const u8, 3);
    perry_runtime::closure::js_closure_set_capture_ptr(cl, 0, promise_val.to_bits() as i64);
    perry_runtime::closure::js_closure_set_capture_ptr(cl, 1, value.to_bits() as i64);
    // Remaining macrotask hops (Node's threadpool digest = 2 setImmediate ticks).
    perry_runtime::closure::js_closure_set_capture_ptr(cl, 2, 2);
    perry_runtime::timer::js_set_immediate_callback(cl as i64);
    promise
}
