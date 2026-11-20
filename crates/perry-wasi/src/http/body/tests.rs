use super::*;
use futures_lite::future::{block_on, poll_once};

fn fields() -> FieldMap {
    let mut fields = FieldMap::new_mutable(1024);
    fields
        .append("x-checksum".into(), b"complete".to_vec())
        .unwrap();
    fields
}
fn data(frame: Option<BodyFrame>) -> Bytes {
    match frame {
        Some(BodyFrame::Data(bytes)) => bytes,
        _ => panic!("expected data"),
    }
}
fn failed(error: StreamError, expected: Error) {
    match error {
        StreamError::LastOperationFailed(e) => {
            assert_eq!(e.downcast_ref::<Error>(), Some(&expected))
        }
        e => panic!("expected typed HTTP error, got {e:?}"),
    }
}

#[test]
fn outgoing_backpressure_permits_data_then_immutable_trailers() {
    block_on(async {
        let (mut body, mut receiver) = OutgoingBody::new(StreamContext::Request, Some(6));
        let mut output = body.take_stream().unwrap();
        assert!(body.take_stream().is_none());
        assert!(matches!(
            output.write(Bytes::from_static(b"abc")),
            Err(StreamError::Trap(_))
        ));
        assert_eq!(output.check_write().unwrap(), CHUNK_SIZE);
        output.write(Bytes::from_static(b"abc")).unwrap();
        output.flush().unwrap();
        assert_eq!(output.check_write().unwrap(), 0);
        assert!(poll_once(output.ready()).await.is_none());
        assert_eq!(data(receiver.next().await.unwrap()), b"abc"[..]);
        output.ready().await;
        assert_eq!(output.check_write().unwrap(), CHUNK_SIZE);
        output.write(Bytes::from_static(b"def")).unwrap();
        drop(output);
        body.finish(Some(fields())).unwrap().unwrap();
        assert_eq!(data(receiver.next().await.unwrap()), b"def"[..]);
        match receiver.next().await.unwrap().unwrap() {
            BodyFrame::Trailers(mut trailers) => {
                assert_eq!(trailers["x-checksum"], "complete");
                assert_eq!(
                    trailers.append("x-checksum".into(), b"changed".to_vec()),
                    Err(super::super::FieldMapError::Immutable)
                );
            }
            _ => panic!("missing trailers"),
        }
        assert!(receiver.next().await.unwrap().is_none());
    });
}

#[test]
fn outgoing_child_drop_does_not_finish_and_receiver_drop_wakes_writer() {
    block_on(async {
        let (mut body, mut receiver) = OutgoingBody::new(StreamContext::Response, None);
        let output = body.take_stream().unwrap();
        drop(output);
        assert!(poll_once(receiver.next()).await.is_none());
        drop(body);
        assert!(matches!(
            receiver.next().await,
            Err(Error::HttpProtocolError)
        ));
        assert!(receiver.next().await.unwrap().is_none());
        let (mut body, receiver) = OutgoingBody::new(StreamContext::Request, None);
        let mut output = body.take_stream().unwrap();
        output.check_write().unwrap();
        output.write(Bytes::from(vec![4; CHUNK_SIZE])).unwrap();
        assert!(poll_once(output.ready()).await.is_none());
        drop(receiver);
        output.ready().await;
        assert!(matches!(output.check_write(), Err(StreamError::Closed)));
    });
}

#[test]
fn outgoing_length_failures_do_not_emit_excess_and_preserve_typed_direction() {
    block_on(async {
        for context in [StreamContext::Request, StreamContext::Response] {
            let (mut body, mut receiver) = OutgoingBody::new(context, Some(2));
            let mut output = body.take_stream().unwrap();
            output.check_write().unwrap();
            failed(
                output.write(Bytes::from_static(b"abc")).unwrap_err(),
                context.size_error(Some(3)),
            );
            assert!(matches!(output.check_write(), Err(StreamError::Closed)));
            assert!(matches!(receiver.next().await, Err(e) if e == context.size_error(Some(3))));
            drop(output);
            assert_eq!(body.finish(None).unwrap(), Err(context.size_error(Some(3))));
            let (mut body, mut receiver) = OutgoingBody::new(context, Some(5));
            let mut output = body.take_stream().unwrap();
            output.check_write().unwrap();
            output.write(Bytes::from_static(b"abc")).unwrap();
            drop(output);
            assert_eq!(body.finish(None).unwrap(), Err(context.size_error(Some(3))));
            assert_eq!(data(receiver.next().await.unwrap()), b"abc"[..]);
            assert!(matches!(receiver.next().await, Err(e) if e == context.size_error(Some(3))));
        }
    });
}

