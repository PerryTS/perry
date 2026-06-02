import { performance } from "node:perf_hooks";
// measure(name, { start, duration, detail }) attaches detail and honors the
// numeric start/duration directly.
const m = performance.measure("md", { start: 0, duration: 3, detail: { k: 1 } });
console.log("name:", m.name);
console.log("startTime:", m.startTime);
console.log("duration:", m.duration);
console.log("detail:", m.detail);
