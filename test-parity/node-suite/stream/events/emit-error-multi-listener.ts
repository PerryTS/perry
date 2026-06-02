import { Readable } from "node:stream";
// emit('error', e) with multiple listeners — all fire.
const r = new Readable({ read() {} });
let counts = [0, 0, 0];
r.on("error", () => counts[0]++);
r.on("error", () => counts[1]++);
r.on("error", () => counts[2]++);
r.emit("error", new Error("multi"));
console.log("counts:", counts.join(","));
console.log("all 1:", counts.every((c) => c === 1));
