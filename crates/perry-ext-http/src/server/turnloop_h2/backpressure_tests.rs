//! Exercise compat writes with a stream held above its flow-control watermark.
//! The fixture deliberately has no socket/core: all written bytes remain in
//! the real stream outbox, while the socket queue reports zero.
use super::*;
use crate::server::{response, server};
use perry_ffi::{get_handle, get_handle_mut, register_handle};

fn connection(id: i64) -> H2Conn {
    H2Conn {
        id,
        role: Role::Server,
        server_handle: 0,
        // Zero: every glue call this module makes returns early on it, so a
        // pre-scan test touches no handle registry.
        session_handle: 0,
        core: None,
        input: Vec::new(),
        streams: Vec::new(),
        secure: false,
        handshaking: false,
        connecting: false,
        client_tls: None,
        alpn: None,
        peer_address: String::new(),
        peer_port: 0,
        buffered: 0,
        max_session_memory: 10 * 1024 * 1024,
        timer: Timer::None,
        draining: false,
        closing: false,
        read_eof: false,
        destroyed: false,
        queued_opens: Vec::new(),
        allow_http1: false,
        settings: Http2SettingsState::default(),
        preface_done: true,
        core_settings_acked: false,
        owed_settings_acks: 0,
        goaway_opaque: Vec::new(),
        peer_settings: None,
        pending_controls: Vec::new(),
    }
}

struct Fixture {
    conn: i64,
    response: i64,
}
impl Fixture {
    fn new() -> Self {
        let conn = perry_ffi::reserve_handle_id();
        let mut connection = connection(conn);
        connection.streams.push(super::super::stream::H2Stream {
            h2_id: 1,
            handle: 0,
            request_handle: 0,
            response_handle: 0,
            headers: HashMap::new(),
            raw_headers: Vec::new(),
            body: Vec::new(),
            trailers: Vec::new(),
            outbox: Vec::new(),
            outbox_end: false,
            send_trailers: Vec::new(),
            head_received: false,
            headers_sent: false,
            remote_end: false,
            local_end: false,
            dispatched: false,
            no_body: false,
            unreleased: 0,
            withheld: 0,
        });
        insert(connection);
        let mut response = response::ServerResponse::new();
        response.turnloop = Some((conn, 1));
        response.turnloop_streaming = true;
        Self {
            conn,
            response: register_handle(response),
        }
    }
    fn queued(&self) -> usize {
        peek(self.conn, |c| c.streams[0].outbox.len()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        forget(self.conn);
        perry_ffi::free_handle_id(self.conn);
        perry_ffi::drop_handle(self.response);
    }
}

#[test]
fn accepted_http2_backpressure_never_duplicates_the_body() {
    let f = Fixture::new();
    let data = vec![0x61; 64 * 1024];
    let chunk = perry_ffi::alloc_buffer(&data);
    let value = f64::from_bits(0x7ffd_0000_0000_0000 | chunk as u64);
    assert_eq!(response::js_node_http_res_write(f.response, value), 0);
    assert_eq!(f.queued(), data.len());
    let sr = get_handle::<response::ServerResponse>(f.response).unwrap();
    assert!(
        sr.buffered_body.is_empty(),
        "accepted bytes must not be buffered again"
    );
    assert!(sr.needs_drain);
}

#[test]
fn http2_drain_waits_for_stream_outbox_not_just_socket_queue() {
    let f = Fixture::new();
    with_owned(f.conn, |c| c.streams[0].outbox.resize(64 * 1024, 0));
    let sr = get_handle_mut::<response::ServerResponse>(f.response).unwrap();
    sr.needs_drain = true;
    // Listener ids are only collected here, never invoked.
    sr.once_listeners.insert("drain".into(), vec![123]);
    assert!(response::take_drain_listeners_if_ready(f.response).is_empty());
    assert!(
        get_handle::<response::ServerResponse>(f.response)
            .unwrap()
            .needs_drain
    );
    with_owned(f.conn, |c| c.streams[0].outbox.clear());
    assert_eq!(
        response::take_drain_listeners_if_ready(f.response),
        vec![123]
    );
    assert!(response::take_drain_listeners_if_ready(f.response).is_empty());
}

#[test]
fn http2_compat_pump_keeps_unfinished_response_alive() {
    let f = Fixture::new();
    let request = register_handle(String::from("request fixture"));
    let pending = server::HttpPendingRequest {
        server_handle: 0,
        request_handle: request,
        response_handle: f.response,
        skip_default_response: false,
        h2_stream_handle: 0,
        h2_stream_headers: Vec::new(),
        is_check_continue: false,
    };
    crate::server::http2_server::process_pending_h2(pending);
    assert!(
        get_handle::<response::ServerResponse>(f.response).is_some(),
        "a drain-driven producer must retain its response"
    );
    assert!(get_handle::<String>(request).is_some());
    assert!(server::has_in_flight_requests());
    get_handle_mut::<response::ServerResponse>(f.response)
        .unwrap()
        .writable_ended = true;
    server::reap_in_flight_requests();
    assert!(get_handle::<response::ServerResponse>(f.response).is_none());
    assert!(get_handle::<String>(request).is_none());
}
