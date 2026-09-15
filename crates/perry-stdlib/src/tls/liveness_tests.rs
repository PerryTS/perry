//! Exercise real TLS listener lifetime transitions, including bind failure.
use super::*;
use std::sync::atomic::Ordering;

fn drain_until_removed(handle: i64) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while servers().lock().unwrap().contains_key(&handle) {
        assert!(
            std::time::Instant::now() < deadline,
            "listener never retired"
        );
        crate::common::async_bridge::drive_pending(1);
        // SAFETY: this test is the owning JS thread; no user closures are installed.
        unsafe {
            js_tls_process_pending();
        }
    }
}

#[test]
fn tls_counter_balances_open_close_bind_error_and_cancel() {
    assert_eq!(TLS_ACTIVE.load(Ordering::Acquire), 0);
    let undefined = TAG_UNDEFINED_BITS as i64;
    // SAFETY: undefined options/callbacks are valid API arguments; every handle
    // used below comes from this constructor and remains registered until close.
    unsafe {
        let server = js_tls_create_server(undefined, undefined);
        js_tls_server_listen(server, 0.0, undefined, undefined);
        assert_eq!(TLS_ACTIVE.load(Ordering::Acquire), 1);
        crate::common::async_bridge::drive_pending(2);
        assert!(
            servers().lock().unwrap().get(&server).unwrap().bound_port > 0,
            "the native listen subject did not run"
        );
        js_tls_server_close(server, undefined);
        drain_until_removed(server);
        assert_eq!(TLS_ACTIVE.load(Ordering::Acquire), 0);

        let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let failing = js_tls_create_server(undefined, undefined);
        js_tls_server_listen(
            failing,
            f64::from(occupied.local_addr().unwrap().port()),
            undefined,
            undefined,
        );
        assert_eq!(TLS_ACTIVE.load(Ordering::Acquire), 1);
        crate::common::async_bridge::drive_pending(2);
        assert!(
            pending_events().lock().unwrap().iter().any(
                |event| matches!(event, PendingTlsEvent::ServerError(id, _) if *id == failing)
            ),
            "bind-error subject did not run"
        );
        drain_until_removed(failing);
        assert_eq!(TLS_ACTIVE.load(Ordering::Acquire), 0);

        let cancelled = js_tls_create_server(undefined, undefined);
        js_tls_server_listen(cancelled, 0.0, undefined, undefined);
        assert_eq!(TLS_ACTIVE.load(Ordering::Acquire), 1);
        js_tls_server_close(cancelled, undefined);
        drain_until_removed(cancelled);
        assert_eq!(TLS_ACTIVE.load(Ordering::Acquire), 0);
    }
}
