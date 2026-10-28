//! Backend-independent coverage also executed by the Windows native gate.
use crate::{backend::Wake, table::Table, *};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}
struct Counter;
#[global_allocator]
static ALLOCATOR: Counter = Counter;
fn count() {
    if TRACK.try_with(Cell::get).unwrap_or(false) {
        let _ = COUNT.try_with(|n| n.set(n.get() + 1));
    }
}
// SAFETY: every allocation method forwards the unchanged contract to System;
// initialized thread-local counters do not allocate.
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: the caller supplies a valid layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: the caller supplies a valid layout.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count();
        // SAFETY: the caller's allocation and new size are forwarded unchanged.
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: this pointer and layout belong to a previous System allocation.
        unsafe { System.dealloc(ptr, layout) }
    }
}
fn allocations(work: impl FnOnce()) -> usize {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            TRACK.set(false);
        }
    }
    COUNT.set(0);
    TRACK.set(true);
    let reset = Reset;
    work();
    drop(reset);
    COUNT.get()
}

#[derive(Default)]
struct WakeProbe {
    calls: AtomicUsize,
    fail: AtomicBool,
}
impl Wake for WakeProbe {
    fn wake(&self) -> Result<()> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if self.fail.load(Ordering::Relaxed) {
            Err(Error::new(ErrorKind::Other))
        } else {
            Ok(())
        }
    }
    fn syscall_count(&self) -> u64 {
        self.calls.load(Ordering::Relaxed) as u64
    }
}

#[test]
fn notifier_coalesces_retries_and_closes_without_a_backend() {
    let wake = Arc::new(WakeProbe::default());
    let notifier = Notifier::new(wake.clone());
    notifier.notify().expect("running notify");
    notifier.notify().expect("coalesced notify");
    assert_eq!(notifier.wake_syscalls(), 0);
    assert!(!notifier.park(), "pending notification prevents parking");
    assert!(notifier.begin());
    assert!(!notifier.begin(), "notification consumed once");
    assert!(notifier.park());
    wake.fail.store(true, Ordering::Relaxed);
    assert!(notifier.notify().is_err());
    wake.fail.store(false, Ordering::Relaxed);
    notifier.notify().expect("failed wake is retryable");
    notifier.notify().expect("coalesced parked notify");
    assert_eq!(notifier.wake_syscalls(), 2);
    assert!(notifier.begin());
    notifier.external_park(true).expect("external work wake");
    assert!(notifier.is_parked());
    assert_eq!(notifier.wake_syscalls(), 3);
    notifier.running();
    assert!(!notifier.is_parked());
    assert!(notifier.begin());
    notifier.close();
    assert!(notifier.notify().is_err());
    assert_eq!(notifier.wake_syscalls(), 3);
}

