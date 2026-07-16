import { spawn } from "node:child_process";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

const result = await storage.run(
  "child-spawn",
  () =>
    new Promise<string>((resolve, reject) => {
      const chunks: string[] = [];
      const child = spawn("/bin/sh", ["-c", "printf spawned"]);
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

console.log("child spawn output:", result);
console.log("child spawn outside:", String(storage.getStore()));
