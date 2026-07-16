import { AsyncLocalStorage } from "node:async_hooks";
import { mkdirSync, rmSync, watch, writeFileSync } from "node:fs";
const storage = new AsyncLocalStorage<string>();
const directory = "/tmp/perry-async-hooks-fs-watch";
const path = `${directory}/value.txt`;
rmSync(directory, { recursive: true, force: true });
mkdirSync(directory);
writeFileSync(path, "before");
await storage.run(
  "fs-watch",
  () =>
    new Promise<void>((resolve, reject) => {
      let settled = false;
      const timeout = setTimeout(() => {
        watcher.close();
        reject(new Error("fs.watch timeout"));
      }, 2000);
      const watcher = watch(directory, () => {
        if (settled) return;
        settled = true;
        console.log("fs.watch store:", storage.getStore());
        clearTimeout(timeout);
        watcher.close();
        resolve();
      });
      setImmediate(() => writeFileSync(path, "after"));
    }),
);
rmSync(directory, { recursive: true, force: true });
console.log("fs.watch outside:", String(storage.getStore()));
