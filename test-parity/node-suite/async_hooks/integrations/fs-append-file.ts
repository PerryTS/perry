import { AsyncLocalStorage } from "node:async_hooks";
import { appendFile, readFileSync, writeFileSync, unlinkSync } from "node:fs";
const storage = new AsyncLocalStorage<string>();
const path = "/tmp/perry-async-hooks-fs-append.txt";
writeFileSync(path, "a");
await storage.run(
  "fs-append",
  () =>
    new Promise<void>((resolve, reject) => {
      appendFile(path, "b", (error) => {
        console.log("fs.appendFile store:", storage.getStore());
        if (error) return reject(error);
        resolve();
      });
    }),
);
console.log("fs.appendFile content:", readFileSync(path, "utf8"));
unlinkSync(path);
console.log("fs.appendFile outside:", String(storage.getStore()));
