#![cfg(all(
    feature = "turnloop",
    not(all(target_arch = "wasm32", target_os = "unknown"))
))]
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
    time::Duration,
};
use turnloop_http::{
    asynchronous::{
        client::{Client, Options},
        server::{self, Server},
    },
    client::Request,
    http1::{BodyLength, Event, Head, Header},
    http2,
};
use turnloop_io::{
    turnloop::{Config, LocalExecutor, Timeout, backend::Platform},
    *,
};
fn finish<T>(task: &mut turnloop::executor::JoinHandle<T>) -> T {
    match Pin::new(task).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(Ok(v)) => v,
        _ => panic!("task incomplete"),
    }
}
fn response() -> Head {
    Head {
        method: String::new(),
        target: String::new(),
        status: 200,
        version: 1,
        headers: vec![Header::new("content-type", "text/plain")],
        keep_alive: true,
    }
}
fn run_echo(h2: bool) {
    let mut executor = LocalExecutor::<Platform>::new(Config::default()).expect("executor");
    let h = executor.handle();
    let mut server =
        Server::bind(h.clone(), "127.0.0.1:0".parse().expect("address")).expect("server");
    let address = server.local_addr().expect("address");
    let shutdown = server.shutdown();
    let mut task = executor
        .spawn_local(async move {
            server
                .run(move |stream, signal| async move {
                    if h2 {
                        server::http2(stream, signal, |core, event| {
                            match event {
                                http2::Event::Headers {
                                    stream, end_stream, ..
                                } => {
                                    core.send_headers(
                                        stream,
                                        &[Header::new(":status", "200")],
                                        end_stream,
                                    )
                                    .map_err(std::io::Error::other)?;
                                }
                                http2::Event::Data {
                                    stream,
                                    bytes,
                                    end_stream,
                                } => {
                                    core.release_capacity(stream, bytes.len() as u32)
                                        .map_err(std::io::Error::other)?;
                                    assert_eq!(
                                        core.send_data(stream, bytes, end_stream)
                                            .map_err(std::io::Error::other)?,
                                        bytes.len()
                                    );
                                }
                                _ => {}
                            }
                            Ok(())
                        })
                        .await
                    } else {
                        server::http1(stream, signal, |event, out| {
                            match event {
                                Event::Head(_) => out.start(&response(), BodyLength::Chunked)?,
                                Event::Body(bytes) => out.body(bytes)?,
                                Event::End => out.finish(&[])?,
                                _ => {}
                            }
                            Ok(())
                        })
                        .await
                    }
                })
                .await
        })
        .expect("spawn");
    let mut client = executor
        .spawn_local(async move {
            let tls =
                turnloop_tls::ClientConfig::new(Default::default(), 1_789_344_000).expect("config");
            let mut client = Client::new(
                h,
                tls,
                1_789_344_000,
                Options {
                    http2_prior_knowledge: h2,
                    ..Default::default()
                },
            );
            let mut request =
                Request::new(&format!("http://{address}/echo"), "POST").expect("request");
            request.body = vec![b'x'; 32768];
            let mut total = 0;
            for _ in 0..4 {
                let head = client
                    .request(&mut request, |bytes| {
                        assert!(bytes.iter().all(|&b| b == b'x'));
                        total += bytes.len();
                        Ok(())
                    })
                    .await
                    .expect("request");
                assert_eq!(head.status, 200);
            }
            assert_eq!(total, 4 * 32768);
            assert!(client.next_deadline().is_some());
            shutdown.stop();
            total
        })
        .expect("spawn");
    let end = executor.driver().now() + Duration::from_secs(5);
    while !client.is_finished() || !task.is_finished() {
        assert!(executor.driver().now() < end);
        executor.turn(Timeout::Until(end)).expect("turn");
    }
    assert_eq!(finish(&mut client), 4 * 32768);
    finish(&mut task).expect("server drain");
}
#[test]
fn http1_streaming_pool_and_shutdown() {
    run_echo(false);
}
#[test]
fn http2_streaming_pool_and_shutdown() {
    run_echo(true);
}

#[test]
fn idle_http_keepalive_obeys_no_spin() {
    idle_keepalive(false);
}
#[test]
fn idle_http2_keepalive_obeys_no_spin() {
    idle_keepalive(true);
}
fn idle_keepalive(h2: bool) {
    use std::{cell::Cell, rc::Rc};
    let mut executor = LocalExecutor::<Platform>::new(Config::default()).expect("executor");
    let h = executor.handle();
    let mut server =
        Server::bind(h.clone(), "127.0.0.1:0".parse().expect("address")).expect("server");
    let address = server.local_addr().expect("address");
    let signal = server.shutdown();
    let service = executor
        .spawn_local(async move {
            server
                .run(move |stream, signal| async move {
                    if h2 {
                        server::http2(stream, signal, |core, event| {
                            if let http2::Event::Headers { stream, .. } = event {
                                core.send_headers(stream, &[Header::new(":status", "200")], true)
                                    .map_err(std::io::Error::other)?;
                            }
                            Ok(())
                        })
                        .await
                    } else {
                        server::http1(stream, signal, |event, out| {
                            match event {
                                Event::Head(_) => out.start(&response(), BodyLength::Empty)?,
                                Event::End => out.finish(&[])?,
                                _ => {}
                            }
                            Ok(())
                        })
                        .await
                    }
                })
                .await
        })
        .expect("spawn");
    let mut client = executor
        .spawn_local(async move {
            let tls = turnloop_tls::ClientConfig::new(Default::default(), 1_789_344_000)
                .expect("TLS config");
            let mut client = Client::new(
                h,
                tls,
                1_789_344_000,
                Options {
                    http2_prior_knowledge: h2,
                    ..Default::default()
                },
            );
            let mut request = Request::new(&format!("http://{address}/"), "GET").expect("request");
            assert_eq!(
                client
                    .request(&mut request, |_| panic!("empty body"))
                    .await
                    .expect("response")
                    .status,
                200
            );
            client
        })
        .expect("spawn");
    let end = executor.driver().now() + Duration::from_secs(5);
    while !client.is_finished() {
        assert!(executor.driver().now() < end);
        executor.turn(Timeout::Until(end)).expect("turn");
    }
    let _client = finish(&mut client);
    for _ in 0..3 {
        executor.turn(Timeout::Now).expect("settle queued events");
    }
    let mut expiries = 0;
    for delay in [
        Duration::from_micros(500),
        Duration::from_millis(2),
        Duration::from_millis(10),
    ] {
        for _ in 0..20 {
            let h = executor.handle();
            let at = h.now() + delay;
            let done = Rc::new(Cell::new(false));
            let task_done = done.clone();
            let timer = executor
                .spawn_local(async move {
                    h.sleep_until(at).await.expect("timer");
                    assert!(h.now() >= at);
                    task_done.set(true);
                })
                .expect("spawn");
            executor.run_ready();
            let (mut turns, mut empty, mut waits) = (0, 0, 0);
            while !done.get() {
                assert!(turns < 2, "idle keepalive spun");
                let info = executor.turn(Timeout::Until(at)).expect("turn");
                turns += 1;
                empty += info.zero_event_waits;
                // Old revision-2 wait counts included zero-time calls: keep that total.
                waits += info.os_waits + info.discovery_polls;
            }
            assert!(empty <= 1);
            assert!(waits > 0);
            assert!(timer.is_finished());
            expiries += 1;
            executor.turn(Timeout::Now).expect("drain timer close");
        }
    }
    assert_eq!(expiries, 60);
    signal.stop();
    drop(service);
    executor.turn(Timeout::Now).expect("cancel service");
}

