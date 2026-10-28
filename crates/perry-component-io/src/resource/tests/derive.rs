use super::*;

#[test]
fn deriving_at_capacity_never_invokes_native_factory() {
    bounded(|| {
        let driver = Driver::new(1).unwrap();
        let (parent, dropped) = native(&driver);
        let invoked = Arc::new(AtomicBool::new(false));
        let seen = invoked.clone();
        assert!(matches!(
            parent.derive(move |_, _| async move {
                seen.store(true, Ordering::Release);
                Ok::<_, ()>(Rc::new(1))
            }),
            Err(Error::Capacity)
        ));
        barrier(&driver);
        assert!(!invoked.load(Ordering::Acquire));
        assert_eq!(driver.resources(), 1);
        assert_eq!(
            block_on(
                parent
                    .call(|_, value| async move { value.borrow_mut().value.get() })
                    .unwrap()
            ),
            Ok(0)
        );
        drop(parent);
        dropped.recv_timeout(WATCHDOG).unwrap();
    });
}

#[test]
fn cancelling_derivation_releases_both_local_owners_and_admission() {
    bounded(|| {
        let driver = Driver::new(2).unwrap();
        let (parent, parent_dropped) = native(&driver);
        let (send, child_dropped) = mpsc::channel();
        let (started, receive) = mpsc::channel();
        let request = parent
            .derive(move |_, local| async move {
                let child = Native::new(send);
                assert_eq!(local.borrow_mut().owner, child.owner);
                started.send(()).unwrap();
                pending::<()>().await;
                Ok::<_, ()>(child)
            })
            .unwrap();
        receive.recv_timeout(WATCHDOG).unwrap();
        drop(parent);
        barrier(&driver);
        assert_eq!(driver.resources(), 2);
        assert!(parent_dropped.try_recv().is_err());
        assert!(child_dropped.try_recv().is_err());
        request.cancel();
        assert!(matches!(block_on(request), Err(Error::Cancelled)));
        assert_eq!(
            parent_dropped.recv_timeout(WATCHDOG).unwrap(),
            child_dropped.recv_timeout(WATCHDOG).unwrap()
        );
        assert_eq!(driver.resources(), 0);
        assert_eq!(driver.outstanding(), 0);
    });
}

#[test]
fn accepted_native_connection_outlives_its_listener() {
    bounded(|| {
        let driver = Driver::new(3).unwrap();
        let listener = block_on(
            driver
                .submitter()
                .register(|io| async move {
                    let handle = io.driver().tcp_listen(
                        "127.0.0.1:0".parse().unwrap(),
                        &turnloop::ListenOpts::default(),
                    )?;
                    Ok::<_, turnloop::Error>(io.io(handle))
                })
                .unwrap(),
        )
        .unwrap()
        .unwrap();
        let address = block_on(
            listener
                .call(
                    |io, local| async move { io.driver().local_addr(local.borrow_mut().handle()) },
                )
                .unwrap(),
        )
        .unwrap()
        .unwrap();
        let (started, receive) = mpsc::channel();
        let accept = listener
            .derive(move |io, local| async move {
                let handle = local.borrow_mut().handle();
                let mut accept = Box::pin(io.accept(handle));
                poll_fn(|cx| {
                    assert!(accept.as_mut().poll(cx).is_pending());
                    Poll::Ready(())
                })
                .await;
                started.send(()).unwrap();
                let socket = accept.await?;
                // Keep the listener through native acceptance, then allow it to close.
                drop(local);
                Ok::<_, turnloop::Error>(socket)
            })
            .unwrap();
        receive.recv_timeout(WATCHDOG).unwrap();
        drop(listener);
        let mut peer = std::net::TcpStream::connect(address).unwrap();
        peer.set_read_timeout(Some(WATCHDOG)).unwrap();
        peer.set_write_timeout(Some(WATCHDOG)).unwrap();
        let accepted = block_on(accept).unwrap().unwrap();
        barrier(&driver);
        assert_eq!(
            driver.resources(),
            1,
            "listener remains retained by accepted socket"
        );
        peer.write_all(b"a").unwrap();
        let byte = block_on(
            accepted
                .call(|_, local| async move {
                    let mut byte = [0];
                    let count =
                        poll_fn(|cx| Pin::new(&mut *local.borrow_mut()).poll_read(cx, &mut byte))
                            .await
                            .unwrap();
                    assert_eq!(count, 1);
                    byte[0]
                })
                .unwrap(),
        )
        .unwrap();
        assert_eq!(byte, b'a');
        block_on(
            accepted
                .call(|_, local| async move {
                    let count =
                        poll_fn(|cx| Pin::new(&mut *local.borrow_mut()).poll_write(cx, b"b"))
                            .await
                            .unwrap();
                    assert_eq!(count, 1);
                    poll_fn(|cx| Pin::new(&mut *local.borrow_mut()).poll_flush(cx))
                        .await
                        .unwrap();
                })
                .unwrap(),
        )
        .unwrap();
        let mut byte = [0];
        peer.read_exact(&mut byte).unwrap();
        assert_eq!(&byte, b"b");
        drop(accepted);
        assert_eq!(peer.read(&mut byte).unwrap(), 0);
        barrier(&driver);
        assert_eq!(driver.resources(), 0);
    });
}
