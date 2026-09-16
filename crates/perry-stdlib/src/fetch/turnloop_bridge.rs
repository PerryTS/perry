//! Routes `fetch` onto the turnloop client engine, and back.
//!
//! Every transport-bearing `js_fetch_*` entry point calls [`try_dispatch`]
//! first. `true` means the engine accepted the request and will settle the
//! promise exactly once; `false` means it declined and the caller must run its
//! existing reqwest future — the same coexistence rule P1 applied to the tokio
//! socket task, and the reason `reqwest` is not removed from this crate.
//!
//! # What declines, and why each is real
//!
//! * **A worker agent**, which has no `turnloop::Loop` of its own yet (P3/P4
//!   left per-agent loops to a later phase), and the `tokio-wait-driver` A/B
//!   arm, where there is no loop at all.
//! * **A proxy** — either `HTTP_PROXY`/`HTTPS_PROXY` from the environment or
//!   the process-wide dispatcher `undici.setGlobalDispatcher(new ProxyAgent(…))`
//!   installs. `turnloop_http::client::Route` models the CONNECT tunnel, but
//!   Perry's proxy surface is a prebuilt `reqwest::Client` and moving it is its
//!   own change.
//! * **A URL `turnloop_http::client::Request::new` rejects** (a non-http(s)
//!   scheme, embedded credentials, a forbidden method). Declining rather than
//!   failing keeps the existing error text, which the suite pins.
//!
//! # GC
//!
//! `ctx` is the promise address from `js_promise_new_cross_thread`, which pins
//! the promise across the crossing (#9552) exactly as the reqwest path relied
//! on. Nothing else about a request is a JS value: url, method, headers and body
//! are owned Rust data copied out on this thread before submission, and the
//! response handle is built here, on the owning thread, inside the deferred
//! resolution — never in the sink.

use crate::common::async_bridge::{queue_deferred_resolution, queue_promise_resolution};
use crate::turnloop_client::{
    self, ClientError, Declined, Outcome, RequestSpec, ResponseOut, Sink,
};

use super::{
    alloc_fetch_handle_id, handle_to_f64, transport_error::FetchFailure, FetchResponse,
    FETCH_RESPONSES,
};

/// One fetch, as the entry points describe it.
pub(crate) struct FetchDispatch {
    pub(crate) url: String,
    pub(crate) method: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Option<Vec<u8>>,
    pub(crate) abort_key: Option<usize>,
}

/// The `js_fetch_with_options` form: the same dispatch built from the resolved
/// `FetchInputs`. On decline the inputs come back so the caller can run its
/// reqwest future without rebuilding them.
pub(crate) fn try_dispatch_inputs(
    inputs: super::request_handle::FetchInputs,
    abort_key: Option<usize>,
    promise_ptr: usize,
) -> Result<(), super::request_handle::FetchInputs> {
    let dispatch = FetchDispatch {
        url: inputs.url.clone(),
        method: inputs.method.clone(),
        headers: inputs
            .custom_headers
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect(),
        body: inputs.body.clone(),
        abort_key,
    };
    if try_dispatch(dispatch, promise_ptr) {
        Ok(())
    } else {
        Err(inputs)
    }
}

/// The `js_fetch_text` form, which resolves with the decoded body text rather
/// than a `Response` handle.
pub(crate) fn try_dispatch_text(url: String, promise_ptr: usize) -> bool {
    if super::global_proxy_installed() {
        turnloop_client::note_declined();
        return false;
    }
    let spec = RequestSpec {
        url,
        method: "GET".to_string(),
        headers: Vec::new(),
        body: None,
        redirect: turnloop_http::client::RedirectMode::Follow,
        abort_key: None,
    };
    let sink = Sink {
        ctx: promise_ptr,
        on_head: None,
        on_chunk: None,
        on_done: settle_text,
    };
    match turnloop_client::submit(spec, sink) {
        Ok(()) => true,
        Err(_) => {
            turnloop_client::note_declined();
            false
        }
    }
}

fn settle_text(ctx: usize, outcome: Outcome) {
    match outcome {
        Outcome::Ok(response) => {
            let body = response.body;
            queue_deferred_resolution(ctx, true, move || {
                let text = perry_runtime::js_string_from_bytes(body.as_ptr(), body.len() as u32);
                perry_runtime::JSValue::pointer(text as *const u8).bits()
            });
        }
        Outcome::Err(error) if error.aborted => {
            queue_deferred_resolution(ctx, false, super::abort_bridge::abort_error_bits);
        }
        Outcome::Err(error) => {
            let message = format!("Fetch error: {}", error.message);
            queue_deferred_resolution(ctx, false, move || unsafe {
                super::fetch_error_bits(&message)
            });
        }
    }
}

/// Try the turnloop path. `false` means the caller keeps its reqwest future.
pub(crate) fn try_dispatch(dispatch: FetchDispatch, promise_ptr: usize) -> bool {
    if super::global_proxy_installed() {
        turnloop_client::note_declined();
        return false;
    }
    let spec = RequestSpec {
        url: dispatch.url,
        method: dispatch.method,
        headers: dispatch.headers,
        body: dispatch.body,
        // Perry's fetch has always followed redirects (reqwest's default
        // policy). `RedirectMode::Follow` with turnloop-http's own limit of 20
        // is Node's number; reqwest's was 10.
        redirect: turnloop_http::client::RedirectMode::Follow,
        abort_key: dispatch.abort_key,
    };
    let sink = Sink {
        ctx: promise_ptr,
        on_head: None,
        on_chunk: None,
        on_done: settle,
    };
    match turnloop_client::submit(spec, sink) {
        Ok(()) => true,
        Err(Declined::NoLoop | Declined::Proxy | Declined::Unsupported | Declined::NoTls) => {
            turnloop_client::note_declined();
            false
        }
    }
}

/// The engine's completion. Runs on the owning thread from `drain_pending`,
/// after the dispatch has finished with the engine's tables, so it may touch
/// the fetch registries — but it still settles the promise through the deferred
/// queue rather than running JS itself.
fn settle(ctx: usize, outcome: Outcome) {
    match outcome {
        Outcome::Ok(response) => {
            let handle = store(*response);
            queue_promise_resolution(ctx, true, handle_to_f64(handle).to_bits());
        }
        Outcome::Err(error) if error.aborted => {
            queue_deferred_resolution(ctx, false, super::abort_bridge::abort_error_bits);
        }
        Outcome::Err(error) => {
            let failure = failure_for(error);
            queue_deferred_resolution(ctx, false, move || failure.into_js_bits());
        }
    }
}

fn store(response: ResponseOut) -> usize {
    let mut headers = super::HeadersStore::default();
    for (name, value) in response.headers {
        headers.append(&name, &value);
    }
    let id = alloc_fetch_handle_id();
    FETCH_RESPONSES.lock().unwrap().insert(
        id,
        FetchResponse {
            status: response.status,
            status_text: response.status_text,
            headers,
            body: response.body,
            body_present: true,
            body_used: false,
            type_name: "basic".to_string(),
            url: response.final_url,
            redirected: response.redirected,
            cached_headers_id: None,
            cached_body_stream_id: None,
            body_stream_id: None,
        },
    );
    id
}

fn failure_for(error: ClientError) -> FetchFailure {
    FetchFailure::from_client(error.code, error.message, error.syscall)
}
