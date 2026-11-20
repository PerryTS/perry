use super::{
    bindings::http::{outgoing_handler, types::*},
    WasiHttpCtxView,
};
use crate::{
    http::{
        body::{IncomingBody, StreamContext},
        Error, FieldMap,
    },
    io::sync::wasi::io::{
        poll::HostPollable,
        streams::{HostInputStream, HostOutputStream},
    },
    WasiCtx,
};
use bytes::Bytes;
use futures_lite::future::block_on;
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener},
    time::Duration,
};
use wasmtime::component::{Resource, ResourceTable};
fn borrow<T: 'static>(r: &Resource<T>) -> Resource<T> {
    Resource::new_borrow(r.rep())
}
fn own<T: 'static>(r: &Resource<T>) -> Resource<T> {
    Resource::new_own(r.rep())
}
fn view<'a>(ctx: &'a mut WasiCtx, table: &'a mut ResourceTable) -> WasiHttpCtxView<'a> {
    WasiHttpCtxView {
        ctx: &mut ctx.http,
        table,
    }
}
fn bounded(test: impl FnOnce() + Send + 'static) {
    let (send, receive) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        test();
        send.send(()).unwrap();
    });
    receive
        .recv_timeout(Duration::from_secs(15))
        .expect("HTTP WIT watchdog");
    thread.join().unwrap();
}
fn request(
    v: &mut WasiHttpCtxView<'_>,
    address: SocketAddr,
) -> Resource<super::HostOutgoingRequest> {
    let fields = HostFields::new(v).unwrap();
    let req = HostOutgoingRequest::new(v, fields).unwrap();
    HostOutgoingRequest::set_scheme(v, borrow(&req), Some(Scheme::Http))
        .unwrap()
        .unwrap();
    HostOutgoingRequest::set_authority(v, borrow(&req), Some(address.to_string()))
        .unwrap()
        .unwrap();
    HostOutgoingRequest::set_path_with_query(v, borrow(&req), Some("/probe".into()))
        .unwrap()
        .unwrap();
    req
}
fn block_future(v: &mut WasiHttpCtxView<'_>, future: &Resource<super::HostFutureIncomingResponse>) {
    let poll = HostFutureIncomingResponse::subscribe(v, borrow(future)).unwrap();
    HostPollable::block(v.table, borrow(&poll)).unwrap();
    HostPollable::drop(v.table, poll).unwrap();
}

#[test]
fn http_wit_header_children_and_body_stream_ownership_are_enforced() {
    let mut ctx = WasiCtx::new(2).unwrap();
    let mut table = ResourceTable::new();
    let mut v = view(&mut ctx, &mut table);
    let fields = HostFields::new(&mut v).unwrap();
    HostFields::append(&mut v, borrow(&fields), "X-Test".into(), b"one".to_vec()).unwrap();
    let saved = HostFields::clone(&mut v, borrow(&fields)).unwrap();
    let req = HostOutgoingRequest::new(&mut v, fields).unwrap();
    let headers = HostOutgoingRequest::headers(&mut v, borrow(&req)).unwrap();
    assert!(matches!(
        HostFields::append(&mut v, borrow(&headers), "x-test".into(), b"two".to_vec())
            .unwrap_err()
            .downcast()
            .unwrap(),
        HeaderError::Immutable
    ));
    HostFields::set(
        &mut v,
        borrow(&saved),
        "x-test".into(),
        vec![b"changed".to_vec()],
    )
    .unwrap();
    assert_eq!(
        HostFields::get(&mut v, borrow(&headers), "X-TEST".into()).unwrap(),
        vec![b"one".to_vec()]
    );
    assert!(HostOutgoingRequest::drop(&mut v, own(&req)).is_err());
    assert!(outgoing_handler::Host::handle(&mut v, own(&req), None).is_err());
    HostFields::drop(&mut v, headers).unwrap();
    HostFields::drop(&mut v, saved).unwrap();
    let body = HostOutgoingRequest::body(&mut v, borrow(&req))
        .unwrap()
        .unwrap();
    assert!(HostOutgoingRequest::body(&mut v, borrow(&req))
        .unwrap()
        .is_err());
    let stream = HostOutgoingBody::write(&mut v, borrow(&body))
        .unwrap()
        .unwrap();
    assert!(HostOutgoingBody::write(&mut v, borrow(&body))
        .unwrap()
        .is_err());
    assert!(HostOutgoingBody::finish(&mut v, own(&body), None).is_err());
    assert!(HostOutgoingBody::drop(&mut v, own(&body)).is_err());
    // Body is independent of the request; its child stream is not independent of it.
    HostOutgoingRequest::drop(&mut v, req).unwrap();
    HostOutputStream::drop(v.table, stream).unwrap();
    HostOutgoingBody::drop(&mut v, body).unwrap();
    assert!(v.table.is_empty());
}

