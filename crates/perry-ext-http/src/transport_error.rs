//! Classify a client transport failure into the Node `Error` shape.
//!
//! Node hands `request.on('error')` a real `Error` carrying `.code`
//! (`ECONNREFUSED`, `ENOTFOUND`, …), `.syscall` (`connect` / `getaddrinfo`)
//! and `.errno` (the libuv-negative number), with a message like
//! `connect ECONNREFUSED 127.0.0.1:1` or `getaddrinfo ENOTFOUND host`.
//!
//! The turnloop transport reports the code, syscall and errno itself; this
//! module only shapes the message the way Node words it and supplies libuv's
//! errno when the runtime had none. (It used to reverse-engineer all of that
//! from a `reqwest::Error`'s `source()` chain.)

/// `(message, code, syscall, errno)` describing a Node-shaped transport error.
pub(crate) type Classified = (String, String, String, i64);

/// Platform errno (libuv-negative) for a code, used only when the transport
/// reported none.
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

fn connect_message(code: &str, host: &str, port: Option<u16>) -> String {
    match port {
        Some(p) => format!("connect {code} {host}:{p}"),
        None => format!("connect {code} {host}"),
    }
}

/// A connect-phase failure in Node's shape: `(message, code, syscall, errno)`.
/// A resolver failure reads `getaddrinfo ENOTFOUND host`; anything else
/// `connect <CODE> host:port`.
pub(crate) fn connect_failure(
    code: &str,
    syscall: &str,
    errno: i64,
    host: &str,
    port: u16,
) -> Classified {
    let errno = libuv_errno(code, (errno != 0).then(|| (-errno) as i32));
    if syscall == "getaddrinfo" {
        return (
            format!("getaddrinfo {code} {host}"),
            code.to_string(),
            syscall.to_string(),
            errno,
        );
    }
    (
        connect_message(code, host, Some(port)),
        code.to_string(),
        "connect".to_string(),
        errno,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn connect_failures_read_as_node_words_them() {
        let (message, code, syscall, _) =
            connect_failure("ECONNREFUSED", "connect", -111, "127.0.0.1", 1);
        assert_eq!(message, "connect ECONNREFUSED 127.0.0.1:1");
        assert_eq!(
            (code.as_str(), syscall.as_str()),
            ("ECONNREFUSED", "connect")
        );
        let (message, _, syscall, errno) =
            connect_failure("ENOTFOUND", "getaddrinfo", -3008, "nowhere.invalid", 80);
        assert_eq!(message, "getaddrinfo ENOTFOUND nowhere.invalid");
        assert_eq!(syscall, "getaddrinfo");
        assert_ne!(errno, 0);
    }
}