#[test]
fn outgoing_finish_requires_child_drop_and_empty_body_is_distinct_from_abort() {
    block_on(async {
        let (mut body, mut receiver) = OutgoingBody::new(StreamContext::Response, Some(0));
        let mut output = body.take_stream().unwrap();
        assert!(body.finish(None).is_err());
        failed(output.check_write().unwrap_err(), Error::HttpProtocolError);
        assert!(matches!(
            receiver.next().await,
            Err(Error::HttpProtocolError)
        ));
        let (body, mut receiver) = OutgoingBody::new(StreamContext::Response, Some(0));
        body.finish(None).unwrap().unwrap();
        assert!(receiver.next().await.unwrap().is_none());
    });
}

#[test]
fn outgoing_cancelled_receive_and_transport_failure_do_not_lose_state() {
    block_on(async {
        let (mut body, mut receiver) = OutgoingBody::new(StreamContext::Request, None);
        let mut output = body.take_stream().unwrap();
        assert!(poll_once(receiver.next()).await.is_none());
        output.check_write().unwrap();
        output.write(Bytes::from_static(b"retained")).unwrap();
        assert_eq!(data(receiver.next().await.unwrap()), b"retained"[..]);
        receiver.fail(Error::ConnectionWriteTimeout);
        failed(output.flush().unwrap_err(), Error::ConnectionWriteTimeout);
        assert!(matches!(output.check_write(), Err(StreamError::Closed)));
    });
}

#[test]
fn incoming_partial_reads_backpressure_and_cancelled_delivery_preserve_exact_bytes() {
    block_on(async {
        let (mut body, mut sender) = IncomingBody::new(StreamContext::Response, Some(6));
        let mut input = body.take_stream().unwrap();
        assert!(body.take_stream().is_none());
        assert!(poll_once(input.ready()).await.is_none());
        sender.push(Bytes::from_static(b"abc")).await.unwrap();
        sender.push(Bytes::new()).await.unwrap();
        assert!(poll_once(sender.push(Bytes::from_static(b"wrong")))
            .await
            .is_none());
        assert!(input.read(0).unwrap().is_empty());
        assert_eq!(input.read(1).unwrap(), b"a"[..]);
        assert!(poll_once(sender.push(Bytes::from_static(b"wrong")))
            .await
            .is_none());
        assert_eq!(input.read(100).unwrap(), b"bc"[..]);
        sender.push(Bytes::from_static(b"def")).await.unwrap();
        sender.finish(Some(fields())).unwrap();
        assert_eq!(input.read(100).unwrap(), b"def"[..]);
        assert!(matches!(input.read(1), Err(StreamError::Closed)));
        drop(input);
        let mut trailers = body.finish().unwrap();
        trailers.ready().await;
        let mut value = trailers.get().unwrap().unwrap().unwrap().unwrap();
        assert_eq!(value["x-checksum"], "complete");
        assert_eq!(
            value.remove_all("x-checksum".into()),
            Err(super::super::FieldMapError::Immutable)
        );
        assert!(matches!(trailers.get(), Some(Err(()))));
    });
}

#[test]
fn incoming_finish_discards_data_but_waits_for_trailers_and_keeps_length_accounting() {
    block_on(async {
        let (mut body, mut sender) = IncomingBody::new(StreamContext::Response, Some(6));
        let input = body.take_stream().unwrap();
        sender.push(Bytes::from_static(b"abc")).await.unwrap();
        assert!(poll_once(sender.push(Bytes::from_static(b"def")))
            .await
            .is_none());
        drop(input);
        let mut trailers = body.finish().unwrap();
        assert!(trailers.get().is_none());
        assert!(poll_once(trailers.ready()).await.is_none());
        sender.push(Bytes::from_static(b"def")).await.unwrap();
        assert!(trailers.get().is_none());
        sender.finish(Some(fields())).unwrap();
        trailers.ready().await;
        assert_eq!(
            trailers.get().unwrap().unwrap().unwrap().unwrap()["x-checksum"],
            "complete"
        );
        let (body, mut sender) = IncomingBody::new(StreamContext::Request, Some(10));
        let mut trailers = body.finish().unwrap();
        sender.push(Bytes::from_static(b"short")).await.unwrap();
        assert_eq!(
            sender.finish(None),
            Err(Error::HttpRequestBodySize(Some(5)))
        );
        assert!(matches!(
            trailers.get(),
            Some(Ok(Err(Error::HttpRequestBodySize(Some(5)))))
        ));
    });
}

