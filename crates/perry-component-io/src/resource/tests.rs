use super::*;
use crate::tests::bounded;
use futures_lite::future::block_on;
use futures_util::future::{pending, poll_fn};
use futures_util::io::{AsyncRead, AsyncWrite};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;
mod derive;

const WATCHDOG: Duration = Duration::from_secs(10);
struct Native {
    // Deliberately !Send, constructed, accessed and destroyed on the driver.
    value: Rc<Cell<usize>>,
    owner: thread::ThreadId,
    dropped: mpsc::Sender<thread::ThreadId>,
}
impl Native {
    fn new(dropped: mpsc::Sender<thread::ThreadId>) -> Self {
        Self {
            value: Rc::new(Cell::new(0)),
            owner: thread::current().id(),
            dropped,
        }
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        assert_eq!(thread::current().id(), self.owner);
        let _ = self.dropped.send(self.owner);
    }
}
fn barrier(driver: &Driver) {
    block_on(driver.submit(|_| async {}).unwrap()).unwrap();
}
fn native(driver: &Driver) -> (Resource<Native>, mpsc::Receiver<thread::ThreadId>) {
    let (send, receive) = mpsc::channel();
    let value = block_on(
        driver
            .submitter()
            .register(move |_| async move { Ok::<_, ()>(Native::new(send)) })
            .unwrap(),
    )
    .unwrap()
    .unwrap();
    (value, receive)
}

#[test]
fn native_state_and_last_drop_stay_on_owner_thread() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<Resource<Native>>();
    bounded(|| {
        let driver = Driver::new(2).unwrap();
        let (value, dropped) = native(&driver);
        let clone = value.clone();
        assert_eq!(driver.outstanding(), 0);
        assert_eq!(driver.resources(), 1);
        for expected in 1..=4 {
            let result = block_on(
                value
                    .call(|_, local| async move {
                        let native = local.borrow_mut();
                        assert_eq!(thread::current().id(), native.owner);
                        native.value.set(native.value.get() + 1);
                        (native.value.get(), native.owner)
                    })
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(result.0, expected);
            assert_ne!(result.1, thread::current().id());
        }
        drop(value);
        barrier(&driver);
        assert!(dropped.try_recv().is_err());
        drop(clone);
        let owner = dropped.recv_timeout(WATCHDOG).unwrap();
        assert_ne!(owner, thread::current().id());
        barrier(&driver);
        assert_eq!(driver.resources(), 0);
    });
}

#[test]
fn resource_and_task_limits_are_independent_and_recover() {
    bounded(|| {
        let driver = Driver::new(1).unwrap();
        let (value, dropped) = native(&driver);
        assert!(matches!(
            driver.submitter().register(|_| async { Ok::<_, ()>(()) }),
            Err(Error::Capacity)
        ));
        // A full resource table must still admit operations on its resources.
        assert_eq!(block_on(value.call(|_, _| async { 7 }).unwrap()), Ok(7));
        drop(value);
        dropped.recv_timeout(WATCHDOG).unwrap();
        barrier(&driver);
        let request = driver.submit(|_| pending::<()>()).unwrap();
        assert!(matches!(
            driver.submitter().register(|_| async { Ok::<_, ()>(()) }),
            Err(Error::Capacity)
        ));
        assert_eq!(driver.resources(), 0);
        request.cancel();
        assert_eq!(block_on(request), Err(Error::Cancelled));
        let failed = driver
            .submitter()
            .register(|_| async { Err::<(), _>("native failure") })
            .unwrap();
        assert!(matches!(block_on(failed), Ok(Err("native failure"))));
        assert_eq!(driver.resources(), 0);
        let (value, dropped) = native(&driver);
        drop(value);
        dropped.recv_timeout(WATCHDOG).unwrap();
    });
}

#[test]
fn pending_registration_cancels_local_value_and_releases_capacity() {
    bounded(|| {
        let driver = Driver::new(1).unwrap();
        let (send, dropped) = mpsc::channel();
        let (started, receive) = mpsc::channel();
        let request = driver
            .submitter()
            .register(move |_| async move {
                let value = Native::new(send);
                started.send(()).unwrap();
                pending::<()>().await;
                Ok::<_, ()>(value)
            })
            .unwrap();
        receive.recv_timeout(WATCHDOG).unwrap();
        assert_eq!(driver.resources(), 1);
        request.cancel();
        assert!(matches!(block_on(request), Err(Error::Cancelled)));
        dropped.recv_timeout(WATCHDOG).unwrap();
        assert_eq!(driver.resources(), 0);
        assert_eq!(driver.outstanding(), 0);
    });
}

