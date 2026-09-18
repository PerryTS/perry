// @covers node:http2 end-to-end streams: multiplexing, trailers, per-stream errors (#10327)
//
// ORACLE: Node 26.5.1. Unlike the `_wire_` fixtures in this set, both ends
// here are the implementation under test, so Perry's real hyper/h2 stream path
// is what runs — the loopback control-surface shim is not involved. This is
// the regression floor the transport lane must not break while it replaces the
// control surface.
//
// Established against Node:
//   * four concurrent requests on ONE session complete with distinct odd
//     stream ids 1,3,5,7 in request order;
//   * a stream closed with `stream.close(NGHTTP2_INTERNAL_ERROR)` surfaces on
//     BOTH ends as 'error' ERR_HTTP2_STREAM_ERROR with rstCode 2 (an unhandled
//     'error' on the server stream takes the process down), and its SIBLINGS
//     on the same session complete normally;
//   * `respond(headers, { waitForTrailers: true })` + 'wantTrailers' +
//     `sendTrailers()` delivers a 'trailers' event on the client AFTER the
//     body and before 'end';
//   * response header casing, `:status` typing (number) and `sensitiveHeaders`
//     round-trip;
//   * `session.state.nextStreamID` advances by 2 per request.
import http2 from "node:http2";
import { Buffer } from "node:buffer";
import { barrier, waitEvent, withTimeout } from "./_helpers/h2_wire.ts";

function sleep(ms: number): Promise<void> {
  return new Promise<void>((resolve) => setTimeout(resolve, ms));
}

const server = http2.createServer();
server.on("stream", (stream: any, headers: any) => {
  const path = headers[":path"];
  if (path === "/rst") {
    // `close(code)` with a non-zero code makes the SERVER stream emit
    // ERR_HTTP2_STREAM_ERROR too, not just the client's; an unhandled 'error'
    // there takes the process down.
    stream.on("error", (e: any) => console.log("server stream error:", e.code, "|", e.message));
    stream.close(http2.constants.NGHTTP2_INTERNAL_ERROR);
    return;
  }
  if (path === "/trailers") {
    stream.on("wantTrailers", () => {
      stream.sendTrailers({ "x-trailer": "yes", "x-count": "42" });
    });
    stream.respond({ ":status": 200, "content-type": "text/plain" }, { waitForTrailers: true });
    stream.end("with-trailers");
    return;
  }
  stream.respond({ ":status": 200, "content-type": "text/plain", "x-path": path });
  stream.end("body:" + path);
});
await new Promise<void>((r) => server.listen(0, "127.0.0.1", () => r()));
const port = (server.address() as any).port;
const client: any = http2.connect("http://127.0.0.1:" + port);
if (!(await waitEvent(client, "connect", 800))) console.log("!! client never emitted connect");

console.log("nextStreamID before any request:", client.state.nextStreamID);
// The EventEmitter surface a session is expected to expose. `once` in
// particular is missing from perry-ext-http's http2 session dispatch, which is
// why the shared barrier in `_helpers/h2_wire.ts` uses `on` with a guard.
console.log(
  "session emitter surface:",
  ["on", "once", "addListener", "off", "removeListener", "emit", "removeAllListeners"]
    .map((m: string) => m + "=" + typeof (client as any)[m])
    .join(" "),
);

// ---- multiplexing ----
// Deliberately written against the narrowest stream surface that is already
// known to work end to end (`test-parity/node-suite/http2/plaintext/`): raw
// `'data'` Buffers concatenated by hand, an explicit `end()`, and `resume()`.
// No `setEncoding`, and no reliance on a GET's implicit end-of-stream — both
// are probed on their own lines below, as subjects rather than instruments, so
// a gap in either cannot make this fixture fail for the wrong reason.
function fetchPath(path: string): Promise<string> {
  return withTimeout<string>(new Promise<string>((resolve) => {
    const req: any = client.request({ ":path": path });
    let status: any = null;
    let xpath = "";
    const chunks: any[] = [];
    req.on("response", (h: any) => {
      status = h[":status"];
      xpath = h["x-path"];
    });
    req.on("data", (d: any) => {
      chunks.push(d);
    });
    req.on("end", () => {
      const body = Buffer.concat(chunks).toString("utf8");
      resolve("id=" + req.id + " status=" + status + " typeofStatus=" + typeof status + " x-path=" + xpath + " body=" + body);
    });
    req.resume();
    req.end();
  }), 2000, "!! " + path + " NEVER COMPLETED");
}
const all = await Promise.all([fetchPath("/a"), fetchPath("/b"), fetchPath("/c"), fetchPath("/d")]);
for (let i = 0; i < all.length; i++) console.log("multiplexed:", all[i]);
console.log("nextStreamID after four requests:", client.state.nextStreamID);

// The two stream-surface details `fetchPath` deliberately avoids depending on.
{
  const probe: any = client.request({ ":path": "/probe" });
  console.log(
    "stream surface: setEncoding=" + typeof probe.setEncoding +
    " resume=" + typeof probe.resume +
    " GET auto-ends=" + (probe.writableEnded === true),
  );
  probe.on("error", () => {});
  probe.resume();
  probe.end();
  await sleep(150);
}

// ---- one stream RST'd, siblings unaffected ----
{
  const notes: string[] = [];
  const bad: any = client.request({ ":path": "/rst" });
  const goodA = fetchPath("/sib-a");
  const goodB = fetchPath("/sib-b");
  bad.on("error", (e: any) => notes.push("bad error " + e.name + " " + e.code + " | " + e.message));
  bad.on("close", () => notes.push("bad close rstCode=" + bad.rstCode));
  bad.resume();
  await sleep(250);
  const sib = await Promise.all([goodA, goodB]);
  console.log("rst stream notes:", JSON.stringify(notes));
  console.log("sibling:", sib[0]);
  console.log("sibling:", sib[1]);
  console.log("session alive after a stream error: destroyed=" + client.destroyed + " closed=" + client.closed);
}

// ---- trailers ----
{
  const req: any = client.request({ ":path": "/trailers" });
  const order: string[] = [];
  let trailers = "";
  req.on("response", () => order.push("response"));
  req.setEncoding("utf8");
  req.on("data", () => order.push("data"));
  req.on("trailers", (t: any) => {
    order.push("trailers");
    trailers = JSON.stringify(t);
  });
  await barrier(new Promise<void>((r) => req.on("end", () => {
    order.push("end");
    r();
  })), 2000);
  console.log("trailer event order:", JSON.stringify(order));
  console.log("trailers:", trailers);
}

client.close();
await barrier(new Promise<void>((r) => server.close(() => r())), 500);
