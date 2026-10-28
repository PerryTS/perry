//! SETTINGS as a live, two-sided negotiation (PerryTS/turnloop#87), and the
//! GOAWAY opaque data that rides beside it.
use std::time::{Duration, Instant};
use turnloop_http::{
    http1::Header,
    http2::{self, Connection, Event, Limits, Role, Settings},
};

/// One observed event, owned so it outlives the input it borrowed from.
#[derive(Debug, PartialEq)]
enum Seen {
    Settings(Vec<(u16, u32)>),
    SettingsAck(Vec<(u16, u32)>),
    Headers(u32),
    Data(u32, usize),
    Reset(u32, u32),
    Goaway(u32, Option<Vec<u8>>),
    Other,
}

/// Drive `to` with the documented loop condition until it stops.
fn drive(to: &mut Connection, input: &mut Vec<u8>) -> Result<Vec<Seen>, &'static str> {
    let mut seen = Vec::new();
    loop {
        let step = to.receive(input).map_err(|e| e.code)?;
        let consumed = step.consumed;
        let progressed = consumed > 0 || step.event.is_some();
        if let Some(event) = step.event {
            seen.push(match event {
                Event::Settings(frame) => Seen::Settings(frame.iter().collect()),
                Event::SettingsAck(settings) => Seen::SettingsAck(settings.iter().collect()),
                Event::Headers { stream, .. } => Seen::Headers(stream),
                Event::Data { stream, bytes, .. } => Seen::Data(stream, bytes.len()),
                Event::Reset { stream, code } => Seen::Reset(stream, code),
                Event::Goaway { code, debug, .. } => Seen::Goaway(code, debug.map(<[u8]>::to_vec)),
                _ => Seen::Other,
            });
        }
        input.drain(..consumed);
        if !progressed {
            return Ok(seen);
        }
    }
}
fn ship(from: &mut Connection) -> Vec<u8> {
    let wire = from.output().to_vec();
    from.consume_output(wire.len()).unwrap();
    wire
}
fn request() -> Vec<Header> {
    vec![
        Header::new(":method", "POST"),
        Header::new(":scheme", "http"),
        Header::new(":path", "/"),
        Header::new(":authority", "localhost"),
    ]
}
/// Default-limits client and server, both initial SETTINGS exchanged and acked.
fn handshake() -> (Connection, Connection) {
    let mut client = Connection::new(Role::Client, Limits::default()).unwrap();
    let mut server = Connection::new(Role::Server, Limits::default()).unwrap();
    drive(&mut server, &mut ship(&mut client)).unwrap();
    drive(&mut client, &mut ship(&mut server)).unwrap();
    drive(&mut server, &mut ship(&mut client)).unwrap();
    assert_eq!(
        (client.pending_settings(), server.pending_settings()),
        (0, 0)
    );
    (client, server)
}
const DEFAULT_ADVERTISED: [(u16, u32); 3] = [(3, 100), (5, 16384), (6, 32768)];