#[test]
fn incoming_errors_survive_stream_consumption_and_future_result_is_single_use() {
    block_on(async {
        let (mut body, sender) = IncomingBody::new(StreamContext::Response, None);
        let mut input = body.take_stream().unwrap();
        sender.fail(Error::ConnectionReadTimeout);
        input.ready().await;
        failed(input.read(1).unwrap_err(), Error::ConnectionReadTimeout);
        assert!(matches!(input.read(1), Err(StreamError::Closed)));
        drop(input);
        let mut trailers = body.finish().unwrap();
        assert!(matches!(
            trailers.get(),
            Some(Ok(Err(Error::ConnectionReadTimeout)))
        ));
        assert!(matches!(trailers.get(), Some(Err(()))));
        let (mut body, sender) = IncomingBody::new(StreamContext::Response, None);
        let mut input = body.take_stream().unwrap();
        drop(sender);
        failed(input.read(0).unwrap_err(), Error::HttpResponseIncomplete);
        assert!(matches!(input.read(0), Err(StreamError::Closed)));
        let (mut body, sender) = IncomingBody::new(StreamContext::Request, None);
        let mut input = body.take_stream().unwrap();
        drop(sender);
        failed(input.read(0).unwrap_err(), Error::HttpProtocolError);
    });
}

#[test]
fn incoming_parent_and_future_drop_cancel_a_pending_native_producer() {
    block_on(async {
        let (body, mut sender) = IncomingBody::new(StreamContext::Response, None);
        sender.push(Bytes::from_static(b"full")).await.unwrap();
        assert!(poll_once(sender.push(Bytes::from_static(b"pending")))
            .await
            .is_none());
        drop(body);
        assert_eq!(
            sender.push(Bytes::from_static(b"pending")).await,
            Err(Error::ConnectionTerminated)
        );
        let (body, mut sender) = IncomingBody::new(StreamContext::Response, None);
        let future = body.finish().unwrap();
        drop(future);
        assert_eq!(
            sender.push(Bytes::from_static(b"pending")).await,
            Err(Error::ConnectionTerminated)
        );
        let (mut body, mut sender) = IncomingBody::new(StreamContext::Response, None);
        let input = body.take_stream().unwrap();
        assert!(body.finish().is_err());
        assert_eq!(
            sender.push(Bytes::from_static(b"no child escape")).await,
            Err(Error::ConnectionTerminated)
        );
        drop(input);
    });
}

#[test]
fn incoming_rejects_excess_before_delivery_and_checks_short_completion() {
    block_on(async {
        let (mut body, mut sender) = IncomingBody::new(StreamContext::Response, Some(3));
        let mut input = body.take_stream().unwrap();
        assert_eq!(
            sender.push(Bytes::from_static(b"excess")).await,
            Err(Error::HttpResponseBodySize(Some(6)))
        );
        failed(
            input.read(10).unwrap_err(),
            Error::HttpResponseBodySize(Some(6)),
        );
        let (mut body, mut sender) = IncomingBody::new(StreamContext::Response, Some(10));
        let mut input = body.take_stream().unwrap();
        sender.push(Bytes::from_static(b"short")).await.unwrap();
        assert_eq!(
            sender.finish(None),
            Err(Error::HttpResponseBodySize(Some(5)))
        );
        assert_eq!(input.read(10).unwrap(), b"short"[..]);
        failed(
            input.read(1).unwrap_err(),
            Error::HttpResponseBodySize(Some(5)),
        );
        let (mut body, mut sender) = IncomingBody::new(StreamContext::Response, Some(0));
        let mut input = body.take_stream().unwrap();
        assert!(matches!(
            sender.push(Bytes::from(vec![0; CHUNK_SIZE + 1])).await,
            Err(Error::InternalError(_))
        ));
        sender.push(Bytes::new()).await.unwrap();
        assert!(input.read(1).unwrap().is_empty());
        sender.finish(None).unwrap();
        assert!(matches!(input.read(1), Err(StreamError::Closed)));
    });
}

