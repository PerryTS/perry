use super::*;
use crate::http::body::{OutgoingBody, StreamContext, CHUNK_SIZE};
use bytes::Bytes;
use futures_lite::future::block_on;
use perry_component_io::Driver;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use wasmtime_wasi_io::streams::StreamError;

fn guarded(work: impl FnOnce() + Send + 'static) {
    let (send, receive) = std::sync::mpsc::channel();
    let task = thread::spawn(move || {
        work();
        let _ = send.send(());
    });
    receive
        .recv_timeout(Duration::from_secs(15))
        .expect("HTTP transport watchdog");
    task.join().unwrap();
}
fn listen() -> (TcpListener, SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    (listener, address)
}
fn accept(listener: TcpListener) -> TcpStream {
    let (socket, _) = listener.accept().unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    socket
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    socket
}
fn head(socket: &mut impl Read) -> Vec<u8> {
    let mut out = Vec::new();
    while !out.ends_with(b"\r\n\r\n") {
        let mut b = [0];
        socket.read_exact(&mut b).unwrap();
        out.push(b[0]);
        assert!(out.len() < 32768);
    }
    out
}
fn policy(address: SocketAddr) -> DestinationPolicy {
    DestinationPolicy {
        tcp: true,
        dns: true,
        addresses: vec![(address.ip(), Some(address.port()))],
    }
}
fn spec(address: SocketAddr) -> RequestSpec {
    RequestSpec {
        method: "POST".into(),
        uri: format!("http://{address}/path?q=1").parse().unwrap(),
        headers: FieldMap::new_mutable(32768),
    }
}
fn empty() -> BodyReceiver {
    let (body, receive) = OutgoingBody::new(StreamContext::Request, Some(0));
    body.finish(None).unwrap().unwrap();
    receive
}
fn read_body(mut body: IncomingBody) -> (Vec<u8>, Result<Option<FieldMap>, Error>) {
    block_on(async {
        let mut input = body.take_stream().unwrap();
        let mut out = Vec::new();
        loop {
            input.ready().await;
            match input.read(911) {
                Ok(bytes) => out.extend_from_slice(&bytes),
                Err(StreamError::Closed | StreamError::LastOperationFailed(_)) => break,
                Err(e) => panic!("stream trap {e:?}"),
            }
        }
        drop(input);
        let mut trailers = body.finish().unwrap();
        trailers.ready().await;
        (out, trailers.get().unwrap().unwrap())
    })
}

#[test]
fn http_streams_large_upload_before_response_is_polled_and_receives_trailers() {
    guarded(|| {
        let (listener, address) = listen();
        let peer = thread::spawn(move || {
            let mut stream = accept(listener);
            let h = String::from_utf8(head(&mut stream)).unwrap();
            assert!(h.starts_with("POST /path?q=1 HTTP/1.1\r\n"));
            assert!(h.contains("transfer-encoding: chunked\r\n"));
            for i in 0..32u8 {
                let mut chunk = vec![0; 7 + CHUNK_SIZE + 2];
                stream.read_exact(&mut chunk).unwrap();
                assert_eq!(&chunk[..7], b"10000\r\n");
                assert!(chunk[7..7 + CHUNK_SIZE].iter().all(|b| *b == i));
                assert_eq!(&chunk[7 + CHUNK_SIZE..], b"\r\n");
            }
            let mut trailers = vec![0; b"0\r\nx-checksum: sent\r\n\r\n".len()];
            stream.read_exact(&mut trailers).unwrap();
            assert_eq!(trailers, b"0\r\nx-checksum: sent\r\n\r\n");
            stream.write_all(b"HTTP/1.1 103 Early Hints\r\n\r\nHTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n").unwrap();
            for i in 0..16u8 {
                stream.write_all(b"10000\r\n").unwrap();
                stream.write_all(&vec![i; CHUNK_SIZE]).unwrap();
                stream.write_all(b"\r\n").unwrap();
            }
            stream
                .write_all(b"0\r\nx-checksum: received\r\n\r\n")
                .unwrap();
        });
        let driver = Driver::new(4).unwrap();
        let (mut body, receiver) = OutgoingBody::new(StreamContext::Request, None);
        let mut output = body.take_stream().unwrap();
        let pending = send(
            &driver.submitter(),
            policy(address),
            Options::default(),
            spec(address),
            receiver,
        )
        .unwrap();
        // This completes before anyone polls PendingResponse.
        block_on(async {
            for i in 0..32u8 {
                output.ready().await;
                assert_eq!(output.check_write().unwrap(), CHUNK_SIZE);
                output.write(Bytes::from(vec![i; CHUNK_SIZE])).unwrap();
            }
        });
        drop(output);
        let mut trailers = FieldMap::new_mutable(1024);
        trailers
            .append("x-checksum".into(), b"sent".to_vec())
            .unwrap();
        body.finish(Some(trailers)).unwrap().unwrap();
        let response = block_on(pending.receive()).unwrap();
        assert_eq!(response.status, 200);
        assert!(!response.headers.contains_key("transfer-encoding"));
        let (bytes, trailers) = read_body(response.body);
        assert_eq!(bytes.len(), CHUNK_SIZE * 16);
        for (i, chunk) in bytes.chunks(CHUNK_SIZE).enumerate() {
            assert!(chunk.iter().all(|b| *b == i as u8));
        }
        assert_eq!(trailers.unwrap().unwrap()["x-checksum"], "received");
        peer.join().unwrap();
    });
}

