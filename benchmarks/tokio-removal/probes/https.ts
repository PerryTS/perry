import * as https from "node:https";
import { key, cert } from "./certs.ts";
const server = https.createServer({ key, cert }, (req, res) => res.end("secure:" + req.url));
server.listen(0, () => {
  const port = (server.address() as any).port;
  https.get({ host: "127.0.0.1", port, path: "/p", rejectUnauthorized: false }, (res) => {
    let body = "";
    res.on("data", (d) => (body += d));
    res.on("end", () => {
      console.log("https", res.statusCode, body);
      server.close(() => console.log("https server closed"));
    });
  }).on("error", (e) => console.log("https error", e.message));
});
