//! The listen decision for `http2.createServer` / `http2.createSecureServer`,
//! and the option surface the transport reads out of the JS handle.
//!
//! Split out of `http2_server.rs` so that file stays under the repository's
//! 2000-line-per-file lint cap.

use perry_ffi::{get_handle, get_handle_mut};

use super::{Http2SecureServer, Http2SettingsState};

/// Node's `maxSessionMemory`, in MB. The receive window is not reopened past
/// this much buffered, undispatched body — see `turnloop_h2::stream`'s
/// flow-control policy.
const DEFAULT_MAX_SESSION_MEMORY_MB: usize = 10;

/// Bind and accept HTTP/2 on the agent's turnloop loop, when this thread has
/// one. Returns the listener id and the bound port, or `None` when the caller
/// must keep the hyper path.
///
/// Two reasons to decline are left: a thread acting for an agent another thread
/// already owns has no loop, and a `createSecureServer` with no usable TLS
/// material has nothing to install.
///
/// The cluster worker was a third and is not any more. It declined because it
/// needed the `SO_REUSEPORT` bind only the hyper path's
/// `std::net::TcpListener` could do; turnloop 0.1.0-alpha.6's
/// `ReusePort::Share` is that same bind, so the worker takes this path now.
/// `http2.createServer` has no SCHED_RR fd-inject loop at all (only
/// `http.createServer` does), so the fd-passing half that is still open for
/// plain HTTP never arises here.
pub(super) fn try_listen_on_turnloop(
    server_handle: i64,
    host: &str,
    port: u16,
) -> Option<(i64, u16, String)> {
    if !crate::server::turnloop_h2::enabled() {
        return None;
    }
    let reuse_port = crate::server::cluster_bind::is_cluster_worker();
    // `noDelay` is read here, under the same handle borrow as the TLS config
    // and the settings, because the turnloop listener applies it once at bind
    // time rather than per accepted socket. The hyper HTTP/2 path reads the
    // same field (`http2_server.rs`) and applies it per connection; both honour
    // `server.noDelay()`, which Node defaults to true.
    let (tls, plaintext, settings, allow_http1, no_delay) = {
        let server = get_handle::<Http2SecureServer>(server_handle)?;
        (
            server.tls_config.clone(),
            server.plaintext,
            server.settings.clone(),
            server.allow_http1,
            server.base.no_delay,
        )
    };
    if !plaintext && tls.is_none() {
        // `js_node_http2_create_secure_server` already reported why; refusing
        // here as well would print it twice, and the hyper path refuses too.
        return None;
    }
    let tls = if plaintext { None } else { tls };
    match crate::server::turnloop_h2::listen(
        server_handle,
        host,
        port,
        511,
        tls,
        allow_http1,
        settings,
        DEFAULT_MAX_SESSION_MEMORY_MB * 1024 * 1024,
        reuse_port,
        no_delay,
    ) {
        Ok((id, bound_port, bound_host)) => {
            crate::server::cluster_bind::notify_listening(host, bound_port);
            let server = get_handle_mut::<Http2SecureServer>(server_handle)?;
            server.base.bound_port = bound_port;
            server.base.bound_host = host.to_string();
            server.base.listening = true;
            server.turnloop_listener = id;
            Some((id, bound_port, bound_host))
        }
        Err(err) if err.no_loop => None,
        Err(err) => {
            eprintln!(
                "[node:http2] bind {}:{} failed: {}",
                host,
                port,
                err.message()
            );
            // Returning `None` would send the hyper path at the same address to
            // fail the same way; the failure is reported once and the listen
            // ends here. (The missing `'error'` event is P5's open defect, not
            // this path's — see `docs/turnloop/http2b-report.md`.)
            Some((0, port, host.to_string()))
        }
    }
}

/// `server.close()` on a turnloop HTTP/2 listener: stop accepting. Live
/// sessions finish, which is Node's contract.
pub(super) fn close_turnloop_listener(server_handle: i64) -> bool {
    let id = match get_handle_mut::<Http2SecureServer>(server_handle) {
        Some(server) if server.turnloop_listener != 0 => {
            std::mem::replace(&mut server.turnloop_listener, 0)
        }
        _ => return false,
    };
    crate::server::turnloop_h2::close_listener(id);
    true
}

/// `options.settings` and `options.allowHTTP1` on `createServer` /
/// `createSecureServer`, read once at construction because the options object
/// is not kept until `listen()`.
///
/// `settings` merges over the RFC 9113 defaults the same way
/// `session.settings()` merges.
pub(super) fn server_options(opts: f64) -> (Http2SettingsState, bool) {
    let mut settings = Http2SettingsState::default();
    // Node's own server default for MAX_CONCURRENT_STREAMS is 100; the
    // unlimited `u32::MAX` in `Http2SettingsState::default` is the *protocol*
    // default, which is not what a server advertises.
    settings.max_concurrent_streams = 100;
    let mut allow_http1 = false;
    let value = perry_ffi::JsValue::from_bits(opts.to_bits());
    if value.is_pointer() {
        if let Some(json) = perry_ffi::json_stringify(value) {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json) {
                if let Some(inner) = parsed.get("settings") {
                    settings.apply_json(inner);
                }
                allow_http1 = parsed
                    .get("allowHTTP1")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
            }
        }
    }
    (settings, allow_http1)
}
