import { AsyncLocalStorage } from "node:async_hooks";
import { createServer } from "node:http";

const storage = new AsyncLocalStorage<string>();
const server = createServer((_request, response) => response.end("fetch-ok"));
let thenStore: string | undefined;
let afterAwaitStore: string | undefined;
let body = "";

try {
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const address = server.address();
  if (!address || typeof address === "string")
    throw new Error("missing fetch address");

  await storage.run("fetch-context", async () => {
    const response = await fetch(`http://127.0.0.1:${address.port}`).then(
      (value) => {
        thenStore = storage.getStore();
        return value;
      },
    );
    body = await response.text();
    afterAwaitStore = storage.getStore();
  });
} finally {
  if (server.listening) {
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve())),
    );
  }
}

console.log("fetch stores:", thenStore, afterAwaitStore);
console.log("fetch body/outside:", body, String(storage.getStore()));
