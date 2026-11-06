// #10441/#10442/#10444/#10465 — the `net.Socket` surface cluster the package
// audit hit while compiling real socket-backed npm drivers (mysql2, pg,
// redis, ws) natively instead of through Perry's hand-written bindings:
//
//   #10441 — prependListener/prependOnceListener were missing entirely
//            (silently did nothing; ioredis/iovalkey's RESP parser attach
//            via `stream.prependListener("data", …)` never saw a byte).
//   #10442 — on()/addListener() returned `undefined` on a TYPED `net.Socket`
//            receiver, breaking `sock.on(...).on(...)` chaining.
//   #10444 — pipe() didn't exist on `net.Socket` at all (mongodb's
//            Connection constructor does `.pipe(new SizedMessageTransform)`).
//   #10465 — writable/readable/_writableState/_readableState were missing,
//            and readyState/connecting/pending/destroyed didn't track the
//            real connect/end/close lifecycle.
//
// One flow exercises all four against a real loopback echo server so the
// output is compared byte-for-byte against Node instead of spot-checked.

import * as net from "node:net";
import { PassThrough } from "node:stream";

const server = net.createServer((conn) => {
  conn.on("data", (d) => conn.write(d));
  // Explicit half-close instead of relying on Node's default
  // allowHalfOpen=false auto-end, so this test only exercises the four
  // issues above, not the server's own half-open behavior.
  conn.on("end", () => conn.end());
});

function show(label: string, s: net.Socket) {
  console.log(
    label.padEnd(12),
    "writable=" + s.writable,
    "readable=" + s.readable,
    "readyState=" + s.readyState,
    "connecting=" + s.connecting,
    "pending=" + s.pending,
    "destroyed=" + s.destroyed,
  );
}

server.listen(0, "127.0.0.1", () => {
  const port = (server.address() as net.AddressInfo).port;

  // ── #10465: a never-connected socket ──────────────────────────────────
  const fresh = new net.Socket();
  show("new Socket", fresh);
  console.log(
    "new Socket   _writableState=" + typeof fresh._writableState,
    "_readableState=" + typeof fresh._readableState,
  );
  fresh.destroy();

  // Typed receiver end to end — `net.Socket`, not `any` — since #10442's
  // defect only reproduced on a statically typed receiver.
  const sock: net.Socket = net.connect(port, "127.0.0.1");
  show("connecting", sock);

  // ── #10442: on()/addListener() return value + chaining ────────────────
  console.log("on() returns socket:", sock.on("noop-event", () => {}) === sock);
  console.log(
    "addListener() returns socket:",
    sock.addListener("noop-event", () => {}) === sock,
  );
  try {
    sock.on("__chain_a", () => {}).on("__chain_b", () => {});
    console.log("chained on().on() ok");
  } catch (e: any) {
    console.log("chained on().on() threw:", e.message);
  }

  // ── #10441: prependListener/prependOnceListener ────────────────────────
  const order: string[] = [];
  sock.on("data", () => order.push("normal"));
  const prependRet = sock.prependListener("data", () => order.push("prepend"));
  console.log("prependListener returns socket:", prependRet === sock);
  const prependOnceRet = sock.prependOnceListener("data", () => order.push("prependOnce"));
  console.log("prependOnceListener returns socket:", prependOnceRet === sock);

  sock.on("connect", () => {
    show("connected", sock);
    const anySock: any = sock;
    console.log(
      "untyped read:",
      "writable=" + anySock.writable,
      "readable=" + anySock.readable,
      "readyState=" + anySock.readyState,
    );

    sock.once("data", (chunk: Buffer) => {
      console.log("data order  :", order.join(","));
      console.log("data payload:", JSON.stringify(chunk.toString()));
      // Clear the marker listeners before wiring pipe() below so they don't
      // also fire on the piped chunk.
      sock.removeAllListeners("data");

      // ── #10444: pipe() ──────────────────────────────────────────────
      const dest = new PassThrough();
      const pipeRet = sock.pipe(dest);
      console.log("pipe() returns dest:", pipeRet === dest);
      dest.on("data", (piped: Buffer) => {
        console.log("piped payload:", JSON.stringify(piped.toString()));
        show("mid-stream", sock);
        sock.end();
      });
      sock.write("piped-chunk");
    });

    sock.write("first-chunk");
  });

  sock.on("end", () => {
    show("'end'", sock);
  });

  sock.on("close", () => {
    show("'close'", sock);
    server.close();
  });

  sock.on("error", (e: any) => {
    console.log("socket error:", e.message);
    server.close();
  });
});
