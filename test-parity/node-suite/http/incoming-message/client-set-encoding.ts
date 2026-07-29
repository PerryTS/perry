import http from "node:http";
import { Buffer } from "node:buffer";

const server = http.createServer((_req: any, res: any) => {
  res.writeHead(200, { "Content-Type": "text/plain; charset=utf-8" });
  res.end(Buffer.from("hello utf8", "utf8"));
});

await new Promise<void>((resolve, reject) => {
  server.once("error", reject);
  server.listen(0, "127.0.0.1", resolve);
});
const addr = server.address();
if (!addr || typeof addr === "string") throw new Error("missing address");

try {
  await new Promise<void>((resolve, reject) => {
    const req = http.get(
      { hostname: "127.0.0.1", port: addr.port, path: "/" },
      (res: any) => {
        console.log("setEncoding typeof:", typeof res.setEncoding);
        console.log(
          "setEncoding returns this:",
          res.setEncoding("utf8") === res,
        );
        res.on("data", (chunk: any) => {
          console.log("response chunk typeof:", typeof chunk);
          console.log("response chunk is buffer:", Buffer.isBuffer(chunk));
          console.log("response chunk text:", chunk);
        });
        res.once("error", reject);
        res.on("end", resolve);
      },
    );
    req.once("error", reject);
  });
} finally {
  await new Promise<void>((resolve) => {
    server.close(() => {
      console.log("closed");
      resolve();
    });
  });
}
