//! Node-compatible error mapping for turnloop socket failures (DESIGN §9:
//! "Error text and codes | Perry | Map `Error { kind, os }` to Node's
//! `code`/`errno`/`syscall`").
//!
//! Two inputs, in priority order:
//!
//! 1. the original OS code (`turnloop::Error::os`), which is what Node reports
//!    in `err.errno` and what its `code` string is derived from. A table keyed
//!    on the host's own `libc::E*` / `WSAE*` values keeps darwin, linux and
//!    windows correct without a per-platform code list;
//! 2. the portable [`turnloop::ErrorKind`], used only when the backend had no
//!    OS code to report (a turnloop-internal rejection: a full operation table,
//!    an invalid handle, a cancelled operation).
//!
//! `syscall` is not derivable from either: the same `ECONNRESET` is
//! `syscall: 'read'` or `'write'` depending on what failed. The submitting
//! operation supplies it, which is why [`NodeError`] carries it as a field
//! rather than computing it.

use turnloop::{Error, ErrorKind};

/// A Node-shaped socket error: the three fields `net` puts on the `Error`
/// object it emits (`code`, `errno`, `syscall`) plus the message text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeError {
    /// Node's `err.code`, e.g. `"ECONNREFUSED"`. Always a static name.
    pub code: &'static str,
    /// Node's `err.errno`: the raw OS code, negated the way libuv reports it.
    /// Zero when the failure never reached a syscall.
    pub errno: i32,
    /// Node's `err.syscall`, e.g. `"connect"`. Supplied by the operation.
    pub syscall: &'static str,
}

impl NodeError {
    /// `<syscall> <CODE>` — libuv's message shape, which Node keeps verbatim
    /// for socket errors (`connect ECONNREFUSED 127.0.0.1:1`, minus the
    /// address the caller appends).
    pub fn message(&self) -> String {
        if self.syscall.is_empty() {
            self.code.to_string()
        } else {
            format!("{} {}", self.syscall, self.code)
        }
    }
}

/// Socket-relevant OS codes, as `(host value, Node name)`.
///
/// Deliberately not the whole libuv table (`util_syserr.rs` has that for
/// `util.getSystemErrorName`): only codes a stream socket can actually
/// produce. An unlisted code falls through to the [`ErrorKind`] mapping and
/// then to `UNKNOWN`, which is exactly what Node does for a code libuv has no
/// name for.
#[cfg(unix)]
fn os_table() -> &'static [(i32, &'static str)] {
    &[
        (libc::EACCES, "EACCES"),
        (libc::EADDRINUSE, "EADDRINUSE"),
        (libc::EADDRNOTAVAIL, "EADDRNOTAVAIL"),
        (libc::EAFNOSUPPORT, "EAFNOSUPPORT"),
        (libc::EAGAIN, "EAGAIN"),
        (libc::EALREADY, "EALREADY"),
        (libc::EBADF, "EBADF"),
        (libc::EBUSY, "EBUSY"),
        (libc::ECANCELED, "ECANCELED"),
        (libc::ECONNABORTED, "ECONNABORTED"),
        (libc::ECONNREFUSED, "ECONNREFUSED"),
        (libc::ECONNRESET, "ECONNRESET"),
        (libc::EDESTADDRREQ, "EDESTADDRREQ"),
        (libc::EEXIST, "EEXIST"),
        (libc::EFAULT, "EFAULT"),
        (libc::EHOSTUNREACH, "EHOSTUNREACH"),
        (libc::EINVAL, "EINVAL"),
        (libc::EISCONN, "EISCONN"),
        (libc::ELOOP, "ELOOP"),
        (libc::EMFILE, "EMFILE"),
        (libc::EMSGSIZE, "EMSGSIZE"),
        (libc::ENAMETOOLONG, "ENAMETOOLONG"),
        (libc::ENETDOWN, "ENETDOWN"),
        (libc::ENETUNREACH, "ENETUNREACH"),
        (libc::ENFILE, "ENFILE"),
        (libc::ENOBUFS, "ENOBUFS"),
        (libc::ENOENT, "ENOENT"),
        (libc::ENOMEM, "ENOMEM"),
        (libc::ENOPROTOOPT, "ENOPROTOOPT"),
        (libc::ENOTCONN, "ENOTCONN"),
        (libc::ENOTDIR, "ENOTDIR"),
        (libc::ENOTSOCK, "ENOTSOCK"),
        (libc::ENOTSUP, "ENOTSUP"),
        (libc::EPERM, "EPERM"),
        (libc::EPIPE, "EPIPE"),
        (libc::EPROTO, "EPROTO"),
        (libc::EPROTONOSUPPORT, "EPROTONOSUPPORT"),
        (libc::ETIMEDOUT, "ETIMEDOUT"),
    ]
}

