import { execFile } from "node:child_process";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

const output = await storage.run(
  "child-exec-file",
  () =>
    new Promise<string>((resolve, reject) => {
      execFile("/bin/sh", ["-c", "printf child-file"], (error, stdout) => {
        console.log("child execFile store:", storage.getStore());
        if (error) return reject(error);
        resolve(stdout);
      });
    }),
);

console.log("child execFile output:", output);
console.log("child execFile outside:", String(storage.getStore()));
