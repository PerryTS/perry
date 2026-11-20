import { createHook, executionAsyncId } from "node:async_hooks";
import { access } from "node:fs";

let target = -1;
let triggerMatches = false;
const root = executionAsyncId();
const events: string[] = [];
const hook = createHook({
  init(asyncId, type, triggerAsyncId) {
    if (type === "FSREQCALLBACK" && target === -1) {
      target = asyncId;
      triggerMatches = triggerAsyncId === root;
      events.push("init");
    }
  },
  before(asyncId) {
    if (asyncId === target) events.push("before");
  },
  after(asyncId) {
    if (asyncId === target) events.push("after");
  },
  destroy(asyncId) {
    if (asyncId === target) events.push("destroy");
  },
}).enable();

await new Promise<void>((resolve, reject) =>
  access(import.meta.filename, (error) => (error ? reject(error) : resolve())),
);
await new Promise<void>((resolve) => setImmediate(resolve));
hook.disable();
console.log("fs hook trigger root:", triggerMatches);
console.log("fs hook lifecycle:", events.join(">"));
