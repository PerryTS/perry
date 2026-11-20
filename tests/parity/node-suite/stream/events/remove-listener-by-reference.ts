import { Readable } from "node:stream";
// removeListener requires the exact same function reference.
const r = new Readable({ read() {} });
const fn = () => {};
r.on("custom", fn);
r.on("custom", () => {});
console.log("before:", r.listenerCount("custom"));
r.removeListener("custom", fn);
console.log("after:", r.listenerCount("custom"));
console.log("decremented by 1:", r.listenerCount("custom") === 1);
