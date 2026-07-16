import { AsyncLocalStorage } from "node:async_hooks";
import { unwatchFile, watchFile, writeFileSync, unlinkSync } from "node:fs";
const storage = new AsyncLocalStorage<string>();
const path = "/tmp/perry-async-hooks-fs-watch-file.txt";
writeFileSync(path, "before");
await storage.run(
  "fs-watch-file",
  () =>
    new Promise<void>((resolve, reject) => {
      const timeout = setTimeout(() => {
        unwatchFile(path);
        reject(new Error("fs.watchFile timeout"));
      }, 2000);
      watchFile(path, { interval: 20 }, () => {
        console.log("fs.watchFile store:", storage.getStore());
        clearTimeout(timeout);
        unwatchFile(path);
        resolve();
      });
      setTimeout(() => writeFileSync(path, "after"), 50);
    }),
);
unlinkSync(path);
console.log("fs.watchFile outside:", String(storage.getStore()));
