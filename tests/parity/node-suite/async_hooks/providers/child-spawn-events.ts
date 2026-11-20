import { spawn } from "node:child_process";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();
const shell = process.platform === "win32" ? "cmd.exe" : "/bin/sh";
const shellArgs =
  process.platform === "win32"
    ? ["/d", "/s", "/c", "echo spawned"]
    : ["-c", "printf spawned"];

const result = await storage.run(
  "child-spawn",
  () =>
    new Promise<string>((resolve, reject) => {
      const chunks: string[] = [];
      const child = spawn(shell, shellArgs);
      child.on("spawn", () => {
        console.log("child spawn event store:", storage.getStore());
      });
      child.stdout.on("data", (chunk) => {
        console.log("child stdout store:", storage.getStore());
        chunks.push(String(chunk));
      });
      child.on("error", reject);
      child.on("close", (code) => {
        console.log("child close store:", storage.getStore(), code);
        resolve(chunks.join(""));
      });
    }),
);

console.log("child spawn output:", result.trim());
console.log("child spawn outside:", String(storage.getStore()));
