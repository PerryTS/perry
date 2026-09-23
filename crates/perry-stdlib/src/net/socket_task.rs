//! The per-socket tokio task: connect, optional TLS handshake, and the
//! read/write/command loop that drives a `net.Socket` handle.
//!
//! Split out of `net/mod.rs` (2000-line file cap). Pure move — the
//! functions keep their names, signatures and behaviour; only the two
//! `net` still calls widened to `pub(super)`.

use std::collections::HashMap;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::common::async_bridge::spawn;

use super::{
    ensure_gc_scanner_registered, mark_closed, next_id, push_event, PendingNetEvent, SocketCommand,
    SocketState, TlsClientConfigData, Transport, NET_LISTENERS, NET_SOCKETS,
};

#[cfg(feature = "tls")]
use crate::tls_stream::TlsStream;

#[cfg(feature = "tls")]
use super::tls_config::build_tls_connector;

/// Internal: allocate the handle, spawn the tokio task.
/// `direct_tls` = Some((servername, verify)) runs a TLS handshake before
/// firing 'connect'; None keeps the socket in plain TCP mode.
pub(super) fn spawn_socket_task(
    host: String,
    port: u16,
    direct_tls: Option<(String, bool, TlsClientConfigData)>,
) -> i64 {
    ensure_gc_scanner_registered();
    let id = next_id();
    let (tx, mut rx) = mpsc::unbounded_channel::<SocketCommand>();

    NET_SOCKETS.lock().unwrap().insert(
        id,
        SocketState {
            cmd_tx: tx,
            pending_rx: None,
            is_open: false,
            type_of_service: 0,
        },
    );
    NET_LISTENERS.lock().unwrap().insert(id, HashMap::new());

    spawn(async move {
        let addr = format!("{}:{}", host, port);
        let tcp = match TcpStream::connect(&addr).await {
            Ok(s) => s,
            Err(e) => {
                push_event(PendingNetEvent::Error(id, format!("{}", e)));
                push_event(PendingNetEvent::Close(id));
                mark_closed(id);
                return;
            }
        };

        // Direct-TLS path: run the TLS handshake before signalling connect.
        let transport = match direct_tls {
            #[cfg(feature = "tls")]
            Some((servername, verify, config)) => {
                match do_tls_handshake(tcp, &servername, verify, Some(&config)).await {
                    Ok(tls) => {
                        record_tls_handshake(id, &tls, verify, Some(&config));
                        Transport::Tls(Box::new(tls))
                    }
                    Err(e) => {
                        push_event(PendingNetEvent::Error(id, e));
                        push_event(PendingNetEvent::Close(id));
                        mark_closed(id);
                        return;
                    }
                }
            }
            #[cfg(not(feature = "tls"))]
            Some(_) => {
                push_event(PendingNetEvent::Error(
                    id,
                    "tls feature not compiled in".to_string(),
                ));
                push_event(PendingNetEvent::Close(id));
                mark_closed(id);
                return;
            }
            None => Transport::Plain(tcp),
        };

        if let Some(s) = NET_SOCKETS.lock().unwrap().get_mut(&id) {
            s.is_open = true;
        }
        push_event(PendingNetEvent::Connect(id));

        run_socket_task(id, transport, &mut rx).await;
    });

    id
}

#[cfg(feature = "tls")]
async fn do_tls_handshake(
    tcp: TcpStream,
    servername: &str,
    verify: bool,
    data: Option<&TlsClientConfigData>,
) -> Result<TlsStream<TcpStream>, String> {
    let connector = build_tls_connector(verify, data)?;
    let server_name = rustls::pki_types::ServerName::try_from(servername.to_string())
        .map_err(|e| format!("invalid servername '{}': {}", servername, e))?;
    TlsStream::connect(tcp, connector, server_name)
        .await
        .map_err(|e| format!("tls handshake: {}", e))
}

