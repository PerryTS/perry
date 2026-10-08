//! Transport credit is witnessed against a real server and kernel socket.
use super::*;
use std::cell::Cell;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::Instant;
thread_local! {
    static QUEUED: Cell<usize> = const { Cell::new(0) };
    static HEAD: Cell<bool> = const { Cell::new(false) };
    static CHUNKS: Cell<usize> = const { Cell::new(0) };
}
const WINDOW: usize = 65536;
fn capacity(_: usize) -> usize {
    WINDOW.saturating_sub(QUEUED.with(Cell::get))
}
fn head(_: usize, response: &ResponseOut) {
    assert_eq!(response.status, 200);
    HEAD.with(|v| v.set(true));
}
fn chunk(_: usize, bytes: &[u8]) {
    QUEUED.with(|v| v.set(v.get() + bytes.len()));
    CHUNKS.with(|v| v.set(v.get() + 1));
}
fn done(_: usize, _: Outcome) {}

#[test]
fn consumer_credit_bounds_native_buffers_and_cancel_disconnects() {
    let _owner = become_the_owner_for_test();
    QUEUED.with(|v| v.set(0));
    HEAD.with(|v| v.set(false));
    CHUNKS.with(|v| v.set(0));
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", server.local_addr().unwrap());
    let (send, receive) = mpsc::channel();
    let writer = std::thread::spawn(move || {
        let (mut socket, _) = server.accept().unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut request = [0; 4096];
        socket.read(&mut request).unwrap();
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 134217728\r\n\r\n")
            .unwrap();
        let bytes = [65; 16384];
        let mut sent = 0;
        while sent < 134217728 {
            if socket.write_all(&bytes).is_err() {
                send.send(sent).unwrap();
                return;
            }
            sent += bytes.len();
        }
        send.send(sent).unwrap();
    });
    let ctx = 7654321;
    submit(
        RequestSpec {
            url,
            method: "GET".into(),
            headers: vec![],
            body: None,
            redirect: RedirectMode::Follow,
            abort_key: None,
        },
        Sink {
            ctx,
            on_head: Some(head),
            capacity: Some(capacity),
            on_chunk: Some(chunk),
            on_done: done,
        },
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while capacity(ctx) >= 8192 {
        perry_runtime::event_pump::js_loop_turn_bounded(0);
        assert!(Instant::now() < deadline, "did not fill the stream window");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(HEAD.with(Cell::get));
    assert!(CHUNKS.with(Cell::get) > 0);
    let queued = QUEUED.with(Cell::get);
    for _ in 0..30 {
        perry_runtime::event_pump::js_loop_turn_bounded(0);
    }
    assert_eq!(
        QUEUED.with(Cell::get),
        queued,
        "reading must stop without consumption"
    );
    assert!(queued <= WINDOW);
    ENGINE.with(|e| {
        let e = e.borrow();
        let req = e.requests.values().find(|r| r.sink.ctx == ctx).unwrap();
        assert!(req.body.is_empty() && req.decoded.is_empty());
        assert!(req.queued_bytes <= WINDOW);
        assert!(e.conns[&req.conn.unwrap()].input.len() <= 16384);
    });
    QUEUED.with(|v| v.set(0));
    resume_source(ctx);
    let deadline = Instant::now() + Duration::from_secs(5);
    while QUEUED.with(Cell::get) == 0 {
        perry_runtime::event_pump::js_loop_turn_bounded(0);
        assert!(
            Instant::now() < deadline,
            "consumer progress must resume reading"
        );
    }
    cancel_source(ctx);
    for _ in 0..20 {
        perry_runtime::event_pump::js_loop_turn_bounded(0);
    }
    let sent = receive.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(
        sent < 134217728,
        "cancel must interrupt the download at the server"
    );
    writer.join().unwrap();
}

thread_local! {
    static RESUME_RESULT: std::cell::RefCell<Option<Result<Vec<u8>, String>>> = const { std::cell::RefCell::new(None) };
}
fn resume_done(_: usize, outcome: Outcome) {
    RESUME_RESULT.with(|result| {
        *result.borrow_mut() = Some(match outcome {
            Outcome::Ok(response) => Ok(response.body),
            Outcome::Err(error) => Err(format!("{}: {}", error.code, error.message)),
        });
    });
}

#[test]
fn consumer_resume_before_connect_keeps_the_request_pending() {
    let _owner = become_the_owner_for_test();
    RESUME_RESULT.with(|result| *result.borrow_mut() = None);
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let ctx = 7654322;
    submit(
        RequestSpec {
            url: format!("http://{}/", server.local_addr().unwrap()),
            method: "GET".into(),
            headers: vec![],
            body: None,
            redirect: RedirectMode::Follow,
            abort_key: None,
        },
        Sink {
            ctx,
            on_head: None,
            capacity: None,
            on_chunk: None,
            on_done: resume_done,
        },
    )
    .unwrap();
    // A readable source can request credit before the TCP completion is pumped.
    for _ in 0..3 {
        resume_source(ctx);
        RESUME_RESULT.with(|result| {
            assert!(
                result.borrow().is_none(),
                "early resume completed an idle codec: {:?}",
                result.borrow()
            )
        });
    }
    ENGINE.with(|engine| {
        assert!(engine
            .borrow()
            .requests
            .values()
            .any(|request| request.sink.ctx == ctx))
    });
    cancel_source(ctx);
    RESUME_RESULT.with(|result| {
        assert!(result
            .borrow()
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap_err()
            .contains("ABORT_ERR"))
    });
}

#[test]
fn consumer_resume_before_partial_headers_still_completes_the_body() {
    let _owner = become_the_owner_for_test();
    RESUME_RESULT.with(|result| *result.borrow_mut() = None);
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", server.local_addr().unwrap());
    let writer = std::thread::spawn(move || {
        let (mut socket, _) = server.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = [0; 4096];
        assert!(socket.read(&mut request).unwrap() > 0);
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Len").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        socket
            .write_all(b"gth: 5\r\nConnection: close\r\n\r\nhello")
            .unwrap();
    });
    let ctx = 7654323;
    submit(
        RequestSpec {
            url,
            method: "GET".into(),
            headers: vec![],
            body: None,
            redirect: RedirectMode::Follow,
            abort_key: None,
        },
        Sink {
            ctx,
            on_head: None,
            capacity: None,
            on_chunk: None,
            on_done: resume_done,
        },
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while RESUME_RESULT.with(|result| result.borrow().is_none()) {
        resume_source(ctx);
        perry_runtime::event_pump::js_loop_turn_bounded(0);
        assert!(Instant::now() < deadline, "partial headers stalled");
        std::thread::sleep(Duration::from_millis(1));
    }
    RESUME_RESULT.with(|result| {
        assert_eq!(
            result.borrow().as_ref().unwrap().as_ref().unwrap(),
            b"hello"
        )
    });
    writer.join().unwrap();
}

thread_local! {
    static CHAIN_URL: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}
fn submit_after_response(ctx: usize, outcome: Outcome) {
    if let Outcome::Err(_) = outcome {
        resume_done(ctx, outcome);
        return;
    }
    // This callback starts a distinct exchange while pending deliveries drain.
    submit(
        RequestSpec {
            url: CHAIN_URL.with(|url| url.borrow().clone()),
            method: "GET".into(),
            headers: vec![],
            body: None,
            redirect: RedirectMode::Follow,
            abort_key: None,
        },
        Sink {
            ctx: ctx + 1,
            on_head: None,
            capacity: None,
            on_chunk: None,
            on_done: resume_done,
        },
    )
    .unwrap();
}

#[test]
fn response_callback_does_not_resume_a_new_connecting_request() {
    let _owner = become_the_owner_for_test();
    RESUME_RESULT.with(|result| *result.borrow_mut() = None);
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    server.set_nonblocking(true).unwrap();
    let url = format!("http://{}/", server.local_addr().unwrap());
    CHAIN_URL.with(|value| *value.borrow_mut() = url.clone());
    let writer = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        for _ in 0..2 {
            let mut socket = loop {
                match server.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "next request never connected");
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => panic!("accept: {error}"),
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = [0; 4096];
            if socket.read(&mut request).unwrap() == 0 {
                return;
            }
            // Force a fresh connection for the request issued by the callback.
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
                )
                .unwrap();
        }
    });
    submit(
        RequestSpec {
            url,
            method: "GET".into(),
            headers: vec![],
            body: None,
            redirect: RedirectMode::Follow,
            abort_key: None,
        },
        Sink {
            ctx: 7654324,
            on_head: None,
            capacity: None,
            on_chunk: None,
            on_done: submit_after_response,
        },
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while RESUME_RESULT.with(|result| result.borrow().is_none()) {
        perry_runtime::event_pump::js_loop_turn_bounded(0);
        assert!(Instant::now() < deadline, "callback-issued request stalled");
        std::thread::sleep(Duration::from_millis(1));
    }
    RESUME_RESULT.with(|result| {
        assert_eq!(
            result.borrow().as_ref().unwrap().as_ref().unwrap(),
            b"hello"
        )
    });
    writer.join().unwrap();
}