#[test]
fn http_early_final_response_cancels_unfinished_upload_without_deadlock() {
    guarded(|| {
        let (listener, address) = listen();
        let peer = thread::spawn(move || {
            let mut stream = accept(listener);
            head(&mut stream);
            stream
                .write_all(b"HTTP/1.1 413 Content Too Large\r\nContent-Length: 2\r\n\r\nno")
                .unwrap();
        });
        let driver = Driver::new(2).unwrap();
        let (mut body, receiver) = OutgoingBody::new(StreamContext::Request, None);
        let mut output = body.take_stream().unwrap();
        let pending = send(
            &driver.submitter(),
            policy(address),
            Options::default(),
            spec(address),
            receiver,
        )
        .unwrap();
        // Upload is never written or finished. The server still gets its head.
        let response = block_on(pending.receive()).unwrap();
        assert_eq!(response.status, 413);
        assert_eq!(read_body(response.body).0, b"no");
        block_on(output.ready());
        assert!(output.check_write().is_err());
        drop(output);
        drop(body);
        peer.join().unwrap();
    });
}

#[test]
fn http_capabilities_deny_default_dns_and_resolved_addresses_and_wrong_ports() {
    guarded(|| {
        let driver = Driver::new(2).unwrap();
        let (listener, address) = listen();
        assert!(matches!(
            send(
                &driver.submitter(),
                DestinationPolicy::default(),
                Options::default(),
                spec(address),
                empty()
            ),
            Err(Error::HttpRequestDenied)
        ));
        let mut request = spec(address);
        request.uri = format!("http://localhost:{}/", address.port())
            .parse()
            .unwrap();
        let mut p = policy(address);
        p.dns = false;
        let pending = send(&driver.submitter(), p, Options::default(), request, empty()).unwrap();
        assert!(matches!(
            block_on(pending.receive()),
            Err(Error::HttpRequestDenied)
        ));
        let mut request = spec(address);
        request.uri = format!("http://localhost:{}/", address.port())
            .parse()
            .unwrap();
        let pending = send(
            &driver.submitter(),
            DestinationPolicy {
                tcp: true,
                dns: true,
                addresses: vec![],
            },
            Options::default(),
            request,
            empty(),
        )
        .unwrap();
        assert!(matches!(
            block_on(pending.receive()),
            Err(Error::DestinationIpProhibited)
        ));
        let mut p = policy(address);
        p.addresses[0].1 = Some(0);
        let pending = send(
            &driver.submitter(),
            p,
            Options::default(),
            spec(address),
            empty(),
        )
        .unwrap();
        assert!(matches!(
            block_on(pending.receive()),
            Err(Error::DestinationIpProhibited)
        ));
        listener.set_nonblocking(true).unwrap();
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    });
}

