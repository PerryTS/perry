//! `Expect: 100-continue` (issue #5080): the head goes out before `end()`, the
//! body is withheld until the server's interim `100 Continue`, which fires
//! `'continue'`.
//!
//! This used to be a raw tokio `TcpStream` bypass, because reqwest consumed the
//! interim response. The exchange itself now runs in `client_turnloop`
//! (`Mode::Continue`), where the codec hands the `100` back as
//! `Event::Informational`; this module keeps the Node-facing half — deciding
//! *when* the head is flushed, and handing the body over at `end()`.
//!
//! The head keeps the bypass's framing: `Transfer-Encoding: chunked` unless the
//! caller pinned a `Content-Length` / `Transfer-Encoding`, and
//! `Connection: close`. Since the exchange runs on the same transport as every
//! other request, `https:` gets `'continue'` too — Node emits it for both.

use std::collections::HashMap;

use perry_ffi::{with_handle_mut, Handle};

use crate::ClientRequestHandle;

/// Whether the request headers ask for the `100-continue` handshake.
pub(crate) fn wants_continue(headers: &HashMap<String, String>) -> bool {
    headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("expect") && value.to_ascii_lowercase().contains("100-continue")
    })
}

/// Queue an `arm_expect_continue` for the next event-loop tick. Node flushes a
/// request's head on `nextTick`, not at construction, so deferring lets a
/// post-construction `setHeader(...)` (including a late `Expect:
/// 100-continue`) reach the wire before the head is snapshotted.
pub(crate) fn defer_arm(handle: Handle) {
    crate::push_event(crate::PendingHttpEvent::DeferredArmContinue {
        request_handle: handle,
    });
}

/// #5080 — if the request carries `Expect: 100-continue`, flush its head now
/// and mark the body as withheld. A no-op otherwise, or once armed/ended.
pub(crate) fn arm_expect_continue(handle: Handle) {
    let snapshot = with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| {
        if req.ended
            || req.expects_continue
            || !(req.url.starts_with("http://") || req.url.starts_with("https://"))
        {
            return None;
        }
        if !wants_continue(&req.headers) {
            return None;
        }
        req.expects_continue = true;
        req.continue_body_pending = true;
        Some((
            req.method.clone(),
            req.url.clone(),
            req.headers.clone(),
            req.timeout_ms,
            req.agent_handle,
            req.tls.clone(),
        ))
    })
    .flatten();
    if let Some((method, url, headers, timeout_ms, agent_handle, tls)) = snapshot {
        crate::client_turnloop::dispatch(crate::client_turnloop::Request {
            request_handle: handle,
            method: &method,
            url: &url,
            headers,
            body: Vec::new(),
            timeout_ms,
            agent_handle,
            tls: &tls,
            continue_mode: true,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_continue_predicate_is_case_insensitive() {
        let headers: HashMap<String, String> =
            [("Expect".to_string(), "100-Continue".to_string())].into();
        assert!(wants_continue(&headers));
        let headers: HashMap<String, String> = [("expect".to_string(), "other".to_string())].into();
        assert!(!wants_continue(&headers));
    }
}
