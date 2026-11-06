import http from "node:http";
import { Buffer } from "node:buffer";

const server = http.createServer((req: any, res: any) => {
  console.log("headers typeof:", typeof req.headers);
  console.log("headers x-test:", req.headers["x-test"]);
  console.log("headers content-type:", req.headers["content-type"]);
  console.log("rawHeaders is array:", Array.isArray(req.rawHeaders));
  console.log("rawHeaders has value:", req.rawHeaders.includes("hi"));

  const chunks: Buffer[] = [];
  req.on("data", (chunk: any) => {
    console.log("request chunk typeof:", typeof chunk);
    console.log("request chunk is buffer:", Buffer.isBuffer(chunk));
    console.log("request chunk text:", chunk.toString("utf8"));
    chunks.push(chunk);
  });
  req.on("end", () => {
    console.log("concat body:", Buffer.concat(chunks).toString("utf8"));
    res.end("ok");
  });
});

await new Promise<void>((resolve, reject) => {
  server.once("error", reject);
  server.listen(0, "127.0.0.1", resolve);
});
const addr = server.address();
if (!addr || typeof addr === "string") throw new Error("missing address");

try {
  await new Promise<void>((resolve, reject) => {
    const req = http.request(
      {
        hostname: "127.0.0.1",
        port: addr.port,
        method: "POST",
        path: "/submit",
        headers: {
          "Content-Type": "text/plain",
          "X-Test": "hi",
        },
      },
      (res: any) => {
        res.on("data", () => {});
        res.once("error", reject);
        res.on("end", () => {
          console.log("client status:", res.statusCode);
          resolve();
        });
      },
    );
    req.once("error", reject);
    req.end("hello body");
  });
} finally {
  await new Promise<void>((resolve) => {
    server.close(() => {
      console.log("closed");
      resolve();
    });
  });
}
