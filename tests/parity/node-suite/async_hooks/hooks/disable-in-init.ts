import { createHook } from "node:async_hooks";
import { access } from "node:fs";

let outerInits = 0;
let nestedInits = 0;
let scheduled = false;
let pending = 1;
let finish!: () => void;
const completed = new Promise<void>((resolve) => {
  finish = resolve;
});
function done(error: NodeJS.ErrnoException | null) {
  if (error) throw error;
  if (--pending === 0) finish();
}

const outer = createHook({
  init(_asyncId, type) {
    if (type !== "FSREQCALLBACK") return;
    outerInits++;
    nested.disable();
    if (!scheduled) {
      scheduled = true;
      pending++;
      access(import.meta.filename, done);
    }
  },
}).enable();
const nested = createHook({
  init(_asyncId, type) {
    if (type === "FSREQCALLBACK") nestedInits++;
  },
}).enable();

access(import.meta.filename, done);
await completed;
outer.disable();
nested.disable();
console.log("disable in init outer:", outerInits);
console.log("disable in init nested:", nestedInits);