#[test]
fn tables_timers_posts_and_completion_leases_reuse_storage() {
    let mut handles = Table::new(1);
    let mut ops = Table::new(1);
    let mut timers = timer::Heap::new(1);
    #[cfg(not(turnloop_backend = "web"))]
    let at = Instant::now() + Duration::from_secs(1);
    #[cfg(turnloop_backend = "web")]
    let at = Instant::from_duration(Duration::from_secs(1));
    let pool = BufferPool::new(1, 8);
    let mut output = Completions::with_capacity(1);
    let capacity = output.capacity();
    let notifier = Notifier::new(Arc::new(WakeProbe::default()));
    let poster = Poster::new(2, notifier.clone());
    let mut delivered = 0;
    let mut previous = None;
    let allocated = allocations(|| {
        for token in 0..1000 {
            let handle = handles.insert(token).expect("handle slot reused");
            let op = ops.insert(handle).expect("operation slot reused");
            assert!(handles.insert(0).is_none(), "bounded table capacity");
            if let Some(stale) = previous {
                assert!(handles.get(stale).is_none());
                assert!(handles.remove(stale).is_none());
            }
            *ops.get_mut(op).expect("live op") = handle;
            timers.insert(op, at);
            assert!(timers.pop_expired(at - Duration::from_nanos(1)).is_none());
            assert_eq!(timers.pop_expired(at), Some((op, at)));
            timers.insert(op, at);
            assert!(timers.cancel(op));
            assert!(!timers.cancel(op));
            poster
                .post(Token(token), Payload::U64(handle))
                .expect("post");
            assert!(notifier.begin());
            let post = poster.pop().expect("queued post ran");
            assert_eq!(post.token, Token(token));
            assert!(matches!(post.payload, Payload::U64(value) if value == handle));
            assert!(poster.is_empty());
            let mut lease = pool.acquire().expect("released buffer reused");
            assert!(
                pool.acquire().is_none(),
                "retained lease applies backpressure"
            );
            lease.writable().copy_from_slice(&token.to_le_bytes());
            lease.set_len(8);
            output.entries.push(Completion {
                token: Token(token),
                op: None,
                handle: None,
                terminal: true,
                result: OpResult::Read {
                    n: 8,
                    lease: Some(lease),
                },
            });
            assert_eq!(output.len(), 1);
            let completion = output.drain().next().expect("one completion");
            assert_eq!(completion.token, Token(token));
            let OpResult::Read {
                n,
                lease: Some(lease),
            } = completion.result
            else {
                panic!("expected leased read");
            };
            output.clear();
            assert_eq!(n, 8);
            assert_eq!(lease.as_slice(), token.to_le_bytes());
            assert!(pool.acquire().is_none(), "drain preserves owned lease");
            lease.release();
            assert_eq!(ops.remove(op), Some(handle));
            assert_eq!(handles.remove(handle), Some(token));
            previous = Some(handle);
            delivered += 1;
        }
    });
    assert_eq!(delivered, 1000);
    assert_eq!(allocated, 0, "portable steady-state paths allocate nothing");
    assert_eq!(output.capacity(), capacity);
    assert!(output.is_empty());
    assert_eq!(notifier.wake_syscalls(), 0);
    assert!(pool.acquire().is_some());
    poster.close();
    assert!(poster.post(Token(1001), Payload::U64(1)).is_err());
}

