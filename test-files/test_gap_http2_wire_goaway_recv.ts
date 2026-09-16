// @covers node:http2 'goaway' event argument shape and post-GOAWAY session state (#10327)
//
// ORACLE: Node 26.5.1. Perry's `SessionGoaway` pump arm (perry-ext-http
// `server/http2_server/pump.rs`) always builds a Buffer for the third
// argument, even when the sender supplied no opaque data, and it never changes
// the receiving session's state. Node distinguishes both.
//
// Established against Node, with a raw peer that writes the GOAWAY frame:
//   * `'goaway'` fires with `(code, lastStreamID, opaqueData)`;
//   * opaqueData is a Buffer when the frame carried opaque bytes and
//     `undefined` when it did not — NOT a zero-length Buffer;
//   * with no streams open, a received GOAWAY closes AND destroys the session,
//     and the session answers with its own GOAWAY before the socket goes away;
//   * this happens for a GRACEFUL GOAWAY (NO_ERROR, lastStreamID 2^31-1) too:
//     "graceful" does not keep an otherwise idle session alive;
//   * a `request()` issued after that returns a stream that is already closed
//     with rstCode 2 and emits ERR_HTTP2_INVALID_SESSION.
import http2 from "node:http2";
import {
  RawServerPeer,
  dump,
  sleep,
  frame,
  goawayPayload,
  FRAME_GOAWAY,
  waitEvent,
} from "./_helpers/h2_wire.ts";
import { Buffer } from "node:buffer";

async function goawayCase(
  label: string,
  lastStreamId: number,
  code: number,
  opaque?: Buffer,
): Promise<void> {
  const peer = new RawServerPeer();
  const port = await peer.listen();
  const client: any = http2.connect("http://127.0.0.1:" + port);
  const events: string[] = [];
  client.on("goaway", (c: number, last: number, data: any) => {
    events.push(
      "goaway code=" + c + " last=" + last +
      " dataType=" + (data === undefined ? "undefined" : (Buffer.isBuffer(data) ? "Buffer(" + data.length + ")" : typeof data)) +
      (data === undefined ? "" : " data=" + JSON.stringify(data.toString("latin1"))),
    );
  });
  client.on("close", () => events.push("session close"));
  client.on("error", (e: any) => events.push("session error " + e.code));
  if (!(await waitEvent(client, "connect", 800))) console.log("!! client never emitted connect");
  await sleep(120);
  peer.take();

  peer.send(frame(FRAME_GOAWAY, 0, 0, goawayPayload(lastStreamId, code, opaque)));
  await sleep(250);
  console.log("== " + label + " ==");
  console.log("  events:", JSON.stringify(events));
  console.log("  closed:", client.closed, "destroyed:", client.destroyed);
  dump("  wire written back", peer.take());
  client.destroy();
  peer.close();
}

await goawayCase("GOAWAY(last=0, NO_ERROR, 'byebye')", 0, 0, Buffer.from("byebye", "latin1"));
await goawayCase("GOAWAY(last=0, NO_ERROR, no opaque data)", 0, 0);
await goawayCase("GOAWAY(last=2147483647, NO_ERROR) — the graceful sentinel", 2147483647, 0);
await goawayCase("GOAWAY(last=0, ENHANCE_YOUR_CALM=11)", 0, 11);

// A request issued after a received GOAWAY.
{
  const peer = new RawServerPeer();
  const port = await peer.listen();
  const client: any = http2.connect("http://127.0.0.1:" + port);
  if (!(await waitEvent(client, "connect", 800))) console.log("!! client never emitted connect");
  await sleep(120);
  peer.take();
  peer.send(frame(FRAME_GOAWAY, 0, 0, goawayPayload(0, 0)));
  await sleep(200);
  const notes: string[] = [];
  const stream: any = client.request({ ":path": "/after-goaway" });
  stream.on("error", (e: any) => notes.push("stream error " + e.code + " | " + e.message));
  await sleep(200);
  console.log("== request() after a received GOAWAY ==");
  console.log("  notes:", JSON.stringify(notes));
  console.log("  stream.id:", stream.id, "closed:", stream.closed, "rstCode:", stream.rstCode);
  dump("  wire", peer.take());
  client.destroy();
  peer.close();
}
