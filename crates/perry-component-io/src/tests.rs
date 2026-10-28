use super::*;
use futures_lite::future::block_on;
use futures_util::io::{AsyncReadExt, AsyncWriteExt};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

const WATCHDOG: Duration = Duration::from_secs(10);

#[test]
fn blocking_work_keeps_timers_live_and_preserves_typed_results() {
    bounded(|| {
        let driver = Driver::new(2).unwrap();
        let (started, receive) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let work = driver
            .submitter()
            .blocking(move || {
                started.send(()).unwrap();
                wait.recv_timeout(WATCHDOG).unwrap();
                Err::<Vec<u8>, _>(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
            })
            .unwrap();
        receive.recv_timeout(WATCHDOG).unwrap();
        let timer = driver
            .submit(|io| async move { io.sleep(Duration::from_millis(5)).await })
            .unwrap();
        assert!(matches!(
            driver.submitter().blocking(|| ()),
            Err(Error::Capacity)
        ));
        block_on(timer).unwrap().unwrap();
        release.send(()).unwrap();
        assert_eq!(
            block_on(work).unwrap().unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        assert_eq!(driver.outstanding(), 0);
    });
}

#[test]
fn cancelled_blocking_work_retains_native_inputs_until_syscall_finishes() {
    bounded(|| {
        struct NativeOwner(mpsc::Sender<()>);
        impl Drop for NativeOwner {
            fn drop(&mut self) {
                let _ = self.0.send(());
            }
        }
        let driver = Driver::new(1).unwrap();
        let endpoint = driver.submitter();
        let (started, receive) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let (dropped, observe_drop) = mpsc::channel();
        let native = NativeOwner(dropped);
        let work = endpoint
            .blocking(move || {
                started.send(()).unwrap();
                wait.recv_timeout(WATCHDOG).unwrap();
                drop(native);
            })
            .unwrap();
        receive.recv_timeout(WATCHDOG).unwrap();
        work.cancel();
        assert_eq!(block_on(work), Err(Error::Cancelled));
        assert!(observe_drop.try_recv().is_err());
        assert_eq!(driver.outstanding(), 0);
        drop(driver);
        assert!(matches!(endpoint.blocking(|| ()), Err(Error::Stopped)));
        release.send(()).unwrap();
        observe_drop.recv_timeout(WATCHDOG).unwrap();
    });
}

// Bound every cross-thread wait, including shutdown, so a broken wakeup fails
// the test instead of leaving a CI worker hung indefinitely.
pub(super) fn bounded(test: impl FnOnce() + Send + 'static) {
    let (send, receive) = mpsc::channel();
    let worker = thread::spawn(move || {
        let result = std::panic::catch_unwind(AssertUnwindSafe(test));
        let _ = send.send(result);
    });
    let result = receive.recv_timeout(WATCHDOG).expect("I/O driver watchdog");
    worker.join().unwrap();
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[test]
fn pending_task_does_not_block_other_tasks_or_timer() {
    bounded(|| {
        let driver = Driver::new(4).unwrap();
        let (started, receive) = mpsc::channel();
        let pending = driver
            .submit(move |_| async move {
                started.send(()).unwrap();
                futures_util::future::pending::<()>().await;
            })
            .unwrap();
        receive.recv_timeout(WATCHDOG).unwrap();
        let timer = driver
            .submit(|io| async move {
                io.sleep(Duration::from_millis(10)).await.unwrap();
                42
            })
            .unwrap();
        assert_eq!(block_on(timer), Ok(42));
        pending.cancel();
        assert_eq!(block_on(pending), Err(Error::Cancelled));
        assert_eq!(driver.outstanding(), 0);
    });
}

#[test]
fn tcp_roundtrip_flushes_and_half_closes() {
    bounded(|| {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket.set_read_timeout(Some(WATCHDOG)).unwrap();
            socket.set_write_timeout(Some(WATCHDOG)).unwrap();
            let mut data = Vec::new();
            socket.read_to_end(&mut data).unwrap();
            assert_eq!(data, b"component-turnloop");
            socket.write_all(&data).unwrap();
        });
        let driver = Driver::new(4).unwrap();
        let request = driver
            .submit(move |io| async move {
                let mut stream = io
                    .connect(address, turnloop::TcpOpts::default())
                    .await
                    .unwrap();
                stream.write_all(b"component-turnloop").await.unwrap();
                stream.flush().await.unwrap();
                futures_util::future::poll_fn(|cx| stream.poll_shutdown(cx))
                    .await
                    .unwrap();
                let mut echoed = Vec::new();
                stream.read_to_end(&mut echoed).await.unwrap();
                echoed
            })
            .unwrap();
        assert_eq!(block_on(request).unwrap(), b"component-turnloop");
        server.join().unwrap();
        assert_eq!(driver.outstanding(), 0);
    });
}

#[test]
fn cancelling_pending_read_releases_the_socket() {
    bounded(|| {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (reading, receive) = mpsc::channel();
        let driver = Driver::new(2).unwrap();
        let request = driver
            .submit(move |io| async move {
                let mut stream = io
                    .connect(address, turnloop::TcpOpts::default())
                    .await
                    .unwrap();
                let mut data = [0; 1];
                let mut read = Box::pin(stream.read(&mut data));
                futures_util::future::poll_fn(|cx| {
                    assert!(read.as_mut().poll(cx).is_pending());
                    Poll::Ready(())
                })
                .await;
                reading.send(()).unwrap();
                read.await.unwrap()
            })
            .unwrap();
        let (mut server, _) = listener.accept().unwrap();
        server.set_read_timeout(Some(WATCHDOG)).unwrap();
        receive.recv_timeout(WATCHDOG).unwrap();
        request.cancel();
        assert_eq!(block_on(request), Err(Error::Cancelled));
        assert_eq!(server.read(&mut [0; 1]).unwrap(), 0);
        assert_eq!(driver.outstanding(), 0);
    });
}

struct Dropped(mpsc::Sender<()>);
impl Drop for Dropped {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

#[test]
fn dropping_request_drops_a_polled_local_future() {
    bounded(|| {
        let driver = Driver::new(1).unwrap();
        let (started, start) = mpsc::channel();
        let (dropped, drop_receive) = mpsc::channel();
        let request = driver
            .submit(move |_| async move {
                let _owner = Dropped(dropped);
                // A non-Send owner proves the future was built on its I/O thread.
                let local = std::rc::Rc::new(9);
                started.send(()).unwrap();
                futures_util::future::pending::<()>().await;
                *local
            })
            .unwrap();
        start.recv_timeout(WATCHDOG).unwrap();
        drop(request);
        drop_receive.recv_timeout(WATCHDOG).unwrap();
        drop(driver);
    });
}

#[test]
fn capacity_rejects_without_invoking_factory_and_recovers_after_cancel() {
    bounded(|| {
        assert!(matches!(Driver::new(0), Err(Error::Capacity)));
        let driver = Driver::new(1).unwrap();
        let request = driver
            .submit(|_| futures_util::future::pending::<()>())
            .unwrap();
        assert_eq!(driver.outstanding(), 1);
        assert!(matches!(
            driver.submit(|_| {
                panic!("rejected factory executed");
                #[allow(unreachable_code)]
                async {}
            }),
            Err(Error::Capacity)
        ));
        request.cancel();
        assert_eq!(block_on(request), Err(Error::Cancelled));
        assert_eq!(block_on(driver.submit(|_| async { 7 }).unwrap()), Ok(7));
    });
}

#[test]
fn shutdown_drops_pending_futures_before_returning() {
    bounded(|| {
        let driver = Driver::new(1).unwrap();
        let (started, start) = mpsc::channel();
        let (dropped, drop_receive) = mpsc::channel();
        let request = driver
            .submit(move |_| async move {
                let _owner = Dropped(dropped);
                started.send(()).unwrap();
                futures_util::future::pending::<()>().await;
            })
            .unwrap();
        start.recv_timeout(WATCHDOG).unwrap();
        drop(driver);
        drop_receive
            .try_recv()
            .expect("shutdown must join native cleanup");
        assert_eq!(block_on(request), Err(Error::Stopped));
    });
}

#[test]
fn resource_submitter_cannot_keep_a_disposed_context_alive() {
    bounded(|| {
        let driver = Driver::new(2).unwrap();
        let endpoint = driver.submitter();
        let captured = endpoint.clone();
        let (started, start) = mpsc::channel();
        let request = endpoint
            .submit(move |_| async move {
                let nested = captured.submit(|_| async { 5 }).unwrap();
                assert_eq!(nested.await, Ok(5));
                started.send(()).unwrap();
                futures_util::future::pending::<()>().await;
                drop(captured);
            })
            .unwrap();
        start.recv_timeout(WATCHDOG).unwrap();
        drop(driver);
        assert_eq!(block_on(request), Err(Error::Stopped));
        assert!(matches!(endpoint.submit(|_| async {}), Err(Error::Stopped)));
    });
}

#[test]
fn cancelling_before_first_poll_does_not_invoke_factory() {
    bounded(|| {
        let driver = Driver::new(2).unwrap();
        let (started, start) = mpsc::channel();
        let (release, released) = mpsc::channel();
        // Only this test deliberately parks the I/O thread, to hold a second
        // submission in the queue while its caller cancels it.
        let barrier = driver
            .submit(move |_| {
                started.send(()).unwrap();
                released.recv_timeout(WATCHDOG).unwrap();
                async {}
            })
            .unwrap();
        start.recv_timeout(WATCHDOG).unwrap();
        let cancelled = driver
            .submit(|_| {
                panic!("a cancelled factory must not execute");
                #[allow(unreachable_code)]
                async {}
            })
            .unwrap();
        cancelled.cancel();
        release.send(()).unwrap();
        assert_eq!(block_on(barrier), Ok(()));
        assert_eq!(block_on(cancelled), Err(Error::Cancelled));
        assert_eq!(driver.outstanding(), 0);
    });
}

#[test]
fn independent_drivers_do_not_share_shutdown_or_capacity() {
    bounded(|| {
        let first = Driver::new(1).unwrap();
        let second = Driver::new(1).unwrap();
        let pending = first
            .submit(|_| futures_util::future::pending::<()>())
            .unwrap();
        let (release, released) = oneshot::channel();
        let survivor = second
            .submit(move |_| async { released.await.unwrap() })
            .unwrap();
        drop(first);
        assert_eq!(block_on(pending), Err(Error::Stopped));
        release.send(3).unwrap();
        assert_eq!(block_on(survivor), Ok(3));
    });
}

// Shipping Perry uses panic=abort; only unwind builds can recover a Rust panic.
#[test]
#[cfg(panic = "unwind")]
fn factory_and_future_panics_do_not_poison_driver() {
    bounded(|| {
        let driver = Driver::new(1).unwrap();
        let factory = driver
            .submit(|_| {
                panic!("factory panic");
                #[allow(unreachable_code)]
                async {}
            })
            .unwrap();
        assert_eq!(block_on(factory), Err(Error::Panicked));
        let future = driver.submit(|_| async { panic!("future panic") }).unwrap();
        assert_eq!(block_on(future), Err(Error::Panicked));
        assert_eq!(block_on(driver.submit(|_| async { 9 }).unwrap()), Ok(9));
    });
}
