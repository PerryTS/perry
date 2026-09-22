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
//! * **No loop for this agent at all** — the only remaining case is a host
//!   where `Loop::new` failed. A *worker* agent is no longer one of these
//!   (turnloop P9 gave every agent a loop), and neither is a second
//!   thread acting for an agent another thread owns: turnloop P10 hands that
//!   thread's whole submission to the owner (`turnloop_client::posted`).
//! * **A proxy this client cannot drive** — a proxy URL whose scheme is not
//!   `http` (socks5, https-to-proxy), or one that will not parse. An ordinary
//!   `http://` proxy is no longer a decline: `HTTP_PROXY`/`HTTPS_PROXY` and the
//!   process-wide `undici.setGlobalDispatcher(new ProxyAgent(…))` are both read
//!   as a URL now, and the engine runs the CONNECT tunnel itself.
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

/// The `js_fetch_stream_start` form: Perry's line-oriented SSE poll surface.
///
/// This is the only caller of the engine's `Sink::on_head` / `Sink::on_chunk`
/// hooks. They were added by P6 and left unused, which by CLAUDE.md's
/// kill-policy made them an unexercised mode — a green engine test said nothing
/// about them. `ctx` is the stream id, not a promise: this surface resolves
/// nothing and is polled from JS instead.
///
/// `false` means the engine declined and the caller must run its reqwest task.
pub(crate) fn try_dispatch_stream(
    stream_id: usize,
    url: String,
    method: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
) -> bool {
    let spec = RequestSpec {
        url,
        method,
        headers,
        body,
        redirect: turnloop_http::client::RedirectMode::Follow,
        abort_key: None,
    };
    let sink = Sink {
        ctx: stream_id,
        on_head: Some(stream_head),
        on_chunk: Some(stream_chunk),
        on_done: stream_done,
    };
    match turnloop_client::submit(spec, sink) {
        Ok(()) => true,
        Err(_) => {
            turnloop_client::note_declined();
            false
        }
    }
}

/// The FINAL response's head — the engine never reports a followed redirect's,
/// which matches what `reqwest::Response::status()` reported here.
fn stream_head(ctx: usize, status: u16, _headers: &[(String, String)]) {
    super::with_stream(ctx, |state| {
        state.http_status = status;
        state.status = 1;
    });
}

fn stream_chunk(ctx: usize, bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes).to_string();
    super::with_stream(ctx, |state| state.push_text(&text));
}

fn stream_done(ctx: usize, outcome: Outcome) {
    match outcome {
        // A streaming sink's `on_done` carries an empty body; every byte
        // already went through `stream_chunk`.
        Outcome::Ok(_) => super::with_stream(ctx, |state| state.finish()),
        Outcome::Err(error) => super::with_stream(ctx, |state| {
            // The two message prefixes the reqwest path used, kept: a failure
            // before the head is a connection error, one after it a stream
            // error.
            state.error = if state.status >= 1 {
                format!("Stream error: {}", error.message)
            } else {
                format!("Connection error: {}", error.message)
            };
            state.status = 3;
        }),
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
