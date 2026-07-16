import { AsyncLocalStorage } from "node:async_hooks";
import { statSync, truncate, writeFileSync, unlinkSync } from "node:fs";
const storage = new AsyncLocalStorage<string>();
const path = "/tmp/perry-async-hooks-fs-truncate.txt";
writeFileSync(path, "truncate");
await storage.run(
  "fs-truncate",
  () =>
    new Promise<void>((resolve, reject) => {
      truncate(path, 3, (error) => {
        console.log("fs.truncate store:", storage.getStore());
        if (error) return reject(error);
        resolve();
      });
    }),
);
console.log("fs.truncate size:", statSync(path).size);
unlinkSync(path);
console.log("fs.truncate outside:", String(storage.getStore()));
