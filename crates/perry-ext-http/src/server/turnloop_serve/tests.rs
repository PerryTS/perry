//! Unit tests for the turnloop HTTP server's transport-independent decisions.
//!
//! The wire-format tests live next to the encoder in `wire.rs`; these pin the
//! two things that are the *server's* decisions rather than the codec's — the
//! subsystem slot and Node's idle-close arithmetic.

use crate::server::server::{idle_close_ms, HttpServer};

#[test]
fn the_subsystem_slot_is_within_the_runtime_registry_and_not_nets() {
    // `register_sink` refuses an out-of-range slot, and a binding that picked
    // perry-ext-net's would take its completions: both failures look like a
    // server that accepts nothing.
    assert!((super::SUBSYSTEM as usize) < 4);
    assert_ne!(super::SUBSYSTEM, perry_ext_net::TURNLOOP_SUBSYSTEM);
}

#[test]
fn the_default_idle_close_is_node_s_timeout_plus_its_buffer() {
    // Node 26.5.1, measured: a 5000 ms timeout with the default 1000 ms buffer
    // FINs at 6000 ms; 300 + 1000 FINs at 1303 ms and 1000 + 1000 at 2002 ms.
    let server = HttpServer::with_handler(0);
    assert_eq!(server.keep_alive_timeout, 5_000.0);
    assert_eq!(server.keep_alive_timeout_buffer, 1_000.0);
    assert_eq!(idle_close_ms(&server), 6_000);
}

#[test]
fn a_zero_keep_alive_timeout_arms_no_idle_close() {
    // The other half of the `keepAliveTimeout = 0` fix: zero means "never time
    // out", so there is no deadline to arm — not a zero-length one, which
    // would close the connection immediately.
    let mut server = HttpServer::with_handler(0);
    server.keep_alive_timeout = 0.0;
    assert_eq!(idle_close_ms(&server), 0);

    // The buffer alone must not resurrect a deadline.
    server.keep_alive_timeout_buffer = 30_000.0;
    assert_eq!(idle_close_ms(&server), 0);
}

#[test]
fn a_custom_timeout_and_buffer_add() {
    let mut server = HttpServer::with_handler(0);
    server.keep_alive_timeout = 300.0;
    assert_eq!(idle_close_ms(&server), 1_300);
    server.keep_alive_timeout_buffer = 0.0;
    assert_eq!(idle_close_ms(&server), 300);
}

/// The posting route's outcome must say WHY a job did not land. A permanent
/// `NoRoute` is "no loop anywhere" (`ENOTSUP`); a loop whose postbox refused
/// every bounded retry is busy (`EAGAIN`), not absent. Before this split both
/// read as no-loop, so a listen that met a full postbox reported the wrong
/// error and a caller could not tell the two apart.
#[test]
fn post_outcome_separates_no_route_from_a_busy_owner() {
    use perry_ffi::agent_post::Rejected;

    let accepted = super::post_with(Box::new(()), |_| Ok(()));
    assert_eq!(accepted, super::Posted::Accepted);
    assert_eq!(accepted.error_code(), None);

    let no_route = super::post_with(Box::new(()), |job| Err(Rejected::NoRoute(job)));
    assert_eq!(no_route, super::Posted::NoRoute);
    assert_eq!(no_route.error_code(), Some(super::NO_LOOP_CODE));

    let mut tries = 0;
    let busy = super::post_with(Box::new(()), |job| {
        tries += 1;
        Err(Rejected::Again(job))
    });
    assert_eq!(busy, super::Posted::Busy);
    assert_eq!(busy.error_code(), Some(super::POST_BUSY_CODE));
    assert_eq!(
        tries,
        super::POST_ATTEMPTS,
        "retries must be bounded, and all used"
    );

    // Transient refusals followed by acceptance are an ordinary success.
    let mut refusals = 3;
    let late = super::post_with(Box::new(()), |job| {
        if refusals > 0 {
            refusals -= 1;
            Err(Rejected::Again(job))
        } else {
            Ok(())
        }
    });
    assert_eq!(late, super::Posted::Accepted);
}