/// Item 1. A second SETTINGS while the first is outstanding is ordinary: RFC
/// 9113 section 6.5.3 acknowledges them in order. With a single "awaiting ack"
/// flag the second ack was "unsolicited" and failed the connection.
#[test]
fn settings_acks_are_an_ordered_queue() {
    let mut client = Connection::new(Role::Client, Limits::default()).unwrap();
    let mut server = Connection::new(Role::Server, Limits::default()).unwrap();
    let mut second = Settings::new();
    second.set(Settings::MAX_CONCURRENT_STREAMS, 7);
    let mut third = Settings::new();
    third.set(Settings::MAX_HEADER_LIST_SIZE, 65536);
    // Two more before the peer has seen even the first.
    server.settings(&second).unwrap();
    server.settings(&third).unwrap();
    assert_eq!(server.pending_settings(), 3);

    // The server reads the client's preface and SETTINGS and acks them.
    let seen = drive(&mut server, &mut ship(&mut client)).unwrap();
    assert_eq!(
        seen,
        vec![Seen::Settings(vec![
            (2, 0),
            (3, 100),
            (5, 16384),
            (6, 32768)
        ])]
    );
    let seen = drive(&mut client, &mut ship(&mut server)).unwrap();
    assert_eq!(
        seen,
        vec![
            Seen::Settings(DEFAULT_ADVERTISED.to_vec()),
            Seen::Settings(vec![(3, 7)]),
            Seen::Settings(vec![(6, 65536)]),
            Seen::SettingsAck(vec![(2, 0), (3, 100), (5, 16384), (6, 32768)]),
        ]
    );
    // The client acked all three; the server pairs them with what it sent.
    let seen = drive(&mut server, &mut ship(&mut client)).unwrap();
    assert_eq!(
        seen,
        vec![
            Seen::SettingsAck(DEFAULT_ADVERTISED.to_vec()),
            Seen::SettingsAck(vec![(3, 7)]),
            Seen::SettingsAck(vec![(6, 65536)]),
        ]
    );
    assert_eq!(server.pending_settings(), 0);
    assert_eq!(
        server.local_settings().iter().collect::<Vec<_>>(),
        vec![(3, 7), (5, 16384), (6, 65536)]
    );

    // A fourth ack has nothing to acknowledge, and that is still an error.
    let mut wire = Vec::new();
    http2::encode_frame(4, 1, 0, &[], &mut wire).unwrap();
    assert_eq!(drive(&mut server, &mut wire), Err("PROTOCOL_ERROR"));
}

/// Item 2. The acknowledgement is an event carrying the settings it
/// acknowledges, and it clears the host's deadline for that frame - so a host
/// can resolve `session.settings(obj, callback)` and time the round trip.
#[test]
fn settings_ack_is_an_event_and_clears_its_deadline() {
    let (mut client, mut server) = handshake();
    assert_eq!(server.next_timeout(), None);
    let mut change = Settings::new();
    change.set(Settings::INITIAL_WINDOW_SIZE, 100_000);
    server.settings(&change).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    server.set_settings_deadline(Some(deadline));
    assert_eq!(server.next_timeout(), Some(deadline));

    let seen = drive(&mut client, &mut ship(&mut server)).unwrap();
    assert_eq!(seen, vec![Seen::Settings(vec![(4, 100_000)])]);
    let seen = drive(&mut server, &mut ship(&mut client)).unwrap();
    assert_eq!(seen, vec![Seen::SettingsAck(vec![(4, 100_000)])]);
    assert_eq!(server.next_timeout(), None);
    assert_eq!(
        server.handle_timeout(deadline + Duration::from_secs(1)),
        None
    );
}

/// Item 3. `Event::Settings` carries the peer's frame as sent - order,
/// duplicates and unknown identifiers included - and `remote_settings`
/// accumulates what is in force.
#[test]
fn settings_event_carries_the_peers_parameters() {
    let mut server = Connection::new(Role::Server, Limits::default()).unwrap();
    let mut wire = http2::PREFACE.to_vec();
    let mut payload = Vec::new();
    for (id, value) in [(4u16, 1000u32), (0x7a, 9), (4, 2000), (5, 20000)] {
        payload.extend_from_slice(&id.to_be_bytes());
        payload.extend_from_slice(&value.to_be_bytes());
    }
    http2::encode_frame(4, 0, 0, &payload, &mut wire).unwrap();
    let step = server.receive(&wire).unwrap();
    wire.drain(..step.consumed);
    let step = server.receive(&wire).unwrap();
    let Some(Event::Settings(frame)) = step.event else {
        panic!("expected Settings, got {:?}", step.event);
    };
    assert_eq!(
        frame.iter().collect::<Vec<_>>(),
        vec![(4, 1000), (0x7a, 9), (4, 2000), (5, 20000)]
    );
    assert_eq!(
        frame.get(4),
        Some(2000),
        "the last value is the one in force"
    );
    assert_eq!(frame.get(1), None);
    assert_eq!(
        server.remote_settings().iter().collect::<Vec<_>>(),
        vec![(4, 2000), (5, 20000), (0x7a, 9)]
    );

    // An empty SETTINGS frame is an event too, with nothing in it.
    let mut wire = Vec::new();
    http2::encode_frame(4, 0, 0, &[], &mut wire).unwrap();
    let step = server.receive(&wire).unwrap();
    assert!(matches!(step.event, Some(Event::Settings(frame)) if frame.is_empty()));
}