#[test]
fn drop_response_future_mid_body_closes_once() {
    use turnloop_http::asynchronous::Http1;
    let mut executor = LocalExecutor::<Platform>::new(Config::default()).expect("executor");
    let h = executor.handle();
    let listener = Listener::bind(&h, "127.0.0.1:0".parse().expect("address")).expect("listener");
    let address = listener.local_addr().expect("address");
    let mut server = executor
        .spawn_local(async move {
            let stream = listener.accept().await.expect("accept");
            let mut http = Http1::new(stream, turnloop_http::http1::Mode::Request);
            assert_eq!(http.head().await.expect("head").method, "GET");
            http.event(|event| {
                assert!(matches!(event, Event::End));
                Ok(())
            })
            .await
            .expect("request end");
            http.send_head(&response(), BodyLength::Known(100))
                .await
                .expect("head");
            http.send_body(b"partial").await.expect("partial body");
            let (mut stream, extra) = http.into_parts().expect("parts");
            assert!(extra.is_empty());
            let mut b = [0];
            assert_eq!(
                read(&mut stream, &mut b).await.expect("cancellation EOF"),
                0
            );
            assert_eq!(read(&mut stream, &mut b).await.expect("repeat EOF"), 0);
            1
        })
        .expect("spawn");
    let mut client = executor
        .spawn_local(async move {
            let stream = h
                .connect(address, Default::default())
                .await
                .expect("connect");
            let mut http = Http1::new(stream, turnloop_http::http1::Mode::Response);
            http.response_to("GET");
            let req = Request::new(&format!("http://{address}/"), "GET").expect("request");
            http.send_head(&req.head(false), BodyLength::Empty)
                .await
                .expect("head");
            http.finish_body(&[]).await.expect("finish");
            assert_eq!(http.head().await.expect("response").status, 200);
            let mut bytes = 0;
            http.event(|event| {
                if let Event::Body(b) = event {
                    assert_eq!(b, b"partial");
                    bytes += b.len();
                }
                Ok(())
            })
            .await
            .expect("body");
            assert_eq!(bytes, 7);
            {
                let mut future =
                    std::pin::pin!(http.event(|_| panic!("cancelled event delivered")));
                std::future::poll_fn(|cx| {
                    assert!(future.as_mut().poll(cx).is_pending());
                    Poll::Ready(())
                })
                .await;
            }
            assert!(!http.reusable());
            assert!(http.event(|_| Ok(())).await.is_err());
            bytes
        })
        .expect("spawn");
    let end = executor.driver().now() + Duration::from_secs(5);
    while !client.is_finished() || !server.is_finished() {
        assert!(executor.driver().now() < end);
        executor.turn(Timeout::Until(end)).expect("turn");
    }
    assert_eq!(finish(&mut client), 7);
    assert_eq!(finish(&mut server), 1);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
#[ignore = "requires private Node HTTP fixtures"]
fn node_async_client_redirect_decompression_and_h2() {
    let mut executor = LocalExecutor::<Platform>::new(Config::default()).expect("executor");
    let h = executor.handle();
    let port = std::env::var("TURNLOOP_TEST_HTTP_PORT").expect("HTTP fixture");
    let port2 = std::env::var("TURNLOOP_TEST_HTTP2_PORT").expect("HTTP2 fixture");
    let mut task = executor
        .spawn_local(async move {
            let tls =
                turnloop_tls::ClientConfig::new(Default::default(), 1_789_344_000).expect("config");
            let mut client = Client::new(h.clone(), tls.clone(), 1_789_344_000, Options::default());
            let mut req =
                Request::new(&format!("http://127.0.0.1:{port}/redirect"), "GET").expect("request");
            let mut result = Vec::new();
            let response = client
                .request(&mut req, |bytes| {
                    result.extend_from_slice(bytes);
                    Ok(())
                })
                .await
                .expect("redirect");
            assert_eq!(response.status, 200);
            assert_eq!(result, b"compressed from node");
            assert_eq!(req.redirects, 1);
            let mut client = Client::new(
                h,
                tls,
                1_789_344_000,
                Options {
                    http2_prior_knowledge: true,
                    ..Default::default()
                },
            );
            let mut count = 0;
            for _ in 0..100 {
                let mut request = Request::new(&format!("http://127.0.0.1:{port2}/echo"), "POST")
                    .expect("request");
                request.body = b"async node h2".to_vec();
                let mut received = Vec::new();
                let head = client
                    .request(&mut request, |bytes| {
                        received.extend_from_slice(bytes);
                        Ok(())
                    })
                    .await
                    .expect("h2 response");
                assert_eq!(head.status, 200);
                assert_eq!(received, b"POST /echo async node h2");
                count += 1;
            }
            let mut request = Request::new(&format!("http://127.0.0.1:{port2}/echo"), "POST")
                .expect("large request");
            request.body = vec![b'x'; 262144];
            let mut received = Vec::new();
            assert_eq!(
                client
                    .request(&mut request, |bytes| {
                        received.extend_from_slice(bytes);
                        Ok(())
                    })
                    .await
                    .expect("flow-controlled upload and response")
                    .status,
                200
            );
            assert_eq!(received.len(), 262144 + b"POST /echo ".len());
            assert!(received.starts_with(b"POST /echo "));
            assert!(received[b"POST /echo ".len()..].iter().all(|&b| b == b'x'));
            count + 1
        })
        .expect("spawn");
    let end = executor.driver().now() + Duration::from_secs(15);
    while !task.is_finished() {
        assert!(executor.driver().now() < end);
        executor.turn(Timeout::Until(end)).expect("turn");
    }
    assert_eq!(finish(&mut task), 101);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn node_https_and_http2_against_async_tls_server() {
    use std::process::Command;
    use turnloop_tls::{ServerConfig, TlsStream, rustls::pki_types::PrivatePkcs8KeyDer};
    for h2 in [false, true] {
        let cert =
            rcgen::generate_simple_self_signed(vec!["localhost".into()]).expect("certificate");
        let ca = cert.cert.pem();
        let tls = ServerConfig::new(
            vec![cert.cert.der().clone()],
            PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der()).into(),
            vec![if h2 {
                b"h2".to_vec()
            } else {
                b"http/1.1".to_vec()
            }],
            1_789_344_000,
        )
        .expect("TLS config");
        let mut executor = LocalExecutor::<Platform>::new(Config::default()).expect("executor");
        let h = executor.handle();
        let listener = Listener::bind(&h, "127.0.0.1:0".parse().expect("address")).expect("listen");
        let address = listener.local_addr().expect("address");
        let end = h.now() + Duration::from_secs(10);
        let mut task = executor
            .spawn_local(async move {
                let stream = listener.accept().await.expect("accept");
                let stream = TlsStream::accept(stream, &tls, &h, end, 1_789_344_000)
                    .await
                    .expect("TLS handshake");
                assert_eq!(
                    stream.alpn_protocol(),
                    Some(if h2 {
                        b"h2".as_slice()
                    } else {
                        b"http/1.1".as_slice()
                    })
                );
                let signal = server::Shutdown::default();
                let stop = signal.clone();
                let mut count = 0;
                if h2 {
                    server::http2(stream, signal, |core, event| {
                        if let http2::Event::Headers {
                            stream, end_stream, ..
                        } = event
                        {
                            assert!(end_stream);
                            core.send_headers(
                                stream,
                                &[
                                    Header::new(":status", "200"),
                                    Header::new("content-length", "4"),
                                ],
                                false,
                            )
                            .map_err(std::io::Error::other)?;
                            assert_eq!(
                                core.send_data(stream, b"node", true)
                                    .map_err(std::io::Error::other)?,
                                4
                            );
                            count += 1;
                            stop.stop();
                        }
                        Ok(())
                    })
                    .await
                    .expect("h2 drain");
                } else {
                    server::http1(stream, signal, |event, out| {
                        match event {
                            Event::Head(head) => {
                                assert_eq!(head.method, "GET");
                                out.start(&response(), BodyLength::Known(4))?;
                                out.body(b"node")?;
                            }
                            Event::End => {
                                out.finish(&[])?;
                                count += 1;
                                stop.stop();
                            }
                            _ => {}
                        }
                        Ok(())
                    })
                    .await
                    .expect("h1 drain");
                }
                count
            })
            .expect("spawn");
        let script = if h2 {
            format!(
                "const c=require('node:http2').connect('https://{address}',{{ca:process.env.TURNLOOP_CA,servername:'localhost'}});c.on('error',()=>process.exit(2));const r=c.request({{':path':'/'}});let body='';r.on('data',b=>body+=b);r.on('end',()=>{{if(body!=='node')process.exit(3);c.close()}});r.end();"
            )
        } else {
            format!(
                "require('node:https').get('https://{address}/',{{ca:process.env.TURNLOOP_CA,servername:'localhost',ALPNProtocols:['http/1.1']}},r=>{{let body='';r.on('data',b=>body+=b);r.on('end',()=>{{if(r.statusCode!==200||body!=='node')process.exit(3)}})}}).on('error',()=>process.exit(2));"
            )
        };
        let mut child = Command::new("node")
            .args(["-e", &script])
            .env("TURNLOOP_CA", ca)
            .spawn()
            .expect("Node required");
        while !task.is_finished() {
            assert!(executor.driver().now() < end);
            executor.turn(Timeout::Until(end)).expect("turn");
        }
        assert_eq!(finish(&mut task), 1);
        executor.turn(Timeout::Now).expect("close delivery");
        assert!(child.wait().expect("Node exit").success());
    }
}

#[test]
fn drop_server_cancels_inflight_request_once() {
    use std::{cell::Cell, rc::Rc};
    use turnloop_http::asynchronous::Http1;
    let mut executor = LocalExecutor::<Platform>::new(Config::default()).expect("executor");
    let h = executor.handle();
    let mut server =
        Server::bind(h.clone(), "127.0.0.1:0".parse().expect("address")).expect("server");
    let address = server.local_addr().expect("address");
    let seen = Rc::new(Cell::new(0));
    let received = seen.clone();
    let mut service = executor
        .spawn_local(async move {
            server
                .run(move |stream, signal| {
                    let received = received.clone();
                    async move {
                        server::http1(stream, signal, |event, _| {
                            if let Event::Body(bytes) = event {
                                assert_eq!(bytes, b"partial");
                                received.set(received.get() + bytes.len());
                            }
                            Ok(())
                        })
                        .await
                    }
                })
                .await
        })
        .expect("spawn");
    let mut client = executor
        .spawn_local(async move {
            let stream = h
                .connect(address, Default::default())
                .await
                .expect("connect");
            let mut http = Http1::new(stream, turnloop_http::http1::Mode::Response);
            let request = Request::new(&format!("http://{address}/"), "POST").expect("request");
            http.send_head(&request.head(false), BodyLength::Known(100))
                .await
                .expect("head");
            http.send_body(b"partial").await.expect("partial body");
            assert!(http.head().await.is_err());
            1
        })
        .expect("spawn");
    let end = executor.driver().now() + Duration::from_secs(5);
    while seen.get() == 0 {
        assert!(executor.driver().now() < end);
        executor.turn(Timeout::Until(end)).expect("turn");
    }
    assert_eq!(seen.get(), 7);
    service.cancel();
    while !client.is_finished() {
        assert!(executor.driver().now() < end);
        executor.turn(Timeout::Until(end)).expect("turn");
    }
    assert_eq!(finish(&mut client), 1);
    assert!(matches!(
        Pin::new(&mut service).poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Err(turnloop::JoinError::Cancelled))
    ));
}

