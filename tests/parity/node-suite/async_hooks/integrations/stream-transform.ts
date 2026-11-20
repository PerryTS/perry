import { Transform } from "node:stream";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

const output = await storage.run(
  "transform",
  () =>
    new Promise<string>((resolve, reject) => {
      const chunks: string[] = [];
      const transform = new Transform({
        transform(chunk, encoding, callback) {
          console.log("transform method store:", storage.getStore());
          callback(null, String(chunk).toUpperCase());
        },
      });
      transform.on("error", reject);
      transform.on("data", (chunk) => {
        console.log("transform data store:", storage.getStore());
        chunks.push(String(chunk));
      });
      transform.on("end", () => {
        console.log("transform end store:", storage.getStore());
        resolve(chunks.join(""));
      });
      transform.end("payload");
    }),
);

console.log("transform result:", output);
console.log("transform outside:", String(storage.getStore()));
