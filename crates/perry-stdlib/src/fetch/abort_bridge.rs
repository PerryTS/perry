//! Bridges a JS `AbortSignal` to an in-flight `fetch` so the request rejects
//! when the signal aborts — either `controller.abort()` or an
//! `AbortSignal.timeout(ms)` deadline elapsing.
//!
//! The signal reaches the native fetch via the runtime's pending-signal stash
//! (`js_fetch_set_pending_signal` / `js_fetch_take_pending_signal`), which keeps
//! the 4-arg `js_fetch_with_options` ABI unchanged. On the main thread (at the
//! start of `js_fetch_with_options`) we register a per-request
//! `tokio::sync::Notify` keyed to the signal; the request future `select!`s its
//! send/receive against that notify and, when the abort wins, rejects the fetch
//! promise with a fresh `AbortError`.
//!
//! The signal's abort reaches us through the runtime: `fire_abort_listeners`
//! (run by `controller.abort()` and the `AbortSignal.timeout` deadline) calls
//! `js_fetch_notify_signal_aborted`, which wakes the registered notifies. There
//! is deliberately no per-fetch JS `abort` listener, so reused signals never
//! accumulate stale listener closures.
//!
//! Threading: the spawned future only awaits the `Notify` (Send/Sync) and
//! rejects through `queue_deferred_resolution`, whose converter creates the
//! error on the main thread — it never touches the JS heap from a worker. Refs
//! the AbortSignal runtime in `perry_runtime::url::abort`.

use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

use crate::common::async_bridge::{queue_deferred_resolution, queue_promise_resolution};

unsafe extern "C" {
    // `js_fetch_take_pending_signal` lives in perry-runtime's private
    // `object::global_fetch` module, so it is reached as a `#[no_mangle]`
    // extern rather than by Rust path.
    fn js_fetch_take_pending_signal() -> f64;
}

/// AbortSignal object ptr (as `usize`) → the `Notify`s of every in-flight fetch
/// bound to that signal. One signal can bound several concurrent requests, so
/// the value is a list.
static FETCH_ABORT_WATCHERS: Lazy<Mutex<HashMap<usize, Vec<Arc<Notify>>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn watch(signal_ptr: usize) -> Arc<Notify> {
    let notify = Arc::new(Notify::new());
    FETCH_ABORT_WATCHERS
        .lock()
        .unwrap()
        .entry(signal_ptr)
        .or_default()
        .push(notify.clone());
    notify
}

fn unwatch(signal_ptr: usize, notify: &Arc<Notify>) {
    let mut map = FETCH_ABORT_WATCHERS.lock().unwrap();
    if let Some(list) = map.get_mut(&signal_ptr) {
        list.retain(|n| !Arc::ptr_eq(n, notify));
        if list.is_empty() {
            map.remove(&signal_ptr);
        }
    }
}

/// Wake every in-flight fetch bound to `signal_ptr`. Called on the main thread
/// by the runtime's `fire_abort_listeners` (`perry_runtime::url::abort`) when
/// the signal aborts — `controller.abort()` or an `AbortSignal.timeout`
/// deadline. A registry miss (no in-flight fetch for the signal) is a no-op.
#[no_mangle]
pub extern "C" fn js_fetch_notify_signal_aborted(signal_ptr: i64) {
    let key = signal_ptr as usize;
    let notifies: Vec<Arc<Notify>> = FETCH_ABORT_WATCHERS
        .lock()
        .unwrap()
        .get(&key)
        .cloned()
        .unwrap_or_default();
    for notify in notifies {
        // `notify_one` stores a permit if the request future is not yet parked
        // on `notified()`, so an abort that races the spawn is not lost.
        notify.notify_one();
    }
}

/// A live abort watch for one request: the `Notify` the request future selects
/// on, plus the signal key used to deregister on completion.
pub(crate) struct FetchAbortWatch {
    notify: Arc<Notify>,
    signal_ptr: usize,
}

impl FetchAbortWatch {
    /// Resolves when the bound signal aborts.
    pub(crate) async fn aborted(&self) {
        self.notify.notified().await;
    }
}

impl Drop for FetchAbortWatch {
    fn drop(&mut self) {
        unwatch(self.signal_ptr, &self.notify);
    }
}

/// Outcome of consuming the pending fetch signal on the main thread.
pub(crate) enum SignalState {
    /// No signal, or the value was not an AbortSignal — dispatch normally.
    None,
    /// The signal was already aborted before the request started — the caller
    /// should reject immediately without dispatching.
    AlreadyAborted,
    /// A live watch the request future must race against.
    Watch(FetchAbortWatch),
}

