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
    /// Node's `err.errno`, as libuv reports it: the negated OS code on Linux
    /// and macOS, and libuv's own `-4xxx` value on Windows (where the two are
    /// unrelated — `UV_EADDRINUSE` is -4091, not -10048). See [`libuv_errno`].
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
        errno: libuv_errno(code, err.os),
        syscall,
    }
}

/// libuv's `errno` for a Node error, which is what `err.errno` must report.
///
/// On Linux and macOS libuv's errno IS the negated OS errno (`UV_EADDRINUSE`
/// is -98 and -48 respectively), so negating the OS code is correct there by
/// construction.
///
/// Windows is the platform where that identity does not hold. libuv has its
/// own `-4xxx` space there, unrelated to the Winsock numbers: `UV_EADDRINUSE`
/// is **-4091**, not -10048. Negating the OS code therefore produced a number
/// no Node program would recognise — `err.errno === -4091` silently never
/// matched. The committed fixtures cannot see this because they assert only
/// that `errno` is negative (`test_gap_turnloop_listen_error.ts` prints
/// `errno<0`), which both the right and the wrong value satisfy.
///
/// Values read from the pinned oracle itself —
/// `node -e "require('util').getSystemErrorMap()"` on Node 26.5.1 — rather
/// than transcribed from libuv's headers.
#[cfg(windows)]
pub fn libuv_errno(code: &str, os: Option<i32>) -> i32 {
    // No OS code means no syscall ran, and Node reports no errno at all —
    // never a value looked up from the name. Same contract as the unix arm,
    // where `None` maps to 0 rather than to a negated code.
    if os.is_none() {
        return 0;
    }
    libuv_windows_errno(code)
}

/// libuv's Windows error number for a Node error name, or `UV_UNKNOWN`
/// (-4094) for a name libuv has no Windows number for. Pure, so `cfg(test)`
/// keeps it compiled — and asserted — on every host, not just on Windows.
#[cfg(any(windows, test))]
fn libuv_windows_errno(code: &str) -> i32 {
    match code {
        "E2BIG" => -4093,
        "EACCES" => -4092,
        "EADDRINUSE" => -4091,
        "EADDRNOTAVAIL" => -4090,
        "EAFNOSUPPORT" => -4089,
        "EAGAIN" => -4088,
        "EALREADY" => -4084,
        "EBADF" => -4083,
        "EBUSY" => -4082,
        "ECANCELED" => -4081,
        "ECONNABORTED" => -4079,
        "ECONNREFUSED" => -4078,
        "ECONNRESET" => -4077,
        "EDESTADDRREQ" => -4076,
        "EEXIST" => -4075,
        "EFAULT" => -4074,
        "EHOSTUNREACH" => -4073,
        "EINTR" => -4072,
        "EINVAL" => -4071,
        "EIO" => -4070,
        "EISCONN" => -4069,
        "EISDIR" => -4068,
        "ELOOP" => -4067,
        "EMFILE" => -4066,
        "EMSGSIZE" => -4065,
        "ENAMETOOLONG" => -4064,
        "ENETDOWN" => -4063,
        "ENETUNREACH" => -4062,
        "ENFILE" => -4061,
        "ENOBUFS" => -4060,
        "ENODEV" => -4059,
        "ENOENT" => -4058,
        "ENOMEM" => -4057,
        "ENOPROTOOPT" => -4035,
        "ENOSPC" => -4055,
        "ENOSYS" => -4054,
        "ENOTCONN" => -4053,
        "ENOTDIR" => -4052,
        "ENOTEMPTY" => -4051,
        "ENOTSOCK" => -4050,
        "ENOTSUP" => -4049,
        "EPERM" => -4048,
        "EPIPE" => -4047,
        "EPROTO" => -4046,
        "EPROTONOSUPPORT" => -4045,
        "EPROTOTYPE" => -4044,
        "ERANGE" => -4034,
        "EROFS" => -4043,
        "ESPIPE" => -4042,
        "ESRCH" => -4041,
        "ETIMEDOUT" => -4039,
        "EXDEV" => -4037,
        // Node's catch-all for an OS error libuv has no name for.
        _ => -4094, // UV_UNKNOWN
    }
}

