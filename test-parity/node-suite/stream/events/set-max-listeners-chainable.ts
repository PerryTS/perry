import { Readable } from "node:stream";
// setMaxListeners(N) — should return the emitter (chainable).
const r = new Readable({ read() {} });
const returned = r.setMaxListeners(20);
console.log("returns self:", returned === r);
console.log("max set:", r.getMaxListeners() === 20);
