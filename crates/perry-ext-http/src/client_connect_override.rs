//! HTTP requests on a caller-supplied net.Socket. Override callbacks retain
//! the same actual object; the request/Agent ownership edges are updated
//! before releasing the unconnected placeholder. The Socket's link route
//! becomes HTTP while preserving its driver read and TLS session.

use std::collections::HashMap;

use perry_ffi::Handle;

use super::agent;
use crate::{push_event, PendingHttpEvent};

/// Look up `request_handle`'s own `createConnection` (if any) and, when
/// set, dispatch over it. `None` means "not set / not usable" — the
/// caller (only reached when `agent_handle == 0`) falls back to the default
/// transport.
pub(crate) fn dispatch_for_handle(request_handle: Handle, url: &str) -> Option<i64> {
    let cc = perry_ffi::with_handle_mut::<crate::ClientRequestHandle, _, _>(request_handle, |r| {
        r.request_create_connection
    })
    .unwrap_or(0);
    if cc == 0 {
        return None;
    }
    request_create_connection_socket(cc, url)
}

/// Invoke a request's createConnection override on its JS heap.
pub(crate) fn request_create_connection_socket(
    request_create_connection: i64,
    url: &str,
) -> Option<i64> {
    let (host, port, path) = super::socket_connect_target(url)?;
    let socket_id = unsafe {
        agent::try_request_create_connection_socket(request_create_connection, &host, port, &path)
    }?;
    Some(socket_id)
}

/// Serialize an HTTP/1.1 request (request line + headers + body) into the
/// bytes to write onto a socket. Ordinary responses force `Connection: close`
/// because this path reads until EOF. Upgrade requests preserve the caller's
/// `Connection: Upgrade` header so a `101` can hand the live socket back to
/// JavaScript. `Host` is always derived from the URL.
fn serialize_http_request(
    method: &str,
    path: &str,
    host_header: &str,
    headers: &HashMap<String, String>,
    body: &[u8],
) -> Vec<u8> {
    let wants_upgrade = crate::client_upgrade::wants_upgrade(headers);
    let mut req = format!("{} {} HTTP/1.1\r\nHost: {}\r\n", method, path, host_header);
    let mut has_content_length = false;
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("content-length") {
            has_content_length = true;
        }
        if k.eq_ignore_ascii_case("host")
            || (k.eq_ignore_ascii_case("connection") && !wants_upgrade)
        {
            continue;
        }
        req.push_str(k);
        req.push_str(": ");
        req.push_str(v);
        req.push_str("\r\n");
    }
    if !wants_upgrade {
        req.push_str("Connection: close\r\n");
    }
    if !body.is_empty() && !has_content_length {
        req.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    req.push_str("\r\n");
    let mut out = req.into_bytes();
    out.extend_from_slice(body);
    out
}

/// Run the existing wire request on the actual supplied Socket. Driver and
/// TLS ownership remain with Socket; framing is a separate pure payload.
pub(crate) fn dispatch_request_over_socket(
    request_handle: Handle,
    method: String,
    url: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
    timeout_ms: Option<u64>,
    socket_id: i64,
) {
    let parsed = match url::Url::parse(&url) {
        Ok(u) => u,
        Err(e) => {
            push_event(PendingHttpEvent::Error {
                request_handle,
                error_message: e.to_string(),
            });
            return;
        }
    };
    let host = parsed.host_str().unwrap_or("localhost").to_string();
    let host_header = match parsed.port() {
        Some(p) => format!("{}:{}", host, p),
        None => host,
    };
    let mut path = parsed.path().to_string();
    if path.is_empty() {
        path.push('/');
    }
    if let Some(q) = parsed.query() {
        path.push('?');
        path.push_str(q);
    }
    let req_bytes = serialize_http_request(&method, &path, &host_header, &headers, &body);
    let scope = perry_ffi::TransientRootScope::enter();
    let owner = scope.root_addr(socket_id);
    let value = f64::from_bits(perry_ffi::JsValue::from_object_ptr(owner.get() as *mut u8).bits());
    if perry_ext_net::native_transport::socket_link(value).is_err() {
        push_event(PendingHttpEvent::CodedError {
            request_handle,
            code: "ERR_INVALID_RETURN_VALUE".into(),
            message: "createConnection must return a net.Socket".into(),
        });
        return;
    }
    let Some((previous, agent, key, tls)) =
        perry_ffi::get_handle_mut::<crate::ClientRequestHandle>(request_handle).map(|request| {
            let previous = request.socket_handle;
            request.socket_handle = owner.get();
            request.socket_snapshot = None;
            request.reused_socket = false;
            (
                previous,
                request.agent_handle,
                request.agent_key.clone(),
                request.tls.clone(),
            )
        })
    else {
        return;
    };
    let previous = scope.root_addr(previous);
    if let Some(agent) = perry_ffi::get_handle_mut::<crate::agent::AgentHandle>(agent) {
        if let Some(sockets) = agent.active_socket_handles.get_mut(&key) {
            for socket in sockets {
                if *socket == previous.get() {
                    *socket = owner.get();
                }
            }
        }
    }
    if agent != 0 {
        crate::agent::track_agent_socket(agent, owner.get());
    }
    if previous.get() != 0 && previous.get() != owner.get() {
        perry_ext_net::native_transport::destroy(f64::from_bits(
            perry_ffi::JsValue::from_object_ptr(previous.get() as *mut u8).bits(),
        ));
    }
    crate::client_turnloop::dispatch_supplied(
        crate::client_turnloop::Request {
            request_handle,
            method: &method,
            url: &url,
            headers,
            body,
            timeout_ms,
            agent_handle: agent,
            tls: &tls,
            continue_mode: false,
        },
        f64::from_bits(perry_ffi::JsValue::from_object_ptr(owner.get() as *mut u8).bits()),
        req_bytes,
    );
}

#[cfg(test)]
mod tests {
    use super::serialize_http_request;
    use std::collections::HashMap;

    #[test]
    fn websocket_upgrade_keeps_connection_header() {
        let headers = HashMap::from([
            ("Connection".to_string(), "Upgrade".to_string()),
            ("Upgrade".to_string(), "websocket".to_string()),
        ]);
        let request = String::from_utf8(serialize_http_request(
            "GET",
            "/socket",
            "localhost:1234",
            &headers,
            &[],
        ))
        .unwrap();
        assert!(request.contains("Connection: Upgrade\r\n"), "{request}");
        assert!(!request.contains("Connection: close\r\n"), "{request}");
    }
}