/// Item 4. GOAWAY's opaque data reaches the host, and "none" stays
/// distinguishable from data - Node reports `undefined`, not an empty buffer.
#[test]
fn goaway_carries_its_debug_data_and_absent_stays_absent() {
    let (mut client, mut server) = handshake();
    server.goaway(11, 0, b"enhance").unwrap();
    assert_eq!(
        drive(&mut client, &mut ship(&mut server)).unwrap(),
        vec![Seen::Goaway(11, Some(b"enhance".to_vec()))]
    );
    let (mut client, mut server) = handshake();
    server.shutdown().unwrap();
    assert_eq!(
        drive(&mut client, &mut ship(&mut server)).unwrap(),
        vec![Seen::Goaway(0, None)]
    );
}

/// Item 5. The initial SETTINGS is the host's choice - empty, as Node's server
/// sends it, or any parameters, serialised in ascending identifier order.
#[test]
fn initial_settings_are_host_chosen_and_ordered() {
    let mut server =
        Connection::with_settings(Role::Server, Limits::default(), &Settings::new()).unwrap();
    assert_eq!(server.output(), [0, 0, 0, 4, 0, 0, 0, 0, 0]);

    // Set out of order; written in ascending order.
    let mut chosen = Settings::new();
    chosen
        .set(Settings::MAX_HEADER_LIST_SIZE, 1 << 16)
        .set(Settings::HEADER_TABLE_SIZE, 4096)
        .set(Settings::ENABLE_CONNECT_PROTOCOL, 1);
    let ordered = Connection::with_settings(Role::Server, Limits::default(), &chosen).unwrap();
    let mut expected = Vec::new();
    http2::encode_frame(
        4,
        0,
        0,
        &[
            0, 1, 0, 0, 0x10, 0, // HEADER_TABLE_SIZE 4096
            0, 6, 0, 1, 0, 0, // MAX_HEADER_LIST_SIZE 65536
            0, 8, 0, 0, 0, 1, // ENABLE_CONNECT_PROTOCOL 1
        ],
        &mut expected,
    )
    .unwrap();
    assert_eq!(ordered.output(), expected);

    // `new` still advertises its limits, now in ascending order for a client
    // too: ENABLE_PUSH (2) first rather than appended.
    let client = Connection::new(Role::Client, Limits::default()).unwrap();
    let settings = &client.output()[http2::PREFACE.len()..];
    let frame = http2::decode_frame(settings, 16384).unwrap().unwrap();
    let ids: Vec<u8> = frame.payload.chunks(6).map(|s| s[1]).collect();
    assert_eq!(ids, [2, 3, 5, 6]);

    // A client that names no ENABLE_PUSH still refuses push: it defaults to on.
    let client =
        Connection::with_settings(Role::Client, Limits::default(), &Settings::new()).unwrap();
    assert_eq!(
        &client.output()[http2::PREFACE.len()..],
        [0, 0, 6, 4, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0]
    );

    // An empty-SETTINGS server still serves a request end to end.
    let mut client = Connection::new(Role::Client, Limits::default()).unwrap();
    drive(&mut server, &mut ship(&mut client)).unwrap();
    drive(&mut client, &mut ship(&mut server)).unwrap();
    let id = client.open(&request(), true).unwrap();
    let seen = drive(&mut server, &mut ship(&mut client)).unwrap();
    assert_eq!(seen, vec![Seen::SettingsAck(vec![]), Seen::Headers(id)]);
}

