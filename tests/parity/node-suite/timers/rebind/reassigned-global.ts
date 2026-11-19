// A bare timer call reads the global binding when the program rebinds it:
// `globalThis.setTimeout = wrapper` makes `setTimeout(...)` call the wrapper,
// as in Node. Covers setTimeout, setInterval, clearTimeout, clearInterval,
// setImmediate and queueMicrotask; clearImmediate stays untouched and keeps
// the built-in.
const g = globalThis as any;
// Timer and immediate order races in Node: print what fired, sorted.
const fired: string[] = [];
const orig = {
  setTimeout: g.setTimeout,
  setInterval: g.setInterval,
  clearTimeout: g.clearTimeout,
  clearInterval: g.clearInterval,
  setImmediate: g.setImmediate,
  queueMicrotask: g.queueMicrotask,
};
const calls: string[] = [];
g.setTimeout = function (fn: any, ms?: any, ...rest: any[]) {
  calls.push("setTimeout " + ms);
  return orig.setTimeout(fn, ms, ...rest);
};
global.setInterval = function (fn: any, ms?: any) {
  calls.push("setInterval " + ms);
  return orig.setInterval(fn, ms);
};
Object.defineProperty(globalThis, "clearTimeout", {
  value: function (h: any) {
    calls.push("clearTimeout");
    return orig.clearTimeout(h);
  },
  writable: true,
  configurable: true,
});
Object.assign(globalThis, {
  clearInterval(h: any) {
    calls.push("clearInterval");
    return orig.clearInterval(h);
  },
});
g.setImmediate = function (fn: any) {
  calls.push("setImmediate");
  return orig.setImmediate(fn);
};
g.queueMicrotask = function (fn: any) {
  calls.push("queueMicrotask");
  return orig.queueMicrotask(fn);
};

function later() {
  // Inside a function too: the call reads the binding at call time.
  setTimeout(() => fired.push("timeout with arg 7"), 2);
}

setTimeout(() => fired.push("timeout"), 1);
later();
const iv = setInterval(() => {
  fired.push("interval");
  clearInterval(iv);
}, 1);
const never = setTimeout(() => fired.push("never"), 50);
clearTimeout(never);
setImmediate(() => fired.push("immediate"));
const im = setImmediate(() => fired.push("never immediate"));
clearImmediate(im);
queueMicrotask(() => fired.push("microtask"));
console.log(typeof setTimeout, setTimeout === orig.setTimeout);
console.log(calls.join(", "));

// A non-function replacement is observable through typeof and the call.
setTimeout(() => {
  g.setTimeout = 42;
  console.log(typeof setTimeout);
  try {
    setTimeout(() => {}, 1);
  } catch (e) {
    console.log((e as Error).constructor.name);
  }
  g.setTimeout = orig.setTimeout;
  console.log(fired.sort().join(", "));
}, 20);
