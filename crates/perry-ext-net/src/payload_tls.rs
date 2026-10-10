//! Sans-I/O TLS in SocketFields. Its promise and metadata are ordinary JS edges.

use super::payload_transport::{self as p, TlsLayer, TlsWrite};
use super::{payload_events as events, payload_io as io, payload_socket as socket};
use perry_ffi::native_payload::OwnerLink;
use perry_ffi::turnloop_net as tl;
use perry_ffi::{JsValue, TransientRootScope};
use std::collections::VecDeque;

fn attach_tls_prototype(owner: f64) {
    extern "C" {
        fn js_tls_client_attach_socket_prototype(owner: i64);
    }
    unsafe {
        js_tls_client_attach_socket_prototype(p::raw_owner(owner));
    }
}

pub(crate) fn installed(link: OwnerLink) -> bool {
    unsafe {
        p::socket_ptr(link)
            .ok()
            .is_some_and(|payload| (*payload).ext.tls.is_some())
    }
}

pub(crate) fn install_client(
    owner: f64,
    servername: String,
    verify: bool,
    config: crate::TlsClientConfigData,
) -> Result<(), String> {
    let client_config = crate::tls::build_client_config(verify, Some(&config))?;
    let server_name = crate::turnloop_tls::server_name(&servername)?;
    install_client_config(
        owner,
        client_config,
        server_name,
        servername,
        verify,
        config,
    )
}

/// HTTP supplies its cached verifier/configuration, retaining session
/// resumption while the Socket owns the records through a later upgrade.
pub(crate) fn install_client_config(
    owner: f64,
    client_config: std::sync::Arc<rustls::ClientConfig>,
    server_name: rustls::pki_types::ServerName<'static>,
    servername: String,
    verify: bool,
    config: crate::TlsClientConfigData,
) -> Result<(), String> {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let link = socket::link(owner.get());
    let _account = p::AccountSocket(link);
    if installed(link) {
        return Err("socket is already TLS".into());
    }
    let session = crate::turnloop_tls::TlsSession::client(client_config, server_name)?;
    unsafe {
        (*p::socket_ptr(link).map_err(|_| "socket is closed")?)
            .ext
            .tls = Some(Box::new(TlsLayer {
            session,
            servername,
            verify,
            config,
            cipher_written: 0,
            cipher_acked: 0,
            pending: VecDeque::new(),
            pending_shutdown: None,
            secure_emitted: false,
            closing: false,
            server: false,
        }));
    }
    attach_tls_prototype(owner.get());
    let started = unsafe {
        p::socket_ptr(link).is_ok_and(|payload| (*payload).ext.opened || (*payload).ext.connecting)
    };
    if started {
        drive(owner.get());
    }
    Ok(())
}

pub(crate) fn install_server(
    owner: f64,
    config: std::sync::Arc<rustls::ServerConfig>,
) -> Result<(), String> {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let link = socket::link(owner.get());
    let _account = p::AccountSocket(link);
    if installed(link) {
        return Err("socket is already TLS".into());
    }
    let session = crate::turnloop_tls::TlsSession::server(config)?;
    unsafe {
        (*p::socket_ptr(link).map_err(|_| "socket is closed")?)
            .ext
            .tls = Some(Box::new(TlsLayer {
            session,
            servername: String::new(),
            verify: true,
            config: crate::TlsClientConfigData::default(),
            cipher_written: 0,
            cipher_acked: 0,
            pending: VecDeque::new(),
            pending_shutdown: None,
            secure_emitted: false,
            closing: false,
            server: true,
        }));
    }
    attach_tls_prototype(owner.get());
    Ok(())
}

pub(crate) fn queued(link: OwnerLink) -> usize {
    unsafe {
        p::socket_ptr(link)
            .ok()
            .and_then(|payload| (*payload).ext.tls.as_ref())
            .map_or(0, |layer| {
                layer.pending.iter().map(|write| write.plain_len).sum()
            })
    }
}

pub(crate) fn write(owner: f64, bytes: &[u8], user: u64) -> Result<usize, tl::NetError> {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let window = unsafe { super::native_transport::transport_window(owner.get()) }
        .map_err(|_| bad_fd("write"))?;
    let _account = p::AccountSocket(window.link);
    unsafe { write_proven(owner.get(), window, bytes, user) }
}