/// Values this endpoint could not honour are refused before a byte is sent.
#[test]
fn settings_it_cannot_honour_are_refused() {
    let (_, mut server) = handshake();
    for (id, value) in [
        (Settings::ENABLE_PUSH, 1),
        (Settings::HEADER_TABLE_SIZE, 8192),
        (Settings::INITIAL_WINDOW_SIZE, 0x8000_0000),
        (Settings::MAX_FRAME_SIZE, 16383),
        (Settings::MAX_FRAME_SIZE, 0x0100_0000),
        (Settings::ENABLE_CONNECT_PROTOCOL, 2),
    ] {
        let mut bad = Settings::new();
        bad.set(id, value);
        assert!(server.settings(&bad).is_err(), "{id}={value}");
        assert!(
            Connection::with_settings(Role::Server, Limits::default(), &bad).is_err(),
            "{id}={value}"
        );
    }
    assert!(server.output().is_empty());
    assert_eq!(server.pending_settings(), 0);
}

/// Item 6. A live change applies to the running connection: a loosening at
/// once (the peer may use it before its ack arrives), a tightening on the ack.
#[test]
fn live_settings_change_applies_loosening_now_and_tightening_on_ack() {
    let (mut client, mut server) = handshake();
    let big = vec![0u8; 20000];

    // Loosen MAX_FRAME_SIZE. The client acts on it as soon as it reads the
    // frame, and its 20000-byte DATA may arrive before its ack does.
    let mut larger = Settings::new();
    larger.set(Settings::MAX_FRAME_SIZE, 32768);
    server.settings(&larger).unwrap();
    assert_eq!(
        server.limits().frame_size,
        32768,
        "a loosening applies at once"
    );
    drive(&mut client, &mut ship(&mut server)).unwrap();
    let ack_and_more = ship(&mut client);
    let id = client.open(&request(), false).unwrap();
    assert_eq!(client.send_data(id, &big, false).unwrap(), 20000);
    // Deliver the DATA first, then the ack.
    let mut data_first = ship(&mut client);
    let seen = drive(&mut server, &mut data_first).unwrap();
    assert_eq!(seen, vec![Seen::Headers(id), Seen::Data(id, 20000)]);
    let seen = drive(&mut server, &mut ack_and_more.clone()).unwrap();
    assert_eq!(seen, vec![Seen::SettingsAck(vec![(5, 32768)])]);
    server.release_capacity(id, 20000).unwrap();
    drive(&mut client, &mut ship(&mut server)).unwrap();

    // Tighten it again. Until the ack, a large frame is still legal - the
    // client may not have read the change yet.
    let mut smaller = Settings::new();
    smaller.set(Settings::MAX_FRAME_SIZE, 16384);
    server.settings(&smaller).unwrap();
    assert_eq!(
        server.limits().frame_size,
        32768,
        "a tightening waits for the ack"
    );
    let pending = ship(&mut server);
    assert_eq!(client.send_data(id, &big, false).unwrap(), 20000);
    let seen = drive(&mut server, &mut ship(&mut client)).unwrap();
    assert_eq!(seen, vec![Seen::Data(id, 20000)]);
    server.release_capacity(id, 20000).unwrap();
    drive(&mut client, &mut ship(&mut server)).unwrap();
    // Now the client reads it, acks, and splits its DATA to 16384.
    drive(&mut client, &mut pending.clone()).unwrap();
    let seen = drive(&mut server, &mut ship(&mut client)).unwrap();
    assert_eq!(seen, vec![Seen::SettingsAck(vec![(5, 16384)])]);
    assert_eq!(server.limits().frame_size, 16384);
    assert_eq!(client.send_data(id, &big, false).unwrap(), 16384);
    // A peer that ignores the acknowledged limit is answered FRAME_SIZE_ERROR.
    let mut oversized = Vec::new();
    http2::encode_frame(0, 0, id, &big, &mut oversized).unwrap();
    assert_eq!(drive(&mut server, &mut oversized), Err("FRAME_SIZE_ERROR"));
}

