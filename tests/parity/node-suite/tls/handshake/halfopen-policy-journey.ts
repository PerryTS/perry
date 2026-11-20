import tls from "node:tls";
import net from "node:net";
import { readFileSync } from "node:fs";

// Derived from the unchanged existing-net-socket public TLS handshake control.
const fixture = new URL("../fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
let raw: any;
let client: any;
let peer: any;
let reads = 0;
const deadline = setTimeout(() => { console.error("deadline", "t74-public-wrapper"); process.exit(1); }, 6000);
const server = tls.createServer({ key, cert }, (socket) => {
  peer = socket;
  socket.end("wrapped");
});
server.on("error", (error: any) => { console.error("server error:", error.code); process.exit(1); });
server.listen(0, "127.0.0.1", () => {
  raw = net.connect((server.address() as any).port, "127.0.0.1", () => {
    Object.defineProperty(raw, "allowHalfOpen", {
      configurable: true, enumerable: true,
      get() { reads++; return false; },
    });
    client = tls.connect({ socket: raw, rejectUnauthorized: false });
    const descriptor = Object.getOwnPropertyDescriptor(client, "allowHalfOpen");
    console.log("snapshot:", client.allowHalfOpen, reads, !!descriptor,
      descriptor?.writable, descriptor?.enumerable, descriptor?.configurable);
    Object.defineProperty(raw, "allowHalfOpen", {
      value: true, writable: true, configurable: true, enumerable: true,
    });
    console.log("parent-independent:", client.allowHalfOpen, raw.allowHalfOpen, reads);
    client.allowHalfOpen = true;
    raw.allowHalfOpen = false;
    console.log("wrapper-independent:", client.allowHalfOpen, raw.allowHalfOpen, reads);
    let data = "";
    client.on("secureConnect", () => console.log("secure:", client.encrypted, client.authorized));
    client.on("data", (chunk) => { data += chunk.toString(); });
    client.on("error", (err: any) => { console.error("client error:", err.code); process.exit(1); });
    client.on("end", () => {
      console.log("data:", data);
      console.log("end-before:", client.writableEnded);
      // Opposing parent policy makes logical owner choice observable.
      raw.allowHalfOpen = true;
      client.allowHalfOpen = false;
      setImmediate(() => console.log("end-after:", client.writableEnded, client.allowHalfOpen, raw.allowHalfOpen, reads));
    });
    client.on("close", () => {
      raw.destroy(); peer?.destroy();
      server.close(() => { clearTimeout(deadline); console.log("closed:", true); });
    });
  });
  raw.on("error", (err: any) => { console.error("raw error:", err.code); process.exit(1); });
});