/// On Linux and macOS the negated OS errno IS libuv's errno.
#[cfg(not(windows))]
pub fn libuv_errno(_code: &str, os: Option<i32>) -> i32 {
    os.map_or(0, |os| -os)
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
        // `errno` is libuv's number. On unix that IS the negated OS code; on
        // Windows it is libuv's own -4078, NOT -10061 (see `libuv_errno`).
        // This assertion used to read `-refused` on every platform, which
        // quietly encoded the Windows bug it was meant to describe.
        #[cfg(unix)]
        assert_eq!(mapped.errno, -refused);
        #[cfg(windows)]
        assert_eq!(mapped.errno, -4078);
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

    /// #10385: `err.errno` must be libuv's number, not the negated OS one.
    ///
    /// On Linux and macOS those are the same value, so the bug was invisible
    /// for two platforms and shipped. On Windows libuv uses its own `-4xxx`
    /// space: `UV_EADDRINUSE` is -4091 while the negated Winsock code is
    /// -10048, and a program testing `err.errno === -4091` matched nothing.
    ///
    /// The committed gap fixture cannot catch this — it asserts `errno < 0`,
    /// which both the right and the wrong value satisfy — so the guard lives
    /// here. Expected values come from the pinned oracle itself:
    /// `node -e "require('util').getSystemErrorMap()"` on Node 26.5.1.
    #[test]
    fn errno_is_libuvs_number_not_the_negated_os_code() {
        let cases: &[(&str, i32)] = &[
            ("EADDRINUSE", if cfg!(windows) { -4091 } else { 0 }),
            ("ECONNREFUSED", if cfg!(windows) { -4078 } else { 0 }),
            ("ECONNRESET", if cfg!(windows) { -4077 } else { 0 }),
            ("ECONNABORTED", if cfg!(windows) { -4079 } else { 0 }),
            ("ETIMEDOUT", if cfg!(windows) { -4039 } else { 0 }),
        ];
        for (name, expected_windows) in cases {
            let os = os_code_for_name(name).expect("name resolves");
            let mapped = map_error(
                Error {
                    kind: ErrorKind::Other,
                    os: Some(os),
                },
                "listen",
            );
            assert_eq!(mapped.code, *name);
            if cfg!(windows) {
                assert_eq!(
                    mapped.errno, *expected_windows,
                    "{name}: expected libuv's Windows errno, got the negated OS code?"
                );
                assert_ne!(
                    mapped.errno, -os,
                    "{name}: errno must NOT be the negated Winsock number"
                );
            } else {
                // The identity that made the Windows bug invisible.
                assert_eq!(
                    mapped.errno, -os,
                    "{name}: unix errno is the negated OS code"
                );
            }
        }
    }

    /// The Windows arm runs only on Windows, so its table is asserted here on
    /// every host: a transposed number would otherwise ship unseen from
    /// macOS/Linux CI. Where `util_syserr`'s filesystem table (#10452) also
    /// names a code, the two must agree — they describe the same libuv space.
    #[test]
    fn windows_errno_table_matches_libuv_and_the_fs_table() {
        assert_eq!(libuv_windows_errno("EADDRINUSE"), -4091);
        assert_eq!(libuv_windows_errno("ECONNREFUSED"), -4078);
        assert_eq!(libuv_windows_errno("ECONNRESET"), -4077);
        assert_eq!(libuv_windows_errno("ETIMEDOUT"), -4039);
        assert_eq!(libuv_windows_errno("NOT_A_CODE"), -4094);
        let mut shared = 0;
        for (_, name) in os_table() {
            if let Some(fs_errno) = crate::util_syserr::uv_windows_errno(name) {
                shared += 1;
                assert_eq!(
                    libuv_windows_errno(name),
                    fs_errno,
                    "{name}: turnloop_net and util_syserr disagree on libuv's Windows errno"
                );
            }
        }
        // Liveness: the cross-check must have compared something, or a table
        // rename would turn it vacuous.
        assert!(shared > 0, "no code is shared with util_syserr's table");
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
