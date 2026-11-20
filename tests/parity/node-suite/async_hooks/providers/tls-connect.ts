import { AsyncLocalStorage } from "node:async_hooks";
import { connect, createServer } from "node:tls";
import { CERT, KEY } from "./fixtures/tls-credentials.js";

const storage = new AsyncLocalStorage<string>();
const server = createServer({ cert: CERT, key: KEY }, (socket) => {
  socket.end("tls-payload");
});

await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const address = server.address();
if (!address || typeof address === "string")
  throw new Error("missing TLS address");

let events: string[];
try {
  events = await storage.run(
    "tls-client",
    () =>
      new Promise<string[]>((resolve, reject) => {
        const seen: string[] = [];
        const socket = connect({
          host: "127.0.0.1",
          port: address.port,
          rejectUnauthorized: false,
        });
        socket.on("secureConnect", () => {
          seen.push(`secure:${storage.getStore()}`);
        });
        socket.on("data", (chunk) => {
          seen.push(`data:${storage.getStore()}:${String(chunk)}`);
        });
        socket.on("end", () => {
          seen.push(`end:${storage.getStore()}`);
        });
        socket.on("close", () => {
          seen.push(`close:${storage.getStore()}`);
          resolve(seen);
        });
        socket.on("error", reject);
      }),
  );
} finally {
  await new Promise<void>((resolve, reject) =>
    server.close((error) => (error ? reject(error) : resolve())),
  );
}
console.log("tls client events:", events.join("|"));
console.log("tls outside:", String(storage.getStore()));
