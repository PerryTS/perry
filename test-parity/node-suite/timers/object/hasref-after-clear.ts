import { setTimeout, clearTimeout } from "node:timers";
// Node keeps the Timeout handle's hasRef() returning true after
// clearTimeout — the handle's ref flag is independent of the timer's
// active state. After an explicit unref(), it should be false even
// after a clear.
const timeout = setTimeout(() => {}, 100);
console.log("hasRef before clear:", timeout.hasRef());
clearTimeout(timeout);
console.log("hasRef after clear:", timeout.hasRef());
const unrefed = setTimeout(() => {}, 100);
unrefed.unref();
clearTimeout(unrefed);
console.log("hasRef after unref+clear:", unrefed.hasRef());
