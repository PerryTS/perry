import { AsyncLocalStorage } from "node:async_hooks";
import { copyFile, readFileSync, writeFileSync, rmSync } from "node:fs";
const storage = new AsyncLocalStorage<string>();
const source = "/tmp/perry-async-hooks-fs-copy-source.txt";
const target = "/tmp/perry-async-hooks-fs-copy-target.txt";
rmSync(source, { force: true });
rmSync(target, { force: true });
writeFileSync(source, "copy");
await storage.run(
  "fs-copy",
  () =>
    new Promise<void>((resolve, reject) => {
      copyFile(source, target, (error) => {
        console.log("fs.copyFile store:", storage.getStore());
        if (error) return reject(error);
        resolve();
      });
    }),
);
console.log("fs.copyFile content:", readFileSync(target, "utf8"));
rmSync(source, { force: true });
rmSync(target, { force: true });
console.log("fs.copyFile outside:", String(storage.getStore()));
