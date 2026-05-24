import { Readable } from "node:stream";
// rawListeners() returns the listener array INCLUDING the once-wrapper
// for once() listeners (.listener points to the original fn).
const r = new Readable({ read() {} });
const fn = () => {};
r.once("end", fn);
const raw = r.rawListeners("end");
console.log("raw length:", raw.length);
console.log("raw[0] is wrapper:", (raw[0] as any).listener === fn);
