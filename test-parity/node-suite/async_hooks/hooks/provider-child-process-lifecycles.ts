import { createHook, executionAsyncId } from "node:async_hooks";
import { spawn } from "node:child_process";

type Activity = { id: number; trigger: number; events: string[] };
const root = executionAsyncId();
const activities = new Map<string, Activity[]>();
const byId = new Map<number, Activity>();
for (const type of ["PROCESSWRAP", "PIPEWRAP"]) activities.set(type, []);
let accepting = true;
const hook = createHook({
  init(asyncId, type, triggerAsyncId) {
    if (!accepting) return;
    const list = activities.get(type);
    if (!list) return;
    const activity = { id: asyncId, trigger: triggerAsyncId, events: ["init"] };
    list.push(activity);
    byId.set(asyncId, activity);
  },
  before(asyncId) {
    byId.get(asyncId)?.events.push("before");
  },
  after(asyncId) {
    byId.get(asyncId)?.events.push("after");
  },
  destroy(asyncId) {
    byId.get(asyncId)?.events.push("destroy");
  },
}).enable();

const child = spawn("/bin/sh", ["-c", "printf ok"]);
accepting = false;
child.stdin.end();
let stdout = "";
child.stdout.setEncoding("utf8");
child.stdout.on("data", (chunk) => {
  stdout += chunk;
});
let exitCode: number | null = null;
const completion = new Promise<void>((resolve, reject) => {
  child.once("error", reject);
  child.once("exit", (code) => {
    exitCode = code;
  });
  child.once("close", () => resolve());
});
await completion;
await new Promise<void>((resolve) => setImmediate(resolve));
hook.disable();
const processes = activities.get("PROCESSWRAP")!;
const pipes = activities.get("PIPEWRAP")!;
console.log("child result:", exitCode, stdout);
console.log("child resources:", processes.length, pipes.length);
console.log(
  "child root triggers:",
  processes.length === 1 && processes.every((a) => a.trigger === root),
  pipes.length === 3 && pipes.every((a) => a.trigger === root),
);
console.log(
  "process callbacks:",
  processes.length === 1 &&
    processes.every(
      (a) => a.events.includes("before") && a.events.includes("after"),
    ),
);
console.log(
  "pipe balanced callbacks:",
  pipes.length === 3 &&
    pipes.every(
      (a) =>
        a.events.filter((e) => e === "before").length ===
        a.events.filter((e) => e === "after").length,
    ),
);
