import { createHook, executionAsyncId } from "node:async_hooks";
import { pbkdf2 } from "node:crypto";

let target = -1;
let triggerMatches = false;
const root = executionAsyncId();
const events: string[] = [];
const hook = createHook({
  init(asyncId, type, triggerAsyncId) {
    if (type === "PBKDF2REQUEST" && target === -1) {
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
  pbkdf2("password", "salt", 1, 8, "sha256", (error) =>
    error ? reject(error) : resolve(),
  ),
);
await new Promise<void>((resolve) => setImmediate(resolve));
hook.disable();
console.log("crypto hook trigger root:", triggerMatches);
console.log("crypto hook lifecycle:", events.join(">"));
