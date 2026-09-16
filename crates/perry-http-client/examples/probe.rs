//! Manual end-to-end probe: `cargo run -p perry-http-client --example probe -- <url> [...]`.
//!
//! Not a test — it needs a network — but the only way to establish that the
//! transport, the TLS session and the HTTP/1 codec actually talk to a server.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: probe <url> [url...]");
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
