import { AsyncLocalStorage } from "node:async_hooks";
import { access, constants, writeFileSync, unlinkSync } from "node:fs";
const storage = new AsyncLocalStorage<string>();
const path = "/tmp/perry-async-hooks-fs-access.txt";
writeFileSync(path, "access");
try {
  await storage.run(
    "fs-access",
    () =>
      new Promise<void>((resolve, reject) => {
        access(path, constants.R_OK, (error) => {
          console.log("fs.access store:", storage.getStore());
          if (error) return reject(error);
          resolve();
        });
      }),
  );
} finally {
  unlinkSync(path);
}
console.log("fs.access outside:", String(storage.getStore()));
