import { execFile } from "node:child_process";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();
const shell = process.platform === "win32" ? "cmd.exe" : "/bin/sh";
const shellArgs =
  process.platform === "win32"
    ? ["/d", "/s", "/c", "echo child-file"]
    : ["-c", "printf child-file"];

const output = await storage.run(
  "child-exec-file",
  () =>
    new Promise<string>((resolve, reject) => {
      execFile(shell, shellArgs, (error, stdout) => {
        console.log("child execFile store:", storage.getStore());
        if (error) return reject(error);
        resolve(stdout);
      });
    }),
);

console.log("child execFile output:", output.trim());
console.log("child execFile outside:", String(storage.getStore()));
