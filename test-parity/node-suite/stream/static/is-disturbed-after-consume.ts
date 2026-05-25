import { Readable, isDisturbed } from "node:stream";
// isDisturbed(stream) — false before any read, true after consumption.
const r = Readable.from(["x"]);
console.log("before:", isDisturbed(r));
r.on("data", () => {});
r.on("end", () => {
  console.log("after:", isDisturbed(r));
});
