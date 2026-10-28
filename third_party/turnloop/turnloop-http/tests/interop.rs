#![cfg(not(target_arch = "wasm32"))]
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::Command,
    thread,
    time::{Duration, Instant},
};
use turnloop_http::{
    client::{RedirectMode, Request},
    http1::{self, Head, Header},
    http2,
};
#[path = "support/tls.rs"]
mod tls_support;
fn curl() -> Command {
    let executable = std::env::var_os("TURNLOOP_TEST_CURL").unwrap_or_else(|| "curl".into());
    let mut command = Command::new(executable);
    // Explicit child PATH takes precedence over System32 on Windows. CI uses an
    // absolute override because Git Bash can prepend its own curl to that PATH.
    command.env("PATH", std::env::var_os("PATH").expect("PATH"));
    command
}
// Probe the same executable used by the tests. Windows' bundled curl commonly
// has HTTP/1 support but no HTTP2, regardless of its version number.
fn curl_capability(version: &str, field: &str, capability: &str) -> bool {
    version.lines().any(|line| {
        line.strip_prefix(field)
            .is_some_and(|values| values.split_whitespace().any(|value| value == capability))
    })
}
fn curl_supports(http2: bool) -> bool {
    match curl().arg("-V").output() {
        Ok(output) => {
            assert!(
                output.status.success(),
                "curl -V failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let version = String::from_utf8(output.stdout).expect("curl -V must emit UTF-8");
            let supported = curl_capability(&version, "Protocols:", "http")
                && (!http2 || curl_capability(&version, "Features:", "HTTP2"));
            if std::env::var_os("TURNLOOP_TEST_CURL").is_some() {
                assert!(
                    supported,
                    "explicit CI curl lacks required capabilities: {version}"
                );
            }
            supported
        }
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && std::env::var_os("TURNLOOP_TEST_CURL").is_none() =>
        {
            false
        }
        Err(error) => panic!("cannot probe curl: {error}"),
    }
}
#[test]
fn curl_features_are_tokens_in_the_features_line() {
    let windows = "curl 8.13.0 (Windows) libcurl/8.13.0 Schannel\r\nProtocols: http https\r\nFeatures: HTTPS-proxy SSL threadsafe\r\n";
    assert!(curl_capability(windows, "Protocols:", "http"));
    assert!(!curl_capability(windows, "Features:", "HTTP2"));
    assert!(curl_capability(
        "Features: SSL HTTP2 HTTP3\n",
        "Features:",
        "HTTP2"
    ));
    assert!(!curl_capability(
        "curl HTTP2\nFeatures: NOHTTP2 HTTP2-extra\n",
        "Features:",
        "HTTP2"
    ));
}
fn node_port(mode: &str) -> u16 {
    let key = if mode == "h1" {
        "TURNLOOP_TEST_HTTP_PORT"
    } else {
        "TURNLOOP_TEST_HTTP2_PORT"
    };
    std::env::var(key)
        .expect("run through scripts/test-servers.py --services http run")
        .parse()
        .expect("fixture port must be a u16")
}
fn socket(port: u16) -> TcpStream {
    let s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.set_write_timeout(Some(Duration::from_secs(10))).unwrap();
    s
}
fn accept(listener: TcpListener) -> TcpStream {
    listener.set_nonblocking(true).unwrap();
    let end = Instant::now() + Duration::from_secs(15);
    loop {
        match listener.accept() {
            Ok((s, _)) => {
                s.set_nonblocking(false).unwrap();
                s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
                return s;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < end, "client did not connect");
                thread::sleep(Duration::from_millis(5));
            }
            Err(e) => panic!("{e}"),
        }
    }
}
fn response(stream: &mut impl Read, decoder: &mut http1::Decoder) -> (Head, Vec<u8>, Vec<Header>) {
    let mut buffer = Vec::new();
    let mut body = Vec::new();
    let mut head = None;
    let mut trailers = Vec::new();
    loop {
        let step = decoder.receive(&buffer).unwrap();
        let progressed = step.consumed > 0 || step.event.is_some();
        let end = matches!(step.event, Some(http1::Event::End | http1::Event::Upgrade));
        match step.event {
            Some(http1::Event::Head(h)) => head = Some(h),
            Some(http1::Event::Body(bytes)) => body.extend_from_slice(bytes),
            Some(http1::Event::Trailers(t)) => trailers = t,
            _ => {}
        }
        buffer.drain(..step.consumed);
        if end {
            return (head.unwrap(), body, trailers);
        }
        if !progressed {
            let mut bytes = [0; 37];
            let n = stream.read(&mut bytes).unwrap();
            if n == 0 {
                decoder.eof().unwrap();
            } else {
                buffer.extend_from_slice(&bytes[..n]);
            }
        }
    }
}
fn send(stream: &mut impl Write, request: &Request) {
    let mut bytes = Vec::new();
    let mut encoder = http1::Encoder::start(
        &request.head(false),
        http1::BodyLength::Known(request.body.len() as u64),
        &mut bytes,
    )
    .unwrap();
    encoder.body(&request.body, &mut bytes).unwrap();
    encoder.finish(&[], &mut bytes).unwrap();
    stream.write_all(&bytes).unwrap();
}
#[test]
#[ignore = "requires the private HTTP fixture from scripts/test-servers.py"]
fn node_h1_redirect_compression_trailers_and_socket_reuse() {
    let port = node_port("h1");
    let mut socket = socket(port);
    let mut decoder = http1::Decoder::new(http1::Mode::Response, Default::default());
    let mut request = Request::new(&format!("http://127.0.0.1:{port}/redirect"), "GET").unwrap();
    send(&mut socket, &request);
    let (head, body, _) = response(&mut socket, &mut decoder);
    assert!(body.is_empty());
    assert!(
        request
            .redirect(
                head.status,
                head.get("location")
                    .map(|x| std::str::from_utf8(x).unwrap()),
                RedirectMode::Follow,
                20
            )
            .unwrap()
    );
    decoder.reset().unwrap();
    send(&mut socket, &request);
    let (head, body, _) = response(&mut socket, &mut decoder);
    assert_eq!(head.status, 200);
    let mut decoded = Vec::new();
    turnloop_http::compression::decode("gzip", &body, &mut decoded, 100).unwrap();
    assert_eq!(decoded, b"compressed from node");
    decoder.reset().unwrap();
    request.url.set_path("/trailers");
    send(&mut socket, &request);
    let (_, body, trailers) = response(&mut socket, &mut decoder);
    assert_eq!(body, b"chunk-onechunk-two");
    assert_eq!(trailers, [Header::new("x-check", "verified")]);
    // The Node fixture is shared by every test binary in a run and numbers
    // connections globally, so assert reuse (same socket for both requests on
    // this connection) rather than an absolute connection number.
    let mut reused: Option<Vec<u8>> = None;
    for _ in 0..2 {
        decoder.reset().unwrap();
        request.url.set_path("/reuse");
        send(&mut socket, &request);
        let (head, body, _) = response(&mut socket, &mut decoder);
        let id = head
            .get("x-socket")
            .expect("fixture reports its socket")
            .to_vec();
        match &reused {
            None => reused = Some(id),
            Some(first) => assert_eq!(&id, first, "keep-alive request used a different socket"),
        }
        assert_eq!(body, b"GET /reuse ");
    }
}
fn serve_h1(mut stream: impl Read + Write) {
    let mut decoder = http1::Decoder::new(http1::Mode::Request, Default::default());
    let (head, body, _) = response(&mut stream, &mut decoder);
    assert_eq!(head.target, "/interop");
    assert!(body.is_empty());
    let reply = Head {
        method: String::new(),
        target: String::new(),
        status: 200,
        version: 1,
        headers: vec![Header::new("connection", "close")],
        keep_alive: false,
    };
    let mut wire = Vec::new();
    let mut encoder = http1::Encoder::start(&reply, http1::BodyLength::Chunked, &mut wire).unwrap();
    encoder.body(b"native-http", &mut wire).unwrap();
    encoder
        .finish(&[Header::new("x-check", "native")], &mut wire)
        .unwrap();
    stream.write_all(&wire).unwrap();
}
#[test]
fn curl_and_node_fetch_against_native_http1() {
    let mut ran = 0;
    for node in [true, false] {
        if !node && !curl_supports(false) {
            eprintln!("HTTP/1 curl leg UNRUN: curl -V does not list the http protocol");
            continue;
        }
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || serve_h1(accept(listener)));
        let url = format!("http://{address}/interop");
        let output = if node {
            Command::new("node").args(["--input-type=module","-e","setTimeout(()=>process.exit(70),10000).unref();const r=await fetch(process.argv[1]);if(r.status!==200)process.exit(2);console.log(await r.text());",&url]).output().expect("Node fetch must run")
        } else {
            curl()
                .args([
                    "--silent",
                    "--show-error",
                    "--max-time",
                    "10",
                    "--noproxy",
                    "*",
                    &url,
                ])
                .output()
                .unwrap()
        };
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            "native-http"
        );
        server
            .join()
            .expect("HTTP/1 server must verify the request");
        ran += 1;
        eprintln!(
            "HTTP/1 interop ran: {}",
            if node { "Node fetch" } else { "curl" }
        );
    }
    assert!(ran > 0, "neither HTTP/1 interop client ran");
}
#[test]
fn https_over_unbuffered_tls() {
    let cert = tls_support::certificate();
    let config = tls_support::server_config(&cert);
    let client = turnloop_tls::ClientConfig::new(
        turnloop_tls::ClientOptions {
            ca: Some(vec![cert.cert.der().clone()]),
            alpn: vec![b"http/1.1".to_vec()],
            ..Default::default()
        },
        tls_support::NOW,
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        serve_h1(tls_support::Stream::new(
            config.accept().unwrap(),
            accept(listener),
        ))
    });
    let mut stream = tls_support::Stream::new(
        client
            .connect(turnloop_tls::rustls::pki_types::ServerName::try_from("localhost").unwrap())
            .unwrap(),
        socket(port),
    );
    send(
        &mut stream,
        &Request::new("https://localhost/interop", "GET").unwrap(),
    );
    let (head, body, trailers) = response(
        &mut stream,
        &mut http1::Decoder::new(http1::Mode::Response, Default::default()),
    );
    assert_eq!(head.status, 200);
    assert_eq!(body, b"native-http");
    assert_eq!(trailers, [Header::new("x-check", "native")]);
    server.join().unwrap();
}
fn flush_h2(engine: &mut http2::Connection, socket: &mut TcpStream) {
    socket.write_all(engine.output()).unwrap();
    let n = engine.output().len();
    engine.consume_output(n).unwrap();
}
fn serve_h2(mut socket: TcpStream, total: usize) {
    let mut engine = http2::Connection::new(http2::Role::Server, Default::default()).unwrap();
    let mut input = Vec::new();
    let mut count = 0;
    flush_h2(&mut engine, &mut socket);
    while count < total {
        let step = engine.receive(&input).unwrap();
        let consumed = step.consumed;
        if let Some(http2::Event::Headers {
            stream,
            headers,
            end_stream,
            ..
        }) = step.event
        {
            assert!(end_stream);
            assert!(
                headers
                    .iter()
                    .any(|h| h.name == ":method" && h.value == b"GET")
            );
            engine
                .send_headers(
                    stream,
                    &[
                        Header::new(":status", "200"),
                        Header::new("content-length", "9"),
                    ],
                    false,
                )
                .unwrap();
            assert_eq!(engine.send_data(stream, b"native-h2", true).unwrap(), 9);
            count += 1;
        }
        input.drain(..consumed);
        flush_h2(&mut engine, &mut socket);
        if consumed == 0 {
            let mut bytes = [0; 1024];
            let n = socket.read(&mut bytes).unwrap();
            assert!(n > 0, "EOF after {count}/{total}");
            input.extend_from_slice(&bytes[..n]);
        }
    }
    assert_eq!(count, total);
    engine.shutdown().unwrap();
    flush_h2(&mut engine, &mut socket);
    socket.shutdown(std::net::Shutdown::Write).unwrap();
    let mut tail = [0; 1024];
    loop {
        match socket.read(&mut tail) {
            Ok(0) => break,
            Ok(_) => {}
            // This is a raw std socket, so it does not get the normalisation the
            // crate applies everywhere else: Windows reports the local end of a
            // departed peer as WSAECONNABORTED, and `backend/iocp/socket.rs`
            // (`ERROR_CONNECTION_ABORTED`) and `types.rs` both fold that into
            // `ConnectionReset`. Both mean the peer is gone after a verified
            // exchange, which is a valid end to this drain.
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
                ) =>
            {
                break;
            }
            Err(e) => panic!("peer did not finish shutdown: {e}"),
        }
    }
}
#[test]
fn curl_and_node_h2_hundred_streams_against_native_server() {
    run_h2_clients(|| curl_supports(true));
}
#[test]
fn node_h2_hundred_streams_runs_without_curl_http2() {
    // Exercise the Windows capability branch on every runner with a real Node peer.
    assert_eq!(run_h2_clients(|| false), 1);
}
fn run_h2_clients(curl_http2: impl Fn() -> bool) -> usize {
    let mut ran = 0;
    for node in [true, false] {
        if !node && !curl_http2() {
            eprintln!("HTTP/2 curl leg UNRUN: curl -V does not list HTTP2 with the http protocol");
            continue;
        }
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || serve_h2(accept(listener), if node { 100 } else { 1 }));
        let url = format!("http://{address}/interop");
        let output = if node {
            Command::new("node").args(["--input-type=module","-e",r#"import h2 from 'node:http2'; setTimeout(()=>{console.error('client timeout');process.exit(70);},10000).unref(); const c=h2.connect(process.argv[1]); await Promise.all(Array.from({length:100},()=>new Promise((resolve,reject)=>{const s=c.request({':path':'/interop'});let b='';s.on('response',h=>{if(h[':status']!==200)reject(Error('status'));});s.on('data',x=>b+=x);s.on('end',()=>b==='native-h2'?resolve():reject(Error(b)));s.on('error',reject);s.on('aborted',()=>reject(Error('stream aborted')));s.end();})));c.close();console.log('100 verified');"#,&url]).output().unwrap()
        } else {
            curl()
                .args([
                    "--http2-prior-knowledge",
                    "--silent",
                    "--show-error",
                    "--max-time",
                    "10",
                    "--noproxy",
                    "*",
                    &url,
                ])
                .output()
                .unwrap()
        };
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            if node { "100 verified" } else { "native-h2" }
        );
        server
            .join()
            .expect("HTTP/2 server must verify every stream");
        ran += 1;
        eprintln!(
            "HTTP/2 interop ran: {}",
            if node {
                "Node http2 (100 streams)"
            } else {
                "curl (1 stream)"
            }
        );
    }
    assert!(ran > 0, "neither HTTP/2 interop client ran");
    ran
}
#[test]
#[ignore = "requires the private HTTP fixture from scripts/test-servers.py"]
fn native_h2_client_against_node_hundred_streams() {
    let port = node_port("h2");
    let mut socket = socket(port);
    let mut engine = http2::Connection::new(http2::Role::Client, Default::default()).unwrap();
    for _ in 0..100 {
        engine
            .open(
                &[
                    Header::new(":method", "GET"),
                    Header::new(":scheme", "http"),
                    Header::new(":path", "/interop"),
                    Header::new(":authority", "localhost"),
                ],
                true,
            )
            .unwrap();
    }
    flush_h2(&mut engine, &mut socket);
    let mut input = Vec::new();
    let mut ended = 0;
    let mut bodies = vec![Vec::new(); 100];
    while ended < 100 {
        let step = engine.receive(&input).unwrap();
        let consumed = step.consumed;
        match step.event {
            Some(http2::Event::Headers { headers, .. }) => assert!(
                headers
                    .iter()
                    .any(|h| h.name == ":status" && h.value == b"200")
            ),
            Some(http2::Event::Data {
                stream,
                bytes,
                end_stream,
            }) => {
                bodies[(stream / 2) as usize].extend_from_slice(bytes);
                engine.release_capacity(stream, bytes.len() as u32).unwrap();
                if end_stream {
                    ended += 1;
                }
            }
            _ => {}
        }
        input.drain(..consumed);
        flush_h2(&mut engine, &mut socket);
        if consumed == 0 {
            let mut bytes = [0; 4096];
            let n = socket.read(&mut bytes).unwrap();
            assert!(n > 0);
            input.extend_from_slice(&bytes[..n]);
        }
    }
    assert_eq!(ended, 100);
    for b in bodies {
        assert_eq!(b, b"GET /interop ");
    }
}