#[test]
fn http_wit_request_options_are_consumed_even_when_dispatch_is_denied() {
    let mut ctx = WasiCtx::new(2).unwrap();
    let mut table = ResourceTable::new();
    let mut v = view(&mut ctx, &mut table);
    let options = HostRequestOptions::new(&mut v).unwrap();
    assert_eq!(
        HostRequestOptions::connect_timeout(&mut v, borrow(&options)).unwrap(),
        None
    );
    HostRequestOptions::set_connect_timeout(&mut v, borrow(&options), Some(u64::MAX))
        .unwrap()
        .unwrap();
    HostRequestOptions::set_first_byte_timeout(&mut v, borrow(&options), Some(0))
        .unwrap()
        .unwrap();
    HostRequestOptions::set_between_bytes_timeout(&mut v, borrow(&options), Some(1000))
        .unwrap()
        .unwrap();
    assert_eq!(
        HostRequestOptions::connect_timeout(&mut v, borrow(&options)).unwrap(),
        Some(u64::MAX)
    );
    assert_eq!(
        HostRequestOptions::first_byte_timeout(&mut v, borrow(&options)).unwrap(),
        Some(0)
    );
    assert_eq!(
        HostRequestOptions::between_bytes_timeout(&mut v, borrow(&options)).unwrap(),
        Some(1000)
    );
    let req = request(&mut v, "127.0.0.1:80".parse().unwrap());
    let result = outgoing_handler::Host::handle(&mut v, req, Some(options));
    assert!(matches!(
        result.unwrap_err().downcast().unwrap(),
        ErrorCode::HttpRequestDenied
    ));
    assert!(v.table.is_empty(), "consumed options leaked into the table");
}

#[test]
fn http_wit_request_validation_preserves_previous_values_and_typed_body_errors() {
    let mut ctx = WasiCtx::new(2).unwrap();
    let mut table = ResourceTable::new();
    let mut v = view(&mut ctx, &mut table);
    let fields =
        HostFields::from_list(&mut v, vec![("content-length".into(), b"3".to_vec())]).unwrap();
    let req = HostOutgoingRequest::new(&mut v, fields).unwrap();
    assert!(HostOutgoingRequest::set_method(
        &mut v,
        borrow(&req),
        Method::Other("bad method".into())
    )
    .unwrap()
    .is_err());
    assert!(matches!(
        HostOutgoingRequest::method(&mut v, borrow(&req)).unwrap(),
        Method::Get
    ));
    assert!(HostOutgoingRequest::set_scheme(
        &mut v,
        borrow(&req),
        Some(Scheme::Other("bad scheme".into()))
    )
    .unwrap()
    .is_err());
    assert!(HostOutgoingRequest::scheme(&mut v, borrow(&req))
        .unwrap()
        .is_none());
    let body = HostOutgoingRequest::body(&mut v, borrow(&req))
        .unwrap()
        .unwrap();
    let stream = HostOutgoingBody::write(&mut v, borrow(&body))
        .unwrap()
        .unwrap();
    assert!(HostOutputStream::check_write(v.table, borrow(&stream)).unwrap() > 0);
    HostOutputStream::write(v.table, borrow(&stream), b"a".to_vec()).unwrap();
    HostOutputStream::drop(v.table, stream).unwrap();
    assert!(matches!(
        HostOutgoingBody::finish(&mut v, body, None)
            .unwrap_err()
            .downcast()
            .unwrap(),
        ErrorCode::HttpRequestBodySize(Some(1))
    ));
    HostOutgoingRequest::drop(&mut v, req).unwrap();
    assert!(v.table.is_empty());
}

