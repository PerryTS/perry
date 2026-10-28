use super::*;
use futures_util::future::{select, Either};
#[test]
fn p3_http_cancelled_send_and_dropped_response_stop_native_io() {
    for after_headers in [false, true] {
        with_store(move |mut store| {
            let server = block_on(crate::http::server::Server::bind(
                "127.0.0.1:0".parse().unwrap(),
                Default::default(),
                4,
            ))
            .unwrap();
            let address = server.local_addr();
            store.data_mut().ctx.as_mut().unwrap().http.policy =
                crate::http::transport::DestinationPolicy {
                    tcp: true,
                    dns: false,
                    addresses: vec![(address.ip(), Some(address.port()))],
                };
            block_on(store.run_concurrent(async |accessor| {
                let http = http(accessor);
                let (request, processed) = http.with(|mut s| {
                    let fields = HostFields::new(&mut s.get()).unwrap();
                    let trailers =
                        FutureReader::new(&mut s, async { wasmtime::error::Ok(Ok(None)) }).unwrap();
                    <WasiHttp as HostRequestWithStore<State>>::new(s, fields, None, trailers, None)
                        .unwrap()
                });
                http.with(|mut s| {
                    HostRequest::set_scheme(&mut s.get(), borrow(&request), Some(Scheme::Http))
                        .unwrap()
                        .unwrap();
                    HostRequest::set_authority(
                        &mut s.get(),
                        borrow(&request),
                        Some(address.to_string()),
                    )
                    .unwrap()
                    .unwrap();
                    HostRequest::set_path_with_query(
                        &mut s.get(),
                        borrow(&request),
                        Some("/cancel".into()),
                    )
                    .unwrap()
                    .unwrap();
                });
                let mut sending = Box::pin(<WasiHttp as client::HostWithStore<State>>::send(
                    &http, request,
                ));
                let accepted = Box::pin(server.accept().unwrap().receive());
                let (request, completion) = match select(sending.as_mut(), accepted).await {
                    Either::Right((request, _)) => request.unwrap(),
                    Either::Left(_) => panic!("HTTP send completed before response existed"),
                };
                if after_headers {
                    let (body, receiver) = OutgoingBody::new(StreamContext::Response, None);
                    request
                        .response
                        .send(Ok(crate::http::p2::HostOutgoingResponse {
                            status: http::StatusCode::OK,
                            headers: FieldMap::default(),
                            body: Some(receiver),
                        }))
                        .map_err(|_| ())
                        .unwrap();
                    let response = sending.await.unwrap();
                    http.with(|s| <WasiHttp as HostResponseWithStore<State>>::drop(s, response))
                        .unwrap();
                    drop(body);
                } else {
                    drop(sending);
                    drop(request);
                }
                let error = receive(accessor, processed).await.unwrap().unwrap_err();
                assert!(matches!(
                    error,
                    ErrorCode::ConnectionTerminated | ErrorCode::HttpResponseIncomplete
                ));
                completion.cancel().await;
            }))
            .unwrap();
            drop(server);
            clean(&mut store);
        });
    }
}

#[test]
fn p3_http_store_drop_cancels_unresolved_guest_trailers() {
    use std::sync::atomic::{AtomicBool, Ordering};
    struct Dropped(Arc<AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    with_store(|mut store| {
        let server = block_on(crate::http::server::Server::bind(
            "127.0.0.1:0".parse().unwrap(),
            Default::default(),
            4,
        ))
        .unwrap();
        let address = server.local_addr();
        store.data_mut().ctx.as_mut().unwrap().http.policy =
            crate::http::transport::DestinationPolicy {
                tcp: true,
                dns: false,
                addresses: vec![(address.ip(), Some(address.port()))],
            };
        let dropped = Arc::new(AtomicBool::new(false));
        let guard = Dropped(dropped.clone());
        let (started, observed) = oneshot::channel();
        block_on(store.run_concurrent(async |accessor| {
            let http = http(accessor);
            let (request, processed) = http.with(|mut s| {
                let fields = HostFields::new(&mut s.get()).unwrap();
                let trailers = FutureReader::new(&mut s, async move {
                    let _guard = guard;
                    started.send(()).unwrap();
                    std::future::pending::<
                        wasmtime::Result<Result<Option<Resource<FieldMap>>, ErrorCode>>,
                    >()
                    .await
                })
                .unwrap();
                <WasiHttp as HostRequestWithStore<State>>::new(s, fields, None, trailers, None)
                    .unwrap()
            });
            http.with(|mut s| {
                HostRequest::set_scheme(&mut s.get(), borrow(&request), Some(Scheme::Http))
                    .unwrap()
                    .unwrap();
                HostRequest::set_authority(
                    &mut s.get(),
                    borrow(&request),
                    Some(address.to_string()),
                )
                .unwrap()
                .unwrap();
                HostRequest::set_path_with_query(
                    &mut s.get(),
                    borrow(&request),
                    Some("/pending-trailers".into()),
                )
                .unwrap()
                .unwrap();
            });
            let mut sending = Box::pin(<WasiHttp as client::HostWithStore<State>>::send(
                &http, request,
            ));
            let accepted = Box::pin(server.accept().unwrap().receive());
            let (request, completion) = match select(sending.as_mut(), accepted).await {
                Either::Right((request, _)) => request.unwrap(),
                Either::Left(_) => panic!("send completed before pending trailers or response"),
            };
            observed.await.unwrap();
            assert!(!dropped.load(Ordering::SeqCst));
            drop(sending);
            assert!(matches!(
                receive(accessor, processed).await.unwrap(),
                Err(ErrorCode::ConnectionTerminated)
            ));
            drop(request);
            completion.cancel().await;
        }))
        .unwrap();
        // Keep the native driver observable after destroying all guest tasks.
        let ctx = store.data_mut().ctx.take().unwrap();
        drop(store);
        assert!(
            dropped.load(Ordering::SeqCst),
            "Store drop retained the pending guest future"
        );
        drop(server);
        let deadline = Instant::now() + Duration::from_secs(2);
        while ctx.outstanding_io() != 0 || ctx.driver.resources() != 0 {
            assert!(
                Instant::now() < deadline,
                "hard cancellation retained native HTTP work"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    });
}
