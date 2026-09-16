# turnloop WS — WebSockets, and the end of three tungstenite majors

Branch `turnloop/websockets`, based on `turnloop/integration` at `96326a45c4`.
Built and tested on the shared Linux box (EPYC 9354P) against the pinned gap
oracle Node **26.5.1**. Nothing here was run on macOS or Windows, and nothing
was benchmarked.

## The question this lane was given

> Can the WebSocket handshake be driven sans-I/O over a connection Perry keeps
> owning, or does something genuinely need the owned stream?

**It can, and nothing needs the owned stream.** That is not a judgement call —
it is the signature of the function that does it:

```rust
pub fn accept(request: &Head, protocols: &[&str]) -> Result<(Head, Option<String>), Error>
```

`turnloop_websocket::accept` takes a *decoded request head* and returns the
`101` head to write. There is no stream in the type, because RFC 6455's opening
handshake is an HTTP/1.1 request and a `101` and nothing else: a
`Sec-WebSocket-Key` goes in, `SHA-1(key + GUID)` base64'd comes out. The framing
that follows is the same shape — `Connection::receive(&[u8], &mut Vec<u8>)`.

What genuinely needed an owned stream was
`tokio_tungstenite::WebSocketStream<S>`, whose `S: AsyncRead + AsyncWrite +
Unpin + Send + 'static` bound is an API decision of that crate. P5's inventory
entry — *"the handshake needs an owned stream a turnloop connection cannot
produce"* — was right about the consequence and attributed it one layer too
low. **This is not a turnloop gap and there is nothing to file for it.**

The proof is a unit test that constructs a `101` with no transport anywhere in
scope, against RFC 6455 §1.3's worked example
(`turnloop_link::tests::a_handshake_needs_no_stream`).

## What that makes possible, and why nothing moves

P5 solved the same shape for TLS by putting the session *above* the socket, so
no descriptor had to move. A WebSocket is one layer further up and needs even
less. Compare the two upgrade paths `perry-ext-http` now has:

| | `server.on('upgrade')` (P5) | attached `WebSocketServer` (this lane) |
|---|---|---|
| who owns the connection afterwards | `perry-ext-net` | still `perry-ext-http` |
| mechanism | `turnloop_net::transfer` — the subsystem tag changes | nothing; a codec is installed beside the connection |
| why | a `net.Socket` is handed to JS and outlives the HTTP connection | a WebSocket has no such JS object; only the decoder changes |

So the id, the outstanding multishot read, the write queue and the TLS layer are
all untouched. `Conn` gains one `bool`; `feed` routes to
`perry_ext_ws::turnloop_link` instead of the HTTP decoder; `write_raw` — already
TLS-transparent — carries the `101` and every frame. An attached
`WebSocketServer` on an **HTTPS** server therefore works with no extra code at
all, which is the part that would have been expensive with a descriptor handoff.

`perry-ext-http` already depends on `perry-ext-ws`, so the callback direction is
fixed: the host installs a `turnloop_link::Transport` of three function
pointers (`write`, `finish`, `destroy`) at sink-registration time, the same
one-way seam `register_http_address_reader` already uses.

### `finish` is not `destroy`

The transport has two shutdown verbs on purpose. A closing handshake ends with a
close frame written and *then* a shutdown, and `turnloop`'s `close` cancels the
connection's outstanding operations — including the write just queued. P5 hit
this edge from the other side (its `allowHalfOpen` close cancelled the writes an
`'end'` handler had queued, and the fix was to separate "should we shut down"
from "may the socket go away yet"). Collapsing the two here would have sent every
peer 1006 instead of the code it asked for, intermittently.

## One codec instead of three tungstenite majors

The tree carried tungstenite 0.24 (`perry-ui-android`, sync, own thread), 0.29
(`perry-ext-ws`, `perry-ext-http`, `perry-ext-fastify`, `perry-stdlib`) and 0.30
(`turnloop-websocket`, unused). Because the sans-I/O core is a state machine
over byte slices, it serves a tokio stream exactly as well as a turnloop handle
— so it replaced 0.29 in all four crates rather than only in the migrated one.
**tungstenite 0.29 is gone from `Cargo.lock`.**

`perry-ext-ws` keeps `tokio`: a thread acting for an agent another thread already
owns has no loop, and P1's coexistence rule says a reachable configuration is not
deleted. It gained no TLS dependency for the outbound `wss://` client either —
`perry_ext_net::connect_tls_client` hands back a boxed
`AsyncRead + AsyncWrite`, so the TLS stack is named in exactly one crate.

## The `Received` contract (PerryTS/turnloop#86)

`Connection::receive` returns `Received { consumed, message }`, and the reading
is **not** "an event came back, so keep going":

| `consumed` | `message` | meaning |
|---|---|---|
| `0` | `None` | **wait** — no progress is possible until more bytes arrive |
| `> 0` | `None` | **keep going** — a partial frame, or a control frame answered internally |
| `0` | `Some` | **keep going** — a whole frame was already buffered from an earlier call |
| `> 0` | `Some` | **keep going** — tungstenite reads one chunk per pass |

Only the first row terminates the loop. `codec::Codec::receive` is the single
place in Perry that implements it, and `receive_loop_handles_both_zero_cases`
pins all four rows — including the third, which is the one a "stop when
`consumed == 0`" loop silently drops a decoded message on.

`flush` matters too: `receive` *queues* the pong for a ping but only *encodes* it
on a flush, so a host that flushed only around application writes answers a ping
whenever it happens to send something next. `Codec::receive` flushes before it
returns.

## What did NOT move

Named precisely, because each is a hole rather than a preference:

* **The outbound `ws` client's transport.** `new WebSocket(url)` still connects
  on a tokio stream. Its codec and handshake are the shared ones, so this is a
  transport migration that remains, not a protocol one — it needs turnloop DNS
  plus a `turnloop_tls_io` client install, both of which exist.
* **`new WebSocketServer({ port })`.** Same: its accept loop is still
  `tokio::net::TcpListener`, driving the shared codec. The *attached* shape is
  the one group A named, and it is the one that moved.
* **`perry-ext-ws` → `tokio`.** The declining-transport edge, kept by the P1
  coexistence rule. It is now the only tokio edge that crate has.
* **`perry-ui-android` → tungstenite 0.24.** Sync tungstenite on its own thread,
  Android-target only. It is a candidate for the same treatment — the sans-I/O
  core works over a blocking `std::net::TcpStream` too — but it cannot be built
  or run from this box, and a migration nobody can execute is not one this lane
  should land. It keeps the third major alive.
* **`perMessageDeflate`, `maxPayload`, `verifyClient`, and `WebSocketServer`'s
  `path` option.** `path` is the notable one: it is read nowhere, so
  `new WebSocketServer({ port, path: '/ws' })` accepts on **every** path,
  silently. Pre-existing; unchanged here.
* **`'error'` carries a string, not an `Error`.** Pre-existing; `err.message` is
  `undefined`. Unchanged here.
