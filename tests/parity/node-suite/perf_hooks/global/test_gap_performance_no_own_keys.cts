// `performance` is an ordinary object with no own keys whose chain is
// Performance.prototype -> EventTarget.prototype -> Object.prototype; the
// perf_hooks module namespace keeps its own exports and is not a Performance.
const perfHooks = require("perf_hooks");

// String keys only: node also keeps internal symbol-keyed listener state.
function own(o: any): string {
  return Object.getOwnPropertyNames(o).sort().join(",");
}

console.log("own:", JSON.stringify(own(performance)));
console.log("keys:", Object.keys(performance).length);
const forIn: string[] = [];
for (const k in performance) forIn.push(k);
console.log("for-in has own first:", forIn[0] === "Performance", forIn.includes("now"));
console.log("hasOwn now:", Object.prototype.hasOwnProperty.call(performance, "now"));
console.log("hasOwn timerify:", Object.prototype.hasOwnProperty.call(performance, "timerify"));

const p0 = Object.getPrototypeOf(performance);
const p1 = Object.getPrototypeOf(p0);
console.log("proto0:", p0 === Performance.prototype);
console.log("proto1:", p1 === EventTarget.prototype);
console.log("proto2:", Object.getPrototypeOf(p1) === Object.prototype);
console.log("ctor parent:", Object.getPrototypeOf(Performance) === EventTarget);
console.log("instanceof:", performance instanceof Performance, performance instanceof EventTarget);
console.log("module not a Performance:", perfHooks instanceof Performance);

console.log("identity:", perfHooks.performance === performance);
console.log("module keeps members:", typeof perfHooks.timerify, typeof perfHooks.monitorEventLoopDelay,
  typeof perfHooks.PerformanceObserver, typeof perfHooks.constants);

const p: any = performance;
console.log("value read:", typeof p.now(), typeof p.timeOrigin, typeof p.mark, typeof p.timerify);
p.mark("a");
console.log("mark via value:", p.getEntriesByName("a").length);
const now = performance.now;
try {
  now.call({});
  console.log("foreign receiver: no throw");
} catch (e: any) {
  console.log("foreign receiver:", e.name);
}

let fired = 0;
performance.addEventListener("x", () => { fired++; });
performance.dispatchEvent(new Event("x"));
console.log("event target:", fired);

p.extra = 1;
console.log("own after store:", own(performance));
