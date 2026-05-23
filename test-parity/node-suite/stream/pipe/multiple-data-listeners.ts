import { Readable } from "node:stream";
// Multiple 'data' listeners share the same flow — each receives every chunk.
const r = Readable.from(["x"]);
let a = 0, b = 0;
r.on("data", () => a++);
r.on("data", () => b++);
r.on("end", () => console.log("a:", a, "b:", b));
