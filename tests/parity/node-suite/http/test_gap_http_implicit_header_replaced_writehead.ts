// node's `_implicitHeader`: the first `write()` / `end()` / `flushHeaders()` of
// a response without a head runs `this.writeHead(this.statusCode)` as a
// PROPERTY call, so a program that replaced `res.writeHead` observes it.
// `on-headers` (used by `compression`, which Next.js's server installs) hooks
// the head that way to append `Vary: Accept-Encoding`; with the head committed
// natively the hook never ran and the header was missing.
import http from "node:http";

const server = http.createServer((req, res) => {
  const original = res.writeHead;
  let fired = 0;
  res.writeHead = function (this: http.ServerResponse, ...args: any[]) {
    fired++;
    this.setHeader("vary", "rsc, Accept-Encoding");
    this.setHeader("x-hook", String(fired));
    this.setHeader("x-status-arg", String(args[0]));
    return (original as any).apply(this, args);
  } as any;
  res.setHeader("content-type", "text/plain");
  res.statusCode = req.url === "/created" ? 201 : 200;
  if (req.url === "/write") {
    res.write("a");
    res.end("b");
  } else if (req.url === "/write-cb") {
    res.write("a", () => {});
    res.end("b");
  } else if (req.url === "/flush") {
    res.flushHeaders();
    res.end("flushed");
  } else if (req.url === "/explicit") {
    res.writeHead(202);
    res.end("explicit");
  } else {
    res.end("hello");
  }
});

server.listen(0, "127.0.0.1", async () => {
  const address = server.address();
  const port = typeof address === "object" && address ? address.port : 0;
  for (const path of ["/", "/created", "/write", "/write-cb", "/flush", "/explicit"]) {
    const r = await fetch(`http://127.0.0.1:${port}${path}`);
    console.log(
      path,
      r.status,
      r.headers.get("vary"),
      r.headers.get("x-hook"),
      r.headers.get("x-status-arg"),
      await r.text(),
    );
  }
  server.close();
});
