// @covers node:http2 RST_STREAM(REFUSED_STREAM) vs GOAWAY on maxConcurrentStreams (#10327)
//
// ORACLE: Node 26.5.1. REFUSED_STREAM is real, but its trigger is the
// SETTINGS_MAX_CONCURRENT_STREAMS limit, not a preceding GOAWAY — after a
// GOAWAY, Node silently ignores the extra stream instead (see
// test_gap_http2_wire_goaway_server.ts).
//
// And the limit has TWO regimes, which is the part no reimplementation guesses:
//
//   * BEFORE the peer ACKs the server's SETTINGS the limit is not yet binding,
//     so exceeding it is tolerated and each excess stream is answered with
//     RST_STREAM code=7 (NGHTTP2_REFUSED_STREAM) while the session survives;
//   * AFTER the peer has ACKed it, exceeding the limit is a protocol
//     violation, and nghttp2 answers RST_STREAM code=2 on the FIRST stream
//     followed by GOAWAY code=2 (INTERNAL_ERROR), tearing the connection down.
//     The session error surfaces as ERR_HTTP2_ERROR with errno -505
//     (NGHTTP2_ERR_PROTO).
//
// Also established here:
//   * refused streams never reach the 'stream' handler;
//   * in the tolerated regime, finishing a held stream frees a slot and a
//     later stream id is accepted normally;
//   * a client whose own stream is RST'd by the peer sees the stream close
//     with that rstCode while its SIBLING streams keep working and the
//     session stays up.
//
// None of this is reachable through Perry's loopback control surface: the peer
// is a raw socket, so there is no in-process `Http2SessionHandle` to synthesise
// events at.
import http2 from "node:http2";
import {
  RawClientPeer,
  RawServerPeer,
  dump,
  sleep,
  requestFrame,
  frame,
  rstPayload,
  FRAME_RST_STREAM,
  FRAME_SETTINGS,
  FLAG_ACK,
  barrier,
  waitEvent,
} from "./_helpers/h2_wire.ts";

// ---- part 1: limit NOT yet acknowledged -> polite REFUSED_STREAM ----
{
  const server = http2.createServer({ settings: { maxConcurrentStreams: 2 } });
  const accepted: string[] = [];
  const held: any[] = [];
  server.on("stream", (stream: any, headers: any) => {
    accepted.push("id=" + stream.id + " path=" + headers[":path"]);
    stream.on("error", () => {});
    held.push(stream);
  });
  await new Promise<void>((r) => server.listen(0, "127.0.0.1", () => r()));
  const port = (server.address() as any).port;
  const peer = await RawClientPeer.connect(port);
  peer.autoAck = false; // leave the server's SETTINGS unacknowledged
  await sleep(200);
  dump("server SETTINGS advertising maxConcurrentStreams=2", peer.take());

  peer.send(requestFrame(1, "/a"));
  peer.send(requestFrame(3, "/b"));
  peer.send(requestFrame(5, "/c"));
  peer.send(requestFrame(7, "/d"));
  await sleep(350);
  dump("four concurrent streams, limit NOT yet acknowledged", peer.take());
  console.log("  streams the handler saw:", JSON.stringify(accepted));
  console.log("  peer socket closed:", peer.closed);

  held[0].respond({ ":status": 200 });
  held[0].end("done");
  await sleep(250);
  peer.take();
  peer.send(requestFrame(9, "/e"));
  await sleep(300);
  dump("stream 9 after one slot was freed", peer.take());
  console.log("  streams the handler saw:", JSON.stringify(accepted));
  console.log("  peer socket closed:", peer.closed);

  peer.destroy();
  await barrier(new Promise<void>((r) => server.close(() => r())), 500);
}

// ---- part 2: limit ACKNOWLEDGED -> protocol error, session torn down ----
{
  const server = http2.createServer({ settings: { maxConcurrentStreams: 2 } });
  const accepted: string[] = [];
  const notes: string[] = [];
  server.on("session", (s: any) => {
    s.on("error", (e: any) => notes.push("session error " + e.code + " errno=" + e.errno + " | " + e.message));
    s.on("close", () => notes.push("session close"));
  });
  server.on("stream", (stream: any, headers: any) => {
    accepted.push("id=" + stream.id);
    stream.on("error", (e: any) => notes.push("stream " + stream.id + " error " + e.code));
  });
  await new Promise<void>((r) => server.listen(0, "127.0.0.1", () => r()));
  const port = (server.address() as any).port;
  const peer = await RawClientPeer.connect(port);
  peer.autoAck = false;
  await sleep(200);
  peer.send(frame(FRAME_SETTINGS, FLAG_ACK, 0)); // acknowledge the limit
  await sleep(120);
  peer.take();

  peer.send(requestFrame(1, "/a"));
  peer.send(requestFrame(3, "/b"));
  peer.send(requestFrame(5, "/c"));
  peer.send(requestFrame(7, "/d"));
  await sleep(350);
  dump("four concurrent streams, limit ACKNOWLEDGED", peer.take());
  console.log("  streams the handler saw:", JSON.stringify(accepted));
  console.log("  notes:", JSON.stringify(notes));
  console.log("  peer socket closed:", peer.closed);

  peer.destroy();
  await barrier(new Promise<void>((r) => server.close(() => r())), 500);
}

// ---- part 3: a client stream RST'd by the peer, siblings unaffected ----
{
  const peer = new RawServerPeer();
  const port = await peer.listen();
  const client: any = http2.connect("http://127.0.0.1:" + port);
  if (!(await waitEvent(client, "connect", 800))) console.log("!! client never emitted connect");
  const notes: string[] = [];
  const a: any = client.request({ ":path": "/a" });
  const b: any = client.request({ ":path": "/b" });
  a.on("error", (e: any) => notes.push("a error " + e.code + " | " + e.message));
  a.on("close", () => notes.push("a close rstCode=" + a.rstCode + " closed=" + a.closed));
  b.on("error", (e: any) => notes.push("b error " + e.code));
  b.on("close", () => notes.push("b close rstCode=" + b.rstCode));
  await sleep(200);
  console.log("== client streams opened: a.id=" + a.id + " b.id=" + b.id + " ==");
  peer.take();
  peer.send(frame(FRAME_RST_STREAM, 0, a.id, rstPayload(http2.constants.NGHTTP2_CANCEL)));
  await sleep(250);
  console.log("  notes:", JSON.stringify(notes));
  console.log("  session destroyed:", client.destroyed, "closed:", client.closed);
  console.log("  sibling b still open:", b.closed === false);
  dump("  wire", peer.take());
  client.destroy();
  peer.close();
}
