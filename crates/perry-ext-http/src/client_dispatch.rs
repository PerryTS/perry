//! Async reqwest dispatch for `http.request` / `https.request` (and the
//! `get` variants). Extracted from `lib.rs` to keep that file under the
//! 2000-line lint cap; the logic is unchanged apart from the #4906
//! client-side TLS selection added at the top of `dispatch_request`.

use std::collections::HashMap;

use perry_ffi::{spawn_blocking_with_reactor as spawn_blocking, Handle};

use crate::{
    agent, dispatch_plain_http_request, push_event, tls_client, ClientInflightGuard,
    PendingHttpEvent, HTTP_CLIENT,
};

/// Spawn the actual reqwest send. The `spawn_blocking_with_reactor`
/// shim runs the closure inside `runtime().spawn(async { ... })`, so
/// we're already in an async context — `Handle::current().block_on`
/// from here would panic with "Cannot start a runtime from within a
/// runtime" (issue #769). Instead, spawn the request future as a
/// fresh detached task on the same multi-thread runtime; it drives
/// itself via `await` chains while we return immediately. Mirrors
/// the `spawn_socket_runner` pattern in `perry-ext-net`.
pub(crate) fn dispatch_request(
    request_handle: Handle,
    method: String,
    url: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
    timeout_ms: Option<u64>,
    agent_handle: Handle,
    tls: tls_client::TlsOptions,
) {
    // #4906: an https request carrying client-side TLS options
    // (rejectUnauthorized / ca / checkServerIdentity, or a process-wide
    // NODE_TLS_REJECT_UNAUTHORIZED=0) needs a verifier configured per its
    // options — the pooled default client always validates against the
    // native roots — so build a dedicated client (folding in the Agent's
    // pool config). #2154: otherwise pick the per-Agent client when one
    // was supplied, falling back to the global HTTP_CLIENT.
    let client: reqwest::Client = if url.starts_with("https://") && tls.needs_custom_client() {
        let pool = (agent_handle != 0)
            .then(|| agent::agent_pool_config(agent_handle))
            .flatten();
        match tls.build_client(pool) {
            Ok(custom) => custom,
            Err(error_message) => {
                push_event(PendingHttpEvent::Error {
                    request_handle,
                    error_message,
                });
                return;
            }
        }
    } else if agent_handle != 0 {
        agent::client_for_agent(agent_handle)
    } else {
        HTTP_CLIENT.clone()
    };
    spawn_blocking(move || {
        // Defeat LTO dead-stripping of tokio's CONTEXT statics — same
        // workaround perry-ext-net needs (see spawn_socket_runner).
        let try_h = tokio::runtime::Handle::try_current();
        std::hint::black_box(&try_h);
        if try_h.is_err() {
            push_event(PendingHttpEvent::Error {
                request_handle,
                error_message: "http client runtime unavailable".to_string(),
            });
            return;
        }
        let handle = tokio::runtime::Handle::current();
        // #5892 remainder (issue_4909 early-exit): the outer closure's
        // EXT_BLOCKING gate drops the moment we return, and the first
        // `push_event` is a full response round-trip away — without this
        // guard the exchange is invisible to the exit gate in between, so a
        // program whose only other live handle just closed (in-process
        // server+client) can clean-exit before 'response' fires. Moved into
        // the task so it covers the full stream lifetime (#5779 idle-kick).
        let inflight_guard = ClientInflightGuard::new();
        let jh = handle.spawn(async move {
            let _inflight = inflight_guard;
            if let Some(result) = dispatch_plain_http_request(
                request_handle,
                method.as_str(),
                &url,
                &headers,
                &body,
                timeout_ms,
            )
            .await
            {
                if let Err(error_message) = result {
                    push_event(PendingHttpEvent::Error {
                        request_handle,
                        error_message,
                    });
                }
                return;
            }

            let mut req = match method.as_str() {
                "POST" => client.post(&url),
                "PUT" => client.put(&url),
                "DELETE" => client.delete(&url),
                "PATCH" => client.patch(&url),
                "HEAD" => client.head(&url),
                "OPTIONS" => client.request(reqwest::Method::OPTIONS, &url),
                _ => client.get(&url),
            };
            for (k, v) in &headers {
                req = req.header(k.as_str(), v.as_str());
            }
            // Node's default agent is keep-alive (v19+) and sends the
            // header explicitly; servers reading `req.headers.connection`
            // expect it.
            if !headers.keys().any(|k| k.eq_ignore_ascii_case("connection")) {
                req = req.header("Connection", "keep-alive");
            }
            if let Some(ms) = timeout_ms {
                req = req.timeout(std::time::Duration::from_millis(ms));
            } else {
                req = req.timeout(std::time::Duration::from_secs(30));
            }
            if !body.is_empty() {
                req = req.body(body);
            }
            match req.send().await {
                Ok(mut response) => {
                    let status = response.status().as_u16();
                    let status_message = response
                        .status()
                        .canonical_reason()
                        .unwrap_or("")
                        .to_string();
                    let mut hdrs = Vec::new();
                    for (k, v) in response.headers() {
                        if let Ok(s) = v.to_str() {
                            hdrs.push((k.to_string(), s.to_string()));
                        }
                    }
                    // Streaming delivery: hand the head to the main thread
                    // as soon as it arrives, then pump body chunks as they
                    // come off the socket. Client code can react to the
                    // headers (timers, destroy, data listeners) while the
                    // server is still producing the body — Node's model.
                    push_event(PendingHttpEvent::ResponseHead {
                        request_handle,
                        status,
                        status_message,
                        headers: hdrs,
                    });
                    loop {
                        match response.chunk().await {
                            Ok(Some(bytes)) => {
                                push_event(PendingHttpEvent::ResponseChunk {
                                    request_handle,
                                    chunk: bytes,
                                });
                            }
                            Ok(None) => {
                                push_event(PendingHttpEvent::ResponseEnd { request_handle });
                                break;
                            }
                            Err(e) => {
                                if e.is_timeout() {
                                    push_event(PendingHttpEvent::Timeout { request_handle });
                                } else {
                                    push_event(PendingHttpEvent::Error {
                                        request_handle,
                                        error_message: e.to_string(),
                                    });
                                }
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    // #4905: surface transport deadlines as the 'timeout'
                    // event instead of a generic error.
                    if e.is_timeout() {
                        push_event(PendingHttpEvent::Timeout { request_handle });
                    } else if let Some((message, code, syscall, errno)) =
                        crate::transport_error::classify_reqwest(&e, &url)
                    {
                        // A recognized transport failure (connect refused, DNS
                        // lookup failure, …) — hand listeners the real coded
                        // Node Error instead of a bare string.
                        push_event(PendingHttpEvent::TransportError {
                            request_handle,
                            message,
                            code,
                            syscall,
                            errno,
                        });
                    } else {
                        push_event(PendingHttpEvent::Error {
                            request_handle,
                            error_message: e.to_string(),
                        });
                    }
                }
            }
        });
        std::hint::black_box(&jh);
        std::mem::forget(jh);
    });
}