#[cfg(feature = "tls")]
fn record_tls_handshake(
    handle: i64,
    stream: &TlsStream<TcpStream>,
    verify: bool,
    data: Option<&TlsClientConfigData>,
) {
    let connection = stream.session();
    let protocol = match connection.protocol_version() {
        Some(rustls::ProtocolVersion::TLSv1_2) => "TLSv1.2",
        Some(rustls::ProtocolVersion::TLSv1_3) => "TLSv1.3",
        _ => "",
    };
    let alpn = connection.alpn_protocol().unwrap_or_default();
    let peer = connection
        .peer_certificates()
        .and_then(|certs| certs.first())
        .map(|cert| cert.as_ref())
        .unwrap_or_default();
    let trusted_by_configured_ca =
        data.and_then(|data| data.ca.as_ref())
            .is_some_and(|materials| {
                materials.iter().any(|material| {
                    let mut cursor = std::io::Cursor::new(material);
                    let trusted = rustls_pemfile::certs(&mut cursor)
                        .flatten()
                        .any(|cert| cert.as_ref() == peer);
                    trusted
                })
            });
    let authorized = verify || trusted_by_configured_ca;
    let authorization_error = if authorized {
        ""
    } else {
        "DEPTH_ZERO_SELF_SIGNED_CERT"
    };
    let own_certificate = data
        .map(|data| {
            let mut cursor = std::io::Cursor::new(&data.cert);
            let certificate = rustls_pemfile::certs(&mut cursor)
                .flatten()
                .next()
                .map(|cert| cert.as_ref().to_vec())
                .unwrap_or_default();
            certificate
        })
        .unwrap_or_default();
    unsafe {
        perry_runtime::tls::js_tls_client_record_connected(
            handle,
            authorized as i32,
            authorization_error.as_ptr(),
            authorization_error.len(),
            protocol.as_ptr(),
            protocol.len(),
            alpn.as_ptr(),
            alpn.len(),
            peer.as_ptr(),
            peer.len(),
            own_certificate.as_ptr(),
            own_certificate.len(),
        );
    }
}

/// The read/write/command loop. Shared by plain-TCP and direct-TLS paths.
pub(super) async fn run_socket_task(
    id: i64,
    initial_transport: Transport,
    rx: &mut mpsc::UnboundedReceiver<SocketCommand>,
) {
    let mut transport: Option<Transport> = Some(initial_transport);
    let mut buf = vec![0u8; 16 * 1024];

    loop {
        let t = match transport.as_mut() {
            Some(t) => t,
            None => break, // transport taken and not restored → end task
        };

        tokio::select! {
            read_result = t.read(&mut buf) => {
                match read_result {
                    Ok(0) => {
                        // Node's default `allowHalfOpen: false` closes the
                        // writable side after peer EOF. On TLS transports this
                        // also sends close_notify instead of making the peer
                        // report an unclean close without an `end` event.
                        let _ = t.shutdown().await;
                        push_event(PendingNetEvent::End(id));
                        push_event(PendingNetEvent::Close(id));
                        mark_closed(id);
                        break;
                    }
                    Ok(n) => {
                        push_event(PendingNetEvent::Data(id, buf[..n].to_vec()));
                    }
                    Err(e) => {
                        push_event(PendingNetEvent::Error(id, format!("{}", e)));
                        push_event(PendingNetEvent::Close(id));
                        mark_closed(id);
                        break;
                    }
                }
            }
            cmd = rx.recv() => {
                match cmd {
                    Some(SocketCommand::Write(bytes)) => {
                        if let Err(e) = t.write_all(&bytes).await {
                            push_event(PendingNetEvent::Error(id, format!("{}", e)));
                            push_event(PendingNetEvent::Close(id));
                            mark_closed(id);
                            break;
                        }
                    }
                    Some(SocketCommand::End) => {
                        let _ = t.shutdown().await;
                    }
                    Some(SocketCommand::Destroy) | None => {
                        push_event(PendingNetEvent::Close(id));
                        mark_closed(id);
                        break;
                    }
                    #[cfg(feature = "tls")]
                    Some(SocketCommand::UpgradeTls { servername, verify, config, reply }) => {
                        // Take the plain TcpStream out of the enum, run the
                        // handshake, and put a TlsStream back under the same id.
                        // Done inline (blocks reads until handshake completes),
                        // which is what the Postgres SSLRequest flow expects.
                        let old = transport.take();
                        match old {
                            Some(Transport::Plain(tcp)) => {
                                match do_tls_handshake(tcp, &servername, verify, Some(&config)).await {
                                    Ok(tls) => {
                                        record_tls_handshake(id, &tls, verify, Some(&config));
                                        transport = Some(Transport::Tls(Box::new(tls)));
                                        crate::tls::record_tls_client_handle(id);
                                        let _ = reply.send(Ok(()));
                                        push_event(PendingNetEvent::SecureConnect(id));
                                    }
                                    Err(e) => {
                                        let _ = reply.send(Err(e.clone()));
                                        push_event(PendingNetEvent::Error(id, e));
                                        push_event(PendingNetEvent::Close(id));
                                        mark_closed(id);
                                        break;
                                    }
                                }
                            }
                            Some(already_tls @ Transport::Tls(_)) => {
                                transport = Some(already_tls);
                                let _ = reply.send(Err("socket is already TLS".to_string()));
                            }
                            None => {
                                let _ = reply.send(Err("socket closed".to_string()));
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
}
