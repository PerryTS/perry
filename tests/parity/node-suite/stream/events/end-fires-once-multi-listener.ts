import { Readable } from "node:stream";
// 'end' fires exactly once even with multiple listeners.
let counts = [0, 0, 0];
const r = Readable.from(["x"]);
r.on("data", () => {});
r.on("end", () => counts[0]++);
r.on("end", () => counts[1]++);
r.on("end", () => counts[2]++);
setImmediate(() => {
  setImmediate(() => {
    console.log("counts:", counts.join(","));
    console.log("all 1:", counts.every((n) => n === 1));
  });
});
