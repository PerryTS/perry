import { performance } from "node:perf_hooks";
// measure(name, startMark, endMark) computes startTime/duration from two marks.
performance.mark("a", { startTime: 0 });
performance.mark("b", { startTime: 10 });
const m = performance.measure("foo", "a", "b");
console.log("name:", m.name);
console.log("entryType:", m.entryType);
console.log("startTime:", m.startTime);
console.log("duration:", m.duration);
