//! turnloop P0: the TLS keep-alive count across real listener lifetimes —
//! listen + close, a bind error, and a close issued before the listen task ran.
//! Each phase proves its native subject ran (a bound port, a queued error)
//! before checking that the count came back.
//!
//! The listener is a turnloop handle (`turnloop_server`), so the test owns
//! this agent's loop and drives it with bounded turns.

use super::*;

/// One non-blocking turn plus dispatch. A bounded park would return at once
/// forever: pushing a TLS event notifies the main thread, and only
/// `js_wait_for_event` consumes that notification.
fn turn() {
    perry_runtime::event_pump::js_loop_turn_bounded(0);
    std::thread::sleep(std::time::Duration::from_millis(1));
}

fn drain_until_removed(handle: i64) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while servers().lock().unwrap().contains_key(&handle) {
        assert!(
            std::time::Instant::now() < deadline,
            "listener never retired"
        );
        turn();
        // SAFETY: this test thread is the pump; no user closures are installed.
        unsafe {
            js_tls_process_pending();
        }
    }
}

#[test]
fn tls_keepalive_count_balances_listen_close_bind_error_and_early_close() {
    let _owner = crate::turnloop_client::become_the_owner_for_test();
    let undefined = TAG_UNDEFINED_BITS as i64;
    let baseline = liveness::count_for_test();
    // SAFETY: undefined options/callbacks are valid API arguments, and every
    // handle below comes from `js_tls_create_server` and stays registered until
    // its close event retires it.
    unsafe {
        let server = js_tls_create_server(undefined, undefined);
        js_tls_server_listen(server, 0.0, undefined, undefined);
        assert_eq!(liveness::count_for_test(), baseline + 1);
        let bound = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while servers().lock().unwrap().get(&server).unwrap().bound_port == 0 {
            assert!(std::time::Instant::now() < bound, "listener never bound");
            turn();
        }
        js_tls_server_close(server, undefined);
        drain_until_removed(server);
        assert_eq!(liveness::count_for_test(), baseline);

        // Occupy the port on the wildcard address the listener defaults to: a
        // specific-address holder would not stop a wildcard bind on macOS.
        let occupied = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
        let failing = js_tls_create_server(undefined, undefined);
        js_tls_server_listen(
            failing,
            f64::from(occupied.local_addr().unwrap().port()),
            undefined,
            undefined,
        );
        // turnloop binds synchronously, so the failure has already released
        // the listener's keep-alive; the tokio task bound on a later tick.
        assert_eq!(liveness::count_for_test(), baseline);
        let errored = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !pending_events()
            .lock()
            .unwrap()
            .iter()
            .any(|event| matches!(event, PendingTlsEvent::ServerError(id, _) if *id == failing))
        {
            assert!(
                std::time::Instant::now() < errored,
                "bind error never surfaced"
            );
            turn();
        }
        drain_until_removed(failing);
        assert_eq!(liveness::count_for_test(), baseline, "bind error leaked");

        let early = js_tls_create_server(undefined, undefined);
        js_tls_server_listen(early, 0.0, undefined, undefined);
        js_tls_server_close(early, undefined);
        drain_until_removed(early);
        assert_eq!(liveness::count_for_test(), baseline, "early close leaked");
    }
}