/// Winsock's `WSAE*` numbers. Node reports the `E*` spelling on Windows too,
/// so the table maps the Winsock value onto the same portable name.
#[cfg(windows)]
fn os_table() -> &'static [(i32, &'static str)] {
    &[
        (10013, "EACCES"),          // WSAEACCES
        (10048, "EADDRINUSE"),      // WSAEADDRINUSE
        (10049, "EADDRNOTAVAIL"),   // WSAEADDRNOTAVAIL
        (10047, "EAFNOSUPPORT"),    // WSAEAFNOSUPPORT
        (10035, "EAGAIN"),          // WSAEWOULDBLOCK
        (10037, "EALREADY"),        // WSAEALREADY
        (10009, "EBADF"),           // WSAEBADF
        (10053, "ECONNABORTED"),    // WSAECONNABORTED
        (10061, "ECONNREFUSED"),    // WSAECONNREFUSED
        (10054, "ECONNRESET"),      // WSAECONNRESET
        (10039, "EDESTADDRREQ"),    // WSAEDESTADDRREQ
        (10014, "EFAULT"),          // WSAEFAULT
        (10065, "EHOSTUNREACH"),    // WSAEHOSTUNREACH
        (10022, "EINVAL"),          // WSAEINVAL
        (10056, "EISCONN"),         // WSAEISCONN
        (10062, "ELOOP"),           // WSAELOOP
        (10024, "EMFILE"),          // WSAEMFILE
        (10040, "EMSGSIZE"),        // WSAEMSGSIZE
        (10063, "ENAMETOOLONG"),    // WSAENAMETOOLONG
        (10050, "ENETDOWN"),        // WSAENETDOWN
        (10051, "ENETUNREACH"),     // WSAENETUNREACH
        (10055, "ENOBUFS"),         // WSAENOBUFS
        (10042, "ENOPROTOOPT"),     // WSAENOPROTOOPT
        (10057, "ENOTCONN"),        // WSAENOTCONN
        (10038, "ENOTSOCK"),        // WSAENOTSOCK
        (10045, "ENOTSUP"),         // WSAEOPNOTSUPP
        (10058, "EPIPE"),           // WSAESHUTDOWN
        (10043, "EPROTONOSUPPORT"), // WSAEPROTONOSUPPORT
        (10060, "ETIMEDOUT"),       // WSAETIMEDOUT
        (2, "ENOENT"),              // ERROR_FILE_NOT_FOUND (named pipes)
        (3, "ENOENT"),              // ERROR_PATH_NOT_FOUND
        (5, "EACCES"),              // ERROR_ACCESS_DENIED
        (231, "EBUSY"),             // ERROR_PIPE_BUSY
        (232, "EPIPE"),             // ERROR_NO_DATA
        (109, "EPIPE"),             // ERROR_BROKEN_PIPE
    ]
}

#[cfg(not(any(unix, windows)))]
fn os_table() -> &'static [(i32, &'static str)] {
    &[]
}

/// The portable fallback, used only when the backend reported no OS code.
fn kind_code(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::Cancelled => "ECANCELED",
        ErrorKind::Unsupported => "ENOTSUP",
        ErrorKind::InvalidInput => "EINVAL",
        ErrorKind::NotFound => "ENOENT",
        ErrorKind::WouldBlock => "EAGAIN",
        ErrorKind::TimedOut => "ETIMEDOUT",
        ErrorKind::ConnectionRefused => "ECONNREFUSED",
        ErrorKind::ConnectionReset => "ECONNRESET",
        ErrorKind::BrokenPipe => "EPIPE",
        ErrorKind::ResourceLimit => "ENOMEM",
        // Filesystem categories turnloop 0.1.0-alpha.3 added for its typed
        // file operations. A stream socket does not produce them, but they are
        // mapped rather than folded into UNKNOWN so a future caller of this
        // helper (P2's pipes, P4's file jobs) gets Node's real code.
        ErrorKind::PermissionDenied => "EACCES",
        ErrorKind::AlreadyExists => "EEXIST",
        ErrorKind::NotADirectory => "ENOTDIR",
        ErrorKind::IsADirectory => "EISDIR",
        ErrorKind::DirectoryNotEmpty => "ENOTEMPTY",
        ErrorKind::Other => "UNKNOWN",
    }
}

