import http from "node:http";
import { Buffer } from "node:buffer";

const server = http.createServer((_req: any, res: any) => {
  res.writeHead(200, { "Content-Type": "text/plain; charset=utf-8" });
  res.end(Buffer.from("hello utf8", "utf8"));
});

server.listen(0, () => {
  const addr = server.address();
  const port = typeof addr === "object" && addr !== null ? addr.port : 0;
  http.get({ hostname: "127.0.0.1", port, path: "/" }, (res: any) => {
    console.log("setEncoding typeof:", typeof res.setEncoding);
    console.log("setEncoding returns this:", res.setEncoding("utf8") === res);
    res.on("data", (chunk: any) => {
      console.log("response chunk typeof:", typeof chunk);
      console.log("response chunk is buffer:", Buffer.isBuffer(chunk));
      console.log("response chunk text:", chunk);
    });
    res.on("end", () => {
      server.close(() => console.log("closed"));
    });
  });
});

setTimeout(() => {}, 1500);
