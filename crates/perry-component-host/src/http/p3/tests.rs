use super::*;
use crate::{Imports, Options, WasiOptions};
use futures_lite::future::{block_on, poll_once};
use std::{
    future::Future,
    io::{Read, Write},
    net::{Shutdown, TcpStream},
    time::Duration,
};

const INTERFACE: &str = "wasi:http/handler@0.3.0";
const SOURCE: &str = include_str!("../../../../perry-wasi/src/p3/http/handler.component.wat");

fn world() -> perry_wit::World {
    // This selected host contract exposes resource identities and handler result
    // values. Provider-owned body methods stay in the provider linker; generic
    // application future/stream bindings remain explicitly unsupported.
    let pinned = include_str!("../../../../perry-wasi/wit-p3/deps/http.wit");
    let begin = pinned.find("  record DNS-error-payload").unwrap();
    let end = pinned.find("  /// This type enumerates").unwrap();
    perry_wit::parse(&format!("package wasi:http@0.3.0; interface types {{ {} resource request; resource response; }} interface handler {{ use types.{{request, response, error-code}}; handle: async func(request: request) -> result<response, error-code>; }} world selected {{ import types; export handler; }}", &pinned[begin..end]), None).unwrap()
}
fn host(source: &str) -> Host {
    host_mode(source, false)
}
fn host_mode(source: &str, persistent: bool) -> Host {
    // The selected WIT handler reexports its two used resource types. The
    // low-level provider fixture only exports its callable function.
    let source = source.replace("(instance $handler (export", "(instance $handler (export \"request\" (type $request)) (export \"response\" (type $response)) (export");
    let bytes = wat::parse_str(&source).unwrap();
    let options = Options {
        wasi: Some(WasiOptions::default()),
        ..Default::default()
    };
    if persistent {
        Host::new_for_driver(
            &bytes,
            &world(),
            &Imports::new(),
            &crate::AsyncImports::new(),
            &options,
            std::sync::Arc::new(|_| Ok(Vec::new())),
        )
    } else {
        Host::new(&bytes, &world(), &Imports::new(), &options)
    }
    .unwrap()
}
fn server() -> server::Server {
    block_on(server::Server::bind(
        "127.0.0.1:0".parse().unwrap(),
        server::Options {
            response_head_timeout: Duration::from_secs(5),
            between_bytes_timeout: Duration::from_secs(5),
            ..Default::default()
        },
        1,
    ))
    .unwrap()
}
fn peer(
    server: &server::Server,
    payload: Vec<u8>,
    trailers: bool,
) -> std::thread::JoinHandle<Vec<u8>> {
    let address = server.local_addr();
    std::thread::spawn(move || {
        let mut reader = TcpStream::connect(address).unwrap();
        reader
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut writer = reader.try_clone().unwrap();
        let upload = std::thread::spawn(move || {
            writer
                .write_all(
                    b"POST /echo HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n",
                )
                .unwrap();
            for chunk in payload.chunks(16381) {
                write!(writer, "{:x}\r\n", chunk.len()).unwrap();
                writer.write_all(chunk).unwrap();
                writer.write_all(b"\r\n").unwrap();
            }
            writer
                .write_all(if trailers {
                    b"0\r\nx-native: preserved\r\n\r\n"
                } else {
                    b"0\r\n\r\n"
                })
                .unwrap();
            writer.shutdown(Shutdown::Write).unwrap();
        });
        let mut response = Vec::new();
        reader.read_to_end(&mut response).unwrap();
        upload.join().unwrap();
        response
    })
}
fn decode(bytes: &[u8]) -> (Vec<u8>, &[u8]) {
    assert!(bytes.starts_with(b"HTTP/1.1 200 OK\r\n"));
    let head = bytes.windows(4).position(|p| p == b"\r\n\r\n").unwrap() + 4;
    let mut rest = &bytes[head..];
    let mut body = Vec::new();
    loop {
        let end = rest.windows(2).position(|p| p == b"\r\n").unwrap();
        let n = usize::from_str_radix(std::str::from_utf8(&rest[..end]).unwrap(), 16).unwrap();
        rest = &rest[end + 2..];
        if n == 0 {
            return (body, rest);
        }
        body.extend_from_slice(&rest[..n]);
        assert_eq!(&rest[n..n + 2], b"\r\n");
        rest = &rest[n + 2..];
    }
}
fn assert_empty(host: &mut Host) {
    assert!(host
        .live
        .as_mut()
        .unwrap()
        .store
        .data_mut()
        .wasi
        .as_mut()
        .unwrap()
        .view()
        .table
        .is_empty());
}
#[test]
fn p3_http_host_streams_large_native_bodies_and_trailers_then_reuses_store() {
    super::super::tests::guarded(|| {
        let mut host = host(SOURCE);
        for length in [0, 65537, 3 * 1024 * 1024 + 137] {
            let expected: Vec<u8> = (0..length).map(|n| (n * 23 + 59) as u8).collect();
            let server = server();
            let peer = peer(&server, expected.clone(), true);
            let (request, done) = block_on(server.accept().unwrap().receive()).unwrap();
            block_on(host.handle_http_async(INTERFACE, request, done))
                .unwrap()
                .unwrap();
            let bytes = peer.join().unwrap();
            let (body, trailers) = decode(&bytes);
            assert_eq!(body, expected);
            assert_eq!(trailers, b"x-native: preserved\r\n\r\n");
            assert_empty(&mut host);
        }
        host.dispose().unwrap();
    });
}
fn immediate_error() -> String {
    let mut source=SOURCE.replace("  (core module $guest", "  (core func $drop-request (canon resource.drop $request))\n  (core module $guest\n    (import \"host\" \"drop-request\" (func $drop-request (param i32)))");
    source = source.replace(
        "(with \"host\" (instance",
        "(with \"host\" (instance (export \"drop-request\" (func $drop-request))",
    );
    let start = source.find("    (func (export \"handle\")").unwrap();
    let end = source[start..]
        .find("    (func (export \"callback\")")
        .unwrap()
        + start;
    source.replace_range(start..end,"    (func (export \"handle\") (param $request i32) (result i32)\n      (call $drop-request (local.get $request))\n      (call $return (i32.const 1) (i32.const 15) (i32.const 0) (i64.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0)) (i32.const 0))\n");
    source
}
#[test]
fn p3_http_host_preserves_explicit_http_errors_and_retires_guest_traps() {
    super::super::tests::guarded(|| {
        for trap in [false, true] {
            let source = if trap {
                SOURCE.replace("(global.set $set (call $new-set))", "unreachable")
            } else {
                immediate_error()
            };
            let mut host = host(&source);
            for _ in 0..if trap { 1 } else { 2 } {
                let server = server();
                let peer = peer(&server, Vec::new(), false);
                let (request, done) = block_on(server.accept().unwrap().receive()).unwrap();
                let result = block_on(host.handle_http_async(INTERFACE, request, done));
                if trap {
                    assert!(format!("{:#}", result.unwrap_err()).contains("trapped"));
                    assert!(host.live.is_none());
                } else {
                    assert_eq!(result.unwrap().unwrap_err(), Error::HttpRequestDenied);
                    assert_empty(&mut host);
                }
                assert!(peer.join().unwrap().starts_with(b"HTTP/1.1 500"));
            }
        }
    });
}
#[test]
fn p3_http_host_checks_interface_before_publishing_resources() {
    super::super::tests::guarded(|| {
        let mut host = host(SOURCE);
        let server = server();
        let peer = peer(&server, Vec::new(), false);
        let (request, done) = block_on(server.accept().unwrap().receive()).unwrap();
        assert!(
            block_on(host.handle_http_async("wasi:http/handler@0.3.1", request, done))
                .unwrap_err()
                .to_string()
                .contains("unsupported")
        );
        assert!(peer.join().unwrap().starts_with(b"HTTP/1.1 500"));
        assert_empty(&mut host);
    });
}
#[test]
fn cancelled_p3_http_host_retires_suspended_store_and_native_exchange() {
    super::super::tests::guarded(|| {
        let mut host = host(SOURCE);
        let server = server();
        let mut peer = TcpStream::connect(server.local_addr()).unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        peer.write_all(b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 1\r\n\r\n")
            .unwrap();
        let (request, done) = block_on(server.accept().unwrap().receive()).unwrap();
        let mut pending = Box::pin(host.handle_http_async(INTERFACE, request, done));
        assert!(block_on(poll_once(pending.as_mut())).is_none());
        drop(pending);
        assert!(host.live.is_none());
        let mut bytes = Vec::new();
        let _ = peer.read_to_end(&mut bytes);
        // Cancellation releases the one-exchange admission slot before reuse.
        let pending = server.accept().unwrap();
        drop(pending);
    });
}

#[test]
fn p3_http_host_checks_nominal_resource_signature_before_entering_guest() {
    super::super::tests::guarded(|| {
        let bytes = wat::parse_str(
            r#"(component
          (core module $m
            (func (export "handle") (param i32) (result i32) unreachable)
            (func (export "callback") (param i32 i32 i32) (result i32) unreachable))
          (core instance $m (instantiate $m))
          (func $handle async (param "request" u32) (result u32)
            (canon lift (core func $m "handle") async (callback (core func $m "callback"))))
          (instance $handler (export "handle" (func $handle)))
          (export "wasi:http/handler@0.3.0" (instance $handler)))"#,
        )
        .unwrap();
        let world = perry_wit::parse("package wasi:http@0.3.0; interface handler { handle: async func(request: u32) -> u32; } world wrong { export handler; }", None).unwrap();
        let mut host = Host::new(
            &bytes,
            &world,
            &Imports::new(),
            &Options {
                wasi: Some(WasiOptions::default()),
                ..Default::default()
            },
        )
        .unwrap();
        let server = server();
        let peer = peer(&server, Vec::new(), false);
        let (request, done) = block_on(server.accept().unwrap().receive()).unwrap();
        assert!(format!(
            "{:#}",
            block_on(host.handle_http_async(INTERFACE, request, done)).unwrap_err()
        )
        .contains("signature"));
        assert!(peer.join().unwrap().starts_with(b"HTTP/1.1 500"));
        assert_empty(&mut host);
    });
}

