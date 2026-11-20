import { PassThrough } from "node:stream";
// addListener is an alias of on(); off is an alias of removeListener.
const p = new PassThrough();
const fn = () => {};
p.addListener("data", fn);
console.log("count after addListener:", p.listenerCount("data"));
p.off("data", fn);
console.log("count after off:", p.listenerCount("data"));
