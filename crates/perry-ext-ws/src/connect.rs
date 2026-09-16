//! The outbound client connect, over the tokio transport.
//!
//! This replaces `tokio_tungstenite::connect_async`, which did four things in
//! one call: parse the URL, open the TCP connection, negotiate TLS for `wss://`,
//! and run the handshake. Only the third is not already in the tree — and it is
//! `perry-ext-net`'s, reached through [`perry_ext_net::connect_tls_client`] so
//! this crate never names a TLS stack of its own. The handshake is
//! [`crate::handshake`], which needs no stream.

use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

use crate::codec::{Codec, Role};
use crate::handshake::ClientUpgrade;
use crate::io::Transport;

/// A connected, handshaken WebSocket and whatever frame bytes rode along with
/// the `101`.
pub(crate) struct Connected {
    pub stream: Box<dyn Transport>,
    pub codec: Codec,
    pub leftover: Vec<u8>,
}

struct Target {
    secure: bool,
    host: String,
    port: u16,
    authority: String,
    path: String,
}

fn parse(url: &str) -> Result<Target, String> {
    let parsed = url::Url::parse(url).map_err(|e| format!("Invalid URL: {e}"))?;
    let secure = match parsed.scheme() {
        "ws" | "http" => false,
        "wss" | "https" => true,
        other => {
            return Err(format!(
                "The URL's protocol must be one of \"ws:\", \"wss:\", \"http:\", \"https:\", or \"ws+unix:\" (got \"{other}:\")"
            ))
        }
    };
    let host = parsed
        .host_str()
        .ok_or_else(|| "Invalid URL: no host".to_string())?
        .to_string();
    let port = parsed
        .port_or_known_default()
        .unwrap_or(if secure { 443 } else { 80 });
    // `ws` sends the default port implicitly, like a browser.
    let authority = match parsed.port() {
        Some(explicit) => format!("{host}:{explicit}"),
        None => host.clone(),
    };
    let mut path = parsed.path().to_string();
    if path.is_empty() {
        path.push('/');
    }
    if let Some(query) = parsed.query() {
        path.push('?');
        path.push_str(query);
    }
    Ok(Target {
        secure,
        host,
        port,
        authority,
        path,
    })
}

/// Connect, upgrade, and hand back a stream carrying frames.
pub(crate) async fn connect(
    url: &str,
    protocols: Vec<String>,
    headers: Vec<(String, String)>,
) -> Result<Connected, String> {
    let target = parse(url)?;
    let tcp = tokio::net::TcpStream::connect((target.host.as_str(), target.port))
        .await
        .map_err(|e| format!("connect ECONNREFUSED: {e}"))?;
    // Node's `ws` sets TCP_NODELAY on its sockets; a handshake that sat in
    // Nagle's queue would add a round trip to every connect.
    let _ = tcp.set_nodelay(true);
    let mut stream: Box<dyn Transport> = if target.secure {
        // `Box<dyn TlsClientStream>` is itself a `Transport` (tokio implements
        // AsyncRead/AsyncWrite for Box), so the extra box costs one indirection
        // and keeps every TLS type name inside perry-ext-net.
        let tls = perry_ext_net::connect_tls_client(tcp, &target.host)
            .await
            .map_err(|e| format!("TLS handshake failed: {e}"))?;
        Box::new(tls)
    } else {
        Box::new(tcp)
    };

    let mut nonce = [0u8; 16];
    // RFC 6455 §4.1: the nonce must be unpredictable, not merely unique.
    secure_random(&mut nonce)?;
    let (mut upgrade, request) =
        ClientUpgrade::start(&target.authority, &target.path, nonce, protocols, &headers)
            .map_err(|e| e.message)?;
    stream
        .write_all(&request)
        .await
        .map_err(|e| format!("write: {e}"))?;

    let mut buffer = vec![0u8; 16 * 1024];
    loop {
        let n = stream
            .read(&mut buffer)
            .await
            .map_err(|e| format!("read: {e}"))?;
        if n == 0 {
            return Err("socket hang up before the upgrade completed".to_string());
        }
        if let Some(upgraded) = upgrade.receive(&buffer[..n]).map_err(|e| e.message)? {
            return Ok(Connected {
                stream,
                codec: Codec::new(Role::Client),
                leftover: upgraded.leftover,
            });
        }
    }
}

/// RFC 6455 §4.1's unpredictable nonce, from the same crypto provider the TLS
/// path installs. `ensure_tls_crypto_provider` has already run by the time any
/// connect reaches here, so the default is normally already set.
fn secure_random(out: &mut [u8]) -> Result<(), String> {
    use std::sync::OnceLock;
    static PROVIDER: OnceLock<std::sync::Arc<rustls::crypto::CryptoProvider>> = OnceLock::new();
    let provider = PROVIDER.get_or_init(|| {
        rustls::crypto::CryptoProvider::get_default()
            .cloned()
            .unwrap_or_else(|| std::sync::Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
    });
    provider
        .secure_random
        .fill(out)
        .map_err(|_| "no secure random source".to_string())
}
