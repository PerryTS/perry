import { connect, createServer } from "node:net";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();
const server = createServer((socket) => socket.end());

await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const address = server.address();
if (!address || typeof address === "string")
  throw new Error("missing server address");

let client: ReturnType<typeof connect> | undefined;
try {
  await storage.run(
    "net-connect",
    () =>
      new Promise<void>((resolve, reject) => {
        client = connect(address.port, "127.0.0.1", () => {
          console.log("net connect callback store:", storage.getStore());
        });
        client.on("error", reject);
        client.on("close", () => {
          console.log("net close event store:", storage.getStore());
          resolve();
        });
      }),
  );
} finally {
  client?.destroy();
  await new Promise<void>((resolve, reject) =>
    server.close((error) => (error ? reject(error) : resolve())),
  );
}
console.log("net connect outside:", String(storage.getStore()));
