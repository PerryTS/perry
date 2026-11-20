// #2041 acceptance fixture — the exact shape from the issue report: a chained
// `createServer(...).listen(port, host, cb)` with a three-argument `listen`
// (port number, host string, completion callback).
//
// Pre-#2041 this compiled and linked cleanly but the binary exited before
// serving: the inline-factory receiver was never tagged `HttpServer`, so the
// chained `.listen(...)` never dispatched to the native listen fn — which also
// meant the event-loop's active-handle pump was never registered, so `main()`
// returned and the process terminated in <1s. On top of that, the 3-arg
// overload mis-routed the host string into the callback slot and dropped the
// real callback. The driver fixture.sh launches this, curls it, and asserts a
// 200 "ok".
import { createServer } from "node:http";

createServer((_req, res) => {
  res.statusCode = 200;
  res.setHeader("content-type", "text/plain");
  res.end("ok");
}).listen(44599, "127.0.0.1", () => console.log("listening on 44599"));