#[test]
fn expect_continue_timeout_and_early_response() {
    use turnloop_http::{asynchronous::Http1, http1::Mode};
    let mut executor = LocalExecutor::<Platform>::new(Config::default()).expect("executor");
    let h = executor.handle();
    let listener = Listener::bind(&h, "127.0.0.1:0".parse().expect("address")).expect("listen");
    let address = listener.local_addr().expect("address");
    let mut server = executor
        .spawn_local(async move {
            let mut uploaded = 0;
            for case in 0..3 {
                let stream = listener.accept().await.expect("accept");
                let mut conn = Http1::new(stream, Mode::Request);
                let head = conn.head().await.expect("request head");
                assert!(head.token("expect", "100-continue"));
                if case == 0 {
                    for status in [103, 100] {
                        conn.send_head(
                            &Head {
                                status,
                                ..response()
                            },
                            BodyLength::Empty,
                        )
                        .await
                        .expect("informational");
                    }
                }
                if case < 2 {
                    loop {
                        let mut end = false;
                        assert!(
                            conn.event(|event| {
                                match event {
                                    Event::Body(bytes) => {
                                        assert!(bytes.iter().all(|&b| b == b'x'));
                                        uploaded += bytes.len();
                                    }
                                    Event::End => end = true,
                                    _ => {}
                                }
                                Ok(())
                            })
                            .await
                            .expect("upload")
                        );
                        if end {
                            break;
                        }
                    }
                    // A finished decoder must return immediately without another read.
                    for _ in 0..3 {
                        assert!(
                            !conn
                                .event(|_| panic!("duplicate completion"))
                                .await
                                .expect("terminal")
                        );
                    }
                }
                conn.send_head(
                    &Head {
                        status: if case == 2 { 417 } else { 200 },
                        keep_alive: false,
                        ..response()
                    },
                    BodyLength::Empty,
                )
                .await
                .expect("response");
                conn.finish_body(&[]).await.expect("finish");
                if case == 2 {
                    let mut stream = conn.into_inner().expect("no early upload");
                    assert_eq!(read(&mut stream, &mut [0; 1]).await.expect("peer EOF"), 0);
                }
            }
            uploaded
        })
        .expect("spawn");
    let mut client = executor
        .spawn_local(async move {
            let tls = turnloop_tls::ClientConfig::new(Default::default(), 1_789_344_000)
                .expect("TLS config");
            let client = |continue_timeout| {
                Client::new(
                    h.clone(),
                    tls.clone(),
                    1_789_344_000,
                    Options {
                        continue_timeout,
                        ..Default::default()
                    },
                )
            };
            let mut request = Request::new(&format!("http://{address}/"), "POST").expect("request");
            request.headers.push(Header::new("expect", "100-continue"));
            request.body = vec![b'x'; 16384];
            // Cases 0 and 1 need the 2 ms continue timeout to expire (case 1 never
            // sends 100). Case 2 proves an early final response suppresses the upload,
            // so its timeout must not be able to win the race on a slow runtime.
            let mut short = client(Duration::from_millis(2));
            let mut long = client(Duration::from_secs(30));
            for case in 0..3 {
                let client = if case == 2 { &mut long } else { &mut short };
                let head = client
                    .request(&mut request, |_| panic!("empty response"))
                    .await
                    .expect("response");
                assert_eq!(head.status, if case == 2 { 417 } else { 200 });
            }
            3
        })
        .expect("spawn");
    let end = executor.driver().now() + Duration::from_secs(5);
    while !server.is_finished() || !client.is_finished() {
        assert!(executor.driver().now() < end);
        executor.turn(Timeout::Until(end)).expect("turn");
    }
    assert_eq!(finish(&mut client), 3);
    assert_eq!(finish(&mut server), 32768);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn curl_against_async_http1_server() {
    use std::process::{Command, Stdio};
    let mut executor = LocalExecutor::<Platform>::new(Config::default()).expect("executor");
    let h = executor.handle();
    let listener = Listener::bind(&h, "127.0.0.1:0".parse().expect("address")).expect("listen");
    let address = listener.local_addr().expect("address");
    let mut task = executor
        .spawn_local(async move {
            let stream = listener.accept().await.expect("accept");
            let signal = server::Shutdown::default();
            let stop = signal.clone();
            let mut received = 0;
            server::http1(stream, signal, |event, out| {
                match event {
                    Event::Head(head) => {
                        assert_eq!(head.method, "POST");
                        out.start(&response(), BodyLength::Chunked)?;
                    }
                    Event::Body(bytes) => {
                        assert_eq!(bytes, b"curl async echo");
                        received += bytes.len();
                        out.body(bytes)?;
                    }
                    Event::End => {
                        out.finish(&[])?;
                        stop.stop();
                    }
                    _ => {}
                }
                Ok(())
            })
            .await
            .expect("server drain");
            received
        })
        .expect("spawn");
    let curl = std::env::var_os("TURNLOOP_TEST_CURL").unwrap_or_else(|| "curl".into());
    let mut child = Command::new(curl)
        .args([
            "--http1.1",
            "--silent",
            "--show-error",
            "--fail",
            "--noproxy",
            "*",
            "--max-time",
            "5",
            "--data-binary",
            "curl async echo",
            &format!("http://{address}/"),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("curl required for HTTP/1.1");
    let end = executor.driver().now() + Duration::from_secs(10);
    while !task.is_finished() {
        if executor.driver().now() >= end {
            // Tell a hung server (curl exited, its close never delivered) from a hung curl.
            let exited = child.try_wait();
            let _ = child.kill();
            panic!(
                "server task pending after 10 s; curl before kill: {exited:?}; curl output: {:?}",
                child.wait_with_output()
            );
        }
        executor.turn(Timeout::Until(end)).expect("turn");
    }
    assert_eq!(finish(&mut task), 15);
    executor.turn(Timeout::Now).expect("close delivery");
    let output = child.wait_with_output().expect("curl exit");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"curl async echo");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn node_https_via_authenticated_connect_proxy() {
    use std::{
        io::{BufRead, BufReader},
        process::{Command, Stdio},
    };
    use turnloop_http::client::ProxyEnvironment;
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).expect("certificate");
    let script = r#"
const assert = require('node:assert/strict');
const https = require('node:https'), http = require('node:http'), net = require('node:net');
let connects = 0, requests = 0;
const watchdog = setTimeout(() => process.exit(7), 10000);
const origin = https.createServer({key: process.env.KEY, cert: process.env.CERT}, (req, res) => {
  assert.equal(req.method, 'POST'); assert.equal(req.url, '/through-proxy');
  let body = ''; req.on('data', b => body += b); req.on('end', () => {
    assert.equal(body, 'verified TLS upload'); requests++;
    res.setHeader('connection', 'close'); res.end('verified TLS response');
  });
});
const proxy = http.createServer();
proxy.on('connect', (req, client, head) => {
  assert.equal(req.url, 'localhost:' + origin.address().port);
  assert.equal(req.headers['proxy-authorization'], 'Basic dXNlcjpwYXNz'); connects++;
  const upstream = net.connect(origin.address().port, '127.0.0.1', () => {
    client.write('HTTP/1.1 200 Connection Established\r\n\r\n');
    if (head.length) upstream.write(head);
    client.pipe(upstream); upstream.pipe(client);
  });
  client.on('error', () => upstream.destroy()); upstream.on('error', () => client.destroy());
  client.on('close', () => {
    upstream.destroy(); assert.equal(connects, 1); assert.equal(requests, 1);
    proxy.close(); origin.close(); clearTimeout(watchdog);
  });
});
origin.listen(0, '127.0.0.1', () => proxy.listen(0, '127.0.0.1', () => {
  console.log(origin.address().port + ' ' + proxy.address().port);
}));
"#;
    struct OwnedChild(std::process::Child);
    impl Drop for OwnedChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut node = OwnedChild(
        Command::new("node")
            .args(["-e", script])
            .env("KEY", cert.key_pair.serialize_pem())
            .env("CERT", cert.cert.pem())
            .stdout(Stdio::piped())
            .spawn()
            .expect("Node required"),
    );
    let mut ports = String::new();
    BufReader::new(node.0.stdout.take().expect("stdout"))
        .read_line(&mut ports)
        .expect("listener ports");
    let ports: Vec<u16> = ports
        .split_whitespace()
        .map(|p| p.parse().expect("port"))
        .collect();
    assert_eq!(ports.len(), 2);
    let mut executor = LocalExecutor::<Platform>::new(Config::default()).expect("executor");
    let h = executor.handle();
    let mut task = executor
        .spawn_local(async move {
            let tls = turnloop_tls::ClientConfig::new(
                turnloop_tls::ClientOptions {
                    ca: Some(vec![cert.cert.der().clone()]),
                    alpn: vec![b"http/1.1".to_vec()],
                    ..Default::default()
                },
                1_789_344_000,
            )
            .expect("trusted TLS config");
            let mut client = Client::new(
                h,
                tls,
                1_789_344_000,
                Options {
                    proxy: ProxyEnvironment {
                        https_proxy: Some(format!("http://user:pass@127.0.0.1:{}", ports[1])),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            );
            let mut req = Request::new(
                &format!("https://localhost:{}/through-proxy", ports[0]),
                "POST",
            )
            .expect("request");
            req.body = b"verified TLS upload".to_vec();
            let mut body = Vec::new();
            let head = client
                .request(&mut req, |bytes| {
                    body.extend_from_slice(bytes);
                    Ok(())
                })
                .await
                .expect("CONNECT and TLS request");
            assert_eq!(head.status, 200);
            assert_eq!(body, b"verified TLS response");
            body.len()
        })
        .expect("spawn");
    let end = executor.driver().now() + Duration::from_secs(10);
    while !task.is_finished() {
        assert!(executor.driver().now() < end);
        executor.turn(Timeout::Until(end)).expect("turn");
    }
    assert_eq!(finish(&mut task), 21);
    executor.turn(Timeout::Now).expect("close delivery");
    assert!(node.0.wait().expect("Node exit").success());
}

#[test]
fn cancelled_pooled_request_never_reuses_partial_response() {
    use std::cell::Cell;
    use turnloop_http::{
        asynchronous::{Http1, Http2},
        http1::Mode,
    };
    for h2 in [false, true] {
        let mut executor = LocalExecutor::<Platform>::new(Config::default()).expect("executor");
        let h = executor.handle();
        let listener = Listener::bind(&h, "127.0.0.1:0".parse().expect("address")).expect("listen");
        let address = listener.local_addr().expect("address");
        let mut server = executor
            .spawn_local(async move {
                for request in 0..2 {
                    let stream = listener.accept().await.expect("fresh accept");
                    if h2 {
                        let mut conn = Http2::new(stream, http2::Role::Server).expect("h2");
                        let mut heads = 0;
                        while conn
                            .event(|core, event| {
                                if let http2::Event::Headers { stream, .. } = event {
                                    heads += 1;
                                    core.send_headers(
                                        stream,
                                        &[Header::new(":status", "200")],
                                        request == 1,
                                    )
                                    .map_err(std::io::Error::other)?;
                                    if request == 0 {
                                        assert_eq!(
                                            core.send_data(stream, b"partial", false)
                                                .map_err(std::io::Error::other)?,
                                            7
                                        );
                                    }
                                }
                                Ok(())
                            })
                            .await
                            .expect("receive and peer EOF")
                        {}
                        assert_eq!(heads, 1);
                    } else {
                        let mut conn = Http1::new(stream, Mode::Request);
                        assert_eq!(conn.head().await.expect("head").method, "GET");
                        conn.event(|e| {
                            assert!(matches!(e, Event::End));
                            Ok(())
                        })
                        .await
                        .expect("request end");
                        conn.send_head(
                            &response(),
                            BodyLength::Known(if request == 0 { 100 } else { 0 }),
                        )
                        .await
                        .expect("response head");
                        if request == 0 {
                            conn.send_body(b"partial").await.expect("partial response");
                        } else {
                            conn.finish_body(&[]).await.expect("complete response");
                        }
                        let mut stream = conn.into_inner().expect("transport");
                        assert_eq!(read(&mut stream, &mut [0; 1]).await.expect("peer EOF"), 0);
                    }
                }
                2
            })
            .expect("spawn");
        let mut client = executor
            .spawn_local(async move {
                let tls = turnloop_tls::ClientConfig::new(Default::default(), 1_789_344_000)
                    .expect("TLS config");
                let mut client = Client::new(
                    h,
                    tls,
                    1_789_344_000,
                    Options {
                        http2_prior_knowledge: h2,
                        ..Default::default()
                    },
                );
                let mut request =
                    Request::new(&format!("http://{address}/"), "GET").expect("request");
                let bytes_seen = Cell::new(0);
                {
                    let mut pending = std::pin::pin!(client.request(&mut request, |bytes| {
                        assert_eq!(bytes, b"partial");
                        bytes_seen.set(bytes_seen.get() + bytes.len());
                        Ok(())
                    }));
                    std::future::poll_fn(|cx| {
                        assert!(
                            pending.as_mut().poll(cx).is_pending(),
                            "first response is deliberately unfinished"
                        );
                        if bytes_seen.get() == 7 {
                            Poll::Ready(())
                        } else {
                            Poll::Pending
                        }
                    })
                    .await;
                }
                assert_eq!(bytes_seen.get(), 7);
                assert!(
                    client.next_deadline().is_none(),
                    "cancelled lease must leave the pool"
                );
                assert_eq!(
                    client
                        .request(&mut request, |_| panic!("empty second body"))
                        .await
                        .expect("fresh connection")
                        .status,
                    200
                );
                1
            })
            .expect("spawn");
        let end = executor.driver().now() + Duration::from_secs(5);
        while !server.is_finished() || !client.is_finished() {
            assert!(executor.driver().now() < end);
            executor.turn(Timeout::Until(end)).expect("turn");
        }
        assert_eq!(finish(&mut client), 1);
        assert_eq!(finish(&mut server), 2);
    }
}

#[test]
fn graceful_goaway_drains_existing_pooled_request() {
    use turnloop_http::asynchronous::Http2;
    let mut executor = LocalExecutor::<Platform>::new(Config::default()).expect("executor");
    let h = executor.handle();
    let sh = h.clone();
    let listener = Listener::bind(&h, "127.0.0.1:0".parse().expect("address")).expect("listen");
    let address = listener.local_addr().expect("address");
    let mut server = executor
        .spawn_local(async move {
            let mut conn = Http2::new(
                listener.accept().await.expect("accept"),
                http2::Role::Server,
            )
            .expect("h2");
            let mut stream = None;
            while stream.is_none() {
                assert!(
                    conn.event(|_, event| {
                        if let http2::Event::Headers { stream: id, .. } = event {
                            stream = Some(id);
                        }
                        Ok(())
                    })
                    .await
                    .expect("headers")
                );
            }
            let stream = stream.expect("request stream");
            conn.core
                .send_headers(stream, &[Header::new(":status", "200")], false)
                .expect("response headers");
            assert_eq!(
                conn.core
                    .send_data(stream, b"before", false)
                    .expect("partial"),
                6
            );
            conn.shutdown().await.expect("GOAWAY flush");
            sh.sleep(Duration::from_millis(2))
                .await
                .expect("in-flight response delay");
            assert_eq!(
                conn.core
                    .send_data(stream, b"after", true)
                    .expect("finish existing stream"),
                5
            );
            conn.flush().await.expect("response flush");
            while !conn.core.is_drained() {
                assert!(
                    conn.event(|_, _| Ok(()))
                        .await
                        .expect("drain request END_STREAM")
                );
            }
            11
        })
        .expect("spawn");
    let mut client = executor
        .spawn_local(async move {
            let tls = turnloop_tls::ClientConfig::new(Default::default(), 1_789_344_000)
                .expect("TLS config");
            let mut client = Client::new(
                h,
                tls,
                1_789_344_000,
                Options {
                    http2_prior_knowledge: true,
                    ..Default::default()
                },
            );
            let mut request = Request::new(&format!("http://{address}/"), "GET").expect("request");
            let mut bytes = Vec::new();
            assert_eq!(
                client
                    .request(&mut request, |chunk| {
                        bytes.extend_from_slice(chunk);
                        Ok(())
                    })
                    .await
                    .expect("response survives GOAWAY")
                    .status,
                200
            );
            assert_eq!(bytes, b"beforeafter");
            assert!(
                client.next_deadline().is_none(),
                "drained connection cannot be reused"
            );
            bytes.len()
        })
        .expect("spawn");
    let end = executor.driver().now() + Duration::from_secs(5);
    while !server.is_finished() || !client.is_finished() {
        assert!(executor.driver().now() < end);
        executor.turn(Timeout::Until(end)).expect("turn");
    }
    assert_eq!(finish(&mut server), 11);
    assert_eq!(finish(&mut client), 11);
}

// Lingering close. A server that closes with unread peer bytes sends RST instead
// of FIN; the peer then reads ECONNRESET instead of EOF, and on macOS and Windows
// the reset can discard response bytes it has not read yet (issue #21). These
// tests hold every peer read until the server has finished its final close or
// half-close, and make the peer's late bytes unread at that moment, so they
// observe the reset deterministically rather than by timing luck.
const LINGER_BODY: usize = 32 * 1024;
const TLS_NOW: u64 = 1_789_344_000;
type Wire = turnloop_tls::asynchronous::Transport<AsyncIo<Platform>>;
fn linger_body() -> Vec<u8> {
    (0..LINGER_BODY).map(|i| (i % 251) as u8).collect()
}
/// Rendezvous between a server transport and its test peer. The server holds its
/// first close or half-close until the peer has sent bytes the server's protocol
/// never reads, so they sit unread in the server's receive buffer when it closes.
#[derive(Default)]
struct CloseGate {
    closing: std::cell::Cell<bool>,
    late_sent: std::cell::Cell<bool>,
    shut: std::cell::Cell<bool>,
    released: std::cell::Cell<bool>,
    discarded: std::cell::Cell<usize>,
    server: std::cell::RefCell<Option<Waker>>,
    peer: std::cell::RefCell<Option<Waker>>,
}
impl CloseGate {
    fn wake(slot: &std::cell::RefCell<Option<Waker>>) {
        if let Some(waker) = slot.borrow_mut().take() {
            waker.wake();
        }
    }
    async fn peer_until(&self, ready: impl Fn(&Self) -> bool) {
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
    fn late_bytes_sent(&self) {
        self.late_sent.set(true);
        Self::wake(&self.server);
    }
}
/// Server-side transport that applies the gate to both `poll_close` (a server
/// without lingering) and `poll_shutdown` (a lingering server), and counts bytes
/// read after the server's write side ended.
struct Gated<S> {
    inner: S,
    gate: std::rc::Rc<CloseGate>,
}
impl<S> Gated<S> {
    fn poll_gate(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if !self.gate.closing.replace(true) {
            CloseGate::wake(&self.gate.peer);
        }
        if self.gate.late_sent.get() {
            Poll::Ready(())
        } else {
            *self.gate.server.borrow_mut() = Some(cx.waker().clone());
            Poll::Pending
        }
    }
    fn ended_writes(&self, released: bool) {
        self.gate.shut.set(true);
        self.gate.released.set(self.gate.released.get() || released);
        CloseGate::wake(&self.gate.peer);
    }
}
impl<S: Stream> AsyncRead for Gated<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.inner).poll_read(cx, bytes);
        if let (true, Poll::Ready(Ok(n))) = (this.gate.shut.get(), &result) {
            this.gate.discarded.set(this.gate.discarded.get() + n);
        }
        result
    }
}
impl<S: Stream> AsyncWrite for Gated<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, bytes)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        std::task::ready!(this.poll_gate(cx));
        std::task::ready!(Pin::new(&mut this.inner).poll_close(cx))?;
        this.ended_writes(true);
        Poll::Ready(Ok(()))
    }
}
impl<S: HalfClose> HalfClose for Gated<S> {
    type Backend = S::Backend;
    fn executor(&self) -> Option<ExecutorHandle<S::Backend>> {
        self.inner.executor()
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        std::task::ready!(this.poll_gate(cx));
        std::task::ready!(Pin::new(&mut this.inner).poll_shutdown(cx))?;
        this.ended_writes(false);
        Poll::Ready(Ok(()))
    }
}
struct LingerPeer {
    server: Option<turnloop_tls::ServerConfig>,
    client: Option<turnloop_tls::ClientConfig>,
}
impl LingerPeer {
    fn new(tls: bool, alpn: &[u8]) -> Self {
        if !tls {
            return Self {
                server: None,
                client: None,
            };
        }
        use turnloop_tls::rustls::pki_types::PrivatePkcs8KeyDer;
        let cert =
            rcgen::generate_simple_self_signed(vec!["localhost".into()]).expect("certificate");
        Self {
            server: Some(
                turnloop_tls::ServerConfig::new(
                    vec![cert.cert.der().clone()],
                    PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der()).into(),
                    vec![alpn.to_vec()],
                    TLS_NOW,
                )
                .expect("server TLS config"),
            ),
            client: Some(
                turnloop_tls::ClientConfig::new(
                    turnloop_tls::ClientOptions {
                        ca: Some(vec![cert.cert.der().clone()]),
                        alpn: vec![alpn.to_vec()],
                        ..Default::default()
                    },
                    TLS_NOW,
                )
                .expect("client TLS config"),
            ),
        }
    }
}
async fn accept_wire(
    listener: &Listener<Platform>,
    tls: Option<turnloop_tls::ServerConfig>,
    h: &ExecutorHandle<Platform>,
    end: Instant,
) -> Wire {
    let stream = listener.accept().await.expect("accept");
    match tls {
        Some(config) => turnloop_tls::asynchronous::Transport::Tls(Box::new(
            turnloop_tls::TlsStream::accept(stream, &config, h, end, TLS_NOW)
                .await
                .expect("server handshake"),
        )),
        None => turnloop_tls::asynchronous::Transport::Plain(stream),
    }
}
async fn connect_wire(
    address: std::net::SocketAddr,
    tls: Option<turnloop_tls::ClientConfig>,
    h: &ExecutorHandle<Platform>,
    end: Instant,
) -> Wire {
    let stream = h
        .connect(address, Default::default())
        .await
        .expect("connect");
    match tls {
        Some(config) => turnloop_tls::asynchronous::Transport::Tls(Box::new(
            turnloop_tls::TlsStream::connect(
                stream,
                &config,
                "localhost".try_into().expect("server name"),
                h,
                end,
                TLS_NOW,
            )
            .await
            .expect("client handshake"),
        )),
        None => turnloop_tls::asynchronous::Transport::Plain(stream),
    }
}
/// Every byte until a clean EOF. `ECONNRESET` here is the bug under test.
async fn read_until_eof<S: Stream>(stream: &mut S) -> Vec<u8> {
    let mut received = Vec::new();
    let mut bytes = [0; 4096];
    loop {
        let n = read(stream, &mut bytes)
            .await
            .unwrap_or_else(|e| panic!("reset before EOF after {} bytes: {e}", received.len()));
        if n == 0 {
            return received;
        }
        received.extend_from_slice(&bytes[..n]);
    }
}
fn run_until<T, U>(
    executor: &mut LocalExecutor<Platform>,
    a: &mut turnloop::executor::JoinHandle<T>,
    b: &mut turnloop::executor::JoinHandle<U>,
) {
    let end = executor.driver().now() + Duration::from_secs(10);
    while !a.is_finished() || !b.is_finished() {
        assert!(executor.driver().now() < end, "lingering exchange hung");
        executor.turn(Timeout::Until(end)).expect("turn");
    }
}

