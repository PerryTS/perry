use std::time::{Duration, Instant};
use turnloop_http::{client::*, hpack, http1::*, http2};
fn hex(s: &str) -> Vec<u8> {
    s.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|x| u8::from_str_radix(std::str::from_utf8(x).unwrap(), 16).unwrap())
        .collect()
}
#[test]
fn rfc_hpack_requests_and_huffman() {
    for cases in [
        [
            "828684410f7777772e6578616d706c652e636f6d",
            "828684be58086e6f2d6361636865",
            "828785bf400a637573746f6d2d6b65790c637573746f6d2d76616c7565",
        ],
        [
            "828684418cf1e3c2e5f23a6ba0ab90f4ff",
            "828684be5886a8eb10649cbf",
            "828785bf408825a849e95ba97d7f8925a849e95bb8e8b4bf",
        ],
    ] {
        let mut decoder = hpack::Decoder::new(4096, 32768);
        let mut out = Vec::new();
        for (i, case) in cases.iter().enumerate() {
            decoder.decode(&hex(case), &mut out).unwrap();
            assert_eq!(out[0], Header::new(":method", "GET"));
            assert_eq!(out[3], Header::new(":authority", "www.example.com"));
            if i == 1 {
                assert_eq!(out[4], Header::new("cache-control", "no-cache"));
            }
            if i == 2 {
                assert_eq!(out[4], Header::new("custom-key", "custom-value"));
            }
        }
    }
    let mut encoded = Vec::new();
    hpack::huffman_encode(b"www.example.com", &mut encoded);
    assert_eq!(encoded, hex("f1e3c2e5f23a6ba0ab90f4ff"));
    for invalid in ["ff", "ffffffff", "00", "fffffffc"] {
        assert!(
            hpack::huffman_decode(&hex(invalid), &mut Vec::new(), 1024).is_err(),
            "{invalid}"
        );
    }
    let all: Vec<u8> = (0..=255).collect();
    encoded.clear();
    hpack::huffman_encode(&all, &mut encoded);
    let mut decoded = Vec::new();
    hpack::huffman_decode(&encoded, &mut decoded, 256).unwrap();
    assert_eq!(decoded, all);
    let mut ints = Vec::new();
    hpack::encode_integer(1337, 5, 0, &mut ints);
    assert_eq!(ints, hex("1f9a0a"));
}
#[test]
fn hpack_roundtrip_evictions_and_limits() {
    let mut encoder = hpack::Encoder::new(128);
    let mut decoder = hpack::Decoder::new(128, 1024);
    let mut wire = Vec::new();
    let mut out = Vec::new();
    for i in 0..100 {
        let fields = vec![
            Header::new(":method", "GET"),
            Header::new("custom", i.to_string()),
            Header::new("authorization", "secret"),
        ];
        wire.clear();
        encoder.encode(&fields, &mut wire);
        decoder.decode(&wire, &mut out).unwrap();
        assert_eq!(out, fields);
    }
    assert!(
        hpack::Decoder::new(4096, 10)
            .decode(&[0x82], &mut out)
            .is_err()
    );
    hpack::Decoder::new(4096, 32768)
        .decode(&hex("3fe11f"), &mut out)
        .unwrap();
    for invalid in [
        "80",
        "ff00",
        "3fe21f",
        "8220",
        "4081ff00",
        "ffffffffffffffffffffff7f",
    ] {
        assert!(
            hpack::Decoder::new(4096, 32768)
                .decode(&hex(invalid), &mut out)
                .is_err(),
            "{invalid}"
        );
    }
}
fn collect_fragmented(wire: &[u8], method: &str) -> (Vec<u8>, Vec<Header>, bool) {
    let mut decoder = Decoder::new(Mode::Response, Limits::default());
    decoder.response_to(method);
    let mut buffered = Vec::new();
    let mut body = Vec::new();
    let mut trailers = Vec::new();
    let mut ended = false;
    for b in wire {
        buffered.push(*b);
        loop {
            let step = decoder.receive(&buffered).unwrap();
            let progressed = step.consumed != 0 || step.event.is_some();
            match step.event {
                Some(Event::Body(b)) => body.extend_from_slice(b),
                Some(Event::Trailers(t)) => trailers = t,
                Some(Event::End) => ended = true,
                _ => {}
            }
            buffered.drain(..step.consumed);
            if !progressed {
                break;
            }
        }
    }
    assert!(ended);
    (body, trailers, decoder.reusable())
}
#[test]
fn chunked_trailers_fragmentation_and_lengths() {
    let(body,trailers,reuse)=collect_fragmented(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3;x=y\r\nabc\r\n2\r\nde\r\n0\r\nx-digest: yes\r\n\r\n","GET");
    assert_eq!(body, b"abcde");
    assert_eq!(trailers, [Header::new("x-digest", "yes")]);
    assert!(reuse);
    let (body, _, reuse) = collect_fragmented(
        b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc",
        "GET",
    );
    assert_eq!(body, b"abc");
    assert!(!reuse);
    let (body, _, _) =
        collect_fragmented(b"HTTP/1.1 200 OK\r\nContent-Length: 999\r\n\r\n", "HEAD");
    assert!(body.is_empty());
}
#[test]
fn smuggling_rejected_and_failure_sticky() {
    let heads = [
        "Content-Length: 3\r\nTransfer-Encoding: chunked",
        "Content-Length: 3\r\nContent-Length: 3",
        "Content-Length: 3, 3",
        "Content-Length: +3",
        "Content-Length: 18446744073709551616",
        "Transfer-Encoding: chunked, chunked",
        "Transfer-Encoding: gzip, chunked",
        "Transfer-Encoding : chunked",
        "X: one\r\n folded",
        "X: a\nY: b",
    ];
    for headers in heads {
        let wire = format!("POST / HTTP/1.1\r\nHost: localhost\r\n{headers}\r\n\r\n");
        let mut decoder = Decoder::new(Mode::Request, Limits::default());
        assert!(decoder.receive(wire.as_bytes()).is_err(), "{headers}");
        assert!(
            decoder
                .receive(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .is_err()
        );
    }
    for wire in [
        b"GET / HTTP/1.1\r\n\r\n".as_slice(),
        b"GET / HTTP/1.1\r\nHost: a\r\nHost: b\r\n\r\n",
    ] {
        assert!(
            Decoder::new(Mode::Request, Limits::default())
                .receive(wire)
                .is_err()
        );
    }
}
#[test]
fn informational_upgrade_and_eof() {
    let mut decoder = Decoder::new(Mode::Response, Limits::default());
    assert!(matches!(
        decoder
            .receive(b"HTTP/1.1 100 Continue\r\n\r\n")
            .unwrap()
            .event,
        Some(Event::Informational(_))
    ));
    decoder.response_to("CONNECT");
    let wire = b"HTTP/1.1 200 Connected\r\n\r\nTLS";
    let n = decoder.receive(wire).unwrap().consumed;
    assert!(matches!(
        decoder.receive(&wire[n..]).unwrap().event,
        Some(Event::Upgrade)
    ));
    assert_eq!(&wire[n..], b"TLS");
    let mut decoder = Decoder::new(Mode::Response, Limits::default());
    decoder
        .receive(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\n")
        .unwrap();
    decoder.receive(b"x").unwrap();
    assert!(decoder.eof().is_err());
    let mut decoder = Decoder::new(Mode::Response, Limits::default());
    decoder.receive(b"HTTP/1.0 200 OK\r\n\r\n").unwrap();
    assert!(matches!(
        decoder.receive(b"abc").unwrap().event,
        Some(Event::Body(b"abc"))
    ));
    decoder.eof().unwrap();
    assert!(matches!(
        decoder.receive(&[]).unwrap().event,
        Some(Event::End)
    ));
    assert!(!decoder.reusable());
}
#[test]
fn serialization_validates_before_writing() {
    let request = Request::new("http://localhost/hello", "POST").unwrap();
    let head = request.head(false);
    let mut wire = Vec::new();
    let mut encoder = Encoder::start(&head, BodyLength::Chunked, &mut wire).unwrap();
    encoder.body(b"abc", &mut wire).unwrap();
    encoder
        .finish(&[Header::new("digest", "yes")], &mut wire)
        .unwrap();
    assert!(wire.ends_with(b"3\r\nabc\r\n0\r\ndigest: yes\r\n\r\n"));
    let mut head = head;
    head.headers.push(Header::new("x", "bad\r\nInjected: yes"));
    let before = wire.clone();
    assert!(Encoder::start(&head, BodyLength::Empty, &mut wire).is_err());
    assert_eq!(before, wire);
}
fn request_headers() -> Vec<Header> {
    vec![
        Header::new(":method", "GET"),
        Header::new(":scheme", "http"),
        Header::new(":path", "/"),
        Header::new(":authority", "localhost"),
    ]
}
fn transfer(from: &mut http2::Connection, to: &mut http2::Connection) -> usize {
    let wire = from.output().to_vec();
    from.consume_output(wire.len()).unwrap();
    let mut pos = 0;
    let mut events = 0;
    while pos < wire.len() {
        let step = to.receive(&wire[pos..]).unwrap();
        assert!(step.consumed > 0);
        pos += step.consumed;
        if step.event.is_some() {
            events += 1;
        }
    }
    events
}
#[test]
fn h2_hundred_streams_flow_control_abort_and_goaway() {
    let mut client = http2::Connection::new(http2::Role::Client, http2::Limits::default()).unwrap();
    let mut server = http2::Connection::new(http2::Role::Server, http2::Limits::default()).unwrap();
    transfer(&mut client, &mut server);
    transfer(&mut server, &mut client);
    transfer(&mut client, &mut server);
    let ids: Vec<_> = (0..100)
        .map(|_| client.open(&request_headers(), true).unwrap())
        .collect();
    assert_eq!(transfer(&mut client, &mut server), 100);
    assert!(client.open(&request_headers(), true).is_err());
    for id in &ids {
        server
            .send_headers(*id, &[Header::new(":status", "200")], false)
            .unwrap();
    }
    assert_eq!(transfer(&mut server, &mut client), 100);
    let data = vec![7; 100_000];
    let id = ids[0];
    let mut sent = 0;
    loop {
        let n = server.send_data(id, &data[sent..], false).unwrap();
        if n == 0 {
            break;
        }
        sent += n;
    }
    assert_eq!(sent, 65535);
    assert_eq!(server.send_data(ids[1], b"blocked", true).unwrap(), 0);
    transfer(&mut server, &mut client);
    client.release_capacity(id, 65535).unwrap();
    transfer(&mut client, &mut server);
    assert_eq!(server.send_data(ids[1], b"ready", true).unwrap(), 5);
    transfer(&mut server, &mut client);
    client.reset(id, 8).unwrap();
    assert_eq!(transfer(&mut client, &mut server), 1);
    server.shutdown().unwrap();
    assert_eq!(transfer(&mut server, &mut client), 1);
    assert!(client.open(&request_headers(), true).is_err());
}
#[test]
fn h2_continuation_flood_and_invalid_frames() {
    let mut server = http2::Connection::new(
        http2::Role::Server,
        http2::Limits {
            continuations: 1,
            ..Default::default()
        },
    )
    .unwrap();
    server.receive(http2::PREFACE).unwrap();
    let mut wire = Vec::new();
    http2::encode_frame(4, 0, 0, &[], &mut wire).unwrap();
    server.receive(&wire).unwrap();
    wire.clear();
    http2::encode_frame(1, 0, 1, &[], &mut wire).unwrap();
    server.receive(&wire).unwrap();
    wire.clear();
    http2::encode_frame(9, 0, 1, &[], &mut wire).unwrap();
    server.receive(&wire).unwrap();
    assert_eq!(
        server.receive(&wire).err().unwrap().code,
        "ENHANCE_YOUR_CALM"
    );
    for (kind, stream, payload) in [(6, 0, vec![0; 7]), (4, 1, vec![]), (8, 0, vec![0; 4])] {
        let mut s = http2::Connection::new(http2::Role::Server, Default::default()).unwrap();
        s.receive(http2::PREFACE).unwrap();
        let mut wire = Vec::new();
        http2::encode_frame(4, 0, 0, &[], &mut wire).unwrap();
        s.receive(&wire).unwrap();
        wire.clear();
        http2::encode_frame(kind, 0, stream, &payload, &mut wire).unwrap();
        assert!(s.receive(&wire).is_err());
    }
}
#[test]
fn redirect_pool_proxy_deadline_policy() {
    let mut request = Request::new("https://a.example/start", "POST").unwrap();
    request.body = b"data".to_vec();
    request.headers = vec![
        Header::new("authorization", "secret"),
        Header::new("content-type", "text/plain"),
    ];
    assert!(
        request
            .redirect(
                302,
                Some("https://b.example/next"),
                RedirectMode::Follow,
                20
            )
            .unwrap()
    );
    assert_eq!(request.method, "GET");
    assert!(request.body.is_empty());
    assert!(request.headers.is_empty());
    assert!(
        !request
            .redirect(307, Some("/x"), RedirectMode::Manual, 20)
            .unwrap()
    );
    let now = Instant::now();
    let key = PoolKey::new(&request.url, None);
    let mut pool = Pool::new(1, Duration::from_secs(2));
    let Acquire::Connect(id) = pool.acquire(&key, now) else {
        panic!()
    };
    assert_eq!(pool.acquire(&key, now), Acquire::Wait);
    pool.connected(id, Protocol::Http2, 100).unwrap();
    for _ in 1..100 {
        assert_eq!(pool.acquire(&key, now), Acquire::Reuse(id));
    }
    assert_eq!(pool.acquire(&key, now), Acquire::Wait);
    for _ in 0..100 {
        pool.release(id, true, now).unwrap();
    }
    assert_eq!(pool.next_timeout(), Some(now + Duration::from_secs(2)));
    assert_eq!(pool.handle_timeout(now + Duration::from_secs(2)), Some(id));
    assert_eq!(pool.handle_timeout(now + Duration::from_secs(2)), None);
    let env = ProxyEnvironment {
        http_proxy: Some("http://localhost:8080".into()),
        https_proxy: Some("http://localhost:8081".into()),
        no_proxy: ".example, localhost:8000".into(),
    };
    assert!(env.proxy_for(&request.url).unwrap().is_none());
    let target = Request::new("https://elsewhere.test/", "GET").unwrap().url;
    let mut route = Route::new(target.clone(), env.proxy_for(&target).unwrap());
    assert_eq!(
        route.connect_head(None).unwrap().target,
        "elsewhere.test:443"
    );
    assert!(matches!(
        route.tunnel_response(200).unwrap(),
        TransportRequest::UpgradeTls { .. }
    ));
    let mut op = Lifecycle::default();
    op.transition(Phase::Body, Some(now));
    op.abort();
    op.handle_timeout(now);
    assert_eq!(
        op.poll(),
        Some(Completion::Error(turnloop_http::Error::new(
            "UND_ERR_ABORTED",
            "Request aborted"
        )))
    );
    assert_eq!(op.poll(), None);
}
#[test]
fn gzip_deflate_br_zstd_decoding() {
    use std::io::Write;
    let body = b"hello compression hello compression";
    let mut encoded = flate2::write::GzEncoder::new(Vec::new(), Default::default());
    encoded.write_all(body).unwrap();
    let gzip = encoded.finish().unwrap();
    let mut out = Vec::new();
    turnloop_http::compression::decode("gzip", &gzip, &mut out, 100).unwrap();
    assert_eq!(out, body);
    assert!(turnloop_http::compression::decode("gzip", &gzip, &mut out, 2).is_err());
    let mut encoded = flate2::write::ZlibEncoder::new(Vec::new(), Default::default());
    encoded.write_all(body).unwrap();
    turnloop_http::compression::decode("deflate", &encoded.finish().unwrap(), &mut out, 100)
        .unwrap();
    assert_eq!(out, body);
    let mut encoded = Vec::new();
    {
        let mut encoder = brotli::CompressorWriter::new(&mut encoded, 4096, 4, 22);
        encoder.write_all(body).unwrap();
    }
    turnloop_http::compression::decode("br", &encoded, &mut out, 100).unwrap();
    assert_eq!(out, body);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let encoded = zstd::stream::encode_all(body.as_slice(), 1).unwrap();
        turnloop_http::compression::decode("zstd", &encoded, &mut out, 100).unwrap();
        assert_eq!(out, body);
    }
}

#[test]
fn vendored_hpack_interop_corpus() {
    use std::io::Read;
    let mut corpus = String::new();
    flate2::read::GzDecoder::new(include_bytes!("hpack-corpus/stories.txt.gz").as_slice())
        .read_to_string(&mut corpus)
        .unwrap();
    let mut decoder = hpack::Decoder::new(4096, 1 << 20);
    let mut actual = Vec::new();
    let mut expected = Vec::new();
    let mut story = "";
    let mut count = 0;
    for line in corpus.lines() {
        if let Some(name) = line.strip_prefix("S ") {
            story = name;
            decoder = hpack::Decoder::new(4096, 1 << 20);
        } else if let Some(wire) = line.strip_prefix("C ") {
            decoder
                .decode(&hex(wire), &mut actual)
                .unwrap_or_else(|e| panic!("{story} case {count}: {e}"));
            expected.clear();
        } else if let Some(header) = line.strip_prefix("H ") {
            let (n, v) = header.split_once(' ').unwrap();
            expected.push(Header {
                name: String::from_utf8(hex(n)).unwrap(),
                value: hex(v),
            });
        } else if line == "E" {
            assert_eq!(actual, expected, "{story}, case {count}");
            count += 1;
        } else {
            panic!("bad fixture")
        }
    }
    assert_eq!(count, 3754, "corpus cases must all run");
}

#[test]
fn expect_continue_streaming_abort_and_reuse() {
    let now = Instant::now();
    let mut connection = Http1Connection::new(Default::default());
    let mut head = Request::new("http://localhost/upload", "POST")
        .unwrap()
        .head(false);
    head.headers.push(Header::new("expect", "100-continue"));
    connection
        .start(
            &head,
            BodyLength::Known(4),
            Some(now + Duration::from_secs(5)),
            Some(now + Duration::from_secs(1)),
        )
        .unwrap();
    assert!(!connection.can_send_body());
    assert!(connection.send_body(b"body").is_err());
    let n = connection.output().len();
    connection.consume_output(n).unwrap();
    assert!(
        connection
            .start(&head, BodyLength::Empty, None, None)
            .is_err()
    );
    connection
        .receive(b"HTTP/1.1 100 Continue\r\n\r\n")
        .unwrap();
    assert!(connection.can_send_body());
    connection.send_body(b"bo").unwrap();
    connection.send_body(b"dy").unwrap();
    connection.finish_body(&[]).unwrap();
    assert_eq!(connection.output(), b"body");
    connection.consume_output(4).unwrap();
    connection
        .receive(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n")
        .unwrap();
    assert_eq!(connection.receive(b"OK").unwrap().consumed, 2);
    connection.receive(&[]).unwrap();
    assert_eq!(connection.poll_completion(), Some(Completion::Success));
    assert_eq!(connection.poll_completion(), None);
    assert!(connection.reusable());
    connection
        .start(&head, BodyLength::Known(4), None, Some(now))
        .unwrap();
    connection.handle_timeout(now);
    assert!(connection.can_send_body());
    connection.abort();
    assert_eq!(
        connection.poll_completion().unwrap(),
        Completion::Error(turnloop_http::Error::new(
            "UND_ERR_ABORTED",
            "Request aborted"
        ))
    );
    assert_eq!(connection.poll_completion(), None);
    assert!(!connection.reusable());
}

const STREAMED: &[u8] = b"streamed body streamed body streamed body";
/// Encode `body` with one content coding. zstd has no encoder on every target
/// the suite runs on, so it has a fixture for `STREAMED` only.
fn encode_with(coding: &str, body: &[u8]) -> Vec<u8> {
    use std::io::Write;
    match coding {
        "gzip" => {
            let mut gzip = flate2::write::GzEncoder::new(Vec::new(), Default::default());
            gzip.write_all(body).unwrap();
            gzip.finish().unwrap()
        }
        "deflate" => {
            let mut deflate = flate2::write::DeflateEncoder::new(Vec::new(), Default::default());
            deflate.write_all(body).unwrap();
            deflate.finish().unwrap()
        }
        "br" => {
            let mut br = Vec::new();
            {
                let mut writer = brotli::CompressorWriter::new(&mut br, 4096, 4, 22);
                writer.write_all(body).unwrap();
            }
            br
        }
        "zstd" => {
            assert_eq!(body, STREAMED, "zstd has a fixture for STREAMED only");
            // Generated by Node 26 zlib.zstdCompressSync, also run on WASI.
            hex("28b52ffd2029a500007073747265616d656420626f6479200100114e25")
        }
        _ => panic!("{coding}"),
    }
}
const CODINGS: [&str; 4] = ["gzip", "deflate", "br", "zstd"];
#[test]
fn streaming_compression_fragmented_bounded_and_truncated() {
    let body = STREAMED;
    let cases = CODINGS.map(|coding| (coding, encode_with(coding, body)));
    for (encoding, wire) in cases {
        let mut decoder = turnloop_http::compression::StreamingDecoder::new(encoding, 100).unwrap();
        let mut input = Vec::new();
        let mut result = Vec::new();
        let mut done = false;
        for (i, byte) in wire.iter().enumerate() {
            input.push(*byte);
            loop {
                let mut out = [0; 3];
                let step = decoder
                    .process(&input, &mut out, i + 1 == wire.len())
                    .unwrap();
                result.extend_from_slice(&out[..step.written]);
                input.drain(..step.consumed);
                done = step.finished;
                if done || step.consumed == 0 && step.written == 0 {
                    break;
                }
            }
        }
        if !done {
            let mut out = [0; 100];
            let step = decoder.process(&input, &mut out, true).unwrap();
            result.extend_from_slice(&out[..step.written]);
            done = step.finished;
        }
        assert!(done, "{encoding}");
        assert_eq!(result, body, "{encoding}");
        let mut decoder = turnloop_http::compression::StreamingDecoder::new(encoding, 100).unwrap();
        let mut pos = 0;
        let mut failure = false;
        for _ in 0..100 {
            let mut out = [0; 100];
            match decoder.process(&wire[pos..wire.len() - 1], &mut out, true) {
                Ok(step) => {
                    pos += step.consumed;
                    if step.finished {
                        break;
                    }
                }
                Err(_) => {
                    failure = true;
                    break;
                }
            }
        }
        assert!(failure, "truncated {encoding} must fail");
    }
}

#[test]
fn h2_bodyless_responses_send_lengths_and_failure_completion() {
    let mut client = http2::Connection::new(http2::Role::Client, Default::default()).unwrap();
    let mut server = http2::Connection::new(http2::Role::Server, Default::default()).unwrap();
    transfer(&mut client, &mut server);
    transfer(&mut server, &mut client);
    transfer(&mut client, &mut server);
    let id = client.open(&request_headers(), true).unwrap();
    transfer(&mut client, &mut server);
    server
        .send_headers(
            id,
            &[
                Header::new(":status", "304"),
                Header::new("content-length", "100"),
            ],
            true,
        )
        .unwrap();
    transfer(&mut server, &mut client);
    let id = client.open(&request_headers(), true).unwrap();
    transfer(&mut client, &mut server);
    assert!(
        server
            .send_headers(
                id,
                &[
                    Header::new(":status", "200"),
                    Header::new("content-length", "100")
                ],
                true
            )
            .is_err()
    );
    server
        .send_headers(
            id,
            &[
                Header::new(":status", "200"),
                Header::new("content-length", "2"),
            ],
            false,
        )
        .unwrap();
    assert!(server.send_data(id, b"too much", true).is_err());
    assert_eq!(server.send_data(id, b"ok", true).unwrap(), 2);
    transfer(&mut server, &mut client);
    let last = client.open(&request_headers(), true).unwrap();
    client.eof();
    assert_eq!(client.poll_failed_stream(), Some(last));
    assert_eq!(client.poll_failed_stream(), None);
    let mut client = http2::Connection::new(http2::Role::Client, Default::default()).unwrap();
    let now = Instant::now();
    client.set_settings_deadline(Some(now));
    assert_eq!(client.handle_timeout(now).unwrap().code, "SETTINGS_TIMEOUT");
    assert!(client.handle_timeout(now).is_none());
}

#[test]
fn h2_negative_initial_window_stalls_and_recovers() {
    let mut client = http2::Connection::new(http2::Role::Client, Default::default()).unwrap();
    let mut server = http2::Connection::new(http2::Role::Server, Default::default()).unwrap();
    transfer(&mut client, &mut server);
    transfer(&mut server, &mut client);
    transfer(&mut client, &mut server);
    let id = client.open(&request_headers(), true).unwrap();
    transfer(&mut client, &mut server);
    server
        .send_headers(id, &[Header::new(":status", "200")], false)
        .unwrap();
    assert_eq!(server.send_data(id, &[7; 20], false).unwrap(), 20);
    let mut wire = Vec::new();
    http2::encode_frame(4, 0, 0, &[0, 4, 0, 0, 0, 0], &mut wire).unwrap();
    server.receive(&wire).unwrap();
    assert_eq!(server.send_data(id, b"stalled", false).unwrap(), 0);
    wire.clear();
    http2::encode_frame(8, 0, id, &21u32.to_be_bytes(), &mut wire).unwrap();
    server.receive(&wire).unwrap();
    assert_eq!(server.send_data(id, b"resume", false).unwrap(), 1);
}
#[test]
fn proxy_credentials_and_cross_origin_mixed_case_headers() {
    let mut request = Request::new("http://origin.test/path#fragment", "POST").unwrap();
    request.headers.push(Header {
        name: "Authorization".into(),
        value: b"secret".to_vec(),
    });
    request.headers.push(Header {
        name: "Content-Type".into(),
        value: b"text/plain".to_vec(),
    });
    request
        .redirect(
            302,
            Some("http://other.test/"),
            RedirectMode::Follow,
            DEFAULT_MAX_REDIRECTS,
        )
        .unwrap();
    assert!(request.headers.is_empty());
    let proxy = url::Url::parse("http://us%65r:p%61ss@localhost:8080").unwrap();
    let route = Route::new(request.url.clone(), Some(proxy.clone()));
    let head = route.request_head(&request, None);
    assert_eq!(head.target, "http://other.test/");
    assert_eq!(
        head.get("proxy-authorization"),
        Some(b"Basic dXNlcjpwYXNz".as_slice())
    );
    let request = Request::new("https://origin.test/", "GET").unwrap();
    let route = Route::new(request.url.clone(), Some(proxy));
    assert_eq!(
        route.connect_head(None).unwrap().get("proxy-authorization"),
        Some(b"Basic dXNlcjpwYXNz".as_slice())
    );
    assert!(
        route
            .request_head(&request, None)
            .get("proxy-authorization")
            .is_none()
    );
}

// --- HTTP/2 stream-lifetime contract -----------------------------------------
//
// h2spec's strict suite drives the subject from the peer side only: every
// RST_STREAM in its 5.1 "Stream States" family is one h2spec sends, never one
// the server decides to send, and its example server never closes gracefully.
// The afterlife of a stream the server itself terminated is outside what it can
// reach, and that is where every test below lives.

/// Drive `to` until it stops making progress, returning the events it produced.
/// The loop condition is the documented one: `consumed > 0 || event.is_some()`.
fn drive(to: &mut http2::Connection, input: &mut Vec<u8>) -> Result<Vec<String>, &'static str> {
    let mut seen = Vec::new();
    loop {
        let step = match to.receive(input) {
            Ok(step) => step,
            Err(e) => return Err(e.code),
        };
        let consumed = step.consumed;
        let progressed = consumed > 0 || step.event.is_some();
        if let Some(event) = step.event {
            seen.push(match event {
                http2::Event::Settings(_) => "Settings".to_string(),
                http2::Event::SettingsAck(_) => "SettingsAck".to_string(),
                http2::Event::Headers { stream, .. } => format!("Headers s={stream}"),
                http2::Event::Data { stream, bytes, .. } => {
                    format!("Data s={stream} n={}", bytes.len())
                }
                http2::Event::Reset { stream, code } => format!("Reset s={stream} code={code}"),
                http2::Event::Unprocessed { stream } => format!("Unprocessed s={stream}"),
                http2::Event::Goaway { code, .. } => format!("Goaway code={code}"),
                http2::Event::Ping { ack, .. } => format!("Ping ack={ack}"),
                http2::Event::WindowUpdate { stream } => format!("WindowUpdate s={stream}"),
            });
        }
        input.drain(..consumed);
        if !progressed {
            return Ok(seen);
        }
    }
}
fn ship(from: &mut http2::Connection) -> Vec<u8> {
    let wire = from.output().to_vec();
    from.consume_output(wire.len()).unwrap();
    wire
}
fn post_headers(path: &str) -> Vec<Header> {
    vec![
        Header::new(":method", "POST"),
        Header::new(":scheme", "http"),
        Header::new(":path", path),
        Header::new(":authority", "localhost"),
    ]
}
/// A handshaked client/server pair. The server's table holds `streams` streams;
/// the client's is large, so only the server's is ever under test.
fn handshake(streams: usize) -> (http2::Connection, http2::Connection) {
    let mut server = http2::Connection::new(
        http2::Role::Server,
        http2::Limits {
            streams,
            ..Default::default()
        },
    )
    .unwrap();
    let mut client = http2::Connection::new(http2::Role::Client, http2::Limits::default()).unwrap();
    let mut wire = ship(&mut client);
    drive(&mut server, &mut wire).unwrap();
    let mut wire = ship(&mut server);
    drive(&mut client, &mut wire).unwrap();
    let mut wire = ship(&mut client);
    drive(&mut server, &mut wire).unwrap();
    (client, server)
}

/// A stream reset while it still holds unreleased DATA must not burn its table
/// slot. `add_stream` only recycles a closed slot whose credit has been
/// returned, so a `reset` that left `unreleased` behind leaked the slot for the
/// life of the connection — and when the table filled, the REFUSED_STREAM came
/// out of `receive` as a *connection* error and the session died.
#[test]
fn h2_reset_with_unreleased_data_keeps_its_table_slot() {
    for release_first in [true, false] {
        let (mut client, mut server) = handshake(2);
        let mut accepted = 0;
        for i in 0..6 {
            let path = format!("/{i}");
            let id = client
                .open(&post_headers(&path), false)
                .unwrap_or_else(|e| panic!("client refused open #{i}: {}", e.code));
            assert_eq!(client.send_data(id, b"hello-body", true).unwrap(), 10);
            let mut wire = ship(&mut client);
            match drive(&mut server, &mut wire) {
                // Accepted means the request arrived - not merely that the
                // connection survived. A burnt table slot answers RST_STREAM.
                Ok(events) => {
                    assert_eq!(
                        events,
                        vec![format!("Headers s={id}"), format!("Data s={id} n=10")],
                        "stream #{i} was refused (release_first={release_first})"
                    );
                    accepted += 1;
                }
                Err(code) => panic!(
                    "server failed the connection on stream #{i} (release_first={release_first}): {code}"
                ),
            }
            if release_first {
                // The host that already knows about finding 1 and works around it.
                assert_eq!(server.unreleased(id), Some(10));
                server.release_capacity(id, 10).unwrap();
            }
            assert!(server.reset(id, 8).is_ok());
            // Ship RST_STREAM and any WINDOW_UPDATEs back so the client retires
            // its own record: the client's peer-stream limit must not be what
            // refuses the next open.
            let mut back = ship(&mut server);
            drive(&mut client, &mut back).unwrap();
        }
        assert_eq!(accepted, 6, "release_first={release_first}");
    }
}

/// A reset returns the stream's outstanding connection-level credit. Without
/// it the connection window shrinks by every un-released byte of every reset
/// stream until the peer can no longer send at all.
#[test]
fn h2_reset_returns_the_connection_window() {
    let (mut client, mut server) = handshake(100);
    let body = vec![7u8; 16384];
    let mut charged = 0;
    for i in 0..4 {
        let id = client.open(&post_headers(&format!("/{i}")), false).unwrap();
        assert_eq!(client.send_data(id, &body, false).unwrap(), 16384);
        charged += 16384;
        let mut wire = ship(&mut client);
        drive(&mut server, &mut wire).unwrap();
        server.reset(id, 8).unwrap();
        let mut back = ship(&mut server);
        drive(&mut client, &mut back).unwrap();
    }
    // Every charged byte came back as connection credit, so the client can
    // still fill a fresh 65535-byte connection window.
    assert!(charged > 0);
    let id = client.open(&post_headers("/last"), false).unwrap();
    let mut sent = 0;
    while sent < 65535 {
        let n = client
            .send_data(id, &body[..(65535 - sent).min(16384)], false)
            .unwrap();
        assert_ne!(n, 0, "connection window shrank by {charged} reset bytes");
        sent += n;
    }
    assert_eq!(sent, 65535);
}

/// Frames the peer had in flight when we reset a stream arrive after the
/// RST_STREAM. RFC 9113 §5.1: the endpoint that *sent* RST_STREAM must be
/// prepared to receive them, and may ignore them. Failing the connection here
/// is a race no host can avoid.
#[test]
fn h2_late_frames_for_a_locally_reset_stream_are_ignored() {
    let (mut client, mut server) = handshake(100);
    let id = client.open(&post_headers("/x"), false).unwrap();
    let mut wire = ship(&mut client);
    drive(&mut server, &mut wire).unwrap();
    server.reset(id, 8).unwrap();
    let _ = ship(&mut server);
    // The client has not seen the RST_STREAM yet.
    assert_eq!(client.send_data(id, b"in-flight", false).unwrap(), 9);
    client.reset(id, 8).unwrap();
    let mut late = ship(&mut client);
    let events = drive(&mut server, &mut late).expect("late frames must not fail the connection");
    assert_eq!(events, Vec::<String>::new());
    // The connection is still usable.
    let next = client.open(&post_headers("/y"), true).unwrap();
    let mut wire = ship(&mut client);
    assert_eq!(
        drive(&mut server, &mut wire).unwrap(),
        vec![format!("Headers s={next}")]
    );
}

/// A client that aborts a request resets the stream while the server's response
/// is already on the wire. Those HEADERS and DATA arrive for a stream whose
/// record is gone, and used to end the whole session — on the most ordinary
/// client operation there is.
#[test]
fn h2_response_in_flight_when_the_client_aborts_is_ignored() {
    let (mut client, mut server) = handshake(100);
    let first = client.open(&post_headers("/abort"), true).unwrap();
    let mut wire = ship(&mut client);
    drive(&mut server, &mut wire).unwrap();
    // The server answers; the client aborts before the answer lands.
    server
        .send_headers(first, &[Header::new(":status", "200")], false)
        .unwrap();
    server.send_data(first, b"payload", true).unwrap();
    client.reset(first, 8).unwrap();
    let _ = ship(&mut client);
    let mut answer = ship(&mut server);
    assert_eq!(
        drive(&mut client, &mut answer).expect("an aborted request must not fail the session"),
        Vec::<String>::new()
    );
    // HPACK survived the discarded block, so the next request still decodes.
    let second = client.open(&post_headers("/next"), true).unwrap();
    let mut wire = ship(&mut client);
    drive(&mut server, &mut wire).unwrap();
    server
        .send_headers(second, &[Header::new(":status", "201")], true)
        .unwrap();
    let mut answer = ship(&mut server);
    assert_eq!(
        drive(&mut client, &mut answer).unwrap(),
        vec![format!("Headers s={second}")]
    );
    // A stream the client never opened is still a connection error.
    let mut block = Vec::new();
    hpack::Encoder::new(4096).encode(&[Header::new(":status", "200")], &mut block);
    let mut wire = Vec::new();
    http2::encode_frame(1, 4, 99, &block, &mut wire).unwrap();
    assert_eq!(drive(&mut client, &mut wire), Err("PROTOCOL_ERROR"));
}

/// A host that buffers a body and releases capacity when the application
/// consumes it can call `release_capacity` after the stream is gone. That is a
/// no-op — the credit went back in bulk at termination — not an error.
#[test]
fn h2_release_after_termination_is_a_no_op() {
    let (mut client, mut server) = handshake(100);
    let id = client.open(&post_headers("/x"), false).unwrap();
    client.send_data(id, b"body", false).unwrap();
    let mut wire = ship(&mut client);
    drive(&mut server, &mut wire).unwrap();
    assert_eq!(server.unreleased(id), Some(4));
    server.reset(id, 8).unwrap();
    assert_eq!(server.unreleased(id), None);
    server.release_capacity(id, 4).unwrap();
    // An id this connection has never seen is still an error.
    assert!(server.release_capacity(99, 4).is_err());
    assert!(server.unreleased(99).is_none());
}

/// Exceeding the concurrent-stream limit is a stream error (RFC 9113 §5.1.2),
/// not a connection error. It was reaching `receive`'s error map as a
/// REFUSED_STREAM with no case of its own and going out as PROTOCOL_ERROR.
#[test]
fn h2_stream_limit_refuses_one_stream_not_the_connection() {
    let (mut client, mut server) = handshake(2);
    let mut open = Vec::new();
    for i in 0..2 {
        let id = client.open(&post_headers(&format!("/{i}")), false).unwrap();
        open.push(id);
    }
    let mut wire = ship(&mut client);
    drive(&mut server, &mut wire).unwrap();
    // A third concurrent stream, over the server's advertised limit of two.
    // The client is told MAX_CONCURRENT_STREAMS=2, so build the frame by hand.
    let third = 5;
    let mut encoder = hpack::Encoder::new(4096);
    let mut block = Vec::new();
    encoder.encode(&post_headers("/third"), &mut block);
    let mut wire = Vec::new();
    http2::encode_frame(1, 4, third, &block, &mut wire).unwrap();
    let events = drive(&mut server, &mut wire).expect("stream limit must not fail the connection");
    assert_eq!(events, vec![format!("Reset s={third} code=7")]);
    assert_eq!(
        &server.output()[3..4],
        &[3],
        "RST_STREAM answers the refusal"
    );
    // The two live streams are untouched.
    for id in open {
        server
            .send_headers(id, &[Header::new(":status", "200")], true)
            .unwrap();
    }
}

/// A stream opened after a graceful GOAWAY is the unavoidable race: the peer
/// cannot have seen the GOAWAY when it opened. It used to be a connection
/// error, decided inside `receive` where no host could reach it.
///
/// Node sends **no frame at all** here. Measured against Node 26.5.1 with a raw
/// TCP peer and hand-encoded frames, so no library could shape the answer: no
/// RST_STREAM, no second GOAWAY, no frame naming the stream; the request never
/// reaches the application; and the session stays alive and keeps servicing
/// frames (a PING sent afterwards is still acknowledged). RFC 9113 section 6.8
/// makes such a stream simply "not processed", to be retried on a new
/// connection. So the connection reports it and sends nothing: a frame it
/// emitted could not be un-emitted, and a host that must match Node would then
/// have no way back.
#[test]
fn h2_stream_after_graceful_goaway_is_unprocessed_and_unanswered() {
    let (mut client, mut server) = handshake(100);
    server.shutdown().unwrap();
    let _ = ship(&mut server);
    let late = client.open(&post_headers("/late"), true).unwrap();
    let mut wire = ship(&mut client);
    let events = drive(&mut server, &mut wire).expect("the GOAWAY race must not fail the session");
    assert_eq!(events, vec![format!("Unprocessed s={late}")]);
    assert!(
        server.output().is_empty(),
        "Node sends nothing here; so must we (got {:?})",
        server.output()
    );
    // HPACK state survived the discarded block, so the session still decodes.
    let again = client.open(&post_headers("/after"), true).unwrap();
    let mut wire = ship(&mut client);
    assert_eq!(
        drive(&mut server, &mut wire).unwrap(),
        vec![format!("Unprocessed s={again}")]
    );

    // The decision belongs to the host, not the connection: one that wants to
    // answer still can, and gets exactly one RST_STREAM for the stream it names.
    server.reset(late, 7).unwrap();
    let out = server.output().to_vec();
    assert_eq!(out.len(), 13, "one RST_STREAM frame and nothing else");
    assert_eq!(out[3], 3, "RST_STREAM");
    assert_eq!(u32::from_be_bytes(out[5..9].try_into().unwrap()), late);
    assert_eq!(u32::from_be_bytes(out[9..13].try_into().unwrap()), 7);
}

/// GOAWAY names the last stream that was actually *processed*. A stream that was
/// reported but declined — refused past the limit, or arriving after our own
/// GOAWAY — must not advance it: RFC 9113 §6.8 lets the peer retry everything
/// above `lastStreamID` on a new connection, so naming a declined stream tells
/// the peer a request was handled when it was not, and it is silently lost.
#[test]
fn h2_goaway_names_the_last_processed_stream_not_the_last_seen() {
    // A stream refused by the concurrent-stream limit.
    let (mut client, mut server) = handshake(2);
    let mut live = Vec::new();
    for i in 0..2 {
        live.push(client.open(&post_headers(&format!("/{i}")), false).unwrap());
    }
    let mut wire = ship(&mut client);
    drive(&mut server, &mut wire).unwrap();
    let processed = *live.last().unwrap();
    let refused = processed + 2;
    let mut block = Vec::new();
    hpack::Encoder::new(4096).encode(&post_headers("/over"), &mut block);
    let mut wire = Vec::new();
    http2::encode_frame(1, 4, refused, &block, &mut wire).unwrap();
    assert_eq!(
        drive(&mut server, &mut wire).unwrap(),
        vec![format!("Reset s={refused} code=7")]
    );
    let _ = ship(&mut server);
    server.shutdown().unwrap();
    let out = ship(&mut server);
    assert_eq!(out[3], 7, "GOAWAY");
    let named = u32::from_be_bytes(out[9..13].try_into().unwrap()) & 0x7fffffff;
    assert_eq!(
        named, processed,
        "GOAWAY named {named}; stream {refused} was refused, not processed"
    );

    // And a stream arriving after the GOAWAY does not advance it either. The
    // client's own peer-stream limit is two, so build this one by hand.
    let late = refused + 2;
    let mut wire = Vec::new();
    http2::encode_frame(1, 4, late, &block, &mut wire).unwrap();
    assert_eq!(
        drive(&mut server, &mut wire).unwrap(),
        vec![format!("Unprocessed s={late}")]
    );
    server.shutdown().unwrap();
    let out = ship(&mut server);
    let named = u32::from_be_bytes(out[9..13].try_into().unwrap()) & 0x7fffffff;
    assert_eq!(named, processed, "an unprocessed stream must not be named");
    // The declined ids are still tolerated on the wire, which is what the
    // separate watermark buys: late frames for them must not fail the session.
    server.release_capacity(refused, 0).unwrap();
    assert!(server.reset(late, 7).is_ok());
}

/// `goaway` sets the code, the last stream id and the opaque debug data that
/// `shutdown` cannot express.
#[test]
fn h2_goaway_carries_code_last_stream_and_opaque_data() {
    let (mut client, mut server) = handshake(100);
    server.goaway(11, 0, b"enhance").unwrap();
    let mut wire = ship(&mut server);
    assert_eq!(
        drive(&mut client, &mut wire).unwrap(),
        vec!["Goaway code=11"]
    );
    assert!(client.open(&post_headers("/after"), true).is_err());
    // Opaque data over the peer's SETTINGS_MAX_FRAME_SIZE is refused here
    // rather than sent for the peer to answer with FRAME_SIZE_ERROR.
    assert_eq!(
        server.goaway(0, 0, &vec![0; 16384]).err().map(|e| e.code),
        Some("FRAME_SIZE_ERROR")
    );
}

/// `Step`'s two independent zero cases, both normal, neither previously
/// stated. A host looping while "an event came back" stalls on the second —
/// at the client preface, before a single frame is read.
#[test]
fn h2_step_has_two_independent_zero_cases() {
    let mut server = http2::Connection::new(http2::Role::Server, http2::Limits::default()).unwrap();
    // consumed == 0, event == None: a partial preface. Wait for more input.
    let step = server.receive(&http2::PREFACE[..5]).unwrap();
    assert_eq!((step.consumed, step.event.is_some()), (0, false));

    let mut wire = http2::PREFACE.to_vec();
    http2::encode_frame(4, 0, 0, &[], &mut wire).unwrap(); // peer SETTINGS
    http2::encode_frame(4, 1, 0, &[], &mut wire).unwrap(); // peer SETTINGS ack
    http2::encode_frame(2, 0, 1, &[0, 0, 0, 0, 0], &mut wire).unwrap(); // PRIORITY
    let mut shapes = Vec::new();
    loop {
        let step = server.receive(&wire).unwrap();
        let consumed = step.consumed;
        let progressed = consumed > 0 || step.event.is_some();
        shapes.push((consumed, step.event.is_some()));
        wire.drain(..consumed);
        if !progressed {
            break;
        }
    }
    assert_eq!(
        shapes,
        vec![
            (24, false), // consumed > 0, event == None: the preface. KEEP GOING.
            (9, true),   // SETTINGS
            (9, true),   // SettingsAck: the ack of our initial SETTINGS.
            (14, false), // consumed > 0, event == None: PRIORITY.
            (0, false),  // consumed == 0, event == None: exhausted. STOP.
        ]
    );
    // An HTTP/2 event always consumes; only the stop case has consumed == 0.
    assert!(shapes.iter().all(|(n, event)| !event || *n > 0));
}

/// `Event::Headers` says which of the three blocks it is, so a host does not
/// have to duplicate the `received_head` state the core already keeps.
#[test]
fn h2_headers_events_name_head_informational_and_trailers() {
    let (mut client, mut server) = handshake(100);
    let id = client.open(&post_headers("/x"), false).unwrap();
    let mut wire = ship(&mut client);
    drive(&mut server, &mut wire).unwrap();
    server
        .send_headers(id, &[Header::new(":status", "103")], false)
        .unwrap();
    server
        .send_headers(id, &[Header::new(":status", "200")], false)
        .unwrap();
    server
        .send_headers(id, &[Header::new("x-trailer", "1")], true)
        .unwrap();
    let mut wire = ship(&mut server);
    let mut kinds = Vec::new();
    loop {
        let step = client.receive(&wire).unwrap();
        let consumed = step.consumed;
        let progressed = consumed > 0 || step.event.is_some();
        if let Some(http2::Event::Headers { kind, .. }) = step.event {
            kinds.push(kind);
        }
        wire.drain(..consumed);
        if !progressed {
            break;
        }
    }
    assert_eq!(
        kinds,
        vec![
            http2::HeadersKind::Informational,
            http2::HeadersKind::Head,
            http2::HeadersKind::Trailers,
        ]
    );
}

/// The HTTP/1 analogue of the same contract (PerryTS/turnloop#50): `Event::End`
/// arrives with `consumed == 0`, so "consumed means progress" is wrong there in
/// the opposite direction, and `State::Done` then returns the stop shape
/// forever.
#[test]
fn http1_step_zero_cases_are_the_mirror_image() {
    let mut decoder = Decoder::new(Mode::Response, Default::default());
    let mut shapes = Vec::new();
    let mut wire =
        b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n4\r\nbody\r\n0\r\n\r\n".to_vec();
    for _ in 0..9 {
        let step = decoder.receive(&wire).unwrap();
        let consumed = step.consumed;
        shapes.push((consumed, step.event.is_some()));
        wire.drain(..consumed);
    }
    assert_eq!(
        shapes,
        vec![
            (47, true), // Head
            (3, false), // consumed > 0, event == None: the chunk-size line.
            (4, true),  // Body
            (2, false), // consumed > 0, event == None: the chunk CRLF.
            (3, false), // consumed > 0, event == None: the terminating size.
            (2, false), // consumed > 0, event == None: the empty trailer block.
            (0, true),  // consumed == 0, event == Some(End): #50's shape.
            (0, false), // State::Done: the stop shape, and it never changes.
            (0, false),
        ]
    );
}

/// The step contract is only worth stating if the conditions it rules out
/// actually break. These two loops are the ones hosts wrote, one per decoder.
#[test]
fn step_contract_rejects_both_wrong_loop_conditions() {
    // HTTP/2, "loop while an event came back": stalls at the client preface,
    // before a single frame is read, so the connection never starts.
    let mut server = http2::Connection::new(http2::Role::Server, http2::Limits::default()).unwrap();
    let mut wire = http2::PREFACE.to_vec();
    http2::encode_frame(4, 0, 0, &[], &mut wire).unwrap();
    let full = wire.len();
    let mut events = 0;
    loop {
        let step = server.receive(&wire).unwrap();
        let consumed = step.consumed;
        let Some(_event) = step.event else { break };
        events += 1;
        wire.drain(..consumed);
    }
    assert_eq!(events, 0);
    assert_eq!(wire.len(), full, "not one byte was read");
    // And the stall is not recoverable in place: `receive` has already advanced
    // past the preface, so a host that did not drain `consumed` now feeds it
    // back and the connection fails.
    assert_eq!(
        server.receive(&wire).err().map(|e| e.code),
        Some("FRAME_SIZE_ERROR")
    );

    // The documented condition reaches the peer's SETTINGS.
    let mut server = http2::Connection::new(http2::Role::Server, http2::Limits::default()).unwrap();
    let mut events = 0;
    loop {
        let step = server.receive(&wire).unwrap();
        let consumed = step.consumed;
        let progressed = consumed > 0 || step.event.is_some();
        events += usize::from(step.event.is_some());
        wire.drain(..consumed);
        if !progressed {
            break;
        }
    }
    assert_eq!(events, 1);
    assert!(wire.is_empty());

    // HTTP/1, "loop while input was consumed": drops `Event::End`, which reads
    // no input at all (PerryTS/turnloop#50).
    let message = b"HTTP/1.1 204 No Content\r\n\r\n";
    let mut decoder = Decoder::new(Mode::Response, Default::default());
    let mut wire = message.to_vec();
    let mut ended = false;
    loop {
        let step = decoder.receive(&wire).unwrap();
        if step.consumed == 0 {
            break;
        }
        ended |= matches!(step.event, Some(Event::End));
        wire.drain(..step.consumed);
    }
    assert!(
        !ended,
        "Event::End consumes nothing, so this loop never sees it"
    );
    // The documented condition does see it.
    let mut decoder = Decoder::new(Mode::Response, Default::default());
    let mut wire = message.to_vec();
    let mut ended = false;
    loop {
        let step = decoder.receive(&wire).unwrap();
        let consumed = step.consumed;
        let progressed = consumed > 0 || step.event.is_some();
        ended |= matches!(step.event, Some(Event::End));
        wire.drain(..consumed);
        if !progressed {
            break;
        }
    }
    assert!(ended);
}

/// Drive a request decoder to its stop step with the documented loop, naming
/// each event and its `consumed`. Returns the names and the retained bytes.
fn request_events(decoder: &mut Decoder, wire: &[u8]) -> (Vec<String>, Vec<u8>) {
    let mut wire = wire.to_vec();
    let mut names = Vec::new();
    loop {
        let step = decoder.receive(&wire).unwrap();
        let consumed = step.consumed;
        let progressed = consumed > 0 || step.event.is_some();
        match step.event {
            Some(Event::Head(h)) => names.push(format!("head {} {consumed}", h.method)),
            Some(Event::Body(b)) => names.push(format!("body {} {consumed}", b.len())),
            Some(Event::End) => names.push(format!("end {consumed}")),
            Some(Event::Upgrade) => names.push(format!("upgrade {consumed}")),
            Some(other) => names.push(format!("{other:?}")),
            None => {}
        }
        wire.drain(..consumed);
        if !progressed {
            return (names, wire);
        }
    }
}

/// PerryTS/turnloop#46: a server decoding an upgrade request sees
/// `Event::Upgrade`, as a client decoding the `101` does, and the bytes after it
/// stay with the host. The decision is the host's, so a decline keeps HTTP/1.
#[test]
fn http1_request_side_raises_upgrade() {
    let ws = b"GET /chat HTTP/1.1\r\nHost: a\r\nConnection: keep-alive, Upgrade\r\n\
               Upgrade: websocket\r\n\r\n\x81\x05hello";
    let mut decoder = Decoder::new(Mode::Request, Limits::default());
    let (names, rest) = request_events(&mut decoder, ws);
    assert_eq!(names, ["head GET 84", "upgrade 0"]);
    assert_eq!(
        rest, b"\x81\x05hello",
        "the next protocol's bytes are untouched"
    );
    // Declining is ordinary: the decoder is reusable like after `End`.
    assert!(decoder.reusable());
    decoder.reset().unwrap();
    let (names, _) = request_events(&mut decoder, b"GET / HTTP/1.1\r\nHost: a\r\n\r\n");
    assert_eq!(names, ["head GET 27", "end 0"], "the flag does not leak");

    // A request body is delivered first; the upgrade follows it.
    let mut decoder = Decoder::new(Mode::Request, Limits::default());
    let (names, rest) = request_events(
        &mut decoder,
        b"POST / HTTP/1.1\r\nHost: a\r\nConnection: upgrade\r\nUpgrade: h2c\r\n\
          Content-Length: 3\r\n\r\nabcNEXT",
    );
    assert_eq!(names, ["head POST 82", "body 3 3", "upgrade 0"]);
    assert_eq!(rest, b"NEXT");

    // CONNECT asks to leave HTTP/1 as well.
    let mut decoder = Decoder::new(Mode::Request, Limits::default());
    let (names, rest) = request_events(
        &mut decoder,
        b"CONNECT a:443 HTTP/1.1\r\nHost: a:443\r\n\r\n\x16\x03\x01",
    );
    assert_eq!(names, ["head CONNECT 39", "upgrade 0"]);
    assert_eq!(rest, b"\x16\x03\x01");

    // Neither half alone is an upgrade, and HTTP/1.0 ignores it (RFC 9110 7.8).
    for wire in [
        b"GET / HTTP/1.1\r\nHost: a\r\nUpgrade: websocket\r\n\r\n".as_slice(),
        b"GET / HTTP/1.1\r\nHost: a\r\nConnection: upgrade\r\n\r\n",
        b"GET / HTTP/1.0\r\nConnection: upgrade\r\nUpgrade: websocket\r\n\r\n",
    ] {
        let mut decoder = Decoder::new(Mode::Request, Limits::default());
        let (names, _) = request_events(&mut decoder, wire);
        assert_eq!(names.last().map(String::as_str), Some("end 0"), "{names:?}");
    }
}

fn status_head(status: u16, headers: &[(&str, &str)]) -> Head {
    Head {
        method: String::new(),
        target: String::new(),
        status,
        version: 1,
        headers: headers.iter().map(|(n, v)| Header::new(n, v)).collect(),
        keep_alive: true,
    }
}

/// PerryTS/turnloop#50, as P6 and P11 hit it: a host that feeds the decoder
/// "while there is input" hands over every byte of a keep-alive response and
/// never sees `End`, because `End` comes from a later step that reads nothing.
/// `wants_step` is the signal that step is owed.
#[test]
fn http1_end_needs_a_step_after_the_last_byte() {
    let responses: [&[u8]; 3] = [
        b"HTTP/1.1 200 OK\r\ncontent-length: 5\r\n\r\nhello",
        b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n",
        b"HTTP/1.1 204 No Content\r\n\r\n",
    ];
    for wire in responses {
        // The loop both lanes wrote: stop once every byte is handed over.
        let mut decoder = Decoder::new(Mode::Response, Limits::default());
        decoder.response_to("GET");
        let mut input = wire.to_vec();
        let mut ended = false;
        while !input.is_empty() {
            let step = decoder.receive(&input).unwrap();
            ended |= matches!(step.event, Some(Event::End));
            input.drain(..step.consumed);
        }
        assert!(!ended, "every byte consumed, and still no End");
        assert!(!decoder.reusable(), "so the connection cannot be pooled");
        assert!(decoder.wants_step(), "but the decoder says a step is owed");

        // The same loop, keyed on wants_step as well, completes the message.
        let mut decoder = Decoder::new(Mode::Response, Limits::default());
        decoder.response_to("GET");
        let mut input = wire.to_vec();
        let mut events = Vec::new();
        while !input.is_empty() || decoder.wants_step() {
            let step = decoder.receive(&input).unwrap();
            events.push((step.consumed, matches!(step.event, Some(Event::End))));
            input.drain(..step.consumed);
        }
        assert_eq!(events.last(), Some(&(0, true)), "End reads nothing");
        assert!(decoder.reusable());
        assert!(!decoder.wants_step(), "and nothing more is owed");
    }

    // `eof` ending a close-delimited body also leaves `End` owed.
    let mut decoder = Decoder::new(Mode::Response, Limits::default());
    decoder.response_to("GET");
    let head = b"HTTP/1.1 200 OK\r\n\r\n";
    assert_eq!(decoder.receive(head).unwrap().consumed, head.len());
    assert_eq!(decoder.receive(b"body").unwrap().consumed, 4);
    assert!(
        !decoder.wants_step(),
        "a close-delimited body is open until EOF"
    );
    decoder.eof().unwrap();
    assert!(decoder.wants_step());
    assert!(matches!(
        decoder.receive(&[]).unwrap().event,
        Some(Event::End)
    ));
    assert!(!decoder.wants_step());

    // And an upgrade, on either side, is the same shape.
    let mut decoder = Decoder::new(Mode::Request, Limits::default());
    let wire = b"GET / HTTP/1.1\r\nHost: a\r\nConnection: upgrade\r\nUpgrade: x\r\n\r\n";
    assert_eq!(decoder.receive(wire).unwrap().consumed, wire.len());
    assert!(decoder.wants_step());
    assert!(matches!(
        decoder.receive(&[]).unwrap().event,
        Some(Event::Upgrade)
    ));
    assert!(!decoder.wants_step());
}

/// PerryTS/turnloop#80: when a read returns zero, P11 must decide between a
/// legal end of body and a truncated response before calling `eof`, which
/// commits (and fails the decoder) either way. The queries answer from state.
#[test]
fn http1_mid_message_is_queryable_without_feeding_eof() {
    let position = |d: &Decoder| (d.is_mid_message(), d.eof_is_clean());
    // Content-Length: mid-body, and EOF here would be a truncation.
    let mut decoder = Decoder::new(Mode::Response, Limits::default());
    decoder.response_to("GET");
    assert_eq!(position(&decoder), (false, false), "no response yet");
    let head = b"HTTP/1.1 200 OK\r\ncontent-length: 6\r\n\r\n";
    decoder.receive(head).unwrap();
    assert_eq!(position(&decoder), (true, false));
    decoder.receive(b"par").unwrap();
    assert_eq!(position(&decoder), (true, false));
    // Asking changed nothing: the rest of the body still decodes.
    assert!(matches!(
        decoder.receive(b"tial").unwrap().event,
        Some(Event::Body(b"tia"))
    ));
    assert_eq!(position(&decoder), (false, true), "complete, End owed");
    decoder.receive(&[]).unwrap();
    assert_eq!(position(&decoder), (false, true));
    decoder.eof().unwrap();

    // Close-delimited: mid-body, and EOF is its legal end.
    let mut decoder = Decoder::new(Mode::Response, Limits::default());
    decoder.response_to("GET");
    decoder.receive(b"HTTP/1.0 200 OK\r\n\r\n").unwrap();
    decoder.receive(b"abc").unwrap();
    assert_eq!(position(&decoder), (true, true));
    decoder.eof().unwrap();
    assert_eq!(position(&decoder), (false, true));

    // Chunked framing and trailers are mid-message too.
    let mut decoder = Decoder::new(Mode::Response, Limits::default());
    decoder.response_to("GET");
    let mut wire =
        b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n2\r\nab\r\n0\r\n".to_vec();
    let mut seen = Vec::new();
    loop {
        let step = decoder.receive(&wire).unwrap();
        let (consumed, progressed) = (step.consumed, step.consumed > 0 || step.event.is_some());
        wire.drain(..consumed);
        seen.push(position(&decoder));
        if !progressed {
            break;
        }
    }
    assert!(seen.iter().all(|p| *p == (true, false)), "{seen:?}");

    // An informational response is not the message.
    let mut decoder = Decoder::new(Mode::Response, Limits::default());
    decoder.response_to("GET");
    decoder.receive(b"HTTP/1.1 100 Continue\r\n\r\n").unwrap();
    assert_eq!(position(&decoder), (false, false));

    // The answers agree with what `eof` then does.
    let mut decoder = Decoder::new(Mode::Response, Limits::default());
    decoder.response_to("GET");
    decoder.receive(head).unwrap();
    assert!(!decoder.eof_is_clean());
    assert!(decoder.eof().is_err());
    assert_eq!(position(&decoder), (false, false), "failed");
}

/// PerryTS/turnloop#47, part 1: Node's `res.writeHead(404, "Nope")`.
#[test]
fn http1_encoder_writes_a_custom_reason_phrase() {
    let head = status_head(404, &[]);
    let mut wire = Vec::new();
    Encoder::start_with_reason(&head, "Nope", BodyLength::Empty, &mut wire).unwrap();
    assert_eq!(wire, b"HTTP/1.1 404 Nope\r\n\r\n");
    wire.clear();
    Encoder::start_with_reason(&head, "", BodyLength::Empty, &mut wire).unwrap();
    assert_eq!(wire, b"HTTP/1.1 404 \r\n\r\n", "an empty phrase is legal");
    wire.clear();
    Encoder::start(&head, BodyLength::Empty, &mut wire).unwrap();
    assert_eq!(
        wire, b"HTTP/1.1 404 Not Found\r\n\r\n",
        "start keeps the canonical one"
    );
    // Refused before anything is written: a phrase that would inject a header,
    // and a phrase on a request line, which has no place for one.
    wire.clear();
    for reason in ["a\r\nset-cookie: x", "a\nb", "nul\0"] {
        assert!(
            Encoder::start_with_reason(&head, reason, BodyLength::Empty, &mut wire).is_err(),
            "{reason:?}"
        );
    }
    let request = Request::new("http://localhost/", "GET")
        .unwrap()
        .head(false);
    assert!(Encoder::start_with_reason(&request, "OK", BodyLength::Empty, &mut wire).is_err());
    assert!(wire.is_empty());
}

/// PerryTS/turnloop#47, part 2: a body that ends at EOF, as an HTTP/1.0-style
/// response with no framing does. Decoded back, it is one body ended by `eof`.
#[test]
fn http1_encoder_writes_a_close_delimited_body() {
    let head = status_head(200, &[("content-type", "text/plain")]);
    let mut wire = Vec::new();
    let mut encoder = Encoder::start(&head, BodyLength::CloseDelimited, &mut wire).unwrap();
    encoder.body(b"until ", &mut wire).unwrap();
    encoder.body(b"close", &mut wire).unwrap();
    encoder.finish(&[], &mut wire).unwrap();
    assert_eq!(
        wire, b"HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\n\r\nuntil close",
        "no framing header, no chunk markup, nothing after the body"
    );
    let mut decoder = Decoder::new(Mode::Response, Limits::default());
    decoder.response_to("GET");
    let mut body = Vec::new();
    let mut input = wire.as_slice();
    loop {
        let step = decoder.receive(input).unwrap();
        if let Some(Event::Body(b)) = step.event {
            body.extend_from_slice(b);
        }
        input = &input[step.consumed..];
        if input.is_empty() {
            break;
        }
    }
    decoder.eof().unwrap();
    assert!(matches!(
        decoder.receive(&[]).unwrap().event,
        Some(Event::End)
    ));
    assert_eq!(body, b"until close");

    // Trailers need chunked framing; framing headers contradict EOF framing;
    // a request cannot be close-delimited; 204 and 304 carry no body.
    let mut encoder = Encoder::start(&head, BodyLength::CloseDelimited, &mut wire).unwrap();
    assert!(encoder.finish(&[Header::new("x", "y")], &mut wire).is_err());
    wire.clear();
    for bad in [
        status_head(200, &[("content-length", "5")]),
        status_head(200, &[("transfer-encoding", "chunked")]),
        status_head(204, &[]),
        status_head(304, &[]),
        Request::new("http://localhost/", "POST")
            .unwrap()
            .head(false),
    ] {
        assert!(Encoder::start(&bad, BodyLength::CloseDelimited, &mut wire).is_err());
    }
    assert!(wire.is_empty());
}

/// PerryTS/turnloop#47, part 3: a response that advertises a length and sends
/// no body. `Known(0)` and `Known(n)` both refuse this head; `Omitted` writes
/// it verbatim, and a client decoding a HEAD response reads it as complete.
#[test]
fn http1_encoder_writes_a_body_forbidden_response() {
    let head = status_head(200, &[("content-length", "1234")]);
    let mut wire = Vec::new();
    assert!(Encoder::start(&head, BodyLength::Known(0), &mut wire).is_err());
    assert!(Encoder::start(&head, BodyLength::Empty, &mut wire).is_err());
    let mut encoder = Encoder::start(&head, BodyLength::Omitted, &mut wire).unwrap();
    assert!(encoder.body(b"x", &mut wire).is_err());
    encoder.body(b"", &mut wire).unwrap();
    encoder.finish(&[], &mut wire).unwrap();
    assert_eq!(wire, b"HTTP/1.1 200 OK\r\ncontent-length: 1234\r\n\r\n");
    let mut decoder = Decoder::new(Mode::Response, Limits::default());
    decoder.response_to("HEAD");
    let head_step = decoder.receive(&wire).unwrap();
    assert_eq!(head_step.consumed, wire.len());
    assert!(matches!(
        decoder.receive(&[]).unwrap().event,
        Some(Event::End)
    ));
    assert!(decoder.reusable());

    // A HEAD response may advertise chunked coding, and a 304 its length.
    for head in [
        status_head(200, &[("transfer-encoding", "chunked")]),
        status_head(304, &[("content-length", "1234")]),
        status_head(204, &[]),
    ] {
        wire.clear();
        Encoder::start(&head, BodyLength::Omitted, &mut wire)
            .unwrap()
            .finish(&[], &mut wire)
            .unwrap();
        assert!(wire.ends_with(b"\r\n\r\n"));
        assert!(!wire.windows(2).any(|w| w == b"0\r"), "no chunk markup");
    }
    // A 204 or 1xx may not advertise one (RFC 9110 8.6), and a request is not
    // a response to HEAD.
    wire.clear();
    for bad in [
        status_head(204, &[("content-length", "0")]),
        status_head(103, &[("content-length", "4")]),
        Request::new("http://localhost/", "GET")
            .unwrap()
            .head(false),
    ] {
        assert!(Encoder::start(&bad, BodyLength::Omitted, &mut wire).is_err());
    }
    assert!(wire.is_empty());
}

/// PerryTS/turnloop#54: a step says whether it stopped for input or for output
/// space, so a host reads only when asked to and can size its buffer.
#[test]
fn streaming_decoder_says_whether_it_needs_input_or_output_space() {
    use turnloop_http::compression::StreamingDecoder;
    for coding in CODINGS {
        let wire = encode_with(coding, STREAMED);

        // All input present, a 4-byte output: only output space can stop it,
        // and every call but the last fills the buffer.
        let mut decoder = StreamingDecoder::new(coding, 1000).unwrap();
        let (mut pos, mut result, mut calls) = (0, Vec::new(), 0);
        loop {
            let mut out = [0; 4];
            let step = decoder.process(&wire[pos..], &mut out, true).unwrap();
            calls += 1;
            pos += step.consumed;
            result.extend_from_slice(&out[..step.written]);
            if step.finished {
                assert!(!step.needs_input, "{coding}");
                break;
            }
            assert!(!step.needs_input, "{coding}: all input was present");
            assert_eq!(step.written, 4, "{coding}: stopped only when full");
        }
        assert_eq!(result, STREAMED, "{coding}");
        assert_eq!(pos, wire.len(), "{coding}");
        assert!(calls <= STREAMED.len() / 4 + 1, "{coding}: {calls} calls");

        // One byte at a time into a roomy buffer: only input can stop it, and
        // it asks for more exactly when it has used what it has.
        let mut decoder = StreamingDecoder::new(coding, 1000).unwrap();
        let (mut input, mut result) = (Vec::new(), Vec::new());
        for (i, byte) in wire.iter().enumerate() {
            input.push(*byte);
            let mut out = [0; 1000];
            let step = decoder
                .process(&input, &mut out, i + 1 == wire.len())
                .unwrap();
            input.drain(..step.consumed);
            result.extend_from_slice(&out[..step.written]);
            if i + 1 == wire.len() {
                assert!(step.finished && !step.needs_input, "{coding}");
            } else {
                assert!(step.needs_input && !step.finished, "{coding} at {i}");
            }
        }
        assert_eq!(result, STREAMED, "{coding}");
        assert!(input.is_empty(), "{coding}");
    }
    // An empty output buffer is output-bound, never a request for input.
    let mut decoder = StreamingDecoder::new("gzip", 1000).unwrap();
    let step = decoder.process(b"\x1f", &mut [], false).unwrap();
    assert!(!step.needs_input && !step.finished);
}

/// A host learns that the body is over after it has fed the last byte - an
/// HTTP/1 `End` arrives in a later step - so `end` often comes with no input.
/// Native zstd used to lose its frame boundary on the empty step between.
#[test]
fn streaming_decoder_finishes_when_end_follows_the_last_byte() {
    use turnloop_http::compression::StreamingDecoder;
    for coding in CODINGS {
        let wire = encode_with(coding, STREAMED);
        let mut decoder = StreamingDecoder::new(coding, 1000).unwrap();
        let mut out = [0; 1000];
        let (mut pending, mut result) = (Vec::new(), Vec::new());
        for byte in &wire {
            pending.push(*byte);
            let step = decoder.process(&pending, &mut out, false).unwrap();
            pending.drain(..step.consumed);
            result.extend_from_slice(&out[..step.written]);
        }
        assert!(pending.is_empty(), "{coding}");
        // An idle step with nothing new, then the end. (Brotli marks its own
        // end, so it may already be finished; the others wait for `end`.)
        let step = decoder.process(&[], &mut out, false).unwrap();
        assert_eq!(step.written, 0, "{coding}");
        assert!(
            step.needs_input || coding == "br" && step.finished,
            "{coding}"
        );
        let step = decoder.process(&[], &mut out, true).unwrap();
        assert!(step.finished, "{coding}");
        assert_eq!(result, STREAMED, "{coding}");
    }
}

/// Decode `wire` with `decoder`, `input` bytes per read and `output` bytes of
/// scratch, honouring `needs_input` exactly: read only when it asks.
fn decode_streamed(
    decoder: &mut turnloop_http::compression::StreamingDecoder,
    wire: &[u8],
    input: usize,
    output: usize,
) -> Result<Vec<u8>, turnloop_http::Error> {
    let (mut fed, mut pending, mut result) = (0, Vec::new(), Vec::new());
    let mut out = vec![0; output];
    loop {
        let step = decoder.process(&pending, &mut out, fed == wire.len())?;
        pending.drain(..step.consumed);
        result.extend_from_slice(&out[..step.written]);
        if step.finished {
            return Ok(result);
        }
        if step.needs_input {
            assert!(fed < wire.len(), "asked for input after `end`");
            let next = (fed + input).min(wire.len());
            pending.extend_from_slice(&wire[fed..next]);
            fed = next;
        }
    }
}

/// PerryTS/turnloop#79: a `Content-Encoding` list, and repeated header lines,
/// decode as one chain, innermost (first-listed) last.
#[test]
fn streaming_decoder_applies_a_content_encoding_list_innermost_last() {
    use turnloop_http::compression::{MAX_CODINGS, StreamingDecoder, decode};
    let body: Vec<u8> = (0..20_000u32)
        .flat_map(|i| (i % 97).to_le_bytes())
        .collect();
    // `gzip, br`: gzip was applied first, so br is outermost.
    let wire = encode_with("br", &encode_with("gzip", &body));
    for (input, output) in [(wire.len(), 1 << 20), (1, 7), (1000, 3), (7, 1 << 16)] {
        let mut decoder = StreamingDecoder::new("gzip, br", 1 << 20).unwrap();
        let decoded = decode_streamed(&mut decoder, &wire, input, output).unwrap();
        assert!(decoded == body, "{input}/{output}");
        // Reused for a second body of the same chain.
        decoder.reset(1 << 20).unwrap();
        let decoded = decode_streamed(&mut decoder, &wire, input, output).unwrap();
        assert!(decoded == body, "reset {input}/{output}");
    }
    // Two header lines are the same list, in order.
    let head = Head {
        method: String::new(),
        target: String::new(),
        status: 200,
        version: 1,
        headers: vec![
            Header::new("content-encoding", "gzip"),
            Header::new("x-other", "br"),
            Header::new("Content-Encoding", "br"),
        ],
        keep_alive: true,
    };
    let mut decoder =
        StreamingDecoder::from_codings(head.values("content-encoding"), 1 << 20).unwrap();
    assert!(decode_streamed(&mut decoder, &wire, 4096, 4096).unwrap() == body);
    // The other order is a different coding, and fails.
    let mut decoder = StreamingDecoder::new("br, gzip", 1 << 20).unwrap();
    assert!(decode_streamed(&mut decoder, &wire, 4096, 4096).is_err());

    // The legal `gzip, gzip` that P11 saw fail, with case, `x-gzip`, identity
    // and empty elements - and the convenience `decode` takes a list too.
    let twice = encode_with("gzip", &encode_with("gzip", STREAMED));
    for list in ["gzip, gzip", "GZIP,x-gzip", " gzip ,, identity, Gzip "] {
        let mut out = Vec::new();
        decode(list, &twice, &mut out, 1000).unwrap();
        assert_eq!(out, STREAMED, "{list}");
    }
    // Every coding under every other.
    for inner in CODINGS {
        for outer in ["gzip", "deflate", "br"] {
            let wire = encode_with(outer, &encode_with(inner, STREAMED));
            for (input, output) in [(1, 5), (1, 1000), (1000, 5), (wire.len(), 1000)] {
                let mut decoder = StreamingDecoder::from_codings([inner, outer], 1000).unwrap();
                let decoded = decode_streamed(&mut decoder, &wire, input, output);
                assert!(
                    decoded.as_deref() == Ok(STREAMED),
                    "{inner}, {outer} {input}/{output}: {decoded:?}"
                );
            }
        }
    }

    // Truncation anywhere in the chain is still an error.
    let mut decoder = StreamingDecoder::new("gzip, br", 1 << 20).unwrap();
    assert!(decode_streamed(&mut decoder, &wire[..wire.len() - 1], 64, 64).is_err());
    // The limit bounds the decoded body.
    let mut decoder = StreamingDecoder::new("gzip, br", body.len() - 1).unwrap();
    let error = decode_streamed(&mut decoder, &wire, 4096, 4096).unwrap_err();
    assert_eq!(error.code, "UND_ERR_RES_EXCEEDED_MAX_SIZE");
    // Unknown codings and over-long chains are refused up front.
    for list in ["gzip, compress", "gzip, br\u{e9}"] {
        let error = StreamingDecoder::new(list, 1000).err().map(|e| e.code);
        assert_eq!(error, Some("UND_ERR_NOT_SUPPORTED"), "{list}");
    }
    let chain = ["gzip"; MAX_CODINGS].join(", ");
    assert!(StreamingDecoder::new(&chain, 1000).is_ok());
    let error = StreamingDecoder::new(&format!("{chain}, gzip"), 1000).err();
    assert_eq!(error.map(|e| e.code), Some("UND_ERR_NOT_SUPPORTED"));
    // No coding at all is identity.
    let mut out = Vec::new();
    decode(" , identity", b"plain", &mut out, 100).unwrap();
    assert_eq!(out, b"plain");
}
