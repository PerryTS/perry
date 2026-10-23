use super::*;
use crate::http::{
    body::{IncomingBody, OutgoingBody, StreamContext, CHUNK_SIZE},
    FieldMap,
};
use bytes::Bytes;
use futures_lite::future::block_on;
use std::{
    io::{Read, Write},
    net::{Shutdown, TcpStream},
    thread,
};
use wasmtime_wasi_io::streams::StreamError;

fn guarded(work: impl FnOnce() + Send + 'static) {
    let (send, receive) = std::sync::mpsc::channel();
    let task = thread::spawn(move || {
        work();
        let _ = send.send(());
    });
    receive
        .recv_timeout(Duration::from_secs(20))
        .expect("HTTP server watchdog");
    task.join().unwrap();
}
fn server(options: Options) -> Server {
    block_on(Server::bind("127.0.0.1:0".parse().unwrap(), options, 1)).unwrap()
}
fn connect(server: &Server) -> TcpStream {
    let socket = TcpStream::connect(server.local_addr()).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    socket
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    socket
}
fn head(socket: &mut impl Read) -> String {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        let mut b = [0];
        socket.read_exact(&mut b).unwrap();
        bytes.push(b[0]);
        assert!(bytes.len() < 32768);
    }
    String::from_utf8(bytes).unwrap()
}
fn fields(values: &[(&str, &str)]) -> FieldMap {
    let mut headers = http::HeaderMap::new();
    for (k, v) in values {
        headers.append(
            http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
            v.parse().unwrap(),
        );
    }
    FieldMap::new_immutable(headers)
}
fn response(status: u16) -> HostOutgoingResponse {
    HostOutgoingResponse {
        status: status.try_into().unwrap(),
        headers: FieldMap::default(),
        body: None,
    }
}
fn consume(mut body: IncomingBody) -> (usize, Result<Option<FieldMap>, Error>) {
    block_on(async {
        let mut input = body.take_stream().unwrap();
        let mut count = 0;
        loop {
            input.ready().await;
            match input.read(913) {
                Ok(bytes) => {
                    assert!(bytes.iter().all(|b| *b == 73));
                    count += bytes.len();
                }
                Err(StreamError::Closed | StreamError::LastOperationFailed(_)) => break,
                Err(e) => panic!("unexpected stream trap: {e:?}"),
            }
        }
        drop(input);
        let mut trailers = body.finish().unwrap();
        trailers.ready().await;
        (count, trailers.get().unwrap().unwrap())
    })
}

#[test]
fn server_streams_large_request_and_response_with_trailers_at_capacity_one() {
    guarded(|| {
        let server = server(Options::default());
        let mut socket = connect(&server);
        let peer = thread::spawn(move || {
            socket.write_all(b"POST /upload?q=1 HTTP/1.1\r\nHost: example.test:8080\r\nTransfer-Encoding: chunked\r\n\r\n").unwrap();
            for _ in 0..48 {
                socket.write_all(b"10000\r\n").unwrap();
                socket.write_all(&vec![73; CHUNK_SIZE]).unwrap();
                socket.write_all(b"\r\n").unwrap();
            }
            socket.write_all(b"0\r\nX-Upload: done\r\n\r\n").unwrap();
            let header = head(&mut socket);
            assert!(header.starts_with("HTTP/1.1 201 Created\r\n"));
            assert!(header.contains("transfer-encoding: chunked\r\n"));
            for n in 0..48u8 {
                let mut bytes = vec![0; CHUNK_SIZE + 9];
                socket.read_exact(&mut bytes).unwrap();
                assert_eq!(&bytes[..7], b"10000\r\n");
                assert!(bytes[7..7 + CHUNK_SIZE].iter().all(|b| *b == n));
                assert_eq!(&bytes[7 + CHUNK_SIZE..], b"\r\n");
            }
            assert_eq!(head(&mut socket), "0\r\nx-download: done\r\n\r\n");
            socket.shutdown(Shutdown::Write).unwrap();
            assert_eq!(socket.read(&mut [0]).unwrap(), 0);
        });
        let (mut exchange, done) = block_on(server.accept().unwrap().receive()).unwrap();
        assert_eq!(exchange.request.uri.to_string(), "/upload?q=1");
        assert_eq!(exchange.request.authority, "example.test:8080");
        assert_eq!(exchange.request.method, http::Method::POST);
        assert!(!exchange.request.headers.contains_key("host"));
        let (count, trailers) = consume(exchange.request.body.take().unwrap());
        assert_eq!(count, 48 * CHUNK_SIZE);
        assert_eq!(trailers.unwrap().unwrap().get("x-upload").unwrap(), "done");
        let (mut body, receiver) = OutgoingBody::new(StreamContext::Response, None);
        let mut output = body.take_stream().unwrap();
        let mut resp = response(201);
        resp.body = Some(receiver);
        assert!(exchange.response.send(Ok(resp)).is_ok());
        block_on(async {
            for n in 0..48u8 {
                output.ready().await;
                assert_eq!(output.check_write().unwrap(), CHUNK_SIZE);
                output.write(Bytes::from(vec![n; CHUNK_SIZE])).unwrap();
            }
        });
        drop(output);
        body.finish(Some(fields(&[("x-download", "done")])))
            .unwrap()
            .unwrap();
        block_on(done.finish()).unwrap();
        peer.join().unwrap();
    });
}