const PIPELINED: &[u8] = b"GET /pipelined HTTP/1.1\r\nhost: localhost\r\n\r\n";
fn http1_final_response_survives_pipelined_request(tls: bool) {
    let mut executor = LocalExecutor::<Platform>::new(Config::default()).expect("executor");
    let h = executor.handle();
    let sh = h.clone();
    let listener = Listener::bind(&h, "127.0.0.1:0".parse().expect("address")).expect("listen");
    let address = listener.local_addr().expect("address");
    let peers = LingerPeer::new(tls, b"http/1.1");
    let (server_tls, client_tls) = (peers.server, peers.client);
    let gate = std::rc::Rc::new(CloseGate::default());
    let server_gate = gate.clone();
    let end = h.now() + Duration::from_secs(10);
    let mut server = executor
        .spawn_local(async move {
            let wire = accept_wire(&listener, server_tls, &sh, end).await;
            let body = linger_body();
            let mut heads = 0;
            let gated = Gated {
                inner: wire,
                gate: server_gate,
            };
            server::http1(gated, server::Shutdown::default(), |event, out| {
                match event {
                    Event::Head(head) => {
                        assert_eq!(head.target, "/first", "pipelined request must stay unread");
                        heads += 1;
                        let head = Head {
                            keep_alive: false,
                            ..response()
                        };
                        out.start(&head, BodyLength::Known(LINGER_BODY as u64))?;
                        out.body(&body)?;
                    }
                    Event::End => out.finish(&[])?,
                    _ => {}
                }
                Ok(())
            })
            .await
            .expect("final response and lingering close");
            heads
        })
        .expect("spawn");
    let mut client = executor
        .spawn_local(async move {
            let mut wire = connect_wire(address, client_tls, &h, end).await;
            write_all(&mut wire, b"GET /first HTTP/1.1\r\nhost: localhost\r\n\r\n")
                .await
                .expect("request");
            // Read nothing until the server finished writing and began to close.
            gate.peer_until(|g| g.closing.get()).await;
            write_all(&mut wire, PIPELINED)
                .await
                .expect("pipelined request");
            gate.late_bytes_sent();
            gate.peer_until(|g| g.shut.get()).await;
            let response = read_until_eof(&mut wire).await;
            // TLS: close_notify is a write, which fails on a connection already reset.
            close(&mut wire)
                .await
                .expect("the server is still open for our close");
            (response, gate)
        })
        .expect("spawn");
    run_until(&mut executor, &mut server, &mut client);
    let (response, gate) = finish(&mut client);
    assert_eq!(finish(&mut server), 1);
    let head_end = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("response head")
        + 4;
    let head = std::str::from_utf8(&response[..head_end]).expect("ASCII head");
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert!(head.contains("connection: close"), "{head}");
    assert_eq!(
        &response[head_end..],
        linger_body(),
        "every body byte arrives"
    );
    assert_eq!(
        gate.discarded.get(),
        PIPELINED.len(),
        "server drained the pipelined request"
    );
    assert!(gate.released.get(), "server closed after its peer's EOF");
}
#[test]
fn http1_lingering_close_delivers_response_before_pipelined_request() {
    http1_final_response_survives_pipelined_request(false);
}
#[test]
fn https1_lingering_close_delivers_response_before_pipelined_request() {
    http1_final_response_survives_pipelined_request(true);
}

