import { Readable } from "node:stream";
// After 'end', readable flag becomes false.
const r = Readable.from(["a"]);
console.log("initial readable:", r.readable);
r.on("data", () => {});
r.on("end", () => {
  console.log("after end readable:", r.readable);
});
