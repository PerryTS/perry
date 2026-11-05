import { PassThrough } from "node:stream";
// Streams inherit EE getMaxListeners(); default is 10.
const p = new PassThrough();
console.log("default:", p.getMaxListeners());
p.setMaxListeners(25);
console.log("after set:", p.getMaxListeners());
