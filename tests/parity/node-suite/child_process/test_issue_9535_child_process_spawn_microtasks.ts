// Issue #9535: each child-process event is its own event-loop callback.
// Promise continuations released by `spawn` must therefore run before a
// short-lived child's already-queued data/exit/close events are delivered.
import { spawn } from "node:child_process";
import assert from "node:assert/strict";

const order: string[] = [];
const child = spawn("/bin/echo", ["hello"]);

child.on("spawn", () => order.push("spawn"));
child.stdout!.on("data", () => order.push("data"));
child.on("exit", () => order.push("exit"));
child.on("close", () => order.push("close"));
const closed = new Promise<void>((resolve) => child.on("close", resolve));

// Let the tiny child finish before the first event-loop pump. This removes a
// scheduler race and exercises the bug's defining case: spawn and the full
// lifecycle are already queued together.
const spinUntil = Date.now() + 100;
while (Date.now() < spinUntil) {}

await new Promise<void>((resolve) => child.on("spawn", resolve));
order.push("resumed-after-spawn");
await Promise.resolve();
order.push("microtask");
await new Promise<void>((resolve) => setImmediate(resolve));
order.push("immediate");

await closed;

// `exit` can precede the last stdout read. Compare the guaranteed partial
// order, not the scheduler's choice between data, exit, and setImmediate.
// https://nodejs.org/api/child_process.html#event-exit
for (const event of ["spawn", "resumed-after-spawn", "microtask", "data", "exit", "immediate", "close"]) {
  assert.equal(order.filter((entry) => entry === event).length, 1, event);
}
function before(first: string, second: string) {
  assert.ok(order.indexOf(first) < order.indexOf(second), first + " before " + second + ": " + order.join(" "));
}
before("spawn", "resumed-after-spawn");
before("resumed-after-spawn", "microtask");
for (const event of ["data", "exit", "immediate", "close"]) before("microtask", event);
before("data", "close");
before("exit", "close");
console.log("spawn microtasks precede lifecycle events");
console.log("close follows data and exit");
