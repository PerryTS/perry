import { readFileSync } from "node:fs";
import * as https from "node:https";

const key = readFileSync(
  "test-parity/node-suite/tls/fixtures/localhost-key.pem",
);
const cert = readFileSync(
  "test-parity/node-suite/tls/fixtures/localhost-cert.pem",
);
const server = https.createServer({ key, cert }, (req, res) => {
  console.log("request:", req.method, req.url, req.httpVersion);
  res.writeHead(201, { "x-transport": "https" });
  res.end("secure");
});
server.listen(0, "127.0.0.1", () => {
  https.get({
    ca: cert,
    host: "127.0.0.1",
    path: "/secure",
    port: (server.address() as any).port,
  }, (res) => {
    let body = "";
    res.setEncoding("utf8");
    res.on("data", (chunk) => body += chunk);
    res.on("end", () => {
      console.log(
        "response:",
        res.statusCode,
        res.headers["x-transport"],
        body,
      );
      server.close();
    });
  }).on("error", (error) => {
    console.log("error:", (error as any).code);
    server.close();
  });
});
