import { AsyncLocalStorage } from "node:async_hooks";
import { createServer } from "node:net";

const storage = new AsyncLocalStorage<string>();
const server = createServer();

try {
  await storage.run(
    "connections",
    () =>
      new Promise<void>((resolve, reject) => {
        server.once("error", reject);
        server.listen(0, "127.0.0.1", () => {
          server.getConnections((error, count) => {
            console.log(
              "getConnections callback:",
              error === null,
              count,
              storage.getStore(),
            );
            if (error) reject(error);
            else resolve();
          });
        });
      }),
  );
} finally {
  if (server.listening) {
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve())),
    );
  }
}
console.log("getConnections outside:", String(storage.getStore()));
