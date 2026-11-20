import { createInterface } from "node:readline";
import { Readable } from "node:stream";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

const lines = await storage.run(
  "readline",
  () =>
    new Promise<string>((resolve) => {
      const output: string[] = [];
      const input = Readable.from(["first\n", "second\n"]);
      const rl = createInterface({ input });
      rl.on("line", (line) => {
        console.log("readline line store:", storage.getStore(), line);
        output.push(line);
      });
      rl.on("close", () => {
        console.log("readline close store:", storage.getStore());
        resolve(output.join(","));
      });
    }),
);

console.log("readline result:", lines);
console.log("readline outside:", String(storage.getStore()));
