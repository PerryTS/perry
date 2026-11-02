### Testing

**`node:http2` conformance fixtures, measured against a real wire.** Fourteen new
gap fixtures under `test-files/test_gap_http2_*.ts`, plus the shared frame codec
`test-files/_helpers/h2_wire.ts`, pin what Perry's `node:http2` has to do —
recorded from **Node 26.5.1**, never from Perry's own output.

Perry's HTTP/2 *streams* are real (hyper on the server, the `h2` crate for
`http2.connect`). Its *control surface* is not. `session.settings()`,
`session.ping()` and `session.goaway()` in
`crates/perry-ext-http/src/server/http2_server/controls.rs` never encode a
frame: they walk the process's own handle table with
`iter_handle_ids_of::<Http2SessionHandle>` for a session of the opposite kind
and push a synthetic event at it. `test-parity/node-suite/http2/` passes
because both ends of every fixture there are Perry, in one process, so the
simulation always finds its "peer". Against another process — curl, a browser,
a load balancer — none of those three methods does anything.

Every fixture whose name contains `_wire_` therefore puts a **raw TCP socket**
on one end and hand-encodes/decodes frames, which the loopback path cannot
satisfy: there is no second session handle to find. The codec ships an HPACK
*encoder* only (literal-without-indexing, no Huffman) — enough to open real
streams — because every assertion reads control frames, which carry no header
block.

What the fixtures establish, all oracle-verified and byte-identical across
three Linux runs and one macOS run:

- SETTINGS: a default server's first frame is an **empty** SETTINGS; configured
  settings serialise in ascending identifier order; `pendingSettingsAck` is true
  from connect; one ACK resolves one outstanding SETTINGS, in order, and
  `'localSettings'` plus the user callback fire only then — with **three**
  arguments, `(err, settings, duration)`.
- PING: the round-trip `duration` is a real positive measurement (Perry's pump
  passes the literal `0.0`); `ping()` without a callback **throws**
  `ERR_INVALID_ARG_TYPE`; an unsolicited `PING|ACK` is a protocol error.
- GOAWAY: `'goaway'`'s third argument is `undefined` when the frame carried no
  opaque data, not a zero-length Buffer; sending one does not close the session;
  and from a client session an **odd** `lastStreamID` — 2147483647 included —
  suppresses the frame silently.
- Two corrections to widely-held belief, both measured: a stream opened after a
  graceful GOAWAY gets **no frame at all** from Node, not
  `RST_STREAM(REFUSED_STREAM)`; and REFUSED\_STREAM's real trigger,
  `maxConcurrentStreams`, has two regimes — polite `RST_STREAM code=7` before
  the peer ACKs the limit, `GOAWAY code=2` / `errno -505` after.
- Flow control, the protocol-error → GOAWAY-code table (including nghttp2's
  `"DATA: stream_id == 0"` debug string), `close()` vs `destroy()`, ALPN on
  `createSecureServer`, and cross-session isolation of every control frame.

The fixtures are registered in `test-parity/gap_snapshot.json` at the status
they currently produce, so the transport work flips them to passing and the
snapshot diff is the record of it. Companion document:
`docs/src/testing/http2-conformance.md`, which lists every behaviour with
Node's actual output beside it and the ordered list of what the transport must
implement. No transport code is changed here.
