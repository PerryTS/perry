import { lookup } from "node:dns";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

const family = await storage.run(
  "dns-lookup",
  () =>
    new Promise<number>((resolve, reject) => {
      lookup("localhost", (error, address, addressFamily) => {
        console.log("dns lookup store:", storage.getStore());
        if (error) return reject(error);
        console.log("dns lookup address type:", typeof address);
        resolve(addressFamily);
      });
    }),
);

console.log("dns lookup family valid:", family === 4 || family === 6);
console.log("dns lookup outside:", String(storage.getStore()));
