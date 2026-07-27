import { createServer, get } from "node:http";

const server = createServer((req, res) => {
  let bytes = 0;
  req.on("data", (chunk) => bytes += chunk.length);
  req.on("end", () => {
    console.log("server:", req.method, req.url, bytes, req.complete);
    res.end("ok");
  });
});

await new Promise<void>((resolve, reject) => {
  server.once("error", reject);
  server.listen(0, "127.0.0.1", resolve);
});
const address = server.address();
if (!address || typeof address === "string") throw new Error("missing address");

try {
  await new Promise<void>((resolve, reject) => {
    const req = get(`http://127.0.0.1:${address.port}/auto`, (res) => {
      res.once("error", reject);
      res.on("end", resolve);
      res.resume();
    });
    req.once("error", reject);
    console.log("client:", req.method, req.finished, req.writableEnded);
  });
} finally {
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
