use super::{
    bindings::http::{client, types::*},
    *,
};
mod cancellation;
mod canonical;
use crate::http::{
    body::{IncomingBody, OutgoingBody, StreamContext, CHUNK_SIZE},
    FieldMap,
};
use crate::p3::tests::{with_store, State};
use futures_channel::oneshot;
use futures_lite::future::{block_on, poll_once, zip};
use std::{
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::{Duration, Instant},
};
use wasmtime::{
    component::{
        Accessor, FutureReader, Lift, Resource, Source, StreamConsumer, StreamReader, StreamResult,
    },
    StoreContextMut,
};
fn borrow<T: 'static>(resource: &Resource<T>) -> Resource<T> {
    Resource::new_borrow(resource.rep())
}
fn http(accessor: &Accessor<State>) -> Accessor<State, WasiHttp> {
    accessor.with_getter(|state| WasiHttpCtxView {
        ctx: &mut state.ctx.as_mut().unwrap().http,
        table: &mut state.table,
    })
}
fn receive<T: Lift + Send + 'static>(
    accessor: &Accessor<State>,
    value: FutureReader<T>,
) -> oneshot::Receiver<T> {
    let (send, receive) = oneshot::channel();
    accessor
        .with(|s| {
            helpers::pipe(s, value, move |_, value| {
                let _ = send.send(value);
                Ok(())
            })
        })
        .unwrap();
    receive
}
struct Collect(Arc<Mutex<Vec<u8>>>);
impl<T> StreamConsumer<T> for Collect {
    type Item = u8;
    fn poll_consume(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        store: StoreContextMut<T>,
        source: Source<u8>,
        _: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        let mut source = source.as_direct(store);
        let n = source.remaining().len();
        self.0.lock().unwrap().extend_from_slice(source.remaining());
        source.mark_read(n);
        Poll::Ready(Ok(StreamResult::Completed))
    }
}
fn collect(accessor: &Accessor<State>, contents: StreamReader<u8>) -> Arc<Mutex<Vec<u8>>> {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    accessor
        .with(|s| contents.pipe(s, Collect(bytes.clone())))
        .unwrap();
    bytes
}
fn clean(store: &mut wasmtime::Store<State>) {
    store.assert_concurrent_state_empty();
    let ctx = store.data().ctx.as_ref().unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while ctx.outstanding_io() != 0 || ctx.driver.resources() != 0 {
        assert!(
            Instant::now() < deadline,
            "HTTP native ownership survived disposal"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn p3_http_fields_options_and_processing_results_preserve_ownership() {
    with_store(|mut store| {
        block_on(store.run_concurrent(async |accessor| {
            let http = http(accessor);
            let (req, processed) = http.with(|mut s| {
                let fields = HostFields::new(&mut s.get()).unwrap();
                HostFields::append(
                    &mut s.get(),
                    borrow(&fields),
                    "x-test".into(),
                    b"one".to_vec(),
                )
                .unwrap();
                let options = HostRequestOptions::new(&mut s.get()).unwrap();
                HostRequestOptions::set_connect_timeout(&mut s.get(), borrow(&options), Some(7))
                    .unwrap();
                let trailers =
                    FutureReader::new(&mut s, async { wasmtime::error::Ok(Ok(None)) }).unwrap();
                <WasiHttp as HostRequestWithStore<State>>::new(
                    s,
                    fields,
                    None,
                    trailers,
                    Some(options),
                )
                .unwrap()
            });
            let (headers, options) = http.with(|mut s| {
                let headers = HostRequest::get_headers(&mut s.get(), borrow(&req)).unwrap();
                assert!(matches!(
                    HostFields::append(
                        &mut s.get(),
                        borrow(&headers),
                        "x-test".into(),
                        b"two".to_vec()
                    )
                    .unwrap_err()
                    .downcast()
                    .unwrap(),
                    bindings::http::types::HeaderError::Immutable
                ));
                let options = HostRequest::get_options(&mut s.get(), borrow(&req))
                    .unwrap()
                    .unwrap();
                assert!(matches!(
                    HostRequestOptions::set_connect_timeout(
                        &mut s.get(),
                        borrow(&options),
                        Some(1)
                    )
                    .unwrap_err()
                    .downcast()
                    .unwrap(),
                    bindings::http::types::RequestOptionsError::Immutable
                ));
                let copy = HostRequestOptions::clone(&mut s.get(), borrow(&options)).unwrap();
                HostRequestOptions::set_connect_timeout(&mut s.get(), borrow(&copy), Some(9))
                    .unwrap();
                assert_eq!(
                    HostRequestOptions::get_connect_timeout(&mut s.get(), borrow(&options))
                        .unwrap(),
                    Some(7)
                );
                HostRequestOptions::drop(&mut s.get(), copy).unwrap();
                (headers, options)
            });
            let (contents, trailers) = http.with(|mut s| {
                let result = FutureReader::new(&mut s, async {
                    wasmtime::error::Ok(Err(ErrorCode::LoopDetected))
                })
                .unwrap();
                <WasiHttp as HostRequestWithStore<State>>::consume_body(s, req, result).unwrap()
            });
            let bytes = collect(accessor, contents);
            assert!(receive(accessor, trailers)
                .await
                .unwrap()
                .unwrap()
                .is_none());
            assert!(matches!(
                receive(accessor, processed).await.unwrap(),
                Err(ErrorCode::LoopDetected)
            ));
            assert!(bytes.lock().unwrap().is_empty());
            http.with(|mut s| {
                assert_eq!(
                    HostFields::get(&mut s.get(), borrow(&headers), "x-test".into()).unwrap(),
                    vec![b"one".to_vec()]
                );
                HostFields::drop(&mut s.get(), headers).unwrap();
                HostRequestOptions::drop(&mut s.get(), options).unwrap();
            });
        }))
        .unwrap();
        clean(&mut store);
    });
}
async fn read_native(mut body: IncomingBody) -> (Vec<u8>, Option<FieldMap>) {
    let mut stream = body.take_stream().unwrap();
    let mut bytes = Vec::new();
    loop {
        stream.ready().await;
        match stream.read(CHUNK_SIZE) {
            Ok(chunk) => bytes.extend_from_slice(&chunk),
            Err(wasmtime_wasi_io::streams::StreamError::Closed) => break,
            Err(error) => panic!("native HTTP read: {error}"),
        }
    }
    drop(stream);
    let mut trailers = body.finish().unwrap();
    trailers.ready().await;
    (bytes, trailers.get().unwrap().unwrap().unwrap())
}
#[test]
fn p3_http_client_streams_large_bodies_trailers_and_native_completion() {
    large_http_exchange(false, false);
}
#[test]
fn p3_http_tls_streams_with_explicit_trust_and_credentials() {
    large_http_exchange(true, false);
}
#[test]
fn canonical_http_client_suspends_and_streams_native_exchanges() {
    large_http_exchange(false, true);
}
fn large_http_exchange(tls: bool, canonical: bool) {
    with_store(move |mut store| {
        let guest = if canonical {
            let component = wasmtime::component::Component::new(
                store.engine(),
                include_str!("client.component.wat"),
            )
            .unwrap();
            let mut linker = wasmtime::component::Linker::<State>::new(store.engine());
            crate::p3::add_to_linker(&mut linker).unwrap();
            Some(block_on(linker.instantiate_async(&mut store, &component)).unwrap())
        } else {
            None
        };
        let run = guest.map(|instance| {
            instance
                .get_typed_func::<(Resource<super::Request>,), (Resource<super::Response>,)>(
                    &mut store, "run",
                )
                .unwrap()
        });
        let callbacks = guest.map(|instance| {
            instance
                .get_typed_func::<(), (u32,)>(&mut store, "callbacks")
                .unwrap()
        });
        let mut options = crate::http::server::Options::default();
        if tls {
            use turnloop_tls::rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            options.tls = Some(
                turnloop_tls::ServerConfig::new(
                    CertificateDer::pem_slice_iter(include_bytes!(
                        "../../../../perry-tls-session/tests/test-cert.pem"
                    ))
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap(),
                    PrivateKeyDer::from_pem_slice(include_bytes!(
                        "../../../../perry-tls-session/tests/test-key.pem"
                    ))
                    .unwrap(),
                    vec![b"http/1.1".to_vec()],
                    now,
                )
                .unwrap(),
            );
            store
                .data_mut()
                .ctx
                .as_mut()
                .unwrap()
                .http
                .options
                .extra_ca_pem =
                include_bytes!("../../../../perry-tls-session/tests/test-ca.pem").to_vec();
        }
        let server = block_on(crate::http::server::Server::bind(
            "127.0.0.1:0".parse().unwrap(),
            options,
            4,
        ))
        .unwrap();
        let address = server.local_addr();
        store.data_mut().ctx.as_mut().unwrap().http.policy =
            crate::http::transport::DestinationPolicy {
                tcp: true,
                dns: tls,
                addresses: vec![(address.ip(), Some(address.port()))],
            };
        let expected: Vec<u8> = (0..3 * 1024 * 1024 + 137)
            .map(|n| (n * 13 + 71) as u8)
            .collect();
        block_on(store.run_concurrent(async |accessor| {
            let http = http(accessor);
            let (req, processed) =
                http.with(|mut s| {
                    let headers = HostFields::new(&mut s.get()).unwrap();
                    for (name, value) in [
                        ("X-First", "one"),
                        ("X-Other", "middle"),
                        ("x-FIRST", "two"),
                    ] {
                        HostFields::append(
                            &mut s.get(),
                            borrow(&headers),
                            name.into(),
                            value.as_bytes().to_vec(),
                        )
                        .unwrap();
                    }
                    assert_eq!(
                        HostFields::copy_all(&mut s.get(), borrow(&headers)).unwrap(),
                        header_entries()
                    );
                    let trailers = HostFields::new(&mut s.get()).unwrap();
                    HostFields::append(
                        &mut s.get(),
                        borrow(&trailers),
                        "x-upload".into(),
                        b"complete".to_vec(),
                    )
                    .unwrap();
                    let trailers = FutureReader::new(&mut s, async move {
                        wasmtime::error::Ok(Ok(Some(trailers)))
                    })
                    .unwrap();
                    let contents = StreamReader::new(&mut s, expected.clone()).unwrap();
                    <WasiHttp as HostRequestWithStore<State>>::new(
                        s,
                        headers,
                        Some(contents),
                        trailers,
                        None,
                    )
                    .unwrap()
                });
            http.with(|mut s| {
                HostRequest::set_method(&mut s.get(), borrow(&req), Method::Post)
                    .unwrap()
                    .unwrap();
                HostRequest::set_scheme(
                    &mut s.get(),
                    borrow(&req),
                    Some(if tls { Scheme::Https } else { Scheme::Http }),
                )
                .unwrap()
                .unwrap();
                HostRequest::set_authority(
                    &mut s.get(),
                    borrow(&req),
                    Some(if tls {
                        format!("localhost:{}", address.port())
                    } else {
                        address.to_string()
                    }),
                )
                .unwrap()
                .unwrap();
                HostRequest::set_path_with_query(
                    &mut s.get(),
                    borrow(&req),
                    Some("/stream".into()),
                )
                .unwrap()
                .unwrap();
            });
            zip(
                async {
                    let response = if let Some(run) = run {
                        let (response,) = run.call_concurrent(accessor, (req,)).await.unwrap();
                        assert!(
                            callbacks
                                .unwrap()
                                .call_concurrent(accessor, ())
                                .await
                                .unwrap()
                                .0
                                > 0,
                            "HTTP guest did not suspend through a canonical callback"
                        );
                        response
                    } else {
                        <WasiHttp as client::HostWithStore<State>>::send(&http, req)
                            .await
                            .unwrap()
                    };
                    let mut processed = Box::pin(receive(accessor, processed));
                    assert!(
                        poll_once(processed.as_mut()).await.is_none(),
                        "completion preceded response streaming"
                    );
                    let (contents, trailers) = http.with(|mut s| {
                        let headers =
                            HostResponse::get_headers(&mut s.get(), borrow(&response)).unwrap();
                        assert_eq!(
                            HostFields::copy_all(&mut s.get(), borrow(&headers)).unwrap(),
                            header_entries()
                        );
                        HostFields::drop(&mut s.get(), headers).unwrap();
                        assert_eq!(
                            HostResponse::get_status_code(&mut s.get(), borrow(&response)).unwrap(),
                            201
                        );
                        let success =
                            FutureReader::new(&mut s, async { wasmtime::error::Ok(Ok(())) })
                                .unwrap();
                        <WasiHttp as HostResponseWithStore<State>>::consume_body(
                            s, response, success,
                        )
                        .unwrap()
                    });
                    let bytes = collect(accessor, contents);
                    let trailers = receive(accessor, trailers).await.unwrap().unwrap().unwrap();
                    http.with(|mut s| {
                        assert_eq!(
                            HostFields::get(&mut s.get(), borrow(&trailers), "x-download".into())
                                .unwrap(),
                            vec![b"complete".to_vec()]
                        );
                        HostFields::drop(&mut s.get(), trailers).unwrap();
                    });
                    assert_eq!(*bytes.lock().unwrap(), expected);
                    processed.await.unwrap().unwrap();
                },
                async {
                    let (mut request, completion) =
                        server.accept().unwrap().receive().await.unwrap();
                    assert_eq!(
                        request
                            .request
                            .headers
                            .ordered_iter()
                            .map(|(n, v)| (n.to_owned(), v.as_bytes().to_vec()))
                            .collect::<Vec<_>>(),
                        header_entries()
                    );
                    let mut response_headers = request.request.headers.clone();
                    response_headers.set_immutable();
                    let (bytes, trailers) = read_native(request.request.body.take().unwrap()).await;
                    assert_eq!(bytes, expected);
                    assert_eq!(trailers.unwrap()["x-upload"], "complete");
                    let (mut body, receiver) = OutgoingBody::new(StreamContext::Response, None);
                    let mut output = body.take_stream().unwrap();
                    request
                        .response
                        .send(Ok(crate::http::p2::HostOutgoingResponse {
                            status: http::StatusCode::CREATED,
                            headers: response_headers,
                            body: Some(receiver),
                        }))
                        .map_err(|_| ())
                        .unwrap();
                    for chunk in expected.chunks(CHUNK_SIZE) {
                        output.ready().await;
                        assert!(output.check_write().unwrap() >= chunk.len());
                        output.write(bytes::Bytes::copy_from_slice(chunk)).unwrap();
                    }
                    drop(output);
                    let mut trailers = FieldMap::new_mutable(1024);
                    trailers
                        .append("x-download".into(), b"complete".to_vec())
                        .unwrap();
                    body.finish(Some(trailers)).unwrap().unwrap();
                    completion.finish().await.unwrap();
                },
            )
            .await;
        }))
        .unwrap();
        drop(server);
        clean(&mut store);
    });
}

#[test]
fn p3_http_denials_and_body_size_errors_are_typed_and_dispose_resources() {
    with_store(|mut store| {
        block_on(store.run_concurrent(async |accessor| {
            let http = http(accessor);
            let (request, processed) = http.with(|mut s| {
                let fields = HostFields::new(&mut s.get()).unwrap();
                let contents = StreamReader::new(&mut s, vec![1, 2, 3]).unwrap();
                let trailers =
                    FutureReader::new(&mut s, async { wasmtime::error::Ok(Ok(None)) }).unwrap();
                <WasiHttp as HostRequestWithStore<State>>::new(
                    s,
                    fields,
                    Some(contents),
                    trailers,
                    None,
                )
                .unwrap()
            });
            http.with(|mut s| {
                HostRequest::set_scheme(&mut s.get(), borrow(&request), Some(Scheme::Http))
                    .unwrap()
                    .unwrap();
                HostRequest::set_authority(
                    &mut s.get(),
                    borrow(&request),
                    Some("127.0.0.1:9".into()),
                )
                .unwrap()
                .unwrap();
                HostRequest::set_path_with_query(&mut s.get(), borrow(&request), Some("/".into()))
                    .unwrap()
                    .unwrap();
            });
            assert!(matches!(
                <WasiHttp as client::HostWithStore<State>>::send(&http, request)
                    .await
                    .unwrap_err()
                    .downcast()
                    .unwrap(),
                ErrorCode::HttpRequestDenied
            ));
            assert!(matches!(
                receive(accessor, processed).await.unwrap(),
                Err(ErrorCode::HttpRequestDenied)
            ));

            // Invalid metadata must reach both the send result and the
            // constructor's transmission future, before native I/O starts.
            let (invalid, processed) = http.with(|mut s| {
                let fields = HostFields::new(&mut s.get()).unwrap();
                let trailers =
                    FutureReader::new(&mut s, async { wasmtime::error::Ok(Ok(None)) }).unwrap();
                <WasiHttp as HostRequestWithStore<State>>::new(s, fields, None, trailers, None)
                    .unwrap()
            });
            assert!(matches!(
                <WasiHttp as client::HostWithStore<State>>::send(&http, invalid)
                    .await
                    .unwrap_err()
                    .downcast()
                    .unwrap(),
                ErrorCode::HttpRequestUriInvalid
            ));
            assert!(matches!(
                receive(accessor, processed).await.unwrap(),
                Err(ErrorCode::HttpRequestUriInvalid)
            ));

            for (contents, expected) in [(None, 1), (Some(vec![1]), 2), (Some(vec![1, 2]), 1)] {
                let (request, processed) = http.with(|mut s| {
                    let fields = HostFields::new(&mut s.get()).unwrap();
                    let contents = contents.map(|bytes| StreamReader::new(&mut s, bytes).unwrap());
                    let trailers =
                        FutureReader::new(&mut s, async { wasmtime::error::Ok(Ok(None)) }).unwrap();
                    <WasiHttp as HostRequestWithStore<State>>::new(
                        s, fields, contents, trailers, None,
                    )
                    .unwrap()
                });
                let (send, completion) = oneshot::channel();
                let mut native = http.with(|mut s| {
                    let request = s.get().table.delete(request).unwrap();
                    request
                        .body
                        .into_native(
                            &mut s,
                            Box::pin(async move { completion.await.unwrap() }),
                            Some(expected),
                            ErrorCode::HttpRequestBodySize,
                        )
                        .unwrap()
                });
                let result = loop {
                    match native.next().await {
                        Ok(Some(_)) => (),
                        Ok(None) => panic!("invalid body accepted"),
                        Err(error) => break error,
                    }
                };
                assert!(matches!(result, crate::http::Error::HttpRequestBodySize(_)));
                let _ = send.send(Err(result));
                assert!(matches!(
                    receive(accessor, processed).await.unwrap(),
                    Err(ErrorCode::HttpRequestBodySize(_))
                ));
                drop(native);
            }
        }))
        .unwrap();
        clean(&mut store);
    });
}

fn header_entries() -> Vec<(String, Vec<u8>)> {
    [
        ("X-First", "one"),
        ("X-Other", "middle"),
        ("x-FIRST", "two"),
    ]
    .into_iter()
    .map(|(n, v)| (n.into(), v.as_bytes().to_vec()))
    .collect()
}

#[test]
fn p3_http_forwarded_trailers_wait_for_stream_close() {
    with_store(|mut store| {
        block_on(store.run_concurrent(async |accessor| {
            let http = http(accessor);
            let (seen, trailer_ready) = oneshot::channel();
            let (request, processed) = http.with(|mut s| {
                let fields = HostFields::new(&mut s.get()).unwrap();
                let contents = StreamReader::new(&mut s, vec![42; CHUNK_SIZE * 3]).unwrap();
                let trailers = FutureReader::new(&mut s, async move {
                    seen.send(()).unwrap();
                    wasmtime::error::Ok(Ok(None))
                })
                .unwrap();
                <WasiHttp as HostRequestWithStore<State>>::new(
                    s,
                    fields,
                    Some(contents),
                    trailers,
                    None,
                )
                .unwrap()
            });
            let (contents, trailers) = http.with(|mut s| {
                let success =
                    FutureReader::new(&mut s, async { wasmtime::error::Ok(Ok(())) }).unwrap();
                <WasiHttp as HostRequestWithStore<State>>::consume_body(s, request, success)
                    .unwrap()
            });
            let mut trailers = receive(accessor, trailers);
            trailer_ready.await.unwrap();
            assert!(
                poll_once(&mut trailers).await.is_none(),
                "trailers resolved before the consumer read or closed the body"
            );
            let bytes = collect(accessor, contents);
            assert!(trailers.await.unwrap().unwrap().is_none());
            assert_eq!(*bytes.lock().unwrap(), vec![42; CHUNK_SIZE * 3]);
            receive(accessor, processed).await.unwrap().unwrap();
        }))
        .unwrap();
        clean(&mut store);
    });
}