/// SETTINGS acknowledgement, connection WINDOW_UPDATE and PING: frames an HTTP/2
/// client commonly sends after the server's final response and GOAWAY.
const LATE_H2_FRAMES: &[u8] = &[
    0, 0, 0, 4, 1, 0, 0, 0, 0, // SETTINGS ACK
    0, 0, 4, 8, 0, 0, 0, 0, 0, 0, 1, 0, 0, // WINDOW_UPDATE stream 0, +65536
    0, 0, 8, 6, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, // PING
];
fn http2_final_response_survives_unread_frames(tls: bool) {
    let mut executor = LocalExecutor::<Platform>::new(Config::default()).expect("executor");
    let h = executor.handle();
    let sh = h.clone();
    let listener = Listener::bind(&h, "127.0.0.1:0".parse().expect("address")).expect("listen");
    let address = listener.local_addr().expect("address");
    let peers = LingerPeer::new(tls, b"h2");
    let (server_tls, client_tls) = (peers.server, peers.client);
    let gate = std::rc::Rc::new(CloseGate::default());
    let server_gate = gate.clone();
    let end = h.now() + Duration::from_secs(10);
    let mut server = executor
        .spawn_local(async move {
            let wire = accept_wire(&listener, server_tls, &sh, end).await;
            let body = linger_body();
            let signal = server::Shutdown::default();
            let stop = signal.clone();
            let mut responses = 0;
            let gated = Gated {
                inner: wire,
                gate: server_gate,
            };
            server::http2(gated, signal, |core, event| {
                if let http2::Event::Headers {
                    stream, end_stream, ..
                } = event
                {
                    assert!(end_stream);
                    let length = LINGER_BODY.to_string();
                    core.send_headers(
                        stream,
                        &[
                            Header::new(":status", "200"),
                            Header::new("content-length", length),
                        ],
                        false,
                    )
                    .map_err(std::io::Error::other)?;
                    let mut sent = 0;
                    while sent < body.len() {
                        let n = core
                            .send_data(stream, &body[sent..], true)
                            .map_err(std::io::Error::other)?;
                        assert!(n > 0, "initial flow-control window covers the body");
                        sent += n;
                    }
                    responses += 1;
                    // GOAWAY, drain, then close: the final response of this connection.
                    stop.stop();
                }
                Ok(())
            })
            .await
            .expect("final response and lingering close");
            responses
        })
        .expect("spawn");
    let mut client = executor
        .spawn_local(async move {
            let mut wire = connect_wire(address, client_tls, &h, end).await;
            let mut core =
                http2::Connection::new(http2::Role::Client, Default::default()).expect("h2");
            core.open(
                &[
                    Header::new(":method", "GET"),
                    Header::new(":scheme", if tls { "https" } else { "http" }),
                    Header::new(":authority", "localhost"),
                    Header::new(":path", "/"),
                ],
                true,
            )
            .expect("request");
            drain(&mut wire, &mut core)
                .await
                .expect("preface and request");
            // Read nothing (not even SETTINGS) until the server drained and closes.
            gate.peer_until(|g| g.closing.get()).await;
            write_all(&mut wire, LATE_H2_FRAMES)
                .await
                .expect("late control frames");
            gate.late_bytes_sent();
            gate.peer_until(|g| g.shut.get()).await;
            let wire_bytes = read_until_eof(&mut wire).await;
            close(&mut wire)
                .await
                .expect("the server is still open for our close");
            let (mut status, mut body, mut goaway, mut ended) = (None, Vec::new(), None, false);
            let mut pos = 0;
            while pos < wire_bytes.len() {
                let step = core.receive(&wire_bytes[pos..]).expect("server frames");
                assert!(step.consumed > 0 || step.event.is_some(), "truncated frame");
                pos += step.consumed;
                match step.event {
                    Some(http2::Event::Headers { headers, .. }) => {
                        status = headers
                            .into_iter()
                            .find(|h| h.name == ":status")
                            .map(|h| h.value);
                    }
                    Some(http2::Event::Data {
                        bytes, end_stream, ..
                    }) => {
                        body.extend_from_slice(bytes);
                        ended |= end_stream;
                    }
                    Some(http2::Event::Goaway { code, .. }) => goaway = Some(code),
                    _ => {}
                }
            }
            assert_eq!(status.as_deref(), Some(b"200".as_slice()));
            assert_eq!(goaway, Some(0), "graceful GOAWAY precedes EOF");
            assert!(ended, "END_STREAM arrives");
            (body, gate)
        })
        .expect("spawn");
    run_until(&mut executor, &mut server, &mut client);
    let (body, gate) = finish(&mut client);
    assert_eq!(finish(&mut server), 1);
    assert_eq!(body, linger_body(), "every DATA byte arrives");
    assert_eq!(
        gate.discarded.get(),
        LATE_H2_FRAMES.len(),
        "server drained late frames"
    );
    assert!(gate.released.get(), "server closed after its peer's EOF");
}
#[test]
fn http2_lingering_close_delivers_response_despite_unread_frames() {
    http2_final_response_survives_unread_frames(false);
}
#[test]
fn https2_lingering_close_delivers_response_despite_unread_frames() {
    http2_final_response_survives_unread_frames(true);
}

