import { Readable } from "node:stream";
// readable.readable is a boolean indicating whether it is safe to call
// readable.read() / not destroyed / not yet ended.
const r = new Readable({ read() {} });
console.log("readable initial:", r.readable);
r.push("x");
r.push(null);
r.on("end", () => {
  console.log("readable after end:", r.readable);
});
r.on("data", () => {});