#[cfg(any(unix, windows))]
mod socket_controls {
    use super::*;
    fn pair() -> (Loop, Handle, Handle, Handle) {
        let mut l = Loop::new(Config::default()).unwrap();
        let listener = l
            .tcp_listen("127.0.0.1:0".parse().unwrap(), &ListenOpts::default())
            .unwrap();
        l.accept(listener, Token(1)).unwrap();
        let client = l
            .tcp_connect(
                l.local_addr(listener).unwrap(),
                &TcpOpts::default(),
                Token(2),
            )
            .unwrap();
        let mut accepted = None;
        let mut connected = false;
        let mut out = Completions::default();
        let until = l.now() + Duration::from_secs(5);
        while accepted.is_none() || !connected {
            assert!(l.now() < until);
            l.turn(Timeout::Until(until), &mut out).unwrap();
            for c in out.drain() {
                match c.result {
                    OpResult::Accepted { conn, .. } => accepted = Some(conn),
                    OpResult::Connected => connected = true,
                    other => panic!("{other:?}"),
                }
            }
        }
        (l, listener, client, accepted.unwrap())
    }
    #[test]
    fn keepalive_schedule_is_independent_of_enable_switch() {
        let (mut l, _, client, _) = pair();
        l.set_option(client, SocketOption::KeepAlive(None)).unwrap();
        let parameters = KeepAlive {
            idle: Some(Duration::from_secs(7)),
            interval: Some(Duration::from_secs(3)),
            count: Some(4),
        };
        l.set_keepalive_parameters(client, parameters).unwrap();
        assert!(matches!(
            l.get_option(client, SocketOptionKind::KeepAlive).unwrap(),
            SocketOption::KeepAlive(None)
        ));
        let actual = l.keepalive_parameters(client).unwrap();
        assert_eq!(actual.idle, parameters.idle);
        assert_eq!(actual.interval, parameters.interval);
        assert_eq!(actual.count, parameters.count);
        l.set_option(client, SocketOption::KeepAlive(Some(KeepAlive::default())))
            .unwrap();
        assert!(matches!(
            l.get_option(client, SocketOptionKind::KeepAlive).unwrap(),
            SocketOption::KeepAlive(Some(_))
        ));
        let error = l
            .set_keepalive_parameters(
                client,
                KeepAlive {
                    idle: Some(Duration::from_secs(9)),
                    count: Some(0),
                    ..KeepAlive::default()
                },
            )
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::InvalidInput);
        assert_eq!(
            l.keepalive_parameters(client).unwrap().idle,
            parameters.idle
        );
        assert!(matches!(
            l.get_option(client, SocketOptionKind::KeepAlive).unwrap(),
            SocketOption::KeepAlive(Some(_))
        ));
    }
    #[test]
    fn backlog_changes_only_an_existing_listener() {
        let (mut l, listener, client, _) = pair();
        l.set_listen_backlog(listener, 2).unwrap();
        l.set_listen_backlog(listener, 128).unwrap();
        assert_eq!(
            l.set_listen_backlog(listener, 0).unwrap_err().kind,
            ErrorKind::InvalidInput
        );
        assert_eq!(
            l.set_listen_backlog(listener, u32::MAX).unwrap_err().kind,
            ErrorKind::InvalidInput
        );
        assert_eq!(
            l.set_listen_backlog(client, 8).unwrap_err().kind,
            ErrorKind::InvalidInput
        );
        l.accept(listener, Token(3)).unwrap();
        let _peer = std::net::TcpStream::connect(l.local_addr(listener).unwrap()).unwrap();
        let mut out = Completions::default();
        let until = l.now() + Duration::from_secs(5);
        loop {
            assert!(l.now() < until);
            l.turn(Timeout::Until(until), &mut out).unwrap();
            if out
                .drain()
                .any(|c| matches!(c.result, OpResult::Accepted { .. }))
            {
                break;
            }
        }
    }
    #[test]
    fn receive_shutdown_does_not_wait_for_a_backpressured_write() {
        let (mut l, _, client, _) = pair();
        l.set_option(client, SocketOption::SendBufferSize(1024))
            .unwrap();
        let write = l
            .write(client, WriteBuf::Owned(vec![7; 8 * 1024 * 1024]), Token(7))
            .unwrap();
        l.shutdown_read(client).unwrap();
        assert!(l.cancel(write));
        let mut out = Completions::default();
        let until = l.now() + Duration::from_secs(5);
        loop {
            assert!(l.now() < until);
            l.turn(Timeout::Until(until), &mut out).unwrap();
            if out.drain().any(|c| c.op == Some(write) && c.terminal) {
                break;
            }
        }
    }
    #[test]
    fn receive_shutdown_requires_acknowledgement_and_preserves_write() {
        let (mut l, _, client, peer) = pair();
        let read = l.read(client, ReadBuf::Pooled, Token(3)).unwrap();
        assert_eq!(
            l.shutdown_read(client).unwrap_err().kind,
            ErrorKind::WouldBlock
        );
        assert!(l.cancel(read));
        assert_eq!(
            l.shutdown_read(client).unwrap_err().kind,
            ErrorKind::WouldBlock
        );
        let mut out = Completions::default();
        let until = l.now() + Duration::from_secs(5);
        loop {
            assert!(l.now() < until);
            l.turn(Timeout::Until(until), &mut out).unwrap();
            if out.drain().any(|c| c.op == Some(read)) {
                break;
            }
        }
        l.shutdown_read(client).unwrap();
        match l.read(client, ReadBuf::Pooled, Token(6)) {
            Err(error) => assert_eq!(error.os, Some(10058), "receive must be shut down"),
            Ok(read) => loop {
                assert!(l.now() < until);
                l.turn(Timeout::Until(until), &mut out).unwrap();
                let mut terminal = false;
                for c in out.drain() {
                    assert_eq!(c.op, Some(read));
                    match c.result {
                        OpResult::Eof => terminal = true,
                        OpResult::Err(error) => {
                            assert_eq!(error.os, Some(10058));
                            terminal = true;
                        }
                        other => panic!("receive after shutdown: {other:?}"),
                    }
                }
                if terminal {
                    break;
                }
            },
        }
        l.write(client, WriteBuf::Owned(b"x".to_vec()), Token(4))
            .unwrap();
        l.read(peer, ReadBuf::Pooled, Token(5)).unwrap();
        let mut written = false;
        let mut received = false;
        while !written || !received {
            assert!(l.now() < until);
            l.turn(Timeout::Until(until), &mut out).unwrap();
            for c in out.drain() {
                match c.result {
                    OpResult::Wrote(1) => written = true,
                    OpResult::Read {
                        n: 1,
                        lease: Some(b),
                    } => {
                        assert_eq!(b.as_slice(), b"x");
                        received = true;
                    }
                    other => panic!("{other:?}"),
                }
            }
        }
    }
}