#[test]
fn p3_http_host_fuel_exhaustion_retires_store_and_flushes_error_response() {
    super::super::tests::guarded(|| {
        let source = SOURCE.replace(
            "(global.set $set (call $new-set))",
            "(loop $forever (br $forever))",
        );
        let mut host = host(&source);
        host.live.as_mut().unwrap().fuel = 100;
        let server = server();
        let peer = peer(&server, Vec::new(), false);
        let (request, done) = block_on(server.accept().unwrap().receive()).unwrap();
        let error = block_on(host.handle_http_async(INTERFACE, request, done)).unwrap_err();
        assert!(format!("{error:#}").contains("fuel"));
        assert!(host.live.is_none());
        assert!(peer.join().unwrap().starts_with(b"HTTP/1.1 500"));
    });
}

#[test]
fn p3_http_host_peer_disconnect_retires_streaming_store() {
    super::super::tests::guarded(|| {
        let mut host = host(SOURCE);
        let server = server();
        let address = server.local_addr();
        let peer = std::thread::spawn(move || {
            let mut peer = TcpStream::connect(address).unwrap();
            peer.set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            peer.write_all(
                b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 1000000\r\n\r\n",
            )
            .unwrap();
            peer.write_all(&[42; 65536]).unwrap();
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                peer.read_exact(&mut byte).unwrap();
                head.push(byte[0]);
            }
            assert!(head.starts_with(b"HTTP/1.1 200 OK"));
            peer.shutdown(Shutdown::Both).unwrap();
        });
        let (request, done) = block_on(server.accept().unwrap().receive()).unwrap();
        assert!(block_on(host.handle_http_async(INTERFACE, request, done))
            .unwrap()
            .is_err());
        assert!(host.live.is_none());
        peer.join().unwrap();
    });
}

