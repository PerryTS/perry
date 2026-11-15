// Stream and emitter methods on live http server handles: the request's
// inherited `on` / `setEncoding` / `pipe` reads (the generic property read
// of an http handle compares them with the stream intrinsics), a body read
// through 'data' / 'end', and the response's write / end. (`req.on ===
// http.IncomingMessage.prototype.on` is false in Perry, true in node, on
// main as well: not asserted here.)
import * as http from "node:http";
import { PassThrough } from "node:stream";

const server = http.createServer((req, res) => {
  console.log("server", typeof req.on, typeof req.pipe, typeof req.setEncoding, typeof res.write);
  req.setEncoding("utf8");
  const chunks: string[] = [];
  req.on("data", (c: string) => chunks.push(c));
  req.on("end", () => {
    console.log("body", chunks.join(""));
    res.write("part1,");
    res.end("part2");
  });
});
server.listen(0, () => {
  const port = (server.address() as { port: number }).port;
  const req = http.request({ port, method: "POST", path: "/" }, (resp) => {
    const sink = new PassThrough();
    const got: string[] = [];
    sink.on("data", (c: Buffer) => got.push(c.toString()));
    sink.on("end", () => {
      console.log("client", resp.statusCode, got.join(""));
      server.close();
    });
    resp.pipe(sink);
  });
  req.write("hello ");
  req.end("world");
});
