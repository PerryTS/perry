//! Session/server registration, connect-ordering machinery, and the
//! client `connect()` + outbound-request path.

use super::*;

use std::collections::HashMap;

use perry_ffi::{
    get_handle, get_handle_mut, iter_handle_ids_of, iter_handles_of, iter_handles_of_mut,
    register_handle, JsValue,
};

use crate::server::ensure_gc_scanner_registered;
use crate::server::http2_session_settings::Http2SettingsState;
use crate::server::types::jsvalue_to_owned_string;

pub(crate) fn mark_server_sessions_closed(server_handle: i64) {
    let mut turnloop_conns = Vec::new();
    iter_handles_of_mut::<Http2SessionHandle, _>(|session| {
        if session.server_handle == server_handle {
            session.closed = true;
            session.destroyed = true;
            if session.turnloop_conn != 0 {
                turnloop_conns.push(std::mem::replace(&mut session.turnloop_conn, 0));
            }
        }
    });
    // A turnloop connection is a live handle that keeps the loop referenced;
    // marking the JS session destroyed without closing it would keep the
    // process alive after `server.close()`.
    for conn in turnloop_conns {
        crate::server::turnloop_h2::control::session_destroy(conn);
    }
}

pub(crate) fn h2_listening_server_for_authority(authority: &str) -> Option<i64> {
    let (_, port, _) = parse_authority(authority);
    let mut matched = None;
    iter_handle_ids_of::<Http2SecureServer, _>(|server_id| {
        if matched.is_some() {
            return;
        }
        if get_handle::<Http2SecureServer>(server_id)
            .map(|server| server.base.listening && server.base.bound_port == port)
            .unwrap_or(false)
        {
            matched = Some(server_id);
        }
    });
    matched
}

pub(crate) fn local_server_handle_for_client(session_handle: i64) -> Option<i64> {
    let session = get_handle::<Http2SessionHandle>(session_handle)?;
    if session.session_type != 1 {
        return None;
    }
    if session.server_handle != 0 {
        return Some(session.server_handle);
    }
    h2_listening_server_for_authority(&session.authority)
}

fn has_active_server_session(server_handle: i64) -> bool {
    let mut active = false;
    iter_handles_of::<Http2SessionHandle, _>(|session| {
        if session.server_handle == server_handle && !session.closed && !session.destroyed {
            active = true;
        }
    });
    active
}

#[allow(dead_code)] // retained: server-session listener probe
fn server_has_session_listener(server_handle: i64) -> bool {
    get_handle::<Http2SecureServer>(server_handle)
        .map(|server| crate::server::server::server_has_event_listener(&server.base, "session"))
        .unwrap_or(false)
}

#[allow(dead_code)] // retained: server-session emit bookkeeping
fn has_emitted_server_session(server_handle: i64) -> bool {
    let mut emitted = false;
    iter_handles_of::<Http2SessionHandle, _>(|session| {
        if session.server_handle == server_handle
            && session.session_event_emitted
            && !session.closed
            && !session.destroyed
        {
            emitted = true;
        }
    });
    emitted
}

pub(crate) fn local_client_connect_ready(session_handle: i64) -> bool {
    let Some(server_handle) = local_server_handle_for_client(session_handle) else {
        return true;
    };
    // The client `connect` only needs the server session to be ACTIVE (the
    // handshake established), not for the server's `session` EVENT to have
    // fired — Node emits that event after the client connect. Gating on the
    // emitted event forced a `session`-before-`connect` order that Node never
    // produces.
    has_active_server_session(server_handle)
}

pub(crate) fn local_server_session_event_ready(server_session_handle: i64) -> bool {
    let Some(server_session) = get_handle::<Http2SessionHandle>(server_session_handle) else {
        return true;
    };
    if server_session.session_type != 0
        || server_session.server_handle == 0
        || server_session.connection_port == 0
    {
        return true;
    }
    let server_handle = server_session.server_handle;
    let connection_port = server_session.connection_port;
    let mut ready = true;
    iter_handles_of::<Http2SessionHandle, _>(|session| {
        if session.session_type == 1
            && session.server_handle == server_handle
            && session.connection_port == connection_port
            && !session.closed
            && !session.destroyed
            && !session.connect_event_emitted
        {
            ready = false;
        }
    });
    ready
}

