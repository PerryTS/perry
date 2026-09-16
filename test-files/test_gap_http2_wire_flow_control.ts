// @covers node:http2 flow control: SETTINGS_INITIAL_WINDOW_SIZE and WINDOW_UPDATE (#10327)
//
// ORACLE: Node 26.5.1. A raw peer advertises a small initial window and never
// opens it, so the server's body has to stall at an exact byte count. This is
// arithmetic on the wire — a server that writes the whole body regardless (or
// writes nothing) is visibly wrong, and the loopback simulation has no window
// accounting at all.
//
// Established against Node:
//   * with SETTINGS_INITIAL_WINDOW_SIZE=1000 the server sends exactly 1000
//     body bytes in ONE DATA frame and then stalls;
//   * a stream-level WINDOW_UPDATE of +5000 releases exactly 5000 more;
//   * the CONNECTION window (default 65535) is a second, independent limit:
//     opening only the stream window stops the transfer at 65535 total;
//   * opening both windows delivers the remaining bytes and END_STREAM;
//   * on the receiving side, `stream.pause()` stops the body and `resume()`
//     delivers every byte, with no loss and no duplication.
import http2 from "node:http2";
import { Buffer } from "node:buffer";
import {
  RawClientPeer,
  dump,
  sleep,
  requestFrame,
  frame,
  settingsPayload,
  windowUpdatePayload,
  FRAME_WINDOW_UPDATE,
  SETTING_INITIAL_WINDOW_SIZE,
  barrier,
  waitEvent,
} from "./_helpers/h2_wire.ts";

const BODY_LEN = 200000;

// ---- part 1: window arithmetic on the wire ----
{
  const body = Buffer.alloc(BODY_LEN, 0x61);
  const server = http2.createServer();
  server.on("stream", (stream: any) => {
    stream.respond({ ":status": 200 });
    stream.end(body);
  });
  await new Promise<void>((r) => server.listen(0, "127.0.0.1", () => r()));
  const port = (server.address() as any).port;
  const peer = await RawClientPeer.connect(
    port,
    settingsPayload([[SETTING_INITIAL_WINDOW_SIZE, 1000]]),
  );
  peer.autoAck = false;
  await sleep(150);
  peer.take();

  peer.send(requestFrame(1, "/big"));
  await sleep(350);
  console.log("initialWindowSize=1000 -> dataBytes:", peer.dataBytes, "dataFrames:", peer.dataFrames);

  peer.send(frame(FRAME_WINDOW_UPDATE, 0, 1, windowUpdatePayload(5000)));
  await sleep(300);
  console.log("after stream WINDOW_UPDATE +5000 -> dataBytes:", peer.dataBytes);

  // Open ONLY the stream window wide: the connection window (65535) now binds.
  peer.send(frame(FRAME_WINDOW_UPDATE, 0, 1, windowUpdatePayload(1000000)));
  await sleep(350);
  console.log("after stream WINDOW_UPDATE +1000000 -> dataBytes:", peer.dataBytes);
  console.log("  stalled at the default connection window (65535):", peer.dataBytes === 65535);

  peer.send(frame(FRAME_WINDOW_UPDATE, 0, 0, windowUpdatePayload(1000000)));
  await sleep(500);
  console.log("after connection WINDOW_UPDATE +1000000 -> dataBytes:", peer.dataBytes);
  console.log("  whole body delivered:", peer.dataBytes === BODY_LEN);
  // DATA framing is chunked by the writer, so assert the END_STREAM marker
  // rather than a byte count that TCP segmentation can move.
  const tail = peer.take();
  const last = tail.length === 0 ? "(none)" : tail[tail.length - 1];
  console.log("  final frame is DATA on stream 1 with END_STREAM:",
    last.indexOf("DATA stream=1 ") === 0 && last.indexOf("END_STREAM") > 0);

  peer.destroy();
  await barrier(new Promise<void>((r) => server.close(() => r())), 500);
}

// ---- part 2: stream.pause()/resume() on the receiving side ----
{
  const body = Buffer.alloc(300000, 0x62);
  const server = http2.createServer();
  server.on("stream", (stream: any) => {
    stream.respond({ ":status": 200 });
    stream.end(body);
  });
  await new Promise<void>((r) => server.listen(0, "127.0.0.1", () => r()));
  const port = (server.address() as any).port;
  const client: any = http2.connect("http://127.0.0.1:" + port);
  const req: any = client.request({ ":path": "/big" });
  let got = 0;
  req.on("data", (d: Buffer) => {
    got += d.length;
  });
  req.end();
  if (!(await waitEvent(req, "response", 1000))) console.log("!! no response event");
  req.pause();
  const atPause = got;
  await sleep(350);
  console.log("paused: bytes at pause =", atPause, "after 350ms still paused =", got, "grew =", got > atPause);
  req.resume();
  if (!(await waitEvent(req, "end", 2000))) console.log("!! stream never ended");
  console.log("resumed: total =", got, "expected =", body.length, "exact =", got === body.length);
  client.close();
  await barrier(new Promise<void>((r) => server.close(() => r())), 500);
}
