// @covers node:http2 server-side GOAWAY: graceful shutdown that KEEPS the session (#10327)
//
// ORACLE: Node 26.5.1. This fixture answers the question the loopback
// simulation cannot even pose: what does a REAL HTTP/2 server do after a
// graceful GOAWAY, and what happens to a stream opened after it?
//
// The widely-repeated claim is that the server answers such a stream with
// RST_STREAM(REFUSED_STREAM). Measured against Node that is FALSE — nghttp2
// simply IGNORES a HEADERS frame for a stream id above the GOAWAY's
// lastStreamID: no RST_STREAM, no 'stream' event, no error, and the session
// stays up. RFC 7540 §6.8 permits either; Node picked "ignore". Perry must
// match Node, not the folklore. (REFUSED_STREAM is real, but it belongs to a
// different trigger — see test_gap_http2_wire_refused_stream.ts.)
//
// Established against Node:
//   * `session.goaway(NO_ERROR, lastStreamID, opaqueData)` on a SERVER session
//     writes exactly one GOAWAY frame carrying the opaque bytes;
//   * the session is NOT closed or destroyed afterwards and the socket stays
//     open — already-open streams keep working;
//   * a HEADERS frame for a NEW stream id after that GOAWAY produces no frame
//     at all and no 'stream' event;
//   * `server.close()` writes TWO GOAWAY frames (nghttp2's shutdown notice
//     followed by the real one), both with lastStreamID = the last stream the
//     server actually processed;
//   * a client GOAWAY received while a stream is open fires the server
//     session's 'goaway' event, is answered with the server's own GOAWAY, and
//     does NOT tear the connection down.
import http2 from "node:http2";
import {
  RawClientPeer,
  dump,
  sleep,
  requestFrame,
  frame,
  goawayPayload,
  FRAME_GOAWAY,
  barrier,
} from "./_helpers/h2_wire.ts";
import { Buffer } from "node:buffer";

// ---- part 1: explicit server-side goaway, then a stream after it ----
{
  const server = http2.createServer();
  let session: any = null;
  const notes: string[] = [];
  server.on("session", (s: any) => {
    if (session === null) session = s;
    s.on("goaway", (c: number, last: number, d: any) => {
      notes.push("server 'goaway' code=" + c + " last=" + last +
        " data=" + (d === undefined ? "undefined" : JSON.stringify(d.toString("latin1"))));
    });
    s.on("close", () => notes.push("server session close"));
    s.on("error", (e: any) => notes.push("server session error " + e.code));
  });
  server.on("stream", (stream: any, headers: any) => {
    notes.push("server 'stream' id=" + stream.id + " path=" + headers[":path"]);
    stream.on("error", (e: any) => notes.push("stream " + stream.id + " error " + e.code));
    if (headers[":path"] === "/hold") return; // never responded: keeps the session busy
    stream.respond({ ":status": 200 });
    stream.end("ok");
  });
  await new Promise<void>((r) => server.listen(0, "127.0.0.1", () => r()));
  const port = (server.address() as any).port;
  const peer = await RawClientPeer.connect(port);
  await sleep(150);
  peer.send(requestFrame(1, "/hold", "POST", false));
  await sleep(200);
  peer.take();

  session.goaway(http2.constants.NGHTTP2_NO_ERROR, 1, Buffer.from("draining", "latin1"));
  await sleep(200);
  dump("server session.goaway(NO_ERROR, 1, 'draining')", peer.take());
  console.log("  session closed:", session.closed, "destroyed:", session.destroyed);
  console.log("  peer socket closed:", peer.closed);

  peer.send(requestFrame(3, "/after-goaway"));
  await sleep(300);
  dump("HEADERS for stream 3 AFTER the graceful GOAWAY", peer.take());
  console.log("  peer socket closed:", peer.closed, "session destroyed:", session.destroyed);
  console.log("  notes:", JSON.stringify(notes));

  peer.destroy();
  await barrier(new Promise<void>((r) => server.close(() => r())), 500);
}

// ---- part 2: server.close() is a two-frame graceful shutdown ----
{
  const server = http2.createServer();
  server.on("stream", (stream: any) => {
    stream.respond({ ":status": 200 });
    stream.end("x");
  });
  await new Promise<void>((r) => server.listen(0, "127.0.0.1", () => r()));
  const port = (server.address() as any).port;
  const peer = await RawClientPeer.connect(port);
  await sleep(150);
  peer.send(requestFrame(1, "/one"));
  await sleep(250);
  peer.take();
  server.close();
  await sleep(300);
  dump("server.close() after one completed stream", peer.take());
  console.log("  peer socket closed:", peer.closed);
  peer.destroy();
}

// ---- part 3: the server RECEIVES a client GOAWAY while a stream is open ----
{
  const server = http2.createServer();
  const notes: string[] = [];
  server.on("session", (s: any) => {
    s.on("goaway", (c: number, last: number, d: any) => {
      notes.push("server 'goaway' code=" + c + " last=" + last +
        " data=" + (d === undefined ? "undefined" : JSON.stringify(d.toString("latin1"))));
    });
    s.on("close", () => notes.push("server session close"));
    s.on("error", (e: any) => notes.push("server session error " + e.code));
  });
  server.on("stream", (stream: any, headers: any) => {
    notes.push("server 'stream' id=" + stream.id);
    stream.on("error", (e: any) => notes.push("stream error " + e.code));
    if (headers[":path"] === "/hold") return;
    stream.respond({ ":status": 200 });
    stream.end("x");
  });
  await new Promise<void>((r) => server.listen(0, "127.0.0.1", () => r()));
  const port = (server.address() as any).port;
  const peer = await RawClientPeer.connect(port);
  await sleep(150);
  peer.send(requestFrame(1, "/hold", "POST", false));
  await sleep(200);
  peer.take();
  peer.send(frame(FRAME_GOAWAY, 0, 0, goawayPayload(0, 0, Buffer.from("bye", "latin1"))));
  await sleep(300);
  dump("server reply to a client GOAWAY with stream 1 open", peer.take());
  console.log("  notes:", JSON.stringify(notes));
  console.log("  peer socket closed:", peer.closed);
  peer.destroy();
  await barrier(new Promise<void>((r) => server.close(() => r())), 500);
}