#[no_mangle]
pub unsafe extern "C" fn js_node_http2_connect(
    authority_f64: f64,
    options_f64: f64,
    listener: i64,
) -> i64 {
    ensure_gc_scanner_registered();
    let authority =
        jsvalue_to_owned_string(authority_f64).unwrap_or_else(|| "http://localhost:80".to_string());
    let callback = if listener != 0 {
        listener
    } else {
        closure_arg(Some(options_f64))
    };
    let secure = authority.starts_with("https:");
    let (host, port, host_port) = parse_authority(&authority);
    let local_server_handle = h2_listening_server_for_authority(&host_port).unwrap_or(0);
    let mut listeners = HashMap::new();
    if callback != 0 {
        listeners
            .entry("connect".to_string())
            .or_insert_with(Vec::new)
            .push(callback);
    }
    let session_handle = register_handle(Http2SessionHandle {
        server_handle: local_server_handle,
        connection_port: 0,
        session_event_emitted: false,
        connect_event_emitted: false,
        session_type: 1,
        connected: false,
        encrypted: secure,
        // Replaced by whatever ALPN selected once the handshake completes; the
        // placeholder is what a cleartext session keeps.
        alpn_protocol: "h2c".to_string(),
        connecting: true,
        closed: false,
        destroyed: false,
        pending_settings_ack: false,
        authority: host_port,
        local_settings: Http2SettingsState::default(),
        remote_settings: Http2SettingsState::default(),
        local_window_size: 65_535,
        listeners,
        close_callbacks: Vec::new(),
        pending_callbacks: Vec::new(),
        timeout_callback: 0,
        turnloop_conn: 0,
    });

    // Both schemes go on the loop. That removed **two** private
    // `current_thread` tokio runtimes — one built here per session, one built
    // in `start_client_request` per request (perry#10327) — and makes
    // concurrent `session.request()` calls real multiplexed streams instead of
    // a race for a single `h2::client::SendRequest`.
    //
    // `https://` kept the `h2` path only for want of a public TLS client
    // installer on a turnloop socket, and `perry_ext_net::turnloop_tls_io`
    // grew one (`install_client_session`), so `connect_client` installs a real
    // client session with `h2` in ALPN. The `h2` path it replaced had never
    // worked for `https://` anyway: `parse_authority` returned port 80 for
    // every scheme and the connect opened a CLEARTEXT socket, so the HTTP/2
    // preface went to a TLS listener and the peer answered
    // `InvalidContentType`.
    if crate::server::turnloop_h2::enabled() {
        let tls = secure.then(|| crate::server::turnloop_h2::ClientTls {
            servername: host.clone(),
            verify: client_reject_unauthorized(options_f64),
            ca: client_ca_material(options_f64),
        });
        if let Some(conn_id) =
            crate::server::turnloop_h2::connect_client(session_handle, &host, port, tls)
        {
            bind_turnloop_session(session_handle, conn_id);
            return session_handle;
        }
    }

    decline_client_session(session_handle);
    session_handle
}

/// The message a session gets when there is no transport for it.
pub(crate) const NO_LOOP_MESSAGE: &str =
    "no event loop on this thread: http2.connect needs a turnloop agent";

/// No loop is reachable from this thread, so there is no transport.
///
/// Since turnloop P9 gave every JS agent a loop, that leaves a second thread
/// acting for an agent another thread already owns (a host pump thread;
/// Android's UI thread for `perry-native`), and a host where `Loop::new`
/// failed.
///
/// The `h2` client that used to stand here is gone rather than kept. It built a
/// private `current_thread` runtime per session and a *second* one per request;
/// it ignored `secure` entirely, so `https://` got a CLEARTEXT socket and an
/// HTTP/2 preface sent at a TLS listener; and nothing exercised it. Saying so
/// on `'error'` is the `perry-ext-ws` rule — a real narrowing, written down in
/// `changelog.d/`, rather than a fallback nobody runs.
pub(crate) fn decline_client_session(session_handle: i64) {
    if let Some(session) = get_handle_mut::<Http2SessionHandle>(session_handle) {
        session.connecting = false;
        session.closed = true;
        session.destroyed = true;
    }
    push_h2_event(Http2PendingEvent::ClientError {
        handle: session_handle,
        message: NO_LOOP_MESSAGE.to_string(),
    });
}

/// `options.rejectUnauthorized` for `http2.connect`, defaulting to Node's own
/// `true`.
///
/// `options` may not be an object at all: `http2.connect(authority, listener)`
/// puts the callback in this argument, and a lookup on a function answers
/// `undefined` — which is the same as "unset", so no special case is needed.
unsafe fn client_reject_unauthorized(options: f64) -> bool {
    let value = perry_ffi::object_field_by_name(
        JsValue::from_bits(options.to_bits()),
        "rejectUnauthorized",
    );
    if value.is_undefined() || value.is_null() {
        return true;
    }
    value.to_bool()
}

