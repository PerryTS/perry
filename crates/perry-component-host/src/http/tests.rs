use super::*;
use crate::{Imports, Options, WasiOptions};
use futures_lite::future::block_on;
use perry_wasi::http::server::{Options as ServerOptions, Server};
use std::{
    io::{Read, Write},
    net::{Shutdown, TcpStream},
    time::Duration,
};

const WORLD: &str = r#"
package wasi:http@0.2.0;
interface types { resource incoming-request; resource response-outparam; }
interface incoming-handler {
  use types.{incoming-request, response-outparam};
  handle: func(request: incoming-request, response-out: response-outparam);
}
world proxy { import types; export incoming-handler; export configure: func(mode: u32); }
"#;
const INTERFACE: &str = "wasi:http/incoming-handler@0.2.0";

pub(super) fn guarded(work: impl FnOnce() + Send + 'static) {
    let (send, receive) = std::sync::mpsc::channel();
    let task = std::thread::spawn(move || {
        work();
        let _ = send.send(());
    });
    receive
        .recv_timeout(Duration::from_secs(30))
        .expect("canonical incoming-handler watchdog");
    task.join().unwrap();
}
fn host() -> Host {
    let bytes = wat::parse_str(include_str!(
        "../../../perry-wasi/src/http/p2/incoming.component.wat"
    ))
    .unwrap();
    let world = perry_wit::parse(WORLD, None).unwrap();
    assert!(Host::new(&bytes, &world, &Imports::new(), &Options::default()).is_err());
    // Receiving an explicitly supplied request requires no TCP/DNS/IP grants.
    Host::new(
        &bytes,
        &world,
        &Imports::new(),
        &Options {
            wasi: Some(WasiOptions::default()),
            ..Default::default()
        },
    )
    .unwrap()
}
fn empty_request() -> HostIncomingRequest {
    HostIncomingRequest {
        method: "GET".parse().unwrap(),
        uri: "/".parse().unwrap(),
        scheme: perry_wasi::http::p2::bindings::http::types::Scheme::Http,
        authority: "localhost".into(),
        headers: Default::default(),
        body: None,
    }
}
fn invoke(host: &mut Host, mode: u32) -> (Result<()>, Result<(), Error>, Vec<u8>) {
    host.call("", "configure", &[mode.into()]).unwrap();
    let server = block_on(Server::bind(
        "127.0.0.1:0".parse().unwrap(),
        ServerOptions::default(),
        1,
    ))
    .unwrap();
    let address = server.local_addr();
    let peer = std::thread::spawn(move || {
        let mut socket = TcpStream::connect(address).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        socket
            .write_all(b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 4\r\n\r\npong")
            .unwrap();
        socket.shutdown(Shutdown::Write).unwrap();
        let mut bytes = Vec::new();
        socket.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let (request, done) = block_on(server.accept().unwrap().receive()).unwrap();
    let result = host.handle_http(INTERFACE, request.request, request.response);
    let transport = block_on(done.finish());
    (result, transport, peer.join().unwrap())
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
fn canonical_incoming_http_reuses_instance_and_streams_with_no_outbound_grants() {
    guarded(|| {
        let mut host = host();
        for mode in [0, 3, 0] {
            let (result, transport, bytes) = invoke(&mut host, mode);
            result.unwrap();
            if mode == 0 {
                transport.unwrap();
                assert!(bytes.starts_with(b"HTTP/1.1 202 Accepted\r\n"));
                assert!(bytes.ends_with(b"4\r\npong\r\n0\r\n\r\n"));
            } else {
                assert_eq!(transport.unwrap_err(), Error::HttpRequestDenied);
                assert!(bytes.starts_with(b"HTTP/1.1 500"));
            }
            assert_empty(&mut host);
        }
        host.dispose().unwrap();
    });
}
#[test]
fn canonical_incoming_http_retires_trapped_and_unset_response_instances() {
    guarded(|| {
        for mode in [1, 2, 4] {
            let mut host = host();
            let (result, transport, bytes) = invoke(&mut host, mode);
            let error = format!("{:#}", result.unwrap_err());
            assert!(
                error.contains(if mode == 1 {
                    "without setting response-outparam"
                } else {
                    "trapped"
                }),
                "{error}"
            );
            assert_eq!(transport.unwrap_err(), Error::HttpResponseIncomplete);
            assert!(bytes.starts_with(b"HTTP/1.1 500"));
            assert!(host.live.is_none());
            let (send, _) = oneshot::channel();
            assert!(host
                .handle_http(INTERFACE, empty_request(), send)
                .unwrap_err()
                .to_string()
                .contains("disposed"));
        }
    });
}
#[test]
fn incoming_http_preflight_checks_signature_before_publishing_resources() {
    let bytes = wat::parse_str(
        r#"(component
      (core module $m (func (export "handle") (param i32)))
      (core instance $m (instantiate $m))
      (func (export "handle") (param "value" u32) (canon lift (core func $m "handle"))))"#,
    )
    .unwrap();
    let world = perry_wit::parse(
        "package test:wrong; world guest { export handle: func(value: u32); }",
        None,
    )
    .unwrap();
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
    let (send, receive) = oneshot::channel();
    assert!(host
        .handle_http("", empty_request(), send)
        .unwrap_err()
        .to_string()
        .contains("signature"));
    assert!(block_on(receive).is_err());
    assert_empty(&mut host);
    let (send, _) = oneshot::channel();
    assert!(host
        .handle_http(INTERFACE, empty_request(), send)
        .unwrap_err()
        .to_string()
        .contains("missing selected"));
    assert_empty(&mut host);
    host.call("", "handle", &[1.into()]).unwrap();
}
