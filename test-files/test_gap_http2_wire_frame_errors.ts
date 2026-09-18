// @covers node:http2 protocol-error mapping: GOAWAY codes and 'error' surface (#10327)
//
// ORACLE: Node 26.5.1. A raw peer writes a deliberately malformed frame at an
// `http2.createServer()` and the fixture records BOTH the GOAWAY Node puts on
// the wire and the error Node surfaces to the session. Perry's http2 surface
// parses nothing off a socket, so it cannot detect any of these.
//
// Established against Node (error codes are RFC 7540 §7 values):
//   * SETTINGS whose length is not a multiple of 6   -> GOAWAY code=6 (FRAME_SIZE_ERROR)
//   * SETTINGS|ACK carrying a payload                 -> GOAWAY code=6
//   * PING with a 7-byte payload                      -> GOAWAY code=6
//   * DATA on stream 0                                -> GOAWAY code=1 (PROTOCOL_ERROR),
//       with nghttp2's debug string "DATA: stream_id == 0" in the OPAQUE field
//   * WINDOW_UPDATE with increment 0                  -> GOAWAY code=2 (INTERNAL_ERROR)
//   * RST_STREAM for an idle stream                   -> GOAWAY code=2
//   * HEADERS on stream 0                             -> GOAWAY code=2
//   * a corrupt HPACK block                           -> GOAWAY code=9 (COMPRESSION_ERROR)
//   * an UNKNOWN frame type is IGNORED: no reply, session survives
//
// In every failing case the session-level surface is the SAME:
//   Error [ERR_HTTP2_ERROR]: Protocol error, with errno -505 (NGHTTP2_ERR_PROTO).
// No 'frameError' is emitted on the receiving side.
import http2 from "node:http2";
import { Buffer } from "node:buffer";
import {
  RawClientPeer,
  dump,
  sleep,
  frame,
  headerBlock,
  FRAME_DATA,
  FRAME_HEADERS,
  FRAME_SETTINGS,
  FRAME_PING,
  FRAME_RST_STREAM,
  FRAME_WINDOW_UPDATE,
  FLAG_ACK,
  FLAG_END_HEADERS,
  FLAG_END_STREAM,
  rstPayload,
  windowUpdatePayload,
  barrier,
} from "./_helpers/h2_wire.ts";

async function scenario(label: string, inject: (peer: RawClientPeer) => void): Promise<void> {
  const server = http2.createServer();
  const notes: string[] = [];
  server.on("session", (s: any) => {
    s.on("error", (e: any) => notes.push("session error " + e.name + " " + e.code + " errno=" + e.errno + " | " + e.message));
    s.on("frameError", (t: number, c: number, id: number) => notes.push("frameError type=" + t + " code=" + c + " id=" + id));
    s.on("close", () => notes.push("session close"));
  });
  server.on("stream", (stream: any) => {
    stream.on("error", (e: any) => notes.push("stream error " + e.code));
    stream.respond({ ":status": 200 });
    stream.end("x");
  });
  await new Promise<void>((r) => server.listen(0, "127.0.0.1", () => r()));
  const port = (server.address() as any).port;
  const peer = await RawClientPeer.connect(port);
  peer.autoAck = false;
  await sleep(120);
  peer.take();
  inject(peer);
  await sleep(220);
  console.log("== " + label + " ==");
  dump("  wire", peer.take());
  console.log("  api:", notes.length === 0 ? "(none)" : JSON.stringify(notes));
  console.log("  peer socket closed:", peer.closed);
  peer.destroy();
  await barrier(new Promise<void>((r) => server.close(() => r())), 300);
}

await scenario("SETTINGS with a 5-byte payload", (p) =>
  p.send(frame(FRAME_SETTINGS, 0, 0, Buffer.alloc(5))));
await scenario("SETTINGS|ACK with a 6-byte payload", (p) =>
  p.send(frame(FRAME_SETTINGS, FLAG_ACK, 0, Buffer.alloc(6))));
await scenario("PING with a 7-byte payload", (p) =>
  p.send(frame(FRAME_PING, 0, 0, Buffer.alloc(7))));
await scenario("DATA on stream 0", (p) =>
  p.send(frame(FRAME_DATA, 0, 0, Buffer.from("x", "latin1"))));
await scenario("WINDOW_UPDATE with increment 0", (p) =>
  p.send(frame(FRAME_WINDOW_UPDATE, 0, 0, windowUpdatePayload(0))));
await scenario("RST_STREAM on idle stream 1", (p) =>
  p.send(frame(FRAME_RST_STREAM, 0, 1, rstPayload(http2.constants.NGHTTP2_CANCEL))));
await scenario("HEADERS on stream 0", (p) =>
  p.send(frame(FRAME_HEADERS, FLAG_END_HEADERS | FLAG_END_STREAM, 0,
    headerBlock([[":method", "GET"], [":scheme", "http"], [":authority", "127.0.0.1"], [":path", "/"]]))));
await scenario("corrupt HPACK block on stream 1", (p) =>
  p.send(frame(FRAME_HEADERS, FLAG_END_HEADERS | FLAG_END_STREAM, 1,
    Buffer.from([0xff, 0xff, 0xff, 0xff, 0xff]))));
await scenario("unknown frame type 0x63", (p) =>
  p.send(frame(0x63, 0, 0, Buffer.from("hello", "latin1"))));