/// A peer that reads the final response but never closes: the server closes at
/// its linger deadline (or an earlier `stop_by` deadline) and never spins.
fn linger_deadline_with_silent_peer(options: server::Options, stop_by: Option<Duration>) {
    let mut executor = LocalExecutor::<Platform>::new(Config::default()).expect("executor");
    let h = executor.handle();
    let sh = h.clone();
    let listener = Listener::bind(&h, "127.0.0.1:0".parse().expect("address")).expect("listen");
    let address = listener.local_addr().expect("address");
    let gate = std::rc::Rc::new(CloseGate::default());
    // Nothing late to send: the half-close proceeds at once.
    gate.late_sent.set(true);
    let server_gate = gate.clone();
    let signal = server::Shutdown::with_options(options);
    assert_eq!(signal.options(), options);
    let service_signal = signal.clone();
    let mut server = executor
        .spawn_local(async move {
            let stream = listener.accept().await.expect("accept");
            let gated = Gated {
                inner: stream,
                gate: server_gate,
            };
            server::http1(gated, service_signal, |event, out| {
                match event {
                    Event::Head(_) => out.start(
                        &Head {
                            keep_alive: false,
                            ..response()
                        },
                        BodyLength::Known(4),
                    )?,
                    Event::End => {
                        out.body(b"done")?;
                        out.finish(&[])?;
                    }
                    _ => {}
                }
                Ok(())
            })
            .await
            .expect("lingering close");
            sh.now()
        })
        .expect("spawn");
    let mut client = executor
        .spawn_local(async move {
            let mut stream = h
                .connect(address, Default::default())
                .await
                .expect("connect");
            let sent = h.now();
            write_all(&mut stream, b"GET / HTTP/1.1\r\nhost: localhost\r\n\r\n")
                .await
                .expect("request");
            let response = read_until_eof(&mut stream).await;
            assert!(response.ends_with(b"done"));
            // Keep the connection open and silent.
            (stream, sent)
        })
        .expect("spawn");
    let end = executor.driver().now() + Duration::from_secs(10);
    while !client.is_finished() {
        assert!(executor.driver().now() < end);
        executor.turn(Timeout::Until(end)).expect("turn");
    }
    let (_silent_peer, sent) = finish(&mut client);
    // The peer's EOF is the server's half-close, but the server may observe its
    // own shutdown acknowledgement later: WASI 0.3 closes the send stream (the
    // peer reads EOF) and completes Shutdown only once that stream's result
    // future resolves in a later wait-set step. Deliver it before measuring.
    while !gate.shut.get() {
        assert!(!server.is_finished(), "server ended before its half-close");
        assert!(executor.driver().now() < end);
        executor.turn(Timeout::Until(end)).expect("turn");
    }
    for _ in 0..3 {
        executor.turn(Timeout::Now).expect("settle queued events");
    }
    assert!(!server.is_finished(), "closed before the linger deadline");
    let mut floor = sent + options.linger_timeout;
    // One turn waits for the single expiry, with at most one zero-event wait.
    let (mut turn_limit, mut empty_limit) = (2, 1);
    if let Some(delay) = stop_by {
        let at = executor.driver().now() + delay;
        signal.stop_by(at);
        assert_eq!(signal.deadline(), Some(at));
        // A later deadline never extends an earlier one.
        signal.stop_by(at + Duration::from_secs(60));
        assert_eq!(signal.deadline(), Some(at));
        floor = at;
        // The deadline change woke exactly the lingering service; polling it
        // re-arms its timer. The wait below is then the same single expiry.
        assert_eq!(executor.run_ready(), 1, "stop_by must wake the linger");
        assert!(!server.is_finished());
        // Replacing the timer queues its Cancelled/Closed completions beside the
        // pending read: one nonblocking turn with at most one empty discovery poll
        // (DESIGN section 10, rule 3) delivers them before the single expiry wait.
        (turn_limit, empty_limit) = (3, 2);
    }
    let (mut turns, mut empty) = (0, 0);
    while !server.is_finished() {
        assert!(turns < turn_limit, "lingering close spun");
        let info = executor.turn(Timeout::Until(end)).expect("turn");
        turns += 1;
        empty += info.zero_event_waits;
    }
    assert!(
        empty <= empty_limit,
        "zero-event waits while lingering: {empty}"
    );
    let closed_at = finish(&mut server);
    assert!(closed_at >= floor, "closed before the deadline");
    assert!(gate.released.get(), "deadline closed the socket");
    assert_eq!(gate.discarded.get(), 0);
}
#[test]
fn http_linger_timeout_closes_silent_peer_without_spinning() {
    linger_deadline_with_silent_peer(
        server::Options {
            linger_timeout: Duration::from_millis(500),
        },
        None,
    );
}
#[test]
fn http_stop_by_deadline_ends_lingering_connection() {
    // The linger timeout alone would hold the connection for a minute.
    linger_deadline_with_silent_peer(
        server::Options {
            linger_timeout: Duration::from_secs(60),
        },
        Some(Duration::from_millis(100)),
    );
}

