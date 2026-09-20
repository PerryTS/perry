//! Classify a client transport failure into the Node `Error` shape.
//!
//! Node hands `request.on('error')` a real `Error` carrying `.code`
//! (`ECONNREFUSED`, `ENOTFOUND`, …), `.syscall` (`connect` / `getaddrinfo`)
//! and `.errno` (the libuv-negative number), with a message like
//! `connect ECONNREFUSED 127.0.0.1:1` or `getaddrinfo ENOTFOUND host`.
//! Perry previously passed the bare `reqwest::Error::to_string()` text, which
//! (a) doesn't even contain the OS reason — that lives in the error's
//! `source()` chain, not its `Display` — and (b) reaches listeners as a plain
//! string, so `err.code === 'ECONNREFUSED'` checks (every HTTP client library
//! does them) saw `undefined`.
//!
//! Only the failure modes Perry can name with confidence are classified;
//! anything else returns `None` so the caller keeps the legacy string path
//! (which still maps `ECONNRESET` / `socket hang up` via `error_event_arg`).

use std::error::Error as StdError;
use std::io::ErrorKind;

/// `(message, code, syscall, errno)` describing a Node-shaped transport error.
pub(crate) type Classified = (String, String, String, i64);

/// Host (and explicit-or-default port) parsed from an http(s) URL, for the
/// `connect <CODE> <host>:<port>` message form.
fn host_port(url: &str) -> (String, Option<u16>) {
    match reqwest::Url::parse(url) {
        Ok(u) => (
            u.host_str().unwrap_or_default().to_string(),
            u.port_or_known_default(),
        ),
        Err(_) => (String::new(), None),
    }
}

/// Platform errno (libuv-negative) for a code, used only when no concrete
/// `std::io::Error` surfaced in the source chain to read `raw_os_error()`.
///
/// Three platforms, not two. This table used to be a macOS-vs-else pair, which
/// silently handed Windows the LINUX numbers (`ECONNABORTED` as -103 rather
/// than -4079) — libuv on Windows uses its own `-4xxx` space, unrelated to the
/// host errno, so no value here was right. Windows values read from the pinned
/// oracle: `node -e "require('util').getSystemErrorMap()"` on 26.5.1.
fn fallback_errno(code: &str) -> i64 {
    #[cfg(windows)]
    {
        return match code {
            "ECONNREFUSED" => -4078,
            "ETIMEDOUT" => -4039,
            "ECONNABORTED" => -4079,
            "ECONNRESET" => -4077,
            "EADDRNOTAVAIL" => -4090,
            "EHOSTUNREACH" => -4073,
            "ENETUNREACH" => -4062,
            _ => 0,
        };
    }
    #[cfg(not(windows))]
    {
        match code {
            "ECONNREFUSED" => {
                if cfg!(target_os = "macos") {
                    -61
                } else {
                    -111
                }
            }
            "ETIMEDOUT" => {
                if cfg!(target_os = "macos") {
                    -60
                } else {
                    -110
                }
            }
            "ECONNABORTED" => {
                if cfg!(target_os = "macos") {
                    -53
                } else {
                    -103
                }
            }
            "ECONNRESET" => {
                if cfg!(target_os = "macos") {
                    -54
                } else {
                    -104
                }
            }
            "EADDRNOTAVAIL" => {
                if cfg!(target_os = "macos") {
                    -49
                } else {
                    -99
                }
            }
            "EHOSTUNREACH" => {
                if cfg!(target_os = "macos") {
                    -65
                } else {
                    -113
                }
            }
            "ENETUNREACH" => {
                if cfg!(target_os = "macos") {
                    -51
                } else {
                    -101
                }
            }
            _ => 0,
        }
    }
}

/// libuv's errno for a code, given whatever raw OS errno was recovered.
///
/// Negating the raw OS errno is correct on Linux/macOS, where libuv's errno IS
/// the negated host errno. On Windows it is never correct — libuv uses its own
/// `-4xxx` space — so there the code name is authoritative and the raw Winsock
/// number is discarded.
fn libuv_errno(code: &str, raw: Option<i32>) -> i64 {
    #[cfg(windows)]
    {
        let _ = raw;
        return fallback_errno(code);
    }
    #[cfg(not(windows))]
    {
        raw.map(|n| -(n as i64))
            .filter(|n| *n != 0)
            .unwrap_or_else(|| fallback_errno(code))
    }
}