#[test]
fn proxy_connect_then_tls_then_http() {
    use turnloop_http::client::{Route, TransportRequest};
    let cert = tls_support::certificate();
    let server_config = tls_support::server_config(&cert);
    let client = turnloop_tls::ClientConfig::new(
        turnloop_tls::ClientOptions {
            ca: Some(vec![cert.cert.der().clone()]),
            alpn: vec![b"http/1.1".to_vec()],
            ..Default::default()
        },
        tls_support::NOW,
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let mut socket = accept(listener);
        let mut decoder = http1::Decoder::new(http1::Mode::Request, Default::default());
        let (head, body, _) = response(&mut socket, &mut decoder);
        assert_eq!(head.method, "CONNECT");
        assert_eq!(head.target, "localhost:443");
        assert_eq!(
            head.get("proxy-authorization"),
            Some(b"Basic dXNlcjpwYXNz".as_slice())
        );
        assert!(body.is_empty());
        socket
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .unwrap();
        serve_h1(tls_support::Stream::new(
            server_config.accept().unwrap(),
            socket,
        ));
    });
    let request = Request::new("https://localhost/interop", "GET").unwrap();
    let mut route = Route::new(
        request.url.clone(),
        Some(url::Url::parse(&format!("http://127.0.0.1:{port}")).unwrap()),
    );
    let mut socket = socket(port);
    let mut wire = Vec::new();
    let mut encoder = http1::Encoder::start(
        &route.connect_head(Some(b"Basic dXNlcjpwYXNz")).unwrap(),
        http1::BodyLength::Empty,
        &mut wire,
    )
    .unwrap();
    encoder.finish(&[], &mut wire).unwrap();
    socket.write_all(&wire).unwrap();
    let mut decoder = http1::Decoder::new(http1::Mode::Response, Default::default());
    decoder.response_to("CONNECT");
    let mut input = Vec::new();
    let status = loop {
        let step = decoder.receive(&input).unwrap();
        if let Some(http1::Event::Head(head)) = step.event {
            assert_eq!(step.consumed, input.len());
            break head.status;
        }
        let mut bytes = [0; 64];
        let n = socket.read(&mut bytes).unwrap();
        assert!(n > 0);
        input.extend_from_slice(&bytes[..n]);
    };
    assert!(matches!(
        route.tunnel_response(status).unwrap(),
        TransportRequest::UpgradeTls { .. }
    ));
    assert!(matches!(
        decoder.receive(&[]).unwrap().event,
        Some(http1::Event::Upgrade)
    ));
    let mut stream = tls_support::Stream::new(
        client
            .connect(turnloop_tls::rustls::pki_types::ServerName::try_from("localhost").unwrap())
            .unwrap(),
        socket,
    );
    send(&mut stream, &request);
    let (_, body, _) = response(
        &mut stream,
        &mut http1::Decoder::new(http1::Mode::Response, Default::default()),
    );
    assert_eq!(body, b"native-http");
    server.join().unwrap();
}
#[test]
fn abort_mid_body_closes_socket_and_completes_once() {
    use turnloop_http::client::{Completion, Http1Connection};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let mut socket = accept(listener);
        let mut decoder = http1::Decoder::new(http1::Mode::Request, Default::default());
        let (head, _, _) = response(&mut socket, &mut decoder);
        assert_eq!(head.target, "/abort");
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\npartial")
            .unwrap();
        let mut b = [0; 1];
        assert_eq!(
            socket.read(&mut b).unwrap(),
            0,
            "host must close aborted HTTP/1 transport"
        );
    });
    let mut socket = socket(port);
    let mut engine = Http1Connection::new(Default::default());
    engine
        .start(
            &Request::new("http://localhost/abort", "GET")
                .unwrap()
                .head(false),
            http1::BodyLength::Empty,
            None,
            None,
        )
        .unwrap();
    engine.finish_body(&[]).unwrap();
    socket.write_all(engine.output()).unwrap();
    engine.consume_output(engine.output().len()).unwrap();
    let mut input = Vec::new();
    let mut observed = false;
    while !observed {
        let step = engine.receive(&input).unwrap();
        let n = step.consumed;
        if let Some(http1::Event::Body(bytes)) = step.event {
            assert_eq!(bytes, b"partial");
            observed = true;
        }
        input.drain(..n);
        if n == 0 {
            let mut bytes = [0; 1024];
            let n = socket.read(&mut bytes).unwrap();
            assert!(n > 0);
            input.extend_from_slice(&bytes[..n]);
        }
    }
    engine.abort();
    assert!(
        matches!(engine.poll_completion(),Some(Completion::Error(e))if e.code=="UND_ERR_ABORTED")
    );
    assert_eq!(engine.poll_completion(), None);
    assert!(!engine.reusable());
    drop(socket);
    server.join().unwrap();
}