#[test]
fn http_wit_outgoing_request_streams_and_returns_response_headers_body_and_trailers() {
    bounded(|| {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                let mut b = [0];
                stream.read_exact(&mut b).unwrap();
                head.push(b[0]);
            }
            assert!(head.starts_with(b"POST /probe HTTP/1.1\r\n"));
            let mut body = [0; 27];
            stream.read_exact(&mut body).unwrap();
            assert_eq!(&body, b"4\r\nping\r\n0\r\nx-sent: yes\r\n\r\n");
            stream.write_all(b"HTTP/1.1 201 Created\r\nTransfer-Encoding: chunked\r\nx-reply: yes\r\n\r\n4\r\npong\r\n0\r\nx-done: yes\r\n\r\n").unwrap();
        });
        let mut ctx = WasiCtx::new(2).unwrap();
        ctx.http.policy.tcp = true;
        ctx.http
            .policy
            .addresses
            .push((address.ip(), Some(address.port())));
        let mut table = ResourceTable::new();
        let mut v = view(&mut ctx, &mut table);
        let req = request(&mut v, address);
        HostOutgoingRequest::set_method(&mut v, borrow(&req), Method::Post)
            .unwrap()
            .unwrap();
        let body = HostOutgoingRequest::body(&mut v, borrow(&req))
            .unwrap()
            .unwrap();
        let stream = HostOutgoingBody::write(&mut v, borrow(&body))
            .unwrap()
            .unwrap();
        let future = outgoing_handler::Host::handle(&mut v, req, None).unwrap();
        assert!(HostFutureIncomingResponse::get(&mut v, borrow(&future))
            .unwrap()
            .is_none());
        HostOutputStream::blocking_write_and_flush(v.table, borrow(&stream), b"ping".to_vec())
            .unwrap();
        HostOutputStream::drop(v.table, stream).unwrap();
        let trailers =
            HostFields::from_list(&mut v, vec![("x-sent".into(), b"yes".to_vec())]).unwrap();
        HostOutgoingBody::finish(&mut v, body, Some(trailers)).unwrap();
        block_future(&mut v, &future);
        let response = HostFutureIncomingResponse::get(&mut v, borrow(&future))
            .unwrap()
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(
            HostFutureIncomingResponse::get(&mut v, borrow(&future)).unwrap(),
            Some(Err(()))
        ));
        HostFutureIncomingResponse::drop(&mut v, future).unwrap();
        assert_eq!(
            HostIncomingResponse::status(&mut v, borrow(&response)).unwrap(),
            201
        );
        let headers = HostIncomingResponse::headers(&mut v, borrow(&response)).unwrap();
        assert_eq!(
            HostFields::get(&mut v, borrow(&headers), "x-reply".into()).unwrap(),
            vec![b"yes".to_vec()]
        );
        assert!(HostIncomingResponse::drop(&mut v, own(&response)).is_err());
        HostFields::drop(&mut v, headers).unwrap();
        let body = HostIncomingResponse::consume(&mut v, borrow(&response))
            .unwrap()
            .unwrap();
        assert!(HostIncomingResponse::consume(&mut v, borrow(&response))
            .unwrap()
            .is_err());
        HostIncomingResponse::drop(&mut v, response).unwrap();
        let stream = HostIncomingBody::stream(&mut v, borrow(&body))
            .unwrap()
            .unwrap();
        assert!(HostIncomingBody::finish(&mut v, own(&body)).is_err());
        let mut bytes = Vec::new();
        while bytes.len() < 4 {
            bytes.extend(HostInputStream::blocking_read(v.table, borrow(&stream), 2).unwrap());
        }
        assert_eq!(bytes, b"pong");
        HostInputStream::drop(v.table, stream).unwrap();
        let trailers = HostIncomingBody::finish(&mut v, body).unwrap();
        let poll = HostFutureTrailers::subscribe(&mut v, borrow(&trailers)).unwrap();
        assert!(HostFutureTrailers::drop(&mut v, own(&trailers)).is_err());
        HostPollable::block(v.table, borrow(&poll)).unwrap();
        HostPollable::drop(v.table, poll).unwrap();
        let fields = HostFutureTrailers::get(&mut v, borrow(&trailers))
            .unwrap()
            .unwrap()
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(
            HostFutureTrailers::get(&mut v, borrow(&trailers)).unwrap(),
            Some(Err(()))
        ));
        HostFutureTrailers::drop(&mut v, trailers).unwrap();
        assert_eq!(
            HostFields::get(&mut v, borrow(&fields), "x-done".into()).unwrap(),
            vec![b"yes".to_vec()]
        );
        HostFields::drop(&mut v, fields).unwrap();
        assert!(v.table.is_empty());
        peer.join().unwrap();
    });
}

