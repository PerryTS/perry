import { PassThrough } from "node:stream";
// rawListeners('event') returns the listener array including wrappers
// (e.g. once-wrappers); listeners() returns unwrapped listeners.
const p = new PassThrough();
const fn = () => {};
p.once("data", fn);
console.log("raw count:", p.rawListeners("data").length);
console.log("listeners count:", p.listeners("data").length);
