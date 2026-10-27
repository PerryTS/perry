//! Unconditional process signal policy.
/// Ignore `SIGPIPE` for the life of the process (#9402).
///
/// A Perry program has its own C `main`, emitted by codegen — it never runs
/// Rust's `std::rt` startup, which is where an ordinary Rust binary gets
/// `SIGPIPE` set to `SIG_IGN`. A compiled program therefore inherited the
/// signal's DEFAULT disposition, so **any** truncating consumer killed it
/// mid-write: `| head`, `| grep -q`, `| less` followed by `q`, a client that
/// closed its socket. `claude auto-mode defaults | head -2` exited 141 under
/// Perry and 0 under Node.
///
/// Node (through libuv) does exactly this: the signal is ignored and the
/// failing `write(2)` returns `EPIPE` to the writer, which is a value the
/// program can see and act on rather than a death it cannot.
///
/// Idempotent and safe from any thread — `js_gc_init` calls it once per image
/// entry, and a second entry on another thread re-applies the same
/// disposition. A `SIGPIPE` disposition an embedder installed BEFORE loading
/// the runtime is left alone: only `SIG_DFL` (the state that kills us) is
/// replaced. `process.on('SIGPIPE', …)` still installs its own handler
/// afterwards through [`install_process_signal_handler`], exactly as it did.
///
/// Not inherited by children: Rust's `Command` restores `SIGPIPE` to
/// `SIG_DFL` in the forked child before `exec`, matching Node.
#[cfg(unix)]
pub(crate) fn ignore_sigpipe_at_startup() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        // SAFETY: a query (`act` = NULL) followed by a plain disposition
        // install. `sigaction` is async-signal-safe and touches no Perry state.
        let mut current: libc::sigaction = std::mem::zeroed();
        if libc::sigaction(libc::SIGPIPE, std::ptr::null(), &mut current) != 0 {
            return;
        }
        if current.sa_sigaction != libc::SIG_DFL {
            return;
        }
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = libc::SIG_IGN;
        sa.sa_flags = 0;
        libc::sigemptyset(&mut sa.sa_mask);
        libc::sigaction(libc::SIGPIPE, &sa, std::ptr::null_mut());
    });
}

/// Windows has no `SIGPIPE`: a write to a closed pipe already fails with
/// `ERROR_BROKEN_PIPE` instead of raising a signal.
#[cfg(not(unix))]
pub(crate) fn ignore_sigpipe_at_startup() {}
