use super::*;
use crate::http::body::BodyFrame;
use wasmtime::component::{Component, Linker};
#[test]
fn canonical_http_transfers_body_trailers_and_processing_futures() {
    with_store(|mut store| {
        let component =
            Component::new(store.engine(), include_str!("../streams.component.wat")).unwrap();
        let mut linker = Linker::<State>::new(store.engine());
        crate::p3::add_to_linker(&mut linker).unwrap();
        let expected: Vec<u8> = (0..3 * 1024 * 1024 + 137)
            .map(|n| (n * 19 + 47) as u8)
            .collect();
        block_on(async {
            let instance = linker
                .instantiate_async(&mut store, &component)
                .await
                .unwrap();
            let run = instance
                .get_typed_func::<(
                    Resource<super::super::Request>,
                    FutureReader<Result<(), ErrorCode>>,
                ), ((
                    Resource<super::super::Response>,
                    FutureReader<Result<(), ErrorCode>>,
                ),)>(&mut store, "run")
                .unwrap();
            store
                .run_concurrent(async |accessor| {
                    let http = http(accessor);
                    let (request, processed) = http.with(|mut s| {
                        let headers = HostFields::new(&mut s.get()).unwrap();
                        let trailers = HostFields::new(&mut s.get()).unwrap();
                        HostFields::append(
                            &mut s.get(),
                            borrow(&trailers),
                            "x-canonical".into(),
                            b"preserved".to_vec(),
                        )
                        .unwrap();
                        let contents = StreamReader::new(&mut s, expected.clone()).unwrap();
                        let trailers = FutureReader::new(&mut s, async move {
                            wasmtime::error::Ok(Ok(Some(trailers)))
                        })
                        .unwrap();
                        <WasiHttp as HostRequestWithStore<State>>::new(
                            s,
                            headers,
                            Some(contents),
                            trailers,
                            None,
                        )
                        .unwrap()
                    });
                    let handling = accessor
                        .with(|s| {
                            FutureReader::new(s, async {
                                wasmtime::error::Ok(Err(ErrorCode::LoopDetected))
                            })
                        })
                        .unwrap();
                    let ((response, transmitted),) = run
                        .call_concurrent(accessor, (request, handling))
                        .await
                        .unwrap();
                    let (sent, completion) = oneshot::channel();
                    let mut native = http.with(|mut s| {
                        let response = s.get().table.delete(response).unwrap();
                        response
                            .into_native(&mut s, Box::pin(async move { completion.await.unwrap() }))
                            .unwrap()
                            .body
                            .unwrap()
                    });
                    let mut bytes = Vec::new();
                    let mut trailers = None;
                    while let Some(frame) = native.next().await.unwrap() {
                        match frame {
                            BodyFrame::Data(chunk) => bytes.extend_from_slice(&chunk),
                            BodyFrame::Trailers(fields) => trailers = Some(fields),
                        }
                    }
                    assert_eq!(bytes, expected);
                    assert_eq!(trailers.unwrap()["x-canonical"], "preserved");
                    let mut transmitted = Box::pin(receive(accessor, transmitted));
                    assert!(poll_once(transmitted.as_mut()).await.is_none());
                    sent.send(Ok(())).unwrap();
                    transmitted.await.unwrap().unwrap();
                    assert!(matches!(
                        receive(accessor, processed).await.unwrap(),
                        Err(ErrorCode::LoopDetected)
                    ));
                })
                .await
                .unwrap();
        });
        clean(&mut store);
    });
}

#[test]
fn canonical_http_handler_streams_incoming_native_body_without_network_grants() {
    with_store(|mut store| {
        let component =
            Component::new(store.engine(), include_str!("../handler.component.wat")).unwrap();
        let mut linker = Linker::<State>::new(store.engine());
        crate::p3::add_to_linker(&mut linker).unwrap();
        let expected: Vec<u8> = (0..3 * 1024 * 1024 + 137)
            .map(|n| (n * 23 + 59) as u8)
            .collect();
        block_on(async {
            let instance = linker
                .instantiate_async(&mut store, &component)
                .await
                .unwrap();
            let service = bindings::Service::new(&mut store, &instance).unwrap();
            let (body, mut sender) = IncomingBody::new(StreamContext::Request, None);
            let (request, processed) =
                super::super::Request::from_native(crate::http::p2::HostIncomingRequest {
                    method: http::Method::POST,
                    uri: "http://localhost/echo".parse().unwrap(),
                    scheme: crate::http::p2::bindings::http::types::Scheme::Http,
                    authority: "localhost".into(),
                    headers: FieldMap::default(),
                    body: Some(body),
                });
            store
                .run_concurrent(async |accessor| {
                    let http = http(accessor);
                    let request = http.with(|mut s| s.get().table.push(request)).unwrap();
                    zip(
                        async {
                            let response = service
                                .wasi_http_handler()
                                .call_handle(accessor, request)
                                .await
                                .unwrap()
                                .unwrap();
                            let mut response = http.with(|mut s| {
                                let response = s.get().table.delete(response).unwrap();
                                response
                                    .into_native(&mut s, Box::pin(async { Ok(()) }))
                                    .unwrap()
                                    .body
                                    .unwrap()
                            });
                            let mut bytes = Vec::new();
                            let mut trailers = None;
                            while let Some(frame) = response.next().await.unwrap() {
                                match frame {
                                    BodyFrame::Data(chunk) => bytes.extend_from_slice(&chunk),
                                    BodyFrame::Trailers(fields) => trailers = Some(fields),
                                }
                            }
                            assert_eq!(bytes, expected);
                            assert_eq!(trailers.unwrap()["x-native"], "preserved");
                            processed.await.unwrap();
                        },
                        async {
                            for chunk in expected.chunks(CHUNK_SIZE) {
                                sender
                                    .push(bytes::Bytes::copy_from_slice(chunk))
                                    .await
                                    .unwrap();
                            }
                            let mut fields = FieldMap::new_mutable(1024);
                            fields
                                .append("x-native".into(), b"preserved".to_vec())
                                .unwrap();
                            sender.finish(Some(fields)).unwrap();
                        },
                    )
                    .await;
                })
                .await
                .unwrap();
        });
        clean(&mut store);
    });
}
