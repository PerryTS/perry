// Global timer functions are callable and report typeof "function".
console.log("setTimeout:", typeof setTimeout);
console.log("setInterval:", typeof setInterval);
console.log("setImmediate:", typeof setImmediate);
console.log("clearTimeout:", typeof clearTimeout);
console.log("clearInterval:", typeof clearInterval);
console.log("clearImmediate:", typeof clearImmediate);
console.log(
  "direct globalThis typeof:",
  typeof globalThis.setTimeout,
  typeof globalThis.queueMicrotask,
);

const g: any = globalThis;

for (const name of [
  "setTimeout",
  "clearTimeout",
  "setInterval",
  "clearInterval",
  "setImmediate",
  "clearImmediate",
  "queueMicrotask",
]) {
  const fn = g[name];
  const desc = Object.getOwnPropertyDescriptor(globalThis, name);
  console.log(
    `${name} globalThis:`,
    typeof fn,
    fn?.name,
    fn?.length,
    !!desc,
    desc?.writable,
    desc?.enumerable,
    desc?.configurable,
  );
}

let fired = 0;
const im = setImmediate(() => { fired++; });
clearImmediate(im);
await new Promise<void>((r) => setTimeout(() => r(), 15));
console.log("cleared immediate:", fired === 0);

const delay = g.setTimeout;
const cancelDelay = g.clearTimeout;
const every = g.setInterval;
const cancelEvery = g.clearInterval;
const immediate = g.setImmediate;
const cancelImmediate = g.clearImmediate;
const microtask = g.queueMicrotask;

let timeoutCancelledFired = false;
let immediateCancelledFired = false;
let intervalFired = false;
let microtaskFired = false;
let immediateFired = false;
let timeoutFired = false;
let intervalArg = "";
let immediateArg = "";
let timeoutArg = "";

const cancelledTimeout = delay(() => { timeoutCancelledFired = true; }, 1);
cancelDelay(cancelledTimeout);

const cancelledImmediate = immediate(() => { immediateCancelledFired = true; });
cancelImmediate(cancelledImmediate);

let interval: any;
interval = every((value: string) => {
  intervalFired = true;
  intervalArg = value;
  cancelEvery(interval);
}, 1, "interval-arg");

microtask(() => { microtaskFired = true; });
immediate((value: string) => {
  immediateFired = true;
  immediateArg = value;
}, "immediate-arg");
delay((value: string) => {
  timeoutFired = true;
  timeoutArg = value;
}, 1, "timeout-arg");

await new Promise<void>((r) => delay(() => r(), 25));
console.log(
  "rebound fired:",
  microtaskFired,
  immediateFired,
  timeoutFired,
  intervalFired,
);
console.log("rebound args:", timeoutArg, immediateArg, intervalArg);
console.log(
  "rebound cancelled:",
  !timeoutCancelledFired,
  !immediateCancelledFired,
);
