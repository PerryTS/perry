//! A turnloop connection handed to `net` by an HTTP `'upgrade'` keeps
//! delivering what the peer sends (#10470, #10471).
//!
//! The handoff is `turnloop_net::transfer` + `adopt_turnloop_upgrade`: the id
//! and the outstanding multishot read stay put, and only the completion route
//! moves to this crate's slot. The runtime drops a completion for a slot with
//! no sink installed, and this crate installs its sink lazily — on the first
//! `turnloop_io::enabled()` call, which a program that only runs an HTTP
//! server never makes. Writes do not go through it either, so the listener's
//! `101` reached the client while every byte the client sent afterwards, and
//! its EOF, was dropped on the floor.
//!
//! # Why this is an integration binary with exactly ONE `#[test]`
//!
//! The sink registry is process-global. In the lib test binary some other test
//! has always installed this crate's sink by the time any upgrade test runs,
//! so the bug cannot be seen there. A fresh process where nothing in `net` has
//! run yet is precisely the http-only program the bug lived in.
//!
//! # What makes this non-vacuous
//!
//! The first assertion is that the sink is NOT yet installed, so the test is
//! reproducing the http-only shape rather than passing because something else
//! registered it. The bytes are then observed through `bytesRead`, which only
//! this crate's sink advances.

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use perry_ffi::turnloop_net as tl;

/// perry-ext-net's `#[cfg(test)]` shims are not compiled into the non-test lib
/// an integration binary links, so include the same file.
#[path = "../src/test_async_shims.rs"]
#[allow(dead_code)]
mod test_async_shims;

/// Stand-in for `perry-ext-http`'s `turnloop_serve` slot: the owner of the
/// connection before the upgrade. Nothing may reach it after the transfer.
const HTTP_SUBSYSTEM: u8 = 1;

static STRAY: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

extern "C" fn http_sink(completion: *const tl::NetCompletion) {
    if !completion.is_null() && unsafe { (*completion).kind } == tl::NET_DATA {
        STRAY.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

extern "C" fn http_alloc() -> i64 {
    0
}

fn turn() {
    perry_runtime::event_pump::js_loop_turn_bounded(0);
    std::thread::sleep(Duration::from_millis(1));
}

fn bytes_read(id: i64) -> f64 {
    unsafe { perry_ext_net::js_net_socket_get_bytes_read(id) }
}

#[test]
fn an_upgraded_connection_delivers_reads_to_net() {
    assert!(
        !tl::sink_installed(perry_ext_net::TURNLOOP_SUBSYSTEM),
        "net's sink must not be installed yet, or this does not model an \
         http-only program"
    );
    assert!(tl::register_sink(HTTP_SUBSYSTEM, http_sink, http_alloc));

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let mut peer = TcpStream::connect(listener.local_addr().unwrap()).expect("connect");
    let (accepted, _) = listener.accept().expect("accept");

    // The HTTP server's connection: owned by its subsystem, reading.
    let id = perry_ffi::reserve_handle_id();
    tl::adopt_stream(id, HTTP_SUBSYSTEM, accepted.into())
        .expect("adopt onto this thread's loop; without a loop nothing below is tested");
    tl::read_start(id).expect("the HTTP side's multishot read");

    // The `'upgrade'` handoff, spelled as `turnloop_serve::conn::on_upgrade`
    // and `client_turnloop::handoff` spell it.
    tl::transfer(id, perry_ext_net::TURNLOOP_SUBSYSTEM).expect("transfer");
    assert!(perry_ext_net::adopt_turnloop_upgrade(id));

    peer.write_all(b"ping").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while bytes_read(id) < 4.0 && Instant::now() < deadline {
        turn();
    }
    assert_eq!(
        bytes_read(id),
        4.0,
        "bytes the peer sends after the upgrade must reach the adopted net.Socket"
    );
    assert_eq!(
        STRAY.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "nothing may still be routed to the previous owner"
    );

    let _ = tl::close(id);
    for _ in 0..50 {
        turn();
    }
}
