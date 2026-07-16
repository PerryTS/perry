import { createSocket } from "node:dgram";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

await storage.run(
  "dgram-send",
  () =>
    new Promise<void>((resolve, reject) => {
      const server = createSocket("udp4");
      const client = createSocket("udp4");
      server.on("error", reject);
      client.on("error", reject);
      server.on("message", () => {
        server.close();
        client.close();
        resolve();
      });
      server.bind(0, "127.0.0.1", () => {
        const address = server.address();
        client.send("payload", address.port, "127.0.0.1", (error) => {
          console.log("dgram send callback store:", storage.getStore());
          if (error) reject(error);
        });
      });
    }),
);

console.log("dgram send outside:", String(storage.getStore()));
