// Regression: an http `ServerResponse`'s `res.socket.writable` must be `true`
// while the response is still open.
//
// Node's `res.socket` is the Duplex TCP socket, whose `writable` is `true`
// during an active exchange. Perry aliases `res.socket` to the request handle
// (there is no separate socket object), so a property read of
// `res.socket.writable` resolves on the IncomingMessage. It used to be
// `undefined`, which broke the `on-finished` package's readiness probe:
//
//     isFinished(res) = Boolean(res.finished || (socket && !socket.writable))
//
// With `socket.writable === undefined`, `!socket.writable` was `true`, so
// `isFinished(res)` returned `true` the instant a stream was piped. The `send`
// package (Next.js static file serving, via `serve-static`) treats that as
// "response already finished" and destroys the piped read stream — truncating
// every static file past the first 64 KB (high-water-mark) chunk.
//
// The observable here is the exact `isFinished(res)` computation `on-finished`
// performs at pipe-setup time: it must be `false`. Also asserts the raw
// `res.socket.writable` / `res.finished` inputs so a future regression is easy
// to localize. Byte-for-byte identical to `node --experimental-strip-types`.

import { createServer, get } from "node:http";

const PORT = 18877;

const server = createServer((_req: any, res: any) => {
  const socket = res.socket;
  const socketPresent = !!socket;
  const socketWritable = socket ? socket.writable : "no-socket";
  const resFinished = res.finished;
  // The precise readiness check the `on-finished` package runs on a response.
  const isFinished = Boolean(resFinished || (socket && !socket.writable));

  console.log("socket_present=" + socketPresent);
  console.log("socket_writable=" + socketWritable);
  console.log("res_finished=" + resFinished);
  console.log("isFinished_at_setup=" + isFinished);

  res.end("ok");
});

server.listen(PORT, () => {
  get({ host: "localhost", port: PORT, path: "/" }, (res: any) => {
    res.on("data", () => {});
    res.on("end", () => {
      server.close();
      console.log("done");
    });
  });
});
