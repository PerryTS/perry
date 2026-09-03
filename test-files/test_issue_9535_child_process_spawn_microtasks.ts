// Issue #9535: each child-process event is its own event-loop callback.
// Promise continuations released by `spawn` must therefore run before a
// short-lived child's already-queued data/exit/close events are delivered.
import { spawn } from "node:child_process";

const order: string[] = [];
const child = spawn("/bin/echo", ["hello"]);

child.on("spawn", () => order.push("spawn"));
child.stdout!.on("data", () => order.push("data"));
child.on("exit", () => order.push("exit"));
child.on("close", () => order.push("close"));

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

setTimeout(() => console.log(order.join(" ")), 300);
