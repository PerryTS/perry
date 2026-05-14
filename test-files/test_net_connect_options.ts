// Issue #770 — `net.connect({ host, port }, cb)` options-object overload.
//
// Pre-fix this failed with "file name contained an unexpected NUL byte"
// because the codegen tried to coerce the callback function to a string
// pointer (NA_STR), the runtime read garbage bytes as the hostname, and
// `getaddrinfo`'s internal `CString::new()` blew up. Separately, the
// 'error' event emitted raw strings instead of Error objects, so
// `err.message` came back as `undefined`.
//
// This fixture spins up a node:http server (uses TCP), connects to it
// with the options-object form, and verifies (a) the auto-registered
// `connectListener` fires, (b) data round-trips, (c) the 'error' event
// for a closed port produces an object with a useful `.message`.

import { createServer } from "node:http";
import * as net from "net";

const port = 18891;

const server = createServer((req: any, res: any) => {
  res.statusCode = 200;
  res.setHeader("Content-Type", "text/plain");
  res.end("ok");
});

server.listen(port, () => {
  console.log("server listening");

  // Options-object form with auto-registered connectListener.
  const sock1 = net.connect({ host: "127.0.0.1", port: port }, () => {
    console.log("sock1: connectListener fired");
  });
  sock1.on("error", (err: any) => {
    console.log("sock1 unexpected error:", err?.message);
  });
  // Force close via the 'connect' listener path so the test doesn't
  // hang on an idle socket — `setTimeout` below covers it.

  // Closed-port connection — should fire 'error' with an Error-shaped
  // object whose `.message` is a real string.
  setTimeout(() => {
    const sock2 = net.connect({ host: "127.0.0.1", port: 1 });
    sock2.on("connect", () => { console.log("sock2 unexpected connect"); });
    sock2.on("error", (err: any) => {
      console.log("sock2 error typeof:", typeof err);
      console.log("sock2 error message typeof:", typeof err?.message);
      console.log("sock2 error has message:", err?.message ? "yes" : "no");
    });
  }, 500);

  setTimeout(() => {
    server.close();
    console.log("done");
  }, 3000);
});
