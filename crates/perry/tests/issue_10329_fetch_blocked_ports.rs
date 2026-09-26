//! Fetch rejects blocked destinations before connecting, including redirects.
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::time::{Duration, Instant};

#[test]
fn fetch_rejects_blocked_ports_and_followed_redirects() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let entry = dir.path().join("fetch_blocked_ports.ts");
    let binary = dir.path().join("fetch_blocked_ports");
    std::fs::write(
        &entry,
        include_str!("../../../test-files/test_gap_10329_fetch_blocked_ports.ts"),
    )
    .unwrap();
    let compile = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(dir.path())
        .args(["compile", "--no-cache"])
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .output()
        .expect("compile regression fixture");
    assert!(
        compile.status.success(),
        "compile failed:\n{}\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut served = 0;
        while served < 4 {
            let (mut socket, _) = match listener.accept() {
                Ok(client) => client,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "fixture did not make all four requests"
                    );
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(error) => panic!("accept: {error}"),
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                let count = socket.read(&mut buffer).unwrap();
                assert!(count > 0, "request ended before its headers");
                request.extend_from_slice(&buffer[..count]);
                assert!(request.len() < 16384, "unexpected request size");
            }
            let response = if request.starts_with(b"GET /allowed ") {
                "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
            } else {
                "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:22/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            };
            socket.write_all(response.as_bytes()).unwrap();
            served += 1;
        }
    });
    let run = Command::new(&binary)
        .arg(origin)
        .output()
        .expect("run regression fixture");
    assert!(
        run.status.success(),
        "run failed:\n{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    server.join().expect("HTTP fixture server");
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        concat!(
            "blocked 164\n",
            "normalized TypeError|fetch failed|Error|bad port|undefined\n",
            "allowed 200 ok\n",
            "follow TypeError|fetch failed|Error|bad port|undefined\n",
            "manual 302 http://127.0.0.1:22/\n",
            "error TypeError|fetch failed|Error|unexpected redirect|undefined\n",
        )
    );
}
