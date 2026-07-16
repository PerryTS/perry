import { AsyncLocalStorage } from "node:async_hooks";
import { chmod, lstat, stat, writeFileSync, unlinkSync } from "node:fs";
const storage = new AsyncLocalStorage<string>();
const path = "/tmp/perry-async-hooks-fs-metadata.txt";
writeFileSync(path, "meta");
await storage.run(
  "fs-metadata",
  () =>
    new Promise<void>((resolve, reject) => {
      chmod(path, 0o600, (chmodError) => {
        console.log("fs.chmod store:", storage.getStore());
        if (chmodError) return reject(chmodError);
        lstat(path, (lstatError, linkStats) => {
          console.log(
            "fs.lstat store:",
            storage.getStore(),
            linkStats.isFile(),
          );
          if (lstatError) return reject(lstatError);
          stat(path, (statError, stats) => {
            console.log("fs.stat store:", storage.getStore(), stats.size);
            if (statError) return reject(statError);
            resolve();
          });
        });
      });
    }),
);
unlinkSync(path);
console.log("fs.metadata outside:", String(storage.getStore()));
