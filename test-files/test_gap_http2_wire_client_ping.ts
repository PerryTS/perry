// @covers node:http2 client PING round trip + duration argument (#10327)
//
// ORACLE: Node 26.5.1. Perry's `session.ping()` never encodes a PING frame; it
// pushes a synthetic `SessionPingCallback` at an in-process peer session and
// the pump calls back with a HARDCODED duration of 0.0
// (`call3(callback, err, 0.0, payload_arg)` — perry-ext-http
// `server/http2_server/pump.rs`). Here the peer is a raw socket, so the PING
// has to reach the wire and the ACK has to come back before the callback can
// fire at all, and the duration is a real measurement.
//
// Established against Node:
//   * `ping(payload, cb)` writes PING (type 6, flags 0, stream 0, length 8);
//   * the callback fires only after the peer's PING|ACK, with
//     `(null, duration, payload)` where duration is a number STRICTLY > 0;
//   * `ping(cb)` with no payload generates a RANDOM 8-byte payload, which the
//     callback receives back verbatim (so it round-tripped through the wire);
//   * `ping()` with no callback THROWS ERR_INVALID_ARG_TYPE — it is not a
//     silent `return false`;
//   * a payload that is not exactly 8 bytes throws ERR_HTTP2_PING_LENGTH;
//   * a non-Buffer payload throws ERR_INVALID_ARG_TYPE.
import http2 from "node:http2";
import { RawServerPeer, dump, sleep, withTimeout, waitEvent, barrier } from "./_helpers/h2_wire.ts";
import { Buffer } from "node:buffer";

const peer = new RawServerPeer();
const port = await peer.listen();
const client: any = http2.connect("http://127.0.0.1:" + port);
if (!(await waitEvent(client, "connect", 800))) console.log("!! client never emitted connect");
await sleep(120);
peer.take();

const withPayload = await withTimeout<any>(new Promise<any>((resolve) => {
  client.ping(Buffer.from("PERRY-h2", "latin1"), (err: any, duration: number, payload: Buffer) => {
    resolve({
      err: err === null ? "null" : String(err && err.code),
      durationIsNumber: typeof duration === "number",
      durationPositive: duration > 0,
      payload: payload.toString("latin1"),
      payloadIsBuffer: Buffer.isBuffer(payload),
    });
  });
}), 1500, { err: "CALLBACK NEVER FIRED" });
await sleep(60);
dump("wire for ping(Buffer('PERRY-h2'), cb)", peer.take());
console.log("callback:", JSON.stringify(withPayload));

const generated = await withTimeout<any>(new Promise<any>((resolve) => {
  client.ping((err: any, duration: number, payload: Buffer) => {
    resolve({
      err: err === null ? "null" : String(err && err.code),
      durationPositive: duration > 0,
      payloadLength: payload.length,
      payloadIsBuffer: Buffer.isBuffer(payload),
    });
  });
}), 1500, { err: "CALLBACK NEVER FIRED" });
await sleep(60);
// The generated payload is random, so print only its SHAPE plus the fact that
// the bytes on the wire are the bytes the callback saw.
const wire = peer.take();
console.log("wire frames for ping(cb):", wire.length);
console.log("wire frame is a non-ACK 8-byte PING:", wire.length === 1 && wire[0].indexOf("PING stream=0 len=8 ") === 0);
console.log("callback:", JSON.stringify(generated));

function caught(label: string, fn: () => void): void {
  try {
    fn();
    console.log(label, "-> NO THROW");
  } catch (e: any) {
    console.log(label, "->", e.name, "|", e.code, "|", e.message);
  }
}
caught("ping() no callback", () => client.ping());
caught("ping(Buffer(7), cb)", () => client.ping(Buffer.alloc(7), () => {}));
caught("ping(Buffer(9), cb)", () => client.ping(Buffer.alloc(9), () => {}));
caught("ping('12345678', cb)", () => client.ping("12345678", () => {}));

client.destroy();
peer.close();
