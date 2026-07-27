import { createServer, get } from "node:http";

const server = createServer((req, res) => {
  let bytes = 0;
  req.on("data", (chunk) => bytes += chunk.length);
  req.on("end", () => {
    console.log("server:", req.method, req.url, bytes, req.complete);
    res.end("ok", () => server.close());
  });
});

server.listen(0, "127.0.0.1", () => {
  const address = server.address();
  if (!address || typeof address === "string") {
    throw new Error("missing address");
  }
  const req = get(`http://127.0.0.1:${address.port}/auto`, (res) => {
    res.resume();
  });
  console.log("client:", req.method, req.finished, req.writableEnded);
});
