import { AsyncLocalStorage } from "node:async_hooks";
import { mkdtemp, rmSync } from "node:fs";
const storage = new AsyncLocalStorage<string>();
const made = await storage.run(
  "fs-mkdtemp",
  () =>
    new Promise<string>((resolve, reject) => {
      mkdtemp("/tmp/perry-async-hooks-mkdtemp-", (error, path) => {
        console.log("fs.mkdtemp store:", storage.getStore());
        if (error) return reject(error);
        resolve(path);
      });
    }),
);
console.log(
  "fs.mkdtemp prefix:",
  made.startsWith("/tmp/perry-async-hooks-mkdtemp-"),
);
rmSync(made, { recursive: true, force: true });
console.log("fs.mkdtemp outside:", String(storage.getStore()));
