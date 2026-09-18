//! Manual end-to-end probe for the blocking WebSocket client:
//! `cargo run -p perry-http-client --example ws_probe -- ws://host:port/path`.
//!
//! Not a test — it needs a server — but the framing unit tests drive
//! [`perry_http_client::ws::frame`] with bytes and prove nothing about the
//! handshake, the masking key, ping handling, or reading several frames out of
//! one segment. This connects, subscribes the way `perry publish` does, and
//! prints every message until the peer closes.
use std::time::Duration;

fn main() {
    let url = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: ws_probe <ws:// or wss:// url>");
        std::process::exit(2);
    });
    let mut ws = match perry_http_client::WebSocket::connect(&url, Duration::from_secs(30)) {
        Ok(ws) => ws,
        Err(e) => {
            println!("CONNECT FAILED {url} -> {e}");
            std::process::exit(1);
        }
    };
    println!("connected {url}");
    if let Err(e) = ws.send_text(
        r#"{"type":"subscribe","job_id":"job-1"}"#,
        Duration::from_secs(10),
    ) {
        println!("SEND FAILED -> {e}");
        std::process::exit(1);
    }
    println!("sent subscribe");
    let mut messages = 0usize;
    loop {
        match ws.read_message(Duration::from_secs(20)) {
            Ok(Some(perry_http_client::ws::Message::Text(text))) => {
                messages += 1;
                println!("text[{messages}] {text}");
            }
            Ok(Some(perry_http_client::ws::Message::Binary(bytes))) => {
                messages += 1;
                println!("binary[{messages}] {} bytes", bytes.len());
            }
            Ok(Some(perry_http_client::ws::Message::Close(code))) => {
                println!("close {code:?}");
                break;
            }
            Ok(None) => {
                println!("peer closed after {messages} message(s)");
                break;
            }
            Err(e) => {
                println!("READ FAILED after {messages} message(s) -> {e}");
                ws.close();
                std::process::exit(1);
            }
        }
    }
    ws.close();
    println!("done, {messages} message(s)");
}
