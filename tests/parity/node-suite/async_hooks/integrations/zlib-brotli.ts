import { brotliCompress, brotliDecompress } from "node:zlib";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

const compressed = await storage.run(
  "brotli-compress",
  () =>
    new Promise<Buffer>((resolve, reject) => {
      brotliCompress("brotli-payload", (error, data) => {
        console.log("brotli compress store:", storage.getStore());
        if (error) return reject(error);
        resolve(data);
      });
    }),
);

const restored = await storage.run(
  "brotli-decompress",
  () =>
    new Promise<Buffer>((resolve, reject) => {
      brotliDecompress(compressed, (error, data) => {
        console.log("brotli decompress store:", storage.getStore());
        if (error) return reject(error);
        resolve(data);
      });
    }),
);

console.log("brotli result:", restored.toString());
console.log("brotli outside:", String(storage.getStore()));
