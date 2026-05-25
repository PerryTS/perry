import { Readable, isReadable } from "node:stream";
// isReadable(stream) — true initially, false after end.
const r = new Readable({ read() {} });
console.log("initial:", isReadable(r));
r.push("x");
r.push(null);
r.on("data", () => {});
r.on("end", () => {
  setImmediate(() => console.log("after end:", isReadable(r)));
});
