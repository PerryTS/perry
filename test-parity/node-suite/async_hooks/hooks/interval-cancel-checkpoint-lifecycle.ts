import { createHook } from "node:async_hooks";

let intervalId = -1;
let readyId = -1;
let intervalDestroys = 0;
let readyDestroys = 0;
const events: string[] = [];
const checks: string[] = [];
const hook = createHook({
  init(id, type) {
    if (type !== "Timeout") return;
    if (intervalId === -1) intervalId = id;
    else if (readyId === -1) readyId = id;
  },
  before(id) {
    if (id === intervalId) events.push("before");
  },
  after(id) {
    if (id === intervalId) events.push("after");
  },
  destroy(id) {
    if (id === intervalId) {
      intervalDestroys++;
      events.push("destroy");
    }
    if (id === readyId) readyDestroys++;
  },
}).enable();

let calls = 0;
await new Promise<void>((resolve) => {
  const interval = setInterval(() => {
    calls++;
    clearInterval(interval);
    clearInterval(interval);
    const ready = setTimeout(() => checks.push("unexpected timeout"), 1000);
    clearTimeout(ready);
    process.nextTick(() => {
      clearInterval(interval);
      checks.push("tick:" + (intervalDestroys === 0));
    });
    queueMicrotask(() => {
      clearInterval(interval);
      checks.push("microtask:" + (intervalDestroys === 0));
      resolve();
    });
  }, 1);
});
await new Promise<void>((resolve) => setImmediate(resolve));
await new Promise<void>((resolve) => setImmediate(resolve));
hook.disable();

console.log("cancel checkpoint checks:", checks.sort().join("|"));
console.log("cancel checkpoint calls:", calls);
console.log("cancel checkpoint lifecycle:", events.join(">"));
console.log("cancel checkpoint destroys:", intervalDestroys, readyDestroys);
