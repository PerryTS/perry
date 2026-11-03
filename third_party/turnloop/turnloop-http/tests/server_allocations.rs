//! Allocation gate for the HTTP servers' lingering close over real TCP. After a
//! warm-up connection, the discard phase (from the completed half-close to the
//! final close) must allocate nothing while the peer sends many late bytes.
#![deny(unsafe_op_in_unsafe_fn)]
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
mod native {
    use std::{
        alloc::{GlobalAlloc, Layout, System},
        cell::{Cell, RefCell},
        future::Future,
        io,
        pin::Pin,
        rc::Rc,
        sync::atomic::{AtomicBool, AtomicUsize, Ordering},
        task::{Context, Poll, Waker},
        time::Duration,
    };
    use turnloop_http::{
        asynchronous::server,
        http1::{BodyLength, Event, Head, Header},
        http2,
    };
    use turnloop_io::{
        turnloop::{Config, LocalExecutor, Timeout, backend::Platform},
        *,
    };
    static ACTIVE: AtomicBool = AtomicBool::new(false);
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    struct Counter;
    fn record() {
        if ACTIVE.load(Ordering::Relaxed) {
            COUNT.fetch_add(1, Ordering::Relaxed);
        }
    }
    // SAFETY: the allocator forwards every layout and pointer unchanged to System.
    unsafe impl GlobalAlloc for Counter {
        unsafe fn alloc(&self, l: Layout) -> *mut u8 {
            record();
            // SAFETY: caller-provided allocation layout is forwarded unchanged.
            unsafe { System.alloc(l) }
        }
        unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
            record();
            // SAFETY: caller-provided allocation layout is forwarded unchanged.
            unsafe { System.alloc_zeroed(l) }
        }
        unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
            record();
            // SAFETY: pointer, original layout and new size satisfy GlobalAlloc.
            unsafe { System.realloc(p, l, n) }
        }
        unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
            // SAFETY: pointer/layout belong to a prior System allocation.
            unsafe { System.dealloc(p, l) }
        }
    }
    #[global_allocator]
    static ALLOCATOR: Counter = Counter;

    const LATE: usize = 256;
    /// Discard-phase accounting shared by the server transport and its peer.
    #[derive(Default)]
    pub struct Phase {
        measure: Cell<bool>,
        lingering: Cell<bool>,
        /// Completed half-closes; each resets the discard counters.
        shutdowns: Cell<u32>,
        peer: RefCell<Option<Waker>>,
        pub reads: Cell<u64>,
        pub discarded: Cell<u64>,
        pub allocations: Cell<usize>,
        pub measured: Cell<bool>,
    }
    impl Phase {
        fn wake_peer(&self) {
            if let Some(waker) = self.peer.borrow_mut().take() {
                waker.wake();
            }
        }
        /// Peer side: wait until the server-side accounting satisfies `ready`.
        async fn until(&self, ready: impl Fn(&Self) -> bool) {
            std::future::poll_fn(|cx| {
                if ready(self) {
                    Poll::Ready(())
                } else {
                    *self.peer.borrow_mut() = Some(cx.waker().clone());
                    Poll::Pending
                }
            })
            .await
        }
    }
    /// Starts counting when the server's half-close completes and stops when its
    /// lingering close reaches `poll_close`, so exactly the discard phase counts.
    struct Metered {
        inner: AsyncIo<Platform>,
        phase: Rc<Phase>,
    }
    impl AsyncRead for Metered {
        fn poll_read(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            bytes: &mut [u8],
        ) -> Poll<io::Result<usize>> {
            let this = self.get_mut();
            let result = Pin::new(&mut this.inner).poll_read(cx, bytes);
            if let (true, Poll::Ready(Ok(n @ 1..))) = (this.phase.lingering.get(), &result) {
                this.phase.reads.set(this.phase.reads.get() + 1);
                this.phase
                    .discarded
                    .set(this.phase.discarded.get() + *n as u64);
                this.phase.wake_peer();
            }
            result
        }
    }
    impl AsyncWrite for Metered {
        fn poll_write(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<io::Result<usize>> {
            Pin::new(&mut self.get_mut().inner).poll_write(cx, bytes)
        }
        fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.get_mut().inner).poll_flush(cx)
        }
        fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            let this = self.get_mut();
            if this.phase.lingering.replace(false) && this.phase.measure.get() {
                ACTIVE.store(false, Ordering::Relaxed);
                this.phase.allocations.set(COUNT.load(Ordering::Relaxed));
                this.phase.measured.set(true);
            }
            Pin::new(&mut this.inner).poll_close(cx)
        }
    }
    impl HalfClose for Metered {
        type Backend = Platform;
        fn executor(&self) -> Option<ExecutorHandle<Platform>> {
            HalfClose::executor(&self.inner)
        }
        fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            let this = self.get_mut();
            std::task::ready!(HalfClose::poll_shutdown(Pin::new(&mut this.inner), cx))?;
            if !this.phase.lingering.replace(true) {
                this.phase.reads.set(0);
                this.phase.discarded.set(0);
                this.phase.shutdowns.set(this.phase.shutdowns.get() + 1);
                this.phase.wake_peer();
                if this.phase.measure.get() {
                    COUNT.store(0, Ordering::Relaxed);
                    ACTIVE.store(true, Ordering::Relaxed);
                }
            }
            Poll::Ready(Ok(()))
        }
    }
    fn finish<T>(task: &mut turnloop::executor::JoinHandle<T>) -> T {
        match Pin::new(task).poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(Ok(v)) => v,
            _ => panic!("task incomplete"),
        }
    }
    /// Read the whole response (through the server's half-close), then send late
    /// bytes the server only discards, then close. Lock-step: each chunk is sent
    /// only after the server discarded the previous one, so every chunk is its own
    /// discard read regardless of how the kernel would coalesce paced writes.
    async fn peer(
        h: &ExecutorHandle<Platform>,
        address: std::net::SocketAddr,
        request: &[u8],
        phase: &Phase,
        round: u32,
    ) {
        let mut stream = h
            .connect(address, Default::default())
            .await
            .expect("connect");
        write_all(&mut stream, request).await.expect("request");
        let mut bytes = [0; 1024];
        let mut total = 0;
        loop {
            let n = read(&mut stream, &mut bytes).await.expect("clean EOF");
            if n == 0 {
                break;
            }
            total += n;
        }
        assert!(total > 0, "final response");
        // The server may observe its shutdown acknowledgement after our EOF.
        phase.until(|p| p.shutdowns.get() > round).await;
        for chunk in 1..=LATE as u64 {
            write_all(&mut stream, b"late").await.expect("late bytes");
            phase.until(|p| p.discarded.get() >= 4 * chunk).await;
        }
        close(&mut stream).await.expect("close");
    }
    fn h2_request() -> Vec<u8> {
        let mut core = http2::Connection::new(http2::Role::Client, Default::default()).expect("h2");
        core.open(
            &[
                Header::new(":method", "GET"),
                Header::new(":scheme", "http"),
                Header::new(":authority", "localhost"),
                Header::new(":path", "/"),
            ],
            true,
        )
        .expect("request");
        core.output().to_vec()
    }
    pub fn lingering_discard_allocates_zero(h2: bool) -> Rc<Phase> {
        let mut executor = LocalExecutor::<Platform>::new(Config::default()).expect("executor");
        let h = executor.handle();
        let client_h = h.clone();
        let listener =
            Listener::bind(&h, "127.0.0.1:0".parse().expect("address")).expect("listener");
        let address = listener.local_addr().expect("address");
        let phase = Rc::new(Phase::default());
        let server_phase = phase.clone();
        let mut server = executor
            .spawn_local(async move {
                for round in 0..2 {
                    let stream = listener.accept().await.expect("accept");
                    server_phase.measure.set(round == 1);
                    let metered = Metered {
                        inner: stream,
                        phase: server_phase.clone(),
                    };
                    // A fresh signal per connection: stopping ends only this one.
                    let signal = server::Shutdown::default();
                    if h2 {
                        let stop = signal.clone();
                        server::http2(metered, signal, |core, event| {
                            if let http2::Event::Headers { stream, .. } = event {
                                core.send_headers(stream, &[Header::new(":status", "204")], true)
                                    .map_err(io::Error::other)?;
                                stop.stop();
                            }
                            Ok(())
                        })
                        .await
                        .expect("h2 lingering close");
                    } else {
                        server::http1(metered, signal, |event, out| {
                            match event {
                                Event::Head(_) => out.start(
                                    &Head {
                                        method: String::new(),
                                        target: String::new(),
                                        status: 200,
                                        version: 1,
                                        headers: vec![],
                                        keep_alive: false,
                                    },
                                    BodyLength::Known(0),
                                )?,
                                Event::End => out.finish(&[])?,
                                _ => {}
                            }
                            Ok(())
                        })
                        .await
                        .expect("h1 lingering close");
                    }
                    assert!(
                        !server_phase.lingering.get(),
                        "lingering close reached close"
                    );
                }
            })
            .expect("spawn");
        let request = if h2 {
            h2_request()
        } else {
            b"GET / HTTP/1.1\r\nhost: localhost\r\n\r\n".to_vec()
        };
        let peer_phase = phase.clone();
        let mut client = executor
            .spawn_local(async move {
                for round in 0..2 {
                    peer(&client_h, address, &request, &peer_phase, round).await;
                }
            })
            .expect("spawn");
        let end = executor.driver().now() + Duration::from_secs(20);
        while !client.is_finished() || !server.is_finished() {
            assert!(executor.driver().now() < end);
            executor.turn(Timeout::Until(end)).expect("turn");
        }
        finish(&mut client);
        finish(&mut server);
        phase
    }
}
fn main() {
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    for (name, h2) in [("http1", false), ("http2", true)] {
        let phase = native::lingering_discard_allocates_zero(h2);
        // The subject ran: at least one discard read per lock-stepped chunk,
        // covering every late byte, all inside the measured window.
        assert!(phase.measured.get(), "{name} discard phase was measured");
        assert_eq!(phase.discarded.get(), 4 * 256, "{name} discarded bytes");
        assert!(
            phase.reads.get() >= 256,
            "{name} one discard read per chunk"
        );
        assert_eq!(
            phase.allocations.get(),
            0,
            "{name} lingering discard allocates"
        );
        println!(
            "{name}: {} discard reads, {} bytes, 0 allocations",
            phase.reads.get(),
            phase.discarded.get()
        );
        println!("test {name}_lingering_discard_allocates_zero ... ok");
    }
    println!("test result: ok. 2 passed; 0 failed; 0 ignored;");
}
