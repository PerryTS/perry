import { createServer, request } from "node:http";

const events: string[] = [];
const server = createServer((_req, res) => {
  events.push("request");
  res.end("request");
});
server.on("checkContinue", (req, res) => {
  events.push(`continue:${req.headers.expect}`);
  res.writeContinue();
  res.end("continued");
});
server.on("checkExpectation", (req, res) => {
  events.push(`expectation:${req.headers.expect}`);
  res.end("expected");
});

await new Promise<void>((resolve, reject) => {
  server.once("error", reject);
  server.listen(0, "127.0.0.1", resolve);
});

try {
  const address = server.address();
  if (!address || typeof address === "string") {
    throw new Error("missing address");
  }
  for (const expectation of ["100-continue", "custom"]) {
    await new Promise<void>((resolve, reject) => {
      const req = request({
        hostname: "127.0.0.1",
        port: address.port,
        method: "POST",
        headers: { Expect: expectation },
      }, (res) => {
        res.once("error", reject);
        res.resume();
        res.on("end", resolve);
      });
      req.once("error", reject);
      req.end();
    });
  }
  console.log("events:", events.join("|"));
} finally {
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