#[test]
fn http_first_byte_and_between_byte_timeouts_are_observable() {
    guarded(|| {
        for after_head in [false, true] {
            let (listener, address) = listen();
            let (release, wait) = std::sync::mpsc::channel();
            let peer = thread::spawn(move || {
                let mut socket = accept(listener);
                head(&mut socket);
                if after_head {
                    socket
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\na")
                        .unwrap();
                }
                wait.recv_timeout(Duration::from_secs(5)).unwrap();
            });
            let driver = Driver::new(2).unwrap();
            let options = Options {
                first_byte_timeout: Duration::from_millis(60),
                between_bytes_timeout: Duration::from_millis(60),
                ..Options::default()
            };
            let pending = send(
                &driver.submitter(),
                policy(address),
                options,
                spec(address),
                empty(),
            )
            .unwrap();
            if after_head {
                let response = block_on(pending.receive()).unwrap();
                let (bytes, result) = read_body(response.body);
                assert_eq!(bytes, b"a");
                assert_eq!(result.unwrap_err(), Error::ConnectionReadTimeout);
            } else {
                assert!(matches!(
                    block_on(pending.receive()),
                    Err(Error::ConnectionReadTimeout)
                ));
            }
            release.send(()).unwrap();
            peer.join().unwrap();
        }
    });
}

#[test]
fn http_truncated_body_and_close_delimited_eof_have_distinct_results() {
    guarded(|| {
        for truncated in [false, true] {
            let (listener, address) = listen();
            let peer = thread::spawn(move || {
                let mut socket = accept(listener);
                head(&mut socket);
                let response: &[u8] = if truncated {
                    b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nshort"
                } else {
                    b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nshort"
                };
                socket.write_all(response).unwrap();
                // Drain the already completed empty chunked upload to avoid RST.
                let mut ending = [0; 5];
                let _ = socket.read_exact(&mut ending);
            });
            let driver = Driver::new(2).unwrap();
            let pending = send(
                &driver.submitter(),
                policy(address),
                Options::default(),
                spec(address),
                empty(),
            )
            .unwrap();
            let response = block_on(pending.receive()).unwrap();
            let (bytes, result) = read_body(response.body);
            assert_eq!(bytes, b"short");
            if truncated {
                assert_eq!(result.unwrap_err(), Error::HttpResponseIncomplete);
            } else {
                assert!(result.is_ok());
            }
            peer.join().unwrap();
        }
    });
}

#[test]
fn http_response_body_and_pending_drop_release_driver_capacity() {
    guarded(|| {
        for response_head in [false, true] {
            let (listener, address) = listen();
            let (arrived, wait) = std::sync::mpsc::channel();
            let (release, released) = std::sync::mpsc::channel();
            let peer = thread::spawn(move || {
                let mut socket = accept(listener);
                head(&mut socket);
                if response_head {
                    socket
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 9999999\r\n\r\nbody")
                        .unwrap();
                }
                arrived.send(()).unwrap();
                released.recv_timeout(Duration::from_secs(5)).unwrap();
            });
            let driver = Driver::new(1).unwrap();
            let pending = send(
                &driver.submitter(),
                policy(address),
                Options::default(),
                spec(address),
                empty(),
            )
            .unwrap();
            wait.recv_timeout(Duration::from_secs(5)).unwrap();
            if response_head {
                drop(block_on(pending.receive()).unwrap());
            } else {
                drop(pending);
            }
            let until = std::time::Instant::now() + Duration::from_secs(3);
            while driver.outstanding() != 0 {
                assert!(std::time::Instant::now() < until);
                thread::yield_now();
            }
            block_on(
                driver
                    .submit(|io| async move { io.sleep(Duration::from_millis(1)).await })
                    .unwrap(),
            )
            .unwrap()
            .unwrap();
            release.send(()).unwrap();
            peer.join().unwrap();
        }
    });
}