#[test]
fn p3_http_host_reports_a_task_trap_after_returning_its_response() {
    super::super::tests::guarded(|| {
        // YIELD after task.return schedules a later callback. The fixture's
        // callback rejects that event: this trap belongs to post-return work,
        // after the typed handler result has already reached the embedding.
        let source = SOURCE.replace(
            "(i32.const 0))\n    (func (export \"handle\")",
            "(i32.const 1))\n    (func (export \"handle\")",
        );
        assert_ne!(source, SOURCE);
        let mut host = host(&source);
        let server = server();
        let peer = peer(&server, Vec::new(), false);
        let (request, done) = block_on(server.accept().unwrap().receive()).unwrap();
        let error = block_on(host.handle_http_async(INTERFACE, request, done)).unwrap_err();
        assert!(format!("{error:#}").contains("trapped"));
        assert!(host.live.is_none());
        let _ = peer.join().unwrap();
    });
}

#[test]
fn p3_http_persistent_driver_finishes_a_request_while_another_upload_is_suspended() {
    super::super::tests::guarded(|| {
        let host = host_mode(include_str!("driver.component.wat"), true);
        let (mut driver, client) = host.into_driver().unwrap();
        let server = server();
        let mut slow_peer = TcpStream::connect(server.local_addr()).unwrap();
        slow_peer
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        slow_peer
            .write_all(b"POST /slow HTTP/1.1\r\nHost: localhost\r\nContent-Length: 1\r\n\r\n")
            .unwrap();
        let (request, done) = block_on(server.accept().unwrap().receive()).unwrap();
        let mut first = client.handle_http_async(INTERFACE, request, done).unwrap();
        assert!(block_on(poll_once(&mut driver)).is_none());
        assert!(block_on(poll_once(&mut first)).is_none());
        // A second listener grants independent native admission. Both requests
        // still execute concurrently on the same component Store and memory.
        let fast_server = super::tests::server();
        let expected = (0..65537).map(|n| (n * 23 + 59) as u8).collect::<Vec<_>>();
        let fast_peer = peer(&fast_server, expected.clone(), true);
        let (request, done) = block_on(fast_server.accept().unwrap().receive()).unwrap();
        let mut second = client.handle_http_async(INTERFACE, request, done).unwrap();
        let result = block_on(async {
            std::future::poll_fn(|cx| {
                assert!(std::pin::Pin::new(&mut driver).poll(cx).is_pending());
                std::pin::Pin::new(&mut second).poll(cx)
            })
            .await
        });
        result.unwrap().unwrap();
        assert!(driver.is_active(), "slow upload must still be running");
        assert!(block_on(poll_once(&mut first)).is_none());
        assert!(!client.retained_origins().contains(&second.id()));
        let bytes = fast_peer.join().unwrap();
        let (body, trailers) = decode(&bytes);
        assert_eq!(body, expected);
        assert_eq!(trailers, b"x-native: preserved\r\n\r\n");
        slow_peer.write_all(b"x").unwrap();
        slow_peer.shutdown(Shutdown::Write).unwrap();
        let reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            slow_peer.read_to_end(&mut bytes).unwrap();
            bytes
        });
        block_on(std::future::poll_fn(|cx| {
            assert!(std::pin::Pin::new(&mut driver).poll(cx).is_pending());
            std::pin::Pin::new(&mut first).poll(cx)
        }))
        .unwrap()
        .unwrap();
        let bytes = reader.join().unwrap();
        assert!(bytes.starts_with(b"HTTP/1.1 200 OK\r\n"));
        let head = bytes.windows(4).position(|p| p == b"\r\n\r\n").unwrap() + 4;
        assert_eq!(&bytes[head..], b"x");
        assert!(block_on(poll_once(&mut driver)).is_none());
        assert!(!driver.is_active());
        drop(client);
        let mut host = block_on(driver).unwrap();
        assert_empty(&mut host);
        host.dispose().unwrap();
    });
}