#[test]
fn native_driver_streams_owned_http_bytes_with_bounded_backpressure() {
    let (done, wait) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let driver = perry_component_io::Driver::new(2).unwrap();
        let (mut body, mut sender) =
            IncomingBody::new(StreamContext::Response, Some((CHUNK_SIZE * 32) as u64));
        let mut input = body.take_stream().unwrap();
        let request = driver
            .submitter()
            .submit(move |_| async move {
                for i in 0..32u8 {
                    sender.push(Bytes::from(vec![i; CHUNK_SIZE])).await.unwrap();
                }
                sender.finish(Some(fields())).unwrap();
            })
            .unwrap();
        block_on(async {
            let mut total = 0;
            loop {
                input.ready().await;
                match input.read(911) {
                    Ok(bytes) => {
                        for byte in &bytes {
                            assert_eq!(*byte, (total / CHUNK_SIZE) as u8);
                            total += 1;
                        }
                    }
                    Err(StreamError::Closed) => break,
                    Err(e) => panic!("native stream failed: {e:?}"),
                }
            }
            assert_eq!(total, CHUNK_SIZE * 32);
            drop(input);
            let mut trailers = body.finish().unwrap();
            trailers.ready().await;
            assert_eq!(
                trailers.get().unwrap().unwrap().unwrap().unwrap()["x-checksum"],
                "complete"
            );
            request.await.unwrap();
        });
        drop(driver);
        done.send(()).unwrap();
    });
    wait.recv_timeout(std::time::Duration::from_secs(15))
        .expect("HTTP native producer/consumer watchdog");
    worker.join().unwrap();
}

fn bounded(run: impl FnOnce() + Send + 'static) {
    let (done, wait) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        run();
        done.send(()).unwrap();
    });
    wait.recv_timeout(std::time::Duration::from_secs(15))
        .expect("HTTP stream watchdog");
    worker.join().unwrap();
}

#[test]
fn native_driver_consumes_outgoing_chunks_and_trailers_without_guest_polling() {
    bounded(|| {
        let driver = perry_component_io::Driver::new(2).unwrap();
        let (mut body, mut receiver) =
            OutgoingBody::new(StreamContext::Request, Some((CHUNK_SIZE * 8) as u64));
        let mut output = body.take_stream().unwrap();
        let request = driver
            .submit(move |_| async move {
                let mut count = 0;
                let mut trailers = false;
                while let Some(frame) = receiver.next().await.unwrap() {
                    match frame {
                        BodyFrame::Data(bytes) => {
                            assert!(!trailers);
                            assert_eq!(bytes.len(), CHUNK_SIZE);
                            for byte in bytes {
                                assert_eq!(byte, (count / CHUNK_SIZE) as u8);
                                count += 1;
                            }
                        }
                        BodyFrame::Trailers(fields) => {
                            assert!(!trailers);
                            assert_eq!(fields["x-checksum"], "complete");
                            trailers = true;
                        }
                    }
                }
                assert!(trailers);
                count
            })
            .unwrap();
        block_on(async {
            for i in 0..8u8 {
                output
                    .blocking_write_and_flush(Bytes::from(vec![i; CHUNK_SIZE]))
                    .await
                    .unwrap();
            }
            drop(output);
            body.finish(Some(fields())).unwrap().unwrap();
            assert_eq!(request.await.unwrap(), CHUNK_SIZE * 8);
        });
    });
}

#[test]
fn native_body_cancellation_and_context_teardown_wake_readers_and_preserve_failure() {
    bounded(|| {
        for teardown in [false, true] {
            let driver = perry_component_io::Driver::new(2).unwrap();
            let (mut body, mut sender) = IncomingBody::new(StreamContext::Response, None);
            let mut input = body.take_stream().unwrap();
            let (started, observed) = std::sync::mpsc::channel();
            let request = driver
                .submit(move |_| async move {
                    sender.push(Bytes::from_static(b"accepted")).await.unwrap();
                    assert!(poll_once(sender.push(Bytes::from_static(b"pending")))
                        .await
                        .is_none());
                    started.send(()).unwrap();
                    sender.push(Bytes::from_static(b"pending")).await.unwrap();
                })
                .unwrap();
            observed
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            // Backpressure must yield the I/O thread so unrelated native work progresses.
            let timer = driver
                .submit(|io| async move { io.sleep(std::time::Duration::from_millis(5)).await })
                .unwrap();
            block_on(timer).unwrap().unwrap();
            if teardown {
                drop(driver);
                assert_eq!(block_on(request), Err(perry_component_io::Error::Stopped));
            } else {
                request.cancel();
                assert_eq!(block_on(request), Err(perry_component_io::Error::Cancelled));
                assert_eq!(driver.outstanding(), 0);
            }
            assert_eq!(input.read(64).unwrap(), b"accepted"[..]);
            block_on(input.ready());
            failed(input.read(1).unwrap_err(), Error::HttpResponseIncomplete);
            drop(input);
            let mut trailers = body.finish().unwrap();
            assert!(matches!(
                trailers.get(),
                Some(Ok(Err(Error::HttpResponseIncomplete)))
            ));
        }
    });
}
