//! `SO_REUSEPORT` reaches the kernel through the turnloop listen ABI that
//! `server::turnloop_serve::listen` uses (turnloop lane A).
//!
//! # Why this is an integration binary with exactly ONE `#[test]`
//!
//! An agent's turnloop route is claimed **once per thread, by the first thread
//! to ask**, and every other thread acting for that agent is declined for the
//! rest of its life (`event_pump::agent_loop::claim_route`). A multi-threaded
//! `cargo test` therefore hands the route to whichever test thread asks first,
//! so a *unit* test that asserts `turnloop_serve::enabled()` is a lottery — it
//! passes or fails depending on which harness thread it landed on. That is not
//! hypothetical: the first draft of this coverage lived in
//! `turnloop_serve/tests.rs` and failed two of three assertions in the same
//! run, deterministically, while a sibling assertion on another thread passed.
//!
//! One `#[test]` in its own binary is one thread in its own process, so it is
//! the first asker by construction and the route is always available to it.
//! The pure listen-decision half — is this a shared bind, and which port —
//! stays a unit test (`server::server::turnloop_listen::tests`), because that
//! half needs no loop at all.
//!
//! # What makes this non-vacuous
//!
//! There is no "skip if no loop" arm. The first bind must succeed, and it can
//! only succeed on a thread that owns a loop, so a run with no route fails
//! loudly instead of passing having tested nothing. The exclusive-bind control
//! is the second half: two successful binds prove `SO_REUSEPORT` only if the
//! duplicate bind is otherwise refused.

use perry_ffi::turnloop_net as tl;

/// `server::turnloop_serve`'s own slot in the runtime's completion-sink
/// registry, so these binds are spelled exactly as the code under test spells
/// them rather than borrowing `perry-ext-net`'s slot 0. Nothing is accepted
/// here — `accept_start` is never called and both binds are closed immediately
/// — so no completion is produced for any sink to receive.
const SUBSYSTEM: u8 = 1;

fn listen(id: i64, port: u16, reuse_port: bool) -> Result<u16, String> {
    // `nodelay` is the last argument, `reuse_port` the one before it. That
    // order is the whole point: they were once transposed, which set
    // `SO_REUSEPORT` on every HTTP listener and left `TCP_NODELAY` unset on
    // every accepted connection.
    tl::tcp_listen(id, SUBSYSTEM, "127.0.0.1", port, 511, reuse_port, true)
        .map_err(|e| e.message())?;
    Ok(tl::local_address(id).map(|a| a.port).unwrap_or(port))
}

#[test]
fn a_shared_bind_is_permitted_where_an_exclusive_one_is_refused() {
    // ── The exclusive control ────────────────────────────────────────────
    // Also the liveness assertion: this bind is submitted to *this thread's*
    // turnloop loop, so it cannot succeed without one.
    let port = listen(1, 0, false).expect(
        "the first ephemeral bind must succeed; it is submitted to this thread's \
         turnloop loop, so a failure here means there is no loop and every \
         assertion below would be vacuous",
    );
    let duplicate = listen(2, port, false);
    let refused = duplicate.is_err();
    if duplicate.is_ok() {
        let _ = tl::close(2);
    }
    let _ = tl::close(1);
    assert!(
        refused,
        "a second exclusive listen on {port} must be EADDRINUSE, or the shared \
         bind below proves nothing"
    );

    // ── The subject ──────────────────────────────────────────────────────
    // turnloop 0.1.0-alpha.6 split `ListenOpts::reuse_port` into
    // `ReusePort::{No, Share, Distribute}`; `perry-runtime`'s `listen_opts`
    // maps this `true` to `Share`, which is what `cluster_bind::bind_listener`
    // sets by hand and what a cluster worker had to decline the turnloop path
    // to get. NOT `Distribute`: that is `Unsupported` on macOS and on every BSD
    // but FreeBSD, so asking for it would fail this test on the host it runs on.
    let shared_port = listen(3, 0, true).expect("an ephemeral shared bind must succeed");
    let second = listen(4, shared_port, true);
    let shared = match &second {
        Ok(bound) => {
            assert_eq!(
                *bound, shared_port,
                "the second listener must hold the same port, not a new ephemeral one"
            );
            true
        }
        Err(_) => false,
    };
    if second.is_ok() {
        let _ = tl::close(4);
    }
    let _ = tl::close(3);
    assert!(
        shared,
        "SO_REUSEPORT did not reach the kernel: a cluster worker cannot share {shared_port}, \
         which is the bind `try_listen_on_turnloop` now takes instead of declining to hyper"
    );
}