fn drive_http(
    driver: &mut crate::Driver,
    call: &mut crate::DriverHttpCall,
) -> Result<std::result::Result<(), Error>> {
    let mut stopped = false;
    block_on(std::future::poll_fn(|cx| {
        if !stopped {
            match std::pin::Pin::new(&mut *driver).poll(cx) {
                std::task::Poll::Pending => {}
                std::task::Poll::Ready(Err(_)) => stopped = true,
                std::task::Poll::Ready(Ok(_)) => panic!("driver retained its client"),
            }
        }
        std::pin::Pin::new(&mut *call).poll(cx)
    }))
}

#[test]
fn p3_http_persistent_driver_preserves_explicit_errors_and_flushes_post_return_traps() {
    super::super::tests::guarded(|| {
        for trap in [false, true] {
            let source = if trap {
                SOURCE.replace(
                    "(i32.const 0))\n    (func (export \"handle\")",
                    "(i32.const 1))\n    (func (export \"handle\")",
                )
            } else {
                immediate_error()
            };
            let host = host_mode(&source, true);
            let (mut driver, client) = host.into_driver().unwrap();
            for _ in 0..if trap { 1 } else { 2 } {
                let server = server();
                let peer = peer(&server, Vec::new(), false);
                let (request, done) = block_on(server.accept().unwrap().receive()).unwrap();
                let mut call = client.handle_http_async(INTERFACE, request, done).unwrap();
                let outcome = drive_http(&mut driver, &mut call);
                if trap {
                    let message = format!("{:#}", outcome.unwrap_err());
                    assert!(message.contains("wasm trap"), "{message}");
                    let bytes = peer.join().unwrap();
                    assert!(
                        bytes.starts_with(b"HTTP/1.1 200"),
                        "the response was published before its task trapped"
                    );
                } else {
                    assert_eq!(outcome.unwrap(), Err(Error::HttpRequestDenied));
                    assert!(peer.join().unwrap().starts_with(b"HTTP/1.1 500"));
                }
            }
            if !trap {
                assert!(block_on(poll_once(&mut driver)).is_none());
                drop(client);
                let mut host = block_on(driver).unwrap();
                assert_empty(&mut host);
                host.dispose().unwrap();
            }
        }
    });
}

#[test]
fn p3_http_persistent_driver_cancellation_retires_store_and_releases_native_admission() {
    super::super::tests::guarded(|| {
        let host = host_mode(include_str!("driver.component.wat"), true);
        let (mut driver, client) = host.into_driver().unwrap();
        let server = server();
        let mut peer = TcpStream::connect(server.local_addr()).unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        peer.write_all(b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 1\r\n\r\n")
            .unwrap();
        let (request, done) = block_on(server.accept().unwrap().receive()).unwrap();
        let mut call = client.handle_http_async(INTERFACE, request, done).unwrap();
        assert!(block_on(poll_once(&mut driver)).is_none());
        assert!(block_on(poll_once(&mut call)).is_none());
        drop(call);
        assert!(block_on(poll_once(&mut driver)).unwrap().is_err());
        assert!(client.call("", "absent", &[]).is_err());
        drop(peer);
        let pending = server.accept().unwrap();
        drop(pending);
    });
}