#[test]
fn http_wit_incoming_request_and_outgoing_response_keep_bodies_independent() {
    let mut ctx = WasiCtx::new(2).unwrap();
    let mut table = ResourceTable::new();
    let mut v = view(&mut ctx, &mut table);
    let (body, mut producer) = IncomingBody::new(StreamContext::Request, Some(4));
    block_on(producer.push(Bytes::from_static(b"ping"))).unwrap();
    producer.finish(None).unwrap();
    let request = v
        .table
        .push(super::HostIncomingRequest {
            method: http::Method::PUT,
            uri: "http://localhost/incoming?q=1".parse().unwrap(),
            scheme: Scheme::Http,
            authority: "localhost".into(),
            headers: FieldMap::new_mutable(1024),
            body: Some(body),
        })
        .unwrap();
    assert!(matches!(
        HostIncomingRequest::method(&mut v, borrow(&request)).unwrap(),
        Method::Put
    ));
    assert_eq!(
        HostIncomingRequest::path_with_query(&mut v, borrow(&request))
            .unwrap()
            .as_deref(),
        Some("/incoming?q=1")
    );
    let headers = HostIncomingRequest::headers(&mut v, borrow(&request)).unwrap();
    assert!(
        HostFields::append(&mut v, borrow(&headers), "x-change".into(), b"no".to_vec()).is_err()
    );
    assert!(HostIncomingRequest::drop(&mut v, own(&request)).is_err());
    HostFields::drop(&mut v, headers).unwrap();
    let body = HostIncomingRequest::consume(&mut v, borrow(&request))
        .unwrap()
        .unwrap();
    assert!(HostIncomingRequest::consume(&mut v, borrow(&request))
        .unwrap()
        .is_err());
    HostIncomingRequest::drop(&mut v, request).unwrap();
    let stream = HostIncomingBody::stream(&mut v, borrow(&body))
        .unwrap()
        .unwrap();
    assert_eq!(
        HostInputStream::blocking_read(v.table, borrow(&stream), 4).unwrap(),
        b"ping"
    );
    HostInputStream::drop(v.table, stream).unwrap();
    HostIncomingBody::drop(&mut v, body).unwrap();
    let fields = HostFields::new(&mut v).unwrap();
    let response = HostOutgoingResponse::new(&mut v, fields).unwrap();
    assert!(
        HostOutgoingResponse::set_status_code(&mut v, borrow(&response), 99)
            .unwrap()
            .is_err()
    );
    HostOutgoingResponse::set_status_code(&mut v, borrow(&response), 202)
        .unwrap()
        .unwrap();
    let headers = HostOutgoingResponse::headers(&mut v, borrow(&response)).unwrap();
    assert!(HostOutgoingResponse::drop(&mut v, own(&response)).is_err());
    HostFields::drop(&mut v, headers).unwrap();
    let body = HostOutgoingResponse::body(&mut v, borrow(&response))
        .unwrap()
        .unwrap();
    let stream = HostOutgoingBody::write(&mut v, borrow(&body))
        .unwrap()
        .unwrap();
    let (send, receive) = futures_channel::oneshot::channel();
    let out = v.new_response_outparam(send).unwrap();
    HostResponseOutparam::set(&mut v, out, Ok(response)).unwrap();
    let mut response = block_on(receive).unwrap().unwrap();
    assert_eq!(response.status, 202);
    HostOutputStream::check_write(v.table, borrow(&stream)).unwrap();
    HostOutputStream::write(v.table, borrow(&stream), b"done".to_vec()).unwrap();
    HostOutputStream::drop(v.table, stream).unwrap();
    HostOutgoingBody::finish(&mut v, body, None).unwrap();
    let mut body = response.body.take().unwrap();
    assert!(
        matches!(block_on(body.next()).unwrap(), Some(crate::http::body::BodyFrame::Data(b)) if b == b"done"[..])
    );
    assert!(block_on(body.next()).unwrap().is_none());
    assert!(v.table.is_empty());
}

