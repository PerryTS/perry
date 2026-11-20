### WebSockets on turnloop, and one WebSocket codec instead of three

An attached `new WebSocketServer({ server })` no longer forces its `http` /
`https` server off turnloop. P5 recorded the blocker as "the handshake needs an
owned stream a turnloop connection cannot produce" — the second half is true and
the first half was a property of `tokio_tungstenite`, not of WebSocket. RFC
6455's opening handshake is an HTTP/1.1 request and a `101`, and its framing is a
state machine over byte slices, so with `turnloop-websocket`'s sans-I/O core both
are pure functions of bytes. Nothing has to move: `perry-ext-http` keeps the
connection, its id, its outstanding multishot read and its TLS layer, and only
the decoder changes — the shape P5 used for TLS (a session installed *above* a
turnloop handle), one layer up.

The same core replaces `tokio-tungstenite` in `perry-ext-ws`, `perry-ext-http`,
`perry-ext-fastify` and `perry-stdlib`, which removes four edges from
`scripts/tokio_inventory.json` and takes tungstenite 0.29 out of the tree. One
codec now serves both transports — a turnloop handle id and a tokio stream —
because a sans-I/O state machine has no opinion about either.

`codec::Codec::receive` is the single place the `Received` contract is handled
(PerryTS/turnloop#86): `consumed == 0` with no message is the *only* case that
means "wait"; `consumed > 0` with no message and `consumed == 0` with a message
both mean keep going. `receive_loop_handles_both_zero_cases` pins all four.

Node-fidelity fixes the swap made reachable — each of these was wrong, not merely
missing:

- a binary frame reaches JS as a `Buffer`. It used to be run through
  `String::from_utf8_lossy`, so every non-UTF-8 byte became U+FFFD and the
  payload could not be recovered;
- `'message'` passes `isBinary` as its second argument;
- `'ping'` / `'pong'` events exist — an inbound control frame used to hit a
  catch-all and vanish;
- `close(code, reason)` reaches the wire and `'close'` receives both. The FFI
  took no arguments at all and always sent `Close(None)`, so a peer could never
  observe an application close code;
- `ws.send(buffer)`, `ws.ping()`, `ws.pong()` and `ws.terminate()` exist;
- the hyper upgrade path validates the handshake. It checked neither
  `Sec-WebSocket-Version` nor `Upgrade: websocket`, and answered a request with
  no `Sec-WebSocket-Key` with an empty accept value and a `101` anyway;
- `js_ws_on` replays the pre-listener message backlog, which only
  `js_ws_on_client_i64` did. On the turnloop transport a frame pipelined behind
  the handshake is decoded a pump tick before `wss.on('connection')` runs.

Full writeup, including what did **not** move and why: `docs/turnloop/ws-report.md`.
