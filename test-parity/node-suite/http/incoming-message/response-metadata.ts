import { createServer, get } from "node:http";

const server = createServer((_req, res) => {
  res.writeHead(207, "Multi", { "X-One": "one" });
  res.end("body");
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
  await new Promise<void>((resolve, reject) => {
    get({ hostname: "127.0.0.1", port: address.port }, (res) => {
      console.log(
        "meta:",
        res.statusCode,
        res.statusMessage,
        res.httpVersion,
        res.method,
        res.url,
      );
      console.log(
        "header:",
        res.headers["x-one"],
        res.rawHeaders.includes("X-One"),
      );
      console.log("start state:", res.complete, res.destroyed);
      res.resume();
      res.on("end", () => {
        console.log("end state:", res.complete, res.readableEnded);
        resolve();
      });
    }).once("error", reject);
  });
} finally {
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