#[test]
fn server_early_response_survives_dropped_request_and_unfinished_upload() {
    guarded(|| {
        let server = server(Options {
            linger_timeout: Duration::from_millis(40),
            ..Default::default()
        });
        let mut socket = connect(&server);
        socket
            .write_all(b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 99999999\r\n\r\n")
            .unwrap();
        let (exchange, done) = block_on(server.accept().unwrap().receive()).unwrap();
        drop(exchange.request);
        assert!(exchange.response.send(Ok(response(413))).is_ok());
        assert!(head(&mut socket).starts_with("HTTP/1.1 413"));
        assert_eq!(socket.read(&mut [0]).unwrap(), 0);
        block_on(done.finish()).unwrap();
    });
}

#[test]
fn server_missing_or_error_response_sends_500_and_preserves_failure() {
    guarded(|| {
        for explicit in [false, true] {
            let server = server(Options::default());
            let mut socket = connect(&server);
            socket
                .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .unwrap();
            socket.shutdown(Shutdown::Write).unwrap();
            let (exchange, done) = block_on(server.accept().unwrap().receive()).unwrap();
            if explicit {
                assert!(exchange
                    .response
                    .send(Err(Error::HttpRequestDenied))
                    .is_ok());
            } else {
                drop(exchange.response);
            }
            assert!(head(&mut socket).starts_with("HTTP/1.1 500"));
            assert_eq!(
                block_on(done.finish()).unwrap_err(),
                if explicit {
                    Error::HttpRequestDenied
                } else {
                    Error::HttpResponseIncomplete
                }
            );
        }
    });
}

#[test]
fn server_rejects_ambiguous_authority_and_non_origin_targets() {
    guarded(|| {
        for request in [
            "GET / HTTP/1.1\r\n\r\n",
            "GET / HTTP/1.1\r\nHost: a\r\nHost: b\r\n\r\n",
            "GET / HTTP/1.1\r\nHost: localhost:abc\r\n\r\n",
            "GET http://example.test/ HTTP/1.1\r\nHost: example.test\r\n\r\n",
            "CONNECT example.test:80 HTTP/1.1\r\nHost: example.test\r\n\r\n",
            "POST / HTTP/1.1\r\nHost: a\r\nContent-Length: 4\r\nTransfer-Encoding: chunked\r\n\r\n",
        ] {
            let server = server(Options::default());
            let mut socket = connect(&server);
            socket.write_all(request.as_bytes()).unwrap();
            socket.shutdown(Shutdown::Write).unwrap();
            // Keep the accept alive until the error response has been flushed.
            let pending = server.accept().unwrap();
            assert!(head(&mut socket).starts_with("HTTP/1.1 400"));
            assert!(block_on(pending.receive()).is_err());
        }
    });
}

#[test]
fn server_deadlines_bound_head_wait_and_unfinished_guest_response_body() {
    guarded(|| {
        let options = Options {
            request_head_timeout: Duration::from_millis(40),
            response_head_timeout: Duration::from_millis(40),
            between_bytes_timeout: Duration::from_millis(40),
            linger_timeout: Duration::from_millis(10),
            ..Default::default()
        };
        let server = server(options);
        let mut socket = connect(&server);
        let pending = server.accept().unwrap();
        assert!(head(&mut socket).starts_with("HTTP/1.1 400"));
        assert!(matches!(
            block_on(pending.receive()),
            Err(Error::ConnectionReadTimeout)
        ));
        // Cancellation acknowledgement is needed before reusing limit-one admission.
        drop(server);
        for set in [false, true] {
            let server = self::server(Options {
                response_head_timeout: Duration::from_millis(40),
                between_bytes_timeout: Duration::from_millis(40),
                ..Default::default()
            });
            let mut socket = connect(&server);
            socket
                .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .unwrap();
            socket.shutdown(Shutdown::Write).unwrap();
            let (exchange, done) = block_on(server.accept().unwrap().receive()).unwrap();
            let (body, receiver) = OutgoingBody::new(StreamContext::Response, None);
            let mut sender = Some(exchange.response);
            if set {
                let mut resp = response(200);
                resp.body = Some(receiver);
                assert!(sender.take().unwrap().send(Ok(resp)).is_ok());
            }
            let header = head(&mut socket);
            assert!(header.starts_with(if set { "HTTP/1.1 200" } else { "HTTP/1.1 500" }));
            assert_eq!(
                block_on(done.finish()).unwrap_err(),
                Error::HttpResponseTimeout
            );
            let mut remaining = Vec::new();
            socket.read_to_end(&mut remaining).unwrap();
            assert!(
                remaining.is_empty(),
                "unfinished response must not acquire a final chunk"
            );
            drop(body);
            drop(sender);
        }
    });
}

#[test]
fn server_cancel_releases_capacity_and_shutdown_owns_live_exchanges() {
    guarded(|| {
        let server = server(Options::default());
        let pending = server.accept().unwrap();
        assert!(matches!(
            server.accept(),
            Err(Error::ConnectionLimitReached)
        ));
        block_on(pending.cancel());
        let mut socket = connect(&server);
        socket
            .write_all(b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 10\r\n\r\n")
            .unwrap();
        let (exchange, done) = block_on(server.accept().unwrap().receive()).unwrap();
        drop(server);
        assert_eq!(
            block_on(done.finish()).unwrap_err(),
            Error::ConnectionTerminated
        );
        assert_eq!(socket.read(&mut [0]).unwrap(), 0);
        drop(exchange);
    });
}

#[test]
fn server_head_drains_guest_body_without_sending_it_and_expect_continue_progresses() {
    guarded(|| {
        let server = server(Options::default());
        let mut socket = connect(&server);
        socket.write_all(b"HEAD / HTTP/1.1\r\nHost: [::1]:80\r\nExpect: 100-continue\r\nContent-Length: 1\r\n\r\n").unwrap();
        let pending = server.accept().unwrap();
        assert!(head(&mut socket).starts_with("HTTP/1.1 100"));
        socket.write_all(b"I").unwrap();
        socket.shutdown(Shutdown::Write).unwrap();
        let (mut exchange, done) = block_on(pending.receive()).unwrap();
        assert_eq!(consume(exchange.request.body.take().unwrap()).0, 1);
        let (mut body, receiver) =
            OutgoingBody::new(StreamContext::Response, Some((CHUNK_SIZE * 3) as u64));
        let mut stream = body.take_stream().unwrap();
        let mut resp = response(200);
        resp.headers = fields(&[("content-length", &(CHUNK_SIZE * 3).to_string())]);
        resp.body = Some(receiver);
        assert!(exchange.response.send(Ok(resp)).is_ok());
        block_on(async {
            for _ in 0..3 {
                stream.ready().await;
                stream.check_write().unwrap();
                stream.write(Bytes::from(vec![1; CHUNK_SIZE])).unwrap();
            }
        });
        drop(stream);
        body.finish(None).unwrap().unwrap();
        assert!(head(&mut socket).contains(&format!("content-length: {}\r\n", CHUNK_SIZE * 3)));
        assert_eq!(socket.read(&mut [0]).unwrap(), 0);
        block_on(done.finish()).unwrap();
    });
}

#[test]
fn server_tls_uses_explicit_credentials_and_the_existing_native_driver() {
    guarded(|| {
        use crate::http::transport::{self, DestinationPolicy, RequestSpec};
        use turnloop_tls::rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
        let cert = include_bytes!("../../../../perry-tls-session/tests/test-cert.pem");
        let key = include_bytes!("../../../../perry-tls-session/tests/test-key.pem");
        let ca = include_bytes!("../../../../perry-tls-session/tests/test-ca.pem");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let tls = ServerConfig::new(
            CertificateDer::pem_slice_iter(cert)
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
            PrivateKeyDer::from_pem_slice(key).unwrap(),
            vec![b"http/1.1".to_vec()],
            now,
        )
        .unwrap();
        let server = server(Options {
            tls: Some(tls),
            ..Default::default()
        });
        let address = server.local_addr();
        let client = Driver::new(1).unwrap();
        let (body, receive) = OutgoingBody::new(StreamContext::Request, Some(0));
        body.finish(None).unwrap().unwrap();
        let pending = transport::send(
            &client.submitter(),
            DestinationPolicy {
                tcp: true,
                dns: true,
                addresses: vec![(address.ip(), Some(address.port()))],
            },
            transport::Options {
                extra_ca_pem: ca.to_vec(),
                ..Default::default()
            },
            RequestSpec {
                method: "GET".into(),
                uri: format!("https://localhost:{}/", address.port())
                    .parse()
                    .unwrap(),
                headers: FieldMap::default(),
            },
            receive,
        )
        .unwrap();
        let (exchange, done) = block_on(server.accept().unwrap().receive()).unwrap();
        assert!(matches!(
            exchange.request.scheme,
            crate::http::p2::bindings::http::types::Scheme::Https
        ));
        assert!(exchange.response.send(Ok(response(204))).is_ok());
        let response = block_on(pending.receive()).unwrap();
        assert_eq!(response.status, 204);
        let (count, trailers) = consume(response.body);
        assert_eq!(count, 0);
        trailers.unwrap();
        block_on(done.finish()).unwrap();
    });
}

#[test]
fn server_reports_truncated_and_stalled_request_bodies_to_guest() {
    guarded(|| {
        for truncated in [true, false] {
            let server = server(Options {
                between_bytes_timeout: Duration::from_millis(80),
                ..Default::default()
            });
            let mut socket = connect(&server);
            socket
                .write_all(b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 4\r\n\r\nII")
                .unwrap();
            if truncated {
                socket.shutdown(Shutdown::Write).unwrap();
            }
            let (mut exchange, done) = block_on(server.accept().unwrap().receive()).unwrap();
            let (n, trailers) = consume(exchange.request.body.take().unwrap());
            assert_eq!(n, 2);
            let expected = if truncated {
                Error::HttpProtocolError
            } else {
                Error::ConnectionReadTimeout
            };
            assert_eq!(trailers.unwrap_err(), expected);
            assert!(exchange.response.send(Ok(response(400))).is_ok());
            assert!(head(&mut socket).starts_with("HTTP/1.1 400"));
            if !truncated {
                socket.shutdown(Shutdown::Write).unwrap();
            }
            assert_eq!(block_on(done.finish()).unwrap_err(), expected);
        }
    });
}

#[test]
fn server_invalid_response_and_unfinished_body_never_report_success() {
    guarded(|| {
        for case in ["informational", "missing-body", "unfinished"] {
            let server = server(Options::default());
            let mut socket = connect(&server);
            socket
                .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .unwrap();
            socket.shutdown(Shutdown::Write).unwrap();
            let (exchange, done) = block_on(server.accept().unwrap().receive()).unwrap();
            let mut resp = response(if case == "informational" { 103 } else { 200 });
            let (body, receive) = OutgoingBody::new(StreamContext::Response, None);
            if case == "missing-body" {
                resp.headers = fields(&[("content-length", "4")]);
            }
            if case == "unfinished" {
                resp.body = Some(receive);
            }
            assert!(exchange.response.send(Ok(resp)).is_ok());
            drop(body);
            let mut bytes = Vec::new();
            socket.read_to_end(&mut bytes).unwrap();
            if case == "unfinished" {
                assert!(bytes.starts_with(b"HTTP/1.1 200"));
                assert!(bytes.ends_with(b"\r\n\r\n"));
                assert!(!bytes.ends_with(b"0\r\n\r\n"));
            } else {
                assert!(bytes.starts_with(b"HTTP/1.1 500"));
            }
            assert_eq!(
                block_on(done.finish()).unwrap_err(),
                if case == "missing-body" {
                    Error::HttpResponseBodySize(Some(0))
                } else {
                    Error::HttpProtocolError
                }
            );
        }
    });
}

#[test]
fn server_tls_handshake_deadline_and_configuration_are_enforced() {
    guarded(|| {
        use turnloop_tls::rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
        let make_tls = |alpn| {
            ServerConfig::new(
                CertificateDer::pem_slice_iter(include_bytes!(
                    "../../../../perry-tls-session/tests/test-cert.pem"
                ))
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
                PrivateKeyDer::from_pem_slice(include_bytes!(
                    "../../../../perry-tls-session/tests/test-key.pem"
                ))
                .unwrap(),
                alpn,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            )
            .unwrap()
        };
        assert!(matches!(
            block_on(Server::bind(
                "127.0.0.1:0".parse().unwrap(),
                Options {
                    tls: Some(make_tls(vec![b"h2".to_vec()])),
                    ..Default::default()
                },
                1
            )),
            Err(Error::ConfigurationError)
        ));
        assert!(matches!(
            block_on(Server::bind(
                "127.0.0.1:0".parse().unwrap(),
                Options {
                    request_head_timeout: Duration::MAX,
                    ..Default::default()
                },
                1
            )),
            Err(Error::ConfigurationError)
        ));
        let server = server(Options {
            tls: Some(make_tls(vec![b"http/1.1".to_vec()])),
            handshake_timeout: Duration::from_millis(60),
            ..Default::default()
        });
        let _silent = connect(&server);
        assert!(matches!(
            block_on(server.accept().unwrap().receive()),
            Err(Error::ConnectionTimeout)
        ));
        // A failed handshake releases limit-one admission before returning.
        let mut bad = connect(&server);
        bad.write_all(b"not a TLS handshake\r\n").unwrap();
        assert!(matches!(
            block_on(server.accept().unwrap().receive()),
            Err(Error::TlsProtocolError)
        ));
    });
}
