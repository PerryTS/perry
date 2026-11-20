import { PassThrough } from "node:stream";
// setMaxListeners(0) / Infinity disables the warning entirely.
const p = new PassThrough();
p.setMaxListeners(Infinity);
console.log("infinity:", p.getMaxListeners());
p.setMaxListeners(0);
console.log("zero:", p.getMaxListeners());
