import { AsyncLocalStorage } from "node:async_hooks";
import { createServer, request } from "node:https";
import { CERT, KEY } from "./fixtures/tls-credentials.js";

const storage = new AsyncLocalStorage<string>();
const server = createServer({ cert: CERT, key: KEY }, (_request, response) => {
  response.end("https-payload");
});

await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const address = server.address();
if (!address || typeof address === "string")
  throw new Error("missing HTTPS address");

let events: string[];
try {
  events = await storage.run(
    "https-client",
    () =>
      new Promise<string[]>((resolve, reject) => {
        const seen: string[] = [];
        const req = request(
          {
            host: "127.0.0.1",
            port: address.port,
            rejectUnauthorized: false,
          },
          (response) => {
            seen.push(`response:${storage.getStore()}`);
            response.on("data", (chunk) => {
              seen.push(`data:${storage.getStore()}:${String(chunk)}`);
            });
            response.on("end", () => {
              seen.push(`end:${storage.getStore()}`);
              resolve(seen);
            });
          },
        );
        req.on("finish", () => seen.push(`finish:${storage.getStore()}`));
        req.on("error", reject);
        req.end();
      }),
  );
} finally {
  await new Promise<void>((resolve, reject) =>
    server.close((error) => (error ? reject(error) : resolve())),
  );
}
console.log("https request events:", events.join("|"));
console.log("https outside:", String(storage.getStore()));