/// Map one turnloop error onto Node's `code`/`errno`/`syscall` triple.
///
/// `syscall` is the operation that failed; it is never inferred, because the
/// same OS code means different things per operation (`ECONNRESET` on a read
/// versus on a write) and Node's own message text starts with it.
pub fn map_error(err: Error, syscall: &'static str) -> NodeError {
    let code = err
        .os
        .and_then(|os| {
            os_table()
                .iter()
                .find_map(|(value, name)| (*value == os).then_some(*name))
        })
        .unwrap_or_else(|| kind_code(err.kind));
    NodeError {
        code,
        // libuv (and therefore Node) reports errno as the negated OS code.
        errno: err.os.map_or(0, |os| -os),
        syscall,
    }
}

/// The host OS code for a Node error name, for the reverse direction: a
/// caller that already has the `code` string (because it parsed libuv's
/// message shape) still has to report `err.errno`, and that number is
/// platform-specific.
pub fn os_code_for_name(name: &str) -> Option<i32> {
    os_table()
        .iter()
        .find_map(|(value, code)| (*code == name).then_some(*value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_code_wins_over_the_portable_kind() {
        // `Other` would map to UNKNOWN, but the OS code is the authority: this
        // is the whole reason turnloop carries `os` alongside `kind`.
        #[cfg(unix)]
        let refused = libc::ECONNREFUSED;
        #[cfg(windows)]
        let refused = 10061;
        let mapped = map_error(
            Error {
                kind: ErrorKind::Other,
                os: Some(refused),
            },
            "connect",
        );
        assert_eq!(mapped.code, "ECONNREFUSED");
        assert_eq!(mapped.errno, -refused);
        assert_eq!(mapped.syscall, "connect");
        assert_eq!(mapped.message(), "connect ECONNREFUSED");
    }

    #[test]
    fn portable_kind_is_the_fallback_when_no_syscall_ran() {
        let mapped = map_error(Error::new(ErrorKind::Cancelled), "read");
        assert_eq!(mapped.code, "ECANCELED");
        // No OS code means Node reports no errno, not errno 0 from a lookup.
        assert_eq!(mapped.errno, 0);
    }

    #[test]
    fn an_unknown_os_code_falls_back_rather_than_inventing_a_name() {
        let mapped = map_error(
            Error {
                kind: ErrorKind::BrokenPipe,
                os: Some(0x7f_ff_ff_ff),
            },
            "write",
        );
        assert_eq!(mapped.code, "EPIPE");
    }

    #[test]
    fn the_name_lookup_is_the_inverse_of_the_value_lookup() {
        // The two directions read the same table, so a name that maps to a
        // value must map back — otherwise `err.code` and `err.errno` on one
        // error object would describe different failures.
        for (value, name) in os_table() {
            let back = os_code_for_name(name).expect("name resolves");
            assert_eq!(
                map_error(
                    Error {
                        kind: ErrorKind::Other,
                        os: Some(back)
                    },
                    ""
                )
                .code,
                *name,
                "OS code {value} / name {name} round trip"
            );
        }
        assert_eq!(os_code_for_name("NOT_A_CODE"), None);
    }

    #[test]
    fn every_table_entry_is_reachable_by_its_own_value() {
        // A duplicated host value (two libc constants that collide on some
        // platform) would make one entry permanently unreachable, and the
        // table would silently report the wrong name for it.
        for (value, name) in os_table() {
            let found = os_table()
                .iter()
                .find_map(|(v, n)| (v == value).then_some(*n))
                .expect("entry present");
            assert_eq!(
                found, *name,
                "OS code {value} maps to {found}, shadowing {name}"
            );
        }
    }
}
