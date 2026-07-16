import { unlinkSync, writeFile, readFile, unlink } from "node:fs";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();
const path = "/tmp/perry-async-hooks-fs-callback.txt";

try {
  unlinkSync(path);
} catch {}

await storage.run(
  "fs-callback",
  () =>
    new Promise<void>((resolve, reject) => {
      writeFile(path, "payload", (writeError) => {
        console.log("writeFile store:", storage.getStore());
        if (writeError) return reject(writeError);

        readFile(path, "utf8", (readError, data) => {
          console.log("readFile store:", storage.getStore(), data);
          if (readError) return reject(readError);

          unlink(path, (unlinkError) => {
            console.log("unlink store:", storage.getStore());
            if (unlinkError) return reject(unlinkError);
            resolve();
          });
        });
      });
    }),
);

console.log("fs callback outside:", String(storage.getStore()));
