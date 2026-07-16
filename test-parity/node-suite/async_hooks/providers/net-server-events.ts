import { connect, createServer } from "node:net";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

await storage.run(
  "net-server",
  () =>
    new Promise<void>((resolve, reject) => {
      const server = createServer();
      server.on("connection", (socket) => {
        console.log("net server connection store:", storage.getStore());
        socket.end();
      });
      server.on("error", reject);
      server.on("listening", () => {
        console.log("net server listening store:", storage.getStore());
        const address = server.address();
        if (!address || typeof address === "string")
          return reject(new Error("address"));
        const client = connect(address.port, "127.0.0.1");
        client.on("error", reject);
        client.on("close", () => {
          server.close((error) => (error ? reject(error) : resolve()));
        });
      });
      server.listen(0, "127.0.0.1");
    }),
);

console.log("net server outside:", String(storage.getStore()));
