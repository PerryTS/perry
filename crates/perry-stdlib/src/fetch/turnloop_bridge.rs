//! Routes `fetch` onto the turnloop client engine, and back.
//!
//! Every transport-bearing `js_fetch_*` entry point hands its request to
//! [`dispatch`] (or a sibling). The engine is the only transport: the reqwest
//! future that used to run when it declined is gone, and with it
//! `perry-stdlib`'s `reqwest` dependency. A request the engine refuses to build
//! settles the caller's promise with Node's rejection for the same input
//! (`transport_error::Rejection`).
//!
//! # What is refused, and why each is real
//!
//! * **No loop for this agent at all** — the only remaining case is a host
//!   where `Loop::new` failed. A *worker* agent is not one of these (turnloop
//!   P9 gave every agent a loop), and neither is a second thread acting for an
//!   agent another thread owns: turnloop P10 hands that thread's whole
//!   submission to the owner (`turnloop_client::posted`).
//! * **A proxy this client cannot drive** — a proxy URL whose scheme is not
//!   `http` (socks5, https-to-proxy), or one that will not parse.
//!   `HTTP_PROXY`/`HTTPS_PROXY` and the process-wide
//!   `undici.setGlobalDispatcher(new ProxyAgent(…))` with an `http://` proxy
//!   are served: the engine runs the CONNECT tunnel itself.
//! * **A URL or method `turnloop_http::client::Request::new` rejects** — a
//!   non-http(s) scheme, embedded credentials, a forbidden or malformed
//!   method. These now reject exactly as undici does; reqwest used to send the
//!   last three.
//!
//! # GC
//!
//! `ctx` is the promise address from `js_promise_new_cross_thread`, which pins
//! the promise across the crossing (#9552). Nothing else about a request is a JS value: url, method, headers and body
//! are owned Rust data copied out on this thread before submission, and the
//! response handle is built here, on the owning thread, inside the deferred
//! resolution — never in the sink.

use crate::common::async_bridge::{queue_deferred_resolution, queue_promise_resolution};
use crate::turnloop_client::{self, ClientError, Outcome, RequestSpec, ResponseOut, Sink};

use super::{
    alloc_fetch_handle_id, handle_to_f64,
    transport_error::{FetchFailure, Rejection},
    FetchResponse, FETCH_RESPONSES,
};

/// One fetch, as the entry points describe it.
pub(crate) struct FetchDispatch {
    pub(crate) url: String,
    pub(crate) method: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Option<Vec<u8>>,
    pub(crate) abort_key: Option<usize>,
    /// `RequestInit.redirect`. Only `js_fetch_with_options` can carry a
    /// non-default mode; the fixed-shape entry points (`js_fetch_get`,
    /// `js_fetch_post`, …) have no init object to read one from and always
    /// follow, which is what they did before this field existed.
    pub(crate) redirect: super::FetchRedirectMode,
}

/// Perry's fetch-level mode as the engine's. The engine implements all three
/// (`turnloop_http::client::Request::redirect`): `Manual` stops at the 3xx and
/// hands it back with its `Location`, `Error` fails the request, and `Follow`
/// chases up to `DEFAULT_MAX_REDIRECTS` (20 — Node's number).
pub(super) fn engine_redirect(
    mode: super::FetchRedirectMode,
) -> turnloop_http::client::RedirectMode {
    match mode {
        super::FetchRedirectMode::Follow => turnloop_http::client::RedirectMode::Follow,
        super::FetchRedirectMode::Manual => turnloop_http::client::RedirectMode::Manual,
        super::FetchRedirectMode::Error => turnloop_http::client::RedirectMode::Error,
    }
}

/// The `js_fetch_with_options` form: the same dispatch built from the resolved
/// `FetchInputs`.
pub(crate) fn dispatch_inputs(
    inputs: super::request_handle::FetchInputs,
    abort_key: Option<usize>,
    promise_ptr: usize,
) {
    let super::request_handle::FetchInputs {
        url,
        method,
        body,
        custom_headers,
        redirect,
    } = inputs;
    dispatch(
        FetchDispatch {
            url,
            method,
            headers: custom_headers.into_iter().collect(),
            body,
            abort_key,
            redirect,
        },
        promise_ptr,
    );
}

/// The `js_fetch_text` form, which resolves with the decoded body text rather
/// than a `Response` handle.
pub(crate) fn dispatch_text(url: String, promise_ptr: usize) {
    let spec = RequestSpec {
        url,
        method: "GET".to_string(),
        headers: Vec::new(),
        body: None,
        redirect: turnloop_http::client::RedirectMode::Follow,
        abort_key: None,
    };
    let (url, method) = (spec.url.clone(), spec.method.clone());
    let sink = Sink {
        ctx: promise_ptr,
        on_head: None,
        on_chunk: None,
        on_done: settle_text,
    };
    if let Err(declined) = turnloop_client::submit(spec, sink) {
        turnloop_client::note_declined();
        let message = format!(
            "Fetch error: {}",
            Rejection::for_declined(declined, &url, &method).message()
        );
        queue_deferred_resolution(promise_ptr, false, move || unsafe {
            super::fetch_error_bits(&message)
        });
    }
}

/// The `js_fetch_stream_start` form: Perry's line-oriented SSE poll surface.
///
/// This is the only caller of the engine's `Sink::on_head` / `Sink::on_chunk`
/// hooks. They were added by P6 and left unused, which by CLAUDE.md's
/// kill-policy made them an unexercised mode — a green engine test said nothing
/// about them. `ctx` is the stream id, not a promise: this surface resolves
/// nothing and is polled from JS instead, so a refusal is reported the way a
/// connection failure is — `status = 3` with the error text.
pub(crate) fn dispatch_stream(
    stream_id: usize,
    url: String,
    method: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
) {
    let spec = RequestSpec {
        url,
        method,
        headers,
        body,
        redirect: turnloop_http::client::RedirectMode::Follow,
        abort_key: None,
    };
    let (url, method) = (spec.url.clone(), spec.method.clone());
    let sink = Sink {
        ctx: stream_id,
        on_head: Some(stream_head),
        on_chunk: Some(stream_chunk),
        on_done: stream_done,
    };
    if let Err(declined) = turnloop_client::submit(spec, sink) {
        turnloop_client::note_declined();
        let message = Rejection::for_declined(declined, &url, &method).message();
        super::with_stream(stream_id, |state| {
            state.error = format!("Connection error: {message}");
            state.status = 3;
        });
    }
}

/// The FINAL response's head — the engine never reports a followed redirect's.
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
            // The surface's two message prefixes: a failure before the head is
            // a connection error, one after it a stream error.
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

/// Hand one fetch to the engine. The promise is settled exactly once either
/// way: by the engine's completion, or here with the refusal's rejection.
pub(crate) fn dispatch(dispatch: FetchDispatch, promise_ptr: usize) {
    let spec = RequestSpec {
        url: dispatch.url,
        method: dispatch.method,
        headers: dispatch.headers,
        body: dispatch.body,
        redirect: engine_redirect(dispatch.redirect),
        abort_key: dispatch.abort_key,
    };
    let (url, method) = (spec.url.clone(), spec.method.clone());
    let sink = Sink {
        ctx: promise_ptr,
        on_head: None,
        on_chunk: None,
        on_done: settle,
    };
    if let Err(declined) = turnloop_client::submit(spec, sink) {
        turnloop_client::note_declined();
        let rejection = Rejection::for_declined(declined, &url, &method);
        queue_deferred_resolution(promise_ptr, false, move || rejection.into_js_bits());
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