/// Consume the runtime's pending fetch signal. If it is a live (not yet
/// aborted) AbortSignal, register a `Notify` keyed to it for the request future
/// to race against; an already-aborted signal short-circuits to a reject. MUST
/// run on the main thread (it reads the signal object).
pub(crate) fn take_pending_signal_watch() -> SignalState {
    let signal_value = unsafe { js_fetch_take_pending_signal() };
    let signal = perry_runtime::url::js_abort_signal_resolve_ptr(signal_value);
    if signal.is_null() {
        return SignalState::None;
    }
    if perry_runtime::url::js_abort_signal_is_aborted(signal) != 0 {
        return SignalState::AlreadyAborted;
    }
    let signal_ptr = signal as usize;
    SignalState::Watch(FetchAbortWatch {
        notify: watch(signal_ptr),
        signal_ptr,
    })
}

/// Resolve a freshly-taken `SignalState` against the request's promise: reject
/// up front when the signal was already aborted (returns `None`, telling the
/// caller to return the promise immediately), otherwise yield the optional watch
/// the request should race against.
pub(crate) fn watch_or_reject(
    state: SignalState,
    promise_ptr: usize,
) -> Option<Option<FetchAbortWatch>> {
    match state {
        SignalState::AlreadyAborted => {
            queue_deferred_resolution(promise_ptr, false, abort_error_bits);
            None
        }
        SignalState::Watch(watch) => Some(Some(watch)),
        SignalState::None => Some(None),
    }
}

/// Run `request_future`, racing it against the bound AbortSignal. If the signal
/// aborts first, the request future is dropped (cancelling the in-flight reqwest
/// request) and the promise rejects with an `AbortError`. The `watch`
/// deregisters when dropped at the end of this scope.
pub(crate) async fn race_request<F: std::future::Future<Output = ()>>(
    promise_ptr: usize,
    abort_watch: Option<FetchAbortWatch>,
    request_future: F,
) {
    match abort_watch {
        Some(watch) => {
            tokio::select! {
                _ = request_future => {}
                _ = watch.aborted() => {
                    queue_deferred_resolution(promise_ptr, false, abort_error_bits);
                }
            }
        }
        None => request_future.await,
    }
}

/// Dispatch the `js_fetch_with_options` HTTP request, racing it against the
/// bound AbortSignal. Lives here (rather than inline in `fetch::mod`) to keep
/// that file under the line-size lint gate and to keep the abort orchestration
/// in one place; it reaches the fetch internals through the parent module.
pub(crate) async fn run_request(
    promise_ptr: usize,
    abort_watch: Option<FetchAbortWatch>,
    inputs: super::request_handle::FetchInputs,
) {
    let super::request_handle::FetchInputs {
        url,
        method,
        body,
        custom_headers,
    } = inputs;
    let request_future = async move {
        let client = super::HTTP_CLIENT.clone();
        let mut request = match method.to_uppercase().as_str() {
            "POST" => client.post(&url),
            "PUT" => client.put(&url),
            "DELETE" => client.delete(&url),
            "PATCH" => client.patch(&url),
            "HEAD" => client.head(&url),
            _ => client.get(&url), // Default to GET
        };
        for (key, value) in &custom_headers {
            request = request.header(key.as_str(), value.as_str());
        }
        if let Some(b) = body {
            request = request.body(b);
        }
        match request.send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                let status_text = response
                    .status()
                    .canonical_reason()
                    .unwrap_or("")
                    .to_string();
                let headers = super::headers_from_header_map(response.headers());
                let body = response.bytes().await.unwrap_or_default().to_vec();
                let response_id = super::alloc_fetch_handle_id();
                super::FETCH_RESPONSES.lock().unwrap().insert(
                    response_id,
                    super::FetchResponse {
                        status,
                        status_text,
                        headers,
                        body,
                        body_present: true,
                        body_used: false,
                        type_name: "basic".to_string(),
                        url: url.clone(),
                        redirected: false,
                        cached_headers_id: None,
                        cached_body_stream_id: None,
                    },
                );
                let result_bits = super::handle_to_f64(response_id).to_bits();
                queue_promise_resolution(promise_ptr, true, result_bits);
            }
            Err(e) => {
                let err_msg = format!("Fetch error: {}", e);
                // SAFETY: `fetch_error_bits` allocates an Error JSValue; this is
                // the worker-side error path it replaces verbatim.
                let err_bits = unsafe { super::fetch_error_bits(&err_msg) };
                queue_promise_resolution(promise_ptr, false, err_bits);
            }
        }
    };
    race_request(promise_ptr, abort_watch, request_future).await;
}

/// NaN-boxed bits of a fresh `AbortError` for rejecting an aborted fetch. Built
/// inside a `queue_deferred_resolution` converter so the error object is
/// allocated on the main thread (never on a worker).
pub(crate) fn abort_error_bits() -> u64 {
    perry_runtime::url::js_abort_error_value().to_bits()
}
