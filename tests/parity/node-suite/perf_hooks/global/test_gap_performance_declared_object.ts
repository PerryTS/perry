// performance is one object (global and perf_hooks); its members run as node's:
// mark/measure/clearMarks coerce their name arguments (ToPrimitive runs the
// program toString), timeOrigin is an inherited accessor, detail is cloned.
import { performance as ph } from "perf_hooks";
const perf = typeof performance === "object" ? performance : ph;
console.log(perf === ph, Object.getOwnPropertyNames(perf).length);
let calls = 0;
const o = { toString() { calls++; return "m1"; }, x: 1 };
console.log(typeof perf.timeOrigin, "timeOrigin" in perf, Object.prototype.hasOwnProperty.call(perf, "timeOrigin"));
const m = perf.mark(o as any);
console.log(m.name, m.entryType, calls);
const detail = { k: [1, 2] };
const m2 = perf.mark("m2", { detail });
console.log(m2.detail.k.join(","), m2.detail === detail);
const ms = perf.measure("n", "m1");
console.log(ms.name, ms.entryType, typeof ms.duration);
perf.clearMarks(o as any);
console.log(calls, perf.getEntriesByName("m1").length, perf.getEntriesByName("m2").length);
perf.clearMarks();
perf.clearMeasures("n");
console.log(perf.getEntries().length);
const t = perf.now();
console.log(typeof t, t >= 0, o.x);