/// Submit through the caller's proven payload. `owner` is rooted and the
/// window is current; the caller accounts retained bytes after submission.
/// No JS may run before the native borrow ends. TLS drive
/// runs only after that borrow, and may close or reopen the owner.
pub(crate) unsafe fn write_proven(
    owner: f64,
    window: perry_ffi::native_payload::PayloadWindow<p::SocketPayload>,
    bytes: &[u8],
    user: u64,
) -> Result<usize, tl::NetError> {
    let link = window.link;
    {
        let payload = &mut *window.payload;
        let fields = &mut payload.ext;
        if let Some(layer) = fields.tls.as_mut() {
            layer.session.write(bytes);
            layer.pending.push_back(TlsWrite {
                user,
                plain_len: bytes.len(),
                mark: None,
            });
        } else if fields.direct_tls.is_some() {
            fields.held_tls_writes.push((bytes.to_vec(), user));
            return Ok(fields
                .held_tls_writes
                .iter()
                .map(|write| write.0.len())
                .sum());
        } else {
            return tl::link_write(&mut payload.core, link, bytes, user);
        }
    }
    drive(owner);
    Ok(queued(link))
}

pub(crate) fn shutdown(owner: f64, user: u64) -> Result<(), tl::NetError> {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let link = socket::link(owner.get());
    let _account = p::AccountSocket(link);
    unsafe {
        let fields = &mut (*p::socket_ptr(link).map_err(|_| bad_fd("shutdown"))?).ext;
        if let Some(layer) = fields.tls.as_mut() {
            layer.session.close_notify();
            layer.pending_shutdown = Some(user);
            layer.closing = true;
        } else if fields.direct_tls.is_some() {
            fields.held_tls_end = Some(user);
            return Ok(());
        } else {
            return tl::link_shutdown(
                &mut *p::socket_core(link).map_err(|_| bad_fd("shutdown"))?,
                link,
                user,
            );
        }
    }
    drive(owner.get());
    Ok(())
}

fn bad_fd(syscall: &str) -> tl::NetError {
    tl::NetError {
        code: "EBADF".into(),
        syscall: syscall.into(),
        errno: -9,
        no_loop: false,
    }
}

pub(crate) fn begin_connected(owner: f64) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let link = socket::link(owner.get());
    let _account = p::AccountSocket(link);
    let Some(snapshot) = io::snapshot(link) else {
        return;
    };
    let initial = unsafe {
        p::socket_ptr(link)
            .ok()
            .and_then(|payload| (*payload).ext.direct_tls.take())
    };
    let Some((servername, verify, config)) = initial else {
        if installed(link) {
            drive(owner.get());
        }
        return;
    };
    if let Err(message) = install_client(owner.get(), servername, verify, config) {
        if io::matches(link, &snapshot) {
            fail(owner.get(), &message, false);
        }
        return;
    }
    // Installation and every held write can drive TLS listeners. Old held
    // writes must never flow into a replacement Socket incarnation.
    if !io::matches(link, &snapshot) {
        return;
    }
    let pending = unsafe {
        p::socket_ptr(link).ok().map(|payload| {
            (
                std::mem::take(&mut (*payload).ext.held_tls_writes),
                (*payload).ext.held_tls_end.take(),
            )
        })
    };
    if let Some((writes, end)) = pending {
        for (bytes, user) in writes {
            let result = write(owner.get(), &bytes, user);
            if !io::matches(link, &snapshot) {
                return;
            }
            if let Err(error) = result {
                fail(owner.get(), &error.message(), false);
                return;
            }
        }
        if let Some(user) = end {
            let result = shutdown(owner.get(), user);
            if !io::matches(link, &snapshot) {
                return;
            }
            if let Err(error) = result {
                fail(owner.get(), &error.message(), false);
            }
        }
    }
}

pub(crate) fn receive(owner: f64, bytes: &[u8]) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let link = socket::link(owner.get());
    let _account = p::AccountSocket(link);
    unsafe {
        if let Ok(payload) = p::socket_ptr(link) {
            if let Some(layer) = (*payload).ext.tls.as_mut() {
                layer.session.receive(bytes);
            }
        }
    }
    drive(owner.get());
}

