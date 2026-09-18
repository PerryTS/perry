// @covers node:http2 server PING/ACK on the wire (#10327 / http2 real transport)
//
// ORACLE: Node 26.5.1. A raw TCP peer PINGs an `http2.createServer()` and
// prints the reply. Perry's `session.ping()` never encodes a PING frame — it
// pushes a synthetic `SessionPingCallback` event at an in-process peer — so a
// Perry server has nothing to answer a real PING with.
//
// Established against Node:
//   * a PING is answered with PING|ACK carrying the IDENTICAL 8 payload bytes;
//   * the reply is on stream 0 and its length is exactly 8;
//   * an all-zero payload is echoed as an all-zero payload (not omitted);
//   * `session.ping()` on the SERVER session puts a PING on the wire and the
//     round-trip duration handed to the callback is a positive number
//     (Perry hardcodes 0.0 — `call3(callback, err, 0.0, payload)` in
//     perry-ext-http `server/http2_server/pump.rs`);
//   * an UNSOLICITED PING|ACK is a protocol error: GOAWAY(code=2) and the
//     connection is torn down.
import http2 from "node:http2";
import {
  RawClientPeer,
  dump,
  sleep,
  frame,
  FRAME_PING,
  FLAG_ACK,
  withTimeout,
  barrier,
} from "./_helpers/h2_wire.ts";
import { Buffer } from "node:buffer";

const server = http2.createServer();
let serverSession: any = null;
server.on("session", (s: any) => {
  if (serverSession === null) serverSession = s;
});
await new Promise<void>((r) => server.listen(0, "127.0.0.1", () => r()));
const port = (server.address() as any).port;

const peer = await RawClientPeer.connect(port);
await sleep(200);
peer.take();

peer.send(frame(FRAME_PING, 0, 0, Buffer.from("perryh2!", "latin1")));
await sleep(200);
dump("PING 'perryh2!' -> reply", peer.take());

peer.send(frame(FRAME_PING, 0, 0, Buffer.alloc(8)));
await sleep(200);
dump("PING all-zero -> reply", peer.take());

// The server session's own ping() must reach the wire and be resolvable by
// the raw peer's ACK.
const pingResult = await withTimeout<any>(new Promise<any>((resolve) => {
  serverSession.ping(Buffer.from("srv-ping", "latin1"), (err: any, duration: number, payload: Buffer) => {
    resolve({
      err: err === null ? "null" : String(err && err.code),
      durationIsNumber: typeof duration === "number",
      durationPositive: duration > 0,
      payload: payload.toString("latin1"),
    });
  });
}), 1500, { err: "CALLBACK NEVER FIRED" });
await sleep(50);
dump("server session.ping() on the wire", peer.take());
console.log("server ping callback:", JSON.stringify(pingResult));

// An UNSOLICITED PING|ACK is a protocol error, not a no-op: nghttp2 answers
// GOAWAY(INTERNAL_ERROR=2) and tears the connection down. Kept last because
// it ends the session.
peer.send(frame(FRAME_PING, FLAG_ACK, 0, Buffer.from("ignoreme", "latin1")));
await sleep(250);
dump("unsolicited PING|ACK -> reply", peer.take());
console.log("peer socket closed by the server:", peer.closed);

peer.destroy();
await barrier(new Promise<void>((r) => server.close(() => r())), 500);
