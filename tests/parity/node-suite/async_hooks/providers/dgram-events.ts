import { createSocket } from "node:dgram";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

await storage.run(
  "dgram-events",
  () =>
    new Promise<void>((resolve, reject) => {
      const server = createSocket("udp4");
      const client = createSocket("udp4");
      server.on("error", reject);
      client.on("error", reject);
      server.on("listening", () => {
        console.log("dgram listening store:", storage.getStore());
        client.send("payload", server.address().port, "127.0.0.1");
      });
      server.on("message", () => {
        console.log("dgram message store:", storage.getStore());
        client.close();
        server.close();
      });
      server.on("close", () => {
        console.log("dgram close store:", storage.getStore());
        resolve();
      });
      server.bind(0, "127.0.0.1");
    }),
);

console.log("dgram events outside:", String(storage.getStore()));