/// `options.ca` — a PEM string, a Buffer, or an array of either.
///
/// Node's `ca` REPLACES the platform roots rather than adding to them, so an
/// absent option has to stay an empty list here and mean "keep the defaults"
/// downstream; returning a single empty blob would trust nothing.
unsafe fn client_ca_material(options: f64) -> Vec<Vec<u8>> {
    let value = perry_ffi::object_field_by_name(JsValue::from_bits(options.to_bits()), "ca");
    if value.is_undefined() || value.is_null() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let array = value.as_pointer::<perry_ffi::ArrayHeader>();
    if !array.is_null() && is_js_array(value) {
        let len = perry_ffi::js_array_length(array);
        for index in 0..len {
            if let Some(pem) = pem_bytes(perry_ffi::js_array_get(array, index)) {
                out.push(pem);
            }
        }
        return out;
    }
    if let Some(pem) = pem_bytes(value) {
        out.push(pem);
    }
    out
}

unsafe fn is_js_array(value: JsValue) -> bool {
    extern "C" {
        fn js_array_is_array(value: f64) -> f64;
    }
    JsValue::from_bits(js_array_is_array(f64::from_bits(value.bits())).to_bits()).to_bool()
}

/// One PEM blob, from a string or a Buffer.
///
/// The Buffer read goes through the **canonical runtime registry**: this crate
/// is a separately linked archive and cannot see a Buffer the program runtime
/// allocated, which is exactly what `fs.readFileSync` returns.
unsafe fn pem_bytes(value: JsValue) -> Option<Vec<u8>> {
    if let Some(text) = jsvalue_to_owned_string(f64::from_bits(value.bits())) {
        return Some(text.into_bytes());
    }
    extern "C" {
        fn js_value_buffer_or_typedarray_data(value: f64, out_len: *mut u32) -> *const u8;
    }
    let mut len = 0u32;
    let data = js_value_buffer_or_typedarray_data(f64::from_bits(value.bits()), &mut len);
    if data.is_null() || len == 0 {
        None
    } else {
        Some(std::slice::from_raw_parts(data, len as usize).to_vec())
    }
}

/// `(host, port, host:port)` for an `http2.connect` authority.
///
/// The default port follows the SCHEME. It used to be 80 unconditionally, which
/// is why `http2.connect('https://example.com')` opened a cleartext socket to
/// port 80 — the failure the h2c lane recorded as
/// `received corrupt message of type InvalidContentType`.
pub(crate) fn parse_authority(authority: &str) -> (String, u16, String) {
    let default_port = if authority.starts_with("https://") {
        443
    } else {
        80
    };
    let without_scheme = authority
        .strip_prefix("http://")
        .or_else(|| authority.strip_prefix("https://"))
        .unwrap_or(authority);
    let host_port = without_scheme.split('/').next().unwrap_or(without_scheme);
    if let Some(rest) = host_port.strip_prefix('[') {
        if let Some(end) = rest.find(']') {
            let host = rest[..end].to_string();
            let port = rest[end + 1..]
                .strip_prefix(':')
                .and_then(|p| p.parse::<u16>().ok())
                .unwrap_or(default_port);
            return (host, port, host_port.to_string());
        }
    }
    let mut parts = host_port.rsplitn(2, ':');
    let maybe_port = parts.next().unwrap_or("");
    let maybe_host = parts.next();
    if let (Some(host), Ok(port)) = (maybe_host, maybe_port.parse::<u16>()) {
        (host.to_string(), port, host_port.to_string())
    } else {
        (host_port.to_string(), default_port, host_port.to_string())
    }
}

pub(crate) fn parse_headers_object(value: f64) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let v = JsValue::from_bits(value.to_bits());
    if !v.is_pointer() {
        return out;
    }
    let Some(json) = perry_ffi::json_stringify(v) else {
        return out;
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json) else {
        return out;
    };
    let Some(obj) = parsed.as_object() else {
        return out;
    };
    for (key, value) in obj {
        let value = value
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| value.to_string().trim_matches('"').to_string());
        out.insert(key.to_ascii_lowercase(), value);
    }
    out
}