pub(crate) fn wrote(owner: f64, len: usize) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let link = socket::link(owner.get());
    let _account = p::AccountSocket(link);
    let Some(snapshot) = io::snapshot(link) else {
        return;
    };
    let done = unsafe {
        p::socket_ptr(link).ok().and_then(|payload| {
            let layer = (*payload).ext.tls.as_mut()?;
            layer.cipher_acked = layer.cipher_acked.saturating_add(len as u64);
            let mut done = Vec::new();
            while layer
                .pending
                .front()
                .is_some_and(|write| write.mark.is_some_and(|mark| mark <= layer.cipher_acked))
            {
                // The successful front predicate and pop share one exclusive
                // borrow with no callback or queue mutation between them.
                let write = layer
                    .pending
                    .pop_front()
                    .expect("observed TLS pending front");
                done.push((write.user, write.plain_len));
            }
            Some(done)
        })
    };
    for (user, plain_len) in done.unwrap_or_default() {
        if !io::matches(link, &snapshot) {
            return;
        }
        io::wrote(owner.get(), user, plain_len, queued(link));
    }
}

/// Complete every native borrow before publishing facts or emitting JS.
fn drive(owner: f64) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let link = socket::link(owner.get());
    let _account = p::AccountSocket(link);
    let Some(snapshot) = io::snapshot(link) else {
        return;
    };
    let driven = unsafe {
        p::socket_ptr(link).ok().and_then(|payload| {
            let layer = (*payload).ext.tls.as_mut()?;
            let progress = layer.session.pump();
            let output = layer.session.take_output();
            if !output.is_empty() {
                layer.cipher_written = layer.cipher_written.saturating_add(output.len() as u64);
                for write in layer
                    .pending
                    .iter_mut()
                    .filter(|write| write.mark.is_none())
                {
                    write.mark = Some(layer.cipher_written);
                }
            }
            let end = if layer.session.close_sent() {
                layer.pending_shutdown.take()
            } else {
                None
            };
            let facts = if progress.handshake_done && !layer.secure_emitted {
                layer.secure_emitted = true;
                Some(crate::tls::HandshakeFacts::from_session(
                    &layer.session,
                    &layer.servername,
                    layer.verify,
                    Some(&layer.config),
                ))
            } else {
                None
            };
            layer.closing |= progress.peer_closed;
            Some((
                output,
                end,
                facts,
                layer.server,
                layer.session.take_plaintext(),
                progress.peer_closed,
                layer.session.failure().map(str::to_owned),
                layer.closing,
            ))
        })
    };
    let Some((output, end, facts, server, plain, eof, failure, closing)) = driven else {
        return;
    };
    if !output.is_empty() {
        if let Err(error) = unsafe {
            let Ok(core) = p::socket_core(link) else {
                return;
            };
            if !tl::link_handle_matches(&mut *core, link, &snapshot) {
                return;
            }
            tl::link_write(&mut *core, link, &output, 0)
        } {
            fail(owner.get(), &error.message(), closing);
            return;
        }
    }
    if let Some(user) = end {
        if let Err(error) = unsafe {
            let Ok(core) = p::socket_core(link) else {
                return;
            };
            if !tl::link_handle_matches(&mut *core, link, &snapshot) {
                return;
            }
            tl::link_shutdown(&mut *core, link, user)
        } {
            fail(owner.get(), &error.message(), closing);
            return;
        }
    }
    if let Some(message) = failure {
        fail(owner.get(), &message, closing);
        return;
    }
    if let Some(facts) = facts {
        publish(owner.get(), &facts);
        if !server {
            extern "C" {
                fn js_tls_client_check_identity_from_metadata(owner: i64) -> f64;
            }
            let failure = scope.root_nanbox(unsafe {
                js_tls_client_check_identity_from_metadata(p::raw_owner(owner.get()))
            });
            if !io::matches(link, &snapshot) {
                return;
            }
            if !JsValue::from_bits(failure.get().to_bits()).is_undefined() {
                settle_upgrade(owner.get(), Some("TLS identity callback rejected the peer"));
                socket::destroy(owner.get(), failure.get());
                return;
            }
        }
        if !io::matches(link, &snapshot) {
            return;
        }
        settle_upgrade(owner.get(), None);
        events::emit(
            owner.get(),
            if server { "secure" } else { "secureConnect" },
            &[],
        );
    }
    if !io::matches(link, &snapshot) {
        return;
    }
    if !plain.is_empty() {
        // The handshake's secure listener can change the codec. Read its
        // current route after JS, preserving the cell/read token/ref count.
        unsafe {
            if let Ok(core) = p::socket_core(link) {
                let _ = tl::link_dispatch_plaintext(core, link, &plain, false);
            }
        }
    }
    if eof && io::matches(link, &snapshot) {
        unsafe {
            if let Ok(core) = p::socket_core(link) {
                let _ = tl::link_dispatch_plaintext(core, link, &[], true);
            }
        }
    }
}

