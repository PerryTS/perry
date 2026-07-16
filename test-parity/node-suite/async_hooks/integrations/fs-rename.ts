import { AsyncLocalStorage } from "node:async_hooks";
import { rename, readFileSync, writeFileSync, rmSync } from "node:fs";
const storage = new AsyncLocalStorage<string>();
const source = "/tmp/perry-async-hooks-fs-rename-source.txt";
const target = "/tmp/perry-async-hooks-fs-rename-target.txt";
rmSync(source, { force: true });
rmSync(target, { force: true });
writeFileSync(source, "rename");
await storage.run(
  "fs-rename",
  () =>
    new Promise<void>((resolve, reject) => {
      rename(source, target, (error) => {
        console.log("fs.rename store:", storage.getStore());
        if (error) return reject(error);
        resolve();
      });
    }),
);
console.log("fs.rename content:", readFileSync(target, "utf8"));
rmSync(target, { force: true });
console.log("fs.rename outside:", String(storage.getStore()));
