import { Readable } from "node:stream";
// EE: removing a listener that's not registered is a no-op (no error).
const r = new Readable({ read() {} });
const fn = () => {};
r.removeListener("nothing", fn); // not registered
r.removeListener("data", () => {}); // anonymous, not the same reference
console.log("still no listeners:", r.eventNames().length);
console.log("no error thrown");
