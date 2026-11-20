import { exec } from "node:child_process";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

const output = await storage.run(
  "child-exec",
  () =>
    new Promise<string>((resolve, reject) => {
      exec("printf child-exec", (error, stdout) => {
        console.log("child exec store:", storage.getStore());
        if (error) return reject(error);
        resolve(stdout);
      });
    }),
);

console.log("child exec output:", output);
console.log("child exec outside:", String(storage.getStore()));
