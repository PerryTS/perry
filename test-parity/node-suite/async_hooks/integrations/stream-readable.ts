import { Readable } from "node:stream";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

const output = await storage.run(
  "readable",
  () =>
    new Promise<string>((resolve, reject) => {
      const chunks: string[] = [];
      const readable = new Readable({
        read() {
          console.log("read method store:", storage.getStore());
          this.push("a");
          this.push("b");
          this.push(null);
        },
      });
      readable.on("error", reject);
      readable.on("data", (chunk) => {
        console.log("readable data store:", storage.getStore());
        chunks.push(String(chunk));
      });
      readable.on("end", () => {
        console.log("readable end store:", storage.getStore());
        resolve(chunks.join(""));
      });
    }),
);

console.log("readable result:", output);
console.log("readable outside:", String(storage.getStore()));
