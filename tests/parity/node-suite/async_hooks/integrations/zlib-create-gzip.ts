import { AsyncLocalStorage } from "node:async_hooks";
import { createGzip } from "node:zlib";

const storage = new AsyncLocalStorage<string>();
const events = await storage.run(
  "gzip-stream",
  () =>
    new Promise<string[]>((resolve, reject) => {
      const seen: string[] = [];
      const gzip = createGzip();
      gzip.on("data", (chunk) => {
        seen.push(`data:${storage.getStore()}:${chunk.length > 0}`);
      });
      gzip.on("finish", () => {
        seen.push(`finish:${storage.getStore()}`);
      });
      gzip.on("end", () => {
        seen.push(`end:${storage.getStore()}`);
        resolve(seen);
      });
      gzip.on("error", reject);
      gzip.end("gzip-stream-payload");
    }),
);
console.log("gzip stream events:", events.join("|"));
console.log("gzip stream outside:", String(storage.getStore()));
