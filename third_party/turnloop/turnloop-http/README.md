# turnloop-http

Own HTTP/1.1 and HTTP/2 wire engines, with no runtime or transport dependency.

- `http1::Decoder::receive` consumes a prefix and returns one event. Retain the unconsumed bytes, including bytes after a CONNECT/WebSocket upgrade. A zero-consumption event is progress; call again until both consumption and event are empty. `eof` distinguishes close-delimited completion from truncated framing.
- `client::Http1Connection` adds non-pipelined requests, streamed uploads, `100-continue`, output acknowledgements, deadline/abort state, and one terminal completion per accepted request. Take the completion before reusing the connection.
- `http2::Connection` handles both roles. Flush and acknowledge `output`, retain incomplete input, consume one frame/event at a time. `send_data` can accept only a prefix or zero when flow-controlled; retain the remainder. Return DATA capacity with `release_capacity` after the application consumes it. Queue application responses in the host during stalls. On transport loss call `eof`, then drain `poll_failed_stream`.
- `hpack` provides the independent bounded RFC 7541 codec. The encoder never indexes credentials/cookies. Decode failures poison a context.
- `client::Pool` reserves connections before DNS/connect so parallel commands respect per-origin/proxy limits. `Route` emits resolution/TLS requests and builds proxy CONNECT or absolute-form heads. `Resolver` belongs to the host. `Request::redirect` applies Fetch redirects; use `DEFAULT_MAX_REDIRECTS` (20). JS conversions and promise delivery remain with Perry. `Pool::contains`/`forget` check or drop a stale `ConnectionId`. `Pool::next_timeout` is one O(1) deadline over idle connections and every request registered with `set_request_deadline`; drain `handle_timeout` and `handle_request_timeout`. `client::Deadlines` is the same heap for host-keyed timers.
- `multipart::Form` encodes in-memory `multipart/form-data` text and file parts. The host supplies 16 bytes of entropy; the boundary is regenerated until it occurs in no part.
- `compression::StreamingDecoder` accepts input and caller-owned output. Each step says whether it stopped for input (`needs_input`), for output space, or because the body finished. `new` takes a `Content-Encoding` value, list included (`gzip, br`, decoded innermost-last); `from_codings(head.values("content-encoding"), limit)` merges repeated header lines. Reuse it with `reset` to retain scratch buffers across bodies. Hosts may cache one per content encoding. `decode` is the convenience whole-body path and constructs algorithm state each time.

No engine samples a clock. The host passes `Instant` deadlines and invokes timeout handlers. Hold output storage stable until a completion-shaped write finishes: do not mutate the engine while an I/O operation borrows its output. Error codes are transport causes; Perry creates the JS error objects and detailed OS diagnostics.

WASM uses the published `turnloop-zstd-decoder` fork of ruzstd 0.8.3 with retained sequence tables. Consumers need no workspace patch. Native builds use the reference zstd decoder by default; `pure-rust-zstd` selects the same decoder as WASM, including its allocation gates. See the decoder crate’s `UPSTREAM.md`.

Tests use private ephemeral loopback servers, Node 26, curl, generated TLS certificates, RFC vectors and a vendored HPACK corpus. `examples/h2spec_server.rs` drives the async server on turnloop; the required h2spec gate checks all 147 strict cases. Exact commands and limitations are in the root `LANE_REPORT.md`.

## Getting started on turnloop

Enable the `turnloop` feature. Create `LocalExecutor<Platform>`, clone its handle,
and spawn a task using `asynchronous::client::Client`. `request(&mut Request,
|bytes| ...)` delivers borrowed response chunks and returns the response head.
It retains per-origin connections, follows the existing redirect policy, applies
proxy CONNECT before TLS, and reuses incremental decompression state. `stream`
accepts an AsyncRead upload and a body length; the caller handles redirects for
non-replayable sources. One absolute deadline covers the whole request. The host
must call `expire()` at `next_deadline()` for idle pool eviction. Dropping a
pending request closes its lease; incomplete connections never re-enter the pool.

For explicit protocol control, `asynchronous::{Http1,Http2}` expose streaming
events, writes and HTTP upgrade handoff. HTTP/2 callbacks release receive capacity
after consuming DATA and retain unsent application data while send windows stall.
The pooled facade serializes each client's requests; multiplexing is available
through the lower-level HTTP/2 driver. `Expect: 100-continue` waits for an informational reply or the configured
continue deadline; an early final response suppresses the upload and closes the lease.

`asynchronous::server::Server` owns the TCP accept loop and local service tasks.
Use `server::http1` or `server::http2` inside a service (optionally after TLS/ALPN).
The HTTP/1 callback streams request events into a reusable Response encoder.
`server.shutdown().stop()` stops accepts, closes idle HTTP/1 connections and sends
HTTP/2 GOAWAY while existing streams drain. Wrap drain in an application deadline;
dropping Server cancels its remaining service tasks.

Connections close with a lingering close, as nginx `lingering_close` does: after the
final response (and GOAWAY) is flushed, the server half-closes, reads and discards
peer input until the peer's EOF, then closes. Closing with unread peer bytes (a
pipelined request, HTTP/2 SETTINGS or WINDOW_UPDATE acknowledgements) would send
RST, which can discard the peer's unread copy of the response on macOS and Windows.
`server::Options::linger_timeout` (default 5 s, `Server::bind_with` or
`Shutdown::with_options`) bounds the discard phase, and `Shutdown::stop_by(deadline)`
closes every lingering connection by an overall shutdown deadline. Streams passed to
`server::http1`/`http2` implement `turnloop_io::HalfClose`.

```sh
cargo run -p turnloop-http --features turnloop --example https_get -- https://example.com/
cargo run -p turnloop-http --features turnloop --example tls_echo -- cert.pem key.pem 127.0.0.1:8443
cargo run -p turnloop-http --features turnloop --example https_get -- https://localhost:8443/ cert.pem
```

The TLS echo example negotiates HTTP/1.1 or HTTP/2 through ALPN. Certificates and
private keys are supplied by the host; certificate verification stays enabled.
WASI 0.2/0.3 use wasi:sockets and this same TLS/HTTP path (p3 remains experimental).
The current core resolver uses a native blocking pool: WASI clients need literal
IP URLs, or a host-resolved stream with the low-level HTTP/TLS drivers. The latter
lets the host retain the original hostname for SNI and certificate validation.
On browsers, `asynchronous::web::get` maps to the existing host fetch operation:
bounded GET response bodies, abort and deadlines. The current host interface does
not expose status/headers, custom methods, body streaming, proxy CONNECT or ALPN;
redirects, decompression and TLS are controlled by the browser. Servers/raw sockets
are Unsupported there. See `turnloop-io` for the shared ownership pattern.