/// Map a `std::io::ErrorKind` to a `(code, syscall)` for the connect path.
fn kind_to_code(kind: ErrorKind) -> Option<(&'static str, &'static str)> {
    match kind {
        ErrorKind::ConnectionRefused => Some(("ECONNREFUSED", "connect")),
        ErrorKind::TimedOut => Some(("ETIMEDOUT", "connect")),
        ErrorKind::ConnectionAborted => Some(("ECONNABORTED", "connect")),
        ErrorKind::AddrNotAvailable => Some(("EADDRNOTAVAIL", "connect")),
        ErrorKind::HostUnreachable => Some(("EHOSTUNREACH", "connect")),
        ErrorKind::NetworkUnreachable => Some(("ENETUNREACH", "connect")),
        _ => None,
    }
}

fn connect_message(code: &str, host: &str, port: Option<u16>) -> String {
    match port {
        Some(p) => format!("connect {code} {host}:{p}"),
        None => format!("connect {code} {host}"),
    }
}

/// Classify a `reqwest::Error` raised by `request.send()`. Walks the error's
/// `source()` chain for the underlying `std::io::Error` (whose `raw_os_error`
/// gives the exact errno) and a lowercased text trail (for DNS detection,
/// since resolver failures carry no OS errno).
pub(crate) fn classify_reqwest(e: &reqwest::Error, url: &str) -> Option<Classified> {
    let (host, port) = host_port(url);

    let mut io_errno: Option<i32> = None;
    let mut io_kind: Option<ErrorKind> = None;
    let mut chain = String::new();
    let mut cur: Option<&(dyn StdError + 'static)> = Some(e);
    while let Some(s) = cur {
        chain.push_str(&s.to_string().to_lowercase());
        chain.push(' ');
        if io_kind.is_none() {
            if let Some(io) = s.downcast_ref::<std::io::Error>() {
                io_errno = io.raw_os_error();
                io_kind = Some(io.kind());
            }
        }
        cur = s.source();
    }

    // The peer accepted the request and closed before any response head.
    // reqwest's top-level Display is only "error sending request for url";
    // the useful incomplete-message/reset reason lives in the source chain.
    // Node reports every such pre-response close as ECONNRESET "socket hang
    // up", distinct from a reset while consuming an established response.
    if io_kind == Some(ErrorKind::ConnectionReset)
        || io_kind == Some(ErrorKind::UnexpectedEof)
        || chain.contains("connection closed before message completed")
        || chain.contains("incomplete message")
        || chain.contains("connection reset")
    {
        return Some((
            "socket hang up".to_string(),
            "ECONNRESET".to_string(),
            "read".to_string(),
            libuv_errno("ECONNRESET", io_errno),
        ));
    }

    // Concrete OS connect error (the common case): exact code + errno.
    if let Some((code, syscall)) = io_kind.and_then(kind_to_code) {
        let errno = libuv_errno(code, io_errno);
        return Some((
            connect_message(code, &host, port),
            code.to_string(),
            syscall.to_string(),
            errno,
        ));
    }

    // DNS resolution failure → getaddrinfo ENOTFOUND (no OS errno; libuv -3008).
    let is_dns = chain.contains("dns error")
        || chain.contains("failed to lookup address")
        || chain.contains("failed to lookup")
        || chain.contains("name or service not known")
        || chain.contains("nodename nor servname")
        || chain.contains("no such host")
        || chain.contains("name resolution")
        || chain.contains("name not resolved");
    if is_dns {
        return Some((
            format!("getaddrinfo ENOTFOUND {host}"),
            "ENOTFOUND".to_string(),
            "getaddrinfo".to_string(),
            -3008,
        ));
    }

    // Text fallback when no `io::Error` surfaced but the reason is recognizable.
    if chain.contains("connection refused") {
        return Some((
            connect_message("ECONNREFUSED", &host, port),
            "ECONNREFUSED".to_string(),
            "connect".to_string(),
            fallback_errno("ECONNREFUSED"),
        ));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_port_parses_explicit_port() {
        assert_eq!(
            host_port("http://127.0.0.1:1/p"),
            ("127.0.0.1".to_string(), Some(1))
        );
    }

    #[test]
    fn host_port_defaults_known_scheme() {
        assert_eq!(
            host_port("http://example.com/"),
            ("example.com".to_string(), Some(80))
        );
    }

    #[test]
    fn connect_message_shapes() {
        assert_eq!(
            connect_message("ECONNREFUSED", "127.0.0.1", Some(1)),
            "connect ECONNREFUSED 127.0.0.1:1"
        );
        assert_eq!(
            connect_message("ECONNREFUSED", "127.0.0.1", None),
            "connect ECONNREFUSED 127.0.0.1"
        );
    }

    #[test]
    fn kind_mapping() {
        assert_eq!(
            kind_to_code(ErrorKind::ConnectionRefused),
            Some(("ECONNREFUSED", "connect"))
        );
        assert_eq!(kind_to_code(ErrorKind::NotFound), None);
    }
}