#[test]
fn https_checks_certificate_chain_and_authority_using_the_existing_driver() {
    guarded(|| {
        use turnloop_tls::rustls::{
            self,
            pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer},
        };
        let cert = include_bytes!("../../../../perry-tls-session/tests/test-cert.pem");
        let key = include_bytes!("../../../../perry-tls-session/tests/test-key.pem");
        let ca = include_bytes!("../../../../perry-tls-session/tests/test-ca.pem");
        for scenario in ["trusted", "untrusted", "wrong-name"] {
            let (listener, address) = listen();
            let peer = thread::spawn(move || {
                let config = rustls::ServerConfig::builder_with_provider(std::sync::Arc::new(
                    rustls::crypto::ring::default_provider(),
                ))
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(
                    CertificateDer::pem_slice_iter(cert)
                        .collect::<Result<Vec<_>, _>>()
                        .unwrap(),
                    PrivateKeyDer::from_pem_slice(key).unwrap(),
                )
                .unwrap();
                let conn = rustls::ServerConnection::new(std::sync::Arc::new(config)).unwrap();
                let mut stream = rustls::StreamOwned::new(conn, accept(listener));
                if scenario == "trusted" {
                    head(&mut stream);
                    stream
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nsecure")
                        .unwrap();
                    stream.flush().unwrap();
                    // The peer must consume any in-flight upload before closing.
                    let mut end = [0; 5];
                    let _ = stream.read_exact(&mut end);
                } else {
                    let mut byte = [0];
                    assert!(stream.read(&mut byte).is_err());
                }
            });
            let driver = Driver::new(2).unwrap();
            let mut request = spec(address);
            let hostname = if scenario == "wrong-name" {
                "127.0.0.1"
            } else {
                "localhost"
            };
            request.uri = format!("https://{hostname}:{}/", address.port())
                .parse()
                .unwrap();
            let options = Options {
                extra_ca_pem: if scenario == "untrusted" {
                    vec![]
                } else {
                    ca.to_vec()
                },
                ..Options::default()
            };
            let pending = send(
                &driver.submitter(),
                policy(address),
                options,
                request,
                empty(),
            )
            .unwrap();
            match block_on(pending.receive()) {
                Ok(response) => {
                    assert_eq!(scenario, "trusted");
                    assert_eq!(read_body(response.body).0, b"secure");
                }
                Err(e) => {
                    assert_ne!(scenario, "trusted");
                    assert_eq!(e, Error::TlsCertificateError);
                }
            }
            peer.join().unwrap();
        }
    });
}

#[test]
fn http_finishing_body_keeps_transport_alive_to_drain_and_deliver_trailers() {
    guarded(|| {
        let (listener, address) = listen();
        let peer = thread::spawn(move || {
            let mut stream = accept(listener);
            head(&mut stream);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")
                .unwrap();
            for _ in 0..32 {
                stream.write_all(b"10000\r\n").unwrap();
                stream.write_all(&vec![5; CHUNK_SIZE]).unwrap();
                stream.write_all(b"\r\n").unwrap();
            }
            stream.write_all(b"0\r\nx-drained: yes\r\n\r\n").unwrap();
            let mut ending = [0; 5];
            let _ = stream.read_exact(&mut ending);
        });
        let driver = Driver::new(2).unwrap();
        let response = block_on(
            send(
                &driver.submitter(),
                policy(address),
                Options::default(),
                spec(address),
                empty(),
            )
            .unwrap()
            .receive(),
        )
        .unwrap();
        let mut trailers = response.body.finish().unwrap();
        block_on(trailers.ready());
        assert_eq!(
            trailers.get().unwrap().unwrap().unwrap().unwrap()["x-drained"],
            "yes"
        );
        peer.join().unwrap();
    });
}

