import { PassThrough } from "node:stream";
// on() / removeListener() return the emitter for chaining.
const p = new PassThrough();
const fn = () => {};
console.log("on returns self:", p.on("data", fn) === p);
console.log("removeListener returns self:", p.removeListener("data", fn) === p);
