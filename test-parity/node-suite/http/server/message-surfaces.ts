import http from "node:http";

const server = http.createServer((req: any, res: any) => {
  const signal = req.signal;
  console.log(
    "req method types:",
    [
      typeof req.setTimeout,
      typeof req.socket,
      typeof req.connection,
      req.socket === req.connection,
      typeof req.signal,
    ].join("|"),
  );
  console.log(
    "req signal:",
    [typeof signal, signal === req.signal, signal.aborted, typeof signal.reason].join("|"),
  );
  req.on("close", () => {
    console.log("req close signal:", req.signal.aborted);
  });
  console.log(
    "req meta:",
    [
      req.method,
      req.url,
      req.httpVersion,
      req.complete,
      req.aborted,
      req.destroyed,
    ].join("|"),
  );
  console.log(
    "req headers:",
    [
      req.headers["x-one"],
      Array.isArray(req.headersDistinct["x-one"]),
      req.headersDistinct["x-one"][0],
    ].join("|"),
  );
  console.log(
    "req raw trailers:",
    [
      Array.isArray(req.rawHeaders),
      req.rawHeaders.includes("alpha"),
      typeof req.trailers,
      typeof req.trailersDistinct,
      Array.isArray(req.rawTrailers),
      req.rawTrailers.length,
    ].join("|"),
  );
  console.log(
    "req socket remote:",
    [
      typeof req.socket.remoteAddress,
      typeof req.socket.remotePort,
      typeof req.connection.remoteAddress,
      typeof req.connection.remotePort,
    ].join("|"),
  );
  console.log("req setTimeout self:", req.setTimeout(1) === req);

  console.log(
    "res method types:",
    [
      typeof res.getHeaderNames,
      typeof res.getHeaders,
      typeof res.appendHeader,
      typeof res.setHeaders,
      typeof res.cork,
      typeof res.uncork,
      typeof res.setTimeout,
      typeof res.writeEarlyHints,
    ].join("|"),
  );
  console.log(
    "res defaults:",
    [
      res.headersSent,
      res.writableEnded,
      res.writableFinished,
      res.finished,
      res.sendDate,
      res.strictContentLength,
      typeof res.statusMessage,
    ].join("|"),
  );
  console.log(
    "res aliases:",
    [
      typeof res.socket,
      typeof res.connection,
      res.socket === res.connection,
      typeof res.req,
      res.req === req,
    ].join("|"),
  );
  console.log(
    "res controls:",
    [
      String(res.cork()),
      String(res.uncork()),
      res.setTimeout(1) === res,
      String(res.writeEarlyHints({ "X-Early": "1" })),
    ].join("|"),
  );

  console.log("res setHeader self:", res.setHeader("X-Foo", "one") === res);
  console.log("res hasHeader:", res.hasHeader("x-foo"));
  console.log("res names include:", res.getHeaderNames().includes("x-foo"));
  console.log("res headers value:", String(res.getHeaders()["x-foo"]));
  console.log("res getHeader:", String(res.getHeader("X-Foo")));
  console.log("res append self:", res.appendHeader("X-Foo", "two") === res);
  console.log("res appended value:", String(res.getHeader("x-foo")));

  res.sendDate = false;
  res.strictContentLength = true;
  console.log(
    "res assigned:",
    [res.sendDate, res.strictContentLength].join("|"),
  );

  res.end("ok");
});

server.listen(0, "127.0.0.1", () => {
  const addr = server.address();
  const port = typeof addr === "object" && addr !== null ? addr.port : 0;
  const req = http.request(
    {
      hostname: "127.0.0.1",
      port,
      method: "POST",
      path: "/p?q=1",
      headers: {
        "X-One": "alpha",
      },
    },
    (res: any) => {
      res.on("data", () => {});
      res.on("end", () => {
        server.close(() => console.log("closed"));
      });
    },
  );
  req.end("body");
});

setTimeout(() => {}, 1500);
