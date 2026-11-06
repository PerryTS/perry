import { Writable } from "node:stream";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

const writes = await storage.run(
  "writable",
  () =>
    new Promise<number>((resolve, reject) => {
      let count = 0;
      const writable = new Writable({
        write(chunk, encoding, callback) {
          count++;
          console.log(
            "writable write store:",
            storage.getStore(),
            String(chunk),
          );
          callback();
        },
      });
      writable.on("error", reject);
      writable.on("finish", () => {
        console.log("writable finish store:", storage.getStore());
        resolve(count);
      });
      writable.write("first");
      writable.end("second");
    }),
);

console.log("writable count:", writes);
console.log("writable outside:", String(storage.getStore()));