#[test]
fn http1_idle_keepalive_lingers_at_shutdown() {
    let mut executor = LocalExecutor::<Platform>::new(Config::default()).expect("executor");
    let h = executor.handle();
    let listener = Listener::bind(&h, "127.0.0.1:0".parse().expect("address")).expect("listen");
    let address = listener.local_addr().expect("address");
    let gate = std::rc::Rc::new(CloseGate::default());
    let server_gate = gate.clone();
    let signal = server::Shutdown::default();
    let stop = signal.clone();
    let mut server = executor
        .spawn_local(async move {
            let stream = listener.accept().await.expect("accept");
            let gated = Gated {
                inner: stream,
                gate: server_gate,
            };
            let mut heads = 0;
            server::http1(gated, signal, |event, out| {
                match event {
                    Event::Head(_) => {
                        heads += 1;
                        out.start(&response(), BodyLength::Known(4))?;
                    }
                    Event::End => {
                        out.body(b"idle")?;
                        out.finish(&[])?;
                    }
                    _ => {}
                }
                Ok(())
            })
            .await
            .expect("idle connection closes gracefully at shutdown");
            heads
        })
        .expect("spawn");
    let mut client = executor
        .spawn_local(async move {
            let mut stream = h
                .connect(address, Default::default())
                .await
                .expect("connect");
            write_all(&mut stream, b"GET / HTTP/1.1\r\nhost: localhost\r\n\r\n")
                .await
                .expect("request");
            let mut response = Vec::new();
            let mut bytes = [0; 256];
            while !response.ends_with(b"\r\n\r\nidle") {
                let n = read(&mut stream, &mut bytes).await.expect("response");
                assert!(n > 0, "keep-alive response must not close");
                response.extend_from_slice(&bytes[..n]);
            }
            // The connection is idle between requests when shutdown arrives.
            stop.stop();
            gate.peer_until(|g| g.closing.get()).await;
            write_all(&mut stream, PIPELINED)
                .await
                .expect("request racing the shutdown");
            gate.late_bytes_sent();
            gate.peer_until(|g| g.shut.get()).await;
            assert!(
                read_until_eof(&mut stream).await.is_empty(),
                "a request that raced shutdown is never answered"
            );
            close(&mut stream).await.expect("close");
            gate
        })
        .expect("spawn");
    run_until(&mut executor, &mut server, &mut client);
    let gate = finish(&mut client);
    assert_eq!(finish(&mut server), 1);
    assert_eq!(gate.discarded.get(), PIPELINED.len());
    assert!(gate.released.get());
}