#[test]
fn http_wit_error_conversion_preserves_response_header_context_and_io_errors() {
    let error = Error::HttpResponseHeaderSize {
        field_name: Some("x-huge".into()),
        field_size: Some(42),
    };
    let wit: ErrorCode = error.clone().into();
    assert!(matches!(&wit, ErrorCode::HttpResponseHeaderSize(_)));
    assert_eq!(Error::from(wit), error);
    let mut ctx = WasiCtx::new(2).unwrap();
    let mut table = ResourceTable::new();
    let io = table.push(wasmtime::Error::new(error.clone())).unwrap();
    let mut v = view(&mut ctx, &mut table);
    let code = Host::http_error_code(&mut v, borrow(&io)).unwrap().unwrap();
    assert_eq!(Error::from(code), error);
    v.table.delete(io).unwrap();
    let (send, receive) = futures_channel::oneshot::channel();
    let out = v.new_response_outparam(send).unwrap();
    HostResponseOutparam::set(&mut v, out, Err(ErrorCode::HttpRequestDenied)).unwrap();
    assert!(matches!(
        block_on(receive).unwrap(),
        Err(Error::HttpRequestDenied)
    ));
    let (send, receive) = futures_channel::oneshot::channel();
    let out = v.new_response_outparam(send).unwrap();
    HostResponseOutparam::drop(&mut v, out).unwrap();
    assert!(block_on(receive).is_err());
    assert!(v.table.is_empty());
}

struct State {
    ctx: WasiCtx,
    table: ResourceTable,
}
impl crate::WasiView for State {
    fn ctx(&mut self) -> crate::WasiCtxView<'_> {
        crate::WasiCtxView {
            ctx: &mut self.ctx,
            table: &mut self.table,
        }
    }
}
#[test]
fn http_wit_component_uses_canonical_resources_in_both_linkers() {
    bounded(|| {
        for asynchronous in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let peer = std::thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut bytes = Vec::new();
                while !bytes.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    socket.read_exact(&mut byte).unwrap();
                    bytes.push(byte[0]);
                }
                assert!(bytes.starts_with(b"GET / HTTP/1.1\r\n"));
                let mut ending = [0; 5];
                socket.read_exact(&mut ending).unwrap();
                assert_eq!(&ending, b"0\r\n\r\n");
                socket
                    .write_all(b"HTTP/1.1 202 Accepted\r\nContent-Length: 4\r\n\r\npong")
                    .unwrap();
            });
            let engine = wasmtime::Engine::default();
            let component =
                wasmtime::component::Component::new(&engine, include_str!("http.component.wat"))
                    .unwrap();
            let mut linker = wasmtime::component::Linker::new(&engine);
            if asynchronous {
                crate::p2::add_to_linker_async(&mut linker).unwrap();
            } else {
                crate::p2::add_to_linker_sync(&mut linker).unwrap();
            }
            let mut ctx = WasiCtx::new(2).unwrap();
            ctx.http.policy.tcp = true;
            ctx.http
                .policy
                .addresses
                .push((address.ip(), Some(address.port())));
            let mut store = wasmtime::Store::new(
                &engine,
                State {
                    ctx,
                    table: ResourceTable::new(),
                },
            );
            let instance = if asynchronous {
                block_on(linker.instantiate_async(&mut store, &component)).unwrap()
            } else {
                linker.instantiate(&mut store, &component).unwrap()
            };
            let run = instance
                .get_typed_func::<(String,), (u32,)>(&mut store, "run")
                .unwrap();
            store.data_mut().ctx.http.policy.addresses.clear();
            for (tcp, expected) in [(false, 1015), (true, 1004)] {
                store.data_mut().ctx.http.policy.tcp = tcp;
                let denied = if asynchronous {
                    block_on(run.call_async(&mut store, (address.to_string(),))).unwrap()
                } else {
                    run.call(&mut store, (address.to_string(),)).unwrap()
                };
                assert_eq!(denied, (expected,));
                assert!(store.data().table.is_empty());
            }
            store
                .data_mut()
                .ctx
                .http
                .policy
                .addresses
                .push((address.ip(), Some(address.port())));
            let result = if asynchronous {
                block_on(run.call_async(&mut store, (address.to_string(),))).unwrap()
            } else {
                run.call(&mut store, (address.to_string(),)).unwrap()
            };
            assert_eq!(result, (202,));
            peer.join().unwrap();
            assert!(store.data().table.is_empty());
        }
    });
}
