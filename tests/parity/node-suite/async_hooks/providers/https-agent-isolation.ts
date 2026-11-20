import { AsyncLocalStorage } from "node:async_hooks";
import { Agent, createServer, get } from "node:https";
import { CERT, KEY } from "./fixtures/tls-credentials.js";

const storage = new AsyncLocalStorage<number>();
const server = createServer({ cert: CERT, key: KEY }, (_request, response) => {
  response.end("ok");
});
const agent = new Agent({ keepAlive: true, maxSockets: 1 });

await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const address = server.address();
if (!address || typeof address === "string")
  throw new Error("missing HTTPS address");

const stores: number[] = [];
try {
  for (let index = 0; index < 3; index++) {
    await storage.run(
      index,
      () =>
        new Promise<void>((resolve, reject) => {
          const req = get(
            {
              agent,
              host: "127.0.0.1",
              port: address.port,
              rejectUnauthorized: false,
            },
            (response) => {
              stores.push(storage.getStore() ?? -1);
              response.resume();
              response.on("end", resolve);
            },
          );
          req.on("error", reject);
        }),
    );
  }
} finally {
  agent.destroy();
  await new Promise<void>((resolve, reject) =>
    server.close((error) => (error ? reject(error) : resolve())),
  );
}
console.log("https agent stores:", stores.join(","));
console.log("https agent outside:", String(storage.getStore()));
