import { Readable } from "node:stream";
// removeListener of a function never registered — safely no-op.
const r = new Readable({ read() {} });
const fn = () => {};
const result = r.removeListener("never-registered", fn);
console.log("returns self:", result === r);
console.log("eventNames empty:", r.eventNames().length === 0);