/// Item 6, streams: tightening MAX_CONCURRENT_STREAMS applies on the ack.
/// Streams the peer opened before it read the change are accepted; one past
/// the new limit afterwards is refused as a stream error.
#[test]
fn live_stream_limit_applies_on_ack() {
    let (mut client, mut server) = handshake();
    let mut one = Settings::new();
    one.set(Settings::MAX_CONCURRENT_STREAMS, 1);
    server.settings(&one).unwrap();
    assert_eq!(
        server.limits().streams,
        100,
        "a tightening waits for the ack"
    );
    let pending = ship(&mut server);
    let a = client.open(&request(), false).unwrap();
    let b = client.open(&request(), false).unwrap();
    let seen = drive(&mut server, &mut ship(&mut client)).unwrap();
    assert_eq!(seen, vec![Seen::Headers(a), Seen::Headers(b)]);

    drive(&mut client, &mut pending.clone()).unwrap();
    let seen = drive(&mut server, &mut ship(&mut client)).unwrap();
    assert_eq!(seen, vec![Seen::SettingsAck(vec![(3, 1)])]);
    assert_eq!(server.limits().streams, 1);
    // The client now respects the limit itself...
    assert!(client.open(&request(), true).is_err());
    // ...and a peer that does not is refused: REFUSED_STREAM, not a
    // connection error. The block is static-table only (:method POST,
    // :scheme http, :path /), so it decodes against any HPACK state.
    let mut wire = Vec::new();
    http2::encode_frame(1, 5, 5, &[0x83, 0x86, 0x84], &mut wire).unwrap();
    assert_eq!(
        drive(&mut server, &mut wire).unwrap(),
        vec![Seen::Reset(5, 7)]
    );
}

/// Item 6, windows: INITIAL_WINDOW_SIZE moves every open stream's receive
/// window by the difference (RFC 9113 section 6.9.2) once acknowledged, and
/// new streams start at the new size.
#[test]
fn live_initial_window_change_adjusts_streams() {
    let (mut client, mut server) = handshake();
    let a = client.open(&request(), false).unwrap();
    drive(&mut server, &mut ship(&mut client)).unwrap();
    let mut small = Settings::new();
    small.set(Settings::INITIAL_WINDOW_SIZE, 1000);
    server.settings(&small).unwrap();
    let pending = ship(&mut server);
    // Before the client reads it, 2000 bytes on `a` are within the old window.
    assert_eq!(client.send_data(a, &[0; 2000], false).unwrap(), 2000);
    let seen = drive(&mut server, &mut ship(&mut client)).unwrap();
    assert_eq!(seen, vec![Seen::Data(a, 2000)]);

    drive(&mut client, &mut pending.clone()).unwrap();
    let seen = drive(&mut server, &mut ship(&mut client)).unwrap();
    assert_eq!(seen, vec![Seen::SettingsAck(vec![(4, 1000)])]);
    // A new stream starts at 1000, on both sides.
    let b = client.open(&request(), false).unwrap();
    assert_eq!(client.send_data(b, &[0; 2000], false).unwrap(), 1000);
    let seen = drive(&mut server, &mut ship(&mut client)).unwrap();
    assert_eq!(seen, vec![Seen::Headers(b), Seen::Data(b, 1000)]);
    // `a` is now at 1000 - 65535 + (65535 - 2000) = -1000: one more byte on it
    // overruns the window the peer was told about.
    let mut wire = Vec::new();
    http2::encode_frame(0, 0, a, &[0], &mut wire).unwrap();
    assert_eq!(drive(&mut server, &mut wire), Err("FLOW_CONTROL_ERROR"));
}