#[test]
fn undelivered_registered_resource_is_released() {
    bounded(|| {
        let driver = Driver::new(2).unwrap();
        let (send, dropped) = mpsc::channel();
        let (started, receive) = mpsc::channel();
        let request = driver
            .submitter()
            .register(move |_| async move {
                let value = Native::new(send);
                started.send(()).unwrap();
                Ok::<_, ()>(value)
            })
            .unwrap();
        receive.recv_timeout(WATCHDOG).unwrap();
        barrier(&driver);
        assert_eq!(driver.resources(), 1);
        assert_eq!(driver.outstanding(), 0);
        drop(request);
        dropped.recv_timeout(WATCHDOG).unwrap();
        barrier(&driver);
        assert_eq!(driver.resources(), 0);
    });
}

#[test]
fn pending_operation_retains_value_after_last_remote_drop() {
    bounded(|| {
        let driver = Driver::new(2).unwrap();
        let (value, dropped) = native(&driver);
        let (started, receive) = mpsc::channel();
        let request = value
            .call(move |_, local| async move {
                started.send(()).unwrap();
                pending::<()>().await;
                local.borrow_mut().value.get()
            })
            .unwrap();
        receive.recv_timeout(WATCHDOG).unwrap();
        drop(value);
        barrier(&driver);
        assert!(dropped.try_recv().is_err());
        assert_eq!(driver.resources(), 1);
        request.cancel();
        assert_eq!(block_on(request), Err(Error::Cancelled));
        dropped.recv_timeout(WATCHDOG).unwrap();
        assert_eq!(driver.resources(), 0);
    });
}

#[test]
fn shutdown_destroys_resources_even_with_remote_owners_and_pending_calls() {
    bounded(|| {
        let driver = Driver::new(2).unwrap();
        let endpoint = driver.submitter();
        let (value, dropped) = native(&driver);
        let (idle, idle_dropped) = native(&driver);
        let (started, receive) = mpsc::channel();
        let request = value
            .call(move |_, local| async move {
                started.send(()).unwrap();
                pending::<()>().await;
                local.borrow_mut().value.get()
            })
            .unwrap();
        receive.recv_timeout(WATCHDOG).unwrap();
        drop(driver);
        dropped.recv_timeout(WATCHDOG).unwrap();
        idle_dropped.recv_timeout(WATCHDOG).unwrap();
        assert_eq!(block_on(request), Err(Error::Stopped));
        assert!(matches!(value.call(|_, _| async {}), Err(Error::Stopped)));
        assert!(matches!(idle.call(|_, _| async {}), Err(Error::Stopped)));
        assert!(matches!(
            endpoint.register(|_| async { Ok::<_, ()>(()) }),
            Err(Error::Stopped)
        ));
    });
}

#[test]
fn coalesced_release_batch_does_not_strand_queued_work() {
    bounded(|| {
        let driver = Driver::new(3).unwrap();
        let resources: Vec<_> = (0..3).map(|_| native(&driver)).collect();
        let (started, receive) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let held = driver
            .submit(move |_| async move {
                started.send(()).unwrap();
                // Test-only pause to enqueue more commands than one drain batch
                // and coalesce their native notifications before proceeding.
                gate.recv_timeout(WATCHDOG).unwrap();
                pending::<()>().await;
            })
            .unwrap();
        receive.recv_timeout(WATCHDOG).unwrap();
        let dropped: Vec<_> = resources
            .into_iter()
            .map(|(resource, dropped)| {
                drop(resource);
                dropped
            })
            .collect();
        let queued = driver.submit(|_| async { 42 }).unwrap();
        release.send(()).unwrap();
        assert_eq!(block_on(queued), Ok(42));
        for dropped in dropped {
            dropped.recv_timeout(WATCHDOG).unwrap();
        }
        assert_eq!(driver.resources(), 0);
        held.cancel();
        assert_eq!(block_on(held), Err(Error::Cancelled));
    });
}

#[test]
fn cancelling_last_pending_tcp_owner_closes_native_socket() {
    bounded(|| {
        let driver = Driver::new(2).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let request = driver
            .submitter()
            .register(
                move |io| async move { io.connect(address, turnloop::TcpOpts::default()).await },
            )
            .unwrap();
        let (mut peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(WATCHDOG)).unwrap();
        let socket = block_on(request).unwrap().unwrap();
        let (reading, receive) = mpsc::channel();
        let read = socket
            .call(move |_, local| async move {
                let mut data = [0; 1];
                let mut reading = Some(reading);
                poll_fn(|cx| {
                    let result = Pin::new(&mut *local.borrow_mut()).poll_read(cx, &mut data);
                    assert!(result.is_pending());
                    if let Some(reading) = reading.take() {
                        reading.send(()).unwrap();
                    }
                    result
                })
                .await
            })
            .unwrap();
        receive.recv_timeout(WATCHDOG).unwrap();
        drop(socket);
        barrier(&driver);
        assert_eq!(driver.resources(), 1);
        read.cancel();
        assert!(matches!(block_on(read), Err(Error::Cancelled)));
        assert_eq!(peer.read(&mut [0]).unwrap(), 0);
        assert_eq!(driver.resources(), 0);
    });
}

