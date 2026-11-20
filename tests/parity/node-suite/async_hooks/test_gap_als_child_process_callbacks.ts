// The callback of an internal runtime completion runs in the AsyncLocalStorage
// context of the operation that started it (als.run -> operation -> callback).
import { AsyncLocalStorage } from "node:async_hooks";
const als = new AsyncLocalStorage<string>();

import { exec, spawn } from "node:child_process";
const lines: string[] = [];
const done = () => { if (lines.length === 3) console.log(lines.sort().join("\n")); };
als.run("exec", () => exec("true", () => { lines.push("exec callback: " + als.getStore()); done(); }));
als.run("write", () => {
  const c = spawn("cat");
  c.stdin.write("x", () => { lines.push("stdin write callback: " + als.getStore()); done(); c.stdin.end(); });
});
als.run("error", () => {
  const bad = spawn("/nonexistent-perry-als-probe");
  bad.on("error", () => { lines.push("spawn error event: " + als.getStore()); done(); });
});