/// PerryTS/turnloop#46: an upgrade request reaches the service as
/// `Event::Upgrade`, and the service decides. Declined, the connection stays
/// HTTP/1 and serves the next request. Accepted, the driver closes rather than
/// parse the next protocol's bytes as a request.
#[test]
fn http1_server_upgrade_request_is_the_services_decision() {
    let mut executor = LocalExecutor::<Platform>::new(Config::default()).expect("executor");
    let h = executor.handle();
    let listener = Listener::bind(&h, "127.0.0.1:0".parse().expect("address")).expect("listen");
    let address = listener.local_addr().expect("address");
    let mut server = executor
        .spawn_local(async move {
            let mut events = Vec::new();
            for _ in 0..2 {
                let stream = listener.accept().await.expect("accept");
                server::http1(stream, server::Shutdown::default(), |event, out| {
                    match event {
                        Event::Head(head) => {
                            events.push(format!("head {}", head.target));
                            if head.target == "/accept" {
                                let head = Head {
                                    status: 101,
                                    headers: vec![
                                        Header::new("connection", "upgrade"),
                                        Header::new("upgrade", "test"),
                                    ],
                                    ..response()
                                };
                                out.start(&head, BodyLength::Empty)?;
                            } else {
                                let body = head.target.as_bytes();
                                out.start(&response(), BodyLength::Known(body.len() as u64))?;
                                out.body(body)?;
                            }
                        }
                        Event::Upgrade => {
                            events.push("upgrade".into());
                            out.finish(&[])?;
                        }
                        Event::End => {
                            events.push("end".into());
                            out.finish(&[])?;
                        }
                        _ => {}
                    }
                    Ok(())
                })
                .await
                .expect("the connection ends cleanly");
            }
            events
        })
        .expect("spawn");
    let mut client = executor
        .spawn_local(async move {
            let mut responses = Vec::new();
            for wire in [
                b"GET /decline HTTP/1.1\r\nhost: l\r\nconnection: upgrade\r\nupgrade: test\r\n\r\n\
                  GET /next HTTP/1.1\r\nhost: l\r\nconnection: close\r\n\r\n"
                    .as_slice(),
                b"GET /accept HTTP/1.1\r\nhost: l\r\nconnection: upgrade\r\nupgrade: test\r\n\r\n\
                  not HTTP at all\r\n\r\n",
            ] {
                let mut stream = h
                    .connect(address, Default::default())
                    .await
                    .expect("connect");
                write_all(&mut stream, wire).await.expect("request");
                responses.push(read_until_eof(&mut stream).await);
                close(&mut stream).await.expect("close");
            }
            responses
        })
        .expect("spawn");
    run_until(&mut executor, &mut server, &mut client);
    let responses = finish(&mut client);
    assert_eq!(
        finish(&mut server),
        [
            "head /decline",
            "upgrade",
            "head /next",
            "end",
            "head /accept",
            "upgrade"
        ]
    );
    let declined = String::from_utf8(responses[0].clone()).expect("ASCII");
    assert!(declined.starts_with("HTTP/1.1 200"), "{declined}");
    assert!(
        declined.contains("\r\n\r\n/declineHTTP/1.1 200") && declined.ends_with("\r\n\r\n/next"),
        "{declined}"
    );
    let accepted = String::from_utf8(responses[1].clone()).expect("ASCII");
    assert!(accepted.starts_with("HTTP/1.1 101"), "{accepted}");
    assert!(
        accepted.ends_with("\r\n\r\n"),
        "nothing after the switch: {accepted}"
    );
}