#[test]
fn http_driver_shutdown_wakes_response_stream_and_trailer_future() {
    guarded(|| {
        let (listener, address) = listen();
        let (release, wait) = std::sync::mpsc::channel();
        let peer = thread::spawn(move || {
            let mut socket = accept(listener);
            head(&mut socket);
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 200000\r\n\r\n")
                .unwrap();
            wait.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        let driver = Driver::new(2).unwrap();
        let response = block_on(
            send(
                &driver.submitter(),
                policy(address),
                Options::default(),
                spec(address),
                empty(),
            )
            .unwrap()
            .receive(),
        )
        .unwrap();
        drop(driver);
        let (bytes, result) = read_body(response.body);
        assert!(bytes.is_empty());
        assert_eq!(result.unwrap_err(), Error::HttpResponseIncomplete);
        release.send(()).unwrap();
        peer.join().unwrap();
    });
}

#[test]
fn http_head_response_completes_without_waiting_for_an_advertised_body() {
    guarded(|| {
        let (listener, address) = listen();
        let (release, wait) = std::sync::mpsc::channel();
        let peer = thread::spawn(move || {
            let mut socket = accept(listener);
            head(&mut socket);
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10000\r\n\r\n")
                .unwrap();
            wait.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        let driver = Driver::new(2).unwrap();
        let mut request = spec(address);
        request.method = "HEAD".into();
        let response = block_on(
            send(
                &driver.submitter(),
                policy(address),
                Options::default(),
                request,
                empty(),
            )
            .unwrap()
            .receive(),
        )
        .unwrap();
        assert_eq!(response.headers["content-length"], "10000");
        let (bytes, result) = read_body(response.body);
        assert!(bytes.is_empty());
        assert!(result.is_ok());
        release.send(()).unwrap();
        peer.join().unwrap();
    });
}

#[test]
fn http_invalid_authority_ports_methods_and_schemes_are_rejected_before_submission() {
    let driver = Driver::new(1).unwrap();
    let address = "127.0.0.1:80".parse().unwrap();
    for uri in [
        "http://example.com:65536/",
        "http://example.com:abc/",
        "http://example.com:+80/",
        "http://example.com:/",
        "ftp://example.com/",
        "http://user@example.com/",
    ] {
        if let Ok(uri) = uri.parse::<http::Uri>() {
            let mut request = spec(address);
            request.uri = uri.clone();
            assert!(
                matches!(
                    send(
                        &driver.submitter(),
                        policy(address),
                        Options::default(),
                        request,
                        empty()
                    ),
                    Err(Error::HttpRequestUriInvalid)
                ),
                "{uri}"
            );
        }
    }
    for method in ["CONNECT", "bad method"] {
        let mut request = spec(address);
        request.method = method.into();
        assert!(matches!(
            send(
                &driver.submitter(),
                policy(address),
                Options::default(),
                request,
                empty()
            ),
            Err(Error::HttpRequestMethodInvalid)
        ));
    }
    assert_eq!(driver.outstanding(), 0);
    let options = Options {
        connect_timeout: Duration::MAX,
        ..Options::default()
    };
    let pending = send(
        &driver.submitter(),
        policy(address),
        options,
        spec(address),
        empty(),
    )
    .unwrap();
    assert!(matches!(
        block_on(pending.receive()),
        Err(Error::ConfigurationError)
    ));
}

#[test]
fn http_between_bytes_timeout_restarts_for_fragmented_chunk_framing() {
    guarded(|| {
        let (listener, address) = listen();
        let peer = thread::spawn(move || {
            let mut socket = accept(listener);
            head(&mut socket);
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")
                .unwrap();
            // The chunk-size line takes longer than one timeout in total, but
            // every read progresses within the requested between-byte interval.
            for b in b"0000000000000001\r\nx\r\n0\r\n\r\n" {
                socket.write_all(&[*b]).unwrap();
                thread::sleep(Duration::from_millis(15));
            }
            let mut ending = [0; 5];
            let _ = socket.read_exact(&mut ending);
        });
        let driver = Driver::new(2).unwrap();
        let options = Options {
            between_bytes_timeout: Duration::from_millis(150),
            ..Options::default()
        };
        let response = block_on(
            send(
                &driver.submitter(),
                policy(address),
                options,
                spec(address),
                empty(),
            )
            .unwrap()
            .receive(),
        )
        .unwrap();
        let (bytes, result) = read_body(response.body);
        assert_eq!(bytes, b"x");
        assert!(result.is_ok());
        peer.join().unwrap();
    });
}
