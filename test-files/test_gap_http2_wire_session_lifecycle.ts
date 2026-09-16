// @covers node:http2 session.close() vs destroy() on the wire (#10327)
//
// ORACLE: Node 26.5.1. `close()` and `destroy()` differ in what they put on
// the wire and in the state they leave behind, and both differences are
// observable only against a real peer.
//
// Established against Node:
//   * `close(cb)` on an IDLE session writes TWO GOAWAY frames (nghttp2's
//     shutdown notice then the real one), fires 'close' and then the callback,
//     and ends with closed=true AND destroyed=true;
//   * `close()` while a stream is open writes ONE GOAWAY and leaves
//     closed=true, destroyed=false — the session is draining, not gone;
//   * `destroy()` writes ONE GOAWAY(NO_ERROR) and leaves closed=FALSE,
//     destroyed=true — `closed` is not a superset of `destroyed`;
//   * `destroy(error, code)` writes GOAWAY with that error code and re-emits
//     the error on the session;
//   * a session destroyed without an error emits 'close' but no 'error'.
import http2 from "node:http2";
import {
  RawServerPeer,
  dump,
  sleep,
  waitEvent,
} from "./_helpers/h2_wire.ts";

async function lifecycle(label: string, act: (client: any, events: string[]) => void, withStream: boolean): Promise<void> {
  const peer = new RawServerPeer();
  const port = await peer.listen();
  const client: any = http2.connect("http://127.0.0.1:" + port);
  const events: string[] = [];
  client.on("close", () => events.push("close"));
  client.on("error", (e: any) => events.push("error code=" + e.code + " message=" + e.message));
  if (!(await waitEvent(client, "connect", 800))) console.log("!! client never emitted connect");
  await sleep(100);
  if (withStream) {
    const stream: any = client.request({ ":path": "/hold" });
    stream.on("error", (e: any) => events.push("stream error " + e.code));
    await sleep(100);
  }
  peer.take();
  act(client, events);
  await sleep(250);
  console.log("== " + label + " ==");
  dump("  wire", peer.take());
  console.log("  events:", JSON.stringify(events));
  console.log("  closed:", client.closed, "destroyed:", client.destroyed);
  client.destroy();
  peer.close();
}

await lifecycle("close(cb) on an idle session", (c, ev) => {
  c.close(() => ev.push("close callback"));
}, false);

await lifecycle("close() with a stream open", (c) => {
  c.close();
}, true);

await lifecycle("destroy()", (c) => {
  c.destroy();
}, false);

await lifecycle("destroy(new Error('boom'), NGHTTP2_PROTOCOL_ERROR)", (c) => {
  c.destroy(new Error("boom"), http2.constants.NGHTTP2_PROTOCOL_ERROR);
}, false);