fn publish(owner: f64, facts: &crate::tls::HandshakeFacts) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let state = scope.root_nanbox(socket::state(owner.get()));
    p::record_set(
        state.get(),
        "tlsConnected",
        f64::from_bits(JsValue::TRUE.bits()),
    );
    p::record_set(
        state.get(),
        "encrypted",
        f64::from_bits(JsValue::TRUE.bits()),
    );
    p::record_set(
        state.get(),
        "authorized",
        f64::from_bits(JsValue::from_bool(facts.authorized).bits()),
    );
    if let Some((name, standard)) = facts.cipher.as_ref() {
        let cipher = scope.root_nanbox(f64::from_bits(perry_ffi::alloc_object().bits()));
        p::own_set(cipher.get(), "name", events::string(name));
        p::own_set(cipher.get(), "standardName", events::string(standard));
        p::own_set(cipher.get(), "version", events::string(facts.protocol));
        p::record_set(state.get(), "cipher", cipher.get());
    }
    p::record_set(state.get(), "protocol", events::string(facts.protocol));
    p::record_set(state.get(), "servername", events::string(&facts.servername));
    p::record_set(
        state.get(),
        "alpnProtocol",
        if facts.alpn.is_empty() {
            f64::from_bits(JsValue::FALSE.bits())
        } else {
            events::string(&String::from_utf8_lossy(&facts.alpn))
        },
    );
    p::record_set(
        state.get(),
        "authorizationError",
        if facts.authorized {
            p::undefined()
        } else {
            events::string("DEPTH_ZERO_SELF_SIGNED_CERT")
        },
    );
    p::record_set(
        state.get(),
        "peerCertificateDer",
        f64::from_bits(JsValue::from_object_ptr(perry_ffi::alloc_buffer(&facts.peer)).bits()),
    );
    p::record_set(
        state.get(),
        "ownCertificateDer",
        f64::from_bits(
            JsValue::from_object_ptr(perry_ffi::alloc_buffer(&facts.own_certificate)).bits(),
        ),
    );
}

pub(crate) fn settle_upgrade(owner: f64, failure: Option<&str>) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let state = scope.root_nanbox(socket::state(owner.get()));
    let promise = scope.root_nanbox(p::record_get(state.get(), "upgradePromise"));
    if !JsValue::from_bits(promise.get().to_bits()).is_pointer() {
        return;
    }
    p::record_set(state.get(), "upgradePromise", p::undefined());
    let promise = unsafe {
        perry_ffi::JsPromise::from_raw(
            JsValue::from_bits(promise.get().to_bits()).as_pointer::<perry_ffi::Promise>(),
        )
    };
    match failure {
        Some(message) => promise.reject_string(message),
        None => promise.resolve_undefined(),
    }
}

fn fail(owner: f64, message: &str, closing: bool) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    settle_upgrade(owner.get(), Some(message));
    let error = if closing {
        p::undefined()
    } else {
        {
            let (code, text) = message
                .split_once(':')
                .unwrap_or(("ERR_TLS_HANDSHAKE_FAILED", message));
            socket::error(code, &node_failure_message(code, text.trim_start()))
        }
    };
    socket::destroy(owner.get(), error);
}

/// Node's message for a handshake failure's cause code. Node reports these as
/// an `Error` carrying `.code` and OpenSSL's text; rustls has its own text, so
/// the codes Node users test for get Node's words and anything else keeps
/// rustls's.
fn node_failure_message(code: &str, rustls_message: &str) -> String {
    match code {
        "UNABLE_TO_VERIFY_LEAF_SIGNATURE" => "unable to verify the first certificate".to_string(),
        "CERT_HAS_EXPIRED" => "certificate has expired".to_string(),
        "CERT_NOT_YET_VALID" => "certificate is not yet valid".to_string(),
        "CERT_REVOKED" => "certificate revoked".to_string(),
        "ERR_TLS_CERT_ALTNAME_INVALID" => {
            format!("Hostname/IP does not match certificate's altnames: {rustls_message}")
        }
        _ => rustls_message.to_string(),
    }
}

#[cfg(test)]
mod node_text_tests {
    use super::node_failure_message;

    #[test]
    fn node_codes_get_node_text() {
        assert_eq!(
            node_failure_message("UNABLE_TO_VERIFY_LEAF_SIGNATURE", "x"),
            "unable to verify the first certificate"
        );
        assert_eq!(
            node_failure_message("ERR_SSL_PROTOCOL_ERROR", "rustls text"),
            "rustls text"
        );
    }
}
