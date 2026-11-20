import { gzip, gunzip } from "node:zlib";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

const compressed = await storage.run(
  "gzip",
  () =>
    new Promise<Buffer>((resolve, reject) => {
      gzip("compressed-payload", (error, data) => {
        console.log("gzip store:", storage.getStore());
        if (error) return reject(error);
        resolve(data);
      });
    }),
);

const restored = await storage.run(
  "gunzip",
  () =>
    new Promise<Buffer>((resolve, reject) => {
      gunzip(compressed, (error, data) => {
        console.log("gunzip store:", storage.getStore());
        if (error) return reject(error);
        resolve(data);
      });
    }),
);

console.log("gunzip result:", restored.toString());
console.log("zlib outside:", String(storage.getStore()));