#[test]
fn persistent_tcp_allows_concurrent_read_and_write_and_closes_on_last_drop() {
    bounded(|| {
        let driver = Driver::new(4).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let request = driver
            .submitter()
            .register(
                move |io| async move { io.connect(address, turnloop::TcpOpts::default()).await },
            )
            .unwrap();
        let (mut peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(WATCHDOG)).unwrap();
        peer.set_write_timeout(Some(WATCHDOG)).unwrap();
        let socket = block_on(request).unwrap().unwrap();
        let (reading, receive) = mpsc::channel();
        let read = socket
            .call(move |_, local| async move {
                let mut data = [0; 4];
                let mut offset = 0;
                let mut reading = Some(reading);
                while offset < data.len() {
                    let n = poll_fn(|cx| {
                        let result =
                            Pin::new(&mut *local.borrow_mut()).poll_read(cx, &mut data[offset..]);
                        if result.is_pending() {
                            if let Some(reading) = reading.take() {
                                reading.send(()).unwrap();
                            }
                        }
                        result
                    })
                    .await
                    .unwrap();
                    assert_ne!(n, 0);
                    offset += n;
                }
                data
            })
            .unwrap();
        receive.recv_timeout(WATCHDOG).unwrap();
        let write = socket
            .call(|_, local| async move {
                let data = b"ping";
                let mut offset = 0;
                while offset < data.len() {
                    let n = poll_fn(|cx| {
                        Pin::new(&mut *local.borrow_mut()).poll_write(cx, &data[offset..])
                    })
                    .await
                    .unwrap();
                    assert_ne!(n, 0);
                    offset += n;
                }
                poll_fn(|cx| Pin::new(&mut *local.borrow_mut()).poll_flush(cx))
                    .await
                    .unwrap();
            })
            .unwrap();
        block_on(write).unwrap();
        let mut bytes = [0; 4];
        peer.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"ping");
        peer.write_all(b"pong").unwrap();
        assert_eq!(&block_on(read).unwrap(), b"pong");
        drop(socket);
        assert_eq!(peer.read(&mut [0]).unwrap(), 0);
        barrier(&driver);
        assert_eq!(driver.resources(), 0);
    });
}

#[test]
fn cleanup_is_bounded_independent_and_retains_native_resource() {
    bounded(|| {
        let driver = Driver::new(1).unwrap();
        let (value, dropped) = native(&driver);
        let clone = value.clone();
        let occupied = driver.submit(|_| pending::<()>()).unwrap();
        let (started, receive) = mpsc::channel();
        let first = value
            .call_cleanup(move |_, _| async move {
                started.send(()).unwrap();
                pending::<()>().await;
            })
            .unwrap();
        receive.recv_timeout(WATCHDOG).unwrap();
        let second = clone.call_cleanup(|_, _| pending::<()>()).unwrap();
        assert!(matches!(
            value.call_cleanup(|_, _| async {}),
            Err(Error::Capacity)
        ));
        assert!(matches!(value.call(|_, _| async {}), Err(Error::Capacity)));
        assert_eq!(driver.outstanding(), 1);
        first.cancel();
        assert_eq!(block_on(first), Err(Error::Cancelled));
        assert_eq!(
            block_on(value.call_cleanup(|_, _| async { 42 }).unwrap()),
            Ok(42)
        );
        drop(value);
        drop(clone);
        assert_eq!(driver.resources(), 1);
        assert!(dropped.try_recv().is_err());
        second.cancel();
        assert_eq!(block_on(second), Err(Error::Cancelled));
        dropped.recv_timeout(WATCHDOG).unwrap();
        occupied.cancel();
        assert_eq!(block_on(occupied), Err(Error::Cancelled));
        assert_eq!(driver.resources(), 0);
        assert_eq!(driver.outstanding(), 0);
    });
}

#[test]
fn driver_shutdown_cancels_saturated_cleanup_and_joins_native_owner() {
    bounded(|| {
        let driver = Driver::new(1).unwrap();
        let (value, dropped) = native(&driver);
        let occupied = driver.submit(|_| pending::<()>()).unwrap();
        let cleanup = value.call_cleanup(|_, _| pending::<()>()).unwrap();
        let second = value.call_cleanup(|_, _| pending::<()>()).unwrap();
        drop(driver);
        dropped.recv_timeout(WATCHDOG).unwrap();
        assert_eq!(block_on(occupied), Err(Error::Stopped));
        assert_eq!(block_on(cleanup), Err(Error::Stopped));
        assert_eq!(block_on(second), Err(Error::Stopped));
        assert!(matches!(
            value.call_cleanup(|_, _| async {}),
            Err(Error::Stopped)
        ));
        drop(value);
        assert!(matches!(Driver::new(usize::MAX), Err(Error::Capacity)));
    });
}