/// The HEADERS block for `session.request(headers)` on the turnloop path.
///
/// RFC 9113 §8.3 is strict about this in a way the `h2` path never had to be,
/// because `h2` built the block from a `Request` object: pseudo-headers come
/// first and in no particular order among themselves but **before** every
/// regular field, every name is lowercase, and `:method` / `:scheme` / `:path`
/// are mandatory. `turnloop_http::http2::validate_headers` rejects a block that
/// breaks any of it — as a connection error — so the defaults Node applies are
/// applied here rather than left to the caller.
fn client_request_headers(stream_handle: i64, session_handle: i64) -> Vec<(String, String)> {
    let requested = get_handle::<Http2StreamHandle>(stream_handle)
        .map(|stream| stream.request_headers.clone())
        .unwrap_or_default();
    let authority = get_handle::<Http2SessionHandle>(session_handle)
        .map(|session| session.authority.clone())
        .unwrap_or_default();
    let pick = |name: &str, fallback: &str| {
        requested
            .get(name)
            .filter(|value| !value.is_empty())
            .cloned()
            .unwrap_or_else(|| fallback.to_string())
    };
    let mut out = vec![
        (":method".to_string(), pick(":method", "GET")),
        (":scheme".to_string(), pick(":scheme", "http")),
        (":path".to_string(), pick(":path", "/")),
    ];
    let authority = pick(":authority", &authority);
    if !authority.is_empty() {
        out.push((":authority".to_string(), authority));
    }
    let mut regular: Vec<(String, String)> = requested
        .iter()
        .filter(|(name, _)| !name.starts_with(':'))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.clone()))
        .collect();
    // `request_headers` is a `HashMap`, so its iteration order is not stable
    // across runs and an unordered header block would make every byte-for-byte
    // parity comparison flaky.
    regular.sort();
    out.extend(regular);
    out
}

