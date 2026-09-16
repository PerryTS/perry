// @covers node:http2 session.goaway(code, lastStreamID, opaqueData) on the wire (#10327)
//
// ORACLE: Node 26.5.1. Perry's `queue_session_goaway` (perry-ext-http
// `server/http2_server/controls.rs`) never encodes a GOAWAY frame — it scans
// the process handle table for a peer `Http2SessionHandle` and pushes a
// synthetic `SessionGoaway` event at it. Against a raw socket peer there is no
// such handle, so a Perry session prints "(none)" for every case below.
//
// Established against Node:
//   * `goaway()` with no arguments writes GOAWAY last=0 code=0 with an EMPTY
//     opaque field, i.e. exactly 8 payload bytes;
//   * `goaway(code)` writes that code with last=0;
//   * `goaway(code, lastStreamID)` writes both — but only when lastStreamID is
//     EVEN on a client session; an ODD lastStreamID (2^31-1 included) is
//     dropped silently, with no frame, no throw and no error event;
//   * `goaway(code, lastStreamID, opaqueData)` appends the opaque bytes
//     verbatim after the 8-byte header;
//   * sending a GOAWAY does NOT close or destroy the session: `closed` and
//     `destroyed` stay false and the socket stays up — a graceful GOAWAY is an
//     announcement, not a teardown;
//   * a non-Buffer `opaqueData` throws ERR_INVALID_ARG_TYPE.
import http2 from "node:http2";
import {
  RawServerPeer,
  dump,
  sleep,
  waitEvent,
} from "./_helpers/h2_wire.ts";
import { Buffer } from "node:buffer";

async function session(): Promise<any[]> {
  const peer = new RawServerPeer();
  const port = await peer.listen();
  const client: any = http2.connect("http://127.0.0.1:" + port);
  if (!(await waitEvent(client, "connect", 800))) console.log("!! client never emitted connect");
  await sleep(100);
  peer.take();
  return [client, peer];
}

// A GOAWAY does not close the session, so the code/opaque variants share one
// connection. (The lastStreamID table below cannot: nghttp2 clamps a later
// GOAWAY's lastStreamID to be non-increasing, so each row needs a fresh
// session to show its own value.)
{
  const pair = await session();
  const client = pair[0];
  const peer = pair[1];

  client.goaway();
  await sleep(120);
  dump("goaway()", peer.take());
  console.log("  closed:", client.closed, "destroyed:", client.destroyed);

  client.goaway(http2.constants.NGHTTP2_ENHANCE_YOUR_CALM);
  await sleep(120);
  dump("goaway(NGHTTP2_ENHANCE_YOUR_CALM)", peer.take());
  console.log("  closed:", client.closed, "destroyed:", client.destroyed);

  client.goaway(http2.constants.NGHTTP2_NO_ERROR, 0, Buffer.from("shutting-down", "latin1"));
  await sleep(120);
  dump("goaway(NO_ERROR, 0, Buffer('shutting-down'))", peer.take());
  console.log("  closed:", client.closed, "destroyed:", client.destroyed);
  console.log("  socket still writable:", client.socket.writable === true);

  try {
    client.goaway(0, 0, "bye");
    console.log("goaway(0, 0, 'bye') -> NO THROW");
  } catch (e: any) {
    console.log("goaway(0, 0, 'bye') ->", e.name, "|", e.code, "|", e.message);
  }

  client.destroy();
  peer.close();
}

// lastStreamID parity: from a CLIENT session nghttp2 will only emit a GOAWAY
// whose lastStreamID is EVEN (a server-initiated / push stream). An odd value
// — including 2^31-1, the "graceful shutdown" sentinel every HTTP/2 tutorial
// reaches for — is dropped SILENTLY: no frame, no throw, no error event.
for (const last of [0, 2, 1, 2147483647]) {
  const pair = await session();
  pair[0].goaway(http2.constants.NGHTTP2_NO_ERROR, last);
  await sleep(120);
  dump("client goaway(NO_ERROR, " + last + ")", pair[1].take());
  pair[0].destroy();
  pair[1].close();
}
