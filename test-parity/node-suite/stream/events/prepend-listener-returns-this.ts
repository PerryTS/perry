import { Readable } from "node:stream";
// prependListener returns the emitter (chainable).
const r = new Readable({ read() {} });
const returned = r.prependListener("custom", () => {});
console.log("returns self:", returned === r);