pub(crate) fn start_client_request(stream_handle: i64, body: Vec<u8>) {
    // On turnloop the stream opens on this thread, on the session's existing
    // connection. No runtime, no thread, no `SendRequest` to race for.
    if let Some((session_handle, conn_id)) = get_handle::<Http2StreamHandle>(stream_handle)
        .map(|stream| stream.session_handle)
        .and_then(|session| super::turnloop_conn_of_session(session).map(|conn| (session, conn)))
    {
        let headers = client_request_headers(stream_handle, session_handle);
        crate::server::turnloop_h2::stream::request(conn_id, stream_handle, headers, body);
        return;
    }
    // The session is not on turnloop, so it never connected:
    // `js_node_http2_connect` already errored it. The `h2` `SendRequest` that
    // used to be reached here — through a *second* private `current_thread`
    // runtime, one per request — is gone with it, so say the same thing on the
    // stream rather than hanging.
    push_h2_event(Http2PendingEvent::ClientError {
        handle: stream_handle,
        message: "HTTP/2 session is not connected".to_string(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The events queued for one handle, without draining the global queue —
    /// these tests run in the same process as every other test in this crate
    /// and must not consume each other's events.
    fn errors_for(handle: i64) -> Vec<String> {
        let events = H2_PENDING_EVENTS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        events
            .iter()
            .filter_map(|event| match event {
                Http2PendingEvent::ClientError { handle: h, message } if *h == handle => {
                    Some(message.clone())
                }
                _ => None,
            })
            .collect()
    }

    fn client_session(turnloop_conn: i64) -> Http2SessionHandle {
        Http2SessionHandle {
            server_handle: 0,
            connection_port: 0,
            session_event_emitted: false,
            connect_event_emitted: false,
            session_type: 1,
            connected: false,
            encrypted: true,
            alpn_protocol: "h2c".to_string(),
            connecting: true,
            closed: false,
            destroyed: false,
            pending_settings_ack: false,
            authority: "example.invalid:443".to_string(),
            local_settings: Http2SettingsState::default(),
            remote_settings: Http2SettingsState::default(),
            local_window_size: 65_535,
            listeners: HashMap::new(),
            close_callbacks: Vec::new(),
            pending_callbacks: Vec::new(),
            timeout_callback: 0,
            turnloop_conn,
        }
    }

    /// The capability the tokio inventory recorded as MISSING —
    /// "`turnloop_tls_io` exposes `install_server_session` publicly but only
    /// `begin_client_upgrade` (`pub(crate)`…), so `http2.connect('https://…')`
    /// has no way to install a client session on a turnloop socket".
    ///
    /// It exists, it is `pub`, and it takes neither perry-ext-net's own
    /// `TlsClientConfigData` nor a `JsNativeAsyncCompletion` — which were the
    /// two shape objections in that entry. Coercing it to a plain fn pointer is
    /// the assertion: narrowing it back to `pub(crate)`, or putting either of
    /// those types in the signature, stops this compiling.
    #[test]
    fn a_public_tls_client_installer_exists_for_a_turnloop_socket() {
        let install: fn(i64, String, bool, Vec<Vec<u8>>, Vec<Vec<u8>>) -> Result<(), String> =
            perry_ext_net::turnloop_tls_io::install_client_session;
        // Used, so the coercion cannot be optimized away as a dead binding.
        assert!(!std::ptr::fn_addr_eq(
            install,
            (|_, _, _, _, _| Ok(()))
                as fn(i64, String, bool, Vec<Vec<u8>>, Vec<Vec<u8>>) -> Result<(), String>
        ));
    }

    /// `http2.connect('https://…')` may speak HTTP/2 only if the server selects
    /// `h2`, so the offer is the whole point of having a client installer. It
    /// must be `h2` ALONE: with `http/1.1` in the list a server could select it
    /// and leave the connection holding a protocol this path cannot speak.
    #[test]
    fn the_https_client_offers_h2_and_only_h2() {
        assert_eq!(
            crate::server::turnloop_h2::conn::client_alpn(),
            vec![b"h2".to_vec()]
        );
    }

    /// The subject assertion for everything below: turnloop is genuinely the
    /// HTTP/2 client transport in this binary.
    ///
    /// `turnloop_net::sink_installed` exists so a "turnloop carried this" claim
    /// cannot pass with nothing listening. Asking `enabled()` also *performs*
    /// the registration, so the two are asked in that order.
    ///
    /// This is not decoration. It was written expecting the opposite — that a
    /// cargo-test thread owns no loop — and failed, which is how the decline
    /// tests below came to assert their fixtures rather than the environment.
    #[test]
    fn turnloop_is_live_as_the_http2_client_transport() {
        assert!(
            crate::server::turnloop_h2::enabled(),
            "a cargo-test thread does reach an agent loop; if that stops being \
             true the two decline tests below are the only HTTP/2 client \
             coverage left and must be re-read"
        );
        assert!(
            perry_ffi::turnloop_net::sink_installed(crate::server::turnloop_h2::SUBSYSTEM),
            "enabled() answered yes with no completion sink installed"
        );
    }

    /// A client session with no transport is ERRORED, not left connecting.
    ///
    /// This is what stands where the `h2` fallback stood, so the thing worth
    /// pinning is that the session does not sit in `connecting` forever.
    #[test]
    fn a_session_with_no_loop_is_errored_rather_than_left_connecting() {
        let handle = register_handle(client_session(0));
        assert!(
            get_handle::<Http2SessionHandle>(handle)
                .map(|s| s.connecting)
                .unwrap_or(false),
            "fixture must start connecting, or the assertion below is vacuous"
        );
        assert!(errors_for(handle).is_empty());

        decline_client_session(handle);

        let session = get_handle::<Http2SessionHandle>(handle).expect("session");
        assert!(
            !session.connecting,
            "a declined session must stop connecting"
        );
        assert!(session.closed && session.destroyed);
        assert_eq!(errors_for(handle), vec![NO_LOOP_MESSAGE.to_string()]);
    }

    /// The per-request half of the deleted `h2` fallback: `session.request()` on
    /// a session that is not on turnloop used to build a SECOND private
    /// `current_thread` runtime and take an `h2::client::SendRequest`. With that
    /// gone the stream must be errored, not silently dropped — a dropped one
    /// hangs the program, which is the failure mode a deleted fallback is most
    /// likely to introduce.
    #[test]
    fn a_request_on_a_transportless_session_errors_its_stream() {
        let session_handle = register_handle(client_session(0));
        let stream_handle = register_handle(Http2StreamHandle {
            session_handle,
            id: 0,
            pending: true,
            closed: false,
            destroyed: false,
            aborted: false,
            rst_code: 0,
            headers_sent: false,
            sent_headers: Vec::new(),
            request_headers: HashMap::new(),
            listeners: HashMap::new(),
            encoding: None,
            response_status: 0,
            response_headers: Vec::new(),
            turnloop_conn: 0,
            turnloop_responded: false,
        });
        assert!(
            super::super::turnloop_conn_of_session(session_handle).is_none(),
            "fixture must start with no turnloop connection, or the assertion below is vacuous"
        );

        start_client_request(stream_handle, Vec::new());

        assert_eq!(
            errors_for(stream_handle),
            vec!["HTTP/2 session is not connected".to_string()]
        );
    }
}
