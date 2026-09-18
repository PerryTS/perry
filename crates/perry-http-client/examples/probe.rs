//! Manual end-to-end probe: `cargo run -p perry-http-client --example probe -- <url> [...]`.
//!
//! Not a test — it needs a network — but the only way to establish that the
//! transport, the TLS session and the HTTP/1 codec actually talk to a server.
/// Writes a streamed body to a file and counts it, so `--stream` exercises the
/// same `BodySink` shape the self-updater uses.
struct FileSink {
    file: std::fs::File,
    declared: Option<u64>,
    written: u64,
    heads: usize,
}

impl perry_http_client::BodySink for FileSink {
    fn on_head(
        &mut self,
        status: u16,
        content_length: Option<u64>,
    ) -> perry_http_client::Result<()> {
        self.heads += 1;
        self.declared = content_length;
        println!(
            "     head #{} status={status} content-length={content_length:?}",
            self.heads
        );
        Ok(())
    }

    fn on_chunk(&mut self, bytes: &[u8]) -> perry_http_client::Result<()> {
        use std::io::Write;
        self.file
            .write_all(bytes)
            .map_err(|e| perry_http_client::Error::sink(format!("write failed: {e}")))?;
        self.written += bytes.len() as u64;
        Ok(())
    }
}

fn stream(url: &str, path: &str) -> i32 {
    let client = perry_http_client::Client::new().timeout(std::time::Duration::from_secs(300));
    let file = match std::fs::File::create(path) {
        Ok(file) => file,
        Err(e) => {
            println!("FAIL cannot create {path}: {e}");
            return 1;
        }
    };
    let mut sink = FileSink {
        file,
        declared: None,
        written: 0,
        heads: 0,
    };
    match client.execute_streaming(perry_http_client::Request::get(url), &mut sink) {
        Ok(response) => {
            println!(
                "OK   {url} -> {} final={} heads={} declared={:?} written={} buffered={}",
                response.status,
                response.url,
                sink.heads,
                sink.declared,
                sink.written,
                response.body.len(),
            );
            // The two assertions that matter: the head was offered exactly once
            // even through a redirect, and every declared byte arrived.
            if sink.heads != 1 {
                println!("FAIL expected exactly one head, got {}", sink.heads);
                return 1;
            }
            if let Some(declared) = sink.declared {
                if declared != sink.written {
                    println!("FAIL declared {declared} but wrote {}", sink.written);
                    return 1;
                }
            }
            0
        }
        Err(e) => {
            println!("FAIL {url} -> {e}");
            1
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--stream") {
        let url = args.get(1).expect("usage: probe --stream <url> <path>");
        let path = args.get(2).expect("usage: probe --stream <url> <path>");
        std::process::exit(stream(url, path));
    }
    if args.is_empty() {
        eprintln!("usage: probe <url> [url...]  |  probe --stream <url> <path>");
        std::process::exit(2);
    }
    let client = perry_http_client::Client::new().timeout(std::time::Duration::from_secs(30));
    let mut failures = 0;
    for url in &args {
        match client.execute(perry_http_client::Request::get(url)) {
            Ok(response) => {
                let body = response.text();
                println!(
                    "OK   {url} -> {} {} bytes  final={}  ct={}",
                    response.status,
                    response.body.len(),
                    response.url,
                    response
                        .header("content-type")
                        .map(|v| String::from_utf8_lossy(v).into_owned())
                        .unwrap_or_else(|| "-".into()),
                );
                println!("     first 80: {:?}", &body[..body.len().min(80)]);
            }
            Err(e) => {
                failures += 1;
                println!("FAIL {url} -> {e}");
            }
        }
    }
    std::process::exit(if failures == 0 { 0 } else { 1 });
}
