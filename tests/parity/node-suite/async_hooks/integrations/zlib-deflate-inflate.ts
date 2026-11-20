import { AsyncLocalStorage } from "node:async_hooks";
import { deflate, inflate } from "node:zlib";

const storage = new AsyncLocalStorage<string>();
const compressed = await storage.run(
  "deflate",
  () =>
    new Promise<Buffer>((resolve, reject) => {
      deflate("deflate-payload", (error, data) => {
        console.log("deflate store:", storage.getStore());
        if (error) return reject(error);
        resolve(data);
      });
    }),
);
const restored = await storage.run(
  "inflate",
  () =>
    new Promise<Buffer>((resolve, reject) => {
      inflate(compressed, (error, data) => {
        console.log("inflate store:", storage.getStore());
        if (error) return reject(error);
        resolve(data);
      });
    }),
);
console.log("inflate result:", restored.toString());
console.log("deflate outside:", String(storage.getStore()));
